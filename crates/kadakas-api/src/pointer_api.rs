//! What the person points at (AGR-10): the pointer of the person over the drawing area, as the
//! agent reads it in the `pointer` field of `view.get`.
//!
//! The host that draws the view (the app) shows the pointer to the session once per frame with
//! [`Session::set_pointer`], as the modeler shows the selected faces and edges (`PartSelection`).
//! The session keeps the last sample and answers from it: `age_ms` is computed when the agent
//! asks, from the time of the last move. In 3D the host gives the ray under the pointer in
//! drawing coordinates. The hit is made here with the code of `mesh.pick`, so the hit and
//! `mesh.pick` of the same ray are one answer.
//!
//! Privacy: the pointer position stays in the session. Only a call of the agent reads it. It
//! is not saved and not sent to collaborators (their cursor is a separate, shared position).

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use glam::{DVec2, DVec3, Vec2};
use kadakas_render::{Camera, Projection};
use serde_json::{Value, json};

use kadakas_core::mesh::Scene;
use kadakas_core::model::{ObjectId, Shape};
use kadakas_core::surface::{EdgeId, FaceId, closest_on_segment};

use crate::Session;
use crate::mesh_api;

/// The width of the zone around an edge, in points of the screen. A hit this close to an edge of
/// its face names the edge too.
pub const EDGE_POINTS: f64 = 6.0;

/// The most `view.get` reads of the pointer that one client gets answered in one second. A client
/// that asks more gets the last answer again.
pub const READS_PER_SECOND: u32 = 60;

/// How long (ms) a read of the pointer counts as recent, for the sign in the status bar.
pub const RECENT_READ_MS: f64 = 10_000.0;

/// The least time (ms) between two lines "read pointer" in the activity panel.
pub const READ_NOTE_MS: f64 = 60_000.0;

/// The most lines "read pointer" that the session keeps.
const READ_NOTES_MAX: usize = 50;

/// The size of a screen point of the view in mm at a distance along the ray, as `constant +
/// per_distance * distance`. A parallel view has only the constant, a perspective view only the
/// part that grows with the distance (measured along the ray, so the host has already turned the
/// depth into a distance).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PixelSize {
    pub constant: f64,
    pub per_distance: f64,
}

impl PixelSize {
    /// The size of a point at a distance.
    pub fn at(&self, distance: f64) -> f64 {
        self.constant + self.per_distance * distance
    }
}

/// What the pointer aims at, as the host knows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Aim {
    /// The plan: the point of the drawing (mm, not snapped) and the object that a click at the
    /// cursor selects (none or one).
    Plan { point: DVec2, ids: Vec<ObjectId> },
    /// The 3D view: the ray under the pointer in drawing coordinates (mm, `direction` is a unit
    /// vector).
    Ray {
        origin: DVec3,
        direction: DVec3,
        pixel: PixelSize,
    },
}

/// The pointer in one frame, as the host shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct PointerInput {
    /// The place in the drawing area, in points from its top left corner (the frame of `canvas`
    /// in `view.get`).
    pub canvas: [f64; 2],
    /// The clock of the host in ms. Only differences are used.
    pub now_ms: f64,
    /// The person moves the view (a drag, the wheel) in this frame.
    pub navigating: bool,
    /// The window of the app has the focus of the system. A window without it may get no moves
    /// of the pointer, so the place can be old.
    pub focused: bool,
    pub aim: Aim,
}

#[derive(Clone, Debug)]
struct Sample {
    input: PointerInput,
    /// When the pointer last moved on the canvas (the host clock, ms).
    moved_ms: f64,
    /// The host showed the sample in the last frame that drew the drawing area. A sample that no
    /// frame renewed is old: the drawing area is not on the screen any more.
    fresh: bool,
}

/// What a ray meets: the nearest surface model (the `hit`), or the nearest other object when it
/// is in front of every surface model (then `hit` is `None`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Pick {
    pub(crate) hit: Option<Value>,
    pub(crate) nearest_object: Option<Value>,
}

/// The pick of a ray, kept for one ray and one revision.
struct Memo {
    revision: u64,
    ray: [f64; 6],
    pick: Pick,
}

/// The box of one object of the scene and where its triangles are (scene coordinates).
struct ObjBox {
    id: ObjectId,
    lo: DVec3,
    hi: DVec3,
    tris: Range<usize>,
}

/// The boxes of the objects of one scene.
struct SceneBoxes {
    revision: u64,
    scene: usize,
    list: Vec<ObjBox>,
}

