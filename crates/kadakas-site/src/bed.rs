//! The surface cut of texture beds (3D-05). The triangles of the 3D terrain mesh are cut along
//! the outlines of the beds. Each piece goes to the frontmost bed that contains it, or else to
//! the base (the look of the site model). The beds only separate the triangles: heights do not
//! change, so a cut point has the height of the surface at that place.
//!
//! How the cut works. A triangle is a convex piece. For every bed (from the back), every edge of
//! the outline that crosses a piece cuts it along the line of the edge into two convex pieces.
//! An edge cuts a piece at most once, so the work ends. A piece that no edge crosses lies wholly
//! inside or wholly outside the bed; the middle of the piece tells which (even-odd rule). A piece
//! carries one bit per bed; the highest bit is the frontmost bed. So the pieces of a triangle
//! cover it exactly once and the area is kept.
//!
//! A cut point on an edge of the source mesh is computed from the two source vertices, so the two
//! triangles that share the edge get the same point and share the vertex (no cracks, smooth
//! shading).
//!
//! The work is bounded. A grid per bed finds the outline edges near a triangle, and a triangle
//! far from all outlines is classified at once. The whole work has a budget of steps
//! ([`MAX_WORK`]), and the output is capped at [`MAX_CUT_TRIANGLES`]. When a bound is reached,
//! the remaining triangles go whole to the base and `truncated` is set.

use std::collections::HashMap;

use glam::{DVec2, DVec3};

/// The most triangles that the cut may output for one mesh.
pub const MAX_CUT_TRIANGLES: usize = 2_000_000;
/// The most steps (tests of an edge against a piece or a point) of one cut.
pub const MAX_WORK: u64 = 200_000_000;
/// The most pieces that one source triangle may be cut into.
const MAX_PIECES_PER_TRIANGLE: usize = 4096;
/// Pieces with an area (mm squared) below this are dropped.
const MIN_AREA: f64 = 1e-9;
/// A distance (mm) below this is "on the line".
const ON_LINE: f64 = 1e-7;

/// The triangles of one part of the cut: their own vertices, and indices into them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartMesh {
    pub vertices: Vec<DVec3>,
    pub indices: Vec<u32>,
}

impl PartMesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// The area of the triangles seen from above, in mm squared.
    pub fn area(&self) -> f64 {
        self.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.vertices[i as usize].truncate());
                ((b - a).perp_dot(c - a)).abs() / 2.0
            })
            .sum()
    }
}

/// The result of the cut.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BedCut {
    /// The triangles outside every bed.
    pub base: PartMesh,
    /// One mesh per outline, in the order of the outlines (empty if no triangle is inside).
    pub beds: Vec<PartMesh>,
    /// True if a bound stopped the cutting and some triangles went whole to the base.
    pub truncated: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Key {
    Source(u32),
    /// A cut of the edge between two source vertices (ordered) by the line of an outline edge.
    Cut(u32, u32, u32),
    /// A point inside a triangle, by its position: pieces that meet there share it.
    At([u64; 3]),
}

/// A corner of a piece, and whether the edge to the next corner lies on an edge of the source
/// mesh (the ordered pair of its source vertices).
#[derive(Clone, Copy, Debug)]
struct Corner {
    key: Key,
    p: DVec3,
    edge: Option<(u32, u32)>,
}

type Piece = Vec<Corner>;

/// A bed outline with a grid of its edges.
struct Outline<'a> {
    pts: &'a [DVec2],
    lo: DVec2,
    hi: DVec2,
    cells: usize,
    cell: DVec2,
    grid: Vec<Vec<u32>>,
    /// Is the middle of an empty cell inside? Worked out when first needed.
    inside: Vec<Option<bool>>,
}

