//! Tests of the modeling tools: the state machines, typed values, Esc, the undo count, the
//! selection of faces and edges, and the scene cache.

use glam::{DVec2, DVec3};
use kadakas_core::command::Editor;
use kadakas_core::model::{ObjectId, Shape};
use kadakas_core::surface::{EdgeId, FaceId, SurfaceModel};

use super::*;
use crate::infer3d::{Camera, Constraint, Inference, Inferencer, Kind, Projection, Ray, Rgb, Tag};
use crate::modeler::ModelerTool;

struct Rig {
    ed: Editor,
    tools: Modeling,
    cache: SceneCache,
}

fn rig(tool: ModelerTool) -> Rig {
    let mut tools = Modeling::default();
    tools.choose(tool);
    Rig {
        ed: Editor::with_sequential_ids(),
        tools,
        cache: SceneCache::default(),
    }
}

fn free(point: DVec3) -> Inference {
    Inference {
        point,
        kind: Kind::Free,
        screen: DVec2::ZERO,
        color: Rgb(0, 0, 0),
        label: String::new(),
        lines: Vec::new(),
        locked: false,
        source: Tag::NONE,
        other: Tag::NONE,
        constraint: None,
    }
}

fn down_ray(p: DVec3) -> Ray {
    Ray {
        origin: p + DVec3::Z * 10_000.0,
        dir: -DVec3::Z,
    }
}

impl Rig {
    /// Runs `f` with the world of the document, then runs the actions it gives.
    fn act(&mut self, f: impl FnOnce(&mut Modeling, &World) -> Out) -> Out {
        let rev = self.ed.revision();
        let out = {
            let scenes = self.cache.get(self.ed.doc(), rev);
            let world = World {
                doc: self.ed.doc(),
                scenes,
            };
            f(&mut self.tools, &world)
        };
        let used = run_actions(&mut self.ed, &out.actions, out.redo).expect("the actions run");
        if !out.actions.is_empty() {
            self.tools.committed(used, self.ed.revision());
        }
        out
    }

    /// Like `act`, but the actions are not run.
    fn act_no_run(&mut self, f: impl FnOnce(&mut Modeling, &World) -> Out) -> Out {
        let rev = self.ed.revision();
        let scenes = self.cache.get(self.ed.doc(), rev);
        let world = World {
            doc: self.ed.doc(),
            scenes,
        };
        f(&mut self.tools, &world)
    }

    fn click_inf(&mut self, inf: Inference, ray: Ray) -> Out {
        self.act(|t, w| {
            let p = Pointer {
                inf: &inf,
                ray,
                selected: None,
            };
            t.moved(w, &p);
            t.click(w, &p)
        })
    }

    fn click(&mut self, p: DVec3) -> Out {
        self.click_inf(free(p), down_ray(p))
    }

    fn hover(&mut self, p: DVec3) {
        self.act(|t, w| {
            let inf = free(p);
            t.moved(
                w,
                &Pointer {
                    inf: &inf,
                    ray: down_ray(p),
                    selected: None,
                },
            );
            Out::default()
        });
    }

    fn type_text(&mut self, text: &str) -> Out {
        for c in text.chars() {
            assert!(self.tools.type_char(c), "{c:?} goes into the box");
        }
        let rev = self.ed.revision();
        self.act(|t, w| t.enter(w, None, rev))
    }

    fn model_id(&self) -> ObjectId {
        *self
            .ed
            .doc()
            .objects
            .iter()
            .find(|(_, o)| matches!(o.shape, Shape::SurfaceModel(_)))
            .expect("a surface model")
            .0
    }

    fn model(&self) -> &SurfaceModel {
        model_of(self.ed.doc(), self.model_id()).unwrap()
    }

    fn counts(&self) -> (usize, usize, usize) {
        let m = self.model();
        (m.vertex_count(), m.edge_count(), m.face_count())
    }
}

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

