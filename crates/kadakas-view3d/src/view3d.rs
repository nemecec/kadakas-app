//! State and navigation of the 3D view. No GPU and no egui types, so it is easy to test.
//!
//! The view that the user chose (standard view, projection, render mode) lives in
//! `Session::view`, so that an agent sees it and can change it. This module keeps the
//! orbit camera that follows it.
//!
//! Navigation: drag orbits (the Flyover behavior), Shift-drag, Space-drag or middle-drag
//! pans, the wheel and the pinch gesture zoom toward the pointer.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use kadakas_core::mesh::{Bounds3, Scene};
use kadakas_core::model::{Document, ObjectId};
use kadakas_render::look::{Look, TextureCache};
use kadakas_render::{Camera, StandardView, ViewSettings, pick};

use crate::vertex::LineVertex;

/// Radians of orbit per point of drag.
const ORBIT_PER_POINT: f32 = 0.008;

/// Pick distance for lines in points.
pub const PICK_TOLERANCE: f32 = 5.0;

/// Color of the selection box.
const SELECTION: [u8; 4] = [255, 140, 0, 255];

/// The look of the 3D compare scene (REV-03): the ghosts of removed objects in the color of
/// removed objects, see-through with the opacity of the style (the GPU draws them in its blend
/// pass, far to near). The compare window of `kadakas-app` and the tests of the GPU use this one.
pub fn ghost_look(
    t: &kadakas_review::overlay::Tinted,
    style: &kadakas_review::overlay::CompareStyle,
) -> Look {
    use kadakas_review::overlay::Status;
    let ghosts: std::collections::HashSet<ObjectId> = t.ghosts.iter().copied().collect();
    let [r, g, b] = style.color(Status::Removed);
    Look::ghosted(&t.scene, &|id| ghosts.contains(&id), [r, g, b], style.ghost)
}

/// True when the 2D plan canvas shows, not the 3D view.
pub fn is_plan(settings: ViewSettings) -> bool {
    settings.view.is_plan()
}

pub struct View3d {
    pub camera: Camera,
    /// The settings that the camera follows.
    applied: ViewSettings,
    /// Fit the camera to the scene at the next frame.
    fit_pending: bool,
    selection_cache: Option<SelectionCache>,
    overlay_rev: u64,
    look_cache: Option<LookCache>,
    /// How many hardscapes with a pattern have a paving texture left out, for a scene revision.
    untextured: Option<(u64, usize)>,
    texture_cache: TextureCache,
    geometry: Option<(u64, u64)>,
}

/// The materials of the scene for one document revision.
struct LookCache {
    scene_rev: u64,
    textures: bool,
    look: Arc<Look>,
}

struct SelectionCache {
    scene_rev: u64,
    extra_key: u64,
    ids: Vec<ObjectId>,
    lines: Arc<Vec<LineVertex>>,
}

impl Default for View3d {
    fn default() -> Self {
        let settings = ViewSettings {
            view: StandardView::RightIsometric,
            ..ViewSettings::default()
        };
        View3d {
            camera: Camera::standard(settings.view, settings.projection),
            applied: settings,
            fit_pending: true,
            selection_cache: None,
            overlay_rev: 0,
            look_cache: None,
            untextured: None,
            texture_cache: TextureCache::new(),
            geometry: None,
        }
    }
}

impl View3d {
    /// The materials of the scene. They are resolved again only when the document revision or the
    /// texture setting changes, and decoded textures are kept between looks.
    pub fn look(
        &mut self,
        doc: &Document,
        scene: &Scene,
        scene_rev: u64,
        textures: bool,
    ) -> Arc<Look> {
        if let Some(c) = &self.look_cache
            && c.scene_rev == scene_rev
            && c.textures == textures
        {
            return c.look.clone();
        }
        let look = Arc::new(Look::build_cached(
            doc,
            scene,
            textures,
            &mut self.texture_cache,
        ));
        self.look_cache = Some(LookCache {
            scene_rev,
            textures,
            look: look.clone(),
        });
        look
    }

    /// How many hardscapes with a joint pattern have a paving texture left out of the 3D view (see
    /// `kadakas_render::look::patterned_untextured`). Counted once for each revision.
    pub fn untextured(&mut self, doc: &Document, scene_rev: u64) -> usize {
        match self.untextured {
            Some((rev, n)) if rev == scene_rev => n,
            _ => {
                let n = kadakas_render::look::patterned_untextured(doc);
                self.untextured = Some((scene_rev, n));
                n
            }
        }
    }