impl<'a> Outline<'a> {
    fn new(pts: &'a [DVec2]) -> Outline<'a> {
        let (lo, hi) = pts.iter().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let n = pts.len();
        let cells = ((n as f64).sqrt() / 2.0).ceil().clamp(1.0, 32.0) as usize;
        let cell = ((hi - lo) / cells as f64).max(DVec2::splat(1e-9));
        let mut o = Outline {
            pts,
            lo,
            hi,
            cells,
            cell,
            grid: vec![Vec::new(); cells * cells],
            inside: vec![None; cells * cells],
        };
        for i in 0..n {
            let (a, b) = o.edge(i);
            let (x0, y0) = o.cell_of(a.min(b));
            let (x1, y1) = o.cell_of(a.max(b));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    o.grid[y * cells + x].push(i as u32);
                }
            }
        }
        o
    }

    fn edge(&self, i: usize) -> (DVec2, DVec2) {
        (self.pts[i], self.pts[(i + 1) % self.pts.len()])
    }

    fn cell_of(&self, p: DVec2) -> (usize, usize) {
        let c = ((p - self.lo) / self.cell).floor();
        let k = |v: f64| (v.max(0.0) as usize).min(self.cells - 1);
        (k(c.x), k(c.y))
    }

    fn overlaps(&self, lo: DVec2, hi: DVec2) -> bool {
        lo.x <= self.hi.x && hi.x >= self.lo.x && lo.y <= self.hi.y && hi.y >= self.lo.y
    }

    /// The edges whose cells the box touches, in ascending order.
    fn near(&self, lo: DVec2, hi: DVec2) -> Vec<u32> {
        let (x0, y0) = self.cell_of(lo);
        let (x1, y1) = self.cell_of(hi);
        let mut v: Vec<u32> = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                v.extend_from_slice(&self.grid[y * self.cells + x]);
            }
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Is the point inside (even-odd rule)? Costs one step per edge.
    fn contains(&self, p: DVec2, work: &mut u64) -> bool {
        *work += self.pts.len() as u64;
        let mut inside = false;
        let n = self.pts.len();
        for i in 0..n {
            let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// Is the middle of the cell that holds `p` inside? For a cell without edges.
    fn cell_inside(&mut self, p: DVec2, work: &mut u64) -> bool {
        if p.x < self.lo.x || p.x > self.hi.x || p.y < self.lo.y || p.y > self.hi.y {
            return false;
        }
        let (x, y) = self.cell_of(p);
        let at = y * self.cells + x;
        if let Some(v) = self.inside[at] {
            return v;
        }
        let mid = self.lo + self.cell * DVec2::new(x as f64 + 0.5, y as f64 + 0.5);
        let v = self.contains(mid, work);
        self.inside[at] = Some(v);
        v
    }
}

/// Signed distance of a point to the line through `a` and `b`: positive on the left.
fn side(a: DVec2, dir: DVec2, len: f64, p: DVec2) -> f64 {
    dir.perp_dot(p - a) / len
}

struct Cutter<'a> {
    vertices: &'a [DVec3],
    outlines: Vec<Outline<'a>>,
    work: u64,
    pieces: usize,
}

