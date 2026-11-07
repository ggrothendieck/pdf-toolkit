mod merge;
mod pad;
mod attach;
mod html;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "pdf_toolkit",
    author = "Developer",
    version = "2.0",
    about = "Pure Rust PDF utility for HTML conversion, merging, padding, and attachments"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum AttachmentLevel {
    Document,
    Page,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert multiple HTML file(s), directories, or @file lists to PDF
    #[command(name = "html2pdf")]
    Html2Pdf {
        /// Input HTML file(s), directories, or @file lists (space separated)
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,

        /// Target output directory for generated PDFs (defaults to current directory '.')
        #[arg(long)]
        outdir: Option<PathBuf>,
    },

    /// Merge multiple PDF documents or directories into a single file sequentially
    Merge {
        /// Input PDF file(s), directories, or @file lists (space separated)
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,

        /// Output combined PDF file path
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Append a blank page to the end of multiple PDF file(s), directories, or @file lists
    PadBlank {
        /// Target PDF file(s), directories, or @file lists (space separated)
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,

        /// Target output directory (defaults to '.' for in-place modification)
        #[arg(long)]
        outdir: Option<PathBuf>,
    },

    /// Embed one or more file attachments into multiple PDF file(s) (document or page level)
    #[command(name = "attach-to-pdf")]
    AttachToPdf {
        /// Target PDF file(s), directories, or @file lists (space separated)
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,

        /// Files, directories, or @file lists to embed as attachments (defaults to a directory matching the input file stem)
        #[arg(long, num_args = 1..)]
        attachments: Option<Vec<PathBuf>>,

        /// Attachment level: document (catalog) or page (annotation on final page)
        #[arg(short, long, value_enum, default_value_t = AttachmentLevel::Page)]
        level: AttachmentLevel,

        /// Disable security type verification and allow potentially unsafe or blocked attachments
        #[arg(long)]
        no_sanitize: bool,

        /// Target output directory (defaults to '.' for in-place modification)
        #[arg(long)]
        outdir: Option<PathBuf>,
    },

    /// Embed one or more file attachments as downloadable data URIs into multiple HTML file(s)
    #[command(name = "attach-to-html")]
    AttachToHtml {
        /// Target HTML file(s), directories, or @file lists (space separated)
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,

        /// Files, directories, or @file lists to embed as attachments (defaults to a directory matching the input file stem)
        #[arg(long, num_args = 1..)]
        attachments: Option<Vec<PathBuf>>,

        /// Disable security type verification and allow potentially unsafe or blocked attachments
        #[arg(long)]
        no_sanitize: bool,

        /// Target output directory (defaults to '.' for in-place modification)
        #[arg(long)]
        outdir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Html2Pdf { inputs, outdir } => {
            let expanded_inputs = resolve_and_expand_inputs(inputs, Some(&["html", "htm"]))?;

            if expanded_inputs.is_empty() {
                anyhow::bail!("No valid HTML input files found.");
            }

            let target_dir = outdir.unwrap_or_else(|| PathBuf::from("."));

            for html_path in expanded_inputs {
                let pdf_output = resolve_target_file(&html_path, &target_dir, "pdf")?;
                html::run_html(html_path, pdf_output)?;
            }
        }

        Commands::Merge { inputs, output } => {
            let mut expanded_inputs = resolve_and_expand_inputs(inputs, Some(&["pdf"]))?;

            expanded_inputs.sort();

            let canonical_output = output.canonicalize().unwrap_or_else(|_| output.clone());
            expanded_inputs.retain(|path| {
                let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
                canonical_path != canonical_output
            });

            if expanded_inputs.is_empty() {
                anyhow::bail!("No valid PDF input files found for merging.");
            }

            merge::run_merge(expanded_inputs, output)?;
        }

        Commands::PadBlank { inputs, outdir } => {
            let expanded_inputs = resolve_and_expand_inputs(inputs, Some(&["pdf"]))?;

            if expanded_inputs.is_empty() {
                anyhow::bail!("No valid PDF input files found for padding.");
            }

            let target_dir = outdir.unwrap_or_else(|| PathBuf::from("."));
            let is_in_place = target_dir == Path::new(".");

            for input_path in expanded_inputs {
                let final_output = if is_in_place {
                    input_path.clone()
                } else {
                    resolve_target_file(&input_path, &target_dir, "pdf")?
                };

                let actual_output = if final_output == input_path {
                    let mut tmp_path = input_path.clone();
                    let mut fname = input_path.file_name().unwrap().to_os_string();
                    fname.push(".tmp_pad");
                    tmp_path.set_file_name(fname);
                    tmp_path
                } else {
                    final_output.clone()
                };

                let result = pad::run_pad(input_path.clone(), actual_output.clone());

                if let Err(e) = result {
                    if actual_output != final_output && actual_output.exists() {
                        let _ = std::fs::remove_file(&actual_output);
                    }
                    return Err(e);
                }

                if final_output == input_path {
                    if let Err(e) = std::fs::rename(&actual_output, &input_path) {
                        let _ = std::fs::remove_file(&actual_output);
                        return Err(e.into());
                    }
                } else if actual_output != final_output && actual_output.exists() {
                    if let Err(e) = std::fs::rename(&actual_output, &final_output) {
                        let _ = std::fs::remove_file(&actual_output);
                        return Err(e.into());
                    }
                }
            }
        }

        Commands::AttachToPdf {
            inputs,
            attachments,
            level,
            no_sanitize,
            outdir,
        } => {
            let expanded_inputs = resolve_and_expand_inputs(inputs, Some(&["pdf"]))?;
            if expanded_inputs.is_empty() {
                anyhow::bail!("No valid PDF input files found for attaching.");
            }
            let sanitize = !no_sanitize;
            attach::run_attach_to_pdf(expanded_inputs, attachments, level, sanitize, outdir)?;
        }

        Commands::AttachToHtml {
            inputs,
            attachments,
            no_sanitize,
            outdir,
        } => {
            let expanded_inputs = resolve_and_expand_inputs(inputs, Some(&["html", "htm"]))?;
            if expanded_inputs.is_empty() {
                anyhow::bail!("No valid HTML input files found for attaching.");
            }
            let sanitize = !no_sanitize;
            attach::run_attach_to_html(expanded_inputs, attachments, sanitize, outdir)?;
        }
    }

    Ok(())
}

fn resolve_target_file(input_path: &Path, target_dir: &Path, default_extension: &str) -> Result<PathBuf> {
    if !target_dir.exists() && target_dir != Path::new(".") {
        std::fs::create_dir_all(target_dir)?;
    }

    let file_stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    let filename = format!("{}.{}", file_stem, default_extension);
    Ok(target_dir.join(filename))
}

fn resolve_and_expand_inputs(
    raw_args: Vec<PathBuf>,
    ext_filter: Option<&[&str]>,
) -> Result<Vec<PathBuf>> {
    let mut resolved_paths = Vec::new();

    for arg in raw_args {
        if let Some(s) = arg.to_str() {
            if let Some(list_file_path) = s.strip_prefix('@') {
                let file = std::fs::File::open(list_file_path).with_context(|| {
                    format!("Failed to open response file {:?}", list_file_path)
                })?;
                let reader = io::BufReader::new(file);
                for line in reader.lines() {
                    let line = line?;
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        resolved_paths.push(PathBuf::from(trimmed));
                    }
                }
                continue;
            }
        }
        resolved_paths.push(arg);
    }

    let mut final_files = Vec::new();
    for path in resolved_paths {
        if path.is_dir() {
            collect_files_from_dir(&path, &mut final_files, ext_filter)?;
        } else if path.exists() {
            if matches_extension(&path, ext_filter) {
                final_files.push(path);
            }
        } else {
            anyhow::bail!("Path not found: {:?}", path);
        }
    }

    Ok(final_files)
}

fn matches_extension(path: &Path, ext_filter: Option<&[&str]>) -> bool {
    if let Some(filters) = ext_filter {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            return filters.iter().any(|f| ext.eq_ignore_ascii_case(f));
        }
        return false;
    }
    true
}

fn collect_files_from_dir(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    ext_filter: Option<&[&str]>,
) -> Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if matches_extension(&path, ext_filter) {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}
