use anyhow::{Context, Result};
use lopdf::{Document, Object, Bookmark, Dictionary, ObjectId};
use std::path::PathBuf;

/// Helper function to strictly extract leaf-level `/Page` object IDs from a document
fn extract_leaf_pages(doc: &Document) -> Vec<ObjectId> {
    let mut page_ids = Vec::new();
    for (id, obj) in &doc.objects {
        if let Ok(dict) = obj.as_dict() {
            if dict.get(b"Type").ok().and_then(|o| o.as_name().ok()) == Some(b"Page") {
                page_ids.push(*id);
            }
        }
    }
    // Sort deterministically to maintain stable page ordering
    page_ids.sort_by_key(|k| (k.0, k.1));
    page_ids
}

pub fn run_merge(inputs: Vec<PathBuf>, output: PathBuf) -> Result<()> {
    if inputs.is_empty() {
        anyhow::bail!("No input files provided for merging.");
    }
    println!("Merging {} file(s) using robust object isolation...", inputs.len());

    // 1. Load the base document
    let mut base_doc = Document::load(&inputs[0])
        .with_context(|| format!("Failed to open base PDF {:?}", inputs[0]))?;

    // Add bookmark for the first file
    let base_pages = extract_leaf_pages(&base_doc);
    if let Some(&first_page_id) = base_pages.first() {
        if let Some(name) = inputs[0].file_name().and_then(|n| n.to_str()) {
            let bookmark = Bookmark::new(name.to_string(), [0.0, 0.0, 1.0], 0, first_page_id);
            base_doc.add_bookmark(bookmark, None);
        }
    }

    // 2. Process all subsequent documents
    for path in &inputs[1..] {
        let mut doc = Document::load(path)
            .with_context(|| format!("Failed to open PDF {:?}", path))?;

        // Find the absolute highest object ID currently existing in base_doc
        let max_id: u32 = base_doc.objects.keys().map(|k| k.0).max().unwrap_or(0);

        // Shift incoming document's object IDs so they start safely above base_doc's max_id
        doc.renumber_objects_with(max_id + 1);

        // Extract the renumbered incoming document's first page before merging objects
        let doc_pages = extract_leaf_pages(&doc);
        let doc_first_page = doc_pages.first().copied();

        // Insert all renumbered objects into base_doc
        for (id, obj) in doc.objects {
            base_doc.objects.insert(id, obj);
        }

        // Add bookmark pointing to this document's first page
        if let Some(first_id) = doc_first_page {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let bookmark = Bookmark::new(name.to_string(), [0.0, 0.0, 1.0], 0, first_id);
                base_doc.add_bookmark(bookmark, None);
            }
        }
    }

    // 3. Final global renumber pass to ensure continuous, clean object IDs
    base_doc.renumber_objects();

    // 4. Extract ALL final leaf page IDs from the fully assembled and renumbered base_doc
    let final_page_ids = extract_leaf_pages(&base_doc);
    let page_count = final_page_ids.len() as i64;

    println!("Total assembled leaf pages found: {}", page_count);

    if final_page_ids.is_empty() {
        anyhow::bail!("Error: Assembled PDF contains 0 valid pages.");
    }

    // 5. Build the unified master Pages catalog tree dictionary
    let pages_id = base_doc.new_object_id();
    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
    pages_dict.set(
        "Kids",
        Object::Array(final_page_ids.into_iter().map(Object::Reference).collect()),
    );
    pages_dict.set("Count", Object::Integer(page_count));
    
    base_doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

    // 6. Point the document catalog to our master pages root
    if let Ok(catalog) = base_doc.catalog_mut() {
        catalog.set("Pages", Object::Reference(pages_id));
    }

    // 7. Finalize outlines and zero pages
    base_doc.adjust_zero_pages();
    if let Some(outline_id) = base_doc.build_outline() {
        if let Ok(catalog) = base_doc.catalog_mut() {
            catalog.set("Outlines", Object::Reference(outline_id));
        }
    }

    // 8. Save output
    base_doc.save(&output)
        .with_context(|| format!("Failed to save merged PDF to {:?}", output))?;

    if !output.exists() {
        anyhow::bail!("Merge finished, but output file was not created at {:?}", output);
    }

    println!("Successfully merged into {:?} with all {} pages intact.", output, page_count);
    Ok(())
}
