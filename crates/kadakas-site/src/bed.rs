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
//! triangles that share the edge get the same point and share the vertex. A T-junction (a vertex
//! on one side of an edge and none on the other) can remain where an outline corner lies exactly
//! on a mesh edge; the point is on the edge, so there is no gap. With the normals of the source
//! mesh given, the pieces get interpolated normals, so shading is smooth across the outlines.
//!
//! The work is bounded:
//! - A grid per bed finds the outline edges near a triangle. Its entries are counted and capped
//!   per outline ([`MAX_GRID_ENTRIES`]); a segment is registered in the cells it crosses.
//! - The cutting uses an explicit stack, not recursion.
//! - Every step counts in a budget ([`MAX_WORK`]); the output is capped at [`MAX_CUT_TRIANGLES`]
//!   and one triangle at [`MAX_PIECES_PER_TRIANGLE`] cuts.
//! - When a bound is reached while a triangle is cut, the whole source triangle goes to the base
//!   (no half-cut pieces) and `truncated` is set.

use std::collections::HashMap;

use glam::{DVec2, DVec3};

/// The most triangles that the cut may output for one mesh.
pub const MAX_CUT_TRIANGLES: usize = 2_000_000;
/// The most steps (tests of an edge against a piece, grid entries, points) of one cut.
pub const MAX_WORK: u64 = 200_000_000;
/// The most cuts of one source triangle.
pub const MAX_PIECES_PER_TRIANGLE: usize = 4096;
/// The most grid entries of one outline.
pub const MAX_GRID_ENTRIES: usize = 1 << 17;
/// Pieces with an area (mm squared) below this are dropped.
const MIN_AREA: f64 = 1e-9;

/// The triangles of one part of the cut: their own vertices, and indices into them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartMesh {
    pub vertices: Vec<DVec3>,
    /// One unit normal per vertex, interpolated from the source normals. Empty if none were given.
    pub normals: Vec<DVec3>,
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
    /// The steps of work that the cut counted (for tests of the bounds).
    pub work: u64,
}

/// The normals of a triangle mesh as the scene sink computes them: smooth, with the triangles
/// turned to face up.
pub fn smooth_normals(vertices: &[DVec3], indices: &[u32]) -> Vec<DVec3> {
    let mut n = vec![DVec3::ZERO; vertices.len()];
    for t in indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| i as usize);
        if a >= vertices.len() || b >= vertices.len() || c >= vertices.len() {
            continue;
        }
        let mut f = (vertices[b] - vertices[a]).cross(vertices[c] - vertices[a]);
        if f.z < 0.0 {
            f = -f;
        }
        for i in [a, b, c] {
            n[i] += f;
        }
    }
    n.into_iter()
        .map(|v| v.try_normalize().unwrap_or(DVec3::Z))
        .collect()
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
    n: DVec3,
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
    /// The round in which an edge was last listed by `near` (one slot per edge).
    stamp: Vec<u32>,
    round: u32,
}

impl<'a> Outline<'a> {
    /// The outline with its grid, and the steps that building it took.
    fn new(pts: &'a [DVec2]) -> (Outline<'a>, u64) {
        let (lo, hi) = pts.iter().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let n = pts.len();
        let mut cells = ((n as f64).sqrt() / 2.0).ceil().clamp(1.0, 32.0) as usize;
        let mut work = 0u64;
        loop {
            let cell = ((hi - lo) / cells as f64).max(DVec2::splat(1e-9));
            let mut o = Outline {
                pts,
                lo,
                hi,
                cells,
                cell,
                grid: vec![Vec::new(); cells * cells],
                inside: vec![None; cells * cells],
                stamp: vec![0; n],
                round: 0,
            };
            let mut entries = 0usize;
            let mut fits = true;
            for i in 0..n {
                let (a, b) = o.edge(i);
                work += 1;
                if !o.add_segment(i as u32, a, b, &mut entries) {
                    fits = false;
                    break;
                }
            }
            work += entries as u64;
            if fits || cells == 1 {
                return (o, work);
            }
            // Too many entries (long zigzag edges): a coarser grid has fewer.
            cells /= 2;
        }
    }

