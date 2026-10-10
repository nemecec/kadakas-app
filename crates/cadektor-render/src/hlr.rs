//! Hidden-line removal: 2D vector lines of the visible edges of a 3D scene.
//!
//! This is the "Hidden Line" render mode, the one that matters most for landscape drawings. The
//! result is a list of line segments in image pixels (y down). Sheets draw them as vector lines
//! in SVG and PDF, and `rasterize` draws them in an image.
//!
//! The algorithm is exact and needs no pixels. It takes the edges of the scene (the line list and,
//! if asked, the silhouette and crease edges of the mesh) and cuts each edge where a triangle
//! covers it. Every test is linear along the edge, so each triangle hides one interval of an
//! edge. The remaining intervals are the visible parts. The order of the output only depends on
//! the order of the scene, so the output is the same on every run.
//!
//! A triangle with a corner behind the eye (perspective projection) is not used as a cover.

use std::collections::HashMap;

use cadektor_core::mesh::Scene;
use cadektor_core::model::ObjectId;
use glam::{DVec3, Vec3, Vec4};

use crate::camera::Camera;
use crate::raster::Image;
use crate::views::Projection;

/// Color of edges that the mesh adds (silhouettes and creases).
pub const DERIVED_EDGE: [u8; 4] = [50, 50, 50, 255];

/// Angle between two faces (degrees) from which their shared edge is drawn.
pub const CREASE_ANGLE: f64 = 35.0;

/// Shortest piece of a cut edge that is kept, in pixels.
const MIN_PIECE: f64 = 0.3;

/// Dash and gap length of hidden edges, in pixels.
pub const DASH: (f32, f32) = (6.0, 4.0);

/// One visible or hidden piece of an edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HlSegment {
    /// End points in pixels, y down.
    pub a: [f32; 2],
    pub b: [f32; 2],
    /// True for a part that a surface covers. Only present if hidden edges were asked for.
    pub hidden: bool,
    pub color: [u8; 4],
    /// The object that the edge belongs to. Edges of the mesh that two objects share belong to the first.
    pub object: Option<ObjectId>,
    /// Index of the edge in the scene (line list first, then the edges of the mesh).
    pub source: u32,
}

impl HlSegment {
    pub fn length(&self) -> f32 {
        ((self.b[0] - self.a[0]).powi(2) + (self.b[1] - self.a[1]).powi(2)).sqrt()
    }
}

/// What to compute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HlOptions {
    /// Also return the covered parts, flagged `hidden`.
    pub include_hidden: bool,
    /// Add silhouette and crease edges of the mesh to the edges in the line list.
    pub mesh_edges: bool,
}

impl Default for HlOptions {
    fn default() -> Self {
        HlOptions {
            include_hidden: false,
            mesh_edges: true,
        }
    }
}

/// The result: segments in pixels of an image of `width` by `height`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HiddenLines {
    pub width: u32,
    pub height: u32,
    pub segments: Vec<HlSegment>,
}

impl HiddenLines {
    /// How many different edges have a visible part longer than half a pixel.
    pub fn visible_edge_count(&self) -> usize {
        let mut seen: Vec<u32> = self
            .segments
            .iter()
            .filter(|s| !s.hidden && s.length() > 0.5)
            .map(|s| s.source)
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    }

    /// The visible segments only.
    pub fn visible(&self) -> impl Iterator<Item = &HlSegment> {
        self.segments.iter().filter(|s| !s.hidden)
    }

    /// SVG of the lines. `weight` is the stroke width in pixels. Hidden parts are dashed and gray.
    pub fn to_svg(&self, weight: f32) -> String {
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">",
            w = self.width,
            h = self.height
        );
        out.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>");
        for hidden in [true, false] {
            for s in self.segments.iter().filter(|s| s.hidden == hidden) {
                let c = if hidden {
                    [120, 120, 120, 255]
                } else {
                    s.color
                };
                out.push_str(&format!(
                    "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"rgb({},{},{})\" stroke-width=\"{:.2}\" stroke-linecap=\"round\"{}/>",
                    s.a[0],
                    s.a[1],
                    s.b[0],
                    s.b[1],
                    c[0],
                    c[1],
                    c[2],
                    if hidden { weight * 0.7 } else { weight },
                    if hidden {
                        format!(" stroke-dasharray=\"{} {}\"", DASH.0, DASH.1)
                    } else {
                        String::new()
                    }
                ));
            }
        }
        out.push_str("</svg>");
        out
    }

    /// Draws the lines into an image with anti-aliasing. Visible lines use the color of their edge,
    /// hidden lines are dashed and gray. `weight` is the line width in pixels.
    pub fn rasterize(&self, background: [u8; 4], weight: f32) -> Image {
        let mut img = Image::new(self.width.max(1), self.height.max(1), background);
        let mut cover = vec![0.0f32; img.width as usize * img.height as usize];
        let mut color = vec![[0u8; 3]; cover.len()];
        for hidden in [true, false] {
            for s in self.segments.iter().filter(|s| s.hidden == hidden) {
                let (w, c) = if hidden {
                    (weight * 0.7, [120, 120, 120])
                } else {
                    (weight, [s.color[0], s.color[1], s.color[2]])
                };
                draw_line(
                    &mut cover,
                    &mut color,
                    img.width,
                    img.height,
                    s,
                    w.max(0.5),
                    c,
                    hidden,
                );
            }
        }
        for (i, k) in cover.iter().enumerate() {
            if *k > 0.0 {
                let px = &mut img.rgba[i * 4..i * 4 + 4];
                for ch in 0..3 {
                    px[ch] = (px[ch] as f32 * (1.0 - k) + color[i][ch] as f32 * k).round() as u8;
                }
            }
        }
        img
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_line(
    cover: &mut [f32],
    color: &mut [[u8; 3]],
    width: u32,
    height: u32,
    s: &HlSegment,
    weight: f32,
    rgb: [u8; 3],
    dashed: bool,
) {
    let (a, b) = (
        Vec3::new(s.a[0], s.a[1], 0.0),
        Vec3::new(s.b[0], s.b[1], 0.0),
    );
    let ab = b - a;
    let len2 = ab.length_squared();
    let len = len2.sqrt();
    let r = weight / 2.0 + 1.0;
    let (x0, x1) = (
        (a.x.min(b.x) - r).floor().max(0.0) as i64,
        (a.x.max(b.x) + r).ceil().min(width as f32 - 1.0) as i64,
    );
    let (y0, y1) = (
        (a.y.min(b.y) - r).floor().max(0.0) as i64,
        (a.y.max(b.y) + r).ceil().min(height as f32 - 1.0) as i64,
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, 0.0);
            let t = if len2 > 0.0 {
                ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = (p - (a + ab * t)).length();
            let mut k = (weight / 2.0 + 0.5 - d).clamp(0.0, 1.0);
            if dashed && ((t * len) % (DASH.0 + DASH.1)) > DASH.0 {
                k = 0.0;
            }
            if k > 0.0 {
                let i = y as usize * width as usize + x as usize;
                if k > cover[i] {
                    cover[i] = k;
                    color[i] = rgb;
                }
            }
        }
    }
}