#[test]
fn line_chain_one_undo_step_for_each_segment_and_a_closed_loop_makes_a_face() {
    let mut r = rig(ModelerTool::Line);
    let depth = r.ed.undo_depth();
    r.click(v(0.0, 0.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth, "the first click draws nothing");
    assert!(r.tools.in_progress());
    assert_eq!(r.tools.reference(), Some(v(0.0, 0.0, 0.0)));
    r.click(v(1000.0, 0.0, 0.0));
    // A new model and its first edge are one step.
    assert_eq!(r.ed.undo_depth(), depth + 1);
    r.click(v(1000.0, 1000.0, 0.0));
    r.click(v(0.0, 1000.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth + 3);
    assert_eq!(r.model().face_count(), 0);
    // The click on the start closes the loop: the core makes the face and the tool ends.
    r.click(v(0.0, 0.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth + 4);
    assert_eq!(r.counts(), (4, 4, 1));
    assert!(!r.tools.in_progress());
    // One undo takes back one segment; the model stays until its first step is undone.
    assert!(r.ed.undo());
    assert_eq!(r.model().face_count(), 0);
    for _ in 0..3 {
        assert!(r.ed.undo());
    }
    assert!(
        !r.ed
            .doc()
            .objects
            .values()
            .any(|o| matches!(o.shape, Shape::SurfaceModel(_))),
        "the model and its first edge were one step"
    );
}

#[test]
fn line_escape_ends_the_chain_and_a_zero_length_click_is_ignored() {
    let mut r = rig(ModelerTool::Line);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(500.0, 0.0, 0.0));
    let depth = r.ed.undo_depth();
    r.click(v(500.0, 0.0, 0.0));
    assert_eq!(
        r.ed.undo_depth(),
        depth,
        "the same point twice draws nothing"
    );
    assert!(r.tools.cancel());
    assert!(!r.tools.in_progress());
    // The next click starts a new chain, in the same model.
    r.click(v(0.0, 500.0, 0.0));
    r.click(v(500.0, 500.0, 0.0));
    assert_eq!(r.ed.doc().objects.len(), 1);
    assert_eq!(r.model().edge_count(), 2);
    assert!(r.tools.cancel(), "the second chain is open");
    assert!(!r.tools.cancel(), "nothing to cancel");
}

#[test]
fn line_takes_a_typed_length_along_the_pointer_and_a_point() {
    let mut r = rig(ModelerTool::Line);
    // Nothing started: the text is refused with a message.
    r.tools.type_char('5');
    let rev = r.ed.revision();
    let out = r.act(|t, w| t.enter(w, None, rev));
    assert!(out.message.is_some() && out.actions.is_empty());
    r.tools.cancel();
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(100.0, 0.0, 0.0));
    let out = r.type_text("2500");
    assert!(out.message.is_none(), "{out:?}");
    assert_eq!(r.model().edge_count(), 1);
    let (_, hi) = r.model().local_bounds().unwrap();
    assert!((hi.x - 2500.0).abs() < 1e-6, "{hi:?}");
    // The chain goes on from the new end; a relative point.
    assert_eq!(r.tools.reference(), Some(v(2500.0, 0.0, 0.0)));
    r.type_text("<0,300,0>");
    let (_, hi) = r.model().local_bounds().unwrap();
    assert!((hi.y - 300.0).abs() < 1e-6);
    // A negative length goes the other way.
    r.hover(v(2500.0, 900.0, 0.0));
    r.type_text("-100");
    assert_eq!(r.model().edge_count(), 3);
    // An absolute point.
    r.type_text("[0,0,0]");
    assert_eq!(r.model().edge_count(), 4);
    // Nonsense is a message and the text stays.
    r.tools.type_char('7');
    r.tools.type_char('x');
    r.tools.type_char('y');
    let rev = r.ed.revision();
    let out = r.act(|t, w| t.enter(w, None, rev));
    assert!(out.message.is_some());
    assert_eq!(r.tools.typed(), "7xy");
    assert!(r.tools.cancel(), "Esc clears the text first");
    assert_eq!(r.tools.typed(), "");
}

#[test]
fn rectangle_on_the_ground_is_one_step_with_a_face_and_typed_sizes_work() {
    let mut r = rig(ModelerTool::Rectangle);
    let depth = r.ed.undo_depth();
    r.click(v(100.0, 100.0, 0.0));
    r.hover(v(900.0, 600.0, 0.0));
    let live = r.tools.live(&kadakas_units::current()).unwrap();
    assert_eq!(live.0, LiveLabel::Dimensions);
    assert!(!live.1.is_empty());
    let pv = r.act(|t, w| {
        assert_eq!(t.preview(w).shape.len(), 4);
        Out::default()
    });
    assert_eq!(pv, Out::default());
    r.click(v(900.0, 600.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.counts(), (4, 4, 1));
    let (lo, hi) = r.model().local_bounds().unwrap();
    assert_eq!((lo, hi), (v(100.0, 100.0, 0.0), v(900.0, 600.0, 0.0)));
    // A typed size after the first click, towards the pointer quadrant.
    r.click(v(5000.0, 5000.0, 0.0));
    r.hover(v(4000.0, 4800.0, 0.0));
    r.type_text("300;200");
    assert_eq!(r.ed.undo_depth(), depth + 2);
    assert_eq!(r.model().face_count(), 2);
    let (lo, hi) = r.model().local_bounds().unwrap();
    assert!((hi.x - 5000.0).abs() < 1e-6 && lo.x <= 100.0);
    // The new rectangle is at x 4700..5000 and y 4800..5000: left and down of the click.
    let f = r.model().faces().map(|(_, f)| f).last().unwrap();
    let xs: Vec<f64> = f
        .outer
        .iter()
        .map(|v| r.model().vertex(*v).unwrap().x)
        .collect();
    assert!(xs.iter().any(|x| (x - 4700.0).abs() < 1e-6), "{xs:?}");
}

#[test]
fn a_typed_value_after_the_rectangle_redoes_it_while_nothing_else_happened() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 500.0, 0.0));
    let depth = r.ed.undo_depth();
    assert_eq!(r.tools.phase(), crate::measure_box::Phase::Done);
    r.type_text("2000;800");
    assert_eq!(r.ed.undo_depth(), depth, "the first step was taken back");
    assert_eq!(r.model().face_count(), 1);
    let (_, hi) = r.model().local_bounds().unwrap();
    assert_eq!((hi.x, hi.y), (2000.0, 800.0));
    // `,4` keeps the width and changes the height.
    r.type_text(";300");
    let (_, hi) = r.model().local_bounds().unwrap();
    assert_eq!((hi.x, hi.y), (2000.0, 300.0));
    // After another edit the old rectangle can not be redone.
    r.ed.execute(&kadakas_core::command::Command::EditSurface {
        id: r.model_id(),
        edit: kadakas_core::surface::Edit::AddEdges {
            edges: vec![[[0.0, 0.0, 0.0], [0.0, -500.0, 0.0]]],
        },
    })
    .unwrap();
    r.tools.type_char('5');
    let rev = r.ed.revision();
    let out = r.act(|t, w| t.enter(w, None, rev));
    assert!(out.message.is_some() && out.actions.is_empty(), "{out:?}");
}

