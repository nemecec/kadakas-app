//! Surface models and component instances in the plan (MOD-13). The geometry is made by
//! `kadakas_core::surface::plan`, the same for the screen, the sheets and the SVG. Here it becomes
//! one egui mesh: the faces as triangles in the colours of their materials and the edges as thin
//! quads, in the order of the drawing, so a roof covers what is under it.
//!
//! The work is kept for a revision of the document: the plan of an object, with its faces already
//! cut into triangles. A frame only moves the triangles to the screen. A frame has a budget of
//! vertices: an object that would pass it is drawn as its box, and says so.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use egui::epaint::Mesh;
use egui::{Color32, Painter, Rect, Shape as EShape, Stroke};
use glam::DVec2;
use kadakas_core::model::{Color, Document, Object, ObjectId};
use kadakas_core::surface::plan::{self, Detail, Outcome, PlanDrawing, PlanItem};

use crate::canvas::{Style, draw_color};
use crate::tess;
use crate::view::{View, ViewExt as _};

/// Most vertices that the surface models of one frame add to the painter. 675,000 vertices took
/// 6.6 ms for the mesh and the tessellation (`docs/modeler-limits.md` D1), so this is about 4 ms
/// here, and a frame has 16 ms for everything. An object that would pass it is drawn as its box.
pub const FRAME_VERTEX_BUDGET: usize = 400_000;

/// Most plans that the cache keeps.
const MAX_ENTRIES: usize = 8192;

/// One thing of a plan with the work done that does not depend on the view.
pub enum Piece {
    Face {
        /// The points of the rings joined, and the triangles of them.
        points: Vec<DVec2>,
        triangles: Vec<[u32; 3]>,
        color: [u8; 4],
        lo: DVec2,
        hi: DVec2,
    },
    Edge {
        a: DVec2,
        b: DVec2,
    },
}

/// The plan of an object, ready for the frames.
pub struct Built {
    pub outcome: Outcome,
    pub pieces: Vec<Piece>,
    /// The box of the drawing.
    pub bounds: Option<(DVec2, DVec2)>,
    /// The vertices that the mesh needs when everything is drawn.
    pub cost: usize,
}

impl Built {
    /// Cuts the faces of a drawing into triangles.
    pub fn from_outcome(outcome: Outcome) -> Built {
        let mut pieces = Vec::new();
        let mut cost = 0usize;
        let mut bounds = None;
        if let Outcome::Drawing(d) = &outcome {
            bounds = d.bounds();
            for item in &d.items {
                match item {
                    PlanItem::Face(f) => {
                        if f.color[3] == 0 || f.rings.is_empty() {
                            continue;
                        }
                        let (points, tris) = tess::triangulate_with_holes(&f.rings);
                        let (mut lo, mut hi) =
                            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
                        for p in &f.rings[0] {
                            lo = lo.min(*p);
                            hi = hi.max(*p);
                        }
                        cost += points.len();
                        pieces.push(Piece::Face {
                            points,
                            triangles: tris
                                .into_iter()
                                .map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
                                .collect(),
                            color: f.color,
                            lo,
                            hi,
                        });
                    }
                    PlanItem::Edge(a, b) => {
                        cost += 4;
                        pieces.push(Piece::Edge { a: *a, b: *b });
                    }
                }
            }
        }
        Built {
            outcome,
            pieces,
            bounds,
            cost,
        }
    }

    /// The drawing, when the object has one.
    pub fn drawing(&self) -> Option<&PlanDrawing> {
        match &self.outcome {
            Outcome::Drawing(d) => Some(d),
            Outcome::Broken(..) => None,
        }
    }
}

struct Entry {
    revision: u64,
    used: u64,
    built: Rc<Built>,
}

#[derive(Default)]
struct PlanCache {
    map: HashMap<ObjectId, Entry>,
    tick: u64,
}

thread_local! {
    static PLANS: RefCell<PlanCache> = RefCell::new(PlanCache::default());
    static SPENT: Cell<usize> = const { Cell::new(0) };
    static BUDGET: Cell<usize> = const { Cell::new(FRAME_VERTEX_BUDGET) };
}

