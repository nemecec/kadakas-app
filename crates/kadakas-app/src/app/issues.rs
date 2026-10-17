//! The glue between the Issues UI and the app (REV-05): what needs the view of the app, the
//! selection and the status line. The window code is in `issues_ui.rs`, the logic in
//! `kadakas_ui_core::issues`.

use glam::DVec2;
use kadakas_core::command::Command;
use kadakas_core::issue::{IssueId, ViewSpec, ViewpointId};
use kadakas_l10n::tr;
use kadakas_render::{Projection, StandardView};
use kadakas_ui_core::issues::{self as logic, Draft};
use serde_json::{Value, json};

use super::KadakasApp;
use crate::issues_ui::{Request, create_params, now_ms};

fn d(p: egui::Pos2) -> DVec2 {
    DVec2::new(f64::from(p.x), f64::from(p.y))
}

impl KadakasApp {
    /// Does what the Issues UI asked for. They need the view, which the app owns.
    pub(super) fn run_issue_requests(&mut self) {
        for r in self.issues_ui.take_requests() {
            match r {
                Request::Open(id, vp) => self.open_issue(id, vp),
                Request::Create(draft) => self.create_issue(draft),
                Request::AddViewpoint(id) => self.add_issue_viewpoint(id),
            }
        }
    }

    /// The rectangle of the plan, for the plan view of a viewpoint.
    fn plan_rect(&self) -> (DVec2, DVec2) {
        match self.canvas_rect {
            Some(r) => (d(r.min), d(r.max)),
            None => (DVec2::ZERO, DVec2::new(800.0, 600.0)),
        }
    }

    /// The view that is shown now, as the view of a viewpoint.
    pub(super) fn current_view_spec(&self) -> ViewSpec {
        if self.plan() {
            let (lo, hi) = self.plan_rect();
            logic::plan_spec(&self.view, lo, hi)
        } else {
            let c = &self.view3d.camera;
            // The camera works in scene coordinates (relative to the middle of the drawing).
            // A viewpoint keeps drawing coordinates, so it does not drift when the drawing
            // changes, and a BCF file has the right place.
            let o = kadakas_core::mesh::scene_origin(self.session.editor.doc());
            ViewSpec::Camera {
                target: [
                    o.x + f64::from(c.target.x),
                    o.y + f64::from(c.target.y),
                    o.z + f64::from(c.target.z),
                ],
                yaw: f64::from(c.yaw),
                pitch: f64::from(c.pitch),
                distance: f64::from(c.distance),
                perspective: c.projection == Projection::Perspective,
            }
        }
    }

    /// The params of `render.png` for a snapshot of the view that is shown.
    fn snapshot_options(&self) -> Value {
        let v = self.session.view;
        let name = if v.view.is_plan() {
            StandardView::Top.id()
        } else {
            v.view.id()
        };
        json!({
            "view": name,
            "projection": if v.projection == Projection::Perspective { "perspective" } else { "ortho" },
            "width": 640,
            "height": 480,
        })
    }

    /// A call that changes the document: not while the drawing is read only or waits for the
    /// first snapshot of a shared document.
    fn issue_call(&mut self, method: &str, params: Value) -> Option<Value> {
        if self.read_only() {
            self.status = tr!("compare-ui-read-only");
            return None;
        }
        if self.collab.waiting_for_snapshot() {
            self.status = tr!("collab-ui-joining");
            return None;
        }
        match self.session.handle(method, params) {
            Ok(v) => Some(v),
            Err(e) => {
                self.status = tr!("issues-ui-error", e = e.message);
                None
            }
        }
    }

