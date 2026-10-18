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
//! - The person wins. A change of the view that the agent did not make is seen at the start of
//!   each frame (a drag, a tool, zoom to extents, a standard view, the view history). The input of
//!   the frame and a tool in progress count too. The canvases tell `NavGuard` when the person drags, zooms or presses a
//!   navigation key. A view change of the agent in that moment is dropped with a conflict error
//!   (`person_navigating`) and nothing moves. A running animation stops at once.
//! - Each view change keeps the view from before it, and Back returns to it (`AgentView::back`).
//!   `view.set` and `saved_view.restore` count as view changes too.

use egui::Rect;
use glam::{DVec2, DVec3, Vec3};
use kadakas_api::{REASON_PERSON_NAVIGATING, REASON_VIEW_LOCKED, RpcError, Session};
use kadakas_core::model::ObjectId;
use kadakas_core::uid::Uid;
use kadakas_render::{Camera, StandardView, ViewSettings};
use kadakas_ui_core::agent_view::{
    ANIM_MS, AgentPresence, Aim, Back, CamAnim, CameraReq, FOV_DEG, HColor, Highlight,
    HighlightReq, HighlightSet, MERGE_MS, NavGuard, PanReq, PlanAnim, PlanPose, ZoomReq,
    frame_camera, frame_plan, ground_box, ground_height,
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

/// The view before a change of the agent. The 3D camera is kept with the scene origin it had
/// (the camera works in scene coordinates, the origin moves with the drawing), so Back puts it at
/// the same place in the drawing.
#[derive(Clone, Copy, Debug)]
pub struct Memo {
    settings: ViewSettings,
    /// The plan as the middle (drawing coordinates) and the zoom, so Back is right also when the
    /// drawing area moved in the window since (a palette was resized).
    pose: Option<PlanPose>,
    camera: Camera,
    origin: DVec3,
    /// A view change of the session was already waiting for the window when this was taken.
    pending_before: bool,
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
    /// The person turned off "Agent may move my view": view calls fail with `view_locked`.
    pub locked: bool,
    /// The person closed the small panel (it comes back with the next pointer or view change).
    pub panel_hidden: bool,
    /// When the agent last changed the view (ms): changes that follow at once count as one step
    /// for Back.
    last_change_ms: Option<f64>,
    /// The view as the agent left it. If it is different at the start of a frame, the person (or
    /// a command of the person) changed it: the animation stops and the agent waits.
    seen: Option<(View, Camera, ViewSettings)>,
    /// Frames in which a change of the view is expected from a call of the session (`view.set`).
    expect_change: u8,
    /// The view now is the agent's work. False: the person made it (they moved the view, or went
    /// Back), so the next change of the agent keeps it as the protected entry of Back.
    agent_touched: bool,
    /// The next fit of the view to the window is the app's own work (the first fit after the
    /// launch, or after the agent opened a file), so it does not count as the person navigating.
    app_fit_pending: bool,
}

impl AgentView {
    /// The state at the launch of the app: the startup fit is the app's own work.
    pub fn starting() -> Self {
        AgentView {
            app_fit_pending: true,
            ..Default::default()
        }
    }

    /// The window has just fitted the view to the drawing. If the app asked for that fit (the
    /// launch, a file that the agent opened), the change is not the person's. A fit that the
    /// person asked for (zoom to extents) still holds the agent back.
    pub fn fit_applied(&mut self) {
        if std::mem::take(&mut self.app_fit_pending) {
            self.expect_change = self.expect_change.max(3);
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn back_len(&self) -> usize {
        self.back.len()
    }

    pub fn animating(&self) -> bool {
        self.plan_anim.is_some() || self.cam_anim.is_some()
    }

    /// A pointer ends within this time (ms), or the view animates: the window must look again.
    pub fn needs_repaint_within(&self, now_ms: f64) -> Option<f64> {
        use kadakas_ui_core::agent_view::Until;
        self.highlights
            .items()
            .iter()
            .filter_map(|h| match h.until {
                Until::Ms(ms) => Some((h.made_ms + ms as f64 - now_ms).max(0.0)),
                _ => None,
            })
            .reduce(f64::min)
    }

    /// The person navigates: the animation stops, and the agent waits.
    pub fn person_moved(&mut self, signal: NavSignal, now_ms: f64) {
        if signal.moved || signal.dragging {
            self.plan_anim = None;
            self.cam_anim = None;
        }
        if signal.moved {
            self.nav.note(now_ms);
            self.agent_touched = false;
        }
        self.nav.set_dragging(signal.dragging, now_ms);
    }

    /// Keeps the view from before a change of the agent. Changes that follow within a moment
    /// count as one step. A view that the person made is protected against the limit.
    fn keep(&mut self, memo: Memo, now_ms: f64) {
        let merge =
            !self.back.is_empty() && self.last_change_ms.is_some_and(|t| now_ms - t < MERGE_MS);
        if !merge {
            if self.agent_touched {
                self.back.push(memo);
            } else {
                self.back.push_protected(memo);
            }
        }
        self.agent_touched = true;
        self.last_change_ms = Some(now_ms);
        self.panel_hidden = false;
    }

    /// The window is about to fit the view to a new document (the agent opened a file): the
    /// next few frames change the view, and that is not the person's.
    pub fn expect_fit(&mut self) {
        self.expect_change = 6;
        self.app_fit_pending = true;
    }

    /// A new document: the pointers and the views from before are about the old one.
    pub fn reset(&mut self) {
        self.highlights.clear();
        self.back.clear();
        self.plan_anim = None;
        self.cam_anim = None;
        self.presence.forget();
        self.cycle = None;
        self.seen = None;
        self.agent_touched = false;
    }
}

/// What a call needs from the window.
pub struct Env<'a> {
    pub state: &'a mut AgentView,
    pub session: &'a mut Session,
    pub view: &'a mut View,
    pub view3d: &'a mut View3d,
    pub needs_fit: &'a mut bool,
    /// The drawing area (points, in window coordinates), once the window has drawn it.
    pub canvas: Option<Rect>,
    /// The size of the window in points.
    pub window: [f32; 2],
    pub pixels_per_point: f32,
    /// Animate the view changes (Preferences).
    pub animate: bool,
    pub now_ms: f64,
    /// The person's pointer, wheel, pinch or keys act in this frame (see [`input_busy`]).
    pub input_busy: bool,
    /// The person is in the middle of a tool or types in the Data bar.
    pub tool_busy: bool,
}

/// True when the input of this frame is the person navigating or editing: a pointer button down
/// over the drawing (a drag of the view, a tool, a rubber band), the wheel or a pinch over it, or
/// a key. The window reads this before it answers the calls of the agent, so a call that comes
/// in the same frame as the person's input does not win.
pub fn input_busy(ctx: &egui::Context, canvas: Option<Rect>) -> bool {
    ctx.input(|i| {
        let over = canvas
            .zip(i.pointer.hover_pos())
            .is_some_and(|(r, p)| r.contains(p));
        (over && i.pointer.any_down())
            || (over && i.smooth_scroll_delta != egui::Vec2::ZERO)
            || (i.zoom_delta() - 1.0).abs() > 1e-9
            || i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
    })
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

/// The box of an aim in drawing coordinates, and the objects of it that are not shown.
struct AimBox {
    lo: DVec3,
    hi: DVec3,
    hidden: Vec<Uid>,
}

impl Env<'_> {
    fn plan(&self) -> bool {
        view3d::is_plan(self.session.view)
    }

    /// The corner and the size of the drawing area.
    fn frame(&self) -> Result<(DVec2, DVec2), RpcError> {
        self.canvas
            .map(|r| {
                (
                    DVec2::new(f64::from(r.min.x), f64::from(r.min.y)),
                    DVec2::new(f64::from(r.width()), f64::from(r.height())),
                )
            })
            .filter(|(_, s)| s.x >= 1.0 && s.y >= 1.0)
            .ok_or_else(|| {
                RpcError::conflict(
                    "the window has not drawn the view yet (no drawing area); try again in a moment",
                )
            })
    }

    /// The plan pose that the view has, or will have when the animation ends.
    fn plan_pose(&self, min: DVec2, size: DVec2) -> PlanPose {
        match self.state.plan_anim {
            Some(a) => a.to,
            None => PlanPose::of(*self.view, min, size),
        }
    }

    /// The camera that the view has, or will have when the animation ends.
    fn camera(&self) -> Camera {
        match self.state.cam_anim {
            Some(a) => a.to,
            None => self.view3d.camera,
        }
    }

    /// The person wins: no change while they navigate or edit, and none when they locked the view.
    fn check_not_busy(&self) -> Result<(), RpcError> {
        if self.state.locked {
            return Err(RpcError::conflict(
                "the person turned off \"Agent may move my view\", so this request was dropped and nothing moved. Do not try again until the person says so. The lock covers every call that moves the view: view.zoom, view.pan, view.camera, view.set and saved_view.restore. view.get and view.highlight still work",
            )
            .with_reason(REASON_VIEW_LOCKED));
        }
        if self.state.nav.busy(self.now_ms) || self.input_busy || self.tool_busy {
            return Err(RpcError::conflict(
                "the person is moving the view or working in the drawing right now, so this request was dropped and nothing moved. Try again in a moment",
            )
            .with_reason(REASON_PERSON_NAVIGATING));
        }
        Ok(())
    }

    /// The view as it is now (or will be), to go back to.
    fn memo(&self) -> Memo {
        let pose = self
            .frame()
            .ok()
            .map(|(min, size)| self.plan_pose(min, size));
        Memo {
            settings: self.session.view,
            pose,
            camera: self.camera(),
            origin: self.origin(),
            pending_before: self.session.has_view_change(),
        }
    }

    /// Keeps the view from before a change of the agent (see `AgentView::keep`).
    fn keep_for_back(&mut self) {
        let memo = self.memo();
        let now = self.now_ms;
        self.state.keep(memo, now);
    }

    /// Remembers the view as the agent left it.
    fn mark_seen(&mut self) {
        self.state.seen = Some((*self.view, self.view3d.camera, self.session.view));
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

    /// The height (mm) of the ground, or of what stands there, at a point of the drawing: the
    /// highest surface of the 3D scene (site models, hardscapes, layer elevations), else the
    /// elevation of the active layer.
    fn ground_at(&mut self, xy: DVec2) -> f64 {
        let scene = self.session.scene();
        let o = scene.origin;
        let local = glam::Vec2::new((xy.x - o.x) as f32, (xy.y - o.y) as f32);
        let doc = self.session.editor.doc();
        ground_height(&scene, local)
            .map(|z| f64::from(z) + o.z)
            .unwrap_or_else(|| doc.layer(doc.active_layer).map_or(0.0, |l| l.elevation))
    }

    /// The box of an aim in drawing coordinates (mm). A box of the plan and a place without a
    /// height get the height of the ground there. For objects it is the union of the boxes of
    /// those that are shown; the ones that are not (their layer or class is off, a phase hides
    /// them) are listed in `hidden`.
    fn aim_box(&mut self, aim: &Aim, in_3d: bool) -> Result<AimBox, RpcError> {
        match aim {
            Aim::At(p) => {
                let mut p = *p;
                if p.z.is_nan() {
                    p.z = if in_3d {
                        self.ground_at(DVec2::new(p.x, p.y))
                    } else {
                        0.0
                    };
                }
                Ok(AimBox {
                    lo: p,
                    hi: p,
                    hidden: vec![],
                })
            }
            Aim::Box { lo, hi } => {
                let z = if in_3d {
                    self.ground_at((*lo + *hi) / 2.0)
                } else {
                    0.0
                };
                Ok(AimBox {
                    lo: DVec3::new(lo.x, lo.y, z),
                    hi: DVec3::new(hi.x, hi.y, z),
                    hidden: vec![],
                })
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
                let mut hidden = vec![];
                for u in ids {
                    let id = ObjectId(*u);
                    let Some(o) = doc.object(id) else { continue };
                    if !doc.is_displayed(o) {
                        hidden.push(*u);
                        continue;
                    }
                    let b = scene
                        .as_ref()
                        .and_then(|s| {
                            s.object_bounds(id)
                                .filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
                                .map(|(lo, hi)| (s.to_world(lo), s.to_world(hi)))
                        })
                        .or_else(|| {
                            let (lo, hi) = doc.object_bounds(o);
                            (lo.is_finite() && hi.is_finite()).then(|| {
                                let z = doc.layer(o.layer).map_or(0.0, |l| l.elevation);
                                (lo.extend(z), hi.extend(z))
                            })
                        });
                    if let Some((lo, hi)) = b {
                        acc = Some(match acc {
                            Some((a, b)) => (a.min(lo), b.max(hi)),
                            None => (lo, hi),
                        });
                    }
                }
                match acc {
                    Some((lo, hi)) => Ok(AimBox { lo, hi, hidden }),
                    None if !hidden.is_empty() => Err(RpcError::validation(format!(
                        "all {} of these objects are hidden (their layer or class is off, or a phase hides them), so there is nothing to show. Hidden: {}",
                        hidden.len(),
                        id_list(&hidden)
                    ))),
                    None => Err(RpcError::validation(
                        "these objects have no place in the drawing",
                    )),
                }
            }
        }
    }

    /// Where the agent "is": the middle of an aim, in drawing coordinates.
    fn aim_center(b: &AimBox) -> DVec2 {
        let c = (b.lo + b.hi) / 2.0;
        DVec2::new(c.x, c.y)
    }

    // ---------------------------------------------------------------- the plan

    fn go_plan(&mut self, target: PlanPose, min: DVec2, size: DVec2) {
        *self.needs_fit = false;
        let from = PlanPose::of(*self.view, min, size);
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
            *self.view = target.view(min, size);
        }
        self.mark_seen();
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
        self.mark_seen();
    }

    /// The answer of a view change: the view that will be shown.
    fn view_answer(&self, clamped: bool, hidden: &[Uid]) -> Result<Value, RpcError> {
        let mut out = self.state_json(false)?;
        out["clamped"] = json!(clamped);
        if !hidden.is_empty() {
            out["hidden_ids"] = json!(hidden.iter().map(Uid::to_string).collect::<Vec<_>>());
        }
        Ok(out)
    }

    /// The view as a JSON value. `full` adds what only `view.get` gives.
    fn state_json(&self, full: bool) -> Result<Value, RpcError> {
        let (min, size) = self.frame()?;
        let plan_mode = self.plan();
        let pose = self.plan_pose(min, size);
        let cam = self.camera();
        let (lo, hi) = pose.view(min, size).visible_box(min, min + size);
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
        out["view_locked"] = json!(self.state.locked);
        Ok(out)
    }

    // ---------------------------------------------------------------- the calls

    /// A zoom. The person's own zoom (a click on a pointer) is not held back by their navigation,
    /// does not move the agent cursor and does not go on the Back stack of the agent.
    fn zoom(&mut self, params: &Value, by_person: bool) -> Result<Value, RpcError> {
        let req = ZoomReq::parse(params).map_err(invalid)?;
        let (min, size) = self.frame()?;
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
            let (lo, hi) = ((bx.lo - o).as_vec3(), (bx.hi - o).as_vec3());
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
            if !by_person {
                self.keep_for_back();
            }
            self.go_camera(target);
        } else {
            let cur = self.plan_pose(min, size);
            let (lo, hi) = (DVec2::new(bx.lo.x, bx.lo.y), DVec2::new(bx.hi.x, bx.hi.y));
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
            if !by_person {
                self.keep_for_back();
            }
            self.go_plan(target, min, size);
        }
        if !by_person {
            self.state
                .presence
                .touch(Some(Self::aim_center(&bx)), self.now_ms);
        }
        self.view_answer(clamped, &bx.hidden)
    }

    fn pan(&mut self, params: &Value) -> Result<Value, RpcError> {
        let req = PanReq::parse(params).map_err(invalid)?;
        let (min, size) = self.frame()?;
        self.check_not_busy()?;
        let center;
        if self.plan() {
            let cur = self.plan_pose(min, size);
            let target = PlanPose {
                center: match req {
                    PanReq::By(d) => cur.center + DVec2::new(d.x, d.y),
                    PanReq::To(p, _) => p,
                },
                zoom: cur.zoom,
            };
            center = target.center;
            self.keep_for_back();
            self.go_plan(target, min, size);
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
        self.view_answer(false, &[])
    }

    fn set_camera(&mut self, params: &Value) -> Result<Value, RpcError> {
        let req = CameraReq::parse(params).map_err(invalid)?;
        self.frame()?;
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
        self.view_answer(false, &[])
    }

    fn highlight(&mut self, params: &Value) -> Result<Value, RpcError> {
        let mut req = HighlightReq::parse(params).map_err(invalid)?;
        if let Aim::Objects(ids) = &req.aim {
            self.existing(ids)?;
        }
        // The height of the ground at the pointer, so that a box and a place show at the right
        // height in 3D.
        let mut hidden = vec![];
        let in_3d = !self.plan();
        let place = match req.aim.clone() {
            Aim::At(p) => {
                // The height of the ground is looked up only when 3D is shown (it needs the 3D
                // scene); a pointer made in the plan gets it when 3D is shown (see `step`).
                if in_3d && p.z.is_nan() {
                    let b = self.aim_box(&Aim::At(p), true)?;
                    req.aim = Aim::At(b.lo);
                }
                Some(DVec2::new(p.x, p.y))
            }
            Aim::Box { lo, hi } => {
                if in_3d {
                    req.ground = self.ground_at((lo + hi) / 2.0);
                }
                Some((lo + hi) / 2.0)
            }
            Aim::Objects(ids) => {
                let doc = self.session.editor.doc();
                hidden = ids
                    .iter()
                    .copied()
                    .filter(|u| {
                        doc.object(ObjectId(*u))
                            .is_some_and(|o| !doc.is_displayed(o))
                    })
                    .collect();
                let plan = self.plan();
                self.aim_box(&req.aim, !plan)
                    .ok()
                    .map(|b| Self::aim_center(&b))
            }
        };
        let (now, rev) = (self.now_ms, self.rev());
        let id = self
            .state
            .highlights
            .add(req, now, rev)
            .map_err(RpcError::limit)?;
        self.state.presence.touch(place, now);
        self.state.panel_hidden = false;
        let mut out = json!({"id": id, "count": self.state.highlights.len()});
        if !hidden.is_empty() {
            out["hidden_ids"] = json!(hidden.iter().map(Uid::to_string).collect::<Vec<_>>());
            out["note"] = json!(
                "some of these objects are hidden (their layer or class is off, or a phase hides them); nothing is drawn around them, but they are listed for the person"
            );
        }
        Ok(out)
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

fn id_list(ids: &[Uid]) -> String {
    let shown: Vec<String> = ids.iter().take(5).map(Uid::to_string).collect();
    let more = ids.len().saturating_sub(5);
    if more > 0 {
        format!("{} (and {more} more)", shown.join(", "))
    } else {
        shown.join(", ")
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
    detect_person_change(env);
    let params = if params.is_null() { json!({}) } else { params };
    let params = kadakas_api::params::prepare(env.session.editor.doc(), method, params)?;
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

/// The view from before a change through the session (`view.set`, `saved_view.restore`). It is
/// refused when the person navigates or locked the view.
pub fn memo_for_session_change(env: &mut Env) -> Result<Memo, RpcError> {
    detect_person_change(env);
    env.check_not_busy()?;
    Ok(env.memo())
}

/// Keeps the memo when the call changed the view.
pub fn keep_if_changed(env: &mut Env, memo: Memo) {
    let changed = env.session.has_view_change()
        && (!memo.pending_before || env.session.view != memo.settings);
    if changed {
        let now = env.now_ms;
        env.state.keep(memo, now);
        env.state.plan_anim = None;
        env.state.cam_anim = None;
        // The window applies the change in this frame and the next: it is not the person's.
        env.state.expect_change = 3;
    }
}

/// Looks whether somebody else changed the view since the agent left it (a drag, a tool, a view
/// command of the person: zoom to extents, a standard view, the view history, an opened issue,
/// Back). The person wins: the animation stops and the agent waits. This runs before every call
/// of the agent is answered, and at the start of each frame, so a command of the person made in
/// the rest of the last frame is never missed.
pub fn detect_person_change(env: &mut Env) {
    let current = (*env.view, env.view3d.camera, env.session.view);
    if env.state.expect_change > 0 {
        return;
    }
    if env.state.seen.is_some_and(|s| s != current) {
        env.state.plan_anim = None;
        env.state.cam_anim = None;
        env.state.nav.note(env.now_ms);
        env.state.agent_touched = false;
        env.mark_seen();
    }
}

/// One frame: ends the pointers that are over and moves the animations on. Gives true while
/// something animates (the window asks for the next frame).
pub fn step(env: &mut Env) -> bool {
    let rev = env.rev();
    let now = env.now_ms;
    // The person wins over a running animation.
    detect_person_change(env);
    env.state.expect_change = env.state.expect_change.saturating_sub(1);
    let doc = env.session.editor.doc();
    env.state
        .highlights
        .tick(now, rev, &|u| doc.object(ObjectId(u)).is_some());
    if let Ok((min, size)) = env.frame()
        && let Some(a) = env.state.plan_anim
    {
        let (pose, done) = a.at(now);
        *env.view = pose.view(min, size);
        if done {
            env.state.plan_anim = None;
        }
    }
    if let Some(a) = env.state.cam_anim {
        let (cam, done) = a.at(now);
        env.view3d.camera = cam;
        if done {
            env.state.cam_anim = None;
        }
    }
    // Pointers made in the plan get the height of the ground now that 3D is shown.
    if !env.plan() && env.state.highlights.needs_ground() {
        let mut highlights = std::mem::take(&mut env.state.highlights);
        highlights.resolve_ground(&mut |xy| env.ground_at(xy));
        env.state.highlights = highlights;
    }
    env.mark_seen();
    env.state.animating()
}

/// Back: the view from before the last change of the agent. Gives false when there is none.
/// Back is the person's own view command, so it holds the agent back for a moment as a drag does.
pub fn go_back(env: &mut Env) -> bool {
    let Some(memo) = env.state.back.pop() else {
        return false;
    };
    env.state.plan_anim = None;
    env.state.cam_anim = None;
    env.state.nav.note(env.now_ms);
    env.state.last_change_ms = None;
    env.state.agent_touched = false;
    env.session.view = memo.settings;
    if let (Some(pose), Ok((min, size))) = (memo.pose, env.frame()) {
        *env.view = pose.view(min, size);
    }
    env.view3d.sync(memo.settings, false);
    // The camera was kept with the scene origin of that time; the origin may have moved.
    let mut camera = memo.camera;
    camera.target += (memo.origin - env.origin()).as_vec3();
    env.view3d.camera = camera;
    env.view3d.fit_done();
    *env.needs_fit = false;
    env.mark_seen();
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

/// The person's own zoom to a box in drawing mm, for the Activity panel: animated like a click on
/// a pointer, no Back entry of the agent, and the lock "Agent may move my view" does not apply
/// (the person moves their own view). False if the view cannot be framed yet.
pub fn zoom_to_box(env: &mut Env, lo: DVec2, hi: DVec2) -> bool {
    env.zoom(&json!({"box": [lo.x, lo.y, hi.x, hi.y]}), true)
        .is_ok()
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
