//! Modeling in the 3D view (MOD-08): the pointer of the Select tool and of the drawing tools, the
//! inference marks and the preview of the shape being drawn. The logic is in
//! `kadakas_ui_core::modeling` (headless); this module reads the pointer, runs the inference,
//! feeds the tools, runs the finished actions and draws.
//!
//! The inference scene is built once per document revision (`SceneCache`), never every frame.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use egui::{Color32, Pos2, Rect, Stroke};
use glam::{DVec2, DVec3, Vec2};
use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::mesh::Scene;
use kadakas_l10n::tr;
use kadakas_ui_core::infer3d::{self, Inference, Inferencer, Key, PAUSE_SECONDS, Rgb};
use kadakas_ui_core::keys::Modifiers;
use kadakas_ui_core::modeler::ModelerTool;
use kadakas_ui_core::modeling::{
    self, Item, Modeling, Out, Pointer, Preview, SceneCache, SubSelection, World,
};
use kadakas_view3d::vertex::LineVertex;
use kadakas_view3d::view3d::{self, View3d};

/// The color of selected edges and of the hatch of selected faces.
const SELECTED: [u8; 4] = [30, 100, 235, 255];
/// Lines in the hatch of a selected face.
const HATCH_LINES: usize = 14;
/// Pixels a drag must move before it is a selection rectangle.
const MIN_BAND: f32 = 4.0;

/// The state of modeling in the window.
pub struct Edit3d {
    pub modeling: Modeling,
    pub cache: SceneCache,
    /// The selected faces and edges.
    pub sub: SubSelection,
    inferencer: Inferencer,
    picker: Inferencer,
    /// How long the last build of the inference scene took.
    pub last_build: Duration,
    /// The world position of the local origin of the scene in the last frame.
    origin: Option<DVec3>,
    moved_at: f64,
    paused: bool,
    last_pointer: Option<Pos2>,
    shift_down: bool,
    /// The last inference under the pointer, for the marks.
    hover: Option<Inference>,
    preview: Preview,
    band: Option<(Vec2, Vec2)>,
    /// A message for the status line (a refused value, a failed edit).
    pub message: Option<String>,
    visuals: Option<Visuals>,
}

/// The lines of the selected faces and edges for one revision and selection.
struct Visuals {
    key: u64,
    /// The selected edges in world coordinates.
    edges: Vec<[DVec3; 2]>,
    /// The hatch of the selected faces in world coordinates.
    hatch: Vec<[DVec3; 2]>,
}

impl Default for Edit3d {
    fn default() -> Self {
        Edit3d {
            modeling: Modeling::default(),
            cache: SceneCache::default(),
            sub: SubSelection::default(),
            inferencer: Inferencer::default(),
            picker: Inferencer::default(),
            last_build: Duration::ZERO,
            origin: None,
            moved_at: 0.0,
            paused: false,
            last_pointer: None,
            shift_down: false,
            hover: None,
            preview: Preview::default(),
            band: None,
            message: None,
            visuals: None,
        }
    }
}

/// What the pointer code needs from the canvas.
pub struct Input<'a> {
    pub ui: &'a egui::Ui,
    pub resp: &'a egui::Response,
    pub rect: Rect,
    pub size: Vec2,
    pub scene: &'a Scene,
    pub tool: ModelerTool,
    pub mods: Modifiers,
}

fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

