
use anyhow::Result;
use std::path::Path;

pub(super) fn file_name(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid or non-UTF-8 attachment filename: {:?}",
                path
            )
        })
}

// Helvetica is used for visible labels. Preserve the complete filename
// separately in the attachment metadata.
pub(super) fn visible_label(name: &str) -> String {
    let mut label: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '?'
            }
        })
        .collect();

    const MAX_CHARS: usize = 43;

    if label.chars().count() > MAX_CHARS {
        label = label
            .chars()
            .take(MAX_CHARS - 3)
            .collect::<String>()
            + "...";
    }

    label
}

// Escape characters that have special meaning inside a PDF literal string.
pub(super) fn pdf_text_literal(text: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(text.len());

    for byte in text.bytes() {
        match byte {
            b'(' | b')' | b'\\' => {
                result.push(b'\\');
                result.push(byte);
            }
            _ => result.push(byte),
        }
    }

    result
}

// Use an ASCII fallback for the legacy PDF filename field.
pub(super) fn pdf_filename(name: &str) -> Vec<u8> {
    name.bytes()
        .map(|b| {
            if b.is_ascii() && !b.is_ascii_control() {
                b
            } else {
                b'_'
            }
        })
        .collect()
}

// Preserve Unicode filenames using a UTF-16BE PDF string with a BOM.
pub(super) fn pdf_unicode_string(name: &str) -> lopdf::Object {
    let mut bytes = vec![0xFE, 0xFF];

    for unit in name.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }

    lopdf::Object::string_literal(bytes)
}