/// The reads of one client in the current second, and its last answer.
struct ReadRate {
    window_ms: f64,
    count: u32,
    last: Value,
}

/// The pointer of the session.
#[derive(Default)]
pub(crate) struct PointerStore {
    sample: Option<Sample>,
    memo: Option<Memo>,
    boxes: Option<SceneBoxes>,
    /// The person turned the reading off (Preferences).
    disabled: bool,
    /// The reads of each client (the connection of the call).
    rates: BTreeMap<u64, ReadRate>,
    /// When an agent last read the pointer (the clock of the host).
    last_read_ms: Option<f64>,
    /// The times of the reads that the activity panel lists: at most one in a minute.
    notes: Vec<f64>,
}

/// Why `pointer` is `null`.
pub const REASON_NOT_OVER: &str = "not_over_canvas";
pub const REASON_DISABLED: &str = "disabled_by_user";

impl Session {
    /// The host shows the pointer: it is over the drawing area. Call it once per frame, from the
    /// code that draws the area. `age_ms` counts from the last frame in which `canvas` was
    /// another place.
    pub fn set_pointer(&mut self, input: PointerInput) {
        let moved_ms = match &self.pointer.sample {
            Some(old) if old.input.canvas == input.canvas => old.moved_ms,
            _ => input.now_ms,
        };
        self.pointer.sample = Some(Sample {
            input,
            moved_ms,
            fresh: true,
        });
    }

    /// The pointer is not over the drawing area (or the area shows something else): there is no
    /// pointer to read.
    pub fn clear_pointer(&mut self) {
        self.pointer.sample = None;
    }

    /// A frame begins that draws the area: what the last frame showed is old until this frame
    /// shows it again. Call it once per frame, before the drawing area is drawn.
    pub fn pointer_new_frame(&mut self) {
        if let Some(s) = &mut self.pointer.sample {
            s.fresh = false;
        }
    }

    /// The pointer as the host last showed it, or `None` when it is not over the drawing area.
    fn pointer_sample(&self) -> Option<&Sample> {
        self.pointer.sample.as_ref().filter(|s| s.fresh)
    }

    /// The person allows or forbids the reading of the pointer (Preferences, "Agents may see my
    /// pointer"). Forbidden: `view.get` has `pointer` as `null` with the reason `disabled_by_user`.
    /// The status bar and the position box do not depend on it.
    pub fn set_pointer_enabled(&mut self, on: bool) {
        self.pointer.disabled = !on;
    }

    /// True if agents may read the pointer.
    pub fn pointer_enabled(&self) -> bool {
        !self.pointer.disabled
    }

    /// When an agent last read the pointer (the clock of the host, ms). `None`: never.
    pub fn pointer_last_read_ms(&self) -> Option<f64> {
        self.pointer.last_read_ms
    }

    /// True if an agent read the pointer within [`RECENT_READ_MS`] before `now_ms`.
    pub fn pointer_read_recently(&self, now_ms: f64) -> bool {
        self.pointer
            .last_read_ms
            .is_some_and(|t| now_ms - t <= RECENT_READ_MS && now_ms >= t)
    }

    /// The times of the reads that the activity panel lists as "read pointer", oldest first. At
    /// most one read in [`READ_NOTE_MS`] is listed. A read is not a change of the document, so
    /// it is not a step of the activity log: the panel shows these lines apart.
    pub fn pointer_read_notes(&self) -> &[f64] {
        &self.pointer.notes
    }

    /// The pointer is over the plan (not the 3D view, not outside the drawing area).
    pub fn pointer_over_plan(&self) -> bool {
        matches!(
            self.pointer_sample(),
            Some(Sample {
                input: PointerInput {
                    aim: Aim::Plan { .. },
                    ..
                },
                ..
            })
        )
    }

