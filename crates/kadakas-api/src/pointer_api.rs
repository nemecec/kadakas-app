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

use glam::{DVec2, DVec3};
use serde_json::{Value, json};

use kadakas_core::model::ObjectId;
use kadakas_core::surface::{EdgeId, FaceId, closest_on_segment};

use crate::Session;
use crate::mesh_api;

/// The width of the pointer zone around an edge, in pixels. A hit this close to an edge of its
/// face names the edge too.
pub const EDGE_PX: f64 = 6.0;

/// The size of a pixel of the view in mm at a distance along the ray, as `constant +
/// per_distance * distance`. A parallel view has only the constant, a perspective view only the
/// part that grows with the distance.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PixelSize {
    pub constant: f64,
    pub per_distance: f64,
}

impl PixelSize {
    /// The size of a pixel at a distance.
    pub fn at(&self, distance: f64) -> f64 {
        self.constant + self.per_distance * distance
    }
}

/// What the pointer aims at, as the host knows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Aim {
    /// The plan: the point of the drawing (mm, not snapped) and the object under it (what a click
    /// would select; none or one).
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

/// The hit of a ray with the surface models, kept for one ray and one revision.
struct Memo {
    revision: u64,
    ray: [f64; 6],
    hit: Option<Value>,
}

/// The pointer of the session.
#[derive(Default)]
pub(crate) struct PointerStore {
    sample: Option<Sample>,
    memo: Option<Memo>,
}

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

    /// The nearest hit of the ray with the surface models: the `hit` of `mesh.pick` for it. One
    /// ray is cut once for a revision, so the status bar and the agent share the work.
    fn ray_hit(&mut self, origin: DVec3, direction: DVec3) -> Option<Value> {
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
            && m.ray == ray
        {
            return m.hit.clone();
        }
        let hit = mesh_api::nearest_hit(
            self.editor.doc(),
            &mut self.mesh_query,
            revision,
            origin,
            direction,
        );
        self.pointer.memo = Some(Memo {
            revision,
            ray,
            hit: hit.clone(),
        });
        hit
    }

    /// The point of the drawing (mm) where the ray under the pointer meets a surface model, for
    /// the status bar. `None` when the pointer is not over the 3D view, the person moves the view
    /// (the picture changes faster than a hit is useful), the pointer is in the plan, or nothing
    /// is hit.
    pub fn pointer_hit_point(&mut self) -> Option<[f64; 3]> {
        let (origin, direction) = match self.pointer_sample() {
            Some(Sample {
                input:
                    PointerInput {
                        navigating: false,
                        aim:
                            Aim::Ray {
                                origin, direction, ..
                            },
                        ..
                    },
                ..
            }) => (*origin, *direction),
            _ => return None,
        };
        let hit = self.ray_hit(origin, direction)?;
        let p = hit.get("point")?;
        Some([p[0].as_f64()?, p[1].as_f64()?, p[2].as_f64()?])
    }

    /// The `pointer` field of `view.get`: `null` when the pointer is not over the drawing area.
    /// `now_ms` is the clock of the host, the one that `set_pointer` got.
    pub fn pointer_json(&mut self, now_ms: f64) -> Value {
        let Some(s) = self.pointer_sample().cloned() else {
            return Value::Null;
        };
        let age_ms = (now_ms - s.moved_ms).max(0.0).round();
        let mut out = json!({
            "over_canvas": true,
            "canvas": s.input.canvas,
            "age_ms": age_ms,
            "navigating": s.input.navigating,
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
                let mut hit = self.ray_hit(*origin, *direction);
                if let Some(h) = hit.as_mut() {
                    let edge = self.edge_under(h, *pixel);
                    if let Some(e) = edge {
                        h["edge"] = json!(e);
                    }
                }
                out["hit"] = hit.unwrap_or(Value::Null);
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
        let tol = EDGE_PX * pixel.at(hit.get("distance")?.as_f64()?);
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
