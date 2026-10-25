//! Polygons with holes for the fuzz test of the cutting: one generator for each kind of
//! trouble that the reviews found. Every generator makes a valid input (the outline counterclockwise
//! and the holes clockwise, unless the kind says otherwise) from a seed.

use std::f64::consts::TAU;

pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// A number in [0, 1).
    pub fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f()
    }
    /// An integer in 0..n.
    pub fn int(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, p: f64) -> bool {
        self.f() < p
    }
}

/// One input of the cutting: points and loops (the first is the outline), and the relative
/// error of the area that the numbers allow.
pub struct Case {
    pub name: &'static str,
    pub uv: Vec<[f64; 2]>,
    pub loops: Vec<Vec<usize>>,
    pub tol: f64,
}

#[derive(Default)]
struct Build {
    uv: Vec<[f64; 2]>,
    loops: Vec<Vec<usize>>,
}

impl Build {
    fn add(&mut self, pts: &[[f64; 2]]) {
        let base = self.uv.len();
        self.uv.extend_from_slice(pts);
        self.loops.push((base..base + pts.len()).collect());
    }
    fn finish(mut self, name: &'static str, tol: f64) -> Case {
        // A point that two loops share in the file has a point of its own for each loop.
        let uv = std::mem::take(&mut self.uv);
        Case {
            name,
            uv,
            loops: self.loops,
            tol,
        }
    }
}

fn signed_area(p: &[[f64; 2]]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            (a[0] - p[0][0]) * (b[1] - p[0][1]) - (b[0] - p[0][0]) * (a[1] - p[0][1])
        })
        .sum::<f64>()
        / 2.0
}

fn ccw(mut p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if signed_area(&p) < 0.0 {
        p.reverse();
    }
    p
}

fn cw(mut p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if signed_area(&p) > 0.0 {
        p.reverse();
    }
    p
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let n = poly.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        {
            c = !c;
        }
    }
    c
}

