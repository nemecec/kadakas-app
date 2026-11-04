//! What is under the marks of a markup (REV-11): picking at the stored viewpoint.
//!
//! In the plan a point of a mark is a point of the drawing (the center and the zoom of the
//! viewpoint), and the objects there are found with the hit test of the plan (`kadakas_annot::hit`,
//! the code that a click uses, except that a locked layer is met too). In 3D a point of a mark is
//! a ray through the camera of the viewpoint, and the ray is cast with the code of the pointer
//! of the person (AGR-10, `Session::ray_pick`), which is the code of `mesh.pick`.
//!
//! The rules of AGR-10 hold: only what is displayed (`Document::is_displayed`) is met, grey
//! layers and classes are skipped, locked ones are met. The work is bounded: at most
//! [`MAX_SAMPLES`] samples for a mark and [`MAX_RAYS`] samples for a call.
//!
//! The visibility of the document now decides what is met. The viewpoint keeps the visibility
//! of its time, and the answer lists the layers and classes that differ.

use std::collections::{BTreeMap, BTreeSet};

use glam::{DVec2, DVec3, Vec2};
use serde_json::{Value, json};

use kadakas_annot::annot_pick::Prims;
use kadakas_annot::hit::object_hit;
use kadakas_core::issue::ViewSpec;
use kadakas_core::markup::{Mark, MarkKind, MarkupViewpoint, canvas_of};
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::sheets::ViewCamera;
use kadakas_render::Camera;

use crate::{Aim, Session, ray_under};

/// The most samples for one mark.
pub const MAX_SAMPLES: usize = 100;
/// The default number of samples for one mark.
pub const DEFAULT_SAMPLES: usize = 16;
/// The most samples (rays or hit tests) for one call.
pub const MAX_RAYS: usize = 5_000;
/// The distance in points of the canvas within which a plan hit counts.
const PLAN_TOLERANCE_POINTS: f64 = 6.0;
/// The most faces listed for one model.
const MAX_FACES_LISTED: usize = 50;

/// The stored view as the code that picks needs it.
pub(super) enum Frame {
    Plan {
        center: DVec2,
        zoom: f64,
        canvas: [f64; 2],
    },
    View3d {
        camera: Camera,
        origin: DVec3,
        reach: f32,
        canvas: [f64; 2],
    },
}

impl Frame {
    pub(super) fn canvas(&self) -> [f64; 2] {
        match self {
            Frame::Plan { canvas, .. } | Frame::View3d { canvas, .. } => *canvas,
        }
    }
}

/// The saved camera of a 3D viewpoint (drawing coordinates).
pub(super) fn view_camera(vp: &MarkupViewpoint) -> Option<ViewCamera> {
    match &vp.view {
        ViewSpec::Camera {
            target,
            yaw,
            pitch,
            distance,
            perspective,
        } => Some(ViewCamera {
            target: *target,
            yaw: *yaw,
            pitch: *pitch,
            distance: *distance,
            fov: vp.fov_degrees(),
            projection: if *perspective { "perspective" } else { "ortho" }.to_string(),
        }),
        ViewSpec::Plan { .. } => None,
    }
}

/// The camera that looks straight down at a plan viewpoint, parallel, so that the canvas shows
/// the same part of the drawing (drawing coordinates).
pub(super) fn plan_camera(vp: &MarkupViewpoint) -> Option<ViewCamera> {
    let ViewSpec::Plan { center, zoom } = &vp.view else {
        return None;
    };
    let (yaw, pitch) = kadakas_render::StandardView::Top.yaw_pitch();
    let fov = 45f64;
    let half_height = vp.canvas[1] / zoom / 2.0;
    Some(ViewCamera {
        target: [center[0], center[1], 0.0],
        yaw: f64::from(yaw),
        pitch: f64::from(pitch),
        distance: half_height / (fov.to_radians() / 2.0).tan(),
        fov,
        projection: "ortho".to_string(),
    })
}

impl Session {
    pub(super) fn markup_frame(&mut self, vp: &MarkupViewpoint) -> Frame {
        match (&vp.view, view_camera(vp)) {
            (ViewSpec::Plan { center, zoom }, _) => Frame::Plan {
                center: DVec2::new(center[0], center[1]),
                zoom: *zoom,
                canvas: vp.canvas,
            },
            (_, Some(vc)) => {
                let scene = self.scene();
                let reach = scene
                    .bounds
                    .map_or(0.0, |(lo, hi)| (hi - lo).length() + (lo + hi).length());
                Frame::View3d {
                    camera: Camera::from_saved(&vc, scene.origin),
                    origin: scene.origin,
                    reach,
                    canvas: vp.canvas,
                }
            }
            _ => unreachable!("a viewpoint is a plan or a camera"),
        }
    }
}

