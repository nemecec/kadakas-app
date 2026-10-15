//! The unit grids of the four bonds: the cells that units sit in, and the gap lines between them.
//!
//! Everything here works in the frame of the pattern: the origin of the lattice is (0, 0) and the
//! rows run along +X. A cell is the unit plus half a gap on every side.

use std::collections::BTreeSet;

use glam::DVec2;

use super::{Bond, Pattern, Segment};

/// Most grid cells that a herringbone gap pattern covers.
const MAX_CELLS: f64 = 80_000.0;

/// A cell of the lattice: the unit and half a gap around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub lo: DVec2,
    pub hi: DVec2,
}

/// The size of the herringbone cell and the number of cells in the long side of a unit.
fn herringbone_dims(p: &Pattern) -> (f64, i64) {
    let cell = p.unit_width + p.gap;
    let k = ((p.unit_length / p.unit_width).round() as i64).max(2);
    (cell, k)
}

/// The area of a full unit of the pattern, in square mm.
pub fn full_unit_area(p: &Pattern) -> f64 {
    let (lp, wp) = (p.unit_length + p.gap, p.unit_width + p.gap);
    let (w, h) = match p.bond {
        Bond::Stack | Bond::RunningBond => (lp, wp),
        Bond::Herringbone => {
            let (cell, k) = herringbone_dims(p);
            (cell * k as f64, cell)
        }
        Bond::BasketWeave => {
            let block = lp.max(2.0 * wp);
            (block, block / 2.0)
        }
    };
    ((w - p.gap) * (h - p.gap)).max(0.0)
}

/// How many loop steps `for_each_cell` makes over the box. Used to refuse a job before it starts.
pub fn steps_in(p: &Pattern, lo: DVec2, hi: DVec2) -> f64 {
    let span = hi - lo;
    let (lp, wp) = (p.unit_length + p.gap, p.unit_width + p.gap);
    match p.bond {
        Bond::Stack => (span.x / lp + 2.0) * (span.y / wp + 2.0),
        Bond::RunningBond => (span.x / lp + 3.0) * (span.y / wp + 2.0),
        Bond::Herringbone => {
            let (cell, k) = herringbone_dims(p);
            (span.x / cell + 2.0 * k as f64 + 3.0) * (span.y / cell + 2.0 * k as f64 + 3.0)
        }
        Bond::BasketWeave => {
            let block = lp.max(2.0 * wp);
            (span.x / block + 3.0) * (span.y / block + 3.0)
        }
    }
}

fn touches(c: &Cell, lo: DVec2, hi: DVec2) -> bool {
    c.hi.x >= lo.x && c.lo.x <= hi.x && c.hi.y >= lo.y && c.lo.y <= hi.y
}