/// An edge in scene coordinates.
struct Edge {
    a: DVec3,
    b: DVec3,
    color: [u8; 4],
    object: Option<ObjectId>,
}

/// The two end points of an edge on a 0.1 mm grid, in a fixed order.
type EdgeKey = ([i32; 3], [i32; 3]);

fn quant(p: Vec3) -> [i32; 3] {
    // 0.1 mm grid: vertices of one corner that were made separately fall in one cell.
    [
        (p.x * 10.0).round() as i32,
        (p.y * 10.0).round() as i32,
        (p.z * 10.0).round() as i32,
    ]
}

/// Silhouette, crease and boundary edges of the mesh. Edges that already are in `known`
/// are left out.
fn mesh_edges(scene: &Scene, camera: Option<&Camera>, known: &[Edge]) -> Vec<Edge> {
    let mesh = &scene.mesh;
    let key = |a: Vec3, b: Vec3| {
        let (qa, qb) = (quant(a), quant(b));
        if qa <= qb { (qa, qb) } else { (qb, qa) }
    };
    let mut have: std::collections::HashSet<_> = known
        .iter()
        .map(|e| key(e.a.as_vec3(), e.b.as_vec3()))
        .collect();
    // Edge -> (first triangle, second triangle).
    let mut edges: HashMap<EdgeKey, (usize, Option<usize>)> = HashMap::new();
    let mut order = Vec::new();
    for t in 0..mesh.triangle_count() {
        let p = mesh.triangle(t);
        for k in 0..3 {
            let (a, b) = (p[k], p[(k + 1) % 3]);
            if a == b {
                continue;
            }
            let kk = key(a, b);
            match edges.get_mut(&kk) {
                Some(e) => {
                    if e.1.is_none() {
                        e.1 = Some(t);
                    }
                }
                None => {
                    edges.insert(kk, (t, None));
                    order.push((kk, a, b));
                }
            }
        }
    }
    let normal = |t: usize| {
        let p = mesh.triangle(t);
        (p[1] - p[0])
            .cross(p[2] - p[0])
            .as_dvec3()
            .normalize_or_zero()
    };
    // Without a camera there are no silhouettes: only creases and open edges.
    let front = |t: usize, n: DVec3| {
        camera.is_some_and(|camera| match camera.projection {
            Projection::Orthographic => n.dot(-camera.forward().as_dvec3()) > 0.0,
            Projection::Perspective => {
                n.dot(camera.eye().as_dvec3() - mesh.triangle(t)[0].as_dvec3()) > 0.0
            }
        })
    };
    let cos_crease = CREASE_ANGLE.to_radians().cos();
    let mut out = Vec::new();
    for (kk, a, b) in order {
        if have.contains(&kk) {
            continue;
        }
        let (t1, t2) = edges[&kk];
        let n1 = normal(t1);
        let draw = match t2 {
            // An open edge of a surface.
            None => true,
            Some(t2) => {
                let n2 = normal(t2);
                let silhouette = camera.is_some() && front(t1, n1) != front(t2, n2);
                // Two faces of different objects meet: the contour of each object stays.
                let crease = n1.dot(n2) < cos_crease;
                silhouette || crease
            }
        };
        if draw && !mesh.tri_object.is_empty() {
            have.insert(kk);
            out.push(Edge {
                a: a.as_dvec3(),
                b: b.as_dvec3(),
                color: DERIVED_EDGE,
                object: Some(mesh.tri_object[t1]),
            });
        }
    }
    // Where objects cut through each other (a wall in the ground, a pond in the terrain).
    for (seg, object) in intersection_edges(scene) {
        let key = key(seg[0].as_vec3(), seg[1].as_vec3());
        if have.insert(key) {
            out.push(Edge {
                a: seg[0],
                b: seg[1],
                color: DERIVED_EDGE,
                object: Some(object),
            });
        }
    }
    out
}