    /// A fingerprint of the geometry of the scene. An edit that does not change the geometry keeps
    /// it, so the GPU does not upload the mesh or draw the shadow map again.
    pub fn geometry_key(&mut self, scene: &Scene, scene_rev: u64) -> u64 {
        match self.geometry {
            Some((rev, key)) if rev == scene_rev => key,
            _ => {
                let key = kadakas_render::look::scene_key(scene);
                self.geometry = Some((scene_rev, key));
                key
            }
        }
    }

    /// Makes the camera follow `settings`. A change of the standard view turns the camera
    /// to that view and keeps target and zoom. `force` does that even without a change,
    /// as after `view.set`, which also resets an orbit that the user did.
    pub fn sync(&mut self, settings: ViewSettings, force: bool) {
        if settings.view != self.applied.view || force {
            self.camera.set_view(settings.view);
        }
        self.camera.projection = settings.projection;
        self.applied = settings;
    }

    /// Zoom to fit at the next frame.
    pub fn request_fit(&mut self) {
        self.fit_pending = true;
    }

    /// The camera is placed: no fit at the next frame.
    pub fn fit_done(&mut self) {
        self.fit_pending = false;
    }

    pub fn fit_pending(&self) -> bool {
        self.fit_pending
    }

    /// Fits the camera to the box now, for a viewport with this aspect ratio.
    pub fn fit(&mut self, bounds: Option<Bounds3>, aspect: f32) {
        self.camera.fit(bounds, aspect);
        self.fit_pending = false;
    }

    /// Orbit by a drag of `delta` points.
    pub fn orbit(&mut self, delta: Vec2) {
        self.camera
            .orbit(-delta.x * ORBIT_PER_POINT, delta.y * ORBIT_PER_POINT);
    }

    /// Pan by a drag of `delta` points in a viewport `height` points tall.
    pub fn pan(&mut self, delta: Vec2, height: f32) {
        self.camera.pan(delta.x, delta.y, height);
    }

    /// Zoom toward the point `at` (points from the top left of the viewport) of a viewport of `size`.
    pub fn zoom_at(&mut self, factor: f32, at: Vec2, size: Vec2) {
        let ndc = Vec2::new(
            at.x / size.x.max(1.0) * 2.0 - 1.0,
            1.0 - at.y / size.y.max(1.0) * 2.0,
        );
        self.camera
            .zoom_at(factor, ndc, size.x.max(1.0) / size.y.max(1.0));
    }

    /// The object under `at` (points from the top left of the viewport).
    pub fn pick(&self, scene: &Scene, at: Vec2, size: Vec2) -> Option<ObjectId> {
        pick(
            scene,
            &self.camera,
            size.x,
            size.y,
            at.x,
            at.y,
            PICK_TOLERANCE,
        )
    }

    /// Selection boxes as a line list, drawn over the scene. Recomputed only when the
    /// selection or the scene changes. The second value changes whenever the lines do.
    pub fn selection_lines(
        &mut self,
        scene: &Scene,
        scene_rev: u64,
        selection: &[ObjectId],
    ) -> (Arc<Vec<LineVertex>>, u64) {
        self.selection_lines_with(scene, scene_rev, selection, 0, Vec::new)
    }

    /// Like [`View3d::selection_lines`], with more lines of the caller (the hatch of selected
    /// faces). `extra_key` names what the extra lines show: they are made again (by `extra`) only
    /// when the key, the selection or the scene revision changes.
    pub fn selection_lines_with(
        &mut self,
        scene: &Scene,
        scene_rev: u64,
        selection: &[ObjectId],
        extra_key: u64,
        extra: impl FnOnce() -> Vec<LineVertex>,
    ) -> (Arc<Vec<LineVertex>>, u64) {
        let fresh = self.selection_cache.as_ref().is_some_and(|c| {
            c.scene_rev == scene_rev && c.ids == selection && c.extra_key == extra_key
        });
        if !fresh {
            let mut lines = Vec::new();
            for id in selection {
                if let Some((lo, hi)) = scene.object_bounds(*id) {
                    box_lines(lo, hi, &mut lines);
                }
            }
            lines.extend(extra());
            self.selection_cache = Some(SelectionCache {
                scene_rev,
                extra_key,
                ids: selection.to_vec(),
                lines: Arc::new(lines),
            });
            self.overlay_rev += 1;
        }
        let cache = self.selection_cache.as_ref().expect("cache was just set");
        (cache.lines.clone(), self.overlay_rev)
    }
}

