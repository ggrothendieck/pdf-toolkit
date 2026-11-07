use anyhow::{Context, Result};
use pdf_oxide::editor::{DocumentEditor, EditableDocument};
use std::path::PathBuf;

pub fn run_pad(input: PathBuf, output: PathBuf) -> Result<()> {
    println!("Appending blank page to {:?} -> {:?}...", input, output);

    // 1. Open the existing input document
    let mut editor = DocumentEditor::open(&input)
        .with_context(|| format!("Failed to open input PDF {:?}", input))?;

    // 2. Generate a minimal valid single-page blank PDF raw byte structure
    // This creates a standard Letter-sized blank page PDF specification directly.
    let blank_pdf_bytes = generate_minimal_blank_pdf();

    let temp_path = std::env::temp_dir().join(format!("pdf_toolkit_blank_{}.pdf", uuid_like_suffix()));
    std::fs::write(&temp_path, &blank_pdf_bytes)
        .with_context(|| format!("Failed to write temporary blank page PDF to {:?}", temp_path))?;

    // 3. Merge the blank page onto the end of the editor session
    let merge_result = editor.merge_from(&temp_path);

    // Clean up temporary file regardless of merge result success/failure
    let _ = std::fs::remove_file(&temp_path);

    merge_result.with_context(|| "Failed to append blank page via merge")?;

    // 4. Save the final padded document
    editor.save(&output)
        .with_context(|| format!("Failed to save padded PDF to {:?}", output))?;

    println!("Successfully created {:?}", output);
    Ok(())
}

/// Generates a minimal, valid single-page blank PDF file in bytes
fn generate_minimal_blank_pdf() -> Vec<u8> {
    b"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /Resources << /Font << >> >> /MediaBox [0 0 612 792] /Contents 4 0 R >>
endobj
4 0 obj
<< /Length 0 >>
stream
endstream
endobj
xref
0 5
0000000000 65535 f 
0000000009 00000 n 
0000000058 00000 n 
0000000115 00000 n 
000000227 00000 n 
trailer
<< /Size 5 /Root 1 0 R >>
startxref
276
%%EOF"
    .to_vec()
}

/// Simple helper to generate a unique suffix for the temp file
fn uuid_like_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