/// The layers and classes whose visibility now differs from the viewpoint.
pub(super) fn visibility_differences(doc: &Document, vp: &MarkupViewpoint) -> Value {
    let layers: Vec<_> = vp
        .layers
        .iter()
        .filter(|(id, v)| doc.layer(**id).is_some_and(|l| l.visible != **v))
        .map(|(id, _)| *id)
        .collect();
    let classes: Vec<_> = vp
        .classes
        .iter()
        .filter(|(id, v)| doc.class(**id).is_some_and(|c| c.visible != **v))
        .map(|(id, _)| *id)
        .collect();
    json!({
        "matches": layers.is_empty() && classes.is_empty(),
        "layers_differ": layers,
        "classes_differ": classes,
        "note": "Picking and renders use the visibility of the document now.",
    })
}

/// The count of samples a call may still use.
pub(super) struct Budget {
    pub left: usize,
    pub truncated: bool,
}

impl Budget {
    pub fn new() -> Budget {
        Budget {
            left: MAX_RAYS,
            truncated: false,
        }
    }

    fn take(&mut self) -> bool {
        if self.left == 0 {
            self.truncated = true;
            false
        } else {
            self.left -= 1;
            true
        }
    }
}

/// An object that samples met: its kind, the hits and the group that a click on it selects.
type ObjectHits = (&'static str, usize, Option<ObjectId>);

/// What a set of samples met.
#[derive(Default)]
pub(super) struct Sampled {
    samples: usize,
    nothing: usize,
    /// Surface models: the hits and the faces.
    models: BTreeMap<ObjectId, (usize, BTreeSet<u32>)>,
    /// Other objects (plan objects, components, plants): kind and hits.
    objects: BTreeMap<ObjectId, ObjectHits>,
    /// The ids in the order they were met first.
    order: Vec<ObjectId>,
    truncated: bool,
}

impl Sampled {
    fn meet(&mut self, id: ObjectId) {
        if !self.order.contains(&id) {
            self.order.push(id);
        }
    }

    pub fn ids(&self) -> &[ObjectId] {
        &self.order
    }

    pub fn to_json(&self) -> Value {
        let mut models: Vec<(&ObjectId, &(usize, BTreeSet<u32>))> = self.models.iter().collect();
        models.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
        let mut objects: Vec<(&ObjectId, &ObjectHits)> = self.objects.iter().collect();
        objects.sort_by(|a, b| b.1.1.cmp(&a.1.1).then(a.0.cmp(b.0)));
        let mut out = json!({
            "samples": self.samples,
            "nothing": self.nothing,
            "ids": self.order,
            "models": models.iter().map(|(id, (hits, faces))| json!({
                "model": id,
                "hits": hits,
                "faces": faces.iter().take(MAX_FACES_LISTED).collect::<Vec<_>>(),
                "faces_truncated": faces.len() > MAX_FACES_LISTED,
            })).collect::<Vec<_>>(),
            "objects": objects.iter().map(|(id, (kind, hits, selects))| {
                let mut o = json!({"id": id, "kind": kind, "hits": hits});
                if let Some(g) = selects.filter(|g| g != *id) {
                    o["selects"] = json!(g);
                }
                o
            }).collect::<Vec<_>>(),
        });
        if self.truncated {
            out["truncated"] = json!(true);
        }
        out
    }
}

/// The samples of a mark, in relative points.
pub(super) struct Samples {
    /// The points along an open stroke, or inside a closed shape.
    pub points: Vec<[f64; 2]>,
    pub tip: Option<[f64; 2]>,
    pub tail: Option<[f64; 2]>,
}

/// Points along a stroke at equal steps of its length: `n` of them, the first and the last included.
fn along(points: &[[f64; 2]], canvas: [f64; 2], n: usize) -> Vec<[f64; 2]> {
    if points.len() <= 1 || n <= 1 {
        return points.iter().take(1).copied().collect();
    }
    let len =
        |a: [f64; 2], b: [f64; 2]| ((a[0] - b[0]) * canvas[0]).hypot((a[1] - b[1]) * canvas[1]);
    let total: f64 = points.windows(2).map(|w| len(w[0], w[1])).sum();
    if total <= 0.0 {
        return vec![points[0]];
    }
    let mut out = Vec::with_capacity(n);
    let mut seg = 0usize;
    let mut done = 0.0f64;
    for i in 0..n {
        let want = total * i as f64 / (n - 1) as f64;
        while seg + 2 < points.len() && done + len(points[seg], points[seg + 1]) < want {
            done += len(points[seg], points[seg + 1]);
            seg += 1;
        }
        let l = len(points[seg], points[seg + 1]).max(1e-12);
        let t = ((want - done) / l).clamp(0.0, 1.0);
        let (a, b) = (points[seg], points[seg + 1]);
        out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
    }
    out
}

/// Points inside a closed shape on a grid over its box, at most `n`.
fn inside(
    lo: [f64; 2],
    hi: [f64; 2],
    n: usize,
    is_inside: impl Fn([f64; 2]) -> bool,
) -> Vec<[f64; 2]> {
    let k = ((n as f64 * 1.7).sqrt().ceil() as usize).max(2);
    let mut all = Vec::new();
    for j in 0..k {
        for i in 0..k {
            let p = [
                lo[0] + (hi[0] - lo[0]) * (i as f64 + 0.5) / k as f64,
                lo[1] + (hi[1] - lo[1]) * (j as f64 + 0.5) / k as f64,
            ];
            if is_inside(p) {
                all.push(p);
            }
        }
    }
    if all.len() <= n {
        return all;
    }
    (0..n).map(|i| all[i * all.len() / n]).collect()
}

pub(super) fn samples_of(mark: &Mark, canvas: [f64; 2], n: usize) -> Samples {
    let n = n.clamp(1, MAX_SAMPLES);
    let none = Samples {
        points: vec![],
        tip: None,
        tail: None,
    };
    let Some((lo, hi)) = mark.bounds() else {
        return none;
    };
    match &mark.kind {
        MarkKind::Arrow if mark.points.len() == 2 => Samples {
            points: vec![],
            tail: Some(mark.points[0]),
            tip: Some(mark.points[1]),
        },
        MarkKind::Note if mark.points.len() == 1 => Samples {
            points: vec![mark.points[0]],
            ..none
        },
        MarkKind::Rectangle => Samples {
            points: inside(lo, hi, n, |_| true),
            ..none
        },
        MarkKind::Ellipse => {
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let (rx, ry) = (
                ((hi[0] - lo[0]) / 2.0).max(1e-9),
                ((hi[1] - lo[1]) / 2.0).max(1e-9),
            );
            Samples {
                points: inside(lo, hi, n, |p| {
                    ((p[0] - c[0]) / rx).powi(2) + ((p[1] - c[1]) / ry).powi(2) <= 1.0
                }),
                ..none
            }
        }
        MarkKind::Pen | MarkKind::Highlighter if mark.is_closed(canvas) => {
            let poly: Vec<DVec2> = mark.points.iter().map(|p| DVec2::new(p[0], p[1])).collect();
            Samples {
                points: inside(lo, hi, n, |p| {
                    kadakas_core::geom::point_in_polygon(DVec2::new(p[0], p[1]), &poly)
                }),
                ..none
            }
        }
        MarkKind::Pen | MarkKind::Highlighter => Samples {
            points: along(&mark.points, canvas, n.min(mark.points.len().max(1)).max(2)),
            ..none
        },
        _ => none,
    }
}

/// The first point of the mark that gives it a place: the tip of an arrow, the center of a shape,
/// the first point of a stroke, the place of a note.
pub(super) fn anchor_of(mark: &Mark) -> Option<(&'static str, [f64; 2])> {
    let (lo, hi) = mark.bounds()?;
    match &mark.kind {
        MarkKind::Arrow => mark.points.get(1).map(|p| ("tip", *p)),
        MarkKind::Rectangle | MarkKind::Ellipse => {
            Some(("center", [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]))
        }
        MarkKind::Note => mark.points.first().map(|p| ("point", *p)),
        MarkKind::Pen | MarkKind::Highlighter => mark.points.first().map(|p| ("first_point", *p)),
        MarkKind::Other(_) => None,
    }
}

/// The objects of the plan that can be met, with the box of each (drawing mm), the topmost last.
pub(crate) struct PlanIndex {
    items: Vec<(ObjectId, DVec2, DVec2)>,
}

impl PlanIndex {
    fn new(doc: &Document, pk: Prims) -> PlanIndex {
        let items = doc
            .objects
            .values()
            .filter(|o| !matches!(o.shape, Shape::Group { .. } | Shape::Guide(_)))
            .filter(|o| doc.is_pickable(o))
            .filter_map(|o| {
                let (lo, hi) = if matches!(o.shape, Shape::Annotation(_)) {
                    pk.bounds(doc, o)
                } else {
                    doc.object_bounds(o)
                };
                (lo.is_finite() && hi.is_finite()).then_some((o.id, lo, hi))
            })
            .collect();
        PlanIndex { items }
    }
}

impl Session {
    /// The index of the objects of the plan. It is kept for the revision of the document, so a
    /// call with many samples, and the next call, do not read the whole document again.
    pub(super) fn plan_index(&mut self) -> std::sync::Arc<PlanIndex> {
        let revision = self.editor.revision();
        if let Some((r, i)) = &self.markup_index
            && *r == revision
        {
            return i.clone();
        }
        let pk = Prims::cached(&self.annot_cache, revision);
        let index = std::sync::Arc::new(PlanIndex::new(self.editor.doc(), pk));
        self.markup_index = Some((revision, index.clone()));
        index
    }

    /// The objects of the plan at a drawing point, the topmost first.
    fn plan_hits(&self, index: &PlanIndex, at: DVec2, tol: f64) -> Vec<ObjectId> {
        let doc = self.editor.doc();
        let pk = Prims::cached(&self.annot_cache, self.editor.revision());
        index
            .items
            .iter()
            .rev()
            .filter(|(_, lo, hi)| at.cmpge(*lo - tol).all() && at.cmple(*hi + tol).all())
            .filter_map(|(id, _, _)| {
                let o = doc.object(*id)?;
                object_hit(pk, doc, o, at, tol).then_some(*id)
            })
            .collect()
    }

    /// The drawing point of a relative point of a plan view.
    fn plan_point(center: DVec2, zoom: f64, canvas: [f64; 2], rel: [f64; 2]) -> DVec2 {
        DVec2::new(
            center.x + (rel[0] - 0.5) * canvas[0] / zoom,
            center.y - (rel[1] - 0.5) * canvas[1] / zoom,
        )
    }

    /// The ray under a relative point of a 3D view.
    fn view_ray(frame: &Frame, rel: [f64; 2]) -> Option<(DVec3, DVec3)> {
        let Frame::View3d {
            camera,
            origin,
            reach,
            canvas,
        } = frame
        else {
            return None;
        };
        let at = canvas_of(rel, *canvas);
        let aim = ray_under(
            camera,
            *origin,
            Vec2::new(at[0] as f32, at[1] as f32),
            Vec2::new(canvas[0] as f32, canvas[1] as f32),
            *reach,
        )?;
        match aim {
            Aim::Ray {
                origin, direction, ..
            } => Some((origin, direction)),
            Aim::Plan { .. } => None,
        }
    }

    /// What the samples meet.
    pub(super) fn pick_samples(
        &mut self,
        frame: &Frame,
        index: Option<&PlanIndex>,
        points: &[[f64; 2]],
        budget: &mut Budget,
    ) -> Sampled {
        let mut out = Sampled::default();
        for rel in points {
            if !budget.take() {
                out.truncated = true;
                break;
            }
            out.samples += 1;
            match frame {
                Frame::Plan {
                    center,
                    zoom,
                    canvas,
                } => {
                    let at = Self::plan_point(*center, *zoom, *canvas, *rel);
                    let hits = index
                        .map(|i| self.plan_hits(i, at, PLAN_TOLERANCE_POINTS / zoom))
                        .unwrap_or_default();
                    if hits.is_empty() {
                        out.nothing += 1;
                    }
                    let doc = self.editor.doc();
                    for id in hits {
                        let kind = doc.object(id).map_or("object", |o| o.shape.kind());
                        // What a click on the object selects: the group that holds it.
                        let selects = doc.selection_target(id, None);
                        let e = out.objects.entry(id).or_insert((kind, 0, selects));
                        e.1 += 1;
                        out.meet(id);
                    }
                }
                Frame::View3d { .. } => {
                    let Some((origin, direction)) = Self::view_ray(frame, *rel) else {
                        out.nothing += 1;
                        continue;
                    };
                    let pick = self.ray_pick(origin, direction, None);
                    if let Some(hit) = &pick.hit {
                        let model: Option<ObjectId> =
                            serde_json::from_value(hit["model"].clone()).ok();
                        let face = hit["face"].as_u64().map(|f| f as u32);
                        if let Some(model) = model {
                            let e = out.models.entry(model).or_default();
                            e.0 += 1;
                            e.1.extend(face);
                            out.meet(model);
                        }
                    } else if let Some(o) = &pick.nearest_object {
                        let id: Option<ObjectId> = serde_json::from_value(o["id"].clone()).ok();
                        let kind = self
                            .editor
                            .doc()
                            .object(id.unwrap_or_default())
                            .map_or("object", |ob| ob.shape.kind());
                        if let Some(id) = id {
                            out.objects.entry(id).or_insert((kind, 0, None)).1 += 1;
                            out.meet(id);
                        }
                    } else {
                        out.nothing += 1;
                    }
                }
            }
        }
        out
    }

    /// The first surface or ground point behind a relative point, in drawing coordinates.
    pub(super) fn hint_at(
        &mut self,
        frame: &Frame,
        index: Option<&PlanIndex>,
        at: &str,
        rel: [f64; 2],
        budget: &mut Budget,
    ) -> Value {
        if !budget.take() {
            budget.truncated = true;
            return json!({"at": at, "kind": "skipped", "reason": "the sample budget of the call is used"});
        }
        match frame {
            Frame::Plan {
                center,
                zoom,
                canvas,
            } => {
                let p = Self::plan_point(*center, *zoom, *canvas, rel);
                let ids = index
                    .map(|i| self.plan_hits(i, p, PLAN_TOLERANCE_POINTS / zoom))
                    .unwrap_or_default();
                json!({"at": at, "kind": "plan", "point": [p.x, p.y], "ids": ids})
            }
            Frame::View3d { .. } => {
                let Some((origin, direction)) = Self::view_ray(frame, rel) else {
                    return json!({"at": at, "kind": "none"});
                };
                let pick = self.ray_pick(origin, direction, None);
                if let Some(hit) = pick.hit {
                    return json!({
                        "at": at, "kind": "surface", "point": hit["point"],
                        "model": hit["model"], "face": hit["face"], "distance": hit["distance"],
                    });
                }
                if let Some(o) = pick.nearest_object {
                    return json!({
                        "at": at, "kind": "object", "point": o["point"], "id": o["id"],
                        "object_kind": o["kind"], "distance": o["distance"],
                    });
                }
                // Nothing is behind the point: the ground plane (z = 0) of the drawing.
                if direction.z < -1e-9 {
                    let t = -origin.z / direction.z;
                    if t > 0.0 {
                        let p = origin + direction * t;
                        return json!({
                            "at": at, "kind": "ground", "point": [p.x, p.y, 0.0], "distance": t,
                        });
                    }
                }
                json!({"at": at, "kind": "none"})
            }
        }
    }

    /// `under` and `hint3d` of a mark.
    pub(super) fn mark_picks(
        &mut self,
        frame: &Frame,
        index: Option<&PlanIndex>,
        mark: &Mark,
        samples: usize,
        budget: &mut Budget,
    ) -> (Value, Value) {
        let canvas = frame.canvas();
        let s = samples_of(mark, canvas, samples);
        let mut under = if s.tip.is_some() {
            Value::Object(Default::default())
        } else {
            self.pick_samples(frame, index, &s.points, budget).to_json()
        };
        if let (Some(tip), Some(tail)) = (s.tip, s.tail) {
            let tip_hit = self.pick_samples(frame, index, &[tip], budget);
            let tail_hit = self.pick_samples(frame, index, &[tail], budget);
            let mut ids = tip_hit.ids().to_vec();
            for id in tail_hit.ids() {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
            under = json!({"tip": tip_hit.to_json(), "tail": tail_hit.to_json(), "ids": ids});
        }
        let hint = match anchor_of(mark) {
            Some((at, rel)) => self.hint_at(frame, index, at, rel, budget),
            None => Value::Null,
        };
        (under, hint)
    }
}
