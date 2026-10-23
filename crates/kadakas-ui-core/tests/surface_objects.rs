//! Surface models and component instances in the plan tools (MOD-13): a click picks the footprint
//! and not the box, the Select tool drags the model with one undo step, a window selects it,
//! hidden and grey classes cannot be picked, and the snap finds its vertices, edge middles and
//! face centres. Only the public API of the crate is used.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, ClassId, ObjectId, Shape};
use kadakas_core::snapping::{Kind, Settings, Smart, SnapSet};
use kadakas_core::surface::Edit;
use kadakas_core::visibility::VisMode;
use kadakas_ui_core::effects::apply;
use kadakas_ui_core::hit::{box_select_in, hit_test_in};
use kadakas_ui_core::snap::snap_cursor;
use kadakas_ui_core::tools::{Ctx, Input, Tool, ToolMachine, ToolSettings};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn hit_test(doc: &kadakas_core::model::Document, p: DVec2, tol: f64) -> Option<ObjectId> {
    hit_test_in(Default::default(), doc, p, tol, None)
}

fn add_model(ed: &mut Editor, ring: &[(f64, f64)], class: Option<ClassId>) -> ObjectId {
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let n = ring.len();
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges {
            edges: (0..n)
                .map(|i| {
                    [
                        [ring[i].0, ring[i].1, 0.0],
                        [ring[(i + 1) % n].0, ring[(i + 1) % n].1, 0.0],
                    ]
                })
                .collect(),
        },
    })
    .unwrap();
    id
}

/// An L: 2000 x 2000 mm in its box, with the arms 500 mm wide.
const L: [(f64, f64); 6] = [
    (0.0, 0.0),
    (2000.0, 0.0),
    (2000.0, 500.0),
    (500.0, 500.0),
    (500.0, 2000.0),
    (0.0, 2000.0),
];

fn bounds_of(ed: &Editor, id: ObjectId) -> (DVec2, DVec2) {
    ed.doc().object_bounds(ed.doc().object(id).unwrap())
}

#[test]
fn a_click_picks_the_footprint_and_not_the_box() {
    let mut ed = Editor::default();
    let id = add_model(&mut ed, &L, None);
    let hit = |x: f64, y: f64| hit_test(ed.doc(), v(x, y), 5.0);
    assert_eq!(hit(1500.0, 250.0), Some(id));
    assert_eq!(hit(250.0, 1500.0), Some(id));
    // The notch of the L is inside the box and outside the model.
    assert_eq!(hit(1500.0, 1500.0), None);
    // Near an edge counts.
    assert_eq!(hit(2003.0, 250.0), Some(id));
    assert_eq!(hit(2020.0, 250.0), None);
}

#[test]
fn dragging_a_model_with_the_select_tool_moves_it_in_one_undo_step() {
    let mut ed = Editor::default();
    let id = add_model(&mut ed, &L, None);
    let settings = ToolSettings::default();
    let mut selection: Vec<ObjectId> = Vec::new();
    let mut m = ToolMachine::default();
    m.set_tool(Tool::Select);
    let mut send = |ed: &mut Editor, selection: &mut Vec<ObjectId>, input: Input| {
        let fx = {
            let cx = Ctx {
                doc: ed.doc(),
                selection,
                settings: &settings,
                tol: 5.0,
                context: None,
                annot: Default::default(),
            };
            m.handle(input, &cx)
        };
        apply(fx, ed, selection, None);
    };
    send(
        &mut ed,
        &mut selection,
        Input::Press {
            p: v(1500.0, 250.0),
            shift: false,
        },
    );
    assert_eq!(selection, vec![id]);
    send(&mut ed, &mut selection, Input::Move(v(2000.0, 450.0)));
    send(&mut ed, &mut selection, Input::Release(v(2500.0, 750.0)));
    assert_eq!(bounds_of(&ed, id), (v(1000.0, 500.0), v(3000.0, 2500.0)));
    ed.undo();
    assert_eq!(bounds_of(&ed, id), (v(0.0, 0.0), v(2000.0, 2000.0)));
    ed.redo();
    assert_eq!(bounds_of(&ed, id), (v(1000.0, 500.0), v(3000.0, 2500.0)));
}