impl Cutter<'_> {
    /// Does the line of the edge (`a`, `b`) cross the interior of the piece with the edge itself
    /// (not only its line)? If so, the signed distances of the corners are returned.
    fn crossing(&mut self, piece: &Piece, a: DVec2, b: DVec2) -> Option<Vec<f64>> {
        self.work += piece.len() as u64;
        let dir = b - a;
        let len = dir.length();
        if len == 0.0 {
            return None;
        }
        let d: Vec<f64> = piece
            .iter()
            .map(|c| side(a, dir, len, c.p.truncate()))
            .collect();
        let has = |f: &dyn Fn(f64) -> bool| d.iter().any(|v| f(*v));
        if !(has(&|v| v > ON_LINE) && has(&|v| v < -ON_LINE)) {
            return None;
        }
        // Where the line meets the boundary of the piece, as distances along the edge.
        let along = |p: DVec2| dir.dot(p - a) / (len * len);
        let (mut t0, mut t1) = (f64::INFINITY, f64::NEG_INFINITY);
        let n = piece.len();
        for i in 0..n {
            let (p, q) = (piece[i].p.truncate(), piece[(i + 1) % n].p.truncate());
            let (dp, dq) = (d[i], d[(i + 1) % n]);
            let t = if dp.abs() <= ON_LINE {
                Some(along(p))
            } else if (dp > 0.0) != (dq > 0.0) && dq.abs() > ON_LINE {
                Some(along(p + (q - p) * (dp / (dp - dq))))
            } else {
                None
            };
            if let Some(t) = t {
                t0 = t0.min(t);
                t1 = t1.max(t);
            }
        }
        (t0.max(0.0) < t1.min(1.0)).then_some(d)
    }

    /// Cuts a piece along the line of the edge (`a`, `b`) with the distances `d` of its corners.
    /// `line` numbers the line for the keys of new points. Returns the piece on the left and the
    /// piece on the right.
    fn split(&mut self, piece: &Piece, d: &[f64], a: DVec2, b: DVec2, line: u32) -> (Piece, Piece) {
        let dir = b - a;
        let len = dir.length();
        let sign = |v: f64| {
            if v > ON_LINE {
                1
            } else if v < -ON_LINE {
                -1
            } else {
                0
            }
        };
        let (mut left, mut right) = (Piece::new(), Piece::new());
        let n = piece.len();
        for i in 0..n {
            let (c, e) = (piece[i], piece[(i + 1) % n]);
            let (sc, se) = (sign(d[i]), sign(d[(i + 1) % n]));
            if sc >= 0 {
                left.push(Corner {
                    edge: if sc == 0 && se < 0 { None } else { c.edge },
                    ..c
                });
            }
            if sc <= 0 {
                right.push(Corner {
                    edge: if sc == 0 && se > 0 { None } else { c.edge },
                    ..c
                });
            }
            if sc * se < 0 {
                // The edge crosses the line: a new point on it.
                let cut = self.cut_point(&c, &e, d[i], d[(i + 1) % n], a, dir, len, line);
                // On the side of `c` the edge ends at the new point; the closing edge follows.
                // On the side of `e` the new point starts the rest of the edge.
                let (c_side, e_side) = if sc > 0 {
                    (&mut left, &mut right)
                } else {
                    (&mut right, &mut left)
                };
                c_side.push(Corner { edge: None, ..cut });
                e_side.push(Corner { edge: c.edge, ..cut });
            }
        }
        (left, right)
    }

    #[allow(clippy::too_many_arguments)]
    fn cut_point(
        &mut self,
        c: &Corner,
        e: &Corner,
        dc: f64,
        de: f64,
        a: DVec2,
        dir: DVec2,
        len: f64,
        line: u32,
    ) -> Corner {
        match c.edge {
            Some((lo, hi)) => {
                // On an edge of the source mesh: computed from its ends, for both neighbors.
                let (p, q) = (self.vertices[lo as usize], self.vertices[hi as usize]);
                let (dp, dq) = (
                    side(a, dir, len, p.truncate()),
                    side(a, dir, len, q.truncate()),
                );
                let t = if dp == dq {
                    0.5
                } else {
                    (dp / (dp - dq)).clamp(0.0, 1.0)
                };
                Corner {
                    key: Key::Cut(lo, hi, line),
                    p: p + (q - p) * t,
                    edge: None,
                }
            }
            None => {
                // Computed from the two ends in a fixed order, so that the pieces on both sides
                // of the edge get the same point.
                let bits = |p: DVec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                let (u, w, du, dw) = if bits(c.p) <= bits(e.p) {
                    (c.p, e.p, dc, de)
                } else {
                    (e.p, c.p, de, dc)
                };
                let t = (du / (du - dw)).clamp(0.0, 1.0);
                let p = u + (w - u) * t;
                Corner {
                    key: Key::At(bits(p)),
                    p,
                    edge: None,
                }
            }
        }
    }

    /// Cuts a piece by the edges `edges` of outline `bed` (from position `from`) until no edge
    /// crosses it, and calls `leaf` for each piece.
    fn cut(
        &mut self,
        piece: Piece,
        bed: usize,
        edges: &[u32],
        from: usize,
        leaves: &mut Vec<Piece>,
    ) {
        let (lo, hi) = bounds(&piece);
        for (k, &e) in edges.iter().enumerate().skip(from) {
            let (a, b) = self.outlines[bed].edge(e as usize);
            self.work += 1;
            if a.max(b).x < lo.x || a.min(b).x > hi.x || a.max(b).y < lo.y || a.min(b).y > hi.y {
                continue;
            }
            if self.pieces >= MAX_PIECES_PER_TRIANGLE {
                break;
            }
            let Some(d) = self.crossing(&piece, a, b) else {
                continue;
            };
            let line = (bed * LINES_PER_BED) as u32 + e;
            let (l, r) = self.split(&piece, &d, a, b, line);
            self.pieces += 1;
            for half in [l, r] {
                if half.len() >= 3 {
                    self.cut(half, bed, edges, k + 1, leaves);
                }
            }
            return;
        }
        leaves.push(piece);
    }
}

static ZERO3: [DVec2; 3] = [DVec2::ZERO; 3];

/// Edges per bed that a line number leaves room for.
const LINES_PER_BED: usize = 1 << 16;

fn bounds(piece: &Piece) -> (DVec2, DVec2) {
    piece.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), c| (lo.min(c.p.truncate()), hi.max(c.p.truncate())),
    )
}

fn centroid(piece: &Piece) -> DVec2 {
    piece.iter().map(|c| c.p.truncate()).sum::<DVec2>() / piece.len() as f64
}

