
#[path = "names.rs"]
mod names;

#[path = "page_writer.rs"]
mod page_writer;

use anyhow::{bail, Context, Result};
use lopdf::{dictionary, Document, Object};
use std::path::{Path, PathBuf};

use self::names::file_name;
use self::page_writer::{create_attachment_page, Attachment};

const ATTACHMENTS_PER_PAGE: usize = 50;

pub(super) fn attach_page_level(
    input: &Path,
    files: &[PathBuf],
    output: &Path,
) -> Result<()> {
    if files.is_empty() {
        bail!("No attachment files supplied.");
    }

    let mut attachments = Vec::with_capacity(files.len());

    for path in files {
        if !path.is_file() {
            bail!("Attachment file not found: {:?}", path);
        }

        attachments.push(Attachment {
            name: file_name(path)?.to_owned(),
            bytes: std::fs::read(path)
                .with_context(|| {
                    format!("Failed to read attachment {:?}", path)
                })?,
        });
    }

    let mut doc = Document::load(input)
        .with_context(|| format!("Failed to open target PDF {:?}", input))?;

    let original_page_count = doc.get_pages().len();

    if original_page_count == 0 {
        bail!("The input PDF has no pages.");
    }

    let page_count = attachments.len().div_ceil(ATTACHMENTS_PER_PAGE);

    let catalog_id = doc
        .trailer
        .get(b"Root")?
        .as_reference()?;

    let pages_id = doc
        .get_dictionary(catalog_id)?
        .get(b"Pages")?
        .as_reference()?;

    let mut kids = doc
        .get_dictionary(pages_id)?
        .get(b"Kids")?
        .as_array()?
        .clone();

    // Standard Type 1 Helvetica font for visible labels.
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    for page_index in 0..page_count {
        let start = page_index * ATTACHMENTS_PER_PAGE;
        let end = (start + ATTACHMENTS_PER_PAGE).min(attachments.len());

        let page_id = create_attachment_page(
            &mut doc,
            pages_id,
            font_id,
            &attachments[start..end],
        )?;

        kids.push(Object::Reference(page_id));

        println!(
            "Created attachment page {} with {} attachment(s).",
            original_page_count + page_index + 1,
            end - start
        );
    }

    doc.get_dictionary_mut(pages_id)?
        .set("Kids", Object::Array(kids));

    doc.get_dictionary_mut(pages_id)?.set(
        "Count",
        Object::Integer((original_page_count + page_count) as i64),
    );

    doc.save(output)
        .with_context(|| format!("Failed to save PDF to {:?}", output))?;

    println!(
        "Successfully created {:?} with {} attachment page(s).",
        output, page_count
    );

    Ok(())
}