#[test]
fn rectangle_refuses_a_flat_shape_and_escape_cancels() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    let out = r.click(v(500.0, 0.0, 0.0));
    assert!(out.message.is_some() && out.actions.is_empty());
    assert!(r.ed.doc().objects.is_empty());
    r.click(v(0.0, 0.0, 0.0));
    assert!(r.tools.cancel());
    assert!(!r.tools.in_progress());
    // A value of a kind the tool does not take is refused with the list of kinds.
    r.click(v(0.0, 0.0, 0.0));
    r.tools.type_char('4');
    r.tools.type_char('5');
    r.tools.type_char('d');
    r.tools.type_char('e');
    r.tools.type_char('g');
    let rev = r.ed.revision();
    let out = r.act(|t, w| t.enter(w, None, rev));
    assert!(out.message.is_some());
}

#[test]
fn circle_is_one_curve_with_the_typed_sides_and_radius() {
    let mut r = rig(ModelerTool::Circle);
    let depth = r.ed.undo_depth();
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(500.0, 0.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.counts(), (24, 24, 1));
    assert_eq!(r.model().curves().count(), 1);
    // `12s` after the circle redoes it with 12 sides.
    r.type_text("12s");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.counts(), (12, 12, 1));
    assert_eq!(r.tools.sides, 12);
    // A radius after it redoes it too.
    r.type_text("250");
    assert_eq!(r.counts(), (12, 12, 1));
    let (lo, hi) = r.model().local_bounds().unwrap();
    assert!((hi.x - lo.x - 500.0).abs() < 1.0, "{lo:?} {hi:?}");
    // The sides stay for the next circle; a radius typed after the center.
    r.click(v(5000.0, 0.0, 0.0));
    r.type_text("100");
    assert_eq!(r.model().curves().count(), 2);
    assert_eq!(r.model().edge_count(), 24);
    // Sides out of range are refused.
    r.tools.type_char('2');
    r.tools.type_char('s');
    let rev = r.ed.revision();
    let out = r.act(|t, w| t.enter(w, None, rev));
    assert!(out.message.is_some());
    // The preview of a running circle is a ring of the sides.
    r.tools.cancel();
    r.tools.cancel();
    r.click(v(9000.0, 0.0, 0.0));
    r.hover(v(9300.0, 0.0, 0.0));
    r.act(|t, w| {
        assert_eq!(t.preview(w).shape.len(), 12);
        Out::default()
    });
}

#[test]
fn arc_has_a_chord_and_a_bulge_and_is_one_curve() {
    let mut r = rig(ModelerTool::Arc);
    let depth = r.ed.undo_depth();
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 0.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth, "two clicks draw nothing yet");
    r.hover(v(500.0, 300.0, 0.0));
    let live = r.tools.live(&kadakas_units::current()).unwrap();
    assert_eq!(live.0, LiveLabel::Bulge);
    r.click(v(500.0, 300.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.model().edge_count(), 12);
    assert_eq!(r.model().curves().count(), 1);
    let (lo, hi) = r.model().local_bounds().unwrap();
    assert!(
        (hi.y - 300.0).abs() < 1e-6 && lo.y.abs() < 1e-9,
        "{lo:?} {hi:?}"
    );
    // A typed bulge after it redoes the arc (here on the other side: the sign follows the sign
    // of the old bulge, so it stays on the same side).
    r.type_text("100");
    let (_, hi) = r.model().local_bounds().unwrap();
    assert!((hi.y - 100.0).abs() < 1e-6);
    // A typed radius.
    r.type_text("500r");
    let (_, hi) = r.model().local_bounds().unwrap();
    assert!((hi.y - (500.0 - (500.0f64 * 500.0 - 250_000.0).sqrt())).abs() < 1e-6);
    // `6s` sets the number of segments of the arc.
    r.type_text("6s");
    assert_eq!(r.model().edge_count(), 6);
    // Typed chord and bulge for a new arc.
    r.click(v(0.0, 2000.0, 0.0));
    r.hover(v(100.0, 2000.0, 0.0));
    r.type_text("800");
    r.hover(v(400.0, 2100.0, 0.0));
    r.type_text("120");
    assert_eq!(r.model().curves().count(), 2);
    // A radius smaller than half the chord is refused.
    r.click(v(0.0, 4000.0, 0.0));
    r.click(v(1000.0, 4000.0, 0.0));
    r.hover(v(500.0, 4100.0, 0.0));
    r.tools.type_char('9');
    r.tools.type_char('r');
    let rev = r.ed.revision();
    let out = r.act(|t, w| t.enter(w, None, rev));
    assert!(out.message.is_some() && out.actions.is_empty());
}

