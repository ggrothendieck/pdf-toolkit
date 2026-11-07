use std::fs;
use std::path::PathBuf;
use anyhow::Result;

pub fn run_html(input_path: PathBuf, output_path: PathBuf) -> Result<()> {
    let html = fs::read_to_string(&input_path)?;

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let pdf = ironpress::html_to_pdf(&html)?;
    fs::write(output_path, pdf)?;

    Ok(())
}