fn area(piece: &Piece) -> f64 {
    let n = piece.len();
    (0..n)
        .map(|i| {
            piece[i]
                .p
                .truncate()
                .perp_dot(piece[(i + 1) % n].p.truncate())
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

#[derive(Default)]
struct Builder {
    map: HashMap<Key, u32>,
    mesh: PartMesh,
}

impl Builder {
    fn point(&mut self, c: &Corner) -> u32 {
        *self.map.entry(c.key).or_insert_with(|| {
            self.mesh.vertices.push(c.p);
            (self.mesh.vertices.len() - 1) as u32
        })
    }

    fn polygon(&mut self, piece: &Piece) -> usize {
        let ids: Vec<u32> = piece.iter().map(|c| self.point(c)).collect();
        for i in 1..ids.len() - 1 {
            self.mesh.indices.extend([ids[0], ids[i], ids[i + 1]]);
        }
        ids.len() - 2
    }
}

/// Cuts a triangle mesh along the outlines of beds. `outlines` go from the back to the front of
/// the stacking order (at most 64); each has at least 3 points. Indices that point outside
/// `vertices` are skipped. See the module text for the bounds.
pub fn cut_by_beds(vertices: &[DVec3], indices: &[u32], outlines: &[&[DVec2]]) -> BedCut {
    cut_capped(vertices, indices, outlines, MAX_CUT_TRIANGLES, MAX_WORK)
}

fn cut_capped(
    vertices: &[DVec3],
    indices: &[u32],
    outlines: &[&[DVec2]],
    cap: usize,
    max_work: u64,
) -> BedCut {
    let outlines: Vec<&[DVec2]> = outlines
        .iter()
        .copied()
        .take(64)
        .map(|o| if o.len() >= 3 { o } else { &[][..] })
        .collect();
    let mut cutter = Cutter {
        vertices,
        outlines: outlines
            .iter()
            .map(|o| Outline::new(if o.is_empty() { &ZERO3 } else { o }))
            .collect(),
        work: 0,
        pieces: 0,
    };
    let usable: Vec<bool> = outlines.iter().map(|o| !o.is_empty()).collect();
    let mut base = Builder::default();
    let mut beds: Vec<Builder> = outlines.iter().map(|_| Builder::default()).collect();
    let mut produced = 0usize;
    let mut truncated = false;
    for tri in indices.as_chunks::<3>().0 {
        let ids = tri.map(|i| i as usize);
        if ids.iter().any(|&i| i >= vertices.len()) {
            continue;
        }
        let corners: Piece = (0..3)
            .map(|k| {
                let (i, j) = (tri[k], tri[(k + 1) % 3]);
                Corner {
                    key: Key::Source(i),
                    p: vertices[i as usize],
                    edge: Some((i.min(j), i.max(j))),
                }
            })
            .collect();
        if truncated || produced >= cap || cutter.work >= max_work {
            truncated = true;
            produced += base.polygon(&corners);
            continue;
        }
        let (lo, hi) = bounds(&corners);
        cutter.pieces = 0;
        let mut pieces: Vec<(Piece, u64)> = vec![(corners, 0)];
        for (b, usable) in usable.iter().enumerate() {
            if !usable || !cutter.outlines[b].overlaps(lo, hi) {
                continue;
            }
            let edges = cutter.outlines[b].near(lo, hi);
            let mut next = Vec::with_capacity(pieces.len());
            for (piece, bits) in pieces {
                let mut leaves = Vec::new();
                cutter.cut(piece, b, &edges, 0, &mut leaves);
                for leaf in leaves {
                    let mid = centroid(&leaf);
                    let inside = if edges.is_empty() {
                        let mut w = 0;
                        let v = cutter.outlines[b].cell_inside(mid, &mut w);
                        cutter.work += w;
                        v
                    } else {
                        let mut w = 0;
                        let v = cutter.outlines[b].contains(mid, &mut w);
                        cutter.work += w;
                        v
                    };
                    next.push((leaf, bits | (u64::from(inside) << b)));
                }
            }
            pieces = next;
        }
        for (piece, bits) in pieces {
            if piece.len() < 3 || area(&piece) < MIN_AREA {
                continue;
            }
            produced += if bits == 0 {
                base.polygon(&piece)
            } else {
                beds[63 - bits.leading_zeros() as usize].polygon(&piece)
            };
        }
    }
    BedCut {
        base: base.mesh,
        beds: beds.into_iter().map(|b| b.mesh).collect(),
        truncated,
    }
}

#[cfg(test)]
mod tests;
