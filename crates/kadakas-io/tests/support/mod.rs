//! Helpers that several test files of the DXF import share.
#![allow(dead_code)]

pub mod corpus;

use kadakas_core::command::Editor;
use kadakas_core::model::Document;
use kadakas_io::{Context, Format, ImportOptions, IoError, Report, Source, importer};

/// Imports DXF bytes into an empty document and runs the batch.
pub fn import_with(data: &[u8], opts: &ImportOptions) -> Result<(Editor, Report), IoError> {
    let src = Source {
        name: "test".into(),
        data: data.to_vec(),
        extra: Default::default(),
    };
    let doc = Document::with_sequential_ids();
    let r = importer(Format::Dxf).import(&src, &Context::of(&doc), opts)?;
    let mut ed = Editor::new(doc);
    ed.execute(&r.command)
        .expect("a successful import must run");
    Ok((ed, r.report))
}

pub fn import(data: &[u8]) -> Result<(Editor, Report), IoError> {
    import_with(data, &ImportOptions::default())
}

/// The pairs of a text DXF.
pub fn pairs(text: &str) -> Vec<(i32, String)> {
    let mut lines = text.lines();
    let mut out = Vec::new();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        if let Ok(c) = c.trim().parse() {
            out.push((c, v.to_string()));
        }
    }
    out
}

/// The shapes of the objects as JSON, in order. Two imports of the same drawing give equal lists.
pub fn shapes(ed: &Editor) -> Vec<serde_json::Value> {
    ed.doc()
        .objects
        .values()
        .map(|o| serde_json::to_value(&o.shape).unwrap())
        .collect()
}

/// A minimal DXF text with the given header lines and entity lines.
pub fn dxf(header: &str, entities: &str) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

/// The message of the refusal of a file. Panics if the file is imported.
pub fn refusal(data: &[u8]) -> String {
    match import(data) {
        Ok(_) => panic!("the file was imported, but it should be refused"),
        Err(e) => e.to_string(),
    }
}