#[test]
fn the_arc_passes_through_the_chord_ends_and_the_bulge_point() {
    let plane = Plane3::ground(0.0);
    let (a, b) = (v(0.0, 0.0, 0.0), v(1000.0, 0.0, 0.0));
    for h in [300.0, -300.0, 600.0] {
        let pts = arc_points(a, b, h, &plane, 16);
        assert_eq!(pts.len(), 17);
        assert_eq!((pts[0], pts[16]), (a, b));
        let top = pts
            .iter()
            .map(|p| p.y)
            .fold(0.0f64, |m, y| if y.abs() > m.abs() { y } else { m });
        assert!((top.abs() - h.abs()).abs() < h.abs() * 0.03, "{h} {top}");
        assert_eq!(top.signum(), h.signum());
    }
    assert_eq!(radius_to_bulge(1000.0, 400.0), None);
    assert!((radius_to_bulge(1000.0, 500.0).unwrap() - 500.0).abs() < 1e-9);
    assert!(bulge_of(a, b, v(500.0, 200.0, 0.0), &plane) > 0.0);
    assert!(bulge_of(a, b, v(500.0, -200.0, 0.0), &plane) < 0.0);
}

#[test]
fn eraser_erases_the_edges_it_passes_over_in_one_step_and_a_curve_goes_whole() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    let model = r.model_id();
    let edges: Vec<EdgeId> = r.model().edges().map(|(id, _)| id).collect();
    r.tools.choose(ModelerTool::Eraser);
    let depth = r.ed.undo_depth();
    let on_edge = |e: EdgeId| {
        let mut i = free(v(500.0, 0.0, 0.0));
        i.kind = Kind::OnEdge;
        i.source = Tag::edge(0, e);
        i
    };
    // Press, move over a second edge, release: one step.
    r.act(|t, w| {
        let i = on_edge(edges[0]);
        t.press(
            w,
            &Pointer {
                inf: &i,
                ray: down_ray(i.point),
                selected: None,
            },
        );
        let j = on_edge(edges[1]);
        t.moved(
            w,
            &Pointer {
                inf: &j,
                ray: down_ray(j.point),
                selected: None,
            },
        );
        assert_eq!(t.excluded().edges.len(), 2);
        assert_eq!(t.preview(w).erase.len(), 2);
        Out::default()
    });
    let out = r.act(|t, _| t.release());
    assert_eq!(out.actions.len(), 1);
    assert_eq!(r.ed.undo_depth(), depth + 1);
    // Two edges gone: the face is gone.
    assert_eq!(r.model().face_count(), 0);
    assert_eq!(r.model().edge_count(), 2);
    assert!(r.ed.undo());
    assert_eq!(r.model().face_count(), 1);
    // A circle goes whole with one click.
    r.tools.choose(ModelerTool::Circle);
    r.click(v(5000.0, 0.0, 0.0));
    r.click(v(5300.0, 0.0, 0.0));
    assert_eq!(r.model().curves().count(), 1);
    let (cid, curve) = r
        .model()
        .curves()
        .next()
        .map(|(k, e)| (k, e.clone()))
        .unwrap();
    let _ = cid;
    r.tools.choose(ModelerTool::Eraser);
    r.act(|t, w| {
        let i = on_edge(curve[3]);
        let p = Pointer {
            inf: &i,
            ray: down_ray(i.point),
            selected: None,
        };
        t.press(w, &p);
        Out::default()
    });
    let out = r.act(|t, _| t.release());
    assert!(
        matches!(&out.actions[0], Action::EraseEdges { model: m, edges } if *m == model && edges.len() == 24)
    );
    assert_eq!(r.model().curves().count(), 0);
    // Pressing on nothing erases nothing.
    r.act(|t, w| {
        let i = free(v(1.0, 1.0, 0.0));
        t.press(
            w,
            &Pointer {
                inf: &i,
                ray: down_ray(i.point),
                selected: None,
            },
        );
        Out::default()
    });
    let out = r.act(|t, _| t.release());
    assert!(out.actions.is_empty());
}

fn on_face(p: DVec3, face: FaceId) -> Inference {
    let mut i = free(p);
    i.kind = Kind::OnFace;
    i.source = Tag::face(0, face);
    i.constraint = Some(Constraint::Plane {
        origin: p,
        normal: DVec3::Z,
    });
    i
}

fn pull_ray(from: DVec3, to: DVec3) -> Ray {
    Ray {
        origin: from,
        dir: (to - from).normalize(),
    }
}

