use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use lopdf::{Dictionary, Document, Object, Stream, StringFormat};
use pdf_oxide::editor::{DocumentEditor, EditableDocument};

use crate::AttachmentLevel;

const COLUMNS: usize = 2;
const ROWS_PER_PAGE: usize = 15;
const ATTACHMENTS_PER_PAGE: usize = COLUMNS * ROWS_PER_PAGE;

// ==========================================
// SECURITY & SANITIZATION
// ==========================================

/// Validates an attachment against known dangerous types and signature mismatches.
fn validate_attachment(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("Attachment file not found: {:?}", path);
    }

    // 1. Fast-fail check on extension blocklists
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let lower_ext = ext.to_ascii_lowercase();
        match lower_ext.as_str() {
            "exe" | "bat" | "cmd" | "sh" | "msi" | "vbs" | "ps1" | "scr" | "pif" | "com" => {
                bail!("Security violation: Extension '.{}' is explicitly blocked.", ext);
            }
            _ => {}
        }
    }

    // 2. Inspect magic numbers using `infer`
    match infer::get_from_path(path) {
        Ok(Some(kind)) => {
            let mime = kind.mime_type();
            // Block executables, scripts, and compiled binaries regardless of extension
            if mime.starts_with("application/x-msdownload")
                || mime.starts_with("application/x-executable")
                || mime.starts_with("application/vnd.microsoft.portable-executable")
                || mime == "application/javascript"
                || mime == "application/x-sh"
            {
                bail!(
                    "Security violation: File signature indicates a blocked executable or script type (MIME: {}).",
                    mime
                );
            }
        }
        Ok(None) => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let allowed_text_exts = ["txt", "csv", "md", "json", "xml", "log", "ini", "yml", "yaml", "html", "htm"];
            
            if !allowed_text_exts.contains(&ext.as_str()) {
                let mut file = fs::File::open(path)?;
                let mut buffer = [0u8; 1024];
                let n = file.read(&mut buffer)?;
                if n > 0 && std::str::from_utf8(&buffer[..n]).is_err() {
                    bail!("Security rejected: Unrecognized file type with non-text binary content.");
                }
            }
        }
        Err(e) => {
            bail!("Failed to inspect file magic bytes for {:?}: {}", path, e);
        }
    }

    Ok(())
}

// ==========================================
// SHARED UTILITIES & FACTORED-OUT COMMON CODE
// ==========================================

fn resolve_attachments_for_input(
    input_path: &Path,
    explicit_attachments: &Option<Vec<PathBuf>>,
    sanitize: bool,
) -> Result<Vec<PathBuf>> {
    let atts = if let Some(explicit) = explicit_attachments {
        explicit.clone()
    } else {
        let stem_str = match input_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        let companion_dir = input_path.with_file_name(stem_str);
        if !companion_dir.is_dir() {
            return Ok(Vec::new());
        }

        crate::resolve_and_expand_inputs(vec![companion_dir], None)?
    };

    if sanitize {
        for path in &atts {
            validate_attachment(path)?;
        }
    }

    Ok(atts)
}

fn execute_with_output_handling<F>(
    inputs: Vec<PathBuf>,
    outdir: Option<PathBuf>,
    default_ext: &str,
    mut action: F,
) -> Result<()>
where
    F: FnMut(&Path, &PathBuf) -> Result<()>,
{
    let target_dir = outdir.unwrap_or_else(|| PathBuf::from("."));
    let is_in_place = target_dir == Path::new(".");

    if !target_dir.exists() && target_dir != Path::new(".") {
        fs::create_dir_all(&target_dir)?;
    }

    for input_path in inputs {
        let final_output = if is_in_place {
            input_path.clone()
        } else {
            crate::resolve_target_file(&input_path, &target_dir, default_ext)?
        };

        let actual_output = if final_output == input_path {
            let mut tmp_path = input_path.clone();
            let mut fname = input_path.file_name().unwrap().to_os_string();
            fname.push(".tmp_attach");
            tmp_path.set_file_name(fname);
            tmp_path
        } else {
            final_output.clone()
        };

        let result = action(&input_path, &actual_output);

        if let Err(e) = result {
            if actual_output != final_output && actual_output.exists() {
                let _ = fs::remove_file(&actual_output);
            }
            return Err(e);
        }

        if final_output == input_path {
            if let Err(e) = fs::rename(&actual_output, &input_path) {
                let _ = fs::remove_file(&actual_output);
                return Err(e.into());
            }
        } else if actual_output != final_output && actual_output.exists() {
            if let Err(e) = fs::rename(&actual_output, &final_output) {
                let _ = fs::remove_file(&actual_output);
                return Err(e.into());
            }
        }
    }

    Ok(())
}

