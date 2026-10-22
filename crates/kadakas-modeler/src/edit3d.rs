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
use kadakas_ui_core::infer3d::{
    self, GUIDE, GUIDE_DRAW_REACH, Inference, Inferencer, Key, PAUSE_SECONDS, Rgb,
};
use kadakas_ui_core::keys::Modifiers;
use kadakas_ui_core::modeler::ModelerTool;
use kadakas_ui_core::modeling::{
    self, Item, Modeling, Out, Pointer, Preview, Resize, SceneCache, SubSelection, World, XContext,
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
    /// Finds faces only (no snapping to points and edges): the Paint Bucket.
    painter: Inferencer,
    /// The color of the material in the bucket, for the mark at the pointer.
    bucket_color: Option<[u8; 4]>,
    /// How long the last build of the inference scene took.
    pub last_build: Duration,
    /// The world position of the local origin of the scene in the last frame.
    origin: Option<DVec3>,
    moved_at: f64,
    paused: bool,
    last_pointer: Option<Pos2>,
    shift_down: bool,
    /// The copy key (Ctrl, Option on a Mac) was down in the last frame: a press toggles copy.
    copy_down: bool,
    /// The copy key went down alone and nothing else was pressed since.
    copy_arm: bool,
    /// The last inference under the pointer, for the marks.
    hover: Option<Inference>,
    preview: Preview,
    band: Option<(Vec2, Vec2)>,
    /// A message for the status line (a refused value, a failed edit).
    pub message: Option<String>,
    visuals: Option<Visuals>,
    /// The guides and the hidden items to draw, for one revision and set of options.
    extras: Extras,
    /// The Tape Measure asked to resize the model: the program asks the person.
    pub resize_request: Option<Resize>,
}

/// A guide to draw, in world coordinates.
#[derive(Clone, Copy, Debug)]
struct DrawGuide {
    id: kadakas_core::model::ObjectId,
    at: DVec3,
    dir: Option<DVec3>,
    length: Option<f64>,
    hidden: bool,
}

/// The guides, and the hidden items that View > Hidden Geometry and Hidden Objects show.
#[derive(Default)]
struct Extras {
    key: u64,
    guides: Vec<DrawGuide>,
    /// Hidden edges (and all edges of hidden objects), drawn dotted.
    ghost_edges: Vec<[DVec3; 2]>,
    /// Lines across hidden faces, drawn dotted.
    ghost_hatch: Vec<[DVec3; 2]>,
    /// The guides that are selected as objects.
    selected: std::collections::BTreeSet<kadakas_core::model::ObjectId>,
}

/// Most segments of hidden items that are drawn.
const MAX_GHOST: usize = 200_000;
/// Lines across a hidden face.
const GHOST_HATCH_LINES: usize = 3;
/// With more hidden faces than this in one model a hidden face gets one line.
const MANY_HIDDEN_FACES: usize = 2_000;
/// Most ghost lines drawn in one frame. More are drawn with a stride, so the frame stays fast.
const MAX_GHOST_DRAWN: usize = 20_000;
/// More ghost lines than this are drawn solid: a dash costs a shape for every piece.
const MAX_GHOST_DASHED: usize = 2_000;

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
            painter: modeling::paint::face_engine(),
            bucket_color: None,
            last_build: Duration::ZERO,
            origin: None,
            moved_at: 0.0,
            paused: false,
            last_pointer: None,
            shift_down: false,
            copy_down: false,
            copy_arm: false,
            hover: None,
            preview: Preview::default(),
            band: None,
            message: None,
            visuals: None,
            extras: Extras::default(),
            resize_request: None,
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

