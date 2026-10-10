//! The extras of hardscapes and landscape areas on paper: joint patterns, borders, holes and the
//! scattered plants. The same code draws sheets and plan viewports. All lengths on paper are in mm.

use cadektor_core::curve;
use cadektor_core::hardscape::{self, Mode, Sides};
use cadektor_core::landscape;
use cadektor_core::model::{Document, Object, Shape};
use cadektor_core::plants::graphic;
use cadektor_core::plants::style::PlantGraphic;
use glam::DVec2;

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

/// Adds the extras of a hardscape or a landscape area. `st` is the pen and fill of the object.
pub fn extras(doc: &Document, obj: &Object, st: &Style, xf: &Xf, out: &mut Vec<Prim>) {
    let map = |pts: &[DVec2]| -> Vec<DVec2> { pts.iter().map(|p| xf.p(*p)).collect() };
    match &obj.shape {
        Shape::Hardscape { boundary, .. } => {
            let Some((_, style)) = hardscape::data::style_of(doc, obj) else {
                return;
            };
            let ring = curve::flatten(boundary, true, (0.1 / xf.k).max(1.0));
            if let Some(j) = &style.joints
                && xf.len(j.unit_width.min(j.unit_length)) >= MIN_UNIT_ON_PAPER
            {
                let color = j.color.map(|c| c.0).or(st.stroke);
                for (a, b) in hardscape::joints::joint_lines(&ring, j).segments {
                    out.push(line(&[xf.p(a), xf.p(b)], false, color, None, THIN, None));
                }
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