    /// What the ray meets. The nearest surface model is the `hit` of `mesh.pick`. Another object
    /// of the scene (a component, a slab, a site model, a plant) in front of it takes the
    /// place: the hit is then `None` and `nearest_object` names that object. With `pixel`, a ray
    /// less than one point away from the ray of the last pick that had a hit gives that pick
    /// again (the status bar asks on every move of the pointer).
    pub(crate) fn ray_pick(
        &mut self,
        origin: DVec3,
        direction: DVec3,
        pixel: Option<PixelSize>,
    ) -> Pick {
        let revision = self.editor.revision();
        let ray = [
            origin.x,
            origin.y,
            origin.z,
            direction.x,
            direction.y,
            direction.z,
        ];
        if let Some(m) = &self.pointer.memo
            && m.revision == revision
        {
            if m.ray == ray {
                return m.pick.clone();
            }
            if let (Some(px), Some(d)) = (pixel, m.pick.distance()) {
                let old = DVec3::new(m.ray[0], m.ray[1], m.ray[2]);
                let old_dir = DVec3::new(m.ray[3], m.ray[4], m.ray[5]);
                let tol = px.at(d);
                if old.distance(origin) <= tol && old_dir.distance(direction) * d <= tol {
                    return m.pick.clone();
                }
            }
        }
        let surface =
            mesh_api::nearest_hit(self.editor.doc(), &mut self.mesh_query, origin, direction);
        let other = self.nearest_other_object(origin, direction);
        let surface_d = surface.as_ref().and_then(|h| h.get("distance")?.as_f64());
        let pick = match (surface, other) {
            (Some(h), Some(o)) if o.distance >= surface_d.unwrap_or(f64::INFINITY) => Pick {
                hit: Some(h),
                nearest_object: None,
            },
            (_, Some(o)) => Pick {
                hit: None,
                nearest_object: Some(o.json(origin, direction)),
            },
            (hit, None) => Pick {
                hit,
                nearest_object: None,
            },
        };
        self.pointer.memo = Some(Memo {
            revision,
            ray,
            pick: pick.clone(),
        });
        pick
    }

    /// The nearest object of the scene that is not a surface model, along the ray. Objects that
    /// are not displayed or are grey are not met (the person cannot pick them). The test is
    /// exact for triangles. A box around each object keeps most objects from being tried.
    fn nearest_other_object(&mut self, origin: DVec3, direction: DVec3) -> Option<Other> {
        // A drawing of surface models only has nothing else to meet, and the scene (which costs
        // a build after each edit when no window drew it) is not needed.
        let doc = self.editor.doc();
        if doc.objects.values().all(|o| {
            matches!(o.shape, Shape::SurfaceModel(_) | Shape::Guide(_)) || !doc.is_displayed(o)
        }) {
            return None;
        }
        let scene = self.scene();
        let revision = self.editor.revision();
        let key = Arc::as_ptr(&scene) as usize;
        if self
            .pointer
            .boxes
            .as_ref()
            .is_none_or(|b| b.revision != revision || b.scene != key)
        {
            self.pointer.boxes = Some(SceneBoxes {
                revision,
                scene: key,
                list: object_boxes(&scene),
            });
        }
        let boxes = self.pointer.boxes.as_ref()?;
        let doc = self.editor.doc();
        let o = origin - scene.origin;
        let mut best: Option<Other> = None;
        for b in &boxes.list {
            let Some(obj) = doc.object(b.id) else {
                continue;
            };
            if matches!(obj.shape, Shape::SurfaceModel(_))
                || !doc.is_displayed(obj)
                || doc.is_grey(obj)
                || !mesh_api::ray_meets_box(o, direction, b.lo, b.hi)
            {
                continue;
            }
            for t in b.tris.clone() {
                let tri = scene.mesh.triangle(t).map(|p| p.as_dvec3());
                if let Some(d) = ray_triangle(o, direction, &tri)
                    && best.as_ref().is_none_or(|x| d < x.distance)
                {
                    best = Some(Other {
                        id: b.id,
                        kind: obj.shape.kind(),
                        distance: d,
                    });
                }
            }
        }
        best
    }

    /// The point of the drawing (mm) where the ray under the pointer meets a surface model, for
    /// the status bar. `None` when the pointer is not over the 3D view, the person moves the view
    /// (the picture changes faster than a hit is useful), the pointer is in the plan, nothing is
    /// hit, or another object is in front of the model.
    pub fn pointer_hit_point(&mut self) -> Option<[f64; 3]> {
        let (origin, direction, pixel) = match self.pointer_sample() {
            Some(Sample {
                input:
                    PointerInput {
                        navigating: false,
                        aim:
                            Aim::Ray {
                                origin,
                                direction,
                                pixel,
                            },
                        ..
                    },
                ..
            }) => (*origin, *direction, *pixel),
            _ => return None,
        };
        let hit = self.ray_pick(origin, direction, Some(pixel)).hit?;
        let p = hit.get("point")?;
        Some([p[0].as_f64()?, p[1].as_f64()?, p[2].as_f64()?])
    }

