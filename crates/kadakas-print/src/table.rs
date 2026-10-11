//! A worksheet table: the calculated cells of a worksheet drawn inside the frame of a rectangle
//! (see `kadakas_core::worksheet_table`). It is used on sheets and inside plan viewports.

use std::collections::HashMap;
use std::sync::Arc;

use glam::DVec2;
use kadakas_core::ground::GroundSource;
use kadakas_core::model::Document;
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::worksheet_table::TableData;
use kadakas_sheet::{Computed, Value, WorksheetDef, compute_with, layout, layout::is_number};

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
pub struct Tables {
    map: HashMap<ResourceId, Computed>,
    /// The thumbnails of the pictures in the cells: width, height and RGB on white paper.
    pictures: HashMap<ResourceId, (u32, u32, Arc<Vec<u8>>)>,
    /// The site models of the page, for the contours, the analysis colors and the grading graphics.
    /// A page is made from one document, so the revision is always the same.
    site: kadakas_site::SiteCache,
    /// The section drawings, shared with whoever else draws sections, and the revision of the page.
    sections: Option<(kadakas_x3d::section::SharedSections, u64)>,
}

impl Tables {
    /// Calculates the worksheets of all table objects in the document.
    pub fn build(doc: &Document, ground: &dyn GroundSource) -> Tables {
        let mut map = HashMap::new();
        for o in doc.objects.values() {
            let Some(d) = kadakas_core::worksheet_table::data(o) else {
                continue;
            };
            if map.contains_key(&d.worksheet) {
                continue;
            }
            if let Some(ResourceBody::Worksheet(v)) =
                doc.resources.get(d.worksheet).map(|r| &r.body)
                && let Ok(def) = WorksheetDef::from_value(v)
            {
                map.insert(d.worksheet, compute_with(&def, doc, ground));
            }
        }
        // The pictures that the cells show. Each is decoded once.
        let mut pictures = HashMap::new();
        for c in map.values() {
            for (_, _, v) in c.cells() {
                if let Some(id) = v.image()
                    && !pictures.contains_key(&id)
                    && let Some(res) = doc.resources.image(id)
                    && let Some(p) = kadakas_render::picture::thumb_rgb_on_white(res)
                {
                    pictures.insert(id, p);
                }
            }
        }
        Tables {
            map,
            pictures,
            site: Default::default(),
            sections: None,
        }
    }

    /// Draws sections through a shared cache. `revision` is the revision of the document.
    pub fn with_sections(
        mut self,
        store: kadakas_x3d::section::SharedSections,
        revision: u64,
    ) -> Tables {
        self.sections = Some((store, revision));
        self
    }

    /// The shared section cache and the revision, if there is one.
    pub fn sections(&self) -> Option<(&kadakas_x3d::section::SharedSections, u64)> {
        self.sections.as_ref().map(|(s, r)| (s, *r))
    }

    /// The cache of the site models of the page.
    pub fn site(&self) -> &kadakas_site::SiteCache {
        &self.site
    }

    pub fn get(&self, id: ResourceId) -> Option<&Computed> {
        self.map.get(&id)
    }

    /// The thumbnail of a picture in a cell.
    pub fn picture(&self, id: ResourceId) -> Option<&(u32, u32, Arc<Vec<u8>>)> {
        self.pictures.get(&id)
    }
}