/// Sets the budget of vertices of a frame (tests use a small one).
#[cfg(test)]
pub fn set_frame_budget(vertices: usize) {
    BUDGET.with(|b| b.set(vertices));
}

/// A new frame: the budget of vertices is whole again. The canvas calls it before it draws.
pub fn begin_frame() {
    SPENT.with(|s| s.set(0));
}

/// The plan of an object. It is kept for one revision of the document, so a model is not drawn
/// again for every frame. Without a revision it is made again every time. When the cache is full
/// the entries of older revisions go first, then the ones that were used least recently.
pub fn built(doc: &Document, o: &Object, keep: bool) -> Option<Rc<Built>> {
    // A ghost of a compare has the IDs of live objects and another content: it is not kept.
    let revision = if keep && crate::annot_ui::is_live(doc) {
        crate::area_ui::current_revision()
    } else {
        None
    };
    let make = || plan::plan_of_object(doc, o).map(|p| Rc::new(Built::from_outcome(p)));
    let Some(rev) = revision else {
        return make();
    };
    PLANS.with(|c| {
        let mut c = c.borrow_mut();
        c.tick += 1;
        let tick = c.tick;
        if let Some(e) = c.map.get_mut(&o.id)
            && e.revision == rev
        {
            e.used = tick;
            return Some(e.built.clone());
        }
        let b = make()?;
        if c.map.len() >= MAX_ENTRIES {
            // The entries of other revisions, and then a quarter of the rest.
            c.map.retain(|_, e| e.revision == rev);
            if c.map.len() >= MAX_ENTRIES {
                let mut used: Vec<u64> = c.map.values().map(|e| e.used).collect();
                used.sort_unstable();
                let cut = used[used.len() / 4];
                c.map.retain(|_, e| e.used > cut);
            }
        }
        c.map.insert(
            o.id,
            Entry {
                revision: rev,
                used: tick,
                built: b.clone(),
            },
        );
        Some(b)
    })
}

/// How many plans the cache holds now.
#[cfg(test)]
pub fn cached() -> usize {
    PLANS.with(|c| c.borrow().map.len())
}

