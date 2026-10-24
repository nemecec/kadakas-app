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

thread_local! {
    /// Tests that measure change the bound of the units on a sheet.
    static BOUND: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Sets the bound of the units that a viewport draws one by one on this thread (`None`: the
/// default, [`kadakas_core::hardscape::draw::PRINT_BOUND`]). For measurements.
#[doc(hidden)]
pub fn set_bound_for_measurement(bound: Option<usize>) {
    BOUND.with(|b| b.set(bound));
}

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

/// Where the note number `notes` of a part stands on the sheet: at the lower left corner of the
/// part (after the twist of the view), and always inside the frame of the viewport.
fn note_position(xf: &Xf, lo: DVec2, hi: DVec2, notes: usize) -> DVec2 {
    let corners = [
        DVec2::new(lo.x, lo.y.min(hi.y)),
        DVec2::new(hi.x, lo.y.min(hi.y)),
        DVec2::new(hi.x, hi.y.max(lo.y)),
        DVec2::new(lo.x, hi.y.max(lo.y)),
    ]
    .map(|c| xf.p(c));
    let up = 1.5 + 2.4 * notes as f64;
    let mut at = corners
        .iter()
        .fold(DVec2::splat(f64::INFINITY), |m, c| m.min(*c))
        + DVec2::new(1.0, up);
    if let Some((fmin, fmax)) = xf.frame {
        at = at
            .max(fmin + DVec2::new(1.0, up))
            .min(fmax - DVec2::new(1.0, 2.5));
    }
    at
}

/// The pattern plan of a hardscape on paper: the real units of the part in view (whole units as
/// outlines, cut units with a light fill) when the view holds at most `PRINT_BOUND` units, else
/// the gap lines of the plan and a note on the sheet. Zones and courses are in it, laid by the same
/// engine as the report.
fn pattern_prims(doc: &Document, obj: &Object, color: Option<Rgba>, xf: &Xf, out: &mut Vec<Prim>) {
    use kadakas_core::hardscape::draw::{self, Detail, DrawOptions};
    let Some(prepared) = draw::prepared(doc, obj) else {
        return;
    };
    // Notes stand one above the other at the lower left of the part of the object that is in view.
    let mut notes = 0usize;
    let mut note = |bounds: (DVec2, DVec2), text: String, out: &mut Vec<Prim>| {
        let (lo, hi) = match xf.window {
            Some((wlo, whi)) => (bounds.0.max(wlo), bounds.1.min(whi)),
            None => bounds,
        };
        let at = note_position(xf, lo, hi, notes);
        out.push(Prim::Text(TextPrim {
            at,
            text,
            height: 2.0,
            color: color.unwrap_or([0, 0, 0, 255]),
            align: Align::Left,
        }));
        notes += 1;
    };
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            // The plan cannot be laid: the reason is on the sheet.
            let (lo, hi) = doc.object_bounds(obj);
            if lo.is_finite() && hi.is_finite() {
                note((lo, hi), format!("Pattern not drawn: {e}"), out);
            }
            return;
        }
    };
    let (blo, bhi) = prepared.bounds();
    if let Some((wlo, whi)) = xf.window
        && (bhi.x < wlo.x || blo.x > whi.x || bhi.y < wlo.y || blo.y > whi.y)
    {
        return;
    }
    // Units smaller than this on paper are not drawn: they would only make a gray area.
    let min_unit_mm = if xf.k > 0.0 {
        MIN_UNIT_ON_PAPER / xf.k
    } else {
        0.0
    };
    let drawn = prepared.draw(
        xf.window,
        &DrawOptions::units(BOUND.with(|b| b.get()).unwrap_or(draw::PRINT_BOUND))
            .hiding_units_under(min_unit_mm),
    );
    if drawn.has_hidden() {
        note(
            (blo, bhi),
            format!(
                "pattern not drawn for units smaller than {MIN_UNIT_ON_PAPER} mm on paper at this scale"
            ),
            out,
        );
    }
    let hair = THIN;
    for g in &drawn.groups {
        // The units show a course; its band is drawn when the units are hidden.
        if g.hidden && !g.outline.is_empty() {
            let segs: Vec<PSeg> = g
                .outline
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
            (blo, bhi),
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
                pattern_prims(doc, obj, color, xf, out);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn xf(twist: f64) -> Xf {
        Xf {
            model_center: DVec2::new(5000.0, 5000.0),
            paper_center: DVec2::new(210.0, 150.0),
            k: 0.01,
            turn: DVec2::from_angle(f64::to_radians(twist)),
            frame: Some((DVec2::new(10.0, 10.0), DVec2::new(410.0, 290.0))),
            window: None,
        }
    }

    #[test]
    fn notes_stand_inside_the_frame_at_every_twist() {
        // A part that reaches far out of the view: its corners are outside the frame.
        let (lo, hi) = (
            DVec2::new(-50_000.0, -50_000.0),
            DVec2::new(60_000.0, 60_000.0),
        );
        for twist in [0.0, 30.0, 90.0, 180.0, 270.0] {
            for notes in 0..3 {
                let p = note_position(&xf(twist), lo, hi, notes);
                assert!(
                    p.x >= 10.0 && p.x <= 410.0 && p.y >= 10.0 && p.y <= 290.0,
                    "{twist} {notes} {p:?}"
                );
            }
        }
        // A small part in view at twist 180: its lower left corner on the sheet is the upper right one in the model.
        let p = note_position(
            &xf(180.0),
            DVec2::new(4000.0, 4000.0),
            DVec2::new(5000.0, 5000.0),
            0,
        );
        assert!((p - DVec2::new(211.0, 151.5)).length() < 1e-9, "{p:?}");
        assert!(p.x >= 210.0 && p.y >= 150.0, "{p:?}");
        // Without a frame nothing is clamped.
        let mut f = xf(0.0);
        f.frame = None;
        let q = note_position(
            &f,
            DVec2::new(4000.0, 4000.0),
            DVec2::new(5000.0, 5000.0),
            1,
        );
        assert!((q - DVec2::new(200.0 + 1.0, 140.0 + 1.5 + 2.4)).length() < 1e-9);
    }
}