// ==========================================
// ATTACH TO PDF COMMAND
// ==========================================

pub fn run_attach_to_pdf(
    expanded_inputs: Vec<PathBuf>,
    attachments: Option<Vec<PathBuf>>,
    level: AttachmentLevel,
    sanitize: bool,
    outdir: Option<PathBuf>,
) -> Result<()> {
    let explicit_attachments = match attachments {
        Some(att_args) => Some(crate::resolve_and_expand_inputs(att_args, None)?),
        None => None,
    };

    execute_with_output_handling(expanded_inputs, outdir, "pdf", |input_path, actual_output| {
        let attachments_to_embed = resolve_attachments_for_input(input_path, &explicit_attachments, sanitize)?;
        if attachments_to_embed.is_empty() {
            if input_path != actual_output {
                fs::copy(input_path, actual_output)?;
            }
            return Ok(());
        }

        match level {
            AttachmentLevel::Document => {
                attach_document(input_path, &attachments_to_embed, actual_output)
            }
            AttachmentLevel::Page => {
                attach_page(input_path, &attachments_to_embed, actual_output)
            }
        }
    })
}

fn attach_document(
    input: &Path,
    attachments: &[PathBuf],
    output: &Path,
) -> Result<()> {
    let mut editor = DocumentEditor::open(input)
        .with_context(|| format!("Failed to open PDF {:?}", input))?;

    for path in attachments {
        let bytes = fs::read(path)?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin");

        editor.embed_file(filename, bytes)
            .with_context(|| format!("Failed to embed {:?}", path))?;
    }

    editor.save(output)
        .with_context(|| format!("Failed to save PDF {:?}", output))?;

    Ok(())
}

