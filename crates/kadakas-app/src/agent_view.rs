//! The agent drives the view and points (AGR-02, AGR-03): the methods of the app.
//!
//! `view.get`, `view.zoom`, `view.pan`, `view.camera`, `view.highlight` and `view.highlight_clear`
//! need the window, so the app answers them (the headless server gives an error that names the
//! app). The logic that needs no window is in `kadakas_ui_core::agent_view`; the drawing of the
//! pointers, the panel and the People row are in `agent_view_ui.rs`.
//!
//! Rules that keep the person in control:
//!
//! - Nothing here makes a command, an undo step or a revision. Pointers are never saved.
//! - The person wins. The canvases tell `NavGuard` when the person drags, zooms or presses a
//!   navigation key. A view change of the agent in that moment is dropped with a conflict error
//!   (`person_navigating`) and nothing moves. A running animation stops at once.
//! - Each view change keeps the view from before it, and Back returns to it (`AgentView::back`).
//!   `view.set` and `saved_view.restore` count as view changes too.

use egui::Rect;
use glam::{DVec2, DVec3, Vec3};
use kadakas_api::{REASON_PERSON_NAVIGATING, RpcError, Session};
use kadakas_core::model::ObjectId;
use kadakas_core::uid::Uid;
use kadakas_render::{Camera, StandardView, ViewSettings};
use kadakas_ui_core::agent_view::{
    ANIM_MS, AgentPresence, Aim, Back, CamAnim, CameraReq, FOV_DEG, HColor, Highlight,
    HighlightReq, HighlightSet, NavGuard, PanReq, PlanAnim, PlanPose, ZoomReq, frame_camera,
    frame_plan, ground_box,
};
use serde_json::{Value, json};

use crate::view::View;
use crate::view3d::{self, View3d};

/// What a canvas tells about the person's navigation in one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct NavSignal {
    /// The person moved the view (drag, wheel, key).
    pub moved: bool,
    /// The person holds a drag on the view.
    pub dragging: bool,
}

/// The view before a change of the agent.
#[derive(Clone, Copy, Debug)]
pub struct Memo {
    settings: ViewSettings,
    view: View,
    camera: Camera,
}

/// The state of the agent in the window.
#[derive(Default)]
pub struct AgentView {
    pub highlights: HighlightSet,
    pub presence: AgentPresence,
    pub nav: NavGuard,
    back: Back<Memo>,
    plan_anim: Option<PlanAnim>,
    cam_anim: Option<CamAnim>,
    /// The pointer that the key "next pointer" showed last.
    pub cycle: Option<u32>,
    /// A pointer was added, changed or taken away (the panel opens).
    pub news: bool,
}

impl AgentView {
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn back_len(&self) -> usize {
        self.back.len()
    }

    pub fn animating(&self) -> bool {
        self.plan_anim.is_some() || self.cam_anim.is_some()
    }

    /// The person navigates: the animation stops, and the agent waits.
    pub fn person_moved(&mut self, signal: NavSignal, now_ms: f64) {
        if signal.moved || signal.dragging {
            self.plan_anim = None;
            self.cam_anim = None;
        }
        if signal.moved {
            self.nav.note(now_ms);
        }
        self.nav.set_dragging(signal.dragging, now_ms);
    }

    /// A new document: the pointers and the views from before are about the old one.
    pub fn reset(&mut self) {
        self.highlights.clear();
        self.back.clear();
        self.plan_anim = None;
        self.cam_anim = None;
        self.presence.forget();
        self.cycle = None;
    }
}

/// What a call needs from the window.
pub struct Env<'a> {
    pub state: &'a mut AgentView,
    pub session: &'a mut Session,
    pub view: &'a mut View,
    pub view3d: &'a mut View3d,
    pub needs_fit: &'a mut bool,
    /// The drawing area (points), once the window has drawn it.
    pub canvas: Option<Rect>,
    /// The size of the window in points.
    pub window: [f32; 2],
    pub pixels_per_point: f32,
    /// Animate the view changes (Preferences).
    pub animate: bool,
    pub now_ms: f64,
}