/// Most pairs of triangles that are tested for an intersection.
const MAX_INTERSECTION_PAIRS: usize = 30_000_000;

/// The segment where triangle `b` crosses triangle `a`, if the two cut through each other.
/// Triangles that touch, or lie in one plane, give nothing: their shared edges are in the
/// line list already.
pub fn triangle_cut(a: [DVec3; 3], b: [DVec3; 3], tol: f64) -> Option<[DVec3; 2]> {
    let n = (a[1] - a[0]).cross(a[2] - a[0]);
    if n.length() < 1e-12 {
        return None;
    }
    let n = n.normalize();
    let d = b.map(|p| n.dot(p - a[0]));
    if !(d.iter().any(|x| *x > tol) && d.iter().any(|x| *x < -tol)) {
        return None;
    }
    // Where the edges of b cross the plane of a.
    let mut pts: Vec<DVec3> = Vec::with_capacity(2);
    for k in 0..3 {
        let (j, dk, dj) = ((k + 1) % 3, d[k], d[(k + 1) % 3]);
        if (dk > tol && dj < -tol) || (dk < -tol && dj > tol) {
            pts.push(b[k] + (b[j] - b[k]) * (dk / (dk - dj)));
        } else if dk.abs() <= tol {
            pts.push(b[k]);
        }
    }
    if pts.len() != 2 {
        return None;
    }
    // Keep the part inside a.
    let centroid = (a[0] + a[1] + a[2]) / 3.0;
    let (mut lo, mut hi) = (0.0, 1.0);
    for k in 0..3 {
        let (p, q) = (a[k], a[(k + 1) % 3]);
        let mut inward = n.cross(q - p).normalize_or_zero();
        if inward.dot(centroid - p) < 0.0 {
            inward = -inward;
        }
        let g0 = inward.dot(pts[0] - p) + tol;
        let g1 = inward.dot(pts[1] - p) + tol;
        if !cut(&mut lo, &mut hi, g0, g1) {
            return None;
        }
    }
    let (s, e) = (
        pts[0] + (pts[1] - pts[0]) * lo,
        pts[0] + (pts[1] - pts[0]) * hi,
    );
    (s.distance(e) > tol * 4.0).then_some([s, e])
}

/// Segments where triangles of different objects cut through each other, with the object of
/// the triangle that is cut. Triangles are found with a grid, so the cost grows with the number of
/// close pairs, not with all pairs.
pub fn intersection_edges(scene: &Scene) -> Vec<([DVec3; 2], ObjectId)> {
    let mesh = &scene.mesh;
    let n = mesh.triangle_count();
    let Some((lo, hi)) = scene.bounds else {
        return Vec::new();
    };
    if n < 2 {
        return Vec::new();
    }
    let diag = (hi - lo).length().max(1.0) as f64;
    let tol = 1e-5 * diag + 1e-3;
    let boxes: Vec<(DVec3, DVec3)> = (0..n)
        .map(|t| {
            let p = mesh.triangle(t).map(|v| v.as_dvec3());
            (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]))
        })
        .collect();
    let mean_ext = boxes
        .iter()
        .map(|(a, b)| (*b - *a).max_element())
        .sum::<f64>()
        / n as f64;
    let cell = (diag / 64.0).max(mean_ext * 2.0).max(1e-3);
    let cell_of = |p: DVec3| -> [i64; 3] {
        let q = ((p - lo.as_dvec3()) / cell).floor();
        [q.x as i64, q.y as i64, q.z as i64]
    };
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    for (t, (a, b)) in boxes.iter().enumerate() {
        let (c0, c1) = (cell_of(*a), cell_of(*b));
        for x in c0[0]..=c1[0].min(c0[0] + 63) {
            for y in c0[1]..=c1[1].min(c0[1] + 63) {
                for z in c0[2]..=c1[2].min(c0[2] + 63) {
                    grid.entry([x, y, z]).or_default().push(t as u32);
                }
            }
        }
    }
    // Visit cells in a fixed order, so the output is the same on every run.
    let mut cells: Vec<&[i64; 3]> = grid.keys().collect();
    cells.sort();
    let mut tested = 0usize;
    let mut out = Vec::new();
    'cells: for c in cells {
        let list = &grid[c];
        for (ia, &ta) in list.iter().enumerate() {
            for &tb in &list[ia + 1..] {
                let (ta, tb) = (ta as usize, tb as usize);
                if mesh.tri_object[ta] == mesh.tri_object[tb] {
                    continue;
                }
                let (ba, bb) = (&boxes[ta], &boxes[tb]);
                let (omin, omax) = (ba.0.max(bb.0), ba.1.min(bb.1));
                if omin.x > omax.x + tol || omin.y > omax.y + tol || omin.z > omax.z + tol {
                    continue;
                }
                // Test a pair in one cell only: the first cell of the common box.
                if cell_of(omin) != *c {
                    continue;
                }
                tested += 1;
                if tested > MAX_INTERSECTION_PAIRS {
                    break 'cells;
                }
                let pa = mesh.triangle(ta).map(|v| v.as_dvec3());
                let pb = mesh.triangle(tb).map(|v| v.as_dvec3());
                if let Some(seg) = triangle_cut(pa, pb, tol) {
                    out.push((seg, mesh.tri_object[ta]));
                } else if let Some(seg) = triangle_cut(pb, pa, tol) {
                    out.push((seg, mesh.tri_object[tb]));
                }
            }
        }
    }
    out
}

