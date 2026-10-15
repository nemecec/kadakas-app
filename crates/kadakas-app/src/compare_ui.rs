//! The Compare window (REV-03): File > Compare Versions...
//!
//! It shows the colored overlay of two sides of the drawing, on the plan and in 3D: added objects
//! green, changed objects orange, removed objects as ghosts, each with a line pattern and a mark
//! as well as the color. The sides are two versions, or a version and the open drawing. The
//! window lists the changes (a click selects the object and zooms to it; a removed object is
//! framed as a ghost), has a history slider that steps through the versions of a branch, and
//! shows a sheet of both sides side by side.
//!
//! The window only draws. The model is `kadakas_review::overlay`, the cache and the documents of
//! the sides are in `kadakas-api` (`Session::compare_overlay`, `compare_doc`, `compare_scene`),
//! and the logic of the slider and the list is in `kadakas_ui_core::compare`.

use std::collections::HashSet;
use std::sync::Arc;

use egui::{Color32, Ui};
use glam::DVec2;
use kadakas_api::{CompareSide, Session, ShownDoc};
use kadakas_core::model::{Document, LayerId, LayerKind, ObjectId};
use kadakas_core::uid::Uid;
use kadakas_l10n::tr;
use kadakas_render::look::Look;
use kadakas_review::overlay::{
    CompareOverlay, CompareStyle, GhostReport, Palette, Status, UnchangedMode, ghosts_in,
};
use kadakas_ui_core::compare::{
    ClickAction, CompareForm, HistorySlider, RowFilter, SliderMode, click_action, frame_box,
    symbol, visible_rows,
};
use kadakas_ui_core::versions::{Change, ChangeRow, DiffView, Side, VersionList};
use serde_json::json;

use crate::compare_draw::{self, CompareDraw};

/// The most things that the list of changes shows.
const LIST_LIMIT: usize = 500;
/// How long the drawing must stay unchanged before an overlay that involves it is made again, in
/// seconds. The drawing changes at every move of the pointer in a drag.
const QUIET: f64 = 0.5;

/// A part of the model to show, after a click in the list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomRequest {
    pub id: ObjectId,
    pub lo: DVec2,
    pub hi: DVec2,
}

/// The 3D scene of a compare: tinted, with the ghosts added, and the look that makes the ghosts
/// see-through.
pub struct Compare3d {
    pub scene: Arc<kadakas_core::mesh::Scene>,
    pub look: Arc<Look>,
    /// Changes when the scene changes (the GPU uploads it again).
    pub key: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    TwoSides,
    History,
}

/// What the window shows: the overlay of one pair of sides.
struct Shown {
    pair: (Side, Side),
    overlay: Arc<CompareOverlay>,
    /// The document of the new side, when it is not the open drawing.
    doc: Option<Arc<Document>>,
    /// The document of the old side, for drawing the ghosts (`None`: there is no ghost).
    old: Option<Arc<Document>>,
    ghosts: Vec<(kadakas_core::model::Object, &'static str)>,
    /// What became of the ghosts of removed objects in the plan.
    report: GhostReport,
    diff: DiffView,
    /// The revision of the open drawing, when the pair involves it.
    revision: Option<u64>,
}

struct Scene3dKey {
    pair: (Side, Side),
    revision: Option<u64>,
    style: (Palette, UnchangedMode, bool, bool, u32),
}

/// State of the window.
pub struct CompareUi {
    open: bool,
    tab: Tab,
    list: VersionList,
    list_revision: u64,
    form: CompareForm,
    slider: HistorySlider,
    branch: Option<String>,
    show_on_drawing: bool,
    style: CompareStyle,
    filter: RowFilter,
    shown: Option<Shown>,
    /// The comparison is old: the next frame makes it again.
    dirty: bool,
    /// The style changed: the ghosts and the 3D scene are made again (the overlay stays).
    style_dirty: bool,
    /// Gives each 3D scene that is made its own key, so that the GPU uploads it again.
    scene_serial: u64,
    /// When the drawing was last seen to change (the time of egui), for the quiet time.
    changed_at: Option<f64>,
    error: Option<String>,
    focus: Option<ObjectId>,
    zoom: Option<ZoomRequest>,
    scene3d: Option<(Scene3dKey, Arc<Compare3d>)>,
    /// Pairs next to the slider that are not made yet.
    prefetch: Vec<(Side, Side)>,
    #[cfg(not(target_arch = "wasm32"))]
    sheets: SheetsPart,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct SheetsPart {
    ui: crate::sheets_ui::SheetUi,
    layer: Option<LayerId>,
    page: Option<kadakas_print::Page>,
    /// The pair that `page` is made for.
    pair: Option<(Side, Side)>,
    error: Option<String>,
}

impl Default for CompareUi {
    fn default() -> Self {
        CompareUi {
            open: false,
            tab: Tab::TwoSides,
            list: VersionList::default(),
            list_revision: 0,
            form: CompareForm::default(),
            slider: HistorySlider::new(Vec::new()),
            branch: None,
            show_on_drawing: true,
            style: CompareStyle::default(),
            filter: RowFilter::default(),
            shown: None,
            dirty: false,
            style_dirty: false,
            scene_serial: 1 << 40,
            changed_at: None,
            error: None,
            focus: None,
            zoom: None,
            scene3d: None,
            prefetch: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            sheets: SheetsPart::default(),
        }
    }
}

fn api_side(s: Side) -> CompareSide {
    match s {
        Side::Version(id) => CompareSide::Version(id),
        Side::Current => CompareSide::Current,
    }
}

fn side_text(list: &VersionList, side: Side) -> String {
    match side {
        Side::Current => tr!("compare-ui-current"),
        Side::Version(id) => list.name_of(id),
    }
}

fn style_key(s: &CompareStyle) -> (Palette, UnchangedMode, bool, bool, u32) {
    (
        s.palette,
        s.unchanged,
        s.dark,
        s.show_ghosts,
        s.dim.to_bits() ^ s.ghost.to_bits().rotate_left(7) ^ u32::from(s.show_previous),
    )
}

impl CompareUi {
    /// Opens the window and reads the versions.
    pub fn open(&mut self, session: &mut Session) {
        self.open = true;
        self.refresh(session);
    }