/// The methods that the app answers.
pub fn is_app_method(method: &str) -> bool {
    matches!(
        method,
        "view.get"
            | "view.zoom"
            | "view.pan"
            | "view.camera"
            | "view.highlight"
            | "view.highlight_clear"
    )
}

/// The calls that change the view through the session: the app keeps the view from before.
pub fn is_session_view_change(method: &str) -> bool {
    matches!(method, "view.set" | "saved_view.restore")
}

fn invalid(e: String) -> RpcError {
    RpcError::invalid_params(e)
}

impl Env<'_> {
    fn plan(&self) -> bool {
        view3d::is_plan(self.session.view)
    }

    fn size(&self) -> Result<DVec2, RpcError> {
        self.canvas
            .map(|r| DVec2::new(f64::from(r.width()), f64::from(r.height())))
            .filter(|s| s.x >= 1.0 && s.y >= 1.0)
            .ok_or_else(|| {
                RpcError::conflict(
                    "the window has not drawn the view yet (no drawing area); try again in a moment",
                )
            })
    }

    /// The plan pose that the view has, or will have when the animation ends.
    fn plan_pose(&self, size: DVec2) -> PlanPose {
        match self.state.plan_anim {
            Some(a) => a.to,
            None => PlanPose::of(*self.view, size),
        }
    }

    /// The camera that the view has, or will have when the animation ends.
    fn camera(&self) -> Camera {
        match self.state.cam_anim {
            Some(a) => a.to,
            None => self.view3d.camera,
        }
    }

    /// The person wins: no change while they navigate.
    fn check_not_busy(&self) -> Result<(), RpcError> {
        if self.state.nav.busy(self.now_ms) {
            return Err(RpcError::conflict(
                "the person is moving the view right now, so this request was dropped and nothing moved. Try again in a moment",
            )
            .with_reason(REASON_PERSON_NAVIGATING));
        }
        Ok(())
    }

    /// The view as it is now (or will be), to go back to.
    fn memo(&self) -> Memo {
        let view = match (self.state.plan_anim, self.size()) {
            (Some(a), Ok(size)) => a.to.view(size),
            _ => *self.view,
        };
        Memo {
            settings: self.session.view,
            view,
            camera: self.camera(),
        }
    }

    fn keep_for_back(&mut self) {
        let memo = self.memo();
        self.state.back.push(memo);
    }

    fn rev(&self) -> u64 {
        self.session.editor.revision()
    }

    /// The objects of `ids` that exist; an error that names the first that do not.
    fn existing(&self, ids: &[Uid]) -> Result<(), RpcError> {
        let doc = self.session.editor.doc();
        let missing: Vec<String> = ids
            .iter()
            .filter(|u| doc.object(ObjectId(**u)).is_none())
            .map(|u| u.to_string())
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let shown: Vec<&str> = missing.iter().take(3).map(String::as_str).collect();
        Err(RpcError::not_found(format!(
            "no object with ID {}{}",
            shown.join(", "),
            if missing.len() > 3 {
                format!(" (and {} more)", missing.len() - 3)
            } else {
                String::new()
            }
        )))
    }

    /// The scene origin: the 3D camera works in scene coordinates, which are the drawing
    /// coordinates (mm) less this point. Agents always see drawing coordinates.
    fn origin(&self) -> DVec3 {
        kadakas_core::mesh::scene_origin(self.session.editor.doc())
    }

    /// The box of an aim in drawing coordinates (mm). A box of the plan gets the height of the
    /// ground in 3D. For objects it is the union of their boxes.
    fn aim_box(&mut self, aim: &Aim, in_3d: bool) -> Result<(DVec3, DVec3), RpcError> {
        match aim {
            Aim::At(p) => Ok((*p, *p)),
            Aim::Box { lo, hi } => {
                let z = if in_3d { self.ground_z() } else { 0.0 };
                Ok((DVec3::new(lo.x, lo.y, z), DVec3::new(hi.x, hi.y, z)))
            }
            Aim::Objects(ids) => {
                self.existing(ids)?;
                let scene = if in_3d {
                    Some(self.session.scene())
                } else {
                    None
                };
                let doc = self.session.editor.doc();
                let mut acc: Option<(DVec3, DVec3)> = None;
                for u in ids {
                    let id = ObjectId(*u);
                    let Some(o) = doc.object(id) else { continue };
                    let b = scene
                        .as_ref()
                        .and_then(|s| {
                            s.object_bounds(id)
                                .filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
                                .map(|(lo, hi)| (s.to_world(lo), s.to_world(hi)))
                        })
                        .or_else(|| {
                            let (lo, hi) = doc.object_bounds(o);
                            (lo.is_finite() && hi.is_finite())
                                .then(|| (lo.extend(0.0), hi.extend(0.0)))
                        });
                    if let Some((lo, hi)) = b {
                        acc = Some(match acc {
                            Some((a, b)) => (a.min(lo), b.max(hi)),
                            None => (lo, hi),
                        });
                    }
                }
                acc.ok_or_else(|| {
                    RpcError::validation("these objects have no place in the drawing")
                })
            }
        }
    }

    /// The height of the ground in the 3D view: the target of the camera.
    fn ground_z(&self) -> f64 {
        f64::from(self.camera().target.z)
    }

    /// Where the agent "is": the middle of an aim, in drawing coordinates.
    fn aim_center(aim_box: (DVec3, DVec3)) -> DVec2 {
        let c = (aim_box.0 + aim_box.1) / 2.0;
        DVec2::new(c.x, c.y)
    }

    // ---------------------------------------------------------------- the plan

    fn go_plan(&mut self, target: PlanPose, size: DVec2) {
        *self.needs_fit = false;
        let from = PlanPose::of(*self.view, size);
        self.state.cam_anim = None;
        if self.animate && (from.center != target.center || from.zoom != target.zoom) {
            self.state.plan_anim = Some(PlanAnim {
                from,
                to: target,
                start_ms: self.now_ms,
                duration_ms: ANIM_MS,
            });
        } else {
            self.state.plan_anim = None;
            *self.view = target.view(size);
        }
    }

    fn go_camera(&mut self, target: Camera) {
        self.view3d.fit_done();
        let from = self.view3d.camera;
        self.state.plan_anim = None;
        if self.animate && from != target {
            self.state.cam_anim = Some(CamAnim {
                from,
                to: target,
                start_ms: self.now_ms,
                duration_ms: ANIM_MS,
            });
        } else {
            self.state.cam_anim = None;
            self.view3d.camera = target;
        }
    }

    /// The answer of a view change: the view that will be shown.
    fn view_answer(&self, clamped: bool) -> Result<Value, RpcError> {
        let mut out = self.state_json(false)?;
        out["clamped"] = json!(clamped);
        Ok(out)
    }

    /// The view as a JSON value. `full` adds what only `view.get` gives.
    fn state_json(&self, full: bool) -> Result<Value, RpcError> {
        let size = self.size()?;
        let plan_mode = self.plan();
        let pose = self.plan_pose(size);
        let cam = self.camera();
        let (lo, hi) = pose.view(size).visible_box(DVec2::ZERO, size);
        let num = |v: f32| f64::from(v);
        let origin = self.origin();
        let triple = |v: Vec3| {
            let w = origin + v.as_dvec3();
            json!([w.x, w.y, w.z])
        };
        let up = |v: Vec3| json!([num(v.x), num(v.y), num(v.z)]);
        let mut plan = json!({
            "center": [pose.center.x, pose.center.y],
            "zoom": pose.zoom,
            "mm_per_px": 1.0 / pose.zoom,
        });
        let mut camera = json!({
            "view": self.session.view.view.id(),
            "projection": cam.projection.id(),
            "render_mode": self.session.view.mode.id(),
            "eye": triple(cam.eye()),
            "target": triple(cam.target),
            "up": up(cam.up()),
            "fov_deg": FOV_DEG,
            "distance": num(cam.distance),
        });
        if plan_mode {
            plan["visible_box"] = json!({"min": [lo.x, lo.y], "max": [hi.x, hi.y]});
        } else {
            let aspect = (size.x / size.y) as f32;
            camera["visible_box"] = match ground_box(&cam, aspect, cam.target.z) {
                Some((lo, hi)) => json!({
                    "min": [lo.x + origin.x, lo.y + origin.y],
                    "max": [hi.x + origin.x, hi.y + origin.y],
                }),
                None => Value::Null,
            };
        }
        let mut out = json!({
            "mode": if plan_mode { "plan" } else { "3d" },
            "plan": plan,
            "camera": camera,
            "animating": self.state.animating(),
        });
        if !full {
            return Ok(out);
        }
        let doc = self.session.editor.doc();
        let named = |id: Uid, name: &str| json!({"id": id.to_string(), "name": name});
        out["window"] = json!({
            "width": self.window[0],
            "height": self.window[1],
            "pixels_per_point": self.pixels_per_point,
        });
        out["canvas"] = json!({"width": size.x, "height": size.y});
        out["plan"]["layer_scale"] = json!(doc.layer(doc.active_layer).map(|l| l.scale));
        out["selection"] = json!(
            self.session
                .selection
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
        );
        out["active_layer"] = doc
            .layer(doc.active_layer)
            .map_or(Value::Null, |l| named(l.id.0, &l.name));
        out["active_class"] = doc
            .class(doc.active_class)
            .map_or(Value::Null, |c| named(c.id.0, &c.name));
        out["visible_layers"] = json!(
            doc.layers
                .iter()
                .filter(|l| l.visible)
                .map(|l| named(l.id.0, &l.name))
                .collect::<Vec<_>>()
        );
        out["visible_classes"] = json!(
            doc.classes
                .iter()
                .filter(|c| c.visible)
                .map(|c| named(c.id.0, &c.name))
                .collect::<Vec<_>>()
        );
        out["highlights"] = json!(
            self.state
                .highlights
                .items()
                .iter()
                .map(highlight_json)
                .collect::<Vec<_>>()
        );
        out["back"] = json!(self.state.back_len());
        Ok(out)
    }

    // ---------------------------------------------------------------- the calls

    /// A zoom. The person's own zoom (a click on a pointer) is not held back by their navigation
    /// and does not move the agent cursor.
    fn zoom(&mut self, params: &Value, by_person: bool) -> Result<Value, RpcError> {
        let req = ZoomReq::parse(params).map_err(invalid)?;
        let size = self.size()?;
        if !by_person {
            self.check_not_busy()?;
        }
        let in_3d = !self.plan();
        let bx = self.aim_box(&req.aim, in_3d)?;
        let mut clamped = false;
        if in_3d {
            let cam = self.camera();
            let o = self.origin();
            // The camera works in scene coordinates.
            let (lo, hi) = ((bx.0 - o).as_vec3(), (bx.1 - o).as_vec3());
            let target = match &req.aim {
                Aim::At(_) => {
                    let mut c = cam;
                    c.target = lo;
                    match req.scale {
                        Some(s) => frame_camera(c, lo, hi, 0.0, Some(s), size),
                        None => c,
                    }
                }
                _ => frame_camera(cam, lo, hi, req.margin, req.scale, size),
            };
            self.keep_for_back();
            self.go_camera(target);
        } else {
            let cur = self.plan_pose(size);
            let (lo, hi) = (DVec2::new(bx.0.x, bx.0.y), DVec2::new(bx.1.x, bx.1.y));
            let target = match &req.aim {
                Aim::At(p) => PlanPose {
                    center: DVec2::new(p.x, p.y),
                    zoom: req.scale.unwrap_or(cur.zoom),
                },
                _ => {
                    let (pose, c) = frame_plan(lo, hi, req.margin, req.scale, size);
                    clamped = c;
                    pose
                }
            };
            self.keep_for_back();
            self.go_plan(target, size);
        }
        if !by_person {
            self.state
                .presence
                .touch(Some(Self::aim_center(bx)), self.now_ms);
        }
        self.view_answer(clamped)
    }

    fn pan(&mut self, params: &Value) -> Result<Value, RpcError> {
        let req = PanReq::parse(params).map_err(invalid)?;
        let size = self.size()?;
        self.check_not_busy()?;
        let center;
        if self.plan() {
            let cur = self.plan_pose(size);
            let target = PlanPose {
                center: match req {
                    PanReq::By(d) => cur.center + DVec2::new(d.x, d.y),
                    PanReq::To(p, _) => p,
                },
                zoom: cur.zoom,
            };
            center = target.center;
            self.keep_for_back();
            self.go_plan(target, size);
        } else {
            let mut cam = self.camera();
            // The camera works in scene coordinates; agents give drawing coordinates.
            let o = self.origin();
            cam.target = match req {
                PanReq::By(d) => cam.target + d.as_vec3(),
                PanReq::To(p, z) => Vec3::new(
                    (p.x - o.x) as f32,
                    (p.y - o.y) as f32,
                    z.map_or(cam.target.z, |z| z as f32),
                ),
            };
            center = DVec2::new(o.x + f64::from(cam.target.x), o.y + f64::from(cam.target.y));
            self.keep_for_back();
            self.go_camera(cam);
        }
        self.state.presence.touch(Some(center), self.now_ms);
        self.view_answer(false)
    }

    fn set_camera(&mut self, params: &Value) -> Result<Value, RpcError> {
        let req = CameraReq::parse(params).map_err(invalid)?;
        self.size()?;
        self.check_not_busy()?;
        let projection = req.projection.unwrap_or(self.session.view.projection);
        let target = req.camera(self.origin(), projection);
        self.keep_for_back();
        let mut settings = self.session.view;
        if settings.view.is_plan() {
            settings.view = StandardView::RightIsometric;
        }
        settings.projection = projection;
        let from = self.camera();
        self.session.view = settings;
        self.view3d.sync(settings, false);
        self.view3d.camera = from;
        self.go_camera(target);
        self.state
            .presence
            .touch(Some(DVec2::new(req.target.x, req.target.y)), self.now_ms);
        self.view_answer(false)
    }

    fn highlight(&mut self, params: &Value) -> Result<Value, RpcError> {
        let req = HighlightReq::parse(params).map_err(invalid)?;
        if let Aim::Objects(ids) = &req.aim {
            self.existing(ids)?;
        }
        let place = match &req.aim {
            Aim::At(p) => Some(DVec2::new(p.x, p.y)),
            Aim::Box { lo, hi } => Some((*lo + *hi) / 2.0),
            Aim::Objects(_) => {
                let plan = self.plan();
                self.aim_box(&req.aim, !plan).ok().map(Self::aim_center)
            }
        };
        let (now, rev) = (self.now_ms, self.rev());
        let id = self
            .state
            .highlights
            .add(req, now, rev)
            .map_err(|e| RpcError::limit(e))?;
        self.state.presence.touch(place, now);
        self.state.news = true;
        Ok(json!({"id": id, "count": self.state.highlights.len()}))
    }

    fn highlight_clear(&mut self, params: &Value) -> Result<Value, RpcError> {
        let cleared = match params.get("id").filter(|v| !v.is_null()) {
            Some(v) => {
                let id = v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| {
                        invalid("id must be the whole number that view.highlight gave".into())
                    })?;
                if !self.state.highlights.remove(id) {
                    return Err(RpcError::not_found(format!("no pointer with id {id}")));
                }
                1
            }
            None => self.state.highlights.clear(),
        };
        self.state.presence.touch(None, self.now_ms);
        Ok(json!({"cleared": cleared}))
    }
}