/// Edges of the mesh that do not depend on the camera: creases and open edges that the line list
/// does not have already. The GPU hidden-line mode draws them with the line list.
pub fn feature_edges(scene: &Scene) -> Vec<([Vec3; 2], [u8; 4])> {
    let known: Vec<Edge> = (0..scene.lines.segment_count())
        .map(|s| {
            let [a, b] = scene.lines.segment(s);
            Edge {
                a: a.as_dvec3(),
                b: b.as_dvec3(),
                color: scene.lines.colors[s],
                object: None,
            }
        })
        .collect();
    mesh_edges(scene, None, &known)
        .into_iter()
        .map(|e| ([e.a.as_vec3(), e.b.as_vec3()], e.color))
        .collect()
}

/// A triangle prepared as a cover.
struct Cover {
    v0: DVec3,
    normal: DVec3,
    /// Sign that makes `normal . (P - v0)` positive on the eye side.
    eye_sign: f64,
    /// Unit normals of the three side planes, turned to point inside, and a point on each.
    sides: [(DVec3, DVec3); 3],
    min: [f64; 2],
    max: [f64; 2],
}

/// Cuts `lo..hi` by the condition `g0 + (g1 - g0) t > 0`. Returns false when nothing is left.
fn cut(lo: &mut f64, hi: &mut f64, g0: f64, g1: f64) -> bool {
    let d = g1 - g0;
    if d.abs() < 1e-12 {
        return g0 > 0.0;
    }
    let t = -g0 / d;
    if d > 0.0 {
        *lo = lo.max(t);
    } else {
        *hi = hi.min(t);
    }
    *lo < *hi
}

