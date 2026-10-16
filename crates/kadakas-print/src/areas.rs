//! The extras of hardscapes and landscape areas on paper: joint patterns, borders, holes and the
//! scattered plants. The same code draws sheets and plan viewports. All lengths on paper are in mm.

use glam::DVec2;
use kadakas_core::curve;
use kadakas_core::hardscape::{self, Mode, Sides};
use kadakas_core::landscape;
use kadakas_core::model::{Document, Object, Shape};
use kadakas_core::plants::graphic;
use kadakas_core::plants::style::PlantGraphic;

use crate::display::*;
use crate::plan::{Style, Xf};

/// Joint units smaller than this on paper (mm) are not drawn: they would only make a gray area.
const MIN_UNIT_ON_PAPER: f64 = 0.6;
/// Thin lines of the extras, in mm on paper.
const THIN: f64 = 0.1;

fn line(
    points: &[DVec2],
    closed: bool,
    stroke: Option<Rgba>,
    fill: Option<Rgba>,
    width: f64,
    dash: Option<[f64; 2]>,
) -> Prim {
    Prim::Path(PathPrim {
        segs: PathPrim::polyline(points),
        closed,
        stroke,
        fill,
        width,
        dash,
    })
}

fn circle(
    c: DVec2,
    r: f64,
    stroke: Option<Rgba>,
    fill: Option<Rgba>,
    width: f64,
    dash: Option<[f64; 2]>,
) -> Prim {
    let pts: Vec<DVec2> = (0..32)
        .map(|k| c + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 32.0) * r)
        .collect();
    line(&pts, true, stroke, fill, width, dash)
}

/// The color of the fill that marks a cut unit: the pen of the pattern at 18 percent.
fn cut_fill(color: Option<Rgba>) -> Rgba {
    let [r, g, b, _] = color.unwrap_or([0, 0, 0, 255]);
    [r, g, b, 46]
}

/// A ring as segments, closed by a line to its first point, and turned the way a fill needs it
/// (outline counterclockwise, hole clockwise) so that the holes of a unit stay open.
fn ring_segs(ring: &[DVec2], hole: bool, xf: &Xf) -> Vec<PSeg> {
    let mut pts: Vec<DVec2> = ring.iter().map(|p| xf.p(*p)).collect();
    // The paper has y up, as the model, so the turn of a ring is the same on paper.
    let ccw = kadakas_core::geom::signed_area(&pts) >= 0.0;
    if ccw == hole {
        pts.reverse();
    }
    let mut segs = PathPrim::polyline(&pts);
    if let Some(first) = pts.first() {
        segs.push(PSeg::Line(*first));
    }
    segs
}

/// The pattern plan of a hardscape on paper: the real units of the part in view (whole units as
/// outlines, cut units with a light fill) when the view holds at most `PRINT_BOUND` units, else
/// the gap lines of the plan and a note on the sheet. Zones and courses are in it, laid by the same
/// engine as the report.
fn pattern_prims(
    doc: &Document,
    obj: &Object,
    j: &hardscape::JointPattern,
    color: Option<Rgba>,
    xf: &Xf,
    out: &mut Vec<Prim>,
) {
    use kadakas_core::hardscape::draw::{self, Detail, DrawOptions};
    let Some(Ok(prepared)) = draw::prepared(doc, obj) else {
        return;
    };
    let (blo, bhi) = prepared.bounds();
    if let Some((wlo, whi)) = xf.window
        && (bhi.x < wlo.x || blo.x > whi.x || bhi.y < wlo.y || blo.y > whi.y)
    {
        return;
    }
    let note = |text: String, out: &mut Vec<Prim>| {
        let (lo, hi) = match xf.window {
            Some((wlo, whi)) => (blo.max(wlo), bhi.min(whi)),
            None => (blo, bhi),
        };
        let at = xf.p(DVec2::new(lo.x, lo.y.min(hi.y)));
        out.push(Prim::Text(TextPrim {
            at: at + DVec2::new(1.0, 1.5),
            text,
            height: 2.0,
            color: color.unwrap_or([0, 0, 0, 255]),
            align: Align::Left,
        }));
    };
    if xf.len(j.unit_width.min(j.unit_length)) < MIN_UNIT_ON_PAPER
        && j.kind != hardscape::JointKind::Module
    {
        note(
            format!(
                "pattern not drawn: units are smaller than {MIN_UNIT_ON_PAPER} mm on paper at this scale"
            ),
            out,
        );
        return;
    }
    let drawn = prepared.draw(xf.window, &DrawOptions::units(draw::PRINT_BOUND));
    let hair = THIN;
    for g in &drawn.groups {
        if !g.joints.is_empty() {
            let mut segs = Vec::with_capacity(g.joints.len() * 2);
            for (a, b) in &g.joints {
                segs.push(PSeg::Move(xf.p(*a)));
                segs.push(PSeg::Line(xf.p(*b)));
            }
            out.push(Prim::Path(PathPrim {
                segs,
                closed: false,
                stroke: color,
                fill: None,
                width: hair,
                dash: None,
            }));
        }
        if !g.whole.is_empty() {
            let segs: Vec<PSeg> = g
                .whole
                .iter()
                .flat_map(|r| ring_segs(r, false, xf))
                .collect();
            out.push(Prim::Path(PathPrim {
                segs,
                closed: false,
                stroke: color,
                fill: None,
                width: hair,
                dash: None,
            }));
        }
        for u in &g.cut {
            let segs: Vec<PSeg> = u
                .iter()
                .enumerate()
                .flat_map(|(i, r)| ring_segs(r, i > 0, xf))
                .collect();
            out.push(Prim::Path(PathPrim {
                segs,
                closed: false,
                stroke: color,
                fill: Some(cut_fill(color)),
                width: hair,
                dash: None,
            }));
        }
    }
    for ring in &drawn.zone_edges {
        out.push(line(
            &ring.iter().map(|p| xf.p(*p)).collect::<Vec<_>>(),
            true,
            color,
            None,
            hair,
            Some([2.0, 1.0]),
        ));
    }
    if let Detail::Preview {
        units_in_view,
        bound,
        ..
    } = drawn.detail
    {
        note(
            format!(
                "{} ({units_in_view} units in view, more than {bound}): enlarge the scale or the viewport for the real units",
                draw::PREVIEW_LABEL
            ),
            out,
        );
    }
}