/// A pointer as JSON, for `view.get`.
pub fn highlight_json(h: &Highlight) -> Value {
    let (kind, ids): (&str, Value) = match &h.aim {
        Aim::Objects(ids) => (
            "objects",
            json!(ids.iter().map(Uid::to_string).collect::<Vec<_>>()),
        ),
        Aim::Box { .. } => ("box", Value::Null),
        Aim::At(_) => ("place", Value::Null),
    };
    let mut out = json!({
        "id": h.id,
        "kind": kind,
        "text": h.text,
        "color": h.color.id(),
        "until": h.until.id(),
    });
    if !ids.is_null() {
        out["ids"] = ids;
    }
    out
}

/// Runs one call of the app. `Err` is the error response for the agent.
pub fn call(env: &mut Env, method: &str, params: Value) -> Result<Value, RpcError> {
    let params = if params.is_null() { json!({}) } else { params };
    let params = kadakas_api::spec::prepare(env.session.editor.doc(), method, params)?;
    match method {
        "view.get" => env.state_json(true),
        "view.zoom" => env.zoom(&params, false),
        "view.pan" => env.pan(&params),
        "view.camera" => env.set_camera(&params),
        "view.highlight" => env.highlight(&params),
        "view.highlight_clear" => env.highlight_clear(&params),
        _ => Err(RpcError::method_not_found(method)),
    }
}