    fn create_issue(&mut self, draft: Draft) {
        let view = Some(self.current_view_spec());
        let snapshot = Some(self.snapshot_options());
        let params = create_params(&draft, view, snapshot);
        match self.issue_call("issue.create", params) {
            Some(r) => {
                let title = draft.title.trim().to_string();
                self.status = tr!("issues-ui-created", title = title);
                self.issues_ui.selected = serde_json::from_value(r["id"].clone()).ok();
                crate::annot_ui::set_revision(self.session.editor.revision());
            }
            None => {
                let why = self.status.clone();
                self.issues_ui.reopen(draft, why);
            }
        }
    }

    fn add_issue_viewpoint(&mut self, id: IssueId) {
        let selection = self.session.selection.clone();
        let mut p = json!({
            "id": id,
            "view": self.current_view_spec(),
            "snapshot": self.snapshot_options(),
            "now_ms": now_ms(),
        });
        if !selection.is_empty() {
            p["selection"] = json!(selection);
        }
        self.issue_call("issue.add_viewpoint", p);
    }

    /// Opens an issue: restores the layers and classes of its viewpoint, selects its objects and
    /// goes to its view.
    pub(super) fn open_issue(&mut self, id: IssueId, viewpoint: Option<ViewpointId>) {
        let doc = self.session.editor.doc();
        let Some(issue) = doc.issues.get(id).cloned() else {
            return;
        };
        let plan = logic::open_plan(doc, &issue, logic::viewpoint_of(&issue, viewpoint));
        if !plan.commands.is_empty() && !self.read_only() {
            self.exec(&Command::Batch {
                commands: plan.commands.clone(),
            });
        }
        if !self.read_only() {
            self.session.selection = plan.selection.clone();
        }
        match &plan.view {
            Some(spec @ ViewSpec::Plan { .. }) => {
                self.set_standard_view(StandardView::Plan);
                let (lo, hi) = self.plan_rect();
                if let Some(v) = logic::plan_view(spec, lo, hi) {
                    self.view = v;
                }
            }
            Some(ViewSpec::Camera {
                target,
                yaw,
                pitch,
                distance,
                perspective,
            }) => {
                if self.plan() {
                    self.set_standard_view(StandardView::RightIsometric);
                }
                self.session.view.projection = if *perspective {
                    Projection::Perspective
                } else {
                    Projection::Orthographic
                };
                // The standard view is applied in the next frame and would turn the camera back.
                self.pending_camera = Some((*target, *yaw, *pitch, *distance));
            }
            None => {}
        }
        self.status = if plan.missing > 0 {
            tr!(
                "issues-ui-opened-missing",
                title = issue.title.clone(),
                count = plan.missing
            )
        } else {
            tr!("issues-ui-opened", title = issue.title.clone())
        };
    }

    /// A camera of a viewpoint waits for the 3D view to follow the settings of the session.
    pub(super) fn apply_pending_camera(&mut self) {
        let Some((target, yaw, pitch, distance)) = self.pending_camera.take() else {
            return;
        };
        // The viewpoint has drawing coordinates; the camera works relative to the scene origin.
        let o = kadakas_core::mesh::scene_origin(self.session.editor.doc());
        let c = &mut self.view3d.camera;
        c.target = glam::Vec3::new(
            (target[0] - o.x) as f32,
            (target[1] - o.y) as f32,
            (target[2] - o.z) as f32,
        );
        c.yaw = yaw as f32;
        c.pitch = pitch as f32;
        c.distance = (distance as f32).clamp(1.0, 1.0e9);
        self.view3d.fit_done();
    }

    /// The context menu of the drawing: new issues from the selection, from a point or from an
    /// object.
    pub(super) fn issue_context_menu(&mut self, ui: &mut egui::Ui, at: Option<DVec2>) {
        if ui.button(tr!("issues-ui-new-from-selection")).clicked() {
            let selection = self.session.selection.clone();
            self.issues_ui.start_for_selection(&selection);
            ui.close();
        }
        if let Some(p) = at
            && ui.button(tr!("issues-ui-new-here")).clicked()
        {
            self.issues_ui.start_at(p, 0.0);
            ui.close();
        }
    }
}
