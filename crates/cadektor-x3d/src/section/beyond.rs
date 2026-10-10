//! The objects beyond the cut plane, as visible lines: silhouettes, creases and borders of the
//! meshes, with hidden parts removed.
//!
//! The hidden-line pass is the one of `cadektor_render::hlr`, the same as in 3D views and hidden-line
//! viewports. The triangles are put in a scene where the view runs along +Y with X to the right and Z
//! up (the standard front view): x is u (along the section), y is w (the distance from the plane) and
//! z is the height. The cut faces become caps on the plane, so they hide what is behind them, and
//! the lines on the plane itself are left out (the cut outline is drawn by the viewport).

use std::collections::BTreeMap;

use cadektor_core::mesh::{Bounds3, Scene};
use cadektor_core::model::ObjectId;
use cadektor_render::camera::Camera;
use cadektor_render::hlr::{HlOptions, hidden_lines};
use cadektor_render::views::{Projection, StandardView};
use glam::{DVec2, DVec3, Vec3};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use super::{CutFace, Edge};

/// Most edges in a drawing.
pub const MAX_EDGES: usize = 300_000;
/// The image of the hidden-line pass, in pixels on a side. Lines are exact; this is the grid of the cover test.
const IMAGE: u32 = 8192;

/// A triangle in section space (u, v, w).
#[derive(Clone, Debug)]
pub struct Tri {
    pub object: ObjectId,
    pub p: [DVec3; 3],
    /// The original vertices (bits of the scene position).
    pub key: [[u32; 3]; 3],
    /// A plant.
    pub plant: bool,
}

/// A line of the scene in section space.
#[derive(Clone, Debug)]
pub struct Seg {
    pub object: ObjectId,
    pub a: DVec3,
    pub b: DVec3,
}

/// The region that shows: `0 <= u <= len` and `0 <= w <= depth`.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub len: f64,
    pub depth: f64,
}

/// Cuts a triangle by `w >= 0`. The new vertices are computed from the end points in a fixed
/// order, so that triangles that share an edge get the same point.
fn clip_near(t: [DVec3; 3]) -> Vec<[DVec3; 3]> {
    let inside = |p: DVec3| p.z >= 0.0;
    if t.iter().all(|p| inside(*p)) {
        return vec![t];
    }
    if t.iter().all(|p| !inside(*p)) {
        return Vec::new();
    }
    let cross = |a: DVec3, b: DVec3| -> DVec3 {
        let (a, b) = if (a.x, a.y, a.z) <= (b.x, b.y, b.z) {
            (a, b)
        } else {
            (b, a)
        };
        let f = a.z / (a.z - b.z);
        let mut p = a.lerp(b, f);
        p.z = 0.0;
        p
    };
    let mut poly: Vec<DVec3> = Vec::with_capacity(4);
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        if inside(a) {
            poly.push(a);
        }
        if inside(a) != inside(b) {
            poly.push(cross(a, b));
        }
    }
    (1..poly.len().saturating_sub(1))
        .map(|i| [poly[0], poly[i], poly[i + 1]])
        .collect()
}

/// Triangles that cover a cut face (in u, v), counterclockwise. Holes stay open.
fn cap(face: &CutFace) -> Vec<[DVec2; 3]> {
    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    let mut any = false;
    for ring in &face.rings {
        let mut prev = None;
        let mut first = None;
        for p in ring {
            if !(p.x.is_finite() && p.y.is_finite()) {
                continue;
            }
            let Ok(h) = cdt.insert(Point2::new(p.x, p.y)) else {
                continue;
            };
            first.get_or_insert(h);
            if let Some(q) = prev
                && q != h
            {
                cdt.add_constraint_and_split(q, h, |v| v);
                any = true;
            }
            prev = Some(h);
        }
        if let (Some(a), Some(b)) = (prev, first)
            && a != b
        {
            cdt.add_constraint_and_split(a, b, |v| v);
        }
    }
    if !any {
        return Vec::new();
    }
    let inside = |c: DVec2| {
        face.rings
            .iter()
            .filter(|r| r.len() >= 3)
            .fold(false, |acc, r| acc ^ inside_ring(c, r))
    };
    let mut out = Vec::new();
    for f in cdt.inner_faces() {
        let [a, b, c] = f.positions().map(|p| DVec2::new(p.x, p.y));
        let centroid = (a + b + c) / 3.0;
        if !inside(centroid) {
            continue;
        }
        let area = (b - a).perp_dot(c - a);
        if area.abs() < 1e-9 {
            continue;
        }
        out.push(if area > 0.0 { [a, b, c] } else { [a, c, b] });
    }
    out
}

fn inside_ring(p: DVec2, ring: &[DVec2]) -> bool {
    let mut c = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}

/// Distance from `p` to the segment `a`-`b`.
fn distance_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 < 1e-18 {
        0.0
    } else {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}