/// Calls `f` for every cell that touches the box `lo`..`hi`, in rows from bottom to top.
pub fn for_each_cell(p: &Pattern, lo: DVec2, hi: DVec2, f: &mut dyn FnMut(Cell)) {
    let (lp, wp) = (p.unit_length + p.gap, p.unit_width + p.gap);
    match p.bond {
        Bond::Stack => {
            let (i0, i1) = ((lo.x / lp).floor() as i64, (hi.x / lp).floor() as i64);
            let (j0, j1) = ((lo.y / wp).floor() as i64, (hi.y / wp).floor() as i64);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    f(Cell {
                        lo: DVec2::new(i as f64 * lp, j as f64 * wp),
                        hi: DVec2::new((i + 1) as f64 * lp, (j + 1) as f64 * wp),
                    });
                }
            }
        }
        Bond::RunningBond => {
            let (j0, j1) = ((lo.y / wp).floor() as i64, (hi.y / wp).floor() as i64);
            for j in j0..=j1 {
                let shift = if j.rem_euclid(2) == 1 { 0.5 } else { 0.0 };
                let (i0, i1) = (
                    (lo.x / lp - shift).floor() as i64,
                    (hi.x / lp - shift).floor() as i64,
                );
                for i in i0..=i1 {
                    let x = (i as f64 + shift) * lp;
                    f(Cell {
                        lo: DVec2::new(x, j as f64 * wp),
                        hi: DVec2::new(x + lp, (j + 1) as f64 * wp),
                    });
                }
            }
        }
        Bond::Herringbone => {
            let (cell, k) = herringbone_dims(p);
            let a0 = (lo.x / cell).floor() as i64 - k;
            let a1 = (hi.x / cell).floor() as i64 + 1;
            let b0 = (lo.y / cell).floor() as i64 - k;
            let b1 = (hi.y / cell).floor() as i64 + k;
            for b in b0..=b1 {
                for a in a0..=a1 {
                    let c = match (a - b).rem_euclid(2 * k) {
                        0 => Cell {
                            lo: DVec2::new(a as f64, b as f64) * cell,
                            hi: DVec2::new((a + k) as f64, (b + 1) as f64) * cell,
                        },
                        m if m == k => Cell {
                            lo: DVec2::new(a as f64, (b - k + 1) as f64) * cell,
                            hi: DVec2::new((a + 1) as f64, (b + 1) as f64) * cell,
                        },
                        _ => continue,
                    };
                    if touches(&c, lo, hi) {
                        f(c);
                    }
                }
            }
        }
        Bond::BasketWeave => {
            let block = lp.max(2.0 * wp);
            let (i0, i1) = ((lo.x / block).floor() as i64, (hi.x / block).floor() as i64);
            let (j0, j1) = ((lo.y / block).floor() as i64, (hi.y / block).floor() as i64);
            let half = block / 2.0;
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let (x, y) = (i as f64 * block, j as f64 * block);
                    if (i + j).rem_euclid(2) == 0 {
                        for s in 0..2 {
                            f(Cell {
                                lo: DVec2::new(x, y + s as f64 * half),
                                hi: DVec2::new(x + block, y + (s + 1) as f64 * half),
                            });
                        }
                    } else {
                        for s in 0..2 {
                            f(Cell {
                                lo: DVec2::new(x + s as f64 * half, y),
                                hi: DVec2::new(x + (s + 1) as f64 * half, y + block),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Merges unit edges on a grid (integer coordinates) into long lines.
fn merge_grid_edges(edges: BTreeSet<(bool, i64, i64)>) -> Vec<((i64, i64), (i64, i64))> {
    // An edge is (vertical, x, y): from (x, y) to (x + 1, y) or to (x, y + 1).
    // Horizontal edges are grouped by y and run along x. Vertical edges are grouped by x and run along y.
    let mut horizontals: Vec<(i64, i64)> = Vec::new();
    let mut verticals: Vec<(i64, i64)> = Vec::new();
    for (vertical, x, y) in edges {
        if vertical {
            verticals.push((x, y))
        } else {
            horizontals.push((y, x))
        }
    }
    horizontals.sort_unstable();
    verticals.sort_unstable();
    let mut lines = Vec::new();
    for (list, vertical) in [(horizontals, false), (verticals, true)] {
        let mut i = 0;
        while i < list.len() {
            let (fixed, start) = list[i];
            let mut end = start + 1;
            i += 1;
            while i < list.len() && list[i] == (fixed, end) {
                end += 1;
                i += 1;
            }
            lines.push(if vertical {
                ((fixed, start), (fixed, end))
            } else {
                ((start, fixed), (end, fixed))
            });
        }
    }
    lines
}

fn rect_edges(set: &mut BTreeSet<(bool, i64, i64)>, x0: i64, y0: i64, x1: i64, y1: i64) {
    for x in x0..x1 {
        set.insert((false, x, y0));
        set.insert((false, x, y1));
    }
    for y in y0..y1 {
        set.insert((true, x0, y));
        set.insert((true, x1, y));
    }
}

/// Gap lines of the pattern in its own frame, over the box `lo`..`hi`. `None` if there would be
/// more than `max` lines.
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
        Bond::Herringbone => {
            let (cell, k) = herringbone_dims(p);
            let (nx, ny) = (
                (span.x / cell).ceil() as i64 + 2 * k + 2,
                (span.y / cell).ceil() as i64 + 2 * k + 2,
            );
            if nx as f64 * ny as f64 > MAX_CELLS.max(max * 2.0) {
                return None;
            }
            let (a0, b0) = (
                (lo.x / cell).floor() as i64 - k,
                (lo.y / cell).floor() as i64 - k,
            );
            let mut set = BTreeSet::new();
            for a in a0..a0 + nx {
                for b in b0..b0 + ny {
                    match (a - b).rem_euclid(2 * k) {
                        0 => rect_edges(&mut set, a, b, a + k, b + 1),
                        m if m == k => rect_edges(&mut set, a, b - k + 1, a + 1, b + 1),
                        _ => {}
                    }
                }
            }
            for ((x0, y0), (x1, y1)) in merge_grid_edges(set) {
                out.push((
                    DVec2::new(x0 as f64, y0 as f64) * cell,
                    DVec2::new(x1 as f64, y1 as f64) * cell,
                ));
            }
        }
        Bond::BasketWeave => {
            let block = lp.max(2.0 * wp);
            let (nx, ny) = ((span.x / block).ceil() + 2.0, (span.y / block).ceil() + 2.0);
            if nx * ny + nx + ny > max {
                return None;
            }
            let (i0, j0) = ((lo.x / block).floor() as i64, (lo.y / block).floor() as i64);
            for ii in 0..=nx as i64 {
                let x = (i0 + ii) as f64 * block;
                out.push((DVec2::new(x, lo.y), DVec2::new(x, hi.y)));
            }
            for jj in 0..=ny as i64 {
                let y = (j0 + jj) as f64 * block;
                out.push((DVec2::new(lo.x, y), DVec2::new(hi.x, y)));
            }
            for ii in 0..nx as i64 {
                for jj in 0..ny as i64 {
                    let (i, j) = (i0 + ii, j0 + jj);
                    let (x, y) = (i as f64 * block, j as f64 * block);
                    if (i + j).rem_euclid(2) == 0 {
                        out.push((
                            DVec2::new(x, y + block / 2.0),
                            DVec2::new(x + block, y + block / 2.0),
                        ));
                    } else {
                        out.push((
                            DVec2::new(x + block / 2.0, y),
                            DVec2::new(x + block / 2.0, y + block),
                        ));
                    }
                }
            }
        }
    }
    Some(out)
}