#[test]
fn push_pull_pulls_a_rectangle_into_a_box_and_a_double_click_repeats() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    let face = r.model().faces().next().unwrap().0;
    r.tools.choose(ModelerTool::PushPull);
    let depth = r.ed.undo_depth();
    // Click the face, move up, click.
    r.click_inf(
        on_face(v(500.0, 500.0, 0.0), face),
        down_ray(v(500.0, 500.0, 0.0)),
    );
    assert!(r.tools.in_progress());
    assert_eq!(r.tools.excluded().faces.len(), 1);
    // The pointer ray from the side: eye at (-5000, 500, 0) looks at the height 600.
    let eye = v(-5000.0, 500.0, 0.0);
    r.act(|t, w| {
        let i = free(v(500.0, 500.0, 600.0));
        t.moved(
            w,
            &Pointer {
                inf: &i,
                ray: pull_ray(eye, v(500.0, 500.0, 600.0)),
                selected: None,
            },
        );
        let live = t.live(&kadakas_units::current()).unwrap();
        assert_eq!(live.0, LiveLabel::Distance);
        assert!(!t.preview(w).pull.is_empty());
        Out::default()
    });
    let i = free(v(500.0, 500.0, 600.0));
    r.act(|t, w| {
        let p = Pointer {
            inf: &i,
            ray: pull_ray(eye, v(500.0, 500.0, 600.0)),
            selected: None,
        };
        t.click(w, &p)
    });
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.counts(), (8, 12, 6));
    assert!(r.model().check().is_closed());
    let (_, hi) = r.model().local_bounds().unwrap();
    assert!((hi.z - 600.0).abs() < 1e-6, "{hi:?}");
    // A typed distance now redoes it.
    r.type_text("1500");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.model().local_bounds().unwrap().1.z, 1500.0);
    assert!(r.model().check().is_closed());
    // A double click on the face repeats the last distance (the box top moves again).
    let top = r
        .model()
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .unwrap()
        .0;
    let i = on_face(v(500.0, 500.0, 1500.0), top);
    r.act(|t, w| {
        t.double_click(
            w,
            &Pointer {
                inf: &i,
                ray: down_ray(i.point),
                selected: None,
            },
        )
    });
    assert_eq!(r.model().local_bounds().unwrap().1.z, 3000.0);
    assert_eq!(r.ed.undo_depth(), depth + 2);
    // Push/Pull on nothing says so.
    let out = r.click(v(9000.0, 9000.0, 0.0));
    assert!(out.message.is_some() && !r.tools.in_progress());
}

#[test]
fn push_pull_typed_distance_after_the_click_commits_and_escape_cancels() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    let face = r.model().faces().next().unwrap().0;
    r.tools.choose(ModelerTool::PushPull);
    r.click_inf(
        on_face(v(500.0, 500.0, 0.0), face),
        down_ray(v(500.0, 500.0, 0.0)),
    );
    assert!(r.tools.cancel());
    assert_eq!(r.model().face_count(), 1);
    r.click_inf(
        on_face(v(500.0, 500.0, 0.0), face),
        down_ray(v(500.0, 500.0, 0.0)),
    );
    r.type_text("400");
    assert_eq!(r.counts(), (8, 12, 6));
    // Pushing through is refused by the core and the tool says nothing is changed: the error
    // comes back from the command.
    let top = r
        .model()
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .unwrap()
        .0;
    r.tools.choose(ModelerTool::PushPull);
    r.click_inf(
        on_face(v(500.0, 500.0, 400.0), top),
        down_ray(v(500.0, 500.0, 400.0)),
    );
    let rev = r.ed.revision();
    r.tools.type_char('-');
    r.tools.type_char('4');
    r.tools.type_char('0');
    r.tools.type_char('0');
    let out = r.tools.enter_for_test(&mut r.ed, &mut r.cache, rev);
    assert!(
        out.is_err(),
        "pushing through to the opposite face is refused"
    );
    assert_eq!(r.model().local_bounds().unwrap().1.z, 400.0);
}

impl Modeling {
    /// Enter, then run the actions and give the error of the command, if there is one.
    fn enter_for_test(
        &mut self,
        ed: &mut Editor,
        cache: &mut SceneCache,
        rev: u64,
    ) -> Result<(), String> {
        let out = {
            let scenes = cache.get(ed.doc(), rev);
            let world = World {
                doc: ed.doc(),
                scenes,
            };
            self.enter(&world, None, rev)
        };
        run_actions(ed, &out.actions, out.redo).map(|_| ())
    }
}

#[test]
fn the_same_input_gives_the_same_model() {
    let make = || {
        let mut r = rig(ModelerTool::Rectangle);
        r.click(v(0.0, 0.0, 0.0));
        r.click(v(1000.0, 700.0, 0.0));
        let face = r.model().faces().next().unwrap().0;
        r.tools.choose(ModelerTool::PushPull);
        r.click_inf(on_face(v(5.0, 5.0, 0.0), face), down_ray(v(5.0, 5.0, 0.0)));
        r.type_text("300");
        serde_json::to_string(r.model()).unwrap()
    };
    assert_eq!(make(), make());
}

