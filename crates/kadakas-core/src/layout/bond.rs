//! The four bonds, built as modules (cells on a lattice), and the long gap lines of the two bonds
//! whose gaps are straight rows.
//!
//! A cell is a unit plus half a gap on every side; the unit is the cell moved inward by half a gap
//! (see the module docs of `layout`). With `lp = unit_length + gap` and `wp = unit_width + gap`:
//!
//! - stack: one cell `lp` by `wp`, repeat vectors `(lp, 0)` and `(0, wp)`.
//! - running bond: the same cell, repeat vectors `(lp, 0)` and `(lp / 2, wp)`.
//! - herringbone: a flat cell `[0, lp] x [0, wp]` and a standing cell `[lp, lp + wp] x [wp - lp, wp]`
//!   (with lp the longer side), repeat vectors `(wp, wp)` and `(lp + wp, wp - lp)`. Two cells
//!   fill a lattice cell of `2 lp wp`, so the units keep their size for any ratio of length to width.
//! - basket weave: blocks of n units side by side, n = the length divided by the width, rounded. A
//!   block is as wide as the longer of `lp` and `n wp`. When the two differ, the difference goes into
//!   the joints around the units (the units never change their size); `widest_joint` tells how wide
//!   they get, and a difference of more than a quarter of the unit width is refused.

use glam::DVec2;

use super::module::{Compiled, Kind};
use super::{Bond, Pattern, Segment};

/// A basket weave whose joints would get wider than the gap by more than this share of the unit
/// width is refused.
pub const BASKET_SLACK: f64 = 0.25;

fn rect(lo: DVec2, hi: DVec2) -> Vec<DVec2> {
    vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
}

fn kind(lo: DVec2, hi: DVec2, gap: f64) -> Kind {
    let cell = rect(lo, hi);
    let h = DVec2::splat(gap / 2.0);
    let piece = rect(lo + h, hi - h);
    let size = hi - lo - 2.0 * h;
    Kind {
        name: "unit".into(),
        cell,
        area: size.x * size.y,
        pieces: vec![piece],
        lo,
        hi,
    }
}

/// The long and the short side of a pattern, for the bonds that are the same turned by 90 degrees.
fn long_short(p: &Pattern) -> (f64, f64) {
    (
        p.unit_length.max(p.unit_width),
        p.unit_length.min(p.unit_width),
    )
}

fn basket_dims(p: &Pattern) -> (usize, f64) {
    let (l, w) = long_short(p);
    let n = ((l / w).round() as usize).clamp(1, 16);
    let block = (l + p.gap).max(n as f64 * (w + p.gap));
    (n, block)
}

/// The widest joint of the pattern in mm.
pub fn widest_joint(p: &Pattern) -> f64 {
    if p.bond != Bond::BasketWeave {
        return p.gap;
    }
    let (l, w) = long_short(p);
    let (n, block) = basket_dims(p);
    (block - l).max(p.gap + block - n as f64 * (w + p.gap))
}

/// The cells of a bond as a lattice. Fails for a basket weave that does not fit.
pub fn compile(p: &Pattern) -> Result<Compiled, String> {
    let g = p.gap;
    let (lp, wp) = (p.unit_length + g, p.unit_width + g);
    let (kinds, a, b) = match p.bond {
        Bond::Stack => (
            vec![kind(DVec2::ZERO, DVec2::new(lp, wp), g)],
            DVec2::new(lp, 0.0),
            DVec2::new(0.0, wp),
        ),
        Bond::RunningBond => (
            vec![kind(DVec2::ZERO, DVec2::new(lp, wp), g)],
            DVec2::new(lp, 0.0),
            DVec2::new(lp / 2.0, wp),
        ),
        Bond::Herringbone => {
            let (l, w) = long_short(p);
            let (a, b) = (l + g, w + g);
            (
                vec![
                    kind(DVec2::ZERO, DVec2::new(a, b), g),
                    kind(DVec2::new(a, b - a), DVec2::new(a + b, b), g),
                ],
                DVec2::new(b, b),
                DVec2::new(a + b, b - a),
            )
        }
        Bond::BasketWeave => {
            let (l, w) = long_short(p);
            let (n, block) = basket_dims(p);
            if widest_joint(p) - g > BASKET_SLACK * w {
                return Err(format!(
                    "a basket weave of {l} by {w} mm units does not fit: {n} units of {w} mm side by side are {:.1} mm wide, one unit is {l} mm long. Use a length that is about {n} times the width",
                    n as f64 * (w + g) - g
                ));
            }
            let (cl, cw) = (l + g, w + g);
            let mut kinds = Vec::new();
            let ox = (block - cl) / 2.0;
            let oy = (block - n as f64 * cw) / 2.0;
            for i in 0..n {
                let y = oy + i as f64 * cw;
                kinds.push(kind(DVec2::new(ox, y), DVec2::new(ox + cl, y + cw), g));
            }
            for i in 0..n {
                let x = block + oy + i as f64 * cw;
                kinds.push(kind(DVec2::new(x, ox), DVec2::new(x + cw, ox + cl), g));
            }
            (
                kinds,
                DVec2::new(2.0 * block, 0.0),
                DVec2::new(block, block),
            )
        }
        Bond::Module => return Err("a module is not a bond".into()),
    };
    let det = a.perp_dot(b);
    Ok(Compiled { kinds, a, b, det })
}

/// Long gap lines of a stack or a running bond over the box `lo`..`hi` (in the frame of the
/// pattern). `None` if there would be more than `max` lines, or for another bond.
pub fn pattern_lines(p: &Pattern, lo: DVec2, hi: DVec2, max: usize) -> Option<Vec<Segment>> {
    let (lp, wp) = (p.unit_length + p.gap, p.unit_width + p.gap);
    let mut out: Vec<Segment> = Vec::new();
    let span = hi - lo;
    let max = max as f64;
    match p.bond {
        Bond::Stack => {
            let (nx, ny) = ((span.x / lp).ceil() + 1.0, (span.y / wp).ceil() + 1.0);
            if nx + ny > max {
                return None;
            }
            let (i0, j0) = ((lo.x / lp).floor(), (lo.y / wp).floor());
            for k in 0..=nx as i64 {
                let x = (i0 + k as f64) * lp;
                out.push((DVec2::new(x, lo.y), DVec2::new(x, hi.y)));
            }
            for k in 0..=ny as i64 {
                let y = (j0 + k as f64) * wp;
                out.push((DVec2::new(lo.x, y), DVec2::new(hi.x, y)));
            }
        }
        Bond::RunningBond => {
            let (nx, ny) = ((span.x / lp).ceil() + 2.0, (span.y / wp).ceil() + 1.0);
            if nx * ny + ny > max {
                return None;
            }
            let j0 = (lo.y / wp).floor() as i64;
            let i0 = (lo.x / lp).floor();
            for jj in 0..=ny as i64 {
                let j = j0 + jj;
                let y = j as f64 * wp;
                out.push((DVec2::new(lo.x, y), DVec2::new(hi.x, y)));
                let shift = if j.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
                for k in 0..=nx as i64 {
                    let x = (i0 + k as f64 + shift) * lp;
                    out.push((DVec2::new(x, y), DVec2::new(x, y + wp)));
                }
            }
        }
        _ => return None,
    }
    Some(out)
}
