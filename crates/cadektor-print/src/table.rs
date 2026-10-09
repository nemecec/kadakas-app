//! A worksheet table: the calculated cells of a worksheet drawn inside the frame of a rectangle
//! (see `cadektor_core::worksheet_table`). It is used on sheets and inside plan viewports.

use std::collections::HashMap;

use cadektor_core::hardscape::TerrainSource;
use cadektor_core::model::Document;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::worksheet_table::TableData;
use cadektor_sheet::{compute_with, layout, layout::is_number, Computed, Value, WorksheetDef};
use glam::DVec2;

use crate::display::*;
use crate::plan::Xf;

/// Share of a row height from the top to the baseline of the text.
const BASELINE: f64 = 0.72;
/// Width of a digit relative to the text height, to line numbers up on the right.
const DIGIT: f64 = 0.556;
const GRID: Rgba = [150, 150, 150, 255];
const ERROR: Rgba = [190, 40, 30, 255];

/// The calculated worksheets that the tables of a document show. They are calculated once per
/// page, with the ground of the site models, so draped hardscapes and landscape areas are measured
/// as the reports measure them.
#[derive(Default)]
pub struct Tables(HashMap<ResourceId, Computed>);

impl Tables {
    /// Calculates the worksheets of all table objects in the document.
    pub fn build(doc: &Document, ground: &dyn TerrainSource) -> Tables {
        let mut map = HashMap::new();
        for o in doc.objects.values() {
            let Some(d) = cadektor_core::worksheet_table::data(o) else { continue };
            if map.contains_key(&d.worksheet) {
                continue;
            }
            if let Some(ResourceBody::Worksheet(v)) = doc.resources.get(d.worksheet).map(|r| &r.body)
                && let Ok(def) = WorksheetDef::from_value(v)
            {
                map.insert(d.worksheet, compute_with(&def, doc, ground));
            }
        }
        Tables(map)
    }

    pub fn get(&self, id: ResourceId) -> Option<&Computed> {
        self.0.get(&id)
    }
}