    fn refresh(&mut self, session: &mut Session) {
        self.list_revision = session.editor.revision();
        let list = session
            .handle("version.list", json!({"current": false}))
            .map(|v| VersionList::parse(&v))
            .unwrap_or_default();
        // An edit of the drawing alone does not make the comparison old, unless it involves the
        // drawing (the revision check of `update` sees that). A changed list does.
        if list != self.list {
            self.dirty = true;
        }
        self.list = list;
        self.form.retain(&self.list);
        self.slider.update(&self.list, self.branch.as_deref());
    }

    /// Forgets what is shown, for a new document. The window stays open (if it was), with the
    /// versions of the new document.
    pub fn reset(&mut self, session: &mut Session) {
        self.shown = None;
        self.form = CompareForm::default();
        self.slider = HistorySlider::new(Vec::new());
        self.branch = None;
        self.focus = None;
        self.zoom = None;
        self.scene3d = None;
        self.prefetch.clear();
        self.error = None;
        self.dirty = false;
        self.style_dirty = false;
        self.changed_at = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sheets = SheetsPart::default();
        }
        self.list = VersionList::default();
        if self.open {
            self.refresh(session);
        }
    }

    /// True while the drawing is read only: the plan shows a version, or the 3D view shows a
    /// compare (`plan`: the plan is shown).
    pub fn read_only(&self, plan: bool) -> bool {
        self.shown
            .as_ref()
            .filter(|_| self.open && self.show_on_drawing)
            .is_some_and(|s| s.doc.is_some() || !plan)
    }

    #[cfg(test)]
    pub fn close(&mut self) {
        self.open = false;
    }

    #[cfg(test)]
    pub fn show_pair(&mut self, session: &mut Session, from: Side, to: Side) {
        self.open(session);
        self.form.from = Some(from);
        self.form.to = to;
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |u| {
            self.update(u.ctx(), session);
        });
    }

    /// The pair of sides that the window compares now.
    fn wanted_pair(&self) -> Option<(Side, Side)> {
        match self.tab {
            Tab::TwoSides => self.form.pair(),
            Tab::History => self.slider.pair(),
        }
    }