/// The view from before a change through the session (`view.set`, `saved_view.restore`).
pub fn memo_for_session_change(env: &Env) -> Result<Memo, RpcError> {
    env.check_not_busy()?;
    Ok(env.memo())
}

/// Keeps the memo when the call changed the view.
pub fn keep_if_changed(env: &mut Env, memo: Memo) {
    if env.session.has_view_change() {
        env.state.back.push(memo);
        env.state.plan_anim = None;
        env.state.cam_anim = None;
    }
}

/// One frame: ends the pointers that are over and moves the animations on. Gives true while
/// something animates (the window asks for the next frame).
pub fn step(env: &mut Env) -> bool {
    let rev = env.rev();
    let now = env.now_ms;
    let doc = env.session.editor.doc();
    let changed = env
        .state
        .highlights
        .tick(now, rev, &|u| doc.object(ObjectId(u)).is_some());
    if changed {
        env.state.news = true;
    }
    if let Some(size) = env.size().ok() {
        if let Some(a) = env.state.plan_anim {
            let (pose, done) = a.at(now);
            *env.view = pose.view(size);
            if done {
                env.state.plan_anim = None;
            }
        }
    }
    if let Some(a) = env.state.cam_anim {
        let (cam, done) = a.at(now);
        env.view3d.camera = cam;
        if done {
            env.state.cam_anim = None;
        }
    }
    env.state.animating()
}

