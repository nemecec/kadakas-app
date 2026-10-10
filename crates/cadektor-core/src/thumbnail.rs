//! Thumbnails of resources for the Resource Manager. A thumbnail is a short list of paths in a box of
//! 100 x 100 units (Y down). The app draws it with its painter, and `to_svg` makes SVG from the same list.

use std::fmt::Write;

use glam::{DAffine2, DVec2};

use crate::graphics::{GraphicResource, PatternUnits};
use crate::model::{Color, Document, Shape};
use crate::pattern;
use crate::resource::{ResourceBody, ResourceId};
use crate::symbols;

/// Side of the thumbnail box.
pub const SIZE: f64 = 100.0;
const MARGIN: f64 = 8.0;
const INK: [u8; 4] = [40, 40, 40, 255];

/// One path of a thumbnail.
#[derive(Clone, Debug, PartialEq)]
pub struct ThumbPath {
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    pub stroke: Option<[u8; 4]>,
    pub fill: Option<[u8; 4]>,
    pub width: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Thumbnail {
    pub paths: Vec<ThumbPath>,
}

impl Thumbnail {
    /// SVG text of the thumbnail, `px` pixels on a side.
    pub fn to_svg(&self, px: u32) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{px}" height="{px}" viewBox="0 0 {SIZE} {SIZE}">"#
        );
        let css = |c: Option<[u8; 4]>| {
            c.filter(|c| c[3] > 0).map_or("none".to_string(), |c| {
                format!("rgb({},{},{})", c[0], c[1], c[2])
            })
        };
        let alpha = |c: Option<[u8; 4]>| c.map_or(1.0, |c| c[3] as f64 / 255.0);
        for p in &self.paths {
            if p.points.is_empty() {
                continue;
            }
            let d: String = p
                .points
                .iter()
                .enumerate()
                .map(|(i, q)| {
                    format!(
                        "{}{:.2} {:.2}",
                        if i == 0 { "M " } else { " L " },
                        q[0],
                        q[1]
                    )
                })
                .collect();
            let _ = writeln!(
                s,
                r#"<path d="{d}{}" fill="{}" fill-opacity="{:.3}" stroke="{}" stroke-opacity="{:.3}" stroke-width="{}" stroke-linejoin="round"/>"#,
                if p.closed { " Z" } else { "" },
                css(p.fill),
                alpha(p.fill),
                css(p.stroke),
                alpha(p.stroke),
                p.width
            );
        }
        s.push_str("</svg>\n");
        s
    }
}

/// A path in model units, before it is fitted to the box.
struct Raw {
    points: Vec<DVec2>,
    closed: bool,
    stroke: Option<[u8; 4]>,
    fill: Option<[u8; 4]>,
}

fn raw(points: Vec<DVec2>, closed: bool, stroke: Option<[u8; 4]>, fill: Option<[u8; 4]>) -> Raw {
    Raw {
        points,
        closed,
        stroke,
        fill,
    }
}

fn rect_ring(lo: DVec2, hi: DVec2) -> Vec<DVec2> {
    vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
}