#[test]
fn drawing_goes_into_the_selected_model_the_last_model_or_a_new_one() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(100.0, 100.0, 0.0));
    let first = r.model_id();
    // Make a second model with a direct command.
    r.ed.execute(&kadakas_core::command::Command::AddObject {
        shape: Shape::SurfaceModel(Box::default()),
        layer: None,
        class: None,
        attrs: Default::default(),
    })
    .unwrap();
    let second = *r.ed.doc().objects.keys().last().unwrap();
    assert_ne!(first, second);
    // The tool draws in the last model it used, not in the new one.
    r.click(v(500.0, 0.0, 0.0));
    r.click(v(600.0, 100.0, 0.0));
    assert_eq!(model_of(r.ed.doc(), first).unwrap().face_count(), 2);
    assert_eq!(model_of(r.ed.doc(), second).unwrap().face_count(), 0);
    // With the second model selected, it goes there.
    r.act(|t, w| {
        let inf = free(v(900.0, 0.0, 0.0));
        let p = Pointer {
            inf: &inf,
            ray: down_ray(inf.point),
            selected: Some(second),
        };
        t.moved(w, &p);
        t.click(w, &p);
        let inf = free(v(1000.0, 100.0, 0.0));
        let p = Pointer {
            inf: &inf,
            ray: down_ray(inf.point),
            selected: Some(second),
        };
        t.moved(w, &p);
        t.click(w, &p)
    });
    assert_eq!(model_of(r.ed.doc(), second).unwrap().face_count(), 1);
}

#[test]
fn the_delete_key_erases_faces_then_edges_in_one_step() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    let model = r.model_id();
    let face = r.model().faces().next().unwrap().0;
    let edge = r.model().edges().next().unwrap().0;
    let mut sel = SubSelection::default();
    sel.set([Item::Face(model, face), Item::Edge(model, edge)]);
    let actions = delete_actions(&sel);
    assert_eq!(actions.len(), 1);
    let depth = r.ed.undo_depth();
    run_actions(&mut r.ed, &actions, false).unwrap();
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.model().face_count(), 0);
    assert_eq!(r.model().edge_count(), 3);
    assert!(delete_actions(&SubSelection::default()).is_empty());
}

#[test]
fn the_scene_is_built_again_only_when_the_revision_changes() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(100.0, 100.0, 0.0));
    let rev = r.ed.revision();
    let _ = r.cache.get(r.ed.doc(), rev);
    let builds = r.cache.builds;
    for _ in 0..50 {
        r.hover(v(1.0, 1.0, 0.0));
        let rev = r.ed.revision();
        let _ = r.cache.get(r.ed.doc(), rev);
    }
    assert_eq!(r.cache.builds, builds, "no build while nothing changed");
    r.click(v(200.0, 0.0, 0.0));
    r.click(v(300.0, 100.0, 0.0));
    let rev = r.ed.revision();
    let n = r.cache.get(r.ed.doc(), rev).face_count();
    assert_eq!(n, 2);
    assert!(r.cache.builds > builds);
}

fn top_camera() -> Camera {
    Camera::look_at(
        v(500.0, 500.0, 5000.0),
        v(500.0, 500.0, 0.0),
        DVec3::Y,
        Projection::Orthographic { height: 2000.0 },
        DVec2::new(800.0, 800.0),
    )
    .unwrap()
}

#[test]
fn clicking_selects_a_face_or_an_edge_and_the_window_and_crossing_select() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    let model = r.model_id();
    let rev = r.ed.revision();
    let cam = top_camera();
    let scenes = r.cache.get(r.ed.doc(), rev);
    let world = World {
        doc: r.ed.doc(),
        scenes,
    };
    let mut eng = Inferencer::default();
    // The middle of the rectangle is the face.
    let mid = cam.project(v(500.0, 500.0, 0.0)).unwrap();
    assert!(matches!(
        select::pick(&world, &mut eng, &cam, mid),
        Some(Item::Face(m, _)) if m == model
    ));
    // Next to an edge is the edge.
    let near = cam.project(v(500.0, 2.0, 0.0)).unwrap();
    assert!(matches!(
        select::pick(&world, &mut eng, &cam, near),
        Some(Item::Edge(m, _)) if m == model
    ));
    // Far away is nothing.
    let far = cam
        .project(v(3000.0, 3000.0, 0.0))
        .unwrap_or(DVec2::new(-500.0, -500.0));
    assert_eq!(select::pick(&world, &mut eng, &cam, far), None);
    // A window around everything takes the face and four edges; a window around a part takes
    // nothing, and a crossing rectangle on the part takes what it touches.
    let (a, b) = (
        cam.project(v(-100.0, -100.0, 0.0)).unwrap(),
        cam.project(v(1100.0, 1100.0, 0.0)).unwrap(),
    );
    let all = select::rect_select(&world, &cam, a, b, false);
    assert_eq!(all.len(), 5);
    let (c, d) = (
        cam.project(v(400.0, 400.0, 0.0)).unwrap(),
        cam.project(v(600.0, 600.0, 0.0)).unwrap(),
    );
    assert!(select::rect_select(&world, &cam, c, d, false).is_empty());
    let touched = select::rect_select(&world, &cam, c, d, true);
    assert_eq!(touched.len(), 1, "inside the face and touching no edge");
    let (e, f) = (
        cam.project(v(400.0, -100.0, 0.0)).unwrap(),
        cam.project(v(600.0, 100.0, 0.0)).unwrap(),
    );
    let touched = select::rect_select(&world, &cam, e, f, true);
    assert_eq!(touched.len(), 2, "the bottom edge and the face");
    // The selection: click replaces, Shift toggles, a click on nothing clears.
    let mut sel = SubSelection::default();
    let items = all;
    sel.click(Some(items[0]), false);
    sel.click(Some(items[1]), true);
    assert_eq!(sel.len(), 2);
    sel.click(Some(items[1]), true);
    assert_eq!(sel.len(), 1);
    sel.click(None, true);
    assert_eq!(sel.len(), 1);
    sel.click(None, false);
    assert!(sel.is_empty());
    // Items that are gone after an undo are dropped.
    sel.set(items.clone());
    let mut ed2 = Editor::with_sequential_ids();
    let doc2 = ed2.doc().clone();
    sel.retain_existing(&doc2);
    assert!(sel.is_empty());
    let _ = &mut ed2;
}