    /// Why `pointer_json` is `null`: `None` when it is not.
    pub fn pointer_reason(&self) -> Option<&'static str> {
        if self.pointer.disabled {
            Some(REASON_DISABLED)
        } else if self.pointer_sample().is_none() {
            Some(REASON_NOT_OVER)
        } else {
            None
        }
    }

    /// The `pointer` field of `view.get`: `null` when the pointer is not over the drawing area
    /// or the person forbids the reading (see [`Session::pointer_reason`]). `now_ms` is the clock
    /// of the host, the one that `set_pointer` got. A client gets at most [`READS_PER_SECOND`]
    /// answers in a second. After that it gets its last answer again, not an error.
    pub fn pointer_json(&mut self, now_ms: f64) -> Value {
        if self.pointer_reason().is_some() {
            return Value::Null;
        }
        let conn = self.conn;
        if let Some(r) = self.pointer.rates.get_mut(&conn) {
            if now_ms - r.window_ms >= 1000.0 || now_ms < r.window_ms {
                r.window_ms = now_ms;
                r.count = 0;
            }
            if r.count >= READS_PER_SECOND && !r.last.is_null() {
                return r.last.clone();
            }
            r.count += 1;
        } else {
            self.pointer.rates.insert(
                conn,
                ReadRate {
                    window_ms: now_ms,
                    count: 1,
                    last: Value::Null,
                },
            );
        }
        let answer = self.make_pointer_json(now_ms);
        if let Some(r) = self.pointer.rates.get_mut(&conn) {
            r.last = answer.clone();
        }
        self.note_read(now_ms);
        answer
    }

    /// An agent read the pointer: the sign in the status bar and, once a minute, a line in the
    /// activity panel.
    fn note_read(&mut self, now_ms: f64) {
        self.pointer.last_read_ms = Some(now_ms);
        let due = self
            .pointer
            .notes
            .last()
            .is_none_or(|t| now_ms - t >= READ_NOTE_MS || now_ms < *t);
        if due {
            self.pointer.notes.push(now_ms);
            if self.pointer.notes.len() > READ_NOTES_MAX {
                self.pointer.notes.remove(0);
            }
        }
    }

    fn make_pointer_json(&mut self, now_ms: f64) -> Value {
        let Some(s) = self.pointer_sample().cloned() else {
            return Value::Null;
        };
        let age_ms = (now_ms - s.moved_ms).max(0.0).round();
        let mut out = json!({
            "over_canvas": true,
            "canvas": s.input.canvas,
            "age_ms": age_ms,
            "navigating": s.input.navigating,
            "focused": s.input.focused,
        });
        match &s.input.aim {
            Aim::Plan { point, ids } => {
                let doc = self.editor.doc();
                // An object that an edit removed since the sample is not under the pointer.
                let ids: Vec<ObjectId> = ids
                    .iter()
                    .copied()
                    .filter(|id| doc.object(*id).is_some())
                    .collect();
                out["mode"] = json!("plan");
                out["point"] = json!([point.x, point.y]);
                out["ids"] = json!(ids);
            }
            Aim::Ray {
                origin,
                direction,
                pixel,
            } => {
                out["mode"] = json!("3d");
                out["ray"] = json!({
                    "origin": [origin.x, origin.y, origin.z],
                    "direction": [direction.x, direction.y, direction.z],
                });
                let pick = self.ray_pick(*origin, *direction, None);
                let mut hit = pick.hit;
                if let Some(h) = hit.as_mut()
                    && let Some(e) = self.edge_under(h, *pixel)
                {
                    h["edge"] = json!(e);
                }
                out["hit"] = hit.unwrap_or(Value::Null);
                if let Some(o) = pick.nearest_object {
                    out["nearest_object"] = o;
                }
            }
        }
        out
    }

    /// The edge of the hit face that the hit point is close to, if any.
    fn edge_under(&self, hit: &Value, pixel: PixelSize) -> Option<EdgeId> {
        let model: ObjectId = serde_json::from_value(hit.get("model")?.clone()).ok()?;
        let face = FaceId(u32::try_from(hit.get("face")?.as_u64()?).ok()?);
        let p = hit.get("point")?;
        let at = DVec3::new(p[0].as_f64()?, p[1].as_f64()?, p[2].as_f64()?);
        let tol = EDGE_POINTS * pixel.at(hit.get("distance")?.as_f64()?);
        let m = mesh_api::model_of(self.editor.doc(), model).ok()?;
        let uses: Vec<_> = m
            .face(face)?
            .uses()
            .map(|(a, b)| if a <= b { (a, b) } else { (b, a) })
            .collect();
        let mut best: Option<(f64, EdgeId)> = None;
        for (e, ed) in m.edges() {
            if !uses.contains(&ed.key()) || !m.edge_shown(e) {
                continue;
            }
            let (Some(a), Some(b)) = (m.vertex(ed.v[0]), m.vertex(ed.v[1])) else {
                continue;
            };
            let (a, b) = (
                m.transform.transform_point3(a),
                m.transform.transform_point3(b),
            );
            let d = closest_on_segment(at, a, b).distance(at);
            if d <= tol && best.is_none_or(|(bd, be)| d < bd || (d == bd && e < be)) {
                best = Some((d, e));
            }
        }
        best.map(|(_, e)| e)
    }
}

