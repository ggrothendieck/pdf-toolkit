
use anyhow::{Context, Result};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, ObjectId, Stream};

use super::names::{pdf_text_literal, pdf_unicode_string, pdf_filename, visible_label};

const PAGE_WIDTH: i64 = 612;
const PAGE_HEIGHT: i64 = 792;
const ROWS_PER_COLUMN: usize = 25;
const COLUMNS: usize = 2;
const LEFT_MARGIN: i64 = 36;
const COLUMN_WIDTH: i64 = 270;
const ROW_HEIGHT: i64 = 27;
const ICON_SIZE: i64 = 18;

pub(super) struct Attachment {
    pub(super) name: String,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn create_attachment_page(
    doc: &mut Document,
    pages_id: ObjectId,
    font_id: ObjectId,
    attachments: &[Attachment],
) -> Result<ObjectId> {
    let mut operations = vec![
        Operation::new("BT", vec![]),
        Operation::new(
            "Tf",
            vec![Object::Name(b"F1".to_vec()), Object::Integer(16)],
        ),
        Operation::new(
            "Td",
            vec![Object::Integer(LEFT_MARGIN), Object::Integer(752)],
        ),
        Operation::new(
            "Tj",
            vec![Object::string_literal(b"Attachments".to_vec())],
        ),
        Operation::new("ET", vec![]),
    ];

    let resources = dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    };

    let mut annotations = Vec::with_capacity(attachments.len());

    for (index, attachment) in attachments.iter().enumerate() {
        let column = index / ROWS_PER_COLUMN;
        let row = index % ROWS_PER_COLUMN;

        debug_assert!(column < COLUMNS);

        let x = LEFT_MARGIN + column as i64 * COLUMN_WIDTH;
        let y = 720 - row as i64 * ROW_HEIGHT;

        let label = visible_label(&attachment.name);

        // Draw the visible filename beside the attachment icon.
        operations.extend([
            Operation::new("BT", vec![]),
            Operation::new(
                "Tf",
                vec![Object::Name(b"F1".to_vec()), Object::Integer(9)],
            ),
            Operation::new(
                "Td",
                vec![
                    Object::Integer(x + ICON_SIZE + 6),
                    Object::Integer(y),
                ],
            ),
            Operation::new(
                "Tj",
                vec![Object::string_literal(pdf_text_literal(&label))],
            ),
            Operation::new("ET", vec![]),
        ]);

        // Store the attachment bytes in an EmbeddedFile stream.
        let embedded_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "EmbeddedFile",
                "Params" => dictionary! {
                    "Size" => attachment.bytes.len() as i64,
                },
            },
            attachment.bytes.clone(),
        ));

        let file_spec_id = doc.add_object(dictionary! {
            "Type" => "Filespec",
            "F" => Object::string_literal(pdf_filename(&attachment.name)),
            "UF" => pdf_unicode_string(&attachment.name),
            "EF" => dictionary! {
                "F" => embedded_id,
                "UF" => embedded_id,
            },
        });

        // Create a clickable file-attachment annotation.
        let annotation_id = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "FileAttachment",
            "Rect" => Object::Array(vec![
                Object::Integer(x),
                Object::Integer(y - 5),
                Object::Integer(x + ICON_SIZE),
                Object::Integer(y + 13),
            ]),
            "Contents" => pdf_unicode_string(&attachment.name),
            "Name" => "PushPin",
            "FS" => file_spec_id,
            "P" => Object::Null,
            "F" => 4,
        });

        annotations.push(Object::Reference(annotation_id));
    }

    let content = Content { operations }
        .encode()
        .context("Failed to encode attachment-page content")?;

    let content_id = doc.add_object(Stream::new(dictionary! {}, content));

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(PAGE_WIDTH),
            Object::Integer(PAGE_HEIGHT),
        ]),
        "Resources" => resources,
        "Contents" => content_id,
        "Annots" => Object::Array(annotations),
    });

    // Collect IDs first to avoid overlapping immutable and mutable borrows.
    let annotation_ids: Vec<ObjectId> = doc
        .get_dictionary(page_id)?
        .get(b"Annots")?
        .as_array()?
        .iter()
        .map(|annotation| annotation.as_reference())
        .collect::<std::result::Result<Vec<_>, _>>()?;

    for annotation_id in annotation_ids {
        doc.get_dictionary_mut(annotation_id)?
            .set("P", Object::Reference(page_id));
    }

    Ok(page_id)
}

