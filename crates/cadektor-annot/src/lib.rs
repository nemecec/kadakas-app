//! Annotation that needs data. The core draws dimensions, callouts and the other annotations
//! (see `cadektor_core::annot`). This crate adds what needs the worksheet code:
//!
//! - data tags: each line of a tag style is text with fields in braces. A field is a worksheet
//!   expression over the properties of the tagged object, calculated by the same code as the
//!   database rows of worksheets ([`render_template`]);
//! - graphic legends: the criteria and the group expression are worksheet expressions too
//!   ([`legend_rows`]);
//! - `tag_all`: a data tag for every object that matches criteria ([`tag_all`]);
//! - the output of annotations as primitives and SVG ([`prims`], [`svg`]).
//!
//! The crate is pure (no files, no threads), so it also builds for WebAssembly.

use std::collections::HashMap;

use cadektor_core::annot::Annot;
use cadektor_core::annot::draw::{self, Ctx, LegendRow, Prim, TextSource};
use cadektor_core::annot::style::TagStyle;
use cadektor_core::annot::{DataTag, Legend};
use cadektor_core::hardscape::NoTerrain;
use cadektor_core::model::{Color, Document, Object, Shape};
use cadektor_core::plants::link;
use cadektor_sheet::db::ObjEnv;
use cadektor_sheet::eval::eval;
use cadektor_sheet::formula::{self, Expr};
use cadektor_sheet::value::Value;

mod cache;
mod tags;
pub mod svg;

pub use cache::AnnotCache;
pub use tags::{RevisionInfo, revisions, tag_all};

/// The environment of an object: names are its properties (the same as in worksheet databases).
fn env<'a>(doc: &'a Document, obj: &'a Object) -> ObjEnv<'a> {
    ObjEnv { doc, obj, ground: &NoTerrain }
}

/// Calculates one expression for an object.
pub fn eval_for(doc: &Document, obj: &Object, expr: &str) -> Value {
    match formula::parse(expr) {
        Ok(e) => eval(&e, &env(doc, obj)),
        Err(_) => Value::Error(cadektor_sheet::ErrorKind::Parse),
    }
}

/// Fills the fields of a template with the properties of an object. `{IdCode} / {ROUND(Area, 1)} m2`
/// gives `ACE-1 / 12.5 m2`. A field that cannot be calculated shows its error code (`#NAME?`).
/// An unclosed brace stays as it is.
pub fn render_template(doc: &Document, obj: &Object, template: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        out.push_str(&eval_for(doc, obj, &after[..close]).to_text());
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// The lines of a data tag.
pub fn tag_lines(doc: &Document, tag: &DataTag, style: &TagStyle) -> Vec<String> {
    match doc.object(tag.object) {
        Some(obj) => style.lines.iter().map(|l| render_template(doc, obj, l)).collect(),
        None => vec!["?".into()],
    }
}

/// True for the objects that a legend or `tag_all` looks at: drawing objects, not annotations,
/// viewports or the tags of plants.
pub fn is_subject(doc: &Document, obj: &Object) -> bool {
    doc.is_visible(obj) && !matches!(obj.shape, Shape::Annotation(_) | Shape::Viewport(_) | Shape::TitleBlock(_) | Shape::PlantTag { .. })
}

fn parse_criteria(src: &str) -> Result<Option<Expr>, String> {
    if src.trim().is_empty() { Ok(None) } else { formula::parse(src).map(Some).map_err(|e| format!("criteria: {e}")) }
}

/// True if the object matches the criteria. An error in the criteria for this object counts as no match.
fn matches(doc: &Document, obj: &Object, criteria: &Option<Expr>) -> bool {
    criteria.as_ref().is_none_or(|c| eval(c, &env(doc, obj)).to_bool() == Ok(true))
}

/// The objects that match the criteria (a worksheet expression, empty for all), in document order.
pub fn select<'a>(doc: &'a Document, criteria: &str) -> Result<Vec<&'a Object>, String> {
    let c = parse_criteria(criteria)?;
    Ok(doc.objects.values().filter(|o| is_subject(doc, o) && matches(doc, o, &c)).collect())
}

/// The colors a swatch shows for an object: fill (if it has one) and pen.
fn swatch(doc: &Document, obj: &Object) -> (Option<Color>, Option<Color>) {
    let solid = |c: Option<Color>| c.filter(|c| c.0[3] > 0);
    if matches!(obj.shape, Shape::Plant { .. }) && link::link(obj).is_some() {
        let look = link::look(doc, obj);
        return (solid(look.fill).or(solid(look.pen)), solid(look.pen));
    }
    let a = cadektor_core::hardscape::styled_attrs(doc, obj, doc.resolved_attrs(obj));
    (solid(a.fill), solid(a.pen).or(Some(Color::BLACK)))
}

/// Most rows that a legend shows. The rest is one row, `... N more`.
pub const MAX_LEGEND_ROWS: usize = 500;

/// The rows of a legend: the objects that match the criteria, grouped by the group expression
/// (text, ignoring case), sorted by label. Each row has the label, the number of objects and the
/// colors of the first object of the group. After `MAX_LEGEND_ROWS` rows, one row counts the rest.
pub fn legend_rows(doc: &Document, legend: &Legend) -> Vec<LegendRow> {
    let Ok(criteria) = parse_criteria(&legend.criteria) else { return Vec::new() };
    let Ok(group) = formula::parse(if legend.group_by.trim().is_empty() { "Style" } else { &legend.group_by }) else { return Vec::new() };
    let mut rows: Vec<LegendRow> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for obj in doc.objects.values().filter(|o| is_subject(doc, o) && matches(doc, o, &criteria)) {
        let key = match eval(&group, &env(doc, obj)) {
            Value::Empty | Value::Error(_) => continue,
            v => v.to_text(),
        };
        if key.trim().is_empty() {
            continue;
        }
        match index.get(&key.to_lowercase()) {
            Some(&i) => rows[i].count += 1,
            None => {
                let (fill, stroke) = swatch(doc, obj);
                index.insert(key.to_lowercase(), rows.len());
                rows.push(LegendRow { label: key, count: 1, fill, stroke });
            }
        }
    }
    rows.sort_by_cached_key(|r| r.label.to_lowercase());
    if rows.len() > MAX_LEGEND_ROWS {
        let rest = rows.split_off(MAX_LEGEND_ROWS);
        rows.push(LegendRow { label: format!("... {} more", rest.len()), count: rest.iter().map(|r| r.count).sum(), fill: None, stroke: None });
    }
    rows
}

/// The text source that uses the worksheet code.
pub struct Sheets;

impl TextSource for Sheets {
    fn tag_lines(&self, doc: &Document, tag: &DataTag, style: &TagStyle) -> Vec<String> {
        tag_lines(doc, tag, style)
    }

    fn legend_rows(&self, doc: &Document, legend: &Legend) -> Vec<LegendRow> {
        legend_rows(doc, legend)
    }
}

/// The primitives of an annotation object on its layer, in the coordinates of the layer.
pub fn prims(doc: &Document, obj: &Object) -> Vec<Prim> {
    match &obj.shape {
        Shape::Annotation(a) => draw::prims(doc, a, &Ctx::layer(doc, obj.layer), &Sheets),
        _ => Vec::new(),
    }
}

/// The primitives of an annotation in a context of your choice (for example a viewport).
pub fn prims_in(doc: &Document, a: &Annot, ctx: &Ctx) -> Vec<Prim> {
    draw::prims(doc, a, ctx, &Sheets)
}

#[cfg(test)]
mod tests;