#[test]
fn a_window_selects_what_it_encloses_and_a_crossing_what_it_touches() {
    let mut ed = Editor::default();
    let id = add_model(&mut ed, &L, None);
    // Left to right: the whole box must be inside.
    assert!(
        box_select_in(
            Default::default(),
            ed.doc(),
            v(-100.0, -100.0),
            v(2100.0, 2100.0),
            false,
            None
        )
        .contains(&id)
    );
    assert!(
        box_select_in(
            Default::default(),
            ed.doc(),
            v(-100.0, -100.0),
            v(1000.0, 1000.0),
            false,
            None
        )
        .is_empty()
    );
    // Crossing: touching is enough.
    assert!(
        box_select_in(
            Default::default(),
            ed.doc(),
            v(1900.0, 400.0),
            v(2500.0, 800.0),
            true,
            None
        )
        .contains(&id)
    );
}

#[test]
fn a_class_that_is_off_or_grey_cannot_be_picked() {
    let mut ed = Editor::default();
    let class = ClassId(
        ed.execute(&Command::AddClass {
            name: "Sheds".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let id = add_model(&mut ed, &L, Some(class));
    assert_eq!(hit_test(ed.doc(), v(1500.0, 250.0), 5.0), Some(id));
    for mode in [VisMode::Grey, VisMode::Invisible] {
        ed.execute(&Command::SetVisibility {
            layers: vec![],
            classes: vec![class],
            mode,
        })
        .unwrap();
        assert_eq!(hit_test(ed.doc(), v(1500.0, 250.0), 5.0), None, "{mode:?}");
    }
    // A locked layer cannot be picked.
    ed.execute(&Command::SetVisibility {
        layers: vec![],
        classes: vec![class],
        mode: VisMode::Visible,
    })
    .unwrap();
    let layer = ed.doc().object(id).unwrap().layer;
    ed.execute(&Command::ModifyLayer {
        id: layer,
        name: None,
        visible: None,
        locked: Some(true),
        elevation: None,
        scale: None,
    })
    .unwrap();
    assert_eq!(hit_test(ed.doc(), v(1500.0, 250.0), 5.0), None);
}

#[test]
fn the_snap_lands_on_a_vertex_an_edge_middle_and_a_face_centre() {
    let mut ed = Editor::default();
    add_model(
        &mut ed,
        &[(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)],
        None,
    );
    let settings = Settings {
        set: SnapSet {
            grid: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let snap = |x: f64, y: f64| {
        snap_cursor(
            ed.doc(),
            v(x, y),
            8.0,
            100.0,
            &settings,
            &Smart::default(),
            None,
            None,
            &[],
        )
    };
    let s = snap(1004.0, 3.0).expect("a vertex");
    assert_eq!((s.point, s.kind), (v(1000.0, 0.0), Kind::Endpoint));
    let s = snap(503.0, 4.0).expect("a middle");
    assert_eq!((s.point, s.kind), (v(500.0, 0.0), Kind::Midpoint));
    let s = snap(497.0, 506.0).expect("a centre");
    assert_eq!((s.point, s.kind), (v(500.0, 500.0), Kind::Center));
}

#[test]
fn an_instance_that_cannot_be_drawn_is_picked_at_its_mark() {
    let mut ed = Editor::default();
    let id = add_model(
        &mut ed,
        &[(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)],
        None,
    );
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Square".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![id],
            replace: true,
        })
        .unwrap();
    let inst = ObjectId(out.created[1]);
    assert_eq!(hit_test(ed.doc(), v(500.0, 500.0), 5.0), Some(inst));
    // A document from a file holds the same instance with its definition gone.
    let mut doc = ed.doc().clone();
    let Shape::Component { transform, .. } = doc.object(inst).unwrap().shape.clone() else {
        panic!()
    };
    doc.object_mut(inst).unwrap().shape = Shape::Component {
        symbol: kadakas_core::resource::ResourceId::from_num(99),
        transform,
    };
    // The mark is at the origin of the instance: a click there picks it, one far away does not.
    assert_eq!(hit_test(&doc, v(0.0, 0.0), 5.0), Some(inst));
    assert_eq!(hit_test(&doc, v(5000.0, 5000.0), 5.0), None);
}