impl Pick {
    /// The distance of the hit, if there is one.
    fn distance(&self) -> Option<f64> {
        self.hit.as_ref()?.get("distance")?.as_f64()
    }
}

/// An object that is not a surface model, met by a ray.
struct Other {
    id: ObjectId,
    kind: &'static str,
    distance: f64,
}

impl Other {
    fn json(&self, origin: DVec3, direction: DVec3) -> Value {
        let p = origin + direction * self.distance;
        json!({
            "id": self.id,
            "kind": self.kind,
            "distance": self.distance,
            "point": [p.x, p.y, p.z],
        })
    }
}

/// The box around the triangles of each object of the scene (scene coordinates).
fn object_boxes(scene: &Scene) -> Vec<ObjBox> {
    let mut list = Vec::new();
    for id in scene.objects() {
        let Some((tris, _)) = scene.object_ranges(id) else {
            continue;
        };
        if tris.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for t in tris.clone() {
            for p in scene.mesh.triangle(t) {
                let p = p.as_dvec3();
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        if lo.is_finite() && hi.is_finite() {
            // The triangle test has no tolerance, but the scene is f32: pad the box a little.
            let pad = DVec3::splat(1e-3 + 1e-6 * (hi - lo).max_element());
            list.push(ObjBox {
                id,
                lo: lo - pad,
                hi: hi + pad,
                tris,
            });
        }
    }
    list
}

/// The distance along a ray (a unit direction) to a triangle, from either side.
fn ray_triangle(origin: DVec3, dir: DVec3, tri: &[DVec3; 3]) -> Option<f64> {
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

/// The ray under a point of a 3D view, in drawing coordinates. `at` is in points from the top
/// left corner of a view of `size` points. `scene_origin` is the origin of the scene: the camera
/// works in coordinates less this point. A perspective ray starts at the eye. A parallel ray
/// starts on a plane of the view, which can be inside the models, so it starts back by the
/// distance of the camera and `reach` (mm), as the picking of objects does. The size of a
/// point on the screen is given along the ray, so the size at the depth of a hit is the depth
/// over the cosine of the angle between the ray and the view direction.
///
/// The pointer of the person (AGR-10) and the marks of a markup (REV-11) use it, so a mark
/// is picked like the pointer is.
pub fn ray_under(
    camera: &Camera,
    scene_origin: DVec3,
    at: Vec2,
    size: Vec2,
    reach: f32,
) -> Option<Aim> {
    if size.x < 1.0 || size.y < 1.0 || !at.is_finite() {
        return None;
    }
    let aspect = size.x / size.y;
    let ndc = Vec2::new(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0);
    let (mut origin, dir) = camera.ray(ndc, aspect);
    let ortho = camera.projection == Projection::Orthographic;
    if ortho {
        origin -= dir * (camera.distance + reach);
    }
    let direction = dir.as_dvec3().try_normalize()?;
    let pixel = if ortho {
        PixelSize {
            constant: f64::from(camera.half_height() * 2.0 / size.y),
            per_distance: 0.0,
        }
    } else {
        // The size at a depth is depth * 2 tan(fov / 2) / height. The depth of a point at a
        // distance along the ray is distance * cos(angle to the view direction).
        let cos = f64::from(dir.dot(camera.forward())).max(0.0);
        PixelSize {
            constant: 0.0,
            per_distance: f64::from(2.0 * (camera.fov_y / 2.0).tan() / size.y) * cos,
        }
    };
    Some(Aim::Ray {
        origin: scene_origin + origin.as_dvec3(),
        direction,
        pixel,
    })
}