/// The primitives of a table. `lo` and `hi` are the corners of the frame in model space, and the
/// table starts at the top left. Cells that do not fit are cut at the frame.
pub fn table_prims(
    tables: &Tables,
    lo: DVec2,
    hi: DVec2,
    d: &TableData,
    xf: &Xf,
    ink: Rgba,
    out: &mut Vec<Prim>,
) {
    let Some(c) = tables.get(d.worksheet) else {
        return;
    };
    let l = layout(c, d.text_height);
    let h = d.text_height;
    let top = hi.y;
    let frame = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
        .map(|p| xf.p(p))
        .to_vec();
    out.push(Prim::PushClip(frame));
    let mut x = lo.x;
    let mut col_x = vec![x];
    for w in &l.col_widths {
        x += w;
        col_x.push(x);
    }
    let bottom = top - l.height;
    let line = |a: DVec2, b: DVec2| {
        Prim::Path(PathPrim::stroked(
            PathPrim::polyline(&[xf.p(a), xf.p(b)]),
            false,
            GRID,
            0.1,
        ))
    };
    for r in 0..=l.rows {
        let y = top - f64::from(r) * l.row_height;
        out.push(line(DVec2::new(lo.x, y), DVec2::new(x, y)));
    }
    for cx in &col_x {
        out.push(line(DVec2::new(*cx, top), DVec2::new(*cx, bottom)));
    }
    for r in 1..=l.rows {
        for col in 1..=l.cols {
            let (left, right) = (col_x[col as usize - 1], col_x[col as usize]);
            if let Some(id) = c.value(r, col).image() {
                // A picture fills the cell, with a margin, and keeps its proportions.
                if let Some((w, hgt, rgb)) = tables.picture(id) {
                    let margin = 0.15 * h;
                    let cell_top = top - f64::from(r - 1) * l.row_height;
                    let (box_w, box_h) = (right - left - 2.0 * margin, l.row_height - 2.0 * margin);
                    let k = (box_w / f64::from(*w)).min(box_h / f64::from(*hgt));
                    let (pw, ph) = (f64::from(*w) * k, f64::from(*hgt) * k);
                    let (x0, y0) = (
                        left + (right - left - pw) / 2.0,
                        cell_top - (l.row_height + ph) / 2.0,
                    );
                    let (a, b) = (xf.p(DVec2::new(x0, y0)), xf.p(DVec2::new(x0 + pw, y0 + ph)));
                    out.push(Prim::Image(ImagePrim {
                        min: a.min(b),
                        max: a.max(b),
                        width: *w,
                        height: *hgt,
                        rgb: rgb.clone(),
                    }));
                }
                continue;
            }
            let text = c.display(r, col);
            if text.is_empty() {
                continue;
            }
            let row_top = top - f64::from(r - 1) * l.row_height;
            // In a tall row (a table with pictures) the text sits in the middle of the row.
            let base = if l.has_images {
                row_top - l.row_height / 2.0 - 0.35 * h
            } else {
                row_top - l.row_height * BASELINE
            };
            let at_x = if is_number(c, r, col) {
                (right - 0.45 * h - text.chars().count() as f64 * DIGIT * h).max(left + 0.2 * h)
            } else {
                left + 0.45 * h
            };
            let color = if matches!(c.value(r, col), Value::Error(_)) {
                ERROR
            } else {
                ink
            };
            out.push(Prim::Text(TextPrim {
                at: xf.p(DVec2::new(at_x, base)),
                text,
                height: xf.len(h),
                color,
                align: Align::Left,
            }));
        }
    }
    out.push(Prim::PopClip);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::{compose_page, compose_page_on};
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::ground::NoTerrain;
    use kadakas_core::model::{LayerId, ObjectId};
    use kadakas_core::resource::ResourceId;
    use kadakas_sheet::compute;

    fn setup() -> (Editor, ResourceId, LayerId) {
        let mut ed = Editor::default();
        let mut def = WorksheetDef::default();
        def.set_cell("A1", "Item", None).unwrap();
        def.set_cell("B1", "Qty", None).unwrap();
        def.set_cell("A2", "Trees", None).unwrap();
        def.set_cell("B2", "12", None).unwrap();
        def.set_cell("A3", "Bad", None).unwrap();
        def.set_cell("B3", "=1/0", None).unwrap();
        let ws = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Qty".into(),
                body: ResourceBody::Worksheet(def.to_value()),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::AddSheet {
            name: "A1".into(),
            paper: None,
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap();
        let sheet = ed.doc().sheet_layers().next().unwrap().id;
        (ed, ws, sheet)
    }

    fn texts(prims: &[Prim]) -> Vec<String> {
        prims
            .iter()
            .filter_map(|p| {
                if let Prim::Text(t) = p {
                    Some(t.text.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn a_table_on_a_sheet_prints_its_cells() {
        let (mut ed, ws, sheet) = setup();
        let l = layout(
            &compute(
                &WorksheetDef::from_value(match &ed.doc().resources.get(ws).unwrap().body {
                    ResourceBody::Worksheet(v) => v,
                    _ => unreachable!(),
                })
                .unwrap(),
                ed.doc(),
            ),
            3.0,
        );
        ed.execute(&Command::AddWorksheetTable {
            worksheet: ws,
            at: DVec2::new(20.0, 200.0),
            size: DVec2::new(l.width, l.height),
            text_height: Some(3.0),
            layer: Some(sheet),
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
        let page = compose_page(ed.doc(), sheet).unwrap();
        let t = texts(&page.prims);
        for want in ["Item", "Qty", "Trees", "12", "Bad", "#DIV/0!"] {
            assert!(t.contains(&want.to_string()), "{want} in {t:?}");
        }
        let clips = page
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::PushClip(_)))
            .count();
        let pops = page
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::PopClip))
            .count();
        assert_eq!((clips, pops), (1, 1));
        // Numbers line up on the right edge of their column, text on the left.
        let at = |s: &str| {
            page.prims
                .iter()
                .find_map(|p| {
                    if let Prim::Text(t) = p
                        && t.text == s
                    {
                        Some(t.at)
                    } else {
                        None
                    }
                })
                .unwrap()
        };
        assert!(at("12").x > at("Qty").x - 1e-9 && at("Trees").x < at("12").x);
        // The error cell is red.
        assert!(
            page.prims
                .iter()
                .any(|p| matches!(p, Prim::Text(t) if t.text == "#DIV/0!" && t.color == ERROR))
        );
        // The table follows the document: change a cell and the page changes. The sheet is built again for the new revision.
        let mut def = WorksheetDef::default();
        def.set_cell("A1", "Changed", None).unwrap();
        ed.execute(&Command::ModifyResource {
            id: ws,
            name: None,
            body: Some(ResourceBody::Worksheet(def.to_value())),
        })
        .unwrap();
        let page = compose_page(ed.doc(), sheet).unwrap();
        assert_eq!(
            texts(&page.prims)
                .iter()
                .filter(|t| t.as_str() == "Changed")
                .count(),
            1
        );
    }

    /// A hill: the ground rises with x.
    struct Hill;

    impl GroundSource for Hill {
        fn exact(
            &self,
            _: Option<&kadakas_core::model::Object>,
            p: DVec2,
            _: kadakas_core::ground::Surface,
        ) -> Option<f64> {
            Some(p.x * 0.5)
        }
    }

    #[test]
    fn tables_are_calculated_with_the_ground() {
        let mut ed = Editor::default();
        let ring = kadakas_core::curve::corners(&[
            DVec2::ZERO,
            DVec2::new(4000.0, 0.0),
            DVec2::new(4000.0, 4000.0),
            DVec2::new(0.0, 4000.0),
        ]);
        ed.execute(&Command::AddHardscape {
            boundary: Some(ring),
            patch: Box::new(kadakas_core::hardscape::HardscapePatch {
                drape: Some(true),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
        let mut def = WorksheetDef::default();
        def.databases.push(kadakas_sheet::DatabaseDef {
            criteria: "Kind = \"hardscape\"".into(),
            header: false,
            total: false,
            columns: vec![kadakas_sheet::ColumnDef {
                header: "A".into(),
                expr: "ROUND(Area, 3)".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let ws = ResourceId(
            ed.execute(&Command::AddResource {
                name: "A".into(),
                body: ResourceBody::Worksheet(def.to_value()),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&Command::AddSheet {
            name: "A1".into(),
            paper: None,
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap();
        let sheet = ed.doc().sheet_layers().next().unwrap().id;
        ed.execute(&Command::AddWorksheetTable {
            worksheet: ws,
            at: DVec2::new(10.0, 100.0),
            size: DVec2::new(60.0, 10.0),
            text_height: Some(3.0),
            layer: Some(sheet),
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
        // The tables of a page follow the ground they are given.
        let flat = Tables::build(ed.doc(), &NoTerrain);
        let hill = Tables::build(ed.doc(), &Hill);
        assert_eq!(
            flat.get(ws).unwrap().value_at("A1"),
            Some(Value::Number(16.0))
        );
        let Some(Value::Number(on_hill)) = hill.get(ws).unwrap().value_at("A1") else {
            panic!()
        };
        assert!(on_hill > 16.5, "{on_hill}");
        let t_flat = texts(&compose_page(ed.doc(), sheet).unwrap().prims);
        let t_hill = texts(&compose_page_on(ed.doc(), sheet, &Hill).unwrap().prims);
        assert_eq!(t_flat, vec!["16".to_string()]);
        assert_eq!(t_hill, vec![kadakas_sheet::value::format_number(on_hill)]);
        // The page cache and the PDF take the ground too.
        let mut cache = crate::PageCache::default();
        let page = cache.page_on(ed.doc(), 1, sheet, &Hill).unwrap();
        assert_eq!(texts(&page.prims), t_hill);
        let pdf = crate::export_pdf_on(ed.doc(), 2, &mut cache, &[sheet], &Hill).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn a_table_in_a_viewport_is_scaled_and_a_missing_worksheet_draws_nothing() {
        let (mut ed, ws, sheet) = setup();
        let id = ObjectId(
            ed.execute(&Command::AddWorksheetTable {
                worksheet: ws,
                at: DVec2::new(0.0, 5000.0),
                size: DVec2::new(8000.0, 4000.0),
                text_height: Some(200.0),
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap()
            .created[0],
        );
        let layer = ed.doc().object(id).unwrap().layer;
        ed.execute(&Command::AddObject {
            shape: kadakas_core::model::Shape::Viewport(Box::new(
                kadakas_core::sheets::Viewport::new(
                    DVec2::new(10.0, 10.0),
                    DVec2::new(200.0, 140.0),
                    DVec2::new(4000.0, 2500.0),
                    50.0,
                    vec![layer],
                ),
            )),
            layer: Some(sheet),
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
        let page = compose_page(ed.doc(), sheet).unwrap();
        let heights: Vec<f64> = page
            .prims
            .iter()
            .filter_map(|p| {
                if let Prim::Text(t) = p {
                    Some(t.height)
                } else {
                    None
                }
            })
            .collect();
        assert!(!heights.is_empty());
        assert!(
            heights.iter().all(|h| (*h - 200.0 / 50.0).abs() < 1e-9),
            "text is 200 mm in the model, 4 mm on paper at 1:50: {heights:?}"
        );
        // The data of a worksheet that is gone draws only the frame.
        let obj = ed.doc().object(id).unwrap().clone();
        let mut out = vec![];
        let gone = TableData {
            worksheet: ResourceId::from_num(9999),
            text_height: 3.0,
        };
        table_prims(
            &Tables::build(ed.doc(), &NoTerrain),
            DVec2::ZERO,
            DVec2::ONE,
            &gone,
            &Xf::IDENTITY,
            [0, 0, 0, 255],
            &mut out,
        );
        assert!(out.is_empty());
        let not_a_sheet = TableData {
            worksheet: ResourceId(ed.doc().layers[0].id.0),
            text_height: 3.0,
        };
        table_prims(
            &Tables::build(ed.doc(), &NoTerrain),
            DVec2::ZERO,
            DVec2::ONE,
            &not_a_sheet,
            &Xf::IDENTITY,
            [0, 0, 0, 255],
            &mut out,
        );
        assert!(out.is_empty());
        assert!(kadakas_core::worksheet_table::data(&obj).is_some());
    }

    #[test]
    fn a_picture_cell_prints_as_an_image_in_the_pdf_and_the_svg() {
        use kadakas_core::command::Command as C;
        use kadakas_render::picture::{PictureLimits, import_picture, test_png};
        use kadakas_sheet::{ColumnDef, DatabaseDef, RowSource};
        let mut ed = Editor::default();
        let style = ResourceId(
            ed.execute(&C::AddResource {
                name: "Maple".into(),
                body: ResourceBody::plant_style(Default::default()),
            })
            .unwrap()
            .created[0],
        );
        let plain = ResourceId(
            ed.execute(&C::AddResource {
                name: "Oak".into(),
                body: ResourceBody::plant_style(Default::default()),
            })
            .unwrap()
            .created[0],
        );
        for s in [style, plain] {
            ed.execute(&C::AddPlants {
                style: Some(s),
                overrides: Default::default(),
                positions: vec![DVec2::ZERO],
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
        }
        let pic = import_picture(&test_png(60, 30), "maple.png", PictureLimits::default()).unwrap();
        ed.execute(&C::SetPlantPicture {
            style,
            picture: Some(Box::new(pic)),
        })
        .unwrap();
        let mut def = WorksheetDef::default();
        def.databases.push(DatabaseDef {
            source: RowSource::PlantList,
            columns: ["Style", "Image"]
                .map(|e| ColumnDef {
                    header: e.into(),
                    expr: e.into(),
                    ..ColumnDef::default()
                })
                .to_vec(),
            ..DatabaseDef::default()
        });
        let ws = ResourceId(
            ed.execute(&C::AddResource {
                name: "Plants".into(),
                body: ResourceBody::Worksheet(def.to_value()),
            })
            .unwrap()
            .created[0],
        );
        ed.execute(&C::AddSheet {
            name: "A1".into(),
            paper: None,
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap();
        let sheet = ed.doc().sheet_layers().next().unwrap().id;
        let l = layout(&compute(&def, ed.doc()), 3.0);
        assert!(l.has_images);
        ed.execute(&C::AddWorksheetTable {
            worksheet: ws,
            at: DVec2::new(20.0, 200.0),
            size: DVec2::new(l.width, l.height),
            text_height: Some(3.0),
            layer: Some(sheet),
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
        let page = compose_page(ed.doc(), sheet).unwrap();
        let images: Vec<&ImagePrim> = page
            .prims
            .iter()
            .filter_map(|p| {
                if let Prim::Image(i) = p {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(images.len(), 1, "one style has a picture");
        let i = images[0];
        assert_eq!((i.width, i.height), (60, 30));
        assert!(i.max.x > i.min.x && i.max.y > i.min.y);
        // The picture keeps its proportions (2 to 1) and lies inside the table.
        assert!(((i.max.x - i.min.x) / (i.max.y - i.min.y) - 2.0).abs() < 1e-6);
        assert!(
            i.min.x >= 0.0 && i.max.x <= page.size.x && i.min.y >= 0.0 && i.max.y <= page.size.y
        );
        // The picture sits in the second column, to the right of the style name.
        let maple = page
            .prims
            .iter()
            .find_map(|p| match p {
                Prim::Text(t) if t.text == "Maple" => Some(t.at),
                _ => None,
            })
            .unwrap();
        assert!(i.min.x > maple.x);
        let t = texts(&page.prims);
        assert!(t.contains(&"Maple".to_string()) && t.contains(&"Oak".to_string()));
        // PDF and SVG carry the picture.
        let pdf = crate::pdf::write_pdf(&[&page], "plants");
        assert!(
            pdf.windows(14)
                .any(|w| w == b"/Subtype/Image" || w == b"/Subtype /Imag"),
            "the PDF has an image"
        );
        let svg = crate::svg::page_svg(&page);
        assert!(svg.contains("<image"));
    }
}
