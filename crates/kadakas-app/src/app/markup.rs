//! The glue between the markup UI and the app (REV-11): what needs the view of the app, the
//! guards for edits and the status line. The window code is in `markup_ui.rs`, the logic in
//! `kadakas_ui_core::markup`.

use kadakas_core::command::Command;
use kadakas_core::issue::ViewSpec;
use kadakas_core::markup::MarkupId;
use kadakas_l10n::tr;
use kadakas_review::issues::capture_visibility;
use kadakas_ui_core::markup as logic;

use super::KadakasApp;
use crate::issues_ui::{author_name, now_ms};
use crate::layout::Tab;
use crate::markup_ui::{Request, display_name};

impl KadakasApp {
    /// View > Markup: the markup mode on or off. Switching it on shows the palette.
    pub(super) fn toggle_markup(&mut self, _ctx: &egui::Context) {
        let on = !self.markup_ui.mode;
        self.markup_ui.set_mode(on);
        if on {
            self.layouts.show_tab(Tab::Markup);
            if self.machine.is_busy() {
                self.machine.cancel();
            }
        }
    }

    /// Does what the markup UI asked for. They need the view, which the app owns.
    pub(super) fn run_markup_requests(&mut self) {
        for r in self.markup_ui.take_requests() {
            match r {
                Request::Cmd(c) => self.exec(&c),
                Request::Undo => self.undo(),
                Request::New => self.new_markup(),
                Request::Finish => self.finish_markup(),
                Request::GoTo(id) => self.go_to_markup(id),
                Request::Resume(id) => {
                    self.go_to_markup(id);
                    if self.session.editor.doc().markups.get(id).is_some() {
                        self.markup_ui.open = Some(id);
                        self.markup_ui.selected = None;
                        self.markup_ui.set_mode(true);
                    }
                }
            }
        }
        if let Some(n) = self.markup_ui.take_notice() {
            self.status = n;
        }
    }

    /// Starts a markup with the view as it is now: one undo step.
    fn new_markup(&mut self) {
        if self.markup_ui.open.is_some() {
            self.finish_markup();
        }
        let Some(mut viewpoint) = self.markup_ui.current_view().cloned() else {
            self.status = tr!("markup-ui-no-view");
            return;
        };
        let (layers, classes) = capture_visibility(self.session.editor.doc());
        viewpoint.layers = layers;
        viewpoint.classes = classes;
        let cmd = logic::new_markup(viewpoint, &author_name(), now_ms(), None);
        let Some(out) = self.exec_out(&cmd) else {
            return;
        };
        let Some(id) = out.created.first().copied().map(MarkupId) else {
            return;
        };
        self.markup_ui.open = Some(id);
        self.markup_ui.picked = Some(id);
        self.markup_ui.selected = None;
        self.markup_ui.hide_marks = false;
        self.markup_ui.set_mode(true);
        self.status = tr!("markup-ui-created");
    }

    /// Closes the open markup. A markup without marks is removed.
    fn finish_markup(&mut self) {
        let Some(id) = self.markup_ui.close_open() else {
            return;
        };
        let empty = self
            .session
            .editor
            .doc()
            .markups
            .get(id)
            .is_some_and(|m| m.marks.is_empty());
        if empty {
            self.exec(&Command::DeleteMarkup { id });
            if self.markup_ui.picked == Some(id) {
                self.markup_ui.picked = None;
            }
            self.status = tr!("markup-ui-empty-removed");
        } else {
            self.status = tr!("markup-ui-finished");
        }
    }

    /// Shows the viewpoint of a markup: its layers and classes (one undo step, only if they
    /// differ), its view, and its marks.
    fn go_to_markup(&mut self, id: MarkupId) {
        let doc = self.session.editor.doc();
        let Some(m) = doc.markups.get(id).cloned() else {
            return;
        };
        let commands = logic::visibility_commands(doc, &m.viewpoint);
        if !commands.is_empty() && !self.read_only() {
            self.exec(&Command::Batch { commands });
        }
        let width = self
            .markup_ui
            .current_view()
            .map_or(m.viewpoint.canvas[0], |v| v.canvas[0]);
        let spec = match &m.viewpoint.view {
            ViewSpec::Plan { center, zoom } => ViewSpec::Plan {
                center: *center,
                zoom: logic::plan_zoom_for(&m.viewpoint, *zoom, width),
            },
            other => other.clone(),
        };
        self.restore_view_spec(&spec);
        if let Some(fov) = m.viewpoint.fov {
            self.view3d.camera.fov_y = fov.to_radians() as f32;
        }
        self.markup_ui.picked = Some(id);
        self.markup_ui.hide_marks = false;
        let name = display_name(self.session.editor.doc(), &m);
        self.status = tr!("markup-ui-opened", name = name);
    }
}