/// The visible lines beyond the plane, from the hidden-line pass of the renderer. `covers` are the
/// cut faces that hide what is behind them. `tol` is the distance in mm below which lines count as
/// lying on the cut outline.
pub fn edges(
    tris: &[Tri],
    segs: &[Seg],
    region: &Region,
    covers: &[&CutFace],
    tol: f64,
) -> Vec<Edge> {
    // Triangles at or behind the plane, and not wholly beyond the far clip. Section space is
    // (u, v, w); the scene is (u, w, v), which keeps the winding.
    let mut scene = Scene::default();
    let mut depth: BTreeMap<u64, f64> = BTreeMap::new();
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = DVec2::splat(f64::NEG_INFINITY);
    let mut push_tri = |scene: &mut Scene, object: ObjectId, p: [DVec3; 3]| {
        let base = scene.mesh.positions.len() as u32;
        let q = p.map(|v| Vec3::new(v.x as f32, v.z as f32, v.y as f32));
        let n = (q[1] - q[0]).cross(q[2] - q[0]).normalize_or_zero();
        for v in q {
            scene.mesh.positions.push(v);
            scene.mesh.normals.push(n);
            scene.mesh.colors.push([255, 255, 255, 255]);
        }
        scene
            .mesh
            .indices
            .extend_from_slice(&[base, base + 1, base + 2]);
        scene.mesh.tri_object.push(object);
        scene.mesh.tri_part.push(0);
        for v in p {
            lo = lo.min(DVec2::new(v.x, v.y));
            hi = hi.max(DVec2::new(v.x, v.y));
        }
    };
    for t in tris {
        let min_w = t.p.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
        if min_w > region.depth {
            continue;
        }
        let pieces = clip_near(t.p);
        if pieces.is_empty() {
            continue;
        }
        let d = depth.entry(t.object.0).or_insert(f64::INFINITY);
        *d = d.min(min_w.max(0.0));
        for piece in pieces {
            push_tri(&mut scene, t.object, piece);
        }
    }
    for f in covers {
        for tri in cap(f) {
            // Counterclockwise in (u, v) is counterclockwise for the viewer.
            let p = tri.map(|q| DVec3::new(q.x, q.y, 0.0));
            push_tri(&mut scene, f.object, p);
        }
    }
    let mut seg_depth: BTreeMap<u64, (f64, usize)> = BTreeMap::new();
    for s in segs {
        let (wa, wb) = (s.a.z, s.b.z);
        if wa.max(wb) < 0.0 || wa.min(wb) > region.depth {
            continue;
        }
        let (mut a, mut b) = (s.a, s.b);
        if wa < 0.0 {
            a = a.lerp(b, -wa / (wb - wa));
        } else if wb < 0.0 {
            b = b.lerp(a, -wb / (wa - wb));
        }
        for v in [a, b] {
            lo = lo.min(DVec2::new(v.x, v.y));
            hi = hi.max(DVec2::new(v.x, v.y));
            scene
                .lines
                .positions
                .push(Vec3::new(v.x as f32, v.z as f32, v.y as f32));
        }
        scene.lines.colors.push([0, 0, 0, 255]);
        scene.lines.objects.push(s.object);
        let e = seg_depth.entry(s.object.0).or_insert((0.0, 0));
        e.0 += (a.z + b.z) / 2.0;
        e.1 += 1;
    }
    if scene.is_empty() || lo.x > hi.x {
        return Vec::new();
    }
    let ys = scene
        .mesh
        .positions
        .iter()
        .chain(&scene.lines.positions)
        .map(|p| p.y);
    let (wlo, whi) = ys.fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), y| {
        (l.min(y), h.max(y))
    });
    let bounds: Bounds3 = (
        Vec3::new(lo.x as f32, wlo, lo.y as f32),
        Vec3::new(hi.x as f32, whi, hi.y as f32),
    );
    scene.bounds = Some(bounds);

    // The picture is a square that holds the whole drawing; u runs from the center to the right.
    let half = (region.len.max(hi.y - lo.y).max(hi.x - lo.x) / 2.0 * 1.05).max(1.0);
    let (tu, tv) = (region.len / 2.0, (lo.y + hi.y) / 2.0);
    let mut camera = Camera::standard(StandardView::Front, Projection::Orthographic);
    camera.distance = 1.0;
    camera.distance = (half / f64::from(camera.half_height())) as f32;
    camera.target = Vec3::new(tu as f32, (wlo + whi) / 2.0, tv as f32);
    let lines = hidden_lines(
        &scene,
        &camera,
        IMAGE,
        IMAGE,
        &HlOptions {
            include_hidden: false,
            mesh_edges: true,
        },
    );
    let px = f64::from(IMAGE);
    let to_uv = |p: [f32; 2]| {
        DVec2::new(
            tu + (f64::from(p[0]) / px - 0.5) * 2.0 * half,
            tv - (f64::from(p[1]) / px - 0.5) * 2.0 * half,
        )
    };
    // The outline of the cut: lines on it are the outline already.
    let outline: Vec<(DVec2, DVec2)> = covers
        .iter()
        .flat_map(|f| f.rings.iter())
        .flat_map(|r| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()])))
        .collect();
    let near = tol.clamp(0.2, 1.0);
    let on_outline = |a: DVec2, b: DVec2| {
        let m = (a + b) / 2.0;
        [a, m, b].iter().all(|q| {
            outline
                .iter()
                .any(|(c, d)| distance_to_segment(*q, *c, *d) <= near)
        })
    };
    let mut out = Vec::new();
    for s in lines.visible() {
        let (mut a, mut b) = (to_uv(s.a), to_uv(s.b));
        // Keep the part inside the section length.
        let d = b - a;
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut ok = true;
        for (p, q) in [(-d.x, a.x), (d.x, region.len - a.x)] {
            if p.abs() < 1e-12 {
                if q < 0.0 {
                    ok = false;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        if !ok || t0 >= t1 {
            continue;
        }
        (a, b) = (a + d * t0, a + d * t1);
        if a.distance(b) < 1e-3 || on_outline(a, b) {
            continue;
        }
        let Some(object) = s.object else {
            continue;
        };
        let w = depth
            .get(&object.0)
            .copied()
            .or_else(|| seg_depth.get(&object.0).map(|(sum, n)| sum / *n as f64))
            .unwrap_or(0.0);
        out.push(Edge {
            object,
            a,
            b,
            depth: w,
        });
        if out.len() >= MAX_EDGES {
            break;
        }
    }
    out
}