/// Adds the extras of a hardscape or a landscape area. `st` is the pen and fill of the object.
pub fn extras(doc: &Document, obj: &Object, st: &Style, xf: &Xf, out: &mut Vec<Prim>) {
    let map = |pts: &[DVec2]| -> Vec<DVec2> { pts.iter().map(|p| xf.p(*p)).collect() };
    match &obj.shape {
        Shape::Hardscape { boundary, .. } => {
            let Some((_, style)) = hardscape::data::style_of(doc, obj) else {
                return;
            };
            let ring = curve::flatten(boundary, true, (0.1 / xf.k).max(1.0));
            if let Some(j) = &style.joints {
                let color = j.color.map(|c| c.0).or(st.stroke);
                pattern_prims(doc, obj, j, color, xf, out);
            }
            if let Some(b) = &style.border {
                let color = b.color.map(|c| c.0).or(st.stroke);
                let width = xf.len(b.width).clamp(THIN, 2.0);
                let d = hardscape::data(obj).unwrap_or_default();
                if d.mode == Mode::Path && !d.closed {
                    let (left, right) = hardscape::offset::sides(&d.axis(), d.width);
                    let sides: Vec<Vec<DVec2>> = match b.sides {
                        Sides::Both => vec![left, right],
                        Sides::Left => vec![left],
                        Sides::Right => vec![right],
                    };
                    for s in sides.iter().filter(|s| s.len() >= 2) {
                        out.push(line(&map(s), false, color, None, width, None));
                    }
                } else {
                    out.push(line(&map(&ring), true, color, None, width, None));
                }
            }
        }
        Shape::LandscapeArea { .. } => {
            let Some(d) = landscape::data(obj) else {
                return;
            };
            for hole in &d.holes {
                let ring = curve::flatten(hole, true, (0.1 / xf.k).max(1.0));
                out.push(line(
                    &map(&ring),
                    true,
                    st.stroke,
                    Some([255, 255, 255, 230]),
                    THIN,
                    Some([2.0, 1.0]),
                ));
            }
            if !landscape::shows_plants(doc, obj) {
                return;
            }
            let detailed = landscape::scatter(doc, obj);
            let few = detailed.len() <= 300;
            for p in &detailed {
                let Some(s) = doc.resources.plant_style(p.plant) else {
                    continue;
                };
                let fill = s
                    .fill
                    .map(|c| c.0)
                    .map(|[r, g, b, a]| [r, g, b, (a as f64 * 0.35) as u8])
                    .or(Some([90, 150, 80, 90]));
                let pen = s.pen.map(|c| c.0).or(st.stroke);
                if few && s.graphic != PlantGraphic::Circle {
                    for (i, gp) in graphic::paths(s.graphic, p.at, s.spread / 2.0)
                        .iter()
                        .enumerate()
                    {
                        out.push(line(
                            &map(&gp.points),
                            gp.closed,
                            pen,
                            if i == 0 { fill } else { None },
                            THIN,
                            None,
                        ));
                    }
                } else {
                    out.push(circle(
                        xf.p(p.at),
                        xf.len(s.spread / 2.0),
                        pen,
                        fill,
                        THIN,
                        None,
                    ));
                }
                if let Some(zone) = s.protection_radius() {
                    out.push(circle(
                        xf.p(p.at),
                        xf.len(zone),
                        pen,
                        None,
                        THIN,
                        Some([2.0, 1.0]),
                    ));
                }
            }
        }
        _ => {}
    }
}
