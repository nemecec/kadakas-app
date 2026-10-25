//! The strict check of a result of the cutting. A result that passes is right; a refusal is
//! allowed (and counted); a result that does not pass is wrong, which is never allowed.

use std::collections::{HashMap, HashSet};

use super::shapes::{Case, Rng};

pub enum Verdict {
    Valid,
    Wrong(String),
    Refused,
}

type P = [f64; 2];

fn cross(a: P, b: P, c: P) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn area(pts: &[P]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            (a[0] - pts[0][0]) * (b[1] - pts[0][1]) - (b[0] - pts[0][0]) * (a[1] - pts[0][1])
        })
        .sum::<f64>()
        / 2.0
}

/// Signed distance of c from the line ab (positive on the left).
fn side(a: P, b: P, c: P) -> f64 {
    let l = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    if l == 0.0 { 0.0 } else { cross(a, b, c) / l }
}

fn dist_seg(a: P, b: P, p: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

/// Even-odd test of a point against all the loops.
fn in_region(segs: &[(P, P)], p: P) -> bool {
    let mut c = false;
    for &(a, b) in segs {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        {
            c = !c;
        }
    }
    c
}

/// A grid of the segments, to find the ones near a box.
struct Grid {
    lo: P,
    cell: f64,
    n: usize,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    fn new(lo: P, extent: f64, segs: &[(P, P)]) -> Grid {
        let n = ((segs.len() as f64).sqrt().ceil() as usize).clamp(1, 512);
        let cell = (extent / n as f64).max(1e-300);
        let mut g = Grid {
            lo,
            cell,
            n,
            cells: vec![Vec::new(); n * n],
        };
        for (i, &(a, b)) in segs.iter().enumerate() {
            let (x0, y0, x1, y1) = g.range(a.min_p(b), a.max_p(b));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    g.cells[y * n + x].push(i as u32);
                }
            }
        }
        g
    }
    fn range(&self, lo: P, hi: P) -> (usize, usize, usize, usize) {
        let f = |v: f64, o: f64| (((v - o) / self.cell).floor().max(0.0) as usize).min(self.n - 1);
        (
            f(lo[0], self.lo[0]),
            f(lo[1], self.lo[1]),
            f(hi[0], self.lo[0]),
            f(hi[1], self.lo[1]),
        )
    }
    /// The segments in the cells of the box (with repeats).
    fn near(&self, lo: P, hi: P) -> Vec<u32> {
        let (x0, y0, x1, y1) = self.range(lo, hi);
        let mut out = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                out.extend_from_slice(&self.cells[y * self.n + x]);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

trait MinMax {
    fn min_p(self, o: Self) -> Self;
    fn max_p(self, o: Self) -> Self;
}

impl MinMax for P {
    fn min_p(self, o: P) -> P {
        [self[0].min(o[0]), self[1].min(o[1])]
    }
    fn max_p(self, o: P) -> P {
        [self[0].max(o[0]), self[1].max(o[1])]
    }
}

fn key(p: P) -> [u64; 2] {
    [(p[0] + 0.0).to_bits(), (p[1] + 0.0).to_bits()]
}

/// The area of the region that the case describes: the outline less the holes that differ.
fn region_area(case: &Case) -> f64 {
    let pts = |l: &Vec<usize>| l.iter().map(|&i| case.uv[i]).collect::<Vec<P>>();
    let mut seen: HashSet<Vec<[u64; 2]>> = HashSet::new();
    let mut total = area(&pts(&case.loops[0])).abs();
    for l in &case.loops[1..] {
        let p = pts(l);
        let mut k: Vec<[u64; 2]> = p.iter().map(|&q| key(q)).collect();
        k.sort();
        if seen.insert(k) {
            total -= area(&p).abs();
        }
    }
    total
}

pub fn check(case: &Case, r: Result<(Vec<[u32; 3]>, u64), kadakas_step::cutting::Cut>) -> Verdict {
    let (tris, _) = match r {
        Ok(x) => x,
        Err(_) => return Verdict::Refused,
    };
    match verify(case, &tris) {
        Ok(()) => Verdict::Valid,
        Err(why) => Verdict::Wrong(why),
    }
}

fn verify(case: &Case, tris: &[[u32; 3]]) -> Result<(), String> {
    let p = |i: u32| case.uv[i as usize];
    let loops: Vec<Vec<P>> = case
        .loops
        .iter()
        .map(|l| l.iter().map(|&i| case.uv[i]).collect())
        .collect();
    let segs: Vec<(P, P)> = loops
        .iter()
        .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
        .collect();
    // The loops that differ: two holes that are the same count once (even and odd would cancel).
    let mut seen: HashSet<Vec<[u64; 2]>> = HashSet::new();
    let region_segs: Vec<(P, P)> = loops
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            let mut k: Vec<[u64; 2]> = l.iter().map(|&q| key(q)).collect();
            k.sort();
            *i == 0 || seen.insert(k)
        })
        .flat_map(|(_, l)| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
        .collect();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for l in &loops {
        for q in l {
            lo = lo.min_p(*q);
            hi = hi.max_p(*q);
        }
    }
    let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let delta = 1e-10 * extent;
    let want = region_area(case);

    // The area, and no triangle the wrong way.
    let mut got = 0.0;
    for t in tris {
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let cr = cross(a, b, c);
        let longest = [a, b, c]
            .iter()
            .zip([b, c, a])
            .map(|(x, y)| (x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2))
            .fold(0.0, f64::max);
        if cr < -1e-9 * longest {
            return Err(format!("a triangle looks the wrong way: {a:?} {b:?} {c:?}"));
        }
        got += cr / 2.0;
    }
    if (got - want).abs() > case.tol * want.abs() + 1e-12 * extent * extent {
        return Err(format!("the triangles cover {got}, the region is {want}"));
    }

    // No segment of a loop through the inside of a triangle.
    let grid = Grid::new(lo, extent, &segs);
    for t in tris {
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        if cross(a, b, c).abs() < 1e-12 * extent * extent {
            continue;
        }
        let o = if cross(a, b, c) > 0.0 { 1.0 } else { -1.0 };
        let edges = [(a, b), (b, c), (c, a)];
        let inner = |q: P| edges.iter().all(|&(u, v)| o * side(u, v, q) > delta);
        for id in grid.near(a.min_p(b).min_p(c), a.max_p(b).max_p(c)) {
            let (s0, s1) = segs[id as usize];
            if inner(s0) || inner(s1) {
                return Err(format!(
                    "a loop segment {s0:?}-{s1:?} ends inside the triangle {a:?} {b:?} {c:?}"
                ));
            }
            for &(u, v) in &edges {
                let (d0, d1) = (side(u, v, s0), side(u, v, s1));
                let (e0, e1) = (side(s0, s1, u), side(s0, s1, v));
                if d0 * d1 < 0.0
                    && e0 * e1 < 0.0
                    && d0.abs().min(d1.abs()) > delta
                    && e0.abs().min(e1.abs()) > delta
                {
                    return Err(format!(
                        "a loop segment {s0:?}-{s1:?} crosses the edge {u:?}-{v:?} of a triangle"
                    ));
                }
            }
        }
    }

    // Each directed edge once, and the inner edges have a neighbour.
    let mut directed: HashMap<([u64; 2], [u64; 2]), u32> = HashMap::new();
    for t in tris {
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let n = directed.entry((key(u), key(v))).or_default();
            *n += 1;
            if *n > 1 {
                return Err(format!("the edge {u:?}-{v:?} is used twice the same way"));
            }
        }
    }
    let by_key: HashMap<[u64; 2], P> = tris
        .iter()
        .flat_map(|t| t.iter().map(|&i| (key(p(i)), p(i))))
        .collect();
    for &(ku, kv) in directed.keys() {
        if directed.contains_key(&(kv, ku)) {
            continue;
        }
        let (u, v) = (by_key[&ku], by_key[&kv]);
        let on_loop = grid.near(u.min_p(v), u.max_p(v)).into_iter().any(|id| {
            let (s0, s1) = segs[id as usize];
            dist_seg(s0, s1, u) <= delta && dist_seg(s0, s1, v) <= delta
        });
        let len = ((u[0] - v[0]).powi(2) + (u[1] - v[1]).powi(2)).sqrt();
        if !on_loop && len > 1e-6 * extent {
            return Err(format!("the inner edge {u:?}-{v:?} has no neighbour"));
        }
    }

    // Some triangles have their centre in the region, and points are in one triangle or none.
    let mut rng = Rng::new(0xC0FFEE ^ tris.len() as u64);
    for _ in 0..200.min(tris.len()) {
        let t = tris[rng.int(tris.len())];
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        if cross(a, b, c).abs() < 1e-9 * extent * extent * 1e-3 {
            continue;
        }
        let centre = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        let near_loop = segs
            .iter()
            .any(|&(s0, s1)| dist_seg(s0, s1, centre) < 3.0 * delta);
        if !near_loop && !in_region(&region_segs, centre) {
            return Err(format!(
                "the centre {centre:?} of a triangle is outside the region"
            ));
        }
    }
    let tri_data: Vec<[P; 3]> = tris.iter().map(|t| [p(t[0]), p(t[1]), p(t[2])]).collect();
    for _ in 0..200 {
        let q = [rng.range(lo[0], hi[0]), rng.range(lo[1], hi[1])];
        if segs
            .iter()
            .any(|&(s0, s1)| dist_seg(s0, s1, q) < 3.0 * delta)
        {
            continue;
        }
        let (mut inside_count, mut ambiguous) = (0, false);
        for tri in &tri_data {
            let o = if cross(tri[0], tri[1], tri[2]) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let m = (0..3)
                .map(|e| o * side(tri[e], tri[(e + 1) % 3], q))
                .fold(f64::INFINITY, f64::min);
            if m > delta {
                inside_count += 1;
            } else if m > -delta {
                ambiguous = true;
            }
        }
        if ambiguous {
            continue;
        }
        let expected = usize::from(in_region(&region_segs, q));
        if inside_count != expected {
            return Err(format!(
                "the point {q:?} is in {inside_count} triangles, and should be in {expected}"
            ));
        }
    }
    Ok(())
}