/// Computes the visible edges of the scene.
pub fn hidden_lines(
    scene: &Scene,
    camera: &Camera,
    width: u32,
    height: u32,
    opts: &HlOptions,
) -> HiddenLines {
    let (width, height) = (width.max(1), height.max(1));
    let mut result = HiddenLines {
        width,
        height,
        segments: Vec::new(),
    };
    if scene.is_empty() {
        return result;
    }
    let aspect = width as f32 / height as f32;
    let mvp = camera.view_proj(aspect, scene.bounds);
    let ortho = camera.projection == Projection::Orthographic;
    let eye = camera.eye().as_dvec3();
    let dir = camera.forward().as_dvec3();
    let diag = scene
        .bounds
        .map_or(1.0, |(lo, hi)| (hi - lo).length() as f64)
        .max(1.0);
    let tol_depth = 1e-5 * diag + 0.01;
    let tol_side = 1e-5 * diag + 1e-3;

    let screen = |p: DVec3| -> Option<[f64; 2]> {
        let c = mvp * p.as_vec3().extend(1.0);
        (c.w > 0.0).then(|| {
            [
                (c.x / c.w * 0.5 + 0.5) as f64 * width as f64,
                (0.5 - c.y / c.w * 0.5) as f64 * height as f64,
            ]
        })
    };

    // Covers.
    let mesh = &scene.mesh;
    let mut covers: Vec<Cover> = Vec::with_capacity(mesh.triangle_count());
    for t in 0..mesh.triangle_count() {
        let p = mesh.triangle(t).map(|v| v.as_dvec3());
        let clips: Vec<Vec4> = p.iter().map(|v| mvp * v.as_vec3().extend(1.0)).collect();
        if clips.iter().any(|c| c.w <= 1e-6 || c.z < 0.0) {
            continue;
        }
        let scr: Vec<[f64; 2]> = p.iter().map(|v| screen(*v).expect("w > 0")).collect();
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        if n.length() < 1e-9 {
            continue;
        }
        let normal = n.normalize();
        let eye_side = if ortho {
            normal.dot(-dir)
        } else {
            normal.dot(eye - p[0])
        };
        if eye_side.abs() < 1e-9 {
            continue;
        }
        let centroid = (p[0] + p[1] + p[2]) / 3.0;
        let mut sides = [(DVec3::ZERO, DVec3::ZERO); 3];
        let mut ok = true;
        for k in 0..3 {
            let (a, b) = (p[k], p[(k + 1) % 3]);
            let mut nk = if ortho {
                (b - a).cross(dir)
            } else {
                (a - eye).cross(b - eye)
            };
            if nk.length() < 1e-12 {
                ok = false;
                break;
            }
            nk = nk.normalize();
            if nk.dot(centroid - a) < 0.0 {
                nk = -nk;
            }
            sides[k] = (nk, a);
        }
        if !ok {
            continue;
        }
        covers.push(Cover {
            v0: p[0],
            normal,
            eye_sign: eye_side.signum(),
            sides,
            min: [
                scr.iter().map(|s| s[0]).fold(f64::MAX, f64::min),
                scr.iter().map(|s| s[1]).fold(f64::MAX, f64::min),
            ],
            max: [
                scr.iter().map(|s| s[0]).fold(f64::MIN, f64::max),
                scr.iter().map(|s| s[1]).fold(f64::MIN, f64::max),
            ],
        });
    }

    // Grid of covers over the image.
    const GRID: usize = 48;
    let (gw, gh) = (width as f64, height as f64);
    let cell = |x: f64, y: f64| -> (usize, usize) {
        (
            ((x / gw * GRID as f64).floor().max(0.0) as usize).min(GRID - 1),
            ((y / gh * GRID as f64).floor().max(0.0) as usize).min(GRID - 1),
        )
    };
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); GRID * GRID];
    for (i, c) in covers.iter().enumerate() {
        if c.max[0] < 0.0 || c.max[1] < 0.0 || c.min[0] > gw || c.min[1] > gh {
            continue;
        }
        let ((x0, y0), (x1, y1)) = (cell(c.min[0], c.min[1]), cell(c.max[0], c.max[1]));
        for y in y0..=y1 {
            for x in x0..=x1 {
                grid[y * GRID + x].push(i as u32);
            }
        }
    }

    // Edges.
    let mut edges: Vec<Edge> = (0..scene.lines.segment_count())
        .map(|s| {
            let [a, b] = scene.lines.segment(s);
            Edge {
                a: a.as_dvec3(),
                b: b.as_dvec3(),
                color: scene.lines.colors[s],
                object: scene.lines.objects.get(s).copied(),
            }
        })
        .collect();
    if opts.mesh_edges {
        let extra = mesh_edges(scene, Some(camera), &edges);
        edges.extend(extra);
    }

    let mut stamp = vec![u32::MAX; covers.len()];
    for (source, e) in edges.iter().enumerate() {
        // Keep the part in front of the near plane.
        let (ca, cb) = (
            mvp * e.a.as_vec3().extend(1.0),
            mvp * e.b.as_vec3().extend(1.0),
        );
        if ca.z < 0.0 && cb.z < 0.0 {
            continue;
        }
        let (mut a, mut b) = (e.a, e.b);
        if ca.z < 0.0 {
            a = e.a + (e.b - e.a) * (ca.z / (ca.z - cb.z)) as f64;
        } else if cb.z < 0.0 {
            b = e.b + (e.a - e.b) * (cb.z / (cb.z - ca.z)) as f64;
        }
        let (Some(sa), Some(sb)) = (screen(a), screen(b)) else {
            continue;
        };
        let ab = b - a;
        let (smin, smax) = (
            [sa[0].min(sb[0]), sa[1].min(sb[1])],
            [sa[0].max(sb[0]), sa[1].max(sb[1])],
        );
        if smax[0] < 0.0 || smax[1] < 0.0 || smin[0] > gw || smin[1] > gh {
            continue;
        }
        let ((x0, y0), (x1, y1)) = (cell(smin[0], smin[1]), cell(smax[0], smax[1]));
        let mut hidden_spans: Vec<(f64, f64)> = Vec::new();
        for gy in y0..=y1 {
            for gx in x0..=x1 {
                for &ci in &grid[gy * GRID + gx] {
                    if stamp[ci as usize] == source as u32 {
                        continue;
                    }
                    stamp[ci as usize] = source as u32;
                    let c = &covers[ci as usize];
                    if c.max[0] < smin[0]
                        || c.min[0] > smax[0]
                        || c.max[1] < smin[1]
                        || c.min[1] > smax[1]
                    {
                        continue;
                    }
                    let (mut lo, mut hi) = (0.0f64, 1.0f64);
                    // Inside the three side planes.
                    let mut inside = true;
                    for (n, q) in &c.sides {
                        let g0 = n.dot(a - *q) + tol_side;
                        let g1 = n.dot(b - *q) + tol_side;
                        if !cut(&mut lo, &mut hi, g0, g1) {
                            inside = false;
                            break;
                        }
                    }
                    if !inside {
                        continue;
                    }
                    // Behind the plane of the triangle, seen from the eye.
                    let g0 = -c.eye_sign * c.normal.dot(a - c.v0) - tol_depth;
                    let g1 = -c.eye_sign * c.normal.dot(b - c.v0) - tol_depth;
                    if cut(&mut lo, &mut hi, g0, g1) && hi - lo > 1e-9 {
                        hidden_spans.push((lo, hi));
                    }
                }
            }
        }
        hidden_spans.sort_by(|x, y| x.partial_cmp(y).expect("finite"));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for s in hidden_spans {
            match merged.last_mut() {
                Some(last) if s.0 <= last.1 + 1e-9 => last.1 = last.1.max(s.1),
                _ => merged.push(s),
            }
        }
        // Visible spans are the gaps; hidden spans follow if asked for.
        let mut pieces: Vec<(f64, f64, bool)> = Vec::new();
        let mut at = 0.0;
        for (s, t) in &merged {
            if *s > at {
                pieces.push((at, *s, false));
            }
            pieces.push((*s, *t, true));
            at = *t;
        }
        if at < 1.0 {
            pieces.push((at, 1.0, false));
        }
        let split = pieces.len() > 1;
        for (s, t, hidden) in pieces {
            if hidden && !opts.include_hidden {
                continue;
            }
            let (Some(p), Some(q)) = (screen(a + ab * s), screen(a + ab * t)) else {
                continue;
            };
            // Pieces that are only a rounding error at a corner are left out.
            if split && (p[0] - q[0]).hypot(p[1] - q[1]) < MIN_PIECE {
                continue;
            }
            result.segments.push(HlSegment {
                a: [p[0] as f32, p[1] as f32],
                b: [q[0] as f32, q[1] as f32],
                hidden,
                color: e.color,
                object: e.object,
                source: source as u32,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use crate::views::StandardView;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::{Attributes, Shape};
    use glam::DVec2;

    /// A cube of 1000 mm: a hardscape slab with a thickness of 1000 mm.
    fn cube_scene(ed: &mut Editor, at: f64) -> ObjectIdPair {
        cube_at(ed, at, 0.0)
    }

    fn cube_at(ed: &mut Editor, at: f64, y: f64) -> ObjectIdPair {
        let pts = [
            DVec2::new(at, y),
            DVec2::new(at + 1000.0, y),
            DVec2::new(at + 1000.0, y + 1000.0),
            DVec2::new(at, y + 1000.0),
        ];
        let id = add(
            ed,
            Shape::Hardscape {
                boundary: cadektor_core::curve::corners(&pts),
                material: "m".into(),
                thickness: Some(1000.0),
                elevation: Some(1000.0),
            },
            None,
        );
        ObjectIdPair(id)
    }
    struct ObjectIdPair(ObjectId);

    fn iso(scene: &Scene, projection: Projection, view: StandardView, w: u32, h: u32) -> Camera {
        let mut cam = Camera::standard(view, projection);
        cam.fit(scene.bounds, w as f32 / h as f32);
        cam
    }

    #[test]
    fn a_cube_in_isometric_view_has_nine_visible_edges() {
        let mut ed = Editor::default();
        cube_scene(&mut ed, 0.0);
        let scene = scene_of(&ed);
        for view in [
            StandardView::RightIsometric,
            StandardView::LeftIsometric,
            StandardView::LeftRearIsometric,
            StandardView::RightRearIsometric,
        ] {
            let cam = iso(&scene, Projection::Orthographic, view, 400, 400);
            for mesh_edges in [false, true] {
                let hl = hidden_lines(
                    &scene,
                    &cam,
                    400,
                    400,
                    &HlOptions {
                        include_hidden: false,
                        mesh_edges,
                    },
                );
                assert_eq!(
                    hl.visible_edge_count(),
                    9,
                    "{view:?} mesh_edges={mesh_edges}"
                );
                assert!(hl.segments.iter().all(|s| !s.hidden));
                // Whole edges: nine segments, none cut in pieces.
                assert_eq!(hl.segments.len(), 9, "{view:?}");
            }
            let all = hidden_lines(
                &scene,
                &cam,
                400,
                400,
                &HlOptions {
                    include_hidden: true,
                    mesh_edges: true,
                },
            );
            // 12 edges: 9 visible and 3 hidden.
            assert_eq!(
                all.segments.iter().filter(|s| s.hidden).count(),
                3,
                "{view:?}"
            );
            assert_eq!(all.visible_edge_count(), 9);
        }
    }

    #[test]
    fn a_perspective_cube_from_above_also_shows_nine_edges() {
        let mut ed = Editor::default();
        cube_scene(&mut ed, 0.0);
        let scene = scene_of(&ed);
        let cam = iso(
            &scene,
            Projection::Perspective,
            StandardView::RightIsometric,
            400,
            400,
        );
        let hl = hidden_lines(&scene, &cam, 400, 400, &HlOptions::default());
        assert_eq!(hl.visible_edge_count(), 9);
    }

    #[test]
    fn top_view_of_a_cube_shows_the_outline_and_top_edges() {
        let mut ed = Editor::default();
        cube_scene(&mut ed, 0.0);
        let scene = scene_of(&ed);
        let cam = iso(
            &scene,
            Projection::Orthographic,
            StandardView::Top,
            400,
            400,
        );
        let hl = hidden_lines(
            &scene,
            &cam,
            400,
            400,
            &HlOptions {
                include_hidden: true,
                mesh_edges: false,
            },
        );
        // The bottom ring hides exactly behind the top ring. The vertical edges are points.
        let visible: Vec<_> = hl.visible().filter(|s| s.length() > 1.0).collect();
        assert_eq!(visible.len(), 4, "{visible:?}");
    }

    #[test]
    fn a_wall_hides_the_edges_behind_it_and_cuts_partly_hidden_ones() {
        let mut ed = Editor::default();
        cube_scene(&mut ed, 0.0);
        // A second cube behind the first one, seen from the front, is partly covered.
        let back = cube_at(&mut ed, 500.0, 2000.0).0;
        let scene = scene_of(&ed);
        let mut cam = Camera::standard(StandardView::Front, Projection::Orthographic);
        cam.fit(scene.bounds, 1.0);
        let hl = hidden_lines(
            &scene,
            &cam,
            400,
            400,
            &HlOptions {
                include_hidden: true,
                mesh_edges: false,
            },
        );
        // Edges are now in the same place on screen for both cubes, shifted by half a cube.
        assert!(hl.segments.iter().any(|s| s.hidden), "something is covered");
        assert!(
            hl.segments
                .iter()
                .any(|s| !s.hidden && s.object == Some(back))
        );
        // A source edge can be cut in a visible and a hidden piece.
        let mut pieces: HashMap<u32, (bool, bool)> = HashMap::new();
        for s in &hl.segments {
            let e = pieces.entry(s.source).or_default();
            if s.hidden { e.1 = true } else { e.0 = true }
        }
        assert!(
            pieces.values().any(|(v, h)| *v && *h),
            "an edge is partly hidden"
        );
    }

    #[test]
    fn output_is_deterministic_and_svg_and_raster_agree() {
        let mut ed = Editor::default();
        cube_scene(&mut ed, 0.0);
        let scene = scene_of(&ed);
        let cam = iso(
            &scene,
            Projection::Orthographic,
            StandardView::RightIsometric,
            200,
            200,
        );
        let opts = HlOptions {
            include_hidden: true,
            mesh_edges: true,
        };
        let a = hidden_lines(&scene, &cam, 200, 200, &opts);
        let b = hidden_lines(&scene, &cam, 200, 200, &opts);
        assert_eq!(a, b);
        let svg = a.to_svg(1.5);
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert_eq!(svg.matches("<line").count(), a.segments.len());
        assert!(svg.contains("stroke-dasharray"));
        // The image has dark lines on white and some gray dashes.
        let img = a.rasterize([255, 255, 255, 255], 1.5);
        let dark = img.rgba.chunks_exact(4).filter(|p| p[0] < 80).count();
        let white = img.rgba.chunks_exact(4).filter(|p| p[0] == 255).count();
        assert!(dark > 200 && white > 200 * 200 / 2, "{dark} {white}");
        assert_eq!(img, a.rasterize([255, 255, 255, 255], 1.5));
    }

    #[test]
    fn plants_get_silhouettes_from_the_mesh() {
        let mut ed = Editor::default();
        add(
            &mut ed,
            Shape::Plant {
                at: DVec2::ZERO,
                species: "Oak".into(),
                spread: 3000.0,
                height: 6000.0,
            },
            None,
        );
        let scene = scene_of(&ed);
        let cam = iso(
            &scene,
            Projection::Orthographic,
            StandardView::RightIsometric,
            300,
            300,
        );
        let plain = hidden_lines(
            &scene,
            &cam,
            300,
            300,
            &HlOptions {
                include_hidden: false,
                mesh_edges: false,
            },
        );
        let full = hidden_lines(&scene, &cam, 300, 300, &HlOptions::default());
        assert!(full.visible_edge_count() > plain.visible_edge_count() + 8);
        assert!(full.visible().any(|s| s.color == DERIVED_EDGE));
    }

    #[test]
    fn feature_edges_leave_out_what_the_line_list_has() {
        let mut ed = Editor::default();
        cube_scene(&mut ed, 0.0);
        let scene = scene_of(&ed);
        // All twelve edges of the cube are in the line list already.
        assert!(feature_edges(&scene).is_empty());
        add(
            &mut ed,
            Shape::Plant {
                at: DVec2::new(5000.0, 0.0),
                species: "Oak".into(),
                spread: 3000.0,
                height: 6000.0,
            },
            None,
        );
        let scene = scene_of(&ed);
        let f = feature_edges(&scene);
        assert!(!f.is_empty());
        assert!(f.iter().all(|(_, c)| *c == DERIVED_EDGE));
    }

    #[test]
    fn empty_scene_and_edges_behind_the_eye() {
        let ed = Editor::default();
        let scene = scene_of(&ed);
        let cam = Camera::standard(StandardView::Top, Projection::Orthographic);
        let hl = hidden_lines(&scene, &cam, 0, 0, &HlOptions::default());
        assert!(hl.segments.is_empty() && hl.width == 1);
        // A line seen from inside a perspective camera that is pushed past it.
        let mut ed = Editor::default();
        ed.execute(&Command::AddObject {
            shape: Shape::Line {
                a: DVec2::new(-1000.0, 0.0),
                b: DVec2::new(1000.0, 0.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        let scene = scene_of(&ed);
        let mut cam = Camera::standard(StandardView::Top, Projection::Perspective);
        cam.fit(scene.bounds, 1.0);
        let hl = hidden_lines(&scene, &cam, 100, 100, &HlOptions::default());
        assert_eq!(hl.segments.len(), 1);
    }

    #[test]
    fn many_plants_stay_fast() {
        let mut ed = Editor::default();
        let positions: Vec<DVec2> = (0..2_000)
            .map(|i| DVec2::new((i % 50) as f64 * 2500.0, (i / 50) as f64 * 2500.0))
            .collect();
        ed.execute(&Command::AddPlants {
            style: None,
            overrides: Default::default(),
            positions,
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        let scene = scene_of(&ed);
        let cam = iso(
            &scene,
            Projection::Orthographic,
            StandardView::RightIsometric,
            1200,
            900,
        );
        let t = std::time::Instant::now();
        let hl = hidden_lines(&scene, &cam, 1200, 900, &HlOptions::default());
        let took = t.elapsed();
        eprintln!(
            "{} triangles, {} segments, {took:?}",
            scene.mesh.triangle_count(),
            hl.segments.len()
        );
        assert!(took < std::time::Duration::from_secs(60), "{took:?}");
        assert!(hl.visible_edge_count() > 2_000);
    }

    #[test]
    fn triangles_that_cut_each_other_give_a_segment() {
        let a = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(0.0, 10.0, 0.0),
        ];
        // A vertical triangle that pokes through the middle of a.
        let b = [
            DVec3::new(2.0, 2.0, -3.0),
            DVec3::new(2.0, 2.0, 3.0),
            DVec3::new(6.0, 2.0, 0.5),
        ];
        let [s, e] = triangle_cut(a, b, 1e-6).unwrap();
        assert!(s.z.abs() < 1e-9 && e.z.abs() < 1e-9);
        assert!((s.y - 2.0).abs() < 1e-9 && (e.y - 2.0).abs() < 1e-9);
        assert!((s.x - e.x).abs() > 1.0);
        // Cut in the other order gives the same line through b, clipped by a.
        assert!(triangle_cut(b, a, 1e-6).is_some());
        // Outside a: the plane is crossed, but not inside the triangle.
        let far = b.map(|p| p + DVec3::new(20.0, 0.0, 0.0));
        assert!(triangle_cut(a, far, 1e-6).is_none());
        // Touching the plane, lying in it, or a degenerate triangle: nothing.
        let touch = [
            DVec3::new(2.0, 2.0, 0.0),
            DVec3::new(3.0, 2.0, 2.0),
            DVec3::new(4.0, 3.0, 1.0),
        ];
        assert!(triangle_cut(a, touch, 1e-6).is_none());
        let flat = a.map(|p| p + DVec3::new(1.0, 1.0, 0.0));
        assert!(triangle_cut(a, flat, 1e-6).is_none());
        let line = [DVec3::ZERO, DVec3::X, DVec3::X * 2.0];
        assert!(triangle_cut(line, b, 1e-6).is_none());
    }

    #[test]
    fn a_cube_sunk_into_the_ground_gets_a_line_where_it_meets_it() {
        let mut ed = Editor::default();
        add(&mut ed, square(5000.0), Some([120, 160, 120, 255]));
        // The cube reaches from z = -500 to z = 500.
        let pts = [
            DVec2::new(-500.0, -500.0),
            DVec2::new(500.0, -500.0),
            DVec2::new(500.0, 500.0),
            DVec2::new(-500.0, 500.0),
        ];
        let id = add(
            &mut ed,
            Shape::Hardscape {
                boundary: cadektor_core::curve::corners(&pts),
                material: "m".into(),
                thickness: Some(1000.0),
                elevation: Some(500.0),
            },
            None,
        );
        let scene = scene_of(&ed);
        let cuts = intersection_edges(&scene);
        assert!(cuts.len() >= 4, "{}", cuts.len());
        for (seg, object) in &cuts {
            assert!(seg.iter().all(|p| p.z.abs() < 1e-3), "{seg:?}");
            // On the outline of the cube.
            assert!(seg.iter().all(|p| {
                let m = p.x.abs().max(p.y.abs());
                (m - 500.0).abs() < 1e-3
            }));
            // Either the cube or the ground is the cut one, depending on which triangle is tested.
            assert!(*object == id || scene.object_bounds(*object).is_some());
        }
        // The same every time.
        assert_eq!(cuts, intersection_edges(&scene));
        // The camera free edges list has them, and they show in a view from the side.
        assert!(feature_edges(&scene).len() >= 4);
        let cam = iso(
            &scene,
            Projection::Orthographic,
            StandardView::RightIsometric,
            400,
            400,
        );
        let with = hidden_lines(&scene, &cam, 400, 400, &HlOptions::default());
        let without = hidden_lines(
            &scene,
            &cam,
            400,
            400,
            &HlOptions {
                include_hidden: false,
                mesh_edges: false,
            },
        );
        let ring = with
            .visible()
            .filter(|s| s.color == DERIVED_EDGE && s.length() > 2.0)
            .count();
        assert!(
            ring >= 2,
            "the ring where the cube meets the ground is visible: {ring}"
        );
        assert!(with.visible_edge_count() > without.visible_edge_count());
    }

    #[test]
    fn objects_that_do_not_touch_or_share_an_object_give_no_cuts() {
        let mut ed = Editor::default();
        cube_scene(&mut ed, 0.0);
        cube_scene(&mut ed, 5000.0);
        assert!(intersection_edges(&scene_of(&ed)).is_empty());
        // One object alone, or an empty scene.
        let mut one = Editor::default();
        cube_scene(&mut one, 0.0);
        assert!(intersection_edges(&scene_of(&one)).is_empty());
        assert!(intersection_edges(&scene_of(&Editor::default())).is_empty());
        // Two cubes that overlap cut each other.
        let mut ed = Editor::default();
        cube_scene(&mut ed, 0.0);
        cube_scene(&mut ed, 500.0);
        assert!(!intersection_edges(&scene_of(&ed)).is_empty());
    }

    #[test]
    fn cut_intervals() {
        let (mut lo, mut hi) = (0.0, 1.0);
        assert!(cut(&mut lo, &mut hi, -1.0, 1.0));
        assert!((lo - 0.5).abs() < 1e-12 && hi == 1.0);
        let (mut lo, mut hi) = (0.0, 1.0);
        assert!(cut(&mut lo, &mut hi, 1.0, -1.0));
        assert!((hi - 0.5).abs() < 1e-12);
        let (mut lo, mut hi) = (0.0, 1.0);
        assert!(!cut(&mut lo, &mut hi, -1.0, -0.5));
        let (mut lo, mut hi) = (0.0, 1.0);
        assert!(cut(&mut lo, &mut hi, 0.5, 0.5));
        assert!(!cut(&mut lo, &mut hi, -0.5, -0.5));
    }
}