/// The primitives of a table. `lo` and `hi` are the corners of the frame in model space, and the
/// table starts at the top left. Cells that do not fit are cut at the frame.
pub fn table_prims(tables: &Tables, lo: DVec2, hi: DVec2, d: &TableData, xf: &Xf, ink: Rgba, out: &mut Vec<Prim>) {
    let Some(c) = tables.get(d.worksheet) else { return };
    let l = layout(c, d.text_height);
    let h = d.text_height;
    let top = hi.y;
    let frame = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|p| xf.p(p)).to_vec();
    out.push(Prim::PushClip(frame));
    let mut x = lo.x;
    let mut col_x = vec![x];
    for w in &l.col_widths {
        x += w;
        col_x.push(x);
    }
    let bottom = top - l.height;
    let line = |a: DVec2, b: DVec2| Prim::Path(PathPrim::stroked(PathPrim::polyline(&[xf.p(a), xf.p(b)]), false, GRID, 0.1));
    for r in 0..=l.rows {
        let y = top - f64::from(r) * l.row_height;
        out.push(line(DVec2::new(lo.x, y), DVec2::new(x, y)));
    }
    for cx in &col_x {
        out.push(line(DVec2::new(*cx, top), DVec2::new(*cx, bottom)));
    }
    for r in 1..=l.rows {
        for col in 1..=l.cols {
            let text = c.display(r, col);
            if text.is_empty() {
                continue;
            }
            let (left, right) = (col_x[col as usize - 1], col_x[col as usize]);
            let base = top - f64::from(r - 1) * l.row_height - l.row_height * BASELINE;
            let at_x = if is_number(c, r, col) { (right - 0.45 * h - text.chars().count() as f64 * DIGIT * h).max(left + 0.2 * h) } else { left + 0.45 * h };
            let color = if matches!(c.value(r, col), Value::Error(_)) { ERROR } else { ink };
            out.push(Prim::Text(TextPrim { at: xf.p(DVec2::new(at_x, base)), text, height: xf.len(h), color, align: Align::Left }));
        }
    }
    out.push(Prim::PopClip);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::{compose_page, compose_page_on};
    use cadektor_core::hardscape::NoTerrain;
    use cadektor_sheet::compute;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::{LayerId, ObjectId};
    use cadektor_core::resource::ResourceId;

    fn setup() -> (Editor, ResourceId, LayerId) {
        let mut ed = Editor::default();
        let mut def = WorksheetDef::default();
        def.set_cell("A1", "Item", None).unwrap();
        def.set_cell("B1", "Qty", None).unwrap();
        def.set_cell("A2", "Trees", None).unwrap();
        def.set_cell("B2", "12", None).unwrap();
        def.set_cell("A3", "Bad", None).unwrap();
        def.set_cell("B3", "=1/0", None).unwrap();
        let ws = ResourceId(ed.execute(&Command::AddResource { name: "Qty".into(), body: ResourceBody::Worksheet(def.to_value()) }).unwrap().created[0]);
        ed.execute(&Command::AddSheet { name: "A1".into(), paper: None, orientation: None, number: None, title: None }).unwrap();
        let sheet = ed.doc().sheet_layers().next().unwrap().id;
        (ed, ws, sheet)
    }

    fn texts(prims: &[Prim]) -> Vec<String> {
        prims.iter().filter_map(|p| if let Prim::Text(t) = p { Some(t.text.clone()) } else { None }).collect()
    }

    #[test]
    fn a_table_on_a_sheet_prints_its_cells() {
        let (mut ed, ws, sheet) = setup();
        let l = layout(&compute(&WorksheetDef::from_value(match &ed.doc().resources.get(ws).unwrap().body {
            ResourceBody::Worksheet(v) => v,
            _ => unreachable!(),
        })
        .unwrap(), ed.doc()), 3.0);
        ed.execute(&Command::AddWorksheetTable { worksheet: ws, at: DVec2::new(20.0, 200.0), size: DVec2::new(l.width, l.height), text_height: Some(3.0), layer: Some(sheet), class: None, attrs: Default::default() }).unwrap();
        let page = compose_page(ed.doc(), sheet).unwrap();
        let t = texts(&page.prims);
        for want in ["Item", "Qty", "Trees", "12", "Bad", "#DIV/0!"] {
            assert!(t.contains(&want.to_string()), "{want} in {t:?}");
        }
        let clips = page.prims.iter().filter(|p| matches!(p, Prim::PushClip(_))).count();
        let pops = page.prims.iter().filter(|p| matches!(p, Prim::PopClip)).count();
        assert_eq!((clips, pops), (1, 1));
        // Numbers line up on the right edge of their column, text on the left.
        let at = |s: &str| page.prims.iter().find_map(|p| if let Prim::Text(t) = p && t.text == s { Some(t.at) } else { None }).unwrap();
        assert!(at("12").x > at("Qty").x - 1e-9 && at("Trees").x < at("12").x);
        // The error cell is red.
        assert!(page.prims.iter().any(|p| matches!(p, Prim::Text(t) if t.text == "#DIV/0!" && t.color == ERROR)));
        // The table follows the document: change a cell and the page changes. The sheet is built again for the new revision.
        let mut def = WorksheetDef::default();
        def.set_cell("A1", "Changed", None).unwrap();
        ed.execute(&Command::ModifyResource { id: ws, name: None, body: Some(ResourceBody::Worksheet(def.to_value())) }).unwrap();
        let page = compose_page(ed.doc(), sheet).unwrap();
        assert_eq!(texts(&page.prims).iter().filter(|t| t.as_str() == "Changed").count(), 1);
    }

    /// A hill: the ground rises with x.
    struct Hill;

    impl TerrainSource for Hill {
        fn elevation(&self, _: &cadektor_core::model::Object, p: DVec2) -> Option<f64> {
            Some(p.x * 0.5)
        }
    }

    #[test]
    fn tables_are_calculated_with_the_ground() {
        let mut ed = Editor::default();
        let ring = cadektor_core::curve::corners(&[DVec2::ZERO, DVec2::new(4000.0, 0.0), DVec2::new(4000.0, 4000.0), DVec2::new(0.0, 4000.0)]);
        ed.execute(&Command::AddHardscape {
            boundary: Some(ring),
            patch: Box::new(cadektor_core::hardscape::HardscapePatch { drape: Some(true), ..Default::default() }),
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
        let mut def = WorksheetDef::default();
        def.databases.push(cadektor_sheet::DatabaseDef {
            criteria: "Kind = \"hardscape\"".into(),
            header: false,
            total: false,
            columns: vec![cadektor_sheet::ColumnDef { header: "A".into(), expr: "ROUND(Area, 3)".into(), ..Default::default() }],
            ..Default::default()
        });
        let ws = ResourceId(ed.execute(&Command::AddResource { name: "A".into(), body: ResourceBody::Worksheet(def.to_value()) }).unwrap().created[0]);
        ed.execute(&Command::AddSheet { name: "A1".into(), paper: None, orientation: None, number: None, title: None }).unwrap();
        let sheet = ed.doc().sheet_layers().next().unwrap().id;
        ed.execute(&Command::AddWorksheetTable { worksheet: ws, at: DVec2::new(10.0, 100.0), size: DVec2::new(60.0, 10.0), text_height: Some(3.0), layer: Some(sheet), class: None, attrs: Default::default() }).unwrap();
        // The tables of a page follow the ground they are given.
        let flat = Tables::build(ed.doc(), &NoTerrain);
        let hill = Tables::build(ed.doc(), &Hill);
        assert_eq!(flat.get(ws).unwrap().value_at("A1"), Some(Value::Number(16.0)));
        let Some(Value::Number(on_hill)) = hill.get(ws).unwrap().value_at("A1") else { panic!() };
        assert!(on_hill > 16.5, "{on_hill}");
        let t_flat = texts(&compose_page(ed.doc(), sheet).unwrap().prims);
        let t_hill = texts(&compose_page_on(ed.doc(), sheet, &Hill).unwrap().prims);
        assert_eq!(t_flat, vec!["16".to_string()]);
        assert_eq!(t_hill, vec![cadektor_sheet::value::format_number(on_hill)]);
        // The page cache and the PDF take the ground too.
        let mut cache = crate::SheetCache::default();
        let page = cache.page_on(ed.doc(), 1, sheet, &Hill).unwrap();
        assert_eq!(texts(&page.prims), t_hill);
        let pdf = crate::export_pdf_on(ed.doc(), 2, &mut cache, &[sheet], &Hill).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn a_table_in_a_viewport_is_scaled_and_a_missing_worksheet_draws_nothing() {
        let (mut ed, ws, sheet) = setup();
        let id = ObjectId(ed.execute(&Command::AddWorksheetTable { worksheet: ws, at: DVec2::new(0.0, 5000.0), size: DVec2::new(8000.0, 4000.0), text_height: Some(200.0), layer: None, class: None, attrs: Default::default() }).unwrap().created[0]);
        let layer = ed.doc().object(id).unwrap().layer;
        ed.execute(&Command::AddObject {
            shape: cadektor_core::model::Shape::Viewport(Box::new(cadektor_core::sheets::Viewport::new(DVec2::new(10.0, 10.0), DVec2::new(200.0, 140.0), DVec2::new(4000.0, 2500.0), 50.0, vec![layer]))),
            layer: Some(sheet),
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
        let page = compose_page(ed.doc(), sheet).unwrap();
        let heights: Vec<f64> = page.prims.iter().filter_map(|p| if let Prim::Text(t) = p { Some(t.height) } else { None }).collect();
        assert!(!heights.is_empty());
        assert!(heights.iter().all(|h| (*h - 200.0 / 50.0).abs() < 1e-9), "text is 200 mm in the model, 4 mm on paper at 1:50: {heights:?}");
        // The data of a worksheet that is gone draws only the frame.
        let obj = ed.doc().object(id).unwrap().clone();
        let mut out = vec![];
        let gone = TableData { worksheet: ResourceId(9999), text_height: 3.0 };
        table_prims(&Tables::build(ed.doc(), &NoTerrain), DVec2::ZERO, DVec2::ONE, &gone, &Xf::IDENTITY, [0, 0, 0, 255], &mut out);
        assert!(out.is_empty());
        let not_a_sheet = TableData { worksheet: ResourceId(ed.doc().layers[0].id.0), text_height: 3.0 };
        table_prims(&Tables::build(ed.doc(), &NoTerrain), DVec2::ZERO, DVec2::ONE, &not_a_sheet, &Xf::IDENTITY, [0, 0, 0, 255], &mut out);
        assert!(out.is_empty());
        assert!(cadektor_core::worksheet_table::data(&obj).is_some());
    }
}