/// The 12 edges of a box.
fn box_lines(lo: Vec3, hi: Vec3, out: &mut Vec<LineVertex>) {
    let corner = |i: usize| {
        Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        )
    };
    for i in 0..8usize {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                out.extend(LineVertex::segment(
                    corner(i).to_array(),
                    corner(i | bit).to_array(),
                    SELECTION,
                ));
            }
        }
    }
}

/// Result of a click in the 3D view: the new selection.
pub fn click_selection(current: &[ObjectId], hit: Option<ObjectId>, shift: bool) -> Vec<ObjectId> {
    match (hit, shift) {
        (Some(id), false) => vec![id],
        (Some(id), true) => {
            let mut s = current.to_vec();
            match s.iter().position(|x| *x == id) {
                Some(i) => {
                    s.remove(i);
                }
                None => s.push(id),
            }
            s
        }
        (None, true) => current.to_vec(),
        (None, false) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::mesh::build;
    use kadakas_core::model::{Attributes, Shape};
    use kadakas_render::Projection;

    fn scene_with_square() -> (Scene, ObjectId, Editor) {
        let mut ed = Editor::default();
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::Hardscape {
                    boundary: kadakas_core::curve::corners(&[
                        DVec2::splat(-500.0),
                        DVec2::new(500.0, -500.0),
                        DVec2::splat(500.0),
                        DVec2::new(-500.0, 500.0),
                    ]),
                    material: "m".into(),
                    thickness: None,
                    elevation: None,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        (build(ed.doc()), ObjectId(out.created[0]), ed)
    }

    #[test]
    fn plan_is_the_top_view() {
        assert!(is_plan(ViewSettings::default()));
        assert!(!is_plan(ViewSettings {
            view: StandardView::Front,
            ..Default::default()
        }));
        assert!(
            !is_plan(ViewSettings {
                view: StandardView::Top,
                ..Default::default()
            }),
            "the 3D Top view is not the plan"
        );
    }

    #[test]
    fn camera_follows_the_settings() {
        let mut v = View3d::default();
        let iso = v.camera.eye_dir();
        let mut s = ViewSettings {
            view: StandardView::Front,
            ..Default::default()
        };
        v.sync(s, false);
        assert!((v.camera.eye_dir() - Vec3::NEG_Y).length() < 1e-5);
        // The user orbits; a sync without a change keeps that.
        v.orbit(Vec2::new(30.0, 0.0));
        let orbited = v.camera.eye_dir();
        assert!((orbited - Vec3::NEG_Y).length() > 0.01);
        v.sync(s, false);
        assert_eq!(v.camera.eye_dir(), orbited);
        // A forced sync (view.set to the same view) goes back to the standard view.
        v.sync(s, true);
        assert!((v.camera.eye_dir() - Vec3::NEG_Y).length() < 1e-5);
        // Projection follows too, and zoom is kept when the view changes.
        let d = v.camera.distance;
        s.projection = Projection::Perspective;
        s.view = StandardView::RightIsometric;
        v.sync(s, false);
        assert_eq!(v.camera.projection, Projection::Perspective);
        assert_eq!(v.camera.distance, d);
        assert!((v.camera.eye_dir() - iso).length() < 1e-5);
    }

    #[test]
    fn drag_directions() {
        let mut v = View3d::default();
        v.sync(
            ViewSettings {
                view: StandardView::Front,
                ..Default::default()
            },
            true,
        );
        let right = v.camera.right();
        // Dragging right turns the scene to the right: the camera moves to the left.
        v.orbit(Vec2::new(10.0, 0.0));
        assert!(v.camera.eye_dir().dot(right) < 0.0);
        let pitch = v.camera.pitch;
        v.orbit(Vec2::new(0.0, 10.0));
        assert!(v.camera.pitch > pitch, "dragging down looks from higher up");
        v.orbit(Vec2::new(0.0, 1.0e6));
        assert!((v.camera.pitch - std::f32::consts::FRAC_PI_2).abs() < 1e-6);

        let t = v.camera.target;
        v.pan(Vec2::new(0.0, 0.0), 500.0);
        assert_eq!(v.camera.target, t);
        v.pan(Vec2::new(40.0, 0.0), 500.0);
        assert!(
            (v.camera.target - t).dot(v.camera.right()) < 0.0,
            "the scene follows the drag to the right"
        );
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut v = View3d::default();
        let size = Vec2::new(800.0, 600.0);
        let at = Vec2::new(600.0, 150.0);
        let ndc = Vec2::new(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0);
        let aspect = size.x / size.y;
        let (o, d) = v.camera.ray(ndc, aspect);
        let t = (v.camera.target - o).dot(v.camera.forward()) / d.dot(v.camera.forward());
        let p = o + d * t;
        let dist = v.camera.distance;
        v.zoom_at(1.5, at, size);
        assert!((v.camera.distance - dist / 1.5).abs() < 1e-2);
        let q = v.camera.project(p, aspect, None).unwrap();
        assert!((q.x - ndc.x).abs() < 1e-3 && (q.y - ndc.y).abs() < 1e-3);
        // A zero size does not divide by zero.
        v.zoom_at(1.1, Vec2::ZERO, Vec2::ZERO);
        assert!(v.camera.distance.is_finite());
    }

    #[test]
    fn fit_shows_the_scene_and_clears_the_request() {
        let (scene, _, _) = scene_with_square();
        let mut v = View3d::default();
        assert!(v.fit_pending());
        v.fit(scene.bounds, 1.5);
        assert!(!v.fit_pending());
        let (lo, hi) = scene.bounds.unwrap();
        assert!((v.camera.target - (lo + hi) / 2.0).length() < 1e-3);
        v.request_fit();
        assert!(v.fit_pending());
    }

    #[test]
    fn picking_goes_through_the_core_ray_test() {
        let (scene, id, _) = scene_with_square();
        let mut v = View3d::default();
        v.sync(
            ViewSettings {
                view: StandardView::Right,
                ..Default::default()
            },
            true,
        );
        v.fit(scene.bounds, 4.0 / 3.0);
        // Right view: the slab is a thin strip in the middle of the screen.
        let size = Vec2::new(400.0, 300.0);
        assert_eq!(v.pick(&scene, Vec2::new(200.0, 150.0), size), Some(id));
        assert_eq!(v.pick(&scene, Vec2::new(5.0, 5.0), size), None);
    }

    #[test]
    fn selection_lines_are_cached_by_revision_and_selection() {
        let (scene, id, ed) = scene_with_square();
        let mut v = View3d::default();
        let rev = ed.revision();
        let (empty, r0) = v.selection_lines(&scene, rev, &[]);
        assert!(empty.is_empty());
        let (a, r1) = v.selection_lines(&scene, rev, &[id]);
        assert_eq!(a.len(), 24, "12 edges");
        assert_ne!(r0, r1);
        let (b, r2) = v.selection_lines(&scene, rev, &[id]);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(r1, r2, "unchanged: the GPU keeps its buffer");
        let (_, r3) = v.selection_lines(&scene, rev + 1, &[id]);
        assert_ne!(r2, r3, "a new scene revision recomputes");
        // Unknown objects add nothing.
        let (c, _) = v.selection_lines(&scene, rev + 1, &[ObjectId::from_num(999)]);
        assert!(c.is_empty());
        // The box is the object's box.
        let (lo, hi) = scene.object_bounds(id).unwrap();
        let (d, _) = v.selection_lines(&scene, rev + 2, &[id]);
        for p in d.iter() {
            let q = Vec3::from_array(p.pos);
            assert!(q.cmpge(lo).all() && q.cmple(hi).all());
        }
    }

    #[test]
    fn clicks_change_the_selection() {
        let (a, b) = (ObjectId::from_num(1), ObjectId::from_num(2));
        assert_eq!(click_selection(&[a], Some(b), false), vec![b]);
        assert_eq!(click_selection(&[a], Some(b), true), vec![a, b]);
        assert_eq!(click_selection(&[a, b], Some(a), true), vec![b]);
        assert_eq!(click_selection(&[a], None, false), Vec::<ObjectId>::new());
        assert_eq!(click_selection(&[a], None, true), vec![a]);
    }
}