    /// The overlay and the other things the canvas draws, while the window is open and the person
    /// wants to see the overlay on the drawing.
    pub fn draw(&self) -> Option<CompareDraw<'_>> {
        let s = self
            .shown
            .as_ref()
            .filter(|_| self.open && self.show_on_drawing)?;
        Some(CompareDraw {
            overlay: &s.overlay,
            style: &self.style,
            ghosts: &s.ghosts,
            ghost_doc: s.old.as_deref(),
            report: s.report,
            focus: self.focus,
        })
    }

    /// The document that the plan shows instead of the open drawing: a version, read only.
    pub fn history_doc(&self) -> Option<Arc<Document>> {
        self.shown
            .as_ref()
            .filter(|_| self.open && self.show_on_drawing)
            .and_then(|s| s.doc.clone())
    }

    /// Sets the colors for the theme of the canvas.
    pub fn set_dark(&mut self, dark: bool) {
        if self.style.dark != dark {
            self.style.dark = dark;
            self.style_dirty = true;
        }
    }

    /// The part of the model to show, after a click in the list. The plan or the 3D view takes it.
    pub fn take_zoom(&mut self) -> Option<ZoomRequest> {
        self.zoom.take()
    }

    /// The 3D scene with the overlay, while the overlay is on. Made once for each state of the
    /// comparison and the style.
    pub fn scene_3d(&mut self, session: &mut Session) -> Option<Arc<Compare3d>> {
        let shown = self
            .shown
            .as_ref()
            .filter(|_| self.open && self.show_on_drawing)?;
        let key = Scene3dKey {
            pair: shown.pair,
            revision: shown.revision,
            style: style_key(&self.style),
        };
        if let Some((k, scene)) = &self.scene3d
            && k.pair == key.pair
            && k.revision == key.revision
            && k.style == key.style
        {
            return Some(scene.clone());
        }
        let overlay = shown.overlay.clone();
        let pair = shown.pair;
        let style = self.style;
        let t = session
            .compare_tinted(&overlay, api_side(pair.0), api_side(pair.1), &style)
            .ok()?;
        let ghosts: HashSet<ObjectId> = t.ghosts.iter().copied().collect();
        let [r, g, b] = style.color(Status::Removed);
        let look = Look::ghosted(&t.scene, &|id| ghosts.contains(&id), [r, g, b], style.ghost);
        // Each scene has a key of its own: a change of the palette, the theme or the pair
        // gives other geometry, which the GPU must upload again.
        self.scene_serial += 1;
        let made = Arc::new(Compare3d {
            key: self.scene_serial,
            scene: Arc::new(t.scene),
            look: Arc::new(look),
        });
        self.scene3d = Some((key, made.clone()));
        Some(made)
    }

    /// Makes the overlay of the wanted pair, if it is not the one that is shown.
    fn update(&mut self, ctx: &egui::Context, session: &mut Session) {
        let revision = session.editor.revision();
        let now = ctx.input(|i| i.time);
        // The open drawing changed: the list of versions and an overlay that involves the drawing
        // are old. They are made again when the drawing has been quiet.
        if self.list_revision != revision {
            self.list_revision = revision;
            self.changed_at = Some(now);
        }
        if let Some(t) = self.changed_at {
            if now - t >= QUIET {
                self.changed_at = None;
                self.refresh(session);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(QUIET - (now - t)));
                return;
            }
        }
        if self.style_dirty {
            self.style_dirty = false;
            self.restyle(session);
        }
        let want = self.wanted_pair();
        let stale = match (&self.shown, want) {
            (_, None) => self.shown.is_some(),
            (None, Some(_)) => true,
            (Some(s), Some(p)) => {
                s.pair != p || self.dirty || s.revision.is_some_and(|r| r != revision)
            }
        };
        if !stale {
            return;
        }
        self.dirty = false;
        self.error = None;
        self.scene3d = None;
        let Some(pair) = want else {
            self.shown = None;
            return;
        };
        match self.make(session, pair) {
            Ok(s) => {
                self.shown = Some(s);
            }
            Err(e) => {
                self.shown = None;
                self.error = Some(e);
            }
        }
        if self.tab == Tab::History {
            self.prefetch = self.slider.neighbours();
        }
    }

    /// Makes the ghosts again for a new style (the overlay stays).
    fn restyle(&mut self, session: &mut Session) {
        self.scene3d = None;
        let style = self.style;
        let Some(shown) = &mut self.shown else { return };
        let found = match &shown.doc {
            Some(d) => ghosts_in(d, shown.old.as_ref().unwrap_or(d), &shown.overlay, &style),
            None => {
                let live = session.editor.doc();
                ghosts_in(
                    live,
                    shown.old.as_deref().unwrap_or(live),
                    &shown.overlay,
                    &style,
                )
            }
        };
        shown.ghosts = found.items;
        shown.report = found.report;
    }

    fn make(&self, session: &mut Session, pair: (Side, Side)) -> Result<Shown, String> {
        let (from, to) = (api_side(pair.0), api_side(pair.1));
        let overlay = session.compare_overlay(from, to).map_err(|e| e.message)?;
        let shown = session.compare_doc(to).map_err(|e| e.message)?;
        // The old side has the resources that the ghosts need.
        let old = session
            .compare_old_doc(&overlay, from)
            .map_err(|e| e.message)?;
        let (doc, found) = match &shown {
            ShownDoc::Live => {
                let live = session.editor.doc();
                (
                    None,
                    ghosts_in(live, old.as_deref().unwrap_or(live), &overlay, &self.style),
                )
            }
            ShownDoc::Built(d) => (
                Some(d.clone()),
                ghosts_in(d, old.as_deref().unwrap_or(d), &overlay, &self.style),
            ),
        };
        let params = json!({"from": pair.0.to_json(), "to": pair.1.to_json(),
            "kinds": ["objects"], "limit": LIST_LIMIT});
        let diff = session
            .handle("version.diff", params)
            .map(|v| DiffView::parse(&v))
            .map_err(|e| e.message)?;
        let involves_drawing = pair.0 == Side::Current || pair.1 == Side::Current;
        Ok(Shown {
            pair,
            overlay,
            doc,
            old,
            ghosts: found.items,
            report: found.report,
            diff,
            revision: involves_drawing.then(|| session.editor.revision()),
        })
    }

    /// Makes the next pair next to the slider, so that a step is quick.
    fn prefetch_one(&mut self, ctx: &egui::Context, session: &mut Session) {
        if self.prefetch.is_empty() {
            return;
        }
        // The next steps are made only when the window is idle: no key, no pointer move, no scroll.
        // Else a drag of the slider would wait for them.
        let idle = ctx.input(|i| {
            i.events.is_empty()
                && !i.pointer.is_moving()
                && i.smooth_scroll_delta == egui::Vec2::ZERO
        });
        if !idle {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
            return;
        }
        let Some((from, to)) = self.prefetch.pop() else {
            return;
        };
        let _ = session.compare_overlay(api_side(from), api_side(to));
        if !self.prefetch.is_empty() {
            ctx.request_repaint();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        if !self.open {
            return;
        }
        self.update(ctx, session);
        self.prefetch_one(ctx, session);
        let mut open = self.open;
        egui::Window::new(tr!("compare-ui-title"))
            .id(egui::Id::new("win-compare"))
            .open(&mut open)
            .default_width(480.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_height(ctx.content_rect().height() * 0.8)
                    .show(ui, |ui| self.body(ui, session, status));
            });
        self.open = open;
        if !self.open {
            self.focus = None;
        }
    }

    fn body(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        ui.small(tr!("compare-ui-intro"));
        if self.list.rows.is_empty() {
            ui.label(tr!("compare-ui-no-versions"));
            return;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::TwoSides, tr!("compare-ui-tab-two"));
            ui.selectable_value(&mut self.tab, Tab::History, tr!("compare-ui-tab-history"));
        });
        match self.tab {
            Tab::TwoSides => self.two_sides(ui),
            Tab::History => self.history(ui),
        }
        ui.separator();
        self.display_options(ui);
        ui.separator();
        match (&self.shown, &self.error) {
            (_, Some(e)) => {
                ui.colored_label(Color32::from_rgb(200, 60, 40), e);
            }
            (None, None) => {
                ui.label(tr!("compare-ui-pick-two"));
            }
            (Some(_), None) => self.result(ui, session, status),
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.separator();
            self.sheets_section(ui, session);
        }
    }

    fn side_combo(&self, ui: &mut Ui, salt: &str, current: Option<Side>) -> Option<Side> {
        let mut chosen = current;
        let text = current.map_or_else(|| tr!("compare-ui-pick"), |s| side_text(&self.list, s));
        egui::ComboBox::from_id_salt(salt)
            .selected_text(text)
            .show_ui(ui, |ui| {
                for row in self.list.rows.iter().rev() {
                    ui.selectable_value(&mut chosen, Some(Side::Version(row.id)), &row.name);
                }
                ui.selectable_value(&mut chosen, Some(Side::Current), tr!("compare-ui-current"));
            });
        (chosen != current).then_some(chosen).flatten()
    }

    fn two_sides(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(tr!("compare-ui-from"));
            if let Some(s) = self.side_combo(ui, "compare-from", self.form.from) {
                self.form.from = Some(s);
            }
            ui.label(tr!("compare-ui-to"));
            if let Some(s) = self.side_combo(ui, "compare-to", Some(self.form.to)) {
                self.form.to = s;
            }
            if ui
                .add_enabled(
                    self.form.from.is_some(),
                    egui::Button::new(tr!("compare-ui-swap")),
                )
                .clicked()
            {
                self.form.swap();
            }
        });
        if self.form.from.is_some() && self.form.pair().is_none() {
            ui.small(tr!("compare-ui-same-side"));
        }
    }

    fn history(&mut self, ui: &mut Ui) {
        let branches: Vec<String> = {
            let mut b: Vec<String> = self.list.rows.iter().map(|r| r.branch.clone()).collect();
            b.sort();
            b.dedup();
            b
        };
        if branches.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(tr!("compare-ui-branch"));
                let current = self
                    .branch
                    .clone()
                    .unwrap_or_else(|| self.list.branch.clone());
                let mut chosen = current.clone();
                egui::ComboBox::from_id_salt("compare-branch")
                    .selected_text(&chosen)
                    .show_ui(ui, |ui| {
                        for b in &branches {
                            ui.selectable_value(&mut chosen, b.clone(), b);
                        }
                    });
                if chosen != current {
                    self.branch = Some(chosen);
                    self.slider.update(&self.list, self.branch.as_deref());
                    self.dirty = true;
                }
            });
        }
        ui.horizontal(|ui| {
            let mut mode = self.slider.mode();
            ui.radio_value(
                &mut mode,
                SliderMode::Neighbours,
                tr!("compare-ui-mode-neighbours"),
            );
            ui.radio_value(
                &mut mode,
                SliderMode::FixedBase,
                tr!("compare-ui-mode-base"),
            );
            if mode != self.slider.mode() {
                self.slider.set_mode(mode);
            }
        });
        if self.slider.mode() == SliderMode::FixedBase {
            ui.horizontal(|ui| {
                ui.label(tr!("compare-ui-base"));
                let steps: Vec<Side> = self.slider.steps().to_vec();
                let mut chosen = self.slider.base();
                let text = chosen.map_or_else(String::new, |s| side_text(&self.list, s));
                egui::ComboBox::from_id_salt("compare-base")
                    .selected_text(text)
                    .show_ui(ui, |ui| {
                        for s in &steps {
                            ui.selectable_value(&mut chosen, Some(*s), side_text(&self.list, *s));
                        }
                    });
                if let Some(c) = chosen
                    && Some(c) != self.slider.base()
                {
                    self.slider.set_base(c);
                }
            });
        }
        if self.slider.is_empty() {
            ui.small(tr!("compare-ui-slider-empty"));
            return;
        }
        let last = self.slider.len() - 1;
        ui.horizontal(|ui| {
            if ui.button(tr!("compare-ui-newer")).clicked() {
                self.slider.step(-1);
            }
            let mut pos = self.slider.position();
            ui.spacing_mut().slider_width = 200.0;
            if ui
                .add(egui::Slider::new(&mut pos, 0..=last).show_value(false))
                .changed()
            {
                self.slider.set_position(pos);
            }
            if ui.button(tr!("compare-ui-older")).clicked() {
                self.slider.step(1);
            }
        });
        ui.horizontal(|ui| {
            ui.small(tr!("compare-ui-newest"));
            ui.small(tr!(
                "compare-ui-step",
                step = self.slider.position() + 1,
                total = last + 1
            ));
            ui.small(tr!("compare-ui-oldest"));
        });
        if let Some((from, to)) = self.slider.pair() {
            ui.label(tr!(
                "compare-ui-pair",
                from = side_text(&self.list, from),
                to = side_text(&self.list, to)
            ));
        }
    }

    fn display_options(&mut self, ui: &mut Ui) {
        let before = (self.style, self.show_on_drawing);
        ui.checkbox(&mut self.show_on_drawing, tr!("compare-ui-show-on-drawing"));
        ui.horizontal(|ui| {
            ui.label(tr!("compare-ui-unchanged"));
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Show,
                tr!("compare-ui-unchanged-show"),
            );
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Dim,
                tr!("compare-ui-unchanged-dim"),
            );
            ui.radio_value(
                &mut self.style.unchanged,
                UnchangedMode::Hide,
                tr!("compare-ui-unchanged-hide"),
            );
        });
        ui.horizontal(|ui| {
            ui.label(tr!("compare-ui-palette"));
            ui.radio_value(
                &mut self.style.palette,
                Palette::Standard,
                tr!("compare-ui-palette-standard"),
            );
            ui.radio_value(
                &mut self.style.palette,
                Palette::ColorBlind,
                tr!("compare-ui-palette-blind"),
            );
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.style.show_ghosts, tr!("compare-ui-ghosts"));
            ui.checkbox(&mut self.style.show_previous, tr!("compare-ui-previous"));
        });
        if (self.style, self.show_on_drawing) != before {
            self.style_dirty = true;
        }
    }

    fn result(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        let Some(shown) = &self.shown else { return };
        let counts = shown.overlay.counts();
        if shown.overlay.is_empty() {
            ui.label(tr!("compare-ui-no-changes"));
            return;
        }
        let diff_exact = shown.diff.exact;
        // The legend: the swatch, the mark and the count of each status.
        ui.horizontal_wrapped(|ui| {
            for (status, n) in [
                (Status::Added, counts.added),
                (Status::Removed, counts.removed),
                (Status::Changed, counts.changed),
            ] {
                let text = match status {
                    Status::Added => tr!("compare-ui-legend-added", count = n),
                    Status::Removed => tr!("compare-ui-legend-removed", count = n),
                    Status::Changed => tr!("compare-ui-legend-changed", count = n),
                };
                swatch(ui, &self.style, status);
                ui.label(text);
            }
        });
        let overlay_exact = self.shown.as_ref().is_some_and(|s| s.overlay.exact());
        let amber = Color32::from_rgb(200, 120, 0);
        if !diff_exact {
            ui.colored_label(amber, tr!("compare-ui-partial"));
        }
        if !overlay_exact {
            ui.colored_label(amber, tr!("compare-ui-overlay-partial"));
        }
        if diff_exact != overlay_exact {
            // The list and the overlay have their own work limits.
            ui.small(tr!("compare-ui-partial-differ"));
        }
        if let Some(shown) = &self.shown {
            let c = CompareDraw {
                overlay: &shown.overlay,
                style: &self.style,
                ghosts: &shown.ghosts,
                ghost_doc: None,
                report: shown.report,
                focus: None,
            };
            let partial = tr!("compare-ui-note-partial");
            for line in compare_draw::legend_notes(&c) {
                if line != partial {
                    ui.small(line);
                }
            }
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.filter.added, tr!("compare-ui-filter-added"));
            ui.checkbox(&mut self.filter.removed, tr!("compare-ui-filter-removed"));
            ui.checkbox(&mut self.filter.changed, tr!("compare-ui-filter-changed"));
        });
        ui.small(tr!("compare-ui-click"));
        let rows: Vec<ChangeRow> = self
            .shown
            .as_ref()
            .map(|s| s.diff.rows.clone())
            .unwrap_or_default();
        let truncated = self.shown.as_ref().is_some_and(|s| s.diff.truncated);
        let mut clicked: Option<ChangeRow> = None;
        egui::ScrollArea::vertical()
            .id_salt("compare-rows")
            .max_height(240.0)
            .show(ui, |ui| {
                for row in visible_rows(&rows, &self.filter) {
                    let status = match row.change {
                        Change::Added => Status::Added,
                        Change::Removed => Status::Removed,
                        Change::Changed => Status::Changed,
                    };
                    let what = row
                        .shape
                        .clone()
                        .unwrap_or_else(|| row.id.chars().take(8).collect());
                    let mut text = format!("{} {what}", symbol(row.change));
                    if !row.paths.is_empty() {
                        text = format!("{text} ({})", row.paths.join(", "));
                    }
                    ui.horizontal(|ui| {
                        swatch(ui, &self.style, status);
                        let focused = self.focus.is_some_and(|f| f.0.to_string() == row.id);
                        if ui.selectable_label(focused, text).clicked() {
                            clicked = Some(row.clone());
                        }
                    });
                }
            });
        if truncated {
            ui.small(tr!("compare-ui-more"));
        }
        if let Some(row) = clicked {
            self.click(session, &row, status);
        }
    }

    /// A click on a row: select the object and zoom to it, or frame the ghost of a removed object.
    fn click(&mut self, session: &mut Session, row: &ChangeRow, status: &mut String) {
        let Some(shown) = &self.shown else { return };
        let Some(id) = Uid::parse(&row.id).map(ObjectId) else {
            return;
        };
        let live = shown.doc.is_none();
        let bounds = match click_action(row) {
            ClickAction::SelectAndZoom(_) => {
                let doc: &Document = shown.doc.as_deref().unwrap_or(session.editor.doc());
                doc.object(id).map(|o| doc.object_bounds(o))
            }
            ClickAction::FrameGhost(_) => shown
                .overlay
                .shapes_of(id)
                .and_then(|s| s.old.as_ref())
                .map(|o| {
                    let doc: &Document = shown.doc.as_deref().unwrap_or(session.editor.doc());
                    doc.object_bounds(o)
                }),
            ClickAction::Nothing => return,
        };
        let Some((lo, hi)) = bounds else {
            *status = match click_action(row) {
                ClickAction::FrameGhost(_) => tr!("compare-ui-no-ghost"),
                _ => tr!("compare-ui-gone"),
            };
            return;
        };
        if live
            && matches!(click_action(row), ClickAction::SelectAndZoom(_))
            && let Err(e) = session.handle("selection.set", json!({"ids": [row.id]}))
        {
            *status = e.message;
        }
        self.focus = Some(id);
        if let Some((lo, hi)) = frame_box(lo, hi) {
            self.zoom = Some(ZoomRequest { id, lo, hi });
        }
    }

    /// Makes the page of the chosen sheet for the pair of sides, if it is not made.
    #[cfg(not(target_arch = "wasm32"))]
    fn update_sheet(&mut self, session: &mut Session) {
        let pair = self.wanted_pair();
        let make = self.sheets.page.is_none() || self.sheets.pair != pair;
        if let (true, Some(layer), Some((from, to))) = (make, self.sheets.layer, pair) {
            self.sheets.pair = pair;
            match session.compare_sheet_page(layer, api_side(from), api_side(to)) {
                Ok(p) => {
                    self.sheets.page = Some(p);
                    self.sheets.error = None;
                }
                Err(e) => {
                    self.sheets.page = None;
                    self.sheets.error = Some(e.message);
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sheets_section(&mut self, ui: &mut Ui, session: &mut Session) {
        // The sheets of the open drawing and of both sides, each once.
        let mut sheets: Vec<(LayerId, String)> = Vec::new();
        let mut add = |doc: &Document| {
            for l in doc.layers.iter().filter(|l| l.kind == LayerKind::Sheet) {
                if !sheets.iter().any(|s| s.0 == l.id) {
                    sheets.push((l.id, l.name.clone()));
                }
            }
        };
        add(session.editor.doc());
        if let Some(shown) = &self.shown {
            shown.doc.iter().for_each(|d| add(d));
            shown.old.iter().for_each(|d| add(d));
        }
        egui::CollapsingHeader::new(tr!("compare-ui-sheets"))
            .id_salt("compare-sheets")
            .show(ui, |ui| {
                if sheets.is_empty() {
                    ui.small(tr!("compare-ui-no-sheets"));
                    return;
                }
                let name = |l: Option<LayerId>| {
                    l.and_then(|l| sheets.iter().find(|s| s.0 == l))
                        .map_or_else(|| tr!("compare-ui-pick"), |s| s.1.clone())
                };
                let mut chosen = self.sheets.layer;
                egui::ComboBox::from_id_salt("compare-sheet")
                    .selected_text(name(chosen))
                    .show_ui(ui, |ui| {
                        for (id, n) in &sheets {
                            ui.selectable_value(&mut chosen, Some(*id), n);
                        }
                    });
                if chosen != self.sheets.layer {
                    self.sheets.layer = chosen;
                    self.sheets.page = None;
                }
                self.update_sheet(session);
                let pair = self.wanted_pair();
                if let Some(e) = &self.sheets.error {
                    ui.colored_label(Color32::from_rgb(200, 60, 40), e);
                }
                if pair.is_none() {
                    ui.small(tr!("compare-ui-pick-two"));
                }
                if let Some(page) = &self.sheets.page {
                    let width = ui.available_width().max(200.0);
                    let height = (width * (page.size.y / page.size.x) as f32).clamp(80.0, 420.0);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                    let mut view = crate::view::View::default();
                    crate::view::ViewExt::fit(&mut view, DVec2::ZERO, page.size, rect);
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 0.0, Color32::from_gray(150));
                    self.sheets.ui.paint(&painter, &view, page);
                }
            });
    }
}

/// A short line in the pattern of a status, with its mark: the key of the list and the legend.
pub(crate) fn swatch(ui: &mut Ui, style: &CompareStyle, status: Status) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(34.0, 14.0), egui::Sense::hover());
    let t = style.tint(status);
    let [r, g, b] = t.rgb;
    let stroke = egui::Stroke::new(2.5, Color32::from_rgb(r, g, b));
    let (a, z) = (
        rect.left_center(),
        rect.left_center() + egui::vec2(22.0, 0.0),
    );
    match t.pattern {
        kadakas_review::overlay::Pattern::Solid => {
            ui.painter().line_segment([a, z], stroke);
        }
        kadakas_review::overlay::Pattern::Dashed => {
            ui.painter()
                .extend(egui::Shape::dashed_line(&[a, z], stroke, 6.0, 4.0));
        }
        kadakas_review::overlay::Pattern::Dotted => {
            ui.painter()
                .extend(egui::Shape::dashed_line(&[a, z], stroke, 1.5, 3.0));
        }
    }
    ui.painter().text(
        z + egui::vec2(5.0, 0.0),
        egui::Align2::LEFT_CENTER,
        t.symbol,
        egui::FontId::proportional(14.0),
        Color32::from_rgb(r, g, b),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(s: &mut Session, x: i32) -> String {
        s.handle(
            "execute",
            json!({"command": {"op": "add_object", "shape":
                {"kind": "rect", "min": [x, 0], "max": [x + 3000, 3000]},
                "attrs": {"fill": [120, 120, 120, 255]}}}),
        )
        .unwrap()["created"][0]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// Versions 1 and 2, and a drawing with a third change.
    fn session() -> (Session, [String; 3]) {
        let mut s = Session::with_sequential_ids();
        let gone = rect(&mut s, 0);
        let kept = rect(&mut s, 5000);
        s.handle("version.create", json!({"name": "One", "now_ms": 1}))
            .unwrap();
        s.handle(
            "execute",
            json!({"command": {"op": "delete_objects", "ids": [gone]}}),
        )
        .unwrap();
        let added = rect(&mut s, 10000);
        s.handle("version.create", json!({"name": "Two", "now_ms": 2}))
            .unwrap();
        rect(&mut s, 20000);
        (s, [gone, kept, added])
    }

    fn frame(ui: &mut CompareUi, s: &mut Session, status: &mut String) {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |u| {
            ui.show(u.ctx(), s, status);
        });
    }

    #[test]
    fn the_window_compares_two_sides_and_lists_the_changes() {
        let (mut s, [gone, kept, added]) = session();
        let mut ui = CompareUi::default();
        let mut status = String::new();
        // A closed window does nothing.
        frame(&mut ui, &mut s, &mut status);
        assert!(ui.draw().is_none());
        ui.open(&mut s);
        frame(&mut ui, &mut s, &mut status);
        assert!(ui.draw().is_none(), "no pair chosen yet");
        ui.form.from = Some(Side::Version(1));
        ui.form.to = Side::Version(2);
        frame(&mut ui, &mut s, &mut status);
        let shown = ui.shown.as_ref().unwrap();
        let c = shown.overlay.counts();
        assert_eq!((c.added, c.removed, c.changed), (1, 1, 0));
        assert!(shown.doc.is_some(), "a version is shown, not the drawing");
        assert!(ui.history_doc().is_some() && ui.draw().is_some());
        assert_eq!(ui.draw().unwrap().ghosts.len(), 1);
        assert_eq!(shown.diff.rows.len(), 2);
        // A click on an added object zooms to it; a version is read only, so nothing is selected.
        let row = shown
            .diff
            .rows
            .iter()
            .find(|r| r.id == added)
            .unwrap()
            .clone();
        ui.click(&mut s, &row, &mut status);
        let z = ui.take_zoom().unwrap();
        assert!(z.lo.x < 10000.0 && z.hi.x > 13000.0);
        assert!(s.selection.is_empty());
        assert!(ui.take_zoom().is_none());
        // A click on a removed object frames its ghost.
        let row = ui
            .shown
            .as_ref()
            .unwrap()
            .diff
            .rows
            .iter()
            .find(|r| r.id == gone)
            .unwrap()
            .clone();
        ui.click(&mut s, &row, &mut status);
        let z = ui.take_zoom().unwrap();
        assert!(z.lo.x < 0.0 && z.hi.x > 3000.0);
        assert_eq!(ui.focus.map(|f| f.0.to_string()), Some(gone.clone()));
        // The drawing as the new side: the open drawing is shown and a click selects.
        ui.form.from = Some(Side::Version(2));
        ui.form.to = Side::Current;
        frame(&mut ui, &mut s, &mut status);
        assert!(ui.history_doc().is_none());
        let rows = ui.shown.as_ref().unwrap().diff.rows.clone();
        assert_eq!(rows.len(), 1);
        ui.click(&mut s, &rows[0], &mut status);
        assert_eq!(s.selection.len(), 1);
        // A removed object that is not there is said so; other things do nothing.
        let mut missing = rows[0].clone();
        missing.id = Uid::from_num(999).to_string();
        ui.click(&mut s, &missing, &mut status);
        assert!(!status.is_empty());
        let mut layer = rows[0].clone();
        layer.group = "layers".into();
        ui.click(&mut s, &layer, &mut status);
        let _ = kept;
        // Without a pair the overlay goes away; closing the window hides it.
        ui.form.from = None;
        frame(&mut ui, &mut s, &mut status);
        assert!(ui.draw().is_none());
        ui.open = false;
        frame(&mut ui, &mut s, &mut status);
    }

    #[test]
    fn the_history_slider_steps_through_the_versions() {
        let (mut s, _) = session();
        let mut ui = CompareUi::default();
        let mut status = String::new();
        ui.open(&mut s);
        ui.tab = Tab::History;
        frame(&mut ui, &mut s, &mut status);
        // The drawing differs from version Two: the first step is Two against the drawing.
        assert_eq!(ui.slider.pair(), Some((Side::Version(2), Side::Current)));
        assert_eq!(
            ui.shown.as_ref().unwrap().pair,
            (Side::Version(2), Side::Current)
        );
        ui.slider.step(1);
        frame(&mut ui, &mut s, &mut status);
        assert_eq!(
            ui.shown.as_ref().unwrap().pair,
            (Side::Version(1), Side::Version(2))
        );
        assert!(!ui.prefetch.is_empty() || ui.shown.is_some());
        // The idle frames make the pairs next to it.
        for _ in 0..3 {
            frame(&mut ui, &mut s, &mut status);
        }
        assert!(ui.prefetch.is_empty());
        ui.slider.set_mode(SliderMode::FixedBase);
        frame(&mut ui, &mut s, &mut status);
        assert!(ui.shown.is_some());
        // An edit of the drawing is noticed once it has been quiet.
        rect(&mut s, 30000);
        let ctx = egui::Context::default();
        let mut t = 0.0;
        for _ in 0..4 {
            t += 0.4;
            let input = egui::RawInput {
                time: Some(t),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |u| ui.show(u.ctx(), &mut s, &mut status));
        }
        assert_eq!(ui.list_revision, s.editor.revision());
    }

    #[test]
    fn the_scene_for_3d_is_made_once_for_a_state_and_the_style() {
        let (mut s, _) = session();
        let mut ui = CompareUi::default();
        let mut status = String::new();
        ui.open(&mut s);
        ui.form.from = Some(Side::Version(1));
        ui.form.to = Side::Current;
        frame(&mut ui, &mut s, &mut status);
        let a = ui.scene_3d(&mut s).unwrap();
        let b = ui.scene_3d(&mut s).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(a.look.has_transparent(), "the ghost is see-through");
        ui.style.unchanged = UnchangedMode::Hide;
        let c = ui.scene_3d(&mut s).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        // A version as the new side builds its own scene.
        ui.form.to = Side::Version(2);
        frame(&mut ui, &mut s, &mut status);
        assert!(ui.scene_3d(&mut s).is_some());
        ui.show_on_drawing = false;
        assert!(ui.scene_3d(&mut s).is_none());
        // Dark colors follow the theme.
        ui.set_dark(true);
        assert!(ui.style.dark);
    }

    #[test]
    fn sheets_and_errors_show_in_the_window() {
        let (mut s, _) = session();
        s.handle("sheet.create", json!({"name": "S"})).unwrap();
        s.handle("version.create", json!({"name": "Three", "now_ms": 3}))
            .unwrap();
        let mut ui = CompareUi::default();
        let mut status = String::new();
        ui.open(&mut s);
        ui.form.from = Some(Side::Version(3));
        ui.form.to = Side::Current;
        ui.sheets.layer = s
            .editor
            .doc()
            .layers
            .iter()
            .find(|l| l.kind == LayerKind::Sheet)
            .map(|l| l.id);
        frame(&mut ui, &mut s, &mut status);
        ui.update_sheet(&mut s);
        assert!(ui.sheets.page.is_some(), "{:?}", ui.sheets.error);
        // A version that has no such sheet shows an empty page for its side.
        ui.form.from = Some(Side::Version(1));
        ui.update_sheet(&mut s);
        assert!(ui.sheets.page.is_some(), "{:?}", ui.sheets.error);
        // A sheet that is on neither side gives an error text, not a crash.
        ui.sheets.layer = Some(LayerId::from_num(99));
        ui.sheets.page = None;
        ui.update_sheet(&mut s);
        assert!(ui.sheets.error.is_some());
        // A version that is deleted is dropped from the form.
        s.handle("version.delete", json!({"id": 1})).unwrap();
        ui.refresh(&mut s);
        assert_eq!(ui.form.from, None);
        // Texts exist for the labels.
        assert!(!side_text(&ui.list, Side::Current).is_empty());
        assert!(!compare_draw::note_text(&kadakas_review::overlay::Note::Partial).is_empty());
    }

    #[test]
    fn a_new_style_makes_a_new_scene_with_its_own_key_and_a_new_document_resets_the_window() {
        let (mut s, _) = session();
        let mut ui = CompareUi::default();
        let mut status = String::new();
        ui.show_pair(&mut s, Side::Version(1), Side::Current);
        let a = ui.scene_3d(&mut s).unwrap();
        // The same state and style: the same scene, the same key.
        assert!(Arc::ptr_eq(&a, &ui.scene_3d(&mut s).unwrap()));
        // Each change gives geometry with a key that no earlier scene had.
        let mut keys = vec![a.key];
        for change in 0..3 {
            match change {
                0 => ui.style.palette = Palette::ColorBlind,
                1 => ui.style.dim = 0.5,
                _ => ui.style.dark = true,
            }
            ui.style_dirty = true;
            frame(&mut ui, &mut s, &mut status);
            let b = ui.scene_3d(&mut s).unwrap();
            assert!(!keys.contains(&b.key), "{keys:?} {}", b.key);
            keys.push(b.key);
        }
        // Another pair gives another key too.
        ui.form.to = Side::Version(2);
        frame(&mut ui, &mut s, &mut status);
        let c = ui.scene_3d(&mut s).unwrap();
        assert!(!keys.contains(&c.key));
        // An edit of the drawing alone does not make a pair of two versions old.
        ui.show_pair(&mut s, Side::Version(1), Side::Version(2));
        ui.dirty = false;
        rect(&mut s, 40000);
        ui.refresh(&mut s);
        assert!(!ui.dirty, "the list of versions is the same");
        s.handle("version.create", json!({"name": "Three", "now_ms": 3}))
            .unwrap();
        ui.refresh(&mut s);
        assert!(ui.dirty, "a new version changes the list");
        // A new document: nothing is kept.
        ui.reset(&mut s);
        assert!(ui.shown.is_none() && ui.form.from.is_none() && ui.scene3d.is_none());
        assert!(!ui.read_only(false));
    }

    #[test]
    fn the_next_steps_wait_for_an_idle_window_and_notes_name_what_was_left_out() {
        let (mut s, _) = session();
        let mut ui = CompareUi::default();
        let mut status = String::new();
        ui.open(&mut s);
        ui.tab = Tab::History;
        frame(&mut ui, &mut s, &mut status);
        ui.prefetch = vec![
            (Side::Version(1), Side::Version(2)),
            (Side::Version(2), Side::Current),
        ];
        let before = ui.prefetch.len();
        // A key in the frame: no step is made.
        let busy = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: Some(egui::Key::A),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(busy, |u| ui.prefetch_one(u.ctx(), &mut s));
        assert_eq!(ui.prefetch.len(), before, "a busy frame makes no step");
        // An idle frame makes one.
        let _ = ctx.run_ui(Default::default(), |u| ui.prefetch_one(u.ctx(), &mut s));
        assert_eq!(ui.prefetch.len(), before - 1, "an idle frame makes one");
        // The banners and notes of a shown overlay have texts.
        let c = CompareDraw {
            overlay: &CompareOverlay::default(),
            style: &ui.style,
            ghosts: &[],
            ghost_doc: None,
            report: GhostReport {
                drawn: 0,
                hidden: 2,
                not_drawable: 3,
                capped: 4,
            },
            focus: None,
        };
        let lines = compare_draw::legend_notes(&c);
        assert_eq!(lines.len(), 2, "{lines:?}");
    }
}
