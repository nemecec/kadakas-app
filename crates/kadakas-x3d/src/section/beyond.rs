//! The objects beyond the cut plane, as visible lines: silhouettes, creases and borders of the
//! meshes, with hidden parts removed.
//!
//! The hidden-line pass is the one of `kadakas_render::hlr`, the same as in 3D views and hidden-line
//! viewports. The triangles are put in a scene where the view runs along +Y with X to the right and Z
//! up (the standard front view): x is u (along the section), y is w (the distance from the plane) and
//! z is the height. The cut faces become caps on the plane, so they hide what is behind them, and
//! the lines on the plane itself are left out (the cut outline is drawn by the viewport).

use std::collections::BTreeMap;

use glam::{DVec2, DVec3, Vec3};
use kadakas_core::mesh::{Bounds3, Scene};
use kadakas_core::model::ObjectId;
use kadakas_render::camera::Camera;
use kadakas_render::hlr::{HlOptions, hidden_lines};
use kadakas_render::views::{Projection, StandardView};
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
    /// The trunk part of a plant: it is cut like a solid, the canopy only gives a silhouette.
    pub trunk: bool,
    /// A triangle of a surface model whose faces do not close a volume: the cut is a line.
    pub open: bool,
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

/// Cuts a triangle to the slab `0 <= w <= depth`. The new vertices are computed from the end
/// points in a fixed order, so that triangles that share an edge get the same point. The pieces
/// of the cut on the two planes go to `clips` (in u, v): they are not edges of the model.
fn clip_slab(t: [DVec3; 3], depth: f64, clips: &mut Vec<[DVec2; 2]>) -> Vec<[DVec3; 3]> {
    let mut poly: Vec<DVec3> = t.to_vec();
    for (bound, keep_above) in [(0.0, true), (depth, false)] {
        if !bound.is_finite() {
            continue;
        }
        let inside = |p: DVec3| {
            if keep_above {
                p.z >= bound
            } else {
                p.z <= bound
            }
        };
        if poly.iter().all(|p| inside(*p)) {
            continue;
        }
        let cross = |a: DVec3, b: DVec3| -> DVec3 {
            let (a, b) = if (a.x, a.y, a.z) <= (b.x, b.y, b.z) {
                (a, b)
            } else {
                (b, a)
            };
            let f = (a.z - bound) / (a.z - b.z);
            let mut p = a.lerp(b, f);
            p.z = bound;
            p
        };
        let mut next: Vec<DVec3> = Vec::with_capacity(poly.len() + 1);
        let mut on_plane: Vec<DVec3> = Vec::with_capacity(2);
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            if inside(a) {
                next.push(a);
            }
            if inside(a) != inside(b) {
                let c = cross(a, b);
                next.push(c);
                on_plane.push(c);
            }
        }
        if let [p, q] = on_plane[..] {
            clips.push([DVec2::new(p.x, p.y), DVec2::new(q.x, q.y)]);
        }
        poly = next;
        if poly.len() < 3 {
            return Vec::new();
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
            .fold(false, |acc, r| {
                acc ^ kadakas_core::geom::point_in_polygon(c, r)
            })
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

/// The clip segments on a grid, to find the ones near a point without testing all of them.
struct ClipGrid {
    lo: DVec2,
    cell: f64,
    n: usize,
    cells: Vec<Vec<u32>>,
    /// Segments that span many cells; every query tests them.
    long: Vec<u32>,
}

impl ClipGrid {
    fn new(clips: &[[DVec2; 2]], reach: f64, bounds: (DVec2, DVec2)) -> ClipGrid {
        let (lo, hi) = bounds;
        let size = (hi - lo).max_element().max(1.0);
        let n = 256usize;
        let cell = (size / n as f64).max(reach * 2.0);
        let mut g = ClipGrid {
            lo,
            cell,
            n,
            cells: vec![Vec::new(); n * n],
            long: Vec::new(),
        };
        for (i, c) in clips.iter().enumerate() {
            let (a, b) = (
                g.cell_of(c[0].min(c[1]) - DVec2::splat(reach)),
                g.cell_of(c[0].max(c[1]) + DVec2::splat(reach)),
            );
            if (b.0 - a.0 + 1) * (b.1 - a.1 + 1) > 64 {
                g.long.push(i as u32);
                continue;
            }
            for y in a.1..=b.1 {
                for x in a.0..=b.0 {
                    g.cells[y * n + x].push(i as u32);
                }
            }
        }
        g
    }

    fn cell_of(&self, p: DVec2) -> (usize, usize) {
        let c = ((p - self.lo) / self.cell).floor();
        (
            c.x.clamp(0.0, self.n as f64 - 1.0) as usize,
            c.y.clamp(0.0, self.n as f64 - 1.0) as usize,
        )
    }

    /// True if `p` is within `reach` of a clip segment.
    fn near(&self, p: DVec2, clips: &[[DVec2; 2]], reach: f64) -> bool {
        let (x, y) = self.cell_of(p);
        self.cells[y * self.n + x]
            .iter()
            .chain(&self.long)
            .any(|i| {
                let c = clips[*i as usize];
                distance_to_segment(p, c[0], c[1]) <= reach
            })
    }
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
/// lying on the cut outline. `cut_is_drawn` is true when the caller draws the cut (a section); the
/// lines on the plane and on the far clip are then left out. In an elevation nothing else draws them.
/// `soft` are the soft and hidden edges of surface models: they are not drawn as creases. `thinned`
/// gets the number of objects that were drawn by their mesh edges only (too many lines).
#[allow(clippy::too_many_arguments)]
pub fn edges(
    tris: &[Tri],
    segs: &[Seg],
    soft: &[[DVec3; 2]],
    region: &Region,
    covers: &[&CutFace],
    tol: f64,
    cut_is_drawn: bool,
    thinned: &mut usize,
) -> Vec<Edge> {
    // Triangles at or behind the plane, and not wholly beyond the far clip. Section space is
    // (u, v, w); the scene is (u, w, v), which keeps the winding.
    let mut scene = Scene::default();
    let mut depth: BTreeMap<kadakas_core::uid::Uid, f64> = BTreeMap::new();
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
    // Where the slab cuts the triangles: not edges of the model, so lines on them are left out.
    let mut clips: Vec<[DVec2; 2]> = Vec::new();
    for t in tris {
        // Triangles outside the length, or outside the slab, cannot show or hide anything.
        let (mut lo_u, mut hi_u) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut lo_w, mut hi_w) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &t.p {
            lo_u = lo_u.min(p.x);
            hi_u = hi_u.max(p.x);
            lo_w = lo_w.min(p.z);
            hi_w = hi_w.max(p.z);
        }
        if hi_u < 0.0 || lo_u > region.len || hi_w < 0.0 || lo_w > region.depth {
            continue;
        }
        let min_w = lo_w;
        let pieces = clip_slab(t.p, region.depth, &mut clips);
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
    let mut seg_depth: BTreeMap<kadakas_core::uid::Uid, (f64, usize)> = BTreeMap::new();
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
    // The soft and hidden edges of surface models are not creases of the mesh.
    scene.soft_edges = soft
        .iter()
        .map(|s| s.map(|v| Vec3::new(v.x as f32, v.z as f32, v.y as f32)))
        .collect();
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
    *thinned = lines.thinned_objects;
    let px = f64::from(IMAGE);
    let to_uv = |p: [f32; 2]| {
        DVec2::new(
            tu + (f64::from(p[0]) / px - 0.5) * 2.0 * half,
            tv - (f64::from(p[1]) / px - 0.5) * 2.0 * half,
        )
    };
    let near = tol.clamp(0.2, 1.0);
    let grid = ClipGrid::new(&clips, near, (lo, hi));
    let on_clip = |a: DVec2, b: DVec2| {
        let m = (a + b) / 2.0;
        [a, m, b].iter().all(|q| grid.near(*q, &clips, near))
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
        if a.distance(b) < 1e-3 || (cut_is_drawn && on_clip(a, b)) {
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