/// The mesh of a plan: what shows in `clip` (screen points).
pub fn plan_mesh(
    view: &View,
    offset: DVec2,
    b: &Built,
    pen: Color32,
    width: f32,
    clip: Rect,
) -> Mesh {
    let s = |p: DVec2| view.to_screen(p + offset);
    let mut mesh = Mesh::default();
    let half = width.max(1.0) / 2.0;
    for piece in &b.pieces {
        match piece {
            Piece::Face {
                points,
                triangles,
                color,
                lo,
                hi,
            } => {
                if !clip.intersects(Rect::from_two_pos(s(*lo), s(*hi))) {
                    continue;
                }
                let color = draw_color(Color(*color));
                let base = mesh.vertices.len() as u32;
                for p in points {
                    mesh.colored_vertex(s(*p), color);
                }
                for t in triangles {
                    mesh.add_triangle(base + t[0], base + t[1], base + t[2]);
                }
            }
            Piece::Edge { a, b } => {
                let (a, b) = (s(*a), s(*b));
                if !clip.intersects(Rect::from_two_pos(a, b).expand(half)) {
                    continue;
                }
                let d = b - a;
                if d.length() < 1e-3 {
                    continue;
                }
                let n = egui::vec2(-d.y, d.x).normalized() * half;
                let base = mesh.vertices.len() as u32;
                for p in [a + n, a - n, b - n, b + n] {
                    mesh.colored_vertex(p, pen);
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
        }
    }
    mesh
}

/// What the canvas says about a plan that is not the full one, or `None`.
pub fn note_of(d: &PlanDrawing, budget_hit: bool) -> Option<String> {
    if budget_hit {
        return Some(kadakas_l10n::tr!("surface-ui-note-budget"));
    }
    crate::surface_ui::simplified_text(d)
}

/// Draws a surface model or an instance in the plan. `keep` says that the object has its own place
/// in the document (not a part of a symbol), so its plan can be kept for the frames that follow.
pub fn draw(
    painter: &Painter,
    view: &View,
    doc: &Document,
    o: &Object,
    st: Style,
    offset: DVec2,
    keep: bool,
) {
    let Some(built) = built(doc, o, keep) else {
        return;
    };
    match &built.outcome {
        Outcome::Drawing(d) => {
            // The budget of the frame: an object over it is drawn as its box.
            let over = SPENT.with(|s| s.get()) + built.cost > BUDGET.with(Cell::get);
            let mesh = if over {
                let boxed = box_of(&built);
                plan_mesh(view, offset, &boxed, st.pen, st.width, painter.clip_rect())
            } else {
                SPENT.with(|s| s.set(s.get() + built.cost));
                plan_mesh(view, offset, &built, st.pen, st.width, painter.clip_rect())
            };
            if !mesh.is_empty() {
                painter.add(EShape::mesh(mesh));
            }
            // A note at the lower left corner of the object, like the notes of a pattern.
            if let Some(text) = note_of(d, over)
                && let Some((lo, _)) = built.bounds
            {
                let rect = painter.clip_rect();
                let at = view.to_screen(lo + offset);
                let at = egui::pos2(
                    at.x.clamp(rect.min.x + 4.0, rect.max.x - 4.0),
                    at.y.clamp(rect.min.y + 16.0, rect.max.y - 4.0) - 2.0,
                );
                painter.text(
                    at,
                    egui::Align2::LEFT_BOTTOM,
                    text,
                    egui::FontId::proportional(11.0),
                    st.pen,
                );
            }
        }
        Outcome::Broken(p, _) => {
            // A cross in the warning colour, as in the SVG and the print.
            let h = kadakas_core::model::LOCUS_MARKER_HALF;
            let [r, g, b, a] = kadakas_core::component::WARNING_COLOR;
            let stroke = Stroke::new(st.width, Color32::from_rgba_unmultiplied(r, g, b, a));
            let s = |p: DVec2| view.to_screen(p + offset);
            painter.line_segment(
                [s(*p - DVec2::new(h, 0.0)), s(*p + DVec2::new(h, 0.0))],
                stroke,
            );
            painter.line_segment(
                [s(*p - DVec2::new(0.0, h)), s(*p + DVec2::new(0.0, h))],
                stroke,
            );
        }
    }
}

/// The box of a plan as four edges.
fn box_of(b: &Built) -> Built {
    let mut d = PlanDrawing {
        items: Vec::new(),
        detail: Detail::Box,
        faces_total: 0,
        edges_total: 0,
        limits: plan::Limits::DEFAULT,
    };
    if let Some((lo, hi)) = b.bounds {
        let c = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
        for i in 0..4 {
            d.items.push(PlanItem::Edge(c[i], c[(i + 1) % 4]));
        }
    }
    Built::from_outcome(Outcome::Drawing(d))
}

#[cfg(test)]
pub(crate) mod tests {
    //! The plan of surface models on the canvas, in a headless egui frame: what is in the mesh is
    //! what the plan says, and the colour at a point is the colour of the face that is on top.
    use super::*;
    use egui::epaint::Vertex;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::mesh::{SURFACE_BACK, SURFACE_FRONT};
    use kadakas_core::model::{Attributes, Shape};
    use kadakas_core::surface::{Edit, SurfaceModel};

    const PEN: Color32 = Color32::from_rgb(10, 20, 30);

    fn add(ed: &mut Editor, model: SurfaceModel) -> ObjectId {
        ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::new(model)),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        )
    }

    /// A box 1000 x 1000 x 500 mm with its corner at the origin.
    fn box_model() -> SurfaceModel {
        let mut m = SurfaceModel::new();
        let p = [
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [0.0, 1000.0, 0.0],
        ];
        m.apply(&Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        })
        .unwrap();
        let face = m.faces().next().unwrap().0;
        m.apply(&Edit::PushPull {
            face,
            distance: 500.0,
        })
        .unwrap();
        m
    }

    fn rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0))
    }

    /// The view that puts the model point (0, 0) at the screen point (100, 500), 0.2 px per mm.
    fn view() -> View {
        View {
            zoom: 0.2,
            origin: DVec2::new(100.0, 500.0),
        }
    }

    /// The meshes that the object paints, in order.
    fn paint(
        doc: &Document,
        id: ObjectId,
        view: &View,
        offset: DVec2,
        rev: Option<u64>,
    ) -> Vec<Mesh> {
        crate::area_ui::set_revision(rev);
        let ctx = egui::Context::default();
        let o = doc.object(id).unwrap();
        let st = Style {
            pen: PEN,
            fill: Color32::TRANSPARENT,
            width: 2.0,
        };
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect()),
                ..Default::default()
            },
            |ui| {
                let painter = ui.painter_at(rect());
                let counts = kadakas_core::plants::tag::tag_counts(doc);
                crate::canvas::draw_object(&painter, view, doc, &counts, o, st, offset);
            },
        );
        out.shapes
            .into_iter()
            .filter_map(|s| match s.shape {
                EShape::Mesh(m) => Some((*m).clone()),
                _ => None,
            })
            .collect()
    }

    fn triangles(m: &Mesh) -> Vec<[Vertex; 3]> {
        m.indices
            .chunks(3)
            .map(|t| {
                t.iter()
                    .map(|i| m.vertices[*i as usize])
                    .collect::<Vec<_>>()
            })
            .map(|v| [v[0], v[1], v[2]])
            .collect()
    }

    /// The colour at a screen point: the last triangle that covers it.
    fn pixel(m: &Mesh, p: egui::Pos2) -> Option<Color32> {
        let inside = |t: &[Vertex; 3]| {
            let s = |a: egui::Pos2, b: egui::Pos2| (b - a).x * (p - a).y - (b - a).y * (p - a).x;
            let d = [
                s(t[0].pos, t[1].pos),
                s(t[1].pos, t[2].pos),
                s(t[2].pos, t[0].pos),
            ];
            d.iter().all(|x| *x >= 0.0) || d.iter().all(|x| *x <= 0.0)
        };
        triangles(m)
            .iter()
            .rev()
            .find(|t| inside(t))
            .map(|t| t[0].color)
    }

    fn color(c: [u8; 4]) -> Color32 {
        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
    }

    #[test]
    fn the_top_of_a_box_is_on_top_of_its_bottom_and_the_edges_are_drawn_over_them() {
        let mut ed = Editor::default();
        let id = add(&mut ed, box_model());
        let meshes = paint(ed.doc(), id, &view(), DVec2::ZERO, None);
        assert_eq!(meshes.len(), 1);
        let m = &meshes[0];
        // Two faces of two triangles and 8 edges of two triangles.
        assert_eq!(triangles(m).len(), 2 * 2 + 8 * 2);
        // The middle of the box: 100 + 500 * 0.2 = 200, 500 - 100 = 400 (y is up).
        assert_eq!(
            pixel(m, egui::pos2(200.0, 400.0)),
            Some(color(SURFACE_FRONT))
        );
        // Just inside the left edge, at x = 0: the pen, 2 px wide.
        assert_eq!(pixel(m, egui::pos2(100.0, 400.0)), Some(PEN));
        // Outside the model: nothing.
        assert_eq!(pixel(m, egui::pos2(50.0, 400.0)), None);
        // The bottom is under the top and seen from its back.
        let bottom = triangles(m)
            .iter()
            .find(|t| t[0].color == color(SURFACE_BACK))
            .is_some();
        assert!(bottom);
    }

    #[test]
    fn a_soft_edge_and_a_hidden_face_are_not_in_the_mesh() {
        let mut ed = Editor::default();
        let mut model = box_model();
        let top_edge = model
            .edges()
            .find(|(_, e)| {
                model.vertex(e.v[0]).unwrap().z == 500.0 && model.vertex(e.v[1]).unwrap().z == 500.0
            })
            .map(|(e, _)| e)
            .unwrap();
        model
            .apply(&Edit::SetEdgeStyle {
                edges: vec![top_edge],
                soft: Some(true),
                smooth: None,
            })
            .unwrap();
        let top = model
            .faces()
            .find(|(_, f)| f.plane.normal.z > 0.9)
            .map(|(f, _)| f)
            .unwrap();
        let id = add(&mut ed, model);
        let n = |ed: &Editor| triangles(&paint(ed.doc(), id, &view(), DVec2::ZERO, None)[0]).len();
        assert_eq!(n(&ed), 2 * 2 + 7 * 2);
        ed.execute(&Command::EditSurface {
            id,
            edit: Edit::SetHidden {
                faces: vec![top],
                edges: vec![],
                hidden: true,
            },
        })
        .unwrap();
        assert_eq!(n(&ed), 2 + 7 * 2);
        let m = &paint(ed.doc(), id, &view(), DVec2::ZERO, None)[0];
        assert_eq!(
            pixel(m, egui::pos2(200.0, 400.0)),
            Some(color(SURFACE_BACK))
        );
    }

    #[test]
    fn a_ghost_is_drawn_at_its_offset_and_a_model_off_screen_gives_nothing() {
        let mut ed = Editor::default();
        let id = add(&mut ed, box_model());
        let m = &paint(ed.doc(), id, &view(), DVec2::new(1000.0, 0.0), None)[0];
        // 1000 mm to the right: 200 px.
        assert_eq!(
            pixel(m, egui::pos2(400.0, 400.0)),
            Some(color(SURFACE_FRONT))
        );
        assert_eq!(pixel(m, egui::pos2(200.0, 400.0)), None);
        let far = paint(ed.doc(), id, &view(), DVec2::new(1.0e6, 0.0), None);
        assert!(far.is_empty() || far[0].is_empty(), "culled");
    }

    #[test]
    fn the_plan_of_an_object_is_kept_for_a_revision() {
        let mut ed = Editor::default();
        let id = add(&mut ed, box_model());
        let doc = ed.doc();
        let o = doc.object(id).unwrap();
        crate::area_ui::set_revision(Some(41));
        let a = built(doc, o, true).unwrap();
        assert!(Rc::ptr_eq(&a, &built(doc, o, true).unwrap()));
        // A part of a symbol is not kept, and another revision is made again.
        assert!(!Rc::ptr_eq(&a, &built(doc, o, false).unwrap()));
        // The ghost of a compare has the ID of the live object and is not kept.
        crate::annot_ui::set_ghost(true);
        assert!(!Rc::ptr_eq(&a, &built(doc, o, true).unwrap()));
        crate::annot_ui::set_ghost(false);
        assert!(Rc::ptr_eq(&a, &built(doc, o, true).unwrap()));
        crate::area_ui::set_revision(Some(42));
        assert!(!Rc::ptr_eq(&a, &built(doc, o, true).unwrap()));
        crate::area_ui::set_revision(None);
        assert!(!Rc::ptr_eq(&a, &built(doc, o, true).unwrap()));
    }

    #[test]
    fn a_very_big_model_is_drawn_as_its_outline_and_a_very_ragged_one_as_a_box() {
        use kadakas_core::surface::{Edge, EdgeId, Face, FaceId, Plane, VertexId};
        use std::collections::BTreeMap;
        let mut ed = Editor::default();
        // 10,100 separate triangles: 30,300 edges are over the limit, and every edge is a border.
        let n = 10_100u32;
        let (mut vertices, mut edges, mut faces) =
            (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
        let up = Plane::new(glam::DVec3::ZERO, glam::DVec3::Z).unwrap();
        for i in 0..n {
            let (x, y) = (f64::from(i % 100) * 10.0, f64::from(i / 100) * 10.0);
            let v = [
                VertexId(3 * i + 1),
                VertexId(3 * i + 2),
                VertexId(3 * i + 3),
            ];
            vertices.insert(v[0], glam::DVec3::new(x, y, 0.0));
            vertices.insert(v[1], glam::DVec3::new(x + 8.0, y, 0.0));
            vertices.insert(v[2], glam::DVec3::new(x, y + 8.0, 0.0));
            for k in 0..3 {
                edges.insert(
                    EdgeId(3 * n + 3 * i + 1 + k as u32),
                    Edge {
                        v: [v[k], v[(k + 1) % 3]],
                    },
                );
            }
            faces.insert(FaceId(6 * n + i + 1), Face::new(up, v.to_vec(), vec![]));
        }
        let id = add(
            &mut ed,
            SurfaceModel::from_parts(vertices, edges, faces, 100_000),
        );
        let out = built(ed.doc(), ed.doc().object(id).unwrap(), false).unwrap();
        let d = out.drawing().expect("a drawing");
        assert_eq!(d.detail, plan::Detail::Box);
        let m = &paint(ed.doc(), id, &view(), DVec2::ZERO, None)[0];
        // The box: 4 edges and a fill that is not drawn (no fill in the class).
        assert_eq!(triangles(m).len(), 4 * 2);
    }

    #[test]
    fn an_instance_that_cannot_be_drawn_is_a_cross_in_the_warning_colour() {
        let ed = Editor::default();
        // A document from a file can hold an instance whose definition is gone.
        let mut doc = ed.doc().clone();
        let (layer, class) = (doc.active_layer, doc.active_class);
        let id = kadakas_core::model::ObjectId(doc.alloc_id());
        doc.objects.insert(
            id,
            kadakas_core::model::Object::new(
                id,
                layer,
                class,
                Shape::Component {
                    symbol: kadakas_core::resource::ResourceId::from_num(77),
                    transform: [
                        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1000.0, 1000.0, 0.0,
                    ],
                },
            ),
        );
        crate::area_ui::set_revision(None);
        let ctx = egui::Context::default();
        let doc = &doc;
        let st = Style {
            pen: PEN,
            fill: Color32::TRANSPARENT,
            width: 2.0,
        };
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect()),
                ..Default::default()
            },
            |ui| {
                let painter = ui.painter_at(rect());
                let counts = kadakas_core::plants::tag::tag_counts(doc);
                crate::canvas::draw_object(
                    &painter,
                    &view(),
                    doc,
                    &counts,
                    doc.object(id).unwrap(),
                    st,
                    DVec2::ZERO,
                );
            },
        );
        let [r, g, b, _] = kadakas_core::component::WARNING_COLOR;
        let lines: Vec<_> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                EShape::LineSegment { stroke, .. } => Some(stroke.color),
                _ => None,
            })
            .collect();
        assert_eq!(lines, vec![Color32::from_rgb(r, g, b); 2]);
    }

    /// A flat grid of `n` x `n` squares cut in two, bumpy so that the faces are not coplanar.
    pub(super) fn bumpy_grid(n: usize) -> SurfaceModel {
        use kadakas_core::surface::{Edge, EdgeId, Face, FaceId, Plane, VertexId};
        use std::collections::{BTreeMap, HashSet};
        let id_of = |x: usize, y: usize| VertexId((y * (n + 1) + x) as u32 + 1);
        let mut vertices = BTreeMap::new();
        for y in 0..=n {
            for x in 0..=n {
                let z = 300.0 * (x as f64 / 9.0).sin() * (y as f64 / 7.0).cos();
                vertices.insert(
                    id_of(x, y),
                    glam::DVec3::new(x as f64 * 100.0, y as f64 * 100.0, z),
                );
            }
        }
        let (mut edges, mut faces) = (BTreeMap::new(), BTreeMap::new());
        let (mut seen, mut e, mut f) = (HashSet::new(), 0u32, 0u32);
        for y in 0..n {
            for x in 0..n {
                let (a, b, c, d) = (
                    id_of(x, y),
                    id_of(x + 1, y),
                    id_of(x + 1, y + 1),
                    id_of(x, y + 1),
                );
                for tri in [[a, b, c], [a, c, d]] {
                    for k in 0..3 {
                        let (p, q) = (tri[k], tri[(k + 1) % 3]);
                        if seen.insert((p.min(q), p.max(q))) {
                            e += 1;
                            edges.insert(EdgeId(100_000 + e), Edge { v: [p, q] });
                        }
                    }
                    let pos = tri.map(|v| vertices[&v]);
                    let n3 = (pos[1] - pos[0]).cross(pos[2] - pos[0]);
                    let mut t = tri.to_vec();
                    if n3.z < 0.0 {
                        t.swap(1, 2);
                    }
                    f += 1;
                    faces.insert(
                        FaceId(1_000_000 + f),
                        Face::new(Plane::new(pos[0], n3).unwrap(), t, vec![]),
                    );
                }
            }
        }
        SurfaceModel::from_parts(vertices, edges, faces, 2_000_000)
    }

    /// The time of the plan on the screen at the sizes of the limits: the geometry, the egui mesh
    /// and its tessellation. Not run by `cargo test`; the numbers are in `docs/modeler-limits.md`.
    ///
    /// `cargo test --release -p kadakas-app surface_draw::tests::time_of_the_plan -- --ignored --nocapture`
    #[test]
    #[ignore = "a measurement"]
    fn time_of_the_plan_on_the_screen() {
        use std::time::Instant;
        for n in [50usize, 70, 100, 141, 223] {
            let model = bumpy_grid(n);
            let (faces, edges) = (model.face_count(), model.edge_count());
            let part = [plan::Part {
                model: &model,
                xf: model.transform,
                fill: None,
            }];
            let t = Instant::now();
            let d = plan::drawing_with(
                &part,
                None,
                plan::Limits {
                    faces: usize::MAX,
                    edges: usize::MAX,
                    outline: usize::MAX,
                },
            );
            let t_plan = t.elapsed().as_secs_f64() * 1000.0;
            let v = View {
                zoom: 800.0 / (n as f64 * 100.0),
                origin: DVec2::new(0.0, 600.0),
            };
            let t = Instant::now();
            let built = Built::from_outcome(Outcome::Drawing(d));
            let t_build = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let mesh = plan_mesh(&v, DVec2::ZERO, &built, PEN, 1.0, rect());
            let t_mesh = t.elapsed().as_secs_f64() * 1000.0;
            let vertices = mesh.vertices.len();
            let ctx = egui::Context::default();
            let t = Instant::now();
            let out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect()),
                    ..Default::default()
                },
                |ui| {
                    ui.painter_at(rect()).add(EShape::mesh(mesh.clone()));
                },
            );
            let meshes = ctx.tessellate(out.shapes, 1.0);
            let t_tess = t.elapsed().as_secs_f64() * 1000.0;
            let outline = plan::drawing(&part, None);
            println!(
                "{faces:>7} faces {edges:>7} edges: plan {t_plan:>6.1} ms, triangles {t_build:>6.1} ms, mesh {t_mesh:>6.1} ms ({vertices} vertices), egui {t_tess:>6.1} ms ({} meshes); limits give {:?} with {} items",
                meshes.len(),
                outline.detail,
                outline.items.len()
            );
        }
    }

    /// Texts painted in a frame of the object.
    fn texts(doc: &Document, id: ObjectId) -> Vec<String> {
        crate::area_ui::set_revision(None);
        let ctx = egui::Context::default();
        let o = doc.object(id).unwrap();
        let st = Style {
            pen: PEN,
            fill: Color32::TRANSPARENT,
            width: 2.0,
        };
        let v = View {
            zoom: 0.4,
            origin: DVec2::new(100.0, 500.0),
        };
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect()),
                ..Default::default()
            },
            |ui| {
                let painter = ui.painter_at(rect());
                let counts = kadakas_core::plants::tag::tag_counts(doc);
                begin_frame();
                crate::canvas::draw_object(&painter, &v, doc, &counts, o, st, DVec2::ZERO);
            },
        );
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                EShape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_big_model_says_on_the_canvas_that_it_is_drawn_as_an_outline() {
        let mut ed = Editor::default();
        // 101 x 101 squares: 20,402 faces, over the 20,000 of the plan.
        let id = add(&mut ed, bumpy_grid(101));
        let said = texts(ed.doc(), id);
        assert_eq!(
            said,
            ["Drawn as outline: 20,402 faces, limit 20,000"],
            "{said:?}"
        );
        // A small model says nothing.
        let small = add(&mut ed, box_model());
        assert!(texts(ed.doc(), small).is_empty());
    }

    #[test]
    fn an_object_over_the_budget_of_the_frame_is_drawn_as_its_box_and_says_so() {
        let mut ed = Editor::default();
        let id = add(&mut ed, box_model());
        crate::area_ui::set_revision(None);
        set_frame_budget(10);
        let said = texts(ed.doc(), id);
        assert_eq!(said, ["Drawn as box: the frame budget is used up"]);
        let meshes = paint(ed.doc(), id, &view(), DVec2::ZERO, None);
        // The box: four edges as quads.
        assert_eq!(triangles(&meshes[0]).len(), 4 * 2);
        set_frame_budget(FRAME_VERTEX_BUDGET);
        assert!(texts(ed.doc(), id).is_empty());
        // The budget is whole again at the start of a frame.
        set_frame_budget(40);
        begin_frame();
        assert_eq!(paint(ed.doc(), id, &view(), DVec2::ZERO, None).len(), 1);
        set_frame_budget(FRAME_VERTEX_BUDGET);
    }

    #[test]
    fn the_cache_drops_the_plans_that_were_used_least_and_keeps_the_rest() {
        let mut ed = Editor::default();
        let id = add(&mut ed, box_model());
        let out = ed
            .execute(&Command::MakeComponent {
                name: "Block".into(),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![id],
                replace: true,
            })
            .unwrap();
        let inst = ObjectId(out.created[1]);
        let Shape::Component { symbol, .. } = ed.doc().object(inst).unwrap().shape.clone() else {
            panic!()
        };
        for _ in 0..(MAX_ENTRIES + 100) {
            ed.execute(&Command::AddObject {
                shape: Shape::Component {
                    symbol,
                    transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        }
        let doc = ed.doc();
        crate::area_ui::set_revision(Some(91));
        let hot = built(doc, doc.object(inst).unwrap(), true).unwrap();
        for (n, o) in doc.objects.values().enumerate() {
            built(doc, o, true).unwrap();
            // One plan is used all the time.
            if n % 500 == 0 {
                built(doc, doc.object(inst).unwrap(), true).unwrap();
            }
        }
        assert!(cached() <= MAX_ENTRIES, "{}", cached());
        assert!(cached() > MAX_ENTRIES / 2, "not emptied: {}", cached());
        assert!(Rc::ptr_eq(
            &hot,
            &built(doc, doc.object(inst).unwrap(), true).unwrap()
        ));
        // The entries of another revision go first.
        crate::area_ui::set_revision(Some(92));
        built(doc, doc.object(inst).unwrap(), true).unwrap();
        for o in doc.objects.values().take(MAX_ENTRIES) {
            built(doc, o, true).unwrap();
        }
        assert!(cached() <= MAX_ENTRIES);
    }

    /// The time of a plan with thousands of instances of one definition: the first frame (the plans
    /// are made and cut into triangles) and a frame from the cache. Not run by `cargo test`.
    ///
    /// `cargo test --release -p kadakas-app surface_draw::tests::time_of_5000 -- --ignored --nocapture`
    #[test]
    #[ignore = "a measurement"]
    fn time_of_5000_instances() {
        use std::time::Instant;
        let mut ed = Editor::default();
        let id = add(&mut ed, box_model());
        let out = ed
            .execute(&Command::MakeComponent {
                name: "Block".into(),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![id],
                replace: true,
            })
            .unwrap();
        let inst = ObjectId(out.created[1]);
        let Shape::Component { symbol, .. } = ed.doc().object(inst).unwrap().shape.clone() else {
            panic!()
        };
        for i in 1..5000u32 {
            let mut transform = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
            transform[9] = f64::from(i % 100) * 1500.0;
            transform[10] = f64::from(i / 100) * 1500.0;
            ed.execute(&Command::AddObject {
                shape: Shape::Component { symbol, transform },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        }
        let doc = ed.doc();
        let v = View {
            zoom: 0.005,
            origin: DVec2::new(0.0, 600.0),
        };
        crate::area_ui::set_revision(Some(7));
        let frame = || {
            begin_frame();
            let mut vertices = 0usize;
            for o in doc.objects.values() {
                let b = built(doc, o, true).unwrap();
                vertices += plan_mesh(&v, DVec2::ZERO, &b, PEN, 1.0, rect())
                    .vertices
                    .len();
            }
            vertices
        };
        let t = Instant::now();
        let n = frame();
        let cold = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        frame();
        let warm = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "5000 instances of a box: first frame {cold:.0} ms, frame from the cache {warm:.1} ms, {n} vertices, {} plans kept",
            cached()
        );
    }
}

/// Models for the tests of other modules of the app.
#[cfg(test)]
pub(crate) mod tests_support {
    /// A flat model over the limits of the plan: 101 x 101 squares cut in two (20,402 faces).
    pub fn big_model() -> kadakas_core::surface::SurfaceModel {
        super::tests::bumpy_grid(101)
    }
}
