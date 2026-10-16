//! The closed outlines of one block of a DXF file, for a module pattern.
//!
//! The block goes through the DXF import: the file is read again with an ENTITIES section that has
//! only one INSERT of the block, exploded. So polylines with bulges (arcs), rectangles, circles and
//! nested blocks come out as the import makes them, in millimeters.

use kadakas_core::command::Editor;
use kadakas_core::curve::Vertex;
use kadakas_core::layout::object::outline_of;
use kadakas_core::model::Document;

use crate::options::OriginMode;
use crate::{Context, ImportOptions, Importer, IoError, Source};

/// A closed outline of a block: the DXF layer it is on (the type of the unit), and the outline.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockOutline {
    pub layer: String,
    pub outline: Vec<Vertex>,
}

/// The names of the blocks of a file.
pub fn block_names(bytes: &[u8]) -> Result<Vec<String>, IoError> {
    let dxf = super::parse::parse(bytes)?;
    let mut names: Vec<String> = dxf
        .blocks
        .keys()
        .filter(|n| !n.starts_with('*'))
        .cloned()
        .collect();
    names.sort();
    Ok(names)
}

/// The text of the file with an ENTITIES section that holds one INSERT of `block`.
fn with_one_insert(text: &str, block: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_entities = false;
    let mut found = false;
    let insert = format!("0\nINSERT\n8\n0\n2\n{block}\n10\n0\n20\n0\n30\n0\n");
    while i + 1 < lines.len() {
        let (code, value) = (lines[i].trim(), lines[i + 1].trim());
        // A file without an ENTITIES section gets one before its end.
        if !found && !in_entities && code == "0" && value == "EOF" {
            out.push_str(&format!("0\nSECTION\n2\nENTITIES\n{insert}0\nENDSEC\n"));
            found = true;
        }
        if in_entities {
            if code == "0" && value == "ENDSEC" {
                in_entities = false;
                out.push_str(&insert);
                out.push_str("0\nENDSEC\n");
            }
            i += 2;
            continue;
        }
        if code == "2"
            && value == "ENTITIES"
            && i >= 2
            && lines[i - 2].trim() == "0"
            && lines[i - 1].trim() == "SECTION"
        {
            in_entities = true;
            found = true;
            out.push_str("2\nENTITIES\n");
            i += 2;
            continue;
        }
        out.push_str(lines[i]);
        out.push('\n');
        out.push_str(lines[i + 1]);
        out.push('\n');
        i += 2;
    }
    out
}

/// The closed outlines of the block `block` of a DXF file. An error if the block does not exist
/// (the message lists the blocks) or has no closed outline.
pub fn block_outlines(bytes: &[u8], block: &str) -> Result<Vec<BlockOutline>, IoError> {
    let dxf = super::parse::parse(bytes)?;
    if !dxf.blocks.contains_key(block) {
        let mut names: Vec<&String> = dxf.blocks.keys().filter(|n| !n.starts_with('*')).collect();
        names.sort();
        return Err(IoError::Option(format!(
            "the file has no block '{block}'. Blocks: {}",
            if names.is_empty() {
                "none".to_string()
            } else {
                names
                    .iter()
                    .map(|n| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )));
    }
    let text = with_one_insert(&crate::decode_text(bytes), block);
    let source = Source {
        name: "block".into(),
        data: text.into_bytes(),
        extra: Default::default(),
    };
    let opts = ImportOptions {
        origin: OriginMode::File,
        track_source: false,
        explode_blocks: true,
        hatches: false,
        ..ImportOptions::default()
    };
    let scratch = Document::with_sequential_ids();
    let r = super::DxfFormat.import(&source, &Context::of(&scratch), &opts)?;
    let mut ed = Editor::new(scratch);
    ed.execute(&r.command)
        .map_err(|e| IoError::Format(format!("the block cannot be read: {e}")))?;
    let doc = ed.doc();
    let mut out = Vec::new();
    for o in doc.objects.values() {
        if let Ok((outline, _)) = outline_of(o) {
            let layer = doc
                .class(o.class)
                .map(|c| c.name.clone())
                .filter(|n| n != "None")
                .unwrap_or_else(|| "unit".into());
            out.push(BlockOutline { layer, outline });
        }
    }
    if out.is_empty() {
        return Err(IoError::Option(format!(
            "the block '{block}' has no closed polyline, rectangle or circle"
        )));
    }
    Ok(out)
}
