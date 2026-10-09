//! Picking: which object is under a pixel of the 3D view.

use cadektor_core::mesh::Scene;
use cadektor_core::model::ObjectId;
use glam::{Vec2, Vec3};

use crate::camera::Camera;
use crate::views::Projection;

fn ray_triangle(origin: Vec3, dir: Vec3, tri: [Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (tri[1] - tri[0], tri[2] - tri[0]);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - tri[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

/// The nearest triangle that the ray hits, as (object, distance along the ray).
/// Triangles are hit from both sides.
pub fn pick_ray(scene: &Scene, origin: Vec3, dir: Vec3) -> Option<(ObjectId, f32)> {
    let mut best: Option<(ObjectId, f32)> = None;
    for t in 0..scene.mesh.triangle_count() {
        if let Some(d) = ray_triangle(origin, dir, scene.mesh.triangle(t))
            && best.is_none_or(|(_, b)| d < b)
        {
            best = Some((scene.mesh.tri_object[t], d));
        }
    }
    best
}

/// Distance from `p` to segment `a`-`b` in 2D, and the position along it (0..1).
fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 == 0.0 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
    (p.distance(a + ab * t), t)
}

/// The object under the pixel `(px, py)` (Y down) of a `width` x `height` viewport.
/// A triangle is hit by a ray. A line is hit when it passes within `tol_px` of the pixel.
/// A line wins over a surface behind it, so outlines are easy to pick.
pub fn pick(scene: &Scene, camera: &Camera, width: f32, height: f32, px: f32, py: f32, tol_px: f32) -> Option<ObjectId> {
    if width < 1.0 || height < 1.0 {
        return None;
    }
    let aspect = width / height;
    let ndc = Vec2::new(px / width * 2.0 - 1.0, 1.0 - py / height * 2.0);
    let (mut origin, dir) = camera.ray(ndc, aspect);
    // Parallel rays start on the target plane, which can be inside the scene: step back out of it.
    let reach = scene.bounds.map_or(0.0, |(lo, hi)| (hi - lo).length() + (lo + hi).length());
    if camera.projection == Projection::Orthographic {
        origin -= dir * (camera.distance + reach);
    }
    let (eye, fwd) = (camera.eye(), camera.forward());
    let surface = pick_ray(scene, origin, dir).map(|(id, t)| (id, (origin + dir * t - eye).dot(fwd)));

    // Lines, in screen space.
    let mvp = camera.view_proj(aspect, scene.bounds);
    let to_px = |p: Vec3| -> Option<(Vec2, f32)> {
        let c = mvp * p.extend(1.0);
        (c.w > 1e-6).then(|| (Vec2::new((c.x / c.w * 0.5 + 0.5) * width, (0.5 - c.y / c.w * 0.5) * height), c.w))
    };
    let mut line: Option<(ObjectId, f32, f32)> = None; // object, pixel distance, view depth
    let at = Vec2::new(px, py);
    for s in 0..scene.lines.segment_count() {
        let [a, b] = scene.lines.segment(s);
        let (Some((pa, wa)), Some((pb, wb))) = (to_px(a), to_px(b)) else { continue };
        let (d, t) = seg_dist(at, pa, pb);
        if d <= tol_px && line.is_none_or(|(_, bd, _)| d < bd) {
            let depth = match camera.projection {
                Projection::Perspective => wa + (wb - wa) * t,
                Projection::Orthographic => (a + (b - a) * t - eye).dot(fwd),
            };
            line = Some((scene.lines.objects[s], d, depth));
        }
    }
    match (surface, line) {
        (None, None) => None,
        (Some((id, _)), None) => Some(id),
        (None, Some((id, _, _))) => Some(id),
        (Some((sid, sd)), Some((lid, _, ld))) => Some(if ld <= sd + 1e-4 * reach + 1.0 { lid } else { sid }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use crate::views::StandardView;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::{Attributes, Shape};
    use glam::DVec2;

    #[test]
    fn picking_surfaces_lines_and_empty_space() {
        let mut ed = Editor::default();
        let low = add(&mut ed, Shape::Hardscape { boundary: cadektor_core::curve::corners(&[DVec2::splat(-500.0), DVec2::new(500.0, -500.0), DVec2::splat(500.0), DVec2::new(-500.0, 500.0)]), material: "a".into(), thickness: None, elevation: None }, None);
        let high = add(&mut ed, Shape::Hardscape { boundary: cadektor_core::curve::corners(&[DVec2::splat(-200.0), DVec2::new(200.0, -200.0), DVec2::splat(200.0), DVec2::new(-200.0, 200.0)]), material: "b".into(), thickness: None, elevation: Some(400.0) }, None);
        let line = add(&mut ed, Shape::Line { a: DVec2::new(-800.0, 700.0), b: DVec2::new(800.0, 700.0) }, None);
        let scene = scene_of(&ed);
        let cam = top_ortho(1000.0);
        let at = |x: f32, y: f32| pick(&scene, &cam, 100.0, 100.0, x, y, 3.0);
        assert_eq!(at(50.0, 50.0), Some(high), "the higher slab is nearer");
        assert_eq!(at(35.0, 50.0), Some(low));
        assert_eq!(at(50.0, 15.0), Some(line), "the line is 700 mm from the center: 35 px");
        assert_eq!(at(50.0, 17.5), Some(line), "within the tolerance of the line");
        assert_eq!(at(50.0, 90.0), None);
        assert_eq!(at(2.0, 2.0), None);
        assert_eq!(pick(&scene, &cam, 0.0, 0.0, 0.0, 0.0, 3.0), None);

        // The same hits in perspective.
        let mut pcam = Camera::standard(StandardView::Top, Projection::Perspective);
        pcam.distance = 4000.0;
        assert_eq!(pick(&scene, &pcam, 100.0, 100.0, 50.0, 50.0, 3.0), Some(high));

        // Ray test directly.
        let hit = pick_ray(&scene, Vec3::new(0.0, 0.0, 5000.0), Vec3::NEG_Z).unwrap();
        assert_eq!(hit.0, high);
        assert!((hit.1 - 4600.0).abs() < 1e-2);
        assert!(pick_ray(&scene, Vec3::new(0.0, 0.0, 5000.0), Vec3::Z).is_none());
        assert!(pick_ray(&scene, Vec3::new(9000.0, 0.0, 5000.0), Vec3::NEG_Z).is_none());
        // Parallel to the faces.
        assert!(pick_ray(&scene, Vec3::new(-9000.0, 0.0, 5000.0), Vec3::X).is_none());
        // A line behind a surface does not win.
        let mut ed = Editor::default();
        let slab = add(&mut ed, Shape::Hardscape { boundary: cadektor_core::curve::corners(&[DVec2::splat(-500.0), DVec2::new(500.0, -500.0), DVec2::splat(500.0), DVec2::new(-500.0, 500.0)]), material: "a".into(), thickness: None, elevation: Some(3000.0) }, None);
        ed.execute(&Command::AddLayer { name: "Low".into(), elevation: 0.0, scale: None, kind: None }).unwrap();
        let low_layer = ed.doc().layers.last().unwrap().id;
        ed.execute(&Command::AddObject { shape: Shape::Line { a: DVec2::new(-300.0, 0.0), b: DVec2::new(300.0, 0.0) }, layer: Some(low_layer), class: None, attrs: Attributes::default() }).unwrap();
        let scene = scene_of(&ed);
        assert_eq!(pick(&scene, &top_ortho(1000.0), 100.0, 100.0, 50.0, 50.0, 3.0), Some(slab));
    }
}