#[test]
fn a_selected_face_has_hatch_lines_inside_it_and_a_hole_stays_open() {
    let outer = vec![
        v(0.0, 0.0, 0.0),
        v(1000.0, 0.0, 0.0),
        v(1000.0, 1000.0, 0.0),
        v(0.0, 1000.0, 0.0),
    ];
    let hole = vec![
        v(400.0, 400.0, 0.0),
        v(400.0, 600.0, 0.0),
        v(600.0, 600.0, 0.0),
        v(600.0, 400.0, 0.0),
    ];
    let plain = select::face_hatch(std::slice::from_ref(&outer), 9);
    assert_eq!(plain.len(), 9);
    for [a, b] in &plain {
        assert!(a.z == 0.0 && b.z == 0.0);
        assert!(a.x >= -1e-9 && b.x <= 1000.0 + 1e-9);
    }
    let holed = select::face_hatch(&[outer, hole], 9);
    assert!(holed.len() > 9, "lines through the hole are cut in two");
    assert!(select::face_hatch(&[], 5).is_empty());
}

#[test]
fn entity_info_names_a_face_an_edge_and_a_model() {
    use crate::modeler::{EntityInfo, entity_info_with_parts};
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 500.0, 0.0));
    let model = r.model_id();
    let face = r.model().faces().next().unwrap().0;
    let edge = r.model().edges().next().unwrap().0;
    let scene = kadakas_core::mesh::build(r.ed.doc());
    let mut sel = SubSelection::default();
    sel.set([Item::Face(model, face)]);
    match entity_info_with_parts(r.ed.doc(), &scene, &[], &sel) {
        EntityInfo::Face {
            area,
            edges,
            holes,
            normal,
            ..
        } => {
            assert!((area - 500_000.0).abs() < 1e-3);
            assert_eq!((edges, holes), (4, 0));
            assert!(normal[2] > 0.99);
        }
        other => panic!("{other:?}"),
    }
    sel.set([Item::Edge(model, edge)]);
    match entity_info_with_parts(r.ed.doc(), &scene, &[], &sel) {
        EntityInfo::Edge { length, faces, .. } => {
            assert!(length > 400.0);
            assert_eq!(faces, 1);
        }
        other => panic!("{other:?}"),
    }
    sel.set([Item::Edge(model, edge), Item::Face(model, face)]);
    assert!(matches!(
        entity_info_with_parts(r.ed.doc(), &scene, &[], &sel),
        EntityInfo::Parts { faces: 1, edges: 1 }
    ));
    sel.clear();
    match entity_info_with_parts(r.ed.doc(), &scene, &[model], &sel) {
        EntityInfo::One {
            surface: Some(s), ..
        } => {
            assert_eq!((s.vertices, s.edges, s.faces), (4, 4, 1));
            assert!(!s.closed);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn only_numbers_and_marks_start_a_value_and_letters_follow_text() {
    let mut t = Modeling::default();
    t.choose(ModelerTool::Circle);
    assert!(!t.type_char('r'), "a letter alone is a key, not a value");
    assert!(t.type_char('2'));
    assert!(t.type_char('4'));
    assert!(t.type_char('s'), "a letter after text is part of the value");
    assert_eq!(t.typed(), "24s");
    assert!(t.backspace());
    assert_eq!(t.typed(), "24");
    t.choose(ModelerTool::Select);
    assert!(!t.type_char('2'), "Select takes no text");
    t.choose(ModelerTool::Eraser);
    assert!(!t.type_char('2'), "the Eraser takes no text");
    let _ = (
        ObjectId::default(),
        DVec2::ZERO,
        Ray {
            origin: DVec3::ZERO,
            dir: DVec3::Z,
        },
    );
}

/// The cost of building the inference scene (measured in a debug build; `--nocapture` prints it).
/// A grid of `n` by `n` cells is `(n + 1) * 2` long edges that cut each other, `n * n` faces.
#[test]
fn the_scene_build_cost_is_measured_and_stays_small() {
    use kadakas_core::command::Command;
    use kadakas_core::surface::Edit;
    let n = 40usize;
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::AddObject {
        shape: Shape::SurfaceModel(Box::default()),
        layer: None,
        class: None,
        attrs: Default::default(),
    })
    .unwrap();
    let id = *ed.doc().objects.keys().next().unwrap();
    let side = 1000.0 * n as f64;
    let mut edges = Vec::new();
    for i in 0..=n {
        let x = 1000.0 * i as f64;
        edges.push([[x, 0.0, 0.0], [x, side, 0.0]]);
        edges.push([[0.0, x, 0.0], [side, x, 0.0]]);
    }
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges { edges },
    })
    .unwrap();
    let m = model_of(ed.doc(), id).unwrap();
    assert_eq!(m.face_count(), n * n);
    let t = std::time::Instant::now();
    let scenes = Scenes::build(ed.doc(), ed.revision());
    let build = t.elapsed();
    assert_eq!(scenes.face_count(), n * n);
    // One inference at a point on the grid.
    let cam = top_camera();
    let mut eng = Inferencer::default();
    let t = std::time::Instant::now();
    for i in 0..100 {
        let _ = eng.infer_scenes(
            &scenes.refs(),
            &cam,
            DVec2::new(100.0 + f64::from(i), 300.0),
            None,
            &|_| false,
        );
    }
    let infer = t.elapsed() / 100;
    eprintln!(
        "scene build: {build:?} for {} faces, {} edges; one inference: {infer:?}",
        m.face_count(),
        m.edge_count()
    );
    assert!(build < std::time::Duration::from_secs(10));
}