fn attach_page(
    input: &Path,
    attachments: &[PathBuf],
    output: &Path,
) -> Result<()> {
    let mut doc = Document::load(input)
        .with_context(|| format!("Failed to open PDF {:?}", input))?;

    let page_count = attachments.len().div_ceil(ATTACHMENTS_PER_PAGE);

    let mut pages: Vec<((u32, u16), Vec<u8>)> = Vec::new();

    for (index, path) in attachments.iter().enumerate() {
        let page_number = index / ATTACHMENTS_PER_PAGE;
        let position = index % ATTACHMENTS_PER_PAGE;

        if position == 0 {
            let page_id = append_attachment_page(
                &mut doc,
                page_number + 1,
                page_count,
            )?;

            pages.push((page_id, Vec::new()));
        }

        let bytes = fs::read(path)?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin");

        let mut params = Dictionary::new();
        params.set("Size", Object::Integer(bytes.len() as i64));

        let mut stream_dict = Dictionary::new();
        stream_dict.set("Type", Object::Name(b"EmbeddedFile".to_vec()));
        stream_dict.set("Params", Object::Dictionary(params));

        let stream_id = doc.add_object(Object::Stream(
            Stream::new(stream_dict, bytes),
        ));

        let mut ef = Dictionary::new();
        ef.set("F", Object::Reference(stream_id));
        ef.set("UF", Object::Reference(stream_id));

        let mut filespec = Dictionary::new();
        filespec.set("Type", Object::Name(b"Filespec".to_vec()));
        filespec.set(
            "F",
            Object::String(filename.as_bytes().to_vec(), StringFormat::Literal),
        );
        filespec.set(
            "UF",
            Object::String(filename.as_bytes().to_vec(), StringFormat::Literal),
        );
        filespec.set("EF", Object::Dictionary(ef));

        let filespec_id = doc.add_object(Object::Dictionary(filespec));

        let column = position / ROWS_PER_PAGE;
        let row = position % ROWS_PER_PAGE;

        let pin_x = if column == 0 { 52.0 } else { 315.0 };
        let pin_y = 692.0 - row as f32 * 42.0;

        let page_id = pages[page_number].0;

        let mut annotation = Dictionary::new();
        annotation.set("Type", Object::Name(b"Annot".to_vec()));
        annotation.set("Subtype", Object::Name(b"FileAttachment".to_vec()));
        annotation.set("FS", Object::Reference(filespec_id));
        annotation.set("P", Object::Reference(page_id));
        annotation.set(
            "Rect",
            Object::Array(vec![
                Object::Real(pin_x),
                Object::Real(pin_y),
                Object::Real(pin_x + 20.0),
                Object::Real(pin_y + 20.0),
            ]),
        );
        annotation.set("Name", Object::Name(b"PushPin".to_vec()));
        annotation.set(
            "Contents",
            Object::String(filename.as_bytes().to_vec(), StringFormat::Literal),
        );
        annotation.set("F", Object::Integer(4));

        let annotation_id = doc.add_object(Object::Dictionary(annotation));

        add_page_annotation(&mut doc, page_id, annotation_id)?;

        let label = short_filename(filename, 38);
        let label_x = pin_x + 28.0;
        let label_y = pin_y + 5.0;

        let labels = &mut pages[page_number].1;

        labels.extend_from_slice(
            format!(
                "BT\n/F1 10 Tf\n{label_x:.2} {label_y:.2} Td\n"
            )
            .as_bytes(),
        );
        labels.extend_from_slice(pdf_literal(&label).as_bytes());
        labels.extend_from_slice(b" Tj\nET\n");

        let line_y = pin_y - 12.0;

        labels.extend_from_slice(
            format!(
                "0.7 w\n{:.2} {:.2} m\n{:.2} {:.2} l\nS\n",
                pin_x,
                line_y,
                pin_x + 230.0,
                line_y,
            )
            .as_bytes(),
        );
    }

    for (page_id, labels) in pages {
        if labels.is_empty() {
            continue;
        }

        let labels_id = doc.add_object(Object::Stream(
            Stream::new(Dictionary::new(), labels),
        ));

        let page = doc.get_object_mut(page_id)?.as_dict_mut()?;

        let heading_id = page
            .get(b"Contents")?
            .as_reference()?;

        page.set(
            "Contents",
            Object::Array(vec![
                Object::Reference(heading_id),
                Object::Reference(labels_id),
            ]),
        );
    }

    doc.save(output)
        .with_context(|| format!("Failed to save PDF {:?}", output))?;

    Ok(())
}

fn short_filename(filename: &str, max_chars: usize) -> String {
    let chars: Vec<char> = filename.chars().collect();

    if chars.len() <= max_chars {
        return filename.to_string();
    }

    let mut result: String = chars
        .into_iter()
        .take(max_chars.saturating_sub(1))
        .collect();

    result.push('~');
    result
}

fn pdf_literal(text: &str) -> String {
    let mut result = String::from("(");

    for ch in text.chars() {
        match ch {
            '(' | ')' | '\\' => {
                result.push('\\');
                result.push(ch);
            }
            '\n' | '\r' | '\t' => result.push(' '),
            ch if ch.is_ascii() && !ch.is_control() => result.push(ch),
            _ => result.push('?'),
        }
    }

    result.push(')');
    result
}

fn append_attachment_page(
    doc: &mut Document,
    page_number: usize,
    page_count: usize,
) -> Result<(u32, u16)> {
    let pages_id = doc.catalog()?
        .get(b"Pages")?
        .as_reference()?;

    let heading = if page_count > 1 {
        format!("Attachments ({page_number} of {page_count})")
    } else {
        "Attachments".to_string()
    };

    let content = format!(
        "BT\n/F1 18 Tf\n52 744 Td\n{} Tj\nET\n",
        pdf_literal(&heading),
    );

    let content_id = doc.add_object(Object::Stream(
        Stream::new(Dictionary::new(), content.into_bytes()),
    ));

    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));

    let font_id = doc.add_object(Object::Dictionary(font));

    let mut fonts = Dictionary::new();
    fonts.set("F1", Object::Reference(font_id));

    let mut resources = Dictionary::new();
    resources.set("Font", Object::Dictionary(fonts));

    let mut page = Dictionary::new();
    page.set("Type", Object::Name(b"Page".to_vec()));
    page.set("Parent", Object::Reference(pages_id));
    page.set(
        "MediaBox",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792),
        ]),
    );
    page.set("Resources", Object::Dictionary(resources));
    page.set("Contents", Object::Reference(content_id));

    let page_id = doc.add_object(Object::Dictionary(page));

    let pages = doc.get_object_mut(pages_id)?.as_dict_mut()?;

    let kids = pages.get_mut(b"Kids")?.as_array_mut()?;
    kids.push(Object::Reference(page_id));

    let count = pages.get(b"Count")?.as_i64()?;
    pages.set("Count", Object::Integer(count + 1));

    Ok(page_id)
}