fn dist_to_segment(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

fn dist_to_polygon(poly: &[[f64; 2]], p: [f64; 2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| dist_to_segment(poly[i], poly[(i + 1) % n], p))
        .fold(f64::INFINITY, f64::min)
}

/// A convex polygon of 3 to 6 corners in the circle (clockwise, for a hole).
fn hole_in_circle(rng: &mut Rng, cx: f64, cy: f64, r: f64) -> Vec<[f64; 2]> {
    let n = 3 + rng.int(4);
    let phase = rng.range(0.0, TAU);
    let pts: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let a = phase + TAU * (i as f64 + rng.range(-0.25, 0.25)) / n as f64;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect();
    cw(pts)
}

/// Circles inside the outline that do not touch it or each other.
fn scatter(
    rng: &mut Rng,
    outline: &[[f64; 2]],
    n: usize,
    rmin: f64,
    rmax: f64,
) -> Vec<(f64, f64, f64)> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in outline {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let mut out: Vec<(f64, f64, f64)> = Vec::new();
    for _ in 0..n * 20 {
        if out.len() >= n {
            break;
        }
        let r = (rmin.ln() + (rmax.ln() - rmin.ln()) * rng.f()).exp();
        let c = [rng.range(lo[0], hi[0]), rng.range(lo[1], hi[1])];
        if !inside(outline, c) || dist_to_polygon(outline, c) <= r * 1.1 {
            continue;
        }
        if out
            .iter()
            .any(|&(x, y, q)| ((x - c[0]).powi(2) + (y - c[1]).powi(2)).sqrt() <= (q + r) * 1.1)
        {
            continue;
        }
        out.push((c[0], c[1], r));
    }
    out
}

fn with_scattered_holes(
    rng: &mut Rng,
    name: &'static str,
    outline: Vec<[f64; 2]>,
    n: usize,
    rmin: f64,
    rmax: f64,
    tol: f64,
) -> Case {
    let mut b = Build::default();
    let holes = scatter(rng, &outline, n, rmin, rmax);
    b.add(&outline);
    for (x, y, r) in holes {
        let h = hole_in_circle(rng, x, y, r);
        b.add(&h);
    }
    b.finish(name, tol)
}

fn shifted(c: Case, dx: f64, dy: f64, tol: f64, name: &'static str) -> Case {
    Case {
        name,
        uv: c.uv.iter().map(|p| [p[0] + dx, p[1] + dy]).collect(),
        loops: c.loops,
        tol,
    }
}

fn rotated(c: &mut Case, quarter: usize) {
    for p in &mut c.uv {
        for _ in 0..quarter {
            *p = [-p[1], p[0]];
        }
    }
}

fn convex_outline(rng: &mut Rng, size: f64) -> Vec<[f64; 2]> {
    let n = 3 + rng.int(10);
    let (a, b) = (size * rng.range(0.5, 1.0), size * rng.range(0.5, 1.0));
    let phase = rng.range(0.0, TAU);
    ccw((0..n)
        .map(|i| {
            let t = phase + TAU * (i as f64 + rng.range(-0.3, 0.3)) / n as f64;
            [a * t.cos(), b * t.sin()]
        })
        .collect())
}

fn star_outline(rng: &mut Rng, size: f64) -> Vec<[f64; 2]> {
    let n = 5 + rng.int(25);
    ccw((0..2 * n)
        .map(|i| {
            let t = TAU * i as f64 / (2 * n) as f64;
            let r = if i % 2 == 0 {
                size
            } else {
                size * rng.range(0.3, 0.7)
            };
            [r * t.cos(), r * t.sin()]
        })
        .collect())
}

fn comb_outline(rng: &mut Rng) -> Vec<[f64; 2]> {
    let teeth = 3 + rng.int(25);
    let base = rng.range(20.0, 60.0);
    let mut pts = vec![[0.0, 0.0], [teeth as f64 * 4.0, 0.0]];
    for t in (0..teeth).rev() {
        let x = t as f64 * 4.0;
        let h = base + rng.range(5.0, 40.0);
        pts.extend([
            [x + 4.0, h],
            [x + 3.0, h],
            [x + 3.0, base],
            [x + 1.0, base],
            [x + 1.0, h],
            [x, h],
        ]);
    }
    // The last corner of a tooth is the first corner of the next one.
    pts.dedup();
    ccw(pts)
}

fn l_outline(rng: &mut Rng) -> Vec<[f64; 2]> {
    let (w, h) = (rng.range(60.0, 100.0), rng.range(60.0, 100.0));
    let (a, b) = (w * rng.range(0.3, 0.6), h * rng.range(0.15, 0.4));
    let pts = if rng.chance(0.5) {
        vec![[0.0, 0.0], [w, 0.0], [w, b], [a, b], [a, h], [0.0, h]]
    } else {
        // A C shape.
        vec![
            [0.0, 0.0],
            [w, 0.0],
            [w, b],
            [a, b],
            [a, h - b],
            [w, h - b],
            [w, h],
            [0.0, h],
        ]
    };
    ccw(pts)
}

fn grid_case(rng: &mut Rng, name: &'static str, jitter: bool, most: usize) -> Case {
    let (nx, ny) = (1 + rng.int(most), 1 + rng.int(most));
    let pitch = rng.range(2.0, 10.0);
    let side = pitch * rng.range(0.2, 0.7);
    let mut b = Build::default();
    b.add(&ccw(vec![
        [0.0, 0.0],
        [nx as f64 * pitch + pitch, 0.0],
        [nx as f64 * pitch + pitch, ny as f64 * pitch + pitch],
        [0.0, ny as f64 * pitch + pitch],
    ]));
    for j in 0..ny {
        for i in 0..nx {
            if rng.chance(0.1) {
                continue;
            }
            let (x, y) = (
                pitch * (i as f64 + 0.5) + pitch / 2.0 - side / 2.0,
                pitch * (j as f64 + 0.5) + pitch / 2.0 - side / 2.0,
            );
            let jt = |rng: &mut Rng| {
                if jitter {
                    rng.range(-0.1, 0.1) * side
                } else {
                    0.0
                }
            };
            let pts = vec![
                [x + jt(rng), y + jt(rng)],
                [x + jt(rng), y + side + jt(rng)],
                [x + side + jt(rng), y + side + jt(rng)],
                [x + side + jt(rng), y + jt(rng)],
            ];
            b.add(&cw(pts));
        }
    }
    b.finish(name, 1e-9)
}

/// Rectangle [0,w]x[0,h] with a triangle hole whose top corner is at (x, h) on the top edge.
fn edge_triangle(x: f64, h: f64, half: f64, depth: f64) -> Vec<[f64; 2]> {
    cw(vec![[x, h], [x + half, h - depth], [x - half, h - depth]])
}

fn pinwheel(rng: &mut Rng, name: &'static str, k: usize, centre: [f64; 2], r: f64) -> Build {
    let mut b = Build::default();
    b.add(&ccw(vec![
        [centre[0] - 3.0 * r, centre[1] - 3.0 * r],
        [centre[0] + 3.0 * r, centre[1] - 3.0 * r],
        [centre[0] + 3.0 * r, centre[1] + 3.0 * r],
        [centre[0] - 3.0 * r, centre[1] + 3.0 * r],
    ]));
    let _ = name;
    // k wedges with the same corner, with gaps between them.
    let sector = TAU / k as f64;
    let phase = rng.range(0.0, TAU);
    for i in 0..k {
        let a0 = phase + sector * i as f64 + sector * 0.1;
        let a1 = phase + sector * i as f64 + sector * 0.7;
        let rr = r * rng.range(0.5, 1.0);
        let tri = vec![
            centre,
            [centre[0] + rr * a0.cos(), centre[1] + rr * a0.sin()],
            [centre[0] + rr * a1.cos(), centre[1] + rr * a1.sin()],
        ];
        b.add(&cw(tri));
    }
    b
}

pub const KINDS: &[&str] = &[
    "convex",
    "star",
    "comb",
    "concave_nested",
    "grid",
    "tiny_holes",
    "shared_x",
    "collinear",
    "near_touch",
    "offset_1e7",
    "offset_1e9",
    "far_small_1e7",
    "tiny_abs",
    "touch_outline_vertex",
    "touch_outline_edge",
    "touch_holes_pair",
    "touch_holes_fan",
    "touch_holes_chain",
    "touch_two_places",
    "degenerate_repeat",
    "degenerate_spike",
    "degenerate_selftouch",
    "orient_mix",
    "duplicate_holes",
    "fan_at_one_point",
];

pub fn make(kind: &str, seed: u64) -> Case {
    let mut rng =
        Rng::new(seed ^ (kind.len() as u64) << 32 ^ kind.bytes().map(u64::from).sum::<u64>() << 8);
    let rng = &mut rng;
    match kind {
        "convex" => {
            let o = convex_outline(rng, 100.0);
            let n = rng.int(12);
            with_scattered_holes(rng, "convex", o, n, 0.5, 8.0, 1e-9)
        }
        "star" => {
            let o = star_outline(rng, 100.0);
            let n = rng.int(10);
            with_scattered_holes(rng, "star", o, n, 0.3, 5.0, 1e-9)
        }
        "comb" => {
            let o = comb_outline(rng);
            let n = rng.int(10);
            with_scattered_holes(rng, "comb", o, n, 0.3, 4.0, 1e-9)
        }
        "concave_nested" => {
            let o = l_outline(rng);
            let n = 1 + rng.int(20);
            with_scattered_holes(rng, "concave_nested", o, n, 0.5, 10.0, 1e-9)
        }
        "grid" => {
            let jitter = rng.chance(0.5);
            // Up to 30 x 30 holes, in one case of 30 and small ones in the others.
            let most = if seed.is_multiple_of(30) { 30 } else { 8 };
            grid_case(rng, "grid", jitter, most)
        }
        "tiny_holes" => {
            let o = convex_outline(rng, 100.0);
            let n = 1 + rng.int(40);
            with_scattered_holes(rng, "tiny_holes", o, n, 1e-6, 1e-3, 1e-9)
        }
        "shared_x" => {
            // Holes whose rightmost corners have the same x: columns of squares.
            let (cols, rows) = (1 + rng.int(4), 1 + rng.int(8));
            let mut b = Build::default();
            b.add(&ccw(vec![
                [0.0, 0.0],
                [cols as f64 * 10.0 + 10.0, 0.0],
                [cols as f64 * 10.0 + 10.0, rows as f64 * 10.0 + 10.0],
                [0.0, rows as f64 * 10.0 + 10.0],
            ]));
            for c in 0..cols {
                for r in 0..rows {
                    let (x, y, s) = (
                        10.0 * (c as f64 + 1.0),
                        10.0 * (r as f64 + 0.5) + 5.0,
                        rng.range(1.0, 4.0),
                    );
                    b.add(&cw(vec![
                        [x, y],
                        [x, y + s],
                        [x + 3.0, y + s],
                        [x + 3.0, y],
                    ]));
                }
            }
            b.finish("shared_x", 1e-9)
        }
        "collinear" => {
            // A row of holes on one line, and extra corners on the outline's edges.
            let n = 1 + rng.int(30);
            let w = n as f64 * 10.0 + 10.0;
            let mut pts = vec![[0.0, 0.0]];
            for i in 1..4 {
                pts.push([w * i as f64 / 4.0, 0.0]);
            }
            pts.extend([[w, 0.0], [w, 10.0], [0.0, 10.0]]);
            let mut b = Build::default();
            b.add(&pts);
            for i in 0..n {
                let x = 10.0 * i as f64 + 5.0;
                b.add(&cw(vec![
                    [x, 4.0],
                    [x, 6.0],
                    [x + 4.0, 6.0],
                    [x + 4.0, 4.0],
                ]));
            }
            b.finish("collinear", 1e-9)
        }
        "near_touch" => {
            let gap = 10f64.powf(rng.range(-9.0, -3.0));
            let mut b = Build::default();
            b.add(&ccw(vec![
                [0.0, 0.0],
                [30.0, 0.0],
                [30.0, 20.0],
                [0.0, 20.0],
            ]));
            // Two squares with a gap, and a square near the outline.
            b.add(&cw(vec![
                [5.0, 5.0],
                [5.0, 10.0],
                [10.0, 10.0],
                [10.0, 5.0],
            ]));
            let x = 10.0 + gap;
            b.add(&cw(vec![
                [x, 5.0],
                [x, 10.0],
                [x + 5.0, 10.0],
                [x + 5.0, 5.0],
            ]));
            let g2 = 10f64.powf(rng.range(-9.0, -3.0));
            b.add(&cw(vec![
                [20.0, 20.0 - g2 - 3.0],
                [20.0, 20.0 - g2],
                [25.0, 20.0 - g2],
                [25.0, 20.0 - g2 - 3.0],
            ]));
            b.finish("near_touch", 1e-6)
        }
        "offset_1e7" | "offset_1e9" => {
            let base = if rng.chance(0.5) {
                grid_case_small(rng)
            } else {
                let o = comb_outline(rng);
                with_scattered_holes(rng, "x", o, 3, 0.5, 3.0, 1e-9)
            };
            let off = if kind == "offset_1e7" { 1e7 } else { 1e9 };
            let (dx, dy) = (
                off + rng.range(0.0, 1000.0),
                off * 0.37 + rng.range(0.0, 1000.0),
            );
            shifted(
                base,
                dx,
                dy,
                if kind == "offset_1e7" { 1e-7 } else { 1e-6 },
                if kind == "offset_1e7" {
                    "offset_1e7"
                } else {
                    "offset_1e9"
                },
            )
        }
        "far_small_1e7" => {
            let o = ccw(vec![[0.0, 0.0], [0.02, 0.0], [0.02, 0.015], [0.0, 0.015]]);
            let n = 1 + rng.int(8);
            let c = with_scattered_holes(rng, "x", o, n, 5e-4, 2e-3, 1e-9);
            shifted(
                c,
                1e7 + rng.range(0.0, 1000.0),
                3.7e6,
                1e-4,
                "far_small_1e7",
            )
        }
        "tiny_abs" => {
            let s = 10f64.powf(rng.range(-9.0, -6.0));
            let o = ccw(vec![
                [0.0, 0.0],
                [s * 10.0, 0.0],
                [s * 10.0, s * 7.0],
                [0.0, s * 7.0],
            ]);
            let n = 1 + rng.int(6);
            with_scattered_holes(rng, "tiny_abs", o, n, s * 0.1, s * 0.6, 1e-9)
        }
        "touch_outline_vertex" => {
            // A corner of a hole at a corner of the outline: both edges of the hole go inside.
            let o = ccw(if rng.chance(0.5) {
                vec![[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]]
            } else {
                vec![[0.0, 0.0], [40.0, 0.0], [20.0, 35.0]]
            });
            let n = o.len();
            let v = rng.int(n);
            let (p, prev, next) = (o[v], o[(v + n - 1) % n], o[(v + 1) % n]);
            let ang = |q: [f64; 2]| (q[1] - p[1]).atan2(q[0] - p[0]);
            let (a0, mut a1) = (ang(next), ang(prev));
            if a1 < a0 {
                a1 += TAU;
            }
            let (t0, t1) = (a0 + (a1 - a0) * 0.15, a0 + (a1 - a0) * 0.8);
            let r = rng.range(3.0, 8.0);
            let hole = cw(vec![
                p,
                [p[0] + r * t0.cos(), p[1] + r * t0.sin()],
                [p[0] + r * t1.cos(), p[1] + r * t1.sin()],
            ]);
            let mut b = Build::default();
            b.add(&o);
            b.add(&hole);
            if rng.chance(0.5) {
                b.add(&cw(vec![
                    [20.0, 15.0],
                    [20.0, 18.0],
                    [23.0, 18.0],
                    [23.0, 15.0],
                ]));
            }
            b.finish("touch_outline_vertex", 1e-9)
        }
        "touch_outline_edge" => {
            let (w, h) = (rng.range(8.0, 40.0), rng.range(8.0, 30.0));
            let mut b = Build::default();
            b.add(&ccw(vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]));
            // A hole inside, bridged normally, before or after.
            let inner = rng.chance(0.4).then(|| {
                let s = rng.range(0.5, 1.5);
                (w * rng.range(0.3, 0.7), h * 0.3, s)
            });
            let k = 1 + rng.int(5);
            let slot = w / (k as f64 + 1.0);
            let mut holes: Vec<Vec<[f64; 2]>> = Vec::new();
            for i in 0..k {
                let x = slot * (i as f64 + 1.0) + rng.range(-0.2, 0.2) * slot;
                let half = (slot * rng.range(0.1, 0.35)).min(w / 4.0);
                holes.push(edge_triangle(x, h, half, rng.range(0.5, 2.0).min(h * 0.4)));
            }
            if let Some((x, y, s)) = inner {
                holes.push(cw(vec![[x, y], [x, y + s], [x + s, y + s], [x + s, y]]));
            }
            // Any order of the holes in the file.
            for i in (1..holes.len()).rev() {
                holes.swap(i, rng.int(i + 1));
            }
            for hole in &holes {
                b.add(hole);
            }
            let mut c = b.finish("touch_outline_edge", 1e-9);
            rotated(&mut c, rng.int(4));
            c
        }
        "touch_holes_pair" => {
            let mut b = Build::default();
            let s = rng.range(5.0, 12.0);
            b.add(&ccw(vec![
                [0.0, 0.0],
                [10.0 * s, 0.0],
                [10.0 * s, 8.0 * s],
                [0.0, 8.0 * s],
            ]));
            let p = [5.0 * s, 4.0 * s];
            let a = rng.range(0.0, TAU);
            let wedge = |a0: f64, a1: f64, r: f64| {
                cw(vec![
                    p,
                    [p[0] + r * a0.cos(), p[1] + r * a0.sin()],
                    [p[0] + r * a1.cos(), p[1] + r * a1.sin()],
                ])
            };
            b.add(&wedge(a + 0.2, a + 1.2, s * 2.0));
            b.add(&wedge(
                a + 0.2 + std::f64::consts::PI,
                a + 1.5 + std::f64::consts::PI,
                s * 2.0,
            ));
            b.finish("touch_holes_pair", 1e-9)
        }
        "touch_holes_fan" => {
            let k = 3 + rng.int(12);
            pinwheel(rng, "touch_holes_fan", k, [0.0, 0.0], 5.0).finish("touch_holes_fan", 1e-9)
        }
        "touch_holes_chain" => {
            // Diamonds in a row that touch their neighbour at a corner, not the outline.
            let n = 2 + rng.int(10);
            // Sizes that add up exactly, so that the corners that should touch do.
            let r = (4 + rng.int(9)) as f64 / 4.0;
            let mut b = Build::default();
            b.add(&ccw(vec![
                [0.0, 0.0],
                [n as f64 * 2.0 * r + 4.0 * r, 0.0],
                [n as f64 * 2.0 * r + 4.0 * r, 6.0 * r],
                [0.0, 6.0 * r],
            ]));
            for i in 0..n {
                let cx = 2.0 * r + r + 2.0 * r * i as f64;
                b.add(&cw(vec![
                    [cx - r, 3.0 * r],
                    [cx, 3.0 * r + r],
                    [cx + r, 3.0 * r],
                    [cx, 3.0 * r - r],
                ]));
            }
            b.finish("touch_holes_chain", 1e-9)
        }
        "touch_two_places" => {
            // A diamond whose left and right corners are on the outline: the face is in two pieces.
            let (w, h) = (rng.range(10.0, 30.0), rng.range(10.0, 30.0));
            let mut b = Build::default();
            b.add(&ccw(vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]]));
            let y = h * rng.range(0.3, 0.7);
            let d = h * 0.2;
            b.add(&cw(vec![
                [0.0, y],
                [w / 2.0, y + d],
                [w, y],
                [w / 2.0, y - d],
            ]));
            b.finish("touch_two_places", 1e-9)
        }
        "degenerate_repeat" => {
            let o = convex_outline(rng, 100.0);
            let n = rng.int(5);
            let mut c = with_scattered_holes(rng, "degenerate_repeat", o, n, 1.0, 8.0, 1e-9);
            // Some points are written twice in a row.
            let mut loops = Vec::new();
            for l in &c.loops {
                let mut m = Vec::new();
                for &i in l {
                    m.push(i);
                    if rng.chance(0.3) {
                        let j = c.uv.len();
                        c.uv.push(c.uv[i]);
                        m.push(j);
                    }
                }
                loops.push(m);
            }
            c.loops = loops;
            c
        }
        "degenerate_spike" => {
            // A spike of no width out of a corner of the outline, and one into a hole.
            let w = rng.range(20.0, 60.0);
            let mut b = Build::default();
            b.add(&[
                [0.0, 0.0],
                [w, 0.0],
                [w, w / 2.0],
                [w + 5.0, w / 2.0],
                [w, w / 2.0],
                [w, w],
                [0.0, w],
            ]);
            b.add(&cw(vec![
                [w / 4.0, w / 4.0],
                [w / 4.0, w / 2.0],
                [w / 2.0, w / 2.0],
                [w / 2.0, w / 4.0],
            ]));
            b.finish("degenerate_spike", 1e-9)
        }
        "degenerate_selftouch" => {
            let s = rng.range(2.0, 20.0);
            let mut b = Build::default();
            b.add(&[
                [0.0, 0.0],
                [4.0 * s, 0.0],
                [2.0 * s, 2.0 * s],
                [4.0 * s, 4.0 * s],
                [0.0, 4.0 * s],
                [2.0 * s, 2.0 * s],
            ]);
            if rng.chance(0.5) {
                let h = s / 4.0;
                b.add(&cw(vec![
                    [2.0 * s - h, 0.5 * s],
                    [2.0 * s, 0.5 * s + h],
                    [2.0 * s + h, 0.5 * s],
                ]));
            }
            b.finish("degenerate_selftouch", 1e-9)
        }
        "orient_mix" => {
            let o = convex_outline(rng, 100.0);
            let n = rng.int(8);
            let mut c = with_scattered_holes(rng, "orient_mix", o, n, 1.0, 8.0, 1e-9);
            for l in &mut c.loops {
                if rng.chance(0.5) {
                    l.reverse();
                }
            }
            c
        }
        "duplicate_holes" => {
            let o = convex_outline(rng, 100.0);
            let mut b = Build::default();
            b.add(&o);
            // One circle inside the outline, and its hole repeated.
            let circle = scatter(rng, &o, 1, 2.0, 6.0)
                .first()
                .copied()
                .unwrap_or((0.0, 0.0, 0.0));
            let hole = if circle.2 > 0.0 {
                hole_in_circle(rng, circle.0, circle.1, circle.2)
            } else {
                Vec::new()
            };
            for _ in 0..if hole.is_empty() { 0 } else { 2 + rng.int(18) } {
                let mut h = hole.clone();
                let rot = rng.int(h.len());
                h.rotate_left(rot);
                if rng.chance(0.3) {
                    h.reverse();
                }
                b.add(&h);
            }
            b.finish("duplicate_holes", 1e-9)
        }
        "fan_at_one_point" => {
            let k = 3 + rng.int(40);
            let centre = [100.0 * rng.f(), 50.0 * rng.f()];
            pinwheel(rng, "fan_at_one_point", k, centre, 5.0).finish("fan_at_one_point", 1e-9)
        }
        other => panic!("no kind {other}"),
    }
}

fn grid_case_small(rng: &mut Rng) -> Case {
    let mut c = grid_case(rng, "x", false, 8);
    // Keep it small for the offsets.
    c.loops.truncate(1 + 12);
    c
}