#[test]
fn a_refused_redo_gives_the_step_back() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    let face = r.model().faces().next().unwrap().0;
    r.tools.choose(ModelerTool::PushPull);
    r.click_inf(
        on_face(v(500.0, 500.0, 0.0), face),
        down_ray(v(500.0, 500.0, 0.0)),
    );
    r.type_text("400");
    r.tools.choose(ModelerTool::PushPull);
    let top = r
        .model()
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .unwrap()
        .0;
    r.click_inf(
        on_face(v(500.0, 500.0, 400.0), top),
        down_ray(v(500.0, 500.0, 400.0)),
    );
    r.type_text("100");
    let good = r.model().clone();
    let depth2 = r.ed.undo_depth();
    for c in "-900".chars() {
        r.tools.type_char(c);
    }
    let rev = r.ed.revision();
    let res = r.tools.enter_for_test(&mut r.ed, &mut r.cache, rev);
    assert!(res.is_err());
    assert_eq!(r.model(), &good, "the step is back");
    assert_eq!(r.ed.undo_depth(), depth2);
}

#[test]
fn an_edit_builds_only_the_scene_of_the_model_it_changed() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(100.0, 100.0, 0.0));
    r.ed.execute(&kadakas_core::command::Command::AddObject {
        shape: Shape::SurfaceModel(Box::default()),
        layer: None,
        class: None,
        attrs: Default::default(),
    })
    .unwrap();
    let second = *r.ed.doc().objects.keys().last().unwrap();
    let rev = r.ed.revision();
    let _ = r.cache.get(r.ed.doc(), rev);
    let before = r.cache.model_builds;
    // Draw in the second model only (it is selected): the first keeps its scene.
    r.act(|t, w| {
        for p in [v(500.0, 0.0, 0.0), v(600.0, 100.0, 0.0)] {
            let inf = free(p);
            let pt = Pointer {
                inf: &inf,
                ray: down_ray(p),
                selected: Some(second),
            };
            t.moved(w, &pt);
            let o = t.click(w, &pt);
            if !o.actions.is_empty() {
                return o;
            }
        }
        Out::default()
    });
    let rev = r.ed.revision();
    let _ = r.cache.get(r.ed.doc(), rev);
    assert_eq!(
        r.cache.model_builds - before,
        1,
        "one scene was built again"
    );
    assert_eq!(r.cache.get(r.ed.doc(), rev).objects().len(), 2);
}

#[test]
fn the_geometry_being_pulled_is_left_out_of_the_inference() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    let model = r.model_id();
    let face = r.model().faces().next().unwrap().0;
    let rev = r.ed.revision();
    let scenes = r.cache.get(r.ed.doc(), rev);
    let world = World {
        doc: r.ed.doc(),
        scenes,
    };
    let cam = top_camera();
    let mid = cam.project(v(500.0, 500.0, 0.0)).unwrap();
    let mut eng = Inferencer::default();
    let none = Excluded::default();
    let hit = world.infer(&mut eng, &cam, mid, None, &none);
    assert_eq!(hit.source.kind(), crate::infer3d::TagKind::Face);
    let mut ex = Excluded::default();
    ex.faces.insert((model, face));
    let skipped = world.infer(&mut Inferencer::default(), &cam, mid, None, &ex);
    assert_ne!(skipped.source.kind(), crate::infer3d::TagKind::Face);
}

mod landscape;
mod mod08;
mod more;
mod paint;
mod xform;