/// Back: the view from before the last change of the agent. Gives false when there is none.
pub fn go_back(env: &mut Env) -> bool {
    let Some(memo) = env.state.back.pop() else {
        return false;
    };
    env.state.plan_anim = None;
    env.state.cam_anim = None;
    env.session.view = memo.settings;
    *env.view = memo.view;
    env.view3d.sync(memo.settings, false);
    env.view3d.camera = memo.camera;
    env.view3d.fit_done();
    *env.needs_fit = false;
    true
}

/// Shows the next pointer: zooms to it. Gives its ID.
pub fn show_next(env: &mut Env, id: Option<u32>) -> Option<u32> {
    let h = match id {
        Some(id) => env.state.highlights.get(id)?.clone(),
        None => env.state.highlights.next_after(env.state.cycle).cloned()?,
    };
    env.state.cycle = Some(h.id);
    let aim = match &h.aim {
        Aim::At(p) => json!({"at": [p.x, p.y]}),
        Aim::Box { lo, hi } => json!({"box": [lo.x, lo.y, hi.x, hi.y]}),
        Aim::Objects(ids) => json!({"ids": ids.iter().map(Uid::to_string).collect::<Vec<_>>()}),
    };
    env.zoom(&aim, true).is_ok().then_some(h.id)
}

/// The method of a request line, for the lines that may be about the view. A quick look at the
/// text first, so most lines (and big ones) are not parsed twice.
pub fn method_of(line: &str) -> Option<String> {
    if !line.contains("view.") && !line.contains("saved_view") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    Some(v.get("method")?.as_str()?.to_string())
}

/// The error line for a request, with the facts of the error.
pub fn error_line(line: &str, e: &RpcError) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    let id = v.get("id")?.clone();
    Some(kadakas_api::error_response_with(id, e, None, false))
}

/// Answers a request line of the app. `None`: a notification, which gets no answer.
pub fn respond(env: &mut Env, line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    let method = v.get("method")?.as_str()?;
    let params = v.get("params").cloned().unwrap_or(Value::Null);
    let result = call(env, method, params);
    let id = v.get("id")?.clone();
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}).to_string(),
        Err(e) => kadakas_api::error_response_with(id, &e, None, false),
    })
}

/// The color of a pointer as `egui`.
pub fn color32(c: HColor, alpha: u8) -> egui::Color32 {
    let [r, g, b] = c.rgb();
    egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
}