/// Finds the guides and the ghosts of hidden items for the world.
fn refresh_extras(extras: &mut Extras, world: &World, rev: u64) {
    let show = world.scenes.show;
    let key = hash(&(rev, show.guides, show.hidden_geometry, show.hidden_objects));
    if extras.key == key && key != 0 {
        return;
    }
    extras.key = key;
    extras.guides.clear();
    extras.ghost_edges.clear();
    extras.ghost_hatch.clear();
    let doc = world.doc;
    for &id in world.scenes.guide_ids() {
        let Some(o) = doc.object(id) else { continue };
        let kadakas_core::model::Shape::Guide(g) = &o.shape else {
            continue;
        };
        extras.guides.push(DrawGuide {
            id,
            at: g.at + DVec3::new(0.0, 0.0, modeling::elevation(doc, id)),
            dir: g.unit_dir(),
            length: g.length,
            hidden: kadakas_core::visibility::is_hidden(o),
        });
    }
    if !(show.hidden_geometry || show.hidden_objects) {
        return;
    }
    'models: for &id in world.scenes.objects() {
        let Some(m) = modeling::model_of(doc, id) else {
            continue;
        };
        let whole = doc
            .object(id)
            .is_some_and(kadakas_core::visibility::is_hidden);
        for (eid, _) in m.edges() {
            if (whole || m.edge_hidden(eid))
                && let Some(s) = world.edge_ends(id, eid)
            {
                extras.ghost_edges.push(s);
            }
        }
        // A model with very many hidden faces gets one line across each, not several.
        let lines = if m.hidden_faces().len() > MANY_HIDDEN_FACES {
            1
        } else {
            GHOST_HATCH_LINES
        };
        for (fid, _) in m.faces() {
            if m.face_hidden(fid) && !whole {
                extras.ghost_hatch.extend(modeling::select::face_hatch(
                    &world.face_loops(id, fid),
                    lines,
                ));
            }
        }
        if extras.ghost_edges.len() + extras.ghost_hatch.len() > MAX_GHOST {
            break 'models;
        }
    }
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
        self.extras = Extras::default();
        self.resize_request = None;
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
            nav.shift_world((old - origin).as_vec3());
        }
        self.origin = Some(origin);
    }

    /// The text for the Measurements box: what is typed, else the live value of the tool.
    pub fn measurement(&self) -> Option<kadakas_ui_core::modeler::Measurement> {
        use kadakas_ui_core::modeler::Measurement;
        use modeling::LiveLabel;
        let tool = self.modeling.tool();
        if !modeling::takes_values(tool) {
            return None;
        }
        let units = kadakas_units::current();
        let live = self.modeling.live(&units);
        let default = match tool {
            ModelerTool::Rectangle => LiveLabel::Dimensions,
            ModelerTool::Circle => LiveLabel::Radius,
            ModelerTool::PushPull | ModelerTool::TapeMeasure => LiveLabel::Distance,
            ModelerTool::Rotate => LiveLabel::Angle,
            ModelerTool::Scale => LiveLabel::Factor,
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

    /// The inference under the pointer in the last frame: its kind, label and source.
    #[cfg(test)]
    pub fn hover_inference(&self) -> Option<&Inference> {
        self.hover.as_ref()
    }

    // ---- keys --------------------------------------------------------------------------------

    /// An arrow key while a drawing tool runs: locks the inference to an axis. True if used.
    pub fn arrow(&mut self, key: egui::Key) -> bool {
        let k = match key {
            egui::Key::ArrowRight => Key::Right,
            egui::Key::ArrowLeft => Key::Left,
            egui::Key::ArrowUp => Key::Up,
            egui::Key::ArrowDown => Key::Down,
            _ => return false,
        };
        // The protractor of Rotate takes the arrow keys before its center is clicked.
        if !self.modeling.in_progress() && self.modeling.arrow_plane(k) {
            return true;
        }
        if !self.modeling.in_progress() || !modeling::takes_values(self.modeling.tool()) {
            return false;
        }
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
        self.modeling.seen_revision(rev);
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
        let xform = modeling::is_transform_tool(tool);
        let tape = tool == ModelerTool::TapeMeasure;

        // Shift keeps the inference.
        if inp.mods.shift != self.shift_down {
            self.shift_down = inp.mods.shift;
            self.inferencer.key(if inp.mods.shift {
                Key::ShiftDown
            } else {
                Key::ShiftUp
            });
        }

        if xform {
            self.prepare_transform(session, view3d, nav_cam, inp, &cam, rev);
        } else if tape {
            self.ctrl_key(inp);
        } else {
            self.copy_down = false;
        }

        let mut out = Out::default();
        let mut selection_changed = false;
        let mut object_pick: Option<Option<kadakas_core::model::ObjectId>> = None;
        let mut triple: Option<Item> = None;
        let mut guide_hit: Option<kadakas_core::model::ObjectId> = None;
        let mut rect_objects: Option<Vec<kadakas_core::model::ObjectId>> = None;
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
            if let (true, Some(h)) = (drawing || xform || tape, hover) {
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

            // The Paint Bucket: a click paints the side of the face under the pointer, or the
            // objects (see `kadakas_ui_core::modeling::paint`).
            self.bucket_color = self
                .modeling
                .paint
                .material(doc)
                .and_then(|m| kadakas_core::present::material(doc, m))
                .map(|m| m.base_color.0);
            if tool == ModelerTool::PaintBucket {
                let primary = egui::PointerButton::Primary;
                if hover.is_some() {
                    self.last_pointer = hover;
                }
                if inp.resp.clicked_by(primary)
                    && let Some(p) = inp.resp.interact_pointer_pos()
                {
                    let at = local(p);
                    let (mut hit, point) =
                        modeling::paint::hit_face_at(&world, &mut self.painter, &cam, at);
                    // An object in front of the face (a tree in front of a wall) is the target.
                    if let Some(p) = point
                        && let Some(o) =
                            modeling::select::object_in_front(doc, inp.scene, &cam, at, p)
                    {
                        hit = modeling::paint::PaintHit::Object(o);
                    }
                    if hit == modeling::paint::PaintHit::Nothing {
                        view3d.camera = *nav_cam;
                        let px = Vec2::new(at.x as f32, at.y as f32);
                        if let Some(id) = view3d
                            .pick(inp.scene, px, inp.size)
                            .filter(|id| doc.object(*id).is_some())
                        {
                            hit = modeling::paint::PaintHit::Object(id);
                        }
                    }
                    let mods = modeling::paint::PaintMods {
                        ctrl: inp.mods.ctrl,
                        shift: inp.mods.shift,
                        // Alt, and Cmd on a Mac (Ctrl is the connected fill).
                        sample: inp.mods.alt || (cfg!(target_os = "macos") && inp.mods.command),
                    };
                    out = self
                        .modeling
                        .paint
                        .click(doc, hit, &session.selection, mods);
                }
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
                    if items.is_empty() {
                        // No face or edge in the rectangle: the objects that are no surface
                        // models (plants, hardscapes, site models, instances), and the guides.
                        let mut found = modeling::select::rect_select_objects(
                            doc,
                            inp.scene,
                            &cam,
                            DVec2::new(f64::from(a.x), f64::from(a.y)),
                            DVec2::new(f64::from(b.x), f64::from(b.y)),
                            crossing,
                        );
                        found.extend(modeling::select::rect_guides(
                            &world,
                            &cam,
                            DVec2::new(f64::from(a.x), f64::from(a.y)),
                            DVec2::new(f64::from(b.x), f64::from(b.y)),
                            crossing,
                        ));
                        if found.is_empty() {
                            self.sub.set(items);
                            selection_changed = true;
                        } else {
                            rect_objects = Some(found);
                        }
                    } else {
                        if inp.mods.shift {
                            self.sub.add(items);
                        } else {
                            self.sub.set(items);
                        }
                        selection_changed = true;
                    }
                }
                if inp.resp.clicked_by(primary)
                    && let Some(p) = inp.resp.interact_pointer_pos()
                {
                    let hit = modeling::select::pick_visible(
                        &world,
                        &mut self.picker,
                        &cam,
                        local(p),
                        inp.scene,
                    );
                    if inp.resp.triple_clicked_by(primary) {
                        // Everything connected to what was hit.
                        triple = hit;
                    } else if inp.resp.double_clicked_by(primary) {
                        // A face with its edges, or an edge with its faces.
                        if let Some(item) = hit {
                            let near = modeling::select::with_neighbors(doc, item);
                            let near = if world.scenes.show.hidden_geometry {
                                near
                            } else {
                                modeling::select::without_hidden(doc, near)
                            };
                            self.sub.set(near);
                            selection_changed = true;
                        }
                    } else if hit.is_some() {
                        self.sub.click(hit, inp.mods.shift);
                        selection_changed = true;
                    } else {
                        guide_hit =
                            modeling::select::pick_guide(&world, &mut self.picker, &cam, local(p));
                        object_pick = Some(None);
                    }
                }
            } else {
                self.band = None;
            }

            // What to draw.
            self.preview = if drawing || xform || tape {
                self.modeling.preview(&world)
            } else {
                Preview::default()
            };
            refresh_visuals(&mut self.visuals, &self.sub, &world, rev);
            refresh_extras(&mut self.extras, &world, rev);
        }
        self.extras.selected = session.selection.iter().copied().collect();

        if let Some(found) = rect_objects {
            // A window or crossing that holds objects: they become the selection (Shift adds).
            self.sub.clear();
            if inp.mods.shift {
                for id in found {
                    if !session.selection.contains(&id) {
                        session.selection.push(id);
                    }
                }
            } else {
                session.selection = found;
            }
        } else if let Some(item) = triple {
            let all = modeling::select::connected(session.editor.doc(), item);
            let all = if self.cache.show.hidden_geometry {
                all
            } else {
                modeling::select::without_hidden(session.editor.doc(), all)
            };
            self.sub.set(all);
            session.selection.clear();
        } else if selection_changed {
            if !self.sub.is_empty() {
                session.selection.clear();
            }
        } else if let Some(None) = object_pick {
            // A click that is on no face or edge of a surface model: the object under the
            // pointer (or nothing). It replaces the selection of parts, but Shift on empty space
            // keeps it.
            view3d.camera = *nav_cam;
            let size = inp.size;
            if let Some(p) = inp.resp.interact_pointer_pos() {
                let at = Vec2::new(p.x - inp.rect.min.x, p.y - inp.rect.min.y);
                // A guide under the pointer comes before the object behind it.
                let hit = guide_hit.or_else(|| {
                    view3d
                        .pick(inp.scene, at, size)
                        .filter(|id| session.editor.doc().object(*id).is_some())
                });
                if hit.is_some() || !inp.mods.shift {
                    self.sub.clear();
                }
                session.selection =
                    view3d::click_selection(&session.selection, hit, inp.mods.shift);
            }
        }

        self.run(session, out);
    }

    /// Before the pointer events of Move, Rotate and Scale: the copy key, the pick of what is under
    /// the pointer when nothing is selected, and the selection for the tool.
    fn prepare_transform(
        &mut self,
        session: &mut Session,
        view3d: &mut View3d,
        nav_cam: &kadakas_render::Camera,
        inp: &Input,
        cam: &infer3d::Camera,
        rev: u64,
    ) {
        let center = self.ctrl_key(inp);
        // With nothing selected, a click picks what is under the pointer first, as Select does:
        // a face or an edge of a surface model, else an object.
        let primary = egui::PointerButton::Primary;
        self.prepare_pick(session, view3d, nav_cam, inp, cam, rev, primary);
        self.finish_context(session, inp, center);
    }

    /// Ctrl (Option on a Mac) toggles copy (Move, Rotate) or guides (Tape Measure) when it is
    /// pressed alone and let go without another key or button (Ctrl+Z and the like do not
    /// toggle). Scale takes it held, as Shift. Returns whether the key is held.
    fn ctrl_key(&mut self, inp: &Input) -> bool {
        let (key, others) = if cfg!(target_os = "macos") {
            (
                inp.mods.alt,
                inp.mods.ctrl || inp.mods.command || inp.mods.shift,
            )
        } else {
            (inp.mods.ctrl, inp.mods.alt || inp.mods.shift)
        };
        let used = inp.ui.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key { pressed: true, .. }
                        | egui::Event::PointerButton { pressed: true, .. }
                )
            })
        });
        if key && (others || used) {
            self.copy_arm = false;
        } else if key && !self.copy_down {
            self.copy_arm = true;
        }
        if !key && self.copy_down && self.copy_arm && inp.tool != ModelerTool::Scale {
            self.modeling.toggle_ctrl();
            if inp.tool == ModelerTool::TapeMeasure {
                // Say which mode the Tape Measure is in now.
                self.message = Some(if self.modeling.tape_creates_guides() {
                    tr!("modeler-tape-guides")
                } else {
                    tr!("modeler-tape-measure-only")
                });
            }
        }
        if !key {
            self.copy_arm = false;
        }
        self.copy_down = key;
        key
    }

    /// With nothing selected, a click picks what is under the pointer first.
    #[allow(clippy::too_many_arguments)]
    fn prepare_pick(
        &mut self,
        session: &mut Session,
        view3d: &mut View3d,
        nav_cam: &kadakas_render::Camera,
        inp: &Input,
        cam: &infer3d::Camera,
        rev: u64,
        primary: egui::PointerButton,
    ) {
        if inp.resp.clicked_by(primary)
            && session.selection.is_empty()
            && self.sub.is_empty()
            && !self.modeling.in_progress()
            && let Some(p) = inp.resp.interact_pointer_pos()
        {
            let at = Vec2::new(p.x - inp.rect.min.x, p.y - inp.rect.min.y);
            let hit = {
                let doc = session.editor.doc();
                let scenes = self.cache.get(doc, rev);
                let world = World { doc, scenes };
                modeling::select::pick_visible(
                    &world,
                    &mut self.picker,
                    cam,
                    DVec2::new(f64::from(at.x), f64::from(at.y)),
                    inp.scene,
                )
            };
            match hit {
                Some(item) => self.sub.set([item]),
                None => {
                    view3d.camera = *nav_cam;
                    if let Some(id) = view3d
                        .pick(inp.scene, at, inp.size)
                        .filter(|id| session.editor.doc().object(*id).is_some())
                    {
                        session.selection = vec![id];
                    }
                }
            }
        }
    }

    /// The selection and the keys for Move, Rotate and Scale.
    fn finish_context(&mut self, session: &mut Session, inp: &Input, center: bool) {
        // The box around the selected objects that are not surface models (the tool measures
        // those itself).
        let doc = session.editor.doc();
        let mut object_bounds: Option<(DVec3, DVec3)> = None;
        for id in &session.selection {
            if modeling::model_of(doc, *id).is_some() {
                continue;
            }
            if let Some((lo, hi)) = inp.scene.object_bounds(*id) {
                let (lo, hi) = (
                    lo.as_dvec3() + inp.scene.origin,
                    hi.as_dvec3() + inp.scene.origin,
                );
                object_bounds = Some(match object_bounds {
                    None => (lo, hi),
                    Some((a, b)) => (a.min(lo), b.max(hi)),
                });
            }
        }
        self.modeling.set_context(XContext {
            objects: session.selection.clone(),
            parts: self.sub.clone(),
            object_bounds,
            shift: inp.mods.shift,
            center,
        });
    }

    /// Runs the actions of a tool on the session (one undo step) and tells the tool.
    pub fn run(&mut self, session: &mut Session, out: Out) {
        let done_message = out.message.clone();
        if let Some(m) = out.message {
            self.message = Some(m);
        }
        if out.resize.is_some() {
            self.resize_request = out.resize;
        }
        if out.actions.is_empty() {
            return;
        }
        match modeling::run_actions_full(&mut session.editor, &out.actions, out.redo) {
            Ok(ran) => {
                self.modeling.committed(ran.used, session.editor.revision());
                // What the edit changed that was not asked for (a material of a merged face),
                // else what the tool says about it (the Paint Bucket tells what it painted).
                self.message = if ran.notes.is_empty() {
                    done_message
                } else {
                    Some(tr!("modeler-edit-note", note = ran.notes.join("; ")))
                };
                // The copies of a Move or Rotate become the selection: the next one moves them.
                if !ran.objects.is_empty() {
                    self.sub.clear();
                    session.selection = ran.objects;
                } else if !ran.parts.is_empty() {
                    session.selection.clear();
                    self.sub.set(ran.parts);
                }
            }
            Err(e) => {
                self.message = Some(tr!("modeler-edit-failed", reason = e));
                // A step that was taken back for a redo is back again (`exec::run`), so the tool
                // can redo it with a corrected value.
                self.modeling.refused(session.editor.revision());
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

    /// Draws the guides (thin dashed lines and small crosses), the ghosts of hidden items, and the
    /// guide that the Tape Measure or the Eraser is about to make or erase.
    fn paint_extras(&self, painter: &egui::Painter, rect: Rect, cam: &infer3d::Camera) {
        let px = |p: DVec2| Pos2::new(rect.min.x + p.x as f32, rect.min.y + p.y as f32);
        let amber = color(GUIDE);
        let dashed = |a: DVec3, b: DVec3, stroke: Stroke, dash: f32, gap: f32| {
            if let Some([p, q]) = cam.view_segment(a, b) {
                painter.extend(egui::Shape::dashed_line(&[px(p), px(q)], stroke, dash, gap));
            }
        };
        let guide_line = |at: DVec3, dir: DVec3, length: Option<f64>, stroke: Stroke| match length {
            Some(l) => dashed(at, at + dir * l, stroke, 6.0, 4.0),
            None => dashed(
                at - dir * GUIDE_DRAW_REACH,
                at + dir * GUIDE_DRAW_REACH,
                stroke,
                6.0,
                4.0,
            ),
        };
        let cross = |at: DVec3, stroke: Stroke| {
            if let Some(p) = cam.project(at) {
                let c = px(p);
                painter.line_segment([c - egui::vec2(4.0, 4.0), c + egui::vec2(4.0, 4.0)], stroke);
                painter.line_segment(
                    [c - egui::vec2(4.0, -4.0), c + egui::vec2(4.0, -4.0)],
                    stroke,
                );
            }
        };
        let erase = self.preview.erase_guides.clone();
        for g in &self.extras.guides {
            let (stroke, width) = if erase.contains(&g.id) {
                (Color32::from_rgb(220, 50, 50), 2.0)
            } else if self.extras.selected.contains(&g.id) {
                (
                    Color32::from_rgb(SELECTED[0], SELECTED[1], SELECTED[2]),
                    2.0,
                )
            } else if g.hidden {
                (amber.gamma_multiply(0.45), 1.0)
            } else {
                (amber, 1.0)
            };
            let stroke = Stroke::new(width, stroke);
            match g.dir {
                Some(d) => guide_line(g.at, d, g.length, stroke),
                None => cross(g.at, stroke),
            }
        }
        // The guide the Tape Measure would make.
        for g in &self.preview.guides {
            guide_line(g.at, g.dir, g.length, Stroke::new(1.5, amber));
        }
        // Hidden geometry and hidden objects, when the person asked to see them.
        let ghost = Stroke::new(1.0, Color32::from_gray(140));
        let all = self.extras.ghost_edges.len() + self.extras.ghost_hatch.len();
        let stride = all.div_ceil(MAX_GHOST_DRAWN).max(1);
        for s in self
            .extras
            .ghost_edges
            .iter()
            .chain(&self.extras.ghost_hatch)
            .step_by(stride)
        {
            if let Some([p, q]) = cam.view_segment(s[0], s[1]) {
                if all > MAX_GHOST_DASHED {
                    painter.line_segment([px(p), px(q)], ghost);
                } else {
                    painter.extend(egui::Shape::dashed_line(&[px(p), px(q)], ghost, 2.0, 3.0));
                }
            }
        }
    }

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
        self.paint_extras(painter, rect, &cam);
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
        // The grips of Scale on the box around the selection.
        for (i, g) in self.preview.grips.iter().enumerate() {
            if let Some(c) = px(*g) {
                let hot = self.preview.grip_hover == Some(i);
                let fill = if hot {
                    Color32::from_rgb(SELECTED[0], SELECTED[1], SELECTED[2])
                } else {
                    Color32::from_rgb(245, 245, 245)
                };
                let r = Rect::from_center_size(c, egui::vec2(8.0, 8.0));
                painter.rect_filled(r, 1.0, fill);
                painter.rect_stroke(r, 1.0, Stroke::new(1.0, dark), egui::StrokeKind::Inside);
            }
        }
        // The color of the bucket at the pointer.
        if self.modeling.tool() == ModelerTool::PaintBucket
            && let Some(p) = self.last_pointer
        {
            let c = self.bucket_color.unwrap_or([235, 235, 235, 255]);
            let at = p + egui::vec2(14.0, 14.0);
            painter.circle_filled(at, 7.0, Color32::from_rgb(c[0], c[1], c[2]));
            painter.circle_stroke(at, 7.0, Stroke::new(1.5, Color32::from_rgb(30, 30, 30)));
        }
        // A plus at the pointer while Move or Rotate copies.
        if self.preview.copy
            && let Some(p) = self.last_pointer
        {
            painter.text(
                p + egui::vec2(14.0, 14.0),
                egui::Align2::LEFT_TOP,
                "+",
                egui::FontId::proportional(16.0),
                Color32::from_rgb(SELECTED[0], SELECTED[1], SELECTED[2]),
            );
        }
        // The inference: cue lines, the mark and the label.
        if let Some(inf) = &self.hover
            && modeling::takes_values(self.modeling.tool())
            && self.modeling.tool() != ModelerTool::Scale
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

#[cfg(test)]
mod perf {
    use super::*;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::{Attributes, Shape};
    use kadakas_core::surface::Edit;

    /// A model of 160 x 160 cells (about 50,000 edges and 25,000 faces) with every face and edge
    /// hidden, and View > Hidden Geometry on: the cost of the ghosts for one revision. Run with
    /// `cargo test -p kadakas-modeler --release -- --ignored --nocapture perf_ghosts`.
    #[test]
    #[ignore]
    fn perf_ghosts_of_a_large_hidden_model() {
        let mut ed = Editor::with_sequential_ids();
        let id = kadakas_core::model::ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::default()),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let n = 160u32;
        let p = |i: u32, j: u32| [f64::from(i) * 100.0, f64::from(j) * 100.0, 0.0];
        let t = Instant::now();
        for j in 0..n {
            let mut edges: Vec<[[f64; 3]; 2]> = Vec::new();
            for i in 0..n {
                edges.push([p(i, j), p(i + 1, j)]);
                edges.push([p(i, j), p(i, j + 1)]);
                edges.push([p(i + 1, j), p(i + 1, j + 1)]);
                edges.push([p(i, j + 1), p(i + 1, j + 1)]);
            }
            ed.execute(&Command::EditSurface {
                id,
                edit: Edit::AddEdges { edges },
            })
            .unwrap();
        }
        let built = t.elapsed();
        let (faces, edges) = match &ed.doc().object(id).unwrap().shape {
            Shape::SurfaceModel(m) => (
                m.faces().map(|(f, _)| f).collect::<Vec<_>>(),
                m.edges().map(|(e, _)| e).collect::<Vec<_>>(),
            ),
            _ => panic!(),
        };
        let (nf, ne) = (faces.len(), edges.len());
        ed.execute(&Command::EditSurface {
            id,
            edit: Edit::SetHidden {
                faces,
                edges,
                hidden: true,
            },
        })
        .unwrap();
        let mut cache = SceneCache::default();
        cache.show.hidden_geometry = true;
        let rev = ed.revision();
        let t = Instant::now();
        let scenes = cache.get(ed.doc(), rev);
        let scene_ms = t.elapsed().as_secs_f64() * 1000.0;
        let world = World {
            doc: ed.doc(),
            scenes,
        };
        let mut extras = Extras::default();
        let t = Instant::now();
        refresh_extras(&mut extras, &world, rev);
        let ghost_ms = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        refresh_extras(&mut extras, &world, rev);
        let again_ms = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "ghosts: {nf} faces, {ne} edges (built in {:.1} s); inference scene with hidden geometry {scene_ms:.1} ms; ghosts {} edges + {} hatch lines in {ghost_ms:.1} ms; the same revision again {again_ms:.3} ms",
            built.as_secs_f64(),
            extras.ghost_edges.len(),
            extras.ghost_hatch.len()
        );
        // The cost of drawing them: the shapes of one frame and their tessellation.
        let edit = Edit3d {
            extras,
            ..Edit3d::default()
        };
        let ctx = egui::Context::default();
        let mut cam = kadakas_render::Camera::default();
        cam.set_view(kadakas_render::StandardView::Top);
        cam.target = glam::Vec3::new(8000.0, 8000.0, 0.0);
        cam.distance = 20000.0;
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let t = Instant::now();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter_at(rect);
            edit.paint(&painter, rect, &cam, DVec3::ZERO, Vec2::new(1600.0, 900.0));
        });
        let shapes = out.shapes.len();
        let meshes = ctx.tessellate(out.shapes, 1.0).len();
        let draw_ms = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "ghosts drawn: {shapes} shapes, {meshes} meshes in {draw_ms:.1} ms (shapes and tessellation)"
        );
        let extras = &edit.extras;
        assert!(extras.ghost_edges.len() + extras.ghost_hatch.len() <= MAX_GHOST + 200_000);
        assert!(
            again_ms < 1.0,
            "a frame with the same revision must not walk the model"
        );
    }
}