fn hash<T: Hash>(t: &T) -> u64 {
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

fn refresh_visuals(visuals: &mut Option<Visuals>, sub: &SubSelection, world: &World, rev: u64) {
    let key = hash(&(rev, sub));
    if visuals.as_ref().is_some_and(|v| v.key == key) {
        return;
    }
    let mut edges = Vec::new();
    let mut hatch = Vec::new();
    for item in sub.items() {
        match item {
            Item::Edge(m, e) => {
                if let Some(s) = world.edge_ends(m, e) {
                    edges.push(s);
                }
            }
            Item::Face(m, f) => {
                hatch.extend(modeling::select::face_hatch(
                    &world.face_loops(m, f),
                    HATCH_LINES,
                ));
            }
        }
    }
    *visuals = Some(Visuals { key, edges, hatch });
}

impl Edit3d {
    /// Forgets everything of the model (a new or opened file).
    pub fn reset(&mut self) {
        self.sub.clear();
        self.cache.clear();
        self.modeling = Modeling::default();
        self.inferencer.reset();
        self.hover = None;
        self.preview = Preview::default();
        self.band = None;
        self.visuals = None;
        self.origin = None;
    }

    /// Makes the tools follow the tool of the window.
    pub fn follow(&mut self, tool: ModelerTool) {
        if self.modeling.tool() != tool {
            self.modeling.choose(tool);
            self.inferencer.reset();
            self.hover = None;
            self.preview = Preview::default();
            self.band = None;
        }
    }

    /// The scene origin moved (the plan of the model grew): the camera keeps looking at the same
    /// place of the world.
    pub fn keep_view(&mut self, origin: DVec3, nav: &mut kadakas_ui_core::nav3d::Nav3d) {
        if let Some(old) = self.origin
            && old != origin
        {
            nav.camera.target += (old - origin).as_vec3();
        }
        self.origin = Some(origin);
    }

    /// The text for the Measurements box: what is typed, else the live value of the tool.
    pub fn measurement(&self) -> Option<kadakas_ui_core::modeler::Measurement> {
        use kadakas_ui_core::modeler::Measurement;
        use modeling::LiveLabel;
        let tool = self.modeling.tool();
        if !modeling::is_drawing_tool(tool) || tool == ModelerTool::Eraser {
            return None;
        }
        let units = kadakas_units::current();
        let live = self.modeling.live(&units);
        let default = match tool {
            ModelerTool::Rectangle => LiveLabel::Dimensions,
            ModelerTool::Circle => LiveLabel::Radius,
            ModelerTool::PushPull => LiveLabel::Distance,
            _ => LiveLabel::Length,
        };
        if !self.modeling.typed().is_empty() {
            return Some(Measurement::Live(
                live.map_or(default, |l| l.0),
                self.modeling.typed().to_string(),
            ));
        }
        Some(match live {
            Some((l, t)) => Measurement::Live(l, t),
            None => Measurement::Live(default, String::new()),
        })
    }

    // ---- keys --------------------------------------------------------------------------------

    /// An arrow key while a drawing tool runs: locks the inference to an axis. True if used.
    pub fn arrow(&mut self, key: egui::Key) -> bool {
        if !self.modeling.in_progress() || !modeling::is_drawing_tool(self.modeling.tool()) {
            return false;
        }
        let k = match key {
            egui::Key::ArrowRight => Key::Right,
            egui::Key::ArrowLeft => Key::Left,
            egui::Key::ArrowUp => Key::Up,
            egui::Key::ArrowDown => Key::Down,
            _ => return false,
        };
        self.inferencer.key(k);
        true
    }

    // ---- the frame ---------------------------------------------------------------------------

    /// Handles the pointer for one frame: inference, the tools, selection. Runs the finished
    /// actions on the session.
    pub fn pointer(
        &mut self,
        session: &mut Session,
        view3d: &mut View3d,
        nav_cam: &kadakas_render::Camera,
        inp: &Input,
    ) {
        let rev = session.editor.revision();
        let t0 = Instant::now();
        let before = self.cache.builds;
        self.cache.get(session.editor.doc(), rev);
        if self.cache.builds != before {
            self.last_build = t0.elapsed();
        }
        self.sub.retain_existing(session.editor.doc());
        let Some(cam) = modeling::infer_camera(nav_cam, inp.scene.origin, inp.size) else {
            return;
        };
        let local = |p: Pos2| {
            DVec2::new(
                f64::from(p.x - inp.rect.min.x),
                f64::from(p.y - inp.rect.min.y),
            )
        };
        let now = inp.ui.input(|i| i.time);
        let hover = if inp.resp.hovered() {
            inp.ui.input(|i| i.pointer.hover_pos())
        } else {
            None
        };
        let tool = inp.tool;
        let drawing = modeling::is_drawing_tool(tool);

        // Shift keeps the inference.
        if inp.mods.shift != self.shift_down {
            self.shift_down = inp.mods.shift;
            self.inferencer.key(if inp.mods.shift {
                Key::ShiftDown
            } else {
                Key::ShiftUp
            });
        }

        let mut out = Out::default();
        let mut selection_changed = false;
        let mut object_pick: Option<Option<kadakas_core::model::ObjectId>> = None;
        let mut triple_model: Option<kadakas_core::model::ObjectId> = None;
        {
            let doc = session.editor.doc();
            let scenes = self.cache.get(doc, rev);
            let world = World { doc, scenes };
            self.inferencer.options.ground_z = Some(world.ground_z());

            // The inference under the pointer, for the drawing tools.
            let selected_model = session
                .selection
                .iter()
                .copied()
                .find(|id| modeling::model_of(doc, *id).is_some());
            if let (true, Some(h)) = (drawing, hover) {
                let cursor = local(h);
                let inf = world.infer(
                    &mut self.inferencer,
                    &cam,
                    cursor,
                    self.modeling.reference(),
                    &self.modeling.excluded(),
                );
                if self.last_pointer != Some(h) {
                    self.last_pointer = Some(h);
                    self.moved_at = now;
                    self.paused = false;
                }
                let ray = cam.ray(cursor);
                let p = Pointer {
                    inf: &inf,
                    ray,
                    selected: selected_model,
                };
                self.modeling.moved(&world, &p);
                self.hover = Some(inf.clone());
                let primary = egui::PointerButton::Primary;
                if tool == ModelerTool::Eraser {
                    if inp.resp.drag_started_by(primary) || inp.resp.clicked_by(primary) {
                        self.modeling.press(&world, &p);
                    }
                    if inp.resp.drag_stopped_by(primary) || inp.resp.clicked_by(primary) {
                        out = self.modeling.release();
                    }
                } else if inp.resp.double_clicked_by(primary) && tool == ModelerTool::PushPull {
                    out = self.modeling.double_click(&world, &p);
                } else if inp.resp.clicked_by(primary) {
                    out = self.modeling.click(&world, &p);
                    // The next segment starts fresh.
                    self.inferencer.release_locks();
                }
                if !self.paused && now - self.moved_at >= PAUSE_SECONDS {
                    self.inferencer.pause();
                    self.paused = true;
                } else if !self.paused {
                    inp.ui
                        .ctx()
                        .request_repaint_after(Duration::from_secs_f64(PAUSE_SECONDS));
                }
            } else {
                self.hover = None;
            }

            // The Select tool: parts of surface models first, then whole objects.
            if tool == ModelerTool::Select {
                let primary = egui::PointerButton::Primary;
                if inp.resp.drag_started_by(primary)
                    && let Some(o) = inp.ui.input(|i| i.pointer.press_origin())
                {
                    let s = Vec2::new(o.x - inp.rect.min.x, o.y - inp.rect.min.y);
                    self.band = Some((s, s));
                }
                if let (Some((s, _)), true, Some(h)) = (
                    self.band,
                    inp.resp.dragged_by(primary),
                    hover.or(inp.ui.input(|i| i.pointer.latest_pos())),
                ) {
                    self.band = Some((s, Vec2::new(h.x - inp.rect.min.x, h.y - inp.rect.min.y)));
                }
                if inp.resp.drag_stopped_by(primary)
                    && let Some((a, b)) = self.band.take()
                    && (b - a).abs().max_element() >= MIN_BAND
                {
                    let crossing = b.x < a.x;
                    let items = modeling::select::rect_select(
                        &world,
                        &cam,
                        DVec2::new(f64::from(a.x), f64::from(a.y)),
                        DVec2::new(f64::from(b.x), f64::from(b.y)),
                        crossing,
                    );
                    if inp.mods.shift {
                        self.sub.add(items);
                    } else {
                        self.sub.set(items);
                    }
                    selection_changed = true;
                }
                if inp.resp.clicked_by(primary)
                    && let Some(p) = inp.resp.interact_pointer_pos()
                {
                    let hit = modeling::select::pick(&world, &mut self.picker, &cam, local(p));
                    if inp.resp.triple_clicked_by(primary) {
                        triple_model = hit.map(Item::model);
                    } else if inp.resp.double_clicked_by(primary) {
                        // A face with its edges.
                        if let Some(Item::Face(m, f)) = hit {
                            let mut items = vec![Item::Face(m, f)];
                            items.extend(
                                modeling::select::face_edges(doc, m, f)
                                    .into_iter()
                                    .map(|e| Item::Edge(m, e)),
                            );
                            self.sub.set(items);
                            selection_changed = true;
                        }
                    } else if hit.is_some() {
                        self.sub.click(hit, inp.mods.shift);
                        selection_changed = true;
                    } else {
                        object_pick = Some(None);
                    }
                }
            } else {
                self.band = None;
            }

            // What to draw.
            self.preview = if drawing {
                self.modeling.preview(&world)
            } else {
                Preview::default()
            };
            refresh_visuals(&mut self.visuals, &self.sub, &world, rev);
        }

        if let Some(model) = triple_model {
            self.sub.clear();
            session.selection = vec![model];
        } else if selection_changed {
            if !self.sub.is_empty() {
                session.selection.clear();
            }
        } else if let Some(None) = object_pick {
            // Not on a part: a whole object (or nothing).
            view3d.camera = *nav_cam;
            let size = inp.size;
            if let Some(p) = inp.resp.interact_pointer_pos() {
                let at = Vec2::new(p.x - inp.rect.min.x, p.y - inp.rect.min.y);
                let hit = view3d
                    .pick(inp.scene, at, size)
                    .filter(|id| session.editor.doc().object(*id).is_some());
                if hit.is_some() || !inp.mods.shift {
                    self.sub.clear();
                }
                session.selection =
                    view3d::click_selection(&session.selection, hit, inp.mods.shift);
            }
        }

        self.run(session, out);
    }

    /// Runs the actions of a tool on the session (one undo step) and tells the tool.
    pub fn run(&mut self, session: &mut Session, out: Out) {
        if let Some(m) = out.message {
            self.message = Some(m);
        }
        if out.actions.is_empty() {
            return;
        }
        match modeling::run_actions(&mut session.editor, &out.actions, out.redo) {
            Ok(used) => {
                self.modeling.committed(used, session.editor.revision());
                self.message = None;
            }
            Err(e) => {
                self.message = Some(tr!("modeler-edit-failed", reason = e));
                // A step that was taken back for a redo is not given back: the person sees the
                // message and can undo or redo.
            }
        }
    }

    /// Delete: the selected faces and edges, else the selected objects.
    pub fn delete(&mut self, session: &mut Session) {
        if !self.sub.is_empty() {
            let actions = modeling::delete_actions(&self.sub);
            self.sub.clear();
            self.run(
                session,
                Out {
                    actions,
                    ..Out::default()
                },
            );
        } else if !session.selection.is_empty() {
            let ids = std::mem::take(&mut session.selection);
            if let Err(e) = session.editor.execute(&Command::DeleteObjects { ids }) {
                self.message = Some(tr!("modeler-edit-failed", reason = e.to_string()));
            }
        } else {
            self.message = Some(tr!("modeler-delete-nothing"));
        }
    }

    /// The hatch of the selected faces as lines of the GPU overlay, and a key for the cache.
    pub fn overlay(&self, origin: DVec3) -> (u64, Vec<LineVertex>) {
        let Some(v) = &self.visuals else {
            return (0, Vec::new());
        };
        let at = |p: DVec3| (p - origin).as_vec3().to_array();
        let lines = v
            .hatch
            .iter()
            .flat_map(|s| LineVertex::segment(at(s[0]), at(s[1]), SELECTED))
            .collect();
        (v.key ^ hash(&origin.to_array().map(f64::to_bits)), lines)
    }

    // ---- drawing -----------------------------------------------------------------------------

    /// Draws the marks of the tools over the scene.
    pub fn paint(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        nav_cam: &kadakas_render::Camera,
        scene_origin: DVec3,
        size: Vec2,
    ) {
        let Some(cam) = modeling::infer_camera(nav_cam, scene_origin, size) else {
            return;
        };
        let px = |p: DVec3| {
            cam.project(p)
                .map(|q| Pos2::new(rect.min.x + q.x as f32, rect.min.y + q.y as f32))
        };
        let seg = |s: &[DVec3; 2], stroke: Stroke| {
            if let (Some(a), Some(b)) = (px(s[0]), px(s[1])) {
                painter.line_segment([a, b], stroke);
            }
        };
        // Selected edges.
        if let Some(v) = &self.visuals {
            for s in &v.edges {
                seg(
                    s,
                    Stroke::new(
                        3.0,
                        Color32::from_rgb(SELECTED[0], SELECTED[1], SELECTED[2]),
                    ),
                );
            }
        }
        let dark = Color32::from_rgb(30, 30, 30);
        for s in &self.preview.shape {
            seg(s, Stroke::new(1.5, dark));
        }
        for s in &self.preview.erase {
            seg(s, Stroke::new(3.0, Color32::from_rgb(220, 50, 50)));
        }
        for s in &self.preview.pull {
            seg(s, Stroke::new(1.5, Color32::from_rgb(30, 100, 235)));
        }
        // The inference: cue lines, the mark and the label.
        if let Some(inf) = &self.hover
            && modeling::is_drawing_tool(self.modeling.tool())
        {
            for l in &inf.lines {
                if let (Some(a), Some(b)) = (px(l.from), px(l.to)) {
                    let stroke = Stroke::new(1.0, color(l.color));
                    if l.dotted {
                        painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 3.0, 4.0));
                    } else {
                        painter.line_segment([a, b], stroke);
                    }
                }
            }
            if inf.kind != infer3d::Kind::Free
                && let Some(p) = px(inf.point)
            {
                let c = color(inf.color);
                if inf.kind.is_point() {
                    painter.rect_filled(Rect::from_center_size(p, egui::vec2(8.0, 8.0)), 1.0, c);
                } else {
                    painter.circle_filled(p, 4.0, c);
                }
                if !inf.label.is_empty() {
                    painter.text(
                        p + egui::vec2(10.0, -10.0),
                        egui::Align2::LEFT_BOTTOM,
                        &inf.label,
                        egui::FontId::proportional(12.0),
                        c,
                    );
                }
            }
        }
        // The selection rectangle: solid for a window, dashed for a crossing.
        if let Some((a, b)) = self.band
            && (b - a).abs().max_element() >= MIN_BAND
        {
            let r = Rect::from_two_pos(
                Pos2::new(rect.min.x + a.x, rect.min.y + a.y),
                Pos2::new(rect.min.x + b.x, rect.min.y + b.y),
            );
            let c = Color32::from_rgb(SELECTED[0], SELECTED[1], SELECTED[2]);
            if b.x < a.x {
                painter.extend(egui::Shape::dashed_line(
                    &[
                        r.left_top(),
                        r.right_top(),
                        r.right_bottom(),
                        r.left_bottom(),
                        r.left_top(),
                    ],
                    Stroke::new(1.0, c),
                    4.0,
                    3.0,
                ));
            } else {
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, c), egui::StrokeKind::Inside);
            }
        }
    }
}