/// Fits the raw paths into the box. A stroke width of 1 unit; frames 1.5.
fn fit(raws: Vec<Raw>) -> Thumbnail {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in raws.iter().flat_map(|r| &r.points) {
        if p.is_finite() {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    if lo.x > hi.x {
        return Thumbnail::default();
    }
    let size = (hi - lo).max_element().max(1e-9);
    let k = (SIZE - 2.0 * MARGIN) / size;
    let off = DVec2::splat(SIZE / 2.0) - (lo + hi) / 2.0 * k;
    let map = |p: &DVec2| [p.x * k + off.x, SIZE - (p.y * k + off.y)];
    let paths = raws
        .into_iter()
        .filter(|r| r.points.iter().all(|p| p.is_finite()))
        .map(|r| ThumbPath {
            points: r.points.iter().map(map).collect(),
            closed: r.closed,
            stroke: r.stroke,
            fill: r.fill,
            width: 1.0,
        })
        .collect();
    Thumbnail { paths }
}

fn color(c: Color) -> [u8; 4] {
    c.0
}

/// Draws the plain shapes of a symbol under a transform.
fn symbol_raws(doc: &Document, symbol: ResourceId, xf: &DAffine2, out: &mut Vec<Raw>) {
    for e in symbols::expand(&doc.resources, symbol, xf) {
        let stroke = Some(e.attrs.pen.filter(|c| c.0[3] > 0).map_or(INK, color));
        let fill = e.attrs.fill.filter(|c| c.0[3] > 0).map(color);
        for (pts, closed) in symbols::shape_polylines(&e.shape) {
            out.push(raw(pts, closed, stroke, if closed { fill } else { None }));
        }
        if let Shape::Text { at, height, .. } = e.shape {
            out.push(raw(
                vec![at, at + DVec2::new(height * 2.0, 0.0)],
                false,
                stroke,
                None,
            ));
        }
    }
}

fn segs_raws(segs: &[pattern::PatternSeg], out: &mut Vec<Raw>) {
    for s in segs.iter().take(2000) {
        out.push(raw(
            vec![s.a, s.b],
            false,
            Some(s.color.map_or(INK, color)),
            None,
        ));
    }
}

/// The thumbnail of a resource. A resource that does not exist gives an empty thumbnail.
pub fn thumbnail(doc: &Document, id: ResourceId) -> Thumbnail {
    let Some(r) = doc.resources.get(id) else {
        return Thumbnail::default();
    };
    let mut raws: Vec<Raw> = Vec::new();
    match &r.body {
        ResourceBody::Graphic(g) => match g.as_ref() {
            GraphicResource::Symbol(def) => {
                symbol_raws(
                    doc,
                    id,
                    &DAffine2::from_translation(-def.insertion),
                    &mut raws,
                );
                for s in &def.solids {
                    raws.push(raw(
                        s.outline_points()
                            .into_iter()
                            .map(|p| p - def.insertion)
                            .collect(),
                        true,
                        Some(INK),
                        s.color().map(color),
                    ));
                }
            }
            GraphicResource::Hatch(h) => {
                // A square of 30 mm on paper, or ten spacings in the model.
                let side = if h.units == PatternUnits::Paper {
                    30.0
                } else {
                    h.lines.iter().map(|l| l.spacing).fold(1.0, f64::max) * 10.0
                };
                let ring = rect_ring(DVec2::ZERO, DVec2::splat(side));
                segs_raws(
                    &pattern::hatch_segments(h, std::slice::from_ref(&ring), 1.0),
                    &mut raws,
                );
                raws.push(raw(ring, true, Some(INK), None));
            }
            GraphicResource::Tile(t) => {
                let side = t.width.max(t.height) * 3.0;
                let ring = rect_ring(DVec2::ZERO, DVec2::splat(side));
                segs_raws(
                    &pattern::tile_segments(&doc.resources, t, std::slice::from_ref(&ring), 1.0),
                    &mut raws,
                );
                raws.push(raw(ring, true, Some(INK), None));
            }
            GraphicResource::LineType(l) => {
                let len = (l.period() * 6.0).max(1.0);
                let g = pattern::line_type_geometry(
                    &doc.resources,
                    l,
                    &[DVec2::new(0.0, 0.0), DVec2::new(len, 0.0)],
                    false,
                    1.0,
                );
                for st in g.strokes {
                    raws.push(raw(st, false, Some(INK), None));
                }
                for m in g.marks {
                    if let Some(def) = doc.resources.symbol(m.symbol) {
                        let xf = DAffine2::from_translation(m.at)
                            * DAffine2::from_angle(m.angle)
                            * DAffine2::from_scale(DVec2::splat(m.scale))
                            * DAffine2::from_translation(-def.insertion);
                        symbol_raws(doc, m.symbol, &xf, &mut raws);
                    }
                }
                // Keep the line from being drawn as a dot when it is short next to its marks.
                raws.push(raw(
                    vec![DVec2::new(0.0, -len / 6.0), DVec2::new(len, len / 6.0)],
                    false,
                    None,
                    None,
                ));
            }
            GraphicResource::Texture(t) => {
                raws.push(raw(
                    rect_ring(DVec2::ZERO, DVec2::splat(1.0)),
                    true,
                    Some(INK),
                    Some(color(t.color)),
                ));
            }
        },
        ResourceBody::PlantStyle(p) => {
            let at = DVec2::ZERO;
            if let Some(sym) = p.plan_symbol.filter(|s| doc.resources.symbol(*s).is_some()) {
                let def = doc.resources.symbol(sym).expect("checked");
                let b = symbols::expand(
                    &doc.resources,
                    sym,
                    &DAffine2::from_translation(-def.insertion),
                )
                .iter()
                .map(|e| e.shape.bounds())
                .fold(
                    (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                    |a, b| (a.0.min(b.0), a.1.max(b.1)),
                );
                let size = if b.0.is_finite() {
                    (b.1 - b.0).max_element().max(1e-9)
                } else {
                    1.0
                };
                symbol_raws(
                    doc,
                    sym,
                    &(DAffine2::from_scale(DVec2::splat(1.0 / size))
                        * DAffine2::from_translation(-def.insertion)),
                    &mut raws,
                );
            } else {
                let circle: Vec<DVec2> = (0..40)
                    .map(|k| at + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 40.0) * 0.5)
                    .collect();
                raws.push(raw(
                    circle,
                    true,
                    Some(p.pen.map_or(INK, color)),
                    Some(p.fill.map_or([90, 160, 90, 110], |c| {
                        [c.0[0], c.0[1], c.0[2], c.0[3].min(160)]
                    })),
                ));
                raws.push(raw(
                    vec![DVec2::new(-0.08, 0.0), DVec2::new(0.08, 0.0)],
                    false,
                    Some(INK),
                    None,
                ));
                raws.push(raw(
                    vec![DVec2::new(0.0, -0.08), DVec2::new(0.0, 0.08)],
                    false,
                    Some(INK),
                    None,
                ));
            }
        }
        ResourceBody::HardscapeStyle(s) => {
            raws.push(raw(
                rect_ring(DVec2::ZERO, DVec2::new(1.0, 0.7)),
                true,
                Some(s.pen.map_or(INK, color)),
                Some(s.fill.map_or([175, 150, 115, 255], color)),
            ));
            for k in 1..4 {
                let y = 0.7 * k as f64 / 4.0;
                raws.push(raw(
                    vec![DVec2::new(0.0, y), DVec2::new(1.0, y)],
                    false,
                    Some([90, 70, 50, 160]),
                    None,
                ));
            }
        }
        ResourceBody::LandscapeAreaStyle(s) => {
            raws.push(raw(
                rect_ring(DVec2::ZERO, DVec2::new(1.0, 0.7)),
                true,
                Some(s.pen.map_or(INK, color)),
                Some(s.fill.map_or([80, 170, 80, 120], color)),
            ));
            for (x, y) in [
                (0.25, 0.2),
                (0.7, 0.25),
                (0.45, 0.5),
                (0.8, 0.55),
                (0.2, 0.55),
            ] {
                let c = DVec2::new(x, y);
                raws.push(raw(
                    (0..12)
                        .map(|k| {
                            c + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 12.0) * 0.07
                        })
                        .collect(),
                    true,
                    Some(INK),
                    Some([40, 120, 40, 200]),
                ));
            }
        }
        _ => {
            // A page with lines of text, for data resources.
            raws.push(raw(
                rect_ring(DVec2::ZERO, DVec2::new(0.75, 1.0)),
                true,
                Some(INK),
                Some([245, 245, 245, 255]),
            ));
            for k in 1..5 {
                let y = 0.18 * k as f64;
                raws.push(raw(
                    vec![DVec2::new(0.12, y), DVec2::new(0.63, y)],
                    false,
                    Some([120, 120, 120, 255]),
                    None,
                ));
            }
        }
    }
    fit(raws)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::graphics::{
        HatchDef, HatchLine, LineElement, LineTypeDef, SymbolDef, SymbolItem, TextureDef, TileDef,
    };
    use crate::model::Attributes;
    use crate::plants::style::PlantStyle;

    fn add(ed: &mut Editor, name: &str, body: ResourceBody) -> ResourceId {
        ResourceId(
            ed.execute(&Command::AddResource {
                name: name.into(),
                body,
            })
            .unwrap()
            .created[0],
        )
    }

    fn g(x: GraphicResource) -> ResourceBody {
        ResourceBody::Graphic(Box::new(x))
    }

    fn in_box(t: &Thumbnail) -> bool {
        t.paths
            .iter()
            .flat_map(|p| &p.points)
            .all(|q| (0.0..=SIZE).contains(&q[0]) && (0.0..=SIZE).contains(&q[1]))
    }

    #[test]
    fn every_kind_has_a_thumbnail_in_the_box() {
        let mut ed = Editor::default();
        let circle = Shape::Circle {
            center: DVec2::new(500.0, 500.0),
            radius: 200.0,
        };
        let sym = add(
            &mut ed,
            "Dot",
            g(GraphicResource::Symbol(SymbolDef {
                insertion: DVec2::new(500.0, 500.0),
                items: vec![
                    SymbolItem {
                        shape: circle,
                        attrs: Attributes {
                            fill: Some(Color([200, 0, 0, 255])),
                            pen: Some(Color([0, 0, 200, 255])),
                            ..Default::default()
                        },
                        class: None,
                    },
                    SymbolItem {
                        shape: Shape::Text {
                            at: DVec2::new(500.0, 500.0),
                            text: "x".into(),
                            height: 50.0,
                        },
                        attrs: Attributes::default(),
                        class: None,
                    },
                ],
                solids: vec![crate::graphics::Solid3d::Cylinder {
                    center: DVec2::new(500.0, 500.0),
                    radius: 100.0,
                    base: 0.0,
                    height: 300.0,
                    color: None,
                }],
                ..Default::default()
            })),
        );
        let hatch = add(
            &mut ed,
            "Diag",
            g(GraphicResource::Hatch(HatchDef {
                units: PatternUnits::Paper,
                lines: vec![HatchLine::solid(45.0, 3.0)],
            })),
        );
        let world = add(
            &mut ed,
            "WorldHatch",
            g(GraphicResource::Hatch(HatchDef {
                units: PatternUnits::World,
                lines: vec![HatchLine::solid(0.0, 50.0)],
            })),
        );
        let tile = add(
            &mut ed,
            "Dots",
            g(GraphicResource::Tile(TileDef {
                symbol: sym,
                width: 600.0,
                height: 600.0,
                row_shift: 300.0,
                angle: 0.0,
                scale: 1.0,
                units: PatternUnits::World,
            })),
        );
        let lt = add(
            &mut ed,
            "Fence",
            g(GraphicResource::LineType(LineTypeDef {
                units: PatternUnits::Paper,
                elements: vec![
                    LineElement::Dash { length: 4.0 },
                    LineElement::Mark {
                        symbol: sym,
                        scale: 0.01,
                        rotate: true,
                    },
                    LineElement::Gap { length: 2.0 },
                ],
            })),
        );
        let tex = add(
            &mut ed,
            "Oak",
            g(GraphicResource::Texture(TextureDef {
                color: Color([120, 80, 40, 255]),
                size: DVec2::splat(500.0),
                reflectivity: 0.1,
                transparency: 0.0,
                image: None,
            })),
        );
        let plant = add(
            &mut ed,
            "Oak tree",
            ResourceBody::plant_style(PlantStyle::default()),
        );
        let plant_sym = add(
            &mut ed,
            "Oak 2",
            ResourceBody::plant_style(PlantStyle {
                plan_symbol: Some(sym),
                ..PlantStyle::default()
            }),
        );
        let hs = add(
            &mut ed,
            "Setts",
            ResourceBody::HardscapeStyle(Default::default()),
        );
        let la = add(
            &mut ed,
            "Meadow",
            ResourceBody::LandscapeAreaStyle(Default::default()),
        );
        let ws = add(
            &mut ed,
            "Sheet",
            ResourceBody::Worksheet(serde_json::json!({"type": "worksheet"})),
        );
        for id in [
            sym, hatch, world, tile, lt, tex, plant, plant_sym, hs, la, ws,
        ] {
            let t = thumbnail(ed.doc(), id);
            assert!(!t.paths.is_empty(), "resource {id} has no thumbnail");
            assert!(in_box(&t), "resource {id} leaves the box");
            let svg = t.to_svg(48);
            assert!(
                svg.starts_with("<svg")
                    && svg.contains("viewBox=\"0 0 100 100\"")
                    && svg.contains("<path"),
                "{svg}"
            );
        }
        // The hatch thumbnail has many lines, the texture has its color.
        assert!(thumbnail(ed.doc(), hatch).paths.len() > 5);
        assert_eq!(
            thumbnail(ed.doc(), tex).paths[0].fill,
            Some([120, 80, 40, 255])
        );
        assert!(thumbnail(ed.doc(), ResourceId(9999)).paths.is_empty());
        assert_eq!(Thumbnail::default().to_svg(10).matches("<path").count(), 0);
    }

    #[test]
    fn fit_keeps_the_aspect_and_flips_y() {
        let t = fit(vec![raw(
            vec![
                DVec2::new(0.0, 0.0),
                DVec2::new(10.0, 0.0),
                DVec2::new(10.0, 5.0),
            ],
            false,
            Some(INK),
            None,
        )]);
        let p = &t.paths[0].points;
        // The width fills the box (minus the margin). The point at y = 5 is above the one at y = 0, so its Y is smaller.
        assert!((p[1][0] - p[0][0] - (SIZE - 2.0 * MARGIN)).abs() < 1e-9);
        assert!(p[2][1] < p[1][1]);
        assert!(((p[1][1] - p[2][1]) - (SIZE - 2.0 * MARGIN) / 2.0).abs() < 1e-9);
        assert!(fit(vec![]).paths.is_empty());
        assert!(
            fit(vec![raw(
                vec![DVec2::new(f64::NAN, 0.0)],
                false,
                None,
                None
            )])
            .paths
            .is_empty()
        );
        // A single point does not divide by zero.
        assert!(in_box(&fit(vec![raw(
            vec![DVec2::new(3.0, 3.0)],
            true,
            None,
            None
        )])));
    }

    #[test]
    fn svg_skips_empty_paths_and_hides_transparent_colors() {
        let t = Thumbnail {
            paths: vec![
                ThumbPath {
                    points: vec![],
                    closed: false,
                    stroke: None,
                    fill: None,
                    width: 1.0,
                },
                ThumbPath {
                    points: vec![[1.0, 2.0], [3.0, 4.0]],
                    closed: true,
                    stroke: Some([0, 0, 0, 255]),
                    fill: Some([9, 9, 9, 0]),
                    width: 1.0,
                },
            ],
        };
        let svg = t.to_svg(20);
        assert_eq!(svg.matches("<path").count(), 1);
        assert!(svg.contains(r#"fill="none""#) && svg.contains(" Z"));
    }
}