    /// Lists edge `i` in the cells that the segment crosses. False if the entries pass the cap.
    fn add_segment(&mut self, i: u32, a: DVec2, b: DVec2, entries: &mut usize) -> bool {
        let (a, b) = if a.x <= b.x { (a, b) } else { (b, a) };
        let (cx0, _) = self.cell_of(a);
        let (cx1, _) = self.cell_of(b);
        let slop = self.cell.y * 1e-6;
        for cx in cx0..=cx1 {
            // The part of the segment in this column of cells.
            let x0 = a.x.max(self.lo.x + cx as f64 * self.cell.x);
            let x1 = b.x.min(self.lo.x + (cx + 1) as f64 * self.cell.x);
            let at = |x: f64| {
                if b.x > a.x {
                    a.y + (x.clamp(a.x, b.x) - a.x) * (b.y - a.y) / (b.x - a.x)
                } else {
                    a.y
                }
            };
            let (ya, yb) = if b.x > a.x {
                (at(x0), at(x1))
            } else {
                (a.y, b.y)
            };
            let (_, cy0) = self.cell_of(DVec2::new(self.lo.x, ya.min(yb) - slop));
            let (_, cy1) = self.cell_of(DVec2::new(self.lo.x, ya.max(yb) + slop));
            for cy in cy0..=cy1 {
                *entries += 1;
                if *entries > MAX_GRID_ENTRIES {
                    return false;
                }
                self.grid[cy * self.cells + cx].push(i);
            }
        }
        true
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

    /// The edges whose cells the box touches, each once. Every entry visited counts as work.
    fn near(&mut self, lo: DVec2, hi: DVec2, work: &mut u64) -> Vec<u32> {
        let (x0, y0) = self.cell_of(lo);
        let (x1, y1) = self.cell_of(hi);
        self.round = self.round.wrapping_add(1);
        if self.round == 0 {
            self.stamp.fill(0);
            self.round = 1;
        }
        let mut v = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let list = &self.grid[y * self.cells + x];
                *work += 1 + list.len() as u64;
                for &e in list {
                    if self.stamp[e as usize] != self.round {
                        self.stamp[e as usize] = self.round;
                        v.push(e);
                    }
                }
            }
        }
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
    normals: Option<&'a [DVec3]>,
    outlines: Vec<Outline<'a>>,
    work: u64,
    max_work: u64,
    pieces: usize,
    max_pieces: usize,
    /// A distance below this is "on the line": relative to the size and the place of the
    /// triangle that is cut.
    eps: f64,
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
        let eps = self.eps;
        let d: Vec<f64> = piece
            .iter()
            .map(|c| side(a, dir, len, c.p.truncate()))
            .collect();
        let has = |f: &dyn Fn(f64) -> bool| d.iter().any(|v| f(*v));
        if !(has(&|v| v > eps) && has(&|v| v < -eps)) {
            return None;
        }
        // Where the line meets the boundary of the piece, as distances along the edge.
        let along = |p: DVec2| dir.dot(p - a) / (len * len);
        let (mut t0, mut t1) = (f64::INFINITY, f64::NEG_INFINITY);
        let n = piece.len();
        for i in 0..n {
            let (p, q) = (piece[i].p.truncate(), piece[(i + 1) % n].p.truncate());
            let (dp, dq) = (d[i], d[(i + 1) % n]);
            let t = if dp.abs() <= eps {
                Some(along(p))
            } else if (dp > 0.0) != (dq > 0.0) && dq.abs() > eps {
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
        let eps = self.eps;
        let sign = |v: f64| {
            if v > eps {
                1
            } else if v < -eps {
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
                e_side.push(Corner {
                    edge: c.edge,
                    ..cut
                });
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
                let n = self.normals.map_or(DVec3::ZERO, |ns| {
                    ns[lo as usize] + (ns[hi as usize] - ns[lo as usize]) * t
                });
                Corner {
                    key: Key::Cut(lo, hi, line),
                    p: p + (q - p) * t,
                    n,
                    edge: None,
                }
            }
            None => {
                // Computed from the two ends in a fixed order, so that the pieces on both sides
                // of the edge get the same point.
                let bits = |p: DVec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                let (u, w, du, dw) = if bits(c.p) <= bits(e.p) {
                    (c, e, dc, de)
                } else {
                    (e, c, de, dc)
                };
                let t = (du / (du - dw)).clamp(0.0, 1.0);
                let p = u.p + (w.p - u.p) * t;
                Corner {
                    key: Key::At(bits(p)),
                    p,
                    n: u.n + (w.n - u.n) * t,
                    edge: None,
                }
            }
        }
    }

    /// Cuts a piece by the edges `edges` of outline `bed` until no edge crosses it, and adds the
    /// pieces to `leaves`. Uses a stack of (piece, next edge to try). Returns false if the work
    /// budget or the number of cuts of the triangle is used up; `leaves` is then not complete.
    fn cut(&mut self, piece: Piece, bed: usize, edges: &[u32], leaves: &mut Vec<Piece>) -> bool {
        let mut stack = vec![(piece, 0usize)];
        while let Some((piece, from)) = stack.pop() {
            let (lo, hi) = bounds(&piece);
            let mut cut_here = false;
            for (k, &e) in edges.iter().enumerate().skip(from) {
                let (a, b) = self.outlines[bed].edge(e as usize);
                self.work += 1;
                if self.work > self.max_work {
                    return false;
                }
                if a.max(b).x < lo.x || a.min(b).x > hi.x || a.max(b).y < lo.y || a.min(b).y > hi.y
                {
                    continue;
                }
                let Some(d) = self.crossing(&piece, a, b) else {
                    continue;
                };
                if self.pieces >= self.max_pieces {
                    return false;
                }
                let line = (bed * LINES_PER_BED) as u32 + e;
                let (l, r) = self.split(&piece, &d, a, b, line);
                self.pieces += 1;
                // The left piece is handled first.
                for half in [r, l] {
                    if half.len() >= 3 {
                        stack.push((half, k + 1));
                    }
                }
                cut_here = true;
                break;
            }
            if !cut_here {
                leaves.push(piece);
            }
        }
        true
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
    normals: bool,
}

impl Builder {
    fn point(&mut self, c: &Corner) -> u32 {
        *self.map.entry(c.key).or_insert_with(|| {
            self.mesh.vertices.push(c.p);
            if self.normals {
                self.mesh
                    .normals
                    .push(c.n.try_normalize().unwrap_or(DVec3::Z));
            }
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
    cut_with_normals(vertices, indices, outlines, None)
}

/// Like [`cut_by_beds`]. With one normal per source vertex (see [`smooth_normals`]), the pieces
/// get normals interpolated from them (`PartMesh::normals`).
pub fn cut_with_normals(
    vertices: &[DVec3],
    indices: &[u32],
    outlines: &[&[DVec2]],
    normals: Option<&[DVec3]>,
) -> BedCut {
    let limits = Limits {
        cap: MAX_CUT_TRIANGLES,
        max_work: MAX_WORK,
        max_pieces: MAX_PIECES_PER_TRIANGLE,
    };
    run(vertices, indices, outlines, normals, limits)
}

/// The bounds of one cut (the constants, or smaller ones in tests).
#[derive(Clone, Copy)]
struct Limits {
    cap: usize,
    max_work: u64,
    max_pieces: usize,
}

#[cfg(test)]
fn cut_capped(
    vertices: &[DVec3],
    indices: &[u32],
    outlines: &[&[DVec2]],
    cap: usize,
    max_work: u64,
) -> BedCut {
    let limits = Limits {
        cap,
        max_work,
        max_pieces: MAX_PIECES_PER_TRIANGLE,
    };
    run(vertices, indices, outlines, None, limits)
}

fn run(
    vertices: &[DVec3],
    indices: &[u32],
    outlines: &[&[DVec2]],
    normals: Option<&[DVec3]>,
    limits: Limits,
) -> BedCut {
    let normals = normals.filter(|n| n.len() == vertices.len());
    let outlines: Vec<&[DVec2]> = outlines
        .iter()
        .copied()
        .take(64)
        .map(|o| if o.len() >= 3 { o } else { &[][..] })
        .collect();
    let mut work = 0u64;
    let built: Vec<Outline> = outlines
        .iter()
        .map(|o| {
            let (o, w) = Outline::new(if o.is_empty() { &ZERO3 } else { o });
            work += w;
            o
        })
        .collect();
    let mut cutter = Cutter {
        vertices,
        normals,
        outlines: built,
        work,
        max_work: limits.max_work,
        pieces: 0,
        max_pieces: limits.max_pieces,
        eps: 0.0,
    };
    let usable: Vec<bool> = outlines.iter().map(|o| !o.is_empty()).collect();
    let new_builder = || Builder {
        normals: normals.is_some(),
        ..Builder::default()
    };
    let mut base = new_builder();
    let mut beds: Vec<Builder> = outlines.iter().map(|_| new_builder()).collect();
    let mut produced = 0usize;
    let mut truncated = false;
    for tri in indices.as_chunks::<3>().0 {
        if tri.iter().any(|&i| i as usize >= vertices.len()) {
            continue;
        }
        let corners: Piece = (0..3)
            .map(|k| {
                let (i, j) = (tri[k], tri[(k + 1) % 3]);
                Corner {
                    key: Key::Source(i),
                    p: vertices[i as usize],
                    n: normals.map_or(DVec3::ZERO, |n| n[i as usize]),
                    edge: Some((i.min(j), i.max(j))),
                }
            })
            .collect();
        if produced >= limits.cap || cutter.work >= limits.max_work {
            truncated = true;
            produced += base.polygon(&corners);
            continue;
        }
        let (lo, hi) = bounds(&corners);
        let mag = lo.abs().max(hi.abs()).max_element();
        cutter.eps = (1e-9 * (hi - lo).max_element() + 1e-14 * mag).max(1e-12);
        cutter.pieces = 0;
        let mut pieces: Vec<(Piece, u64)> = vec![(corners.clone(), 0)];
        let mut complete = true;
        'beds: for (b, usable) in usable.iter().enumerate() {
            if !usable || !cutter.outlines[b].overlaps(lo, hi) {
                continue;
            }
            if cutter.work >= limits.max_work {
                complete = false;
                break;
            }
            let mut w = 0;
            let edges = cutter.outlines[b].near(lo, hi, &mut w);
            cutter.work += w;
            let mut next = Vec::with_capacity(pieces.len());
            for (piece, bits) in std::mem::take(&mut pieces) {
                let mut leaves = Vec::new();
                if !cutter.cut(piece, b, &edges, &mut leaves) {
                    complete = false;
                    break 'beds;
                }
                for leaf in leaves {
                    let mid = centroid(&leaf);
                    let mut w = 0;
                    let inside = if edges.is_empty() {
                        cutter.outlines[b].cell_inside(mid, &mut w)
                    } else {
                        cutter.outlines[b].contains(mid, &mut w)
                    };
                    cutter.work += w;
                    next.push((leaf, bits | (u64::from(inside) << b)));
                }
            }
            pieces = next;
            if cutter.work > limits.max_work {
                complete = false;
                break;
            }
        }
        if !complete {
            // A bound was reached in the middle of this triangle: it goes whole to the base.
            truncated = true;
            produced += base.polygon(&corners);
            continue;
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
        work: cutter.work,
    }
}

#[cfg(test)]
mod tests;