fn add_page_annotation(
    doc: &mut Document,
    page_id: (u32, u16),
    annotation_id: (u32, u16),
) -> Result<()> {
    let existing = doc
        .get_object(page_id)?
        .as_dict()?
        .get(b"Annots")
        .ok()
        .cloned();

    match existing {
        Some(Object::Array(mut annotations)) => {
            annotations.push(Object::Reference(annotation_id));

            doc.get_object_mut(page_id)?
                .as_dict_mut()?
                .set("Annots", Object::Array(annotations));
        }

        Some(Object::Reference(annots_id)) => {
            doc.get_object_mut(annots_id)?
                .as_array_mut()?
                .push(Object::Reference(annotation_id));
        }

        Some(_) => {
            bail!("Unexpected PDF page Annots entry");
        }

        None => {
            doc.get_object_mut(page_id)?
                .as_dict_mut()?
                .set(
                    "Annots",
                    Object::Array(vec![Object::Reference(annotation_id)]),
                );
        }
    }

    Ok(())
}

// ==========================================
// ATTACH TO HTML COMMAND
// ==========================================

pub fn run_attach_to_html(
    expanded_inputs: Vec<PathBuf>,
    attachments: Option<Vec<PathBuf>>,
    sanitize: bool,
    outdir: Option<PathBuf>,
) -> Result<()> {
    let explicit_attachments = match attachments {
        Some(att_args) => Some(crate::resolve_and_expand_inputs(att_args, None)?),
        None => None,
    };

    execute_with_output_handling(expanded_inputs, outdir, "html", |input_path, actual_output| {
        let attachments_to_embed = resolve_attachments_for_input(input_path, &explicit_attachments, sanitize)?;
        
        let html_content = fs::read_to_string(input_path)
            .with_context(|| format!("Failed to read HTML file {:?}", input_path))?;

        let mut links_html = String::from("\n<div class=\"pdf-toolkit-attachments\" style=\"margin-top: 2em; padding-top: 1em; border-top: 1px solid #ccc;\">\n  <h3>Attachments</h3>\n  <ul>\n");

        for att_path in &attachments_to_embed {
            let bytes = fs::read(att_path)
                .with_context(|| format!("Failed to read attachment file {:?}", att_path))?;
            let filename = att_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("attachment.bin");

            let mime_type = match att_path.extension().and_then(|e| e.to_str()) {
                Some(ext) if ext.eq_ignore_ascii_case("pdf") => "application/pdf",
                Some(ext) if ext.eq_ignore_ascii_case("png") => "image/png",
                Some(ext) if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") => "image/jpeg",
                Some(ext) if ext.eq_ignore_ascii_case("txt") => "text/plain",
                _ => "application/octet-stream",
            };

            let encoded = BASE64_STANDARD.encode(&bytes);
            let data_uri = format!("data:{};base64,{}", mime_type, encoded);

            let link_entry = format!(
                "    <li><a href=\"{}\" download=\"{}\">{}</a> ({})</li>\n",
                data_uri,
                filename,
                filename,
                mime_type
            );
            links_html.push_str(&link_entry);
        }
        links_html.push_str("  </ul>\n</div>\n");

        let final_html = if let Some(idx) = html_content.rfind("</body>") {
            let (head, tail) = html_content.split_at(idx);
            format!("{}{}{}", head, links_html, tail)
        } else {
            format!("{}{}", html_content, links_html)
        };

        fs::write(actual_output, final_html)
            .with_context(|| format!("Failed to write HTML file {:?}", actual_output))?;

        Ok(())
    })
}
