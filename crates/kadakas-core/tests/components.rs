//! Groups and components (MOD-07): make group, make component, explode, make unique, edit in
//! place, the edit context, undo, the file and nesting.

use glam::{DAffine3, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::component::{self, EditContext};
use kadakas_core::model::*;
use kadakas_core::resource::ResourceId;
use kadakas_core::surface::{EdgeId, Edit, FaceId, SurfaceModel};

fn fresh() -> Editor {
    Editor::with_sequential_ids()
}

fn add_shape(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn add_model(ed: &mut Editor) -> ObjectId {
    add_shape(ed, Shape::SurfaceModel(Box::default()))
}

fn square(x: f64, y: f64, w: f64) -> Edit {
    let p = [
        [x, y, 0.0],
        [x + w, y, 0.0],
        [x + w, y + w, 0.0],
        [x, y + w, 0.0],
    ];
    Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    }
}

fn edit(ed: &mut Editor, id: ObjectId, e: Edit) {
    ed.execute(&Command::EditSurface { id, edit: e }).unwrap();
}

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("not a surface model: {}", other.kind()),
    }
}

fn faces(ed: &Editor, id: ObjectId) -> Vec<FaceId> {
    model(ed, id).faces().map(|(f, _)| f).collect()
}

/// A model with two squares side by side: faces 0 and 1 share an edge.
fn two_squares(ed: &mut Editor) -> ObjectId {
    let m = add_model(ed);
    edit(ed, m, square(0.0, 0.0, 1000.0));
    edit(ed, m, square(1000.0, 0.0, 1000.0));
    assert_eq!(model(ed, m).face_count(), 2);
    m
}

fn make_group(
    ed: &mut Editor,
    m: ObjectId,
    f: Vec<FaceId>,
    e: Vec<EdgeId>,
) -> (ObjectId, Vec<ObjectId>) {
    let out = ed
        .execute(&Command::MakeGroup {
            model: Some(m),
            faces: f,
            edges: e,
            objects: vec![],
            name: Some("g".into()),
        })
        .unwrap();
    let ids: Vec<ObjectId> = out.created.iter().map(|u| ObjectId(*u)).collect();
    (ids[0], ids[1..].to_vec())
}

fn make_component(
    ed: &mut Editor,
    name: &str,
    m: ObjectId,
    f: Vec<FaceId>,
) -> (ResourceId, ObjectId) {
    let out = ed
        .execute(&Command::MakeComponent {
            name: name.into(),
            model: Some(m),
            faces: f,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap();
    (ResourceId(out.created[0]), ObjectId(out.created[1]))
}

fn invalid(r: Result<kadakas_core::command::Outcome, CommandError>, needle: &str) {
    match r {
        Err(CommandError::Invalid(m)) => assert!(m.contains(needle), "{m} should contain {needle}"),
        other => panic!("expected an error with {needle:?}, got {other:?}"),
    }
}

#[test]
fn make_group_moves_the_chosen_faces_into_a_model_of_their_own() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let all = faces(&ed, m);
    let before = ed.doc().clone();
    let (g, rest) = make_group(&mut ed, m, vec![all[0]], vec![]);
    assert_eq!(rest.len(), 1);
    let s = rest[0];
    assert!(matches!(&ed.doc().object(g).unwrap().shape, Shape::Group { name } if name == "g"));
    assert_eq!(ed.doc().object(s).unwrap().parent, Some(g));
    assert_eq!(model(&ed, s).face_count(), 1);
    assert_eq!(model(&ed, s).edge_count(), 4);
    // The rest keeps its face; the edge that both faces used stays.
    assert_eq!(model(&ed, m).face_count(), 1);
    assert_eq!(model(&ed, m).edge_count(), 4);
    assert!(model(&ed, m).problem().is_none() && model(&ed, s).problem().is_none());
    // Undo and redo are one step.
    assert!(ed.undo());
    assert_eq!(ed.doc(), &before);
    assert!(ed.redo());
    assert_eq!(model(&ed, s).face_count(), 1);
}

#[test]
fn geometry_in_a_group_does_not_weld_to_the_rest() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let (_, rest) = make_group(&mut ed, m, vec![f[0]], vec![]);
    // Draw an edge across the square that is now in the group: the other model is untouched.
    edit(
        &mut ed,
        m,
        Edit::AddEdges {
            edges: vec![[[1000.0, 500.0, 0.0], [0.0, 500.0, 0.0]]],
        },
    );
    assert_eq!(model(&ed, rest[0]).face_count(), 1);
    assert_eq!(model(&ed, rest[0]).edge_count(), 4);
}

#[test]
fn a_group_of_a_whole_model_moves_the_model_object_itself() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 500.0));
    let f = faces(&ed, m);
    let (g, rest) = make_group(&mut ed, m, f, vec![]);
    assert!(rest.is_empty());
    assert_eq!(ed.doc().object(m).unwrap().parent, Some(g));
    assert_eq!(model(&ed, m).face_count(), 1);
}

#[test]
fn loose_edges_and_curves_can_be_grouped() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    edit(
        &mut ed,
        m,
        Edit::AddEdges {
            edges: vec![[[0.0, 0.0, 0.0], [0.0, 0.0, 700.0]]],
        },
    );
    let loose = model(&ed, m)
        .edges()
        .find(|(_, e)| {
            let (a, b) = (
                model(&ed, m).vertex(e.v[0]).unwrap(),
                model(&ed, m).vertex(e.v[1]).unwrap(),
            );
            a.z != b.z
        })
        .unwrap()
        .0;
    let (_, rest) = make_group(&mut ed, m, vec![], vec![loose]);
    assert_eq!(model(&ed, rest[0]).edge_count(), 1);
    assert_eq!(model(&ed, rest[0]).face_count(), 0);
    assert_eq!(model(&ed, m).face_count(), 2);
    assert!(model(&ed, m).edge(loose).is_none());
}

#[test]
fn make_group_rules() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let go = |ed: &mut Editor, c: Command| ed.execute(&c);
    let base = Command::MakeGroup {
        model: Some(m),
        faces: vec![f[0]],
        edges: vec![],
        objects: vec![],
        name: None,
    };
    // Nothing chosen.
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![],
                name: None,
            },
        ),
        "choose",
    );
    // Geometry without its model.
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: None,
                faces: vec![f[0]],
                edges: vec![],
                objects: vec![],
                name: None,
            },
        ),
        "surface model of",
    );
    // A model that is not one.
    let c = add_shape(
        &mut ed,
        Shape::Circle {
            center: glam::DVec2::ZERO,
            radius: 5.0,
        },
    );
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: Some(c),
                faces: vec![f[0]],
                edges: vec![],
                objects: vec![],
                name: None,
            },
        ),
        "not a surface model",
    );
    // Unknown face and edge.
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: Some(m),
                faces: vec![FaceId(999)],
                edges: vec![],
                objects: vec![],
                name: None,
            },
        ),
        "999",
    );
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: Some(m),
                faces: vec![],
                edges: vec![EdgeId(999)],
                objects: vec![],
                name: None,
            },
        ),
        "999",
    );
    // The model as an object and as the source.
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: Some(m),
                faces: vec![f[0]],
                edges: vec![],
                objects: vec![m],
                name: None,
            },
        ),
        "both",
    );
    // A long name.
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: Some(m),
                faces: vec![f[0]],
                edges: vec![],
                objects: vec![],
                name: Some("x".repeat(201)),
            },
        ),
        "at most",
    );
    // Objects in different groups.
    let (g, _) = make_group(&mut ed, m, vec![f[0]], vec![]);
    let d = add_shape(
        &mut ed,
        Shape::Circle {
            center: glam::DVec2::ZERO,
            radius: 5.0,
        },
    );
    let inner = ObjectId(
        ed.execute(&Command::AddObjectIn {
            parent: g,
            shape: Shape::Circle {
                center: glam::DVec2::ZERO,
                radius: 1.0,
            },
            class: None,
            attrs: Attributes::default(),
            id: None,
        })
        .unwrap()
        .created[0],
    );
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![d, inner],
                name: None,
            },
        ),
        "different groups",
    );
    // Objects on different layers.
    let layer = LayerId(
        ed.execute(&Command::AddLayer {
            name: "L2".into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0],
    );
    let other = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Circle {
                center: glam::DVec2::ZERO,
                radius: 1.0,
            },
            layer: Some(layer),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    invalid(
        go(
            &mut ed,
            Command::MakeGroup {
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![d, other],
                name: None,
            },
        ),
        "one layer",
    );
    // A locked layer.
    ed.execute(&Command::ModifyLayer {
        id: layer,
        name: None,
        visible: None,
        locked: Some(true),
        elevation: None,
        scale: None,
    })
    .unwrap();
    assert!(matches!(
        go(
            &mut ed,
            Command::MakeGroup {
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![other],
                name: None
            }
        ),
        Err(CommandError::LayerLocked(_))
    ));
    let _ = base;
}

#[test]
fn explode_a_group_merges_the_geometry_back() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let original = model(&ed, m).clone();
    let (g, rest) = make_group(&mut ed, m, vec![f[0]], vec![]);
    ed.execute(&Command::Explode { ids: vec![g] }).unwrap();
    assert!(ed.doc().object(g).is_none());
    assert!(ed.doc().object(rest[0]).is_none());
    let merged = model(&ed, m);
    assert_eq!(merged.face_count(), original.face_count());
    assert_eq!(merged.edge_count(), original.edge_count());
    assert_eq!(merged.vertex_count(), original.vertex_count());
    assert!((merged.area() - original.area()).abs() < 1e-6);
    assert!(merged.problem().is_none());
    // One undo brings the group back.
    assert!(ed.undo());
    assert!(ed.doc().object(g).is_some() && ed.doc().object(rest[0]).is_some());
}

#[test]
fn a_moved_group_welds_to_what_it_touches_when_exploded() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let n = add_model(&mut ed);
    edit(&mut ed, n, square(5000.0, 0.0, 1000.0));
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![n],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    // Move the group so that its square touches the first one along an edge.
    ed.execute(&Command::MoveObjects {
        ids: vec![g],
        delta: glam::DVec2::new(-4000.0, 0.0),
    })
    .unwrap();
    ed.execute(&Command::Explode { ids: vec![g] }).unwrap();
    // The group's model was the only one in the group; the model m is the model of the level.
    let merged = model(&ed, m);
    assert_eq!(merged.face_count(), 2);
    assert_eq!(merged.vertex_count(), 6);
    assert_eq!(merged.edge_count(), 7);
    assert!(ed.doc().object(n).is_none());
}

#[test]
fn explode_without_a_model_at_the_level_keeps_the_first_model() {
    let mut ed = fresh();
    let a = add_model(&mut ed);
    edit(&mut ed, a, square(0.0, 0.0, 1000.0));
    let b = add_model(&mut ed);
    edit(&mut ed, b, square(3000.0, 0.0, 1000.0));
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![a, b],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::Explode { ids: vec![g] }).unwrap();
    // The first model became the model of the level and took the second one.
    assert_eq!(ed.doc().object(a).unwrap().parent, None);
    assert!(ed.doc().object(b).is_none());
    assert_eq!(model(&ed, a).face_count(), 2);
}

#[test]
fn explode_takes_groups_inside_up_one_level() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let (inner, _) = make_group(&mut ed, m, vec![f[0]], vec![]);
    let outer = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![inner],
            name: Some("o".into()),
        })
        .unwrap()
        .created[0],
    );
    assert_eq!(ed.doc().object(inner).unwrap().parent, Some(outer));
    ed.execute(&Command::Explode { ids: vec![outer] }).unwrap();
    assert!(ed.doc().object(outer).is_none());
    assert_eq!(ed.doc().object(inner).unwrap().parent, None);
}

#[test]
fn explode_rules() {
    let mut ed = fresh();
    let c = add_shape(
        &mut ed,
        Shape::Circle {
            center: glam::DVec2::ZERO,
            radius: 5.0,
        },
    );
    invalid(
        ed.execute(&Command::Explode { ids: vec![c] }),
        "not a group",
    );
    invalid(
        ed.execute(&Command::Explode { ids: vec![] }),
        "at least one",
    );
    assert!(matches!(
        ed.execute(&Command::Explode {
            ids: vec![ObjectId::from_num(77)]
        }),
        Err(CommandError::NoObject(_))
    ));
}

#[test]
fn a_mirrored_group_keeps_the_side_of_its_faces() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let n = add_model(&mut ed);
    edit(&mut ed, n, square(0.0, 2000.0, 1000.0));
    // Turn the second model over: a mirror in z makes its face point down.
    let Shape::SurfaceModel(sm) = ed.doc().object(n).unwrap().shape.clone() else {
        panic!()
    };
    let mut flipped = (*sm).clone();
    flipped.place(DAffine3::from_scale(DVec3::new(1.0, 1.0, -1.0)));
    ed.execute(&Command::ModifyObject {
        id: n,
        shape: Some(Shape::SurfaceModel(Box::new(flipped))),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![n],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::Explode { ids: vec![g] }).unwrap();
    assert_eq!(model(&ed, m).face_count(), 2);
    assert!(model(&ed, m).check().problems.is_empty());
}

#[test]
fn make_component_creates_a_definition_and_one_instance() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let before = ed.doc().clone();
    let (sym, inst) = make_component(&mut ed, "Bench", m, vec![f[0]]);
    let def = ed.doc().resources.symbol(sym).unwrap();
    assert!(def.is_component() && def.kind() == "component");
    assert_eq!(def.parts.len(), 1);
    let Shape::Component { symbol, transform } = &ed.doc().object(inst).unwrap().shape else {
        panic!()
    };
    assert_eq!(*symbol, sym);
    assert_eq!(component::affine(transform), DAffine3::IDENTITY);
    assert_eq!(model(&ed, m).face_count(), 1);
    // The instance has a size in the plan.
    let b = ed.doc().object_bounds(ed.doc().object(inst).unwrap());
    assert_eq!((b.0.x, b.1.x), (0.0, 1000.0));
    assert!(ed.undo());
    assert_eq!(ed.doc(), &before);
    assert!(ed.redo());
    assert!(ed.doc().object(inst).is_some());
}

#[test]
fn a_component_made_without_replace_adds_only_the_definition() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let out = ed
        .execute(&Command::MakeComponent {
            name: "C".into(),
            model: Some(m),
            faces: vec![f[0]],
            edges: vec![],
            objects: vec![],
            replace: false,
        })
        .unwrap();
    assert_eq!(out.created.len(), 1);
    assert_eq!(model(&ed, m).face_count(), 2);
}

#[test]
fn instances_share_the_definition_and_edit_in_place_changes_all() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let f = faces(&ed, m);
    let (sym, a) = make_component(&mut ed, "Tile", m, f);
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![a],
            delta: glam::DVec2::new(3000.0, 0.0),
        })
        .unwrap();
    let b = ObjectId(out.created[0]);
    let area = |ed: &Editor| -> f64 {
        component::leaves(&ed.doc().resources, sym)
            .unwrap()
            .iter()
            .map(|l| l.model.area())
            .sum()
    };
    assert_eq!(area(&ed), 1_000_000.0);
    let depth = ed.undo_depth();
    let out = ed.execute(&Command::EditComponent { id: a }).unwrap();
    let eg = ObjectId(out.created[0]);
    assert_eq!(ed.undo_depth(), depth + 1);
    // The instance is hidden while it is open; a second open is refused.
    assert!(kadakas_core::symbols::is_being_edited(
        ed.doc().object(a).unwrap()
    ));
    invalid(
        ed.execute(&Command::EditComponent { id: b }),
        "being edited already",
    );
    invalid(
        ed.execute(&Command::EditComponent { id: a }),
        "being edited",
    );
    // Draw a second square in the edit group.
    let inner = ed.doc().children(eg);
    assert_eq!(inner.len(), 1);
    edit(&mut ed, inner[0], square(1000.0, 0.0, 1000.0));
    ed.execute(&Command::CloseComponent {
        group: eg,
        resolve: None,
    })
    .unwrap();
    assert!(ed.doc().object(eg).is_none());
    assert!(!kadakas_core::symbols::is_being_edited(
        ed.doc().object(a).unwrap()
    ));
    // Both instances use the changed definition.
    assert_eq!(area(&ed), 2_000_000.0);
    let (ba, bb) = (
        ed.doc().object_bounds(ed.doc().object(a).unwrap()),
        ed.doc().object_bounds(ed.doc().object(b).unwrap()),
    );
    assert_eq!((ba.1.x - ba.0.x, bb.1.x - bb.0.x), (2000.0, 2000.0));
    assert_eq!(bb.0.x, 3000.0);
    // Undo of the close brings the edit group back; undo of the open removes it.
    assert!(ed.undo());
    assert!(ed.doc().object(eg).is_some());
    assert_eq!(area(&ed), 1_000_000.0);
    // The drawing inside is its own step, and then the open.
    assert!(ed.undo());
    assert!(ed.doc().object(eg).is_some());
    assert!(ed.undo());
    assert!(ed.doc().object(eg).is_none());
    assert!(!kadakas_core::symbols::is_being_edited(
        ed.doc().object(a).unwrap()
    ));
}

#[test]
fn make_unique_gives_an_instance_its_own_definition() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let f = faces(&ed, m);
    let (sym, a) = make_component(&mut ed, "Tile", m, f);
    // Only user: refused.
    invalid(
        ed.execute(&Command::MakeUnique { ids: vec![a] }),
        "already unique",
    );
    let b = ObjectId(
        ed.execute(&Command::CopyObjects {
            ids: vec![a],
            delta: glam::DVec2::new(3000.0, 0.0),
        })
        .unwrap()
        .created[0],
    );
    let out = ed.execute(&Command::MakeUnique { ids: vec![b] }).unwrap();
    let copy = ResourceId(out.created[0]);
    assert_ne!(copy, sym);
    assert_eq!(ed.doc().resources.get(copy).unwrap().name, "Tile#1");
    let Shape::Component { symbol, .. } = &ed.doc().object(b).unwrap().shape else {
        panic!()
    };
    assert_eq!(*symbol, copy);
    // Edit one: the other does not change.
    let eg = ObjectId(
        ed.execute(&Command::EditComponent { id: b })
            .unwrap()
            .created[0],
    );
    let inner = ed.doc().children(eg)[0];
    edit(&mut ed, inner, square(1000.0, 0.0, 1000.0));
    ed.execute(&Command::CloseComponent {
        group: eg,
        resolve: None,
    })
    .unwrap();
    let area = |s| -> f64 {
        component::leaves(&ed.doc().resources, s)
            .unwrap()
            .iter()
            .map(|l| l.model.area())
            .sum()
    };
    assert_eq!(area(sym), 1_000_000.0);
    assert_eq!(area(copy), 2_000_000.0);
    // A second make unique of the copy: the name moves to #2 only if #1 is taken.
    let c = ObjectId(
        ed.execute(&Command::CopyObjects {
            ids: vec![b],
            delta: glam::DVec2::new(0.0, 3000.0),
        })
        .unwrap()
        .created[0],
    );
    let out = ed.execute(&Command::MakeUnique { ids: vec![c] }).unwrap();
    assert_eq!(
        ed.doc()
            .resources
            .get(ResourceId(out.created[0]))
            .unwrap()
            .name,
        "Tile#2"
    );
    // Selected instances that share a definition share the copy.
    let d = ObjectId(
        ed.execute(&Command::CopyObjects {
            ids: vec![a],
            delta: glam::DVec2::new(0.0, 6000.0),
        })
        .unwrap()
        .created[0],
    );
    let e = ObjectId(
        ed.execute(&Command::CopyObjects {
            ids: vec![a],
            delta: glam::DVec2::new(0.0, 9000.0),
        })
        .unwrap()
        .created[0],
    );
    let out = ed
        .execute(&Command::MakeUnique { ids: vec![d, e] })
        .unwrap();
    assert_eq!(out.created.len(), 1);
    assert!(ed.undo());
    assert!(ed.doc().resources.get(ResourceId(out.created[0])).is_none());
    // Rules.
    invalid(
        ed.execute(&Command::MakeUnique { ids: vec![] }),
        "at least one",
    );
    let plain = add_shape(
        &mut ed,
        Shape::Circle {
            center: glam::DVec2::ZERO,
            radius: 1.0,
        },
    );
    invalid(
        ed.execute(&Command::MakeUnique { ids: vec![plain] }),
        "not a component",
    );
}

#[test]
fn explode_an_instance_makes_plain_geometry_and_purges_the_last_definition() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let (sym, a) = make_component(&mut ed, "Tile", m, vec![f[0]]);
    let b = ObjectId(
        ed.execute(&Command::CopyObjects {
            ids: vec![a],
            delta: glam::DVec2::new(0.0, 3000.0),
        })
        .unwrap()
        .created[0],
    );
    // One of two: the definition stays.
    ed.execute(&Command::Explode { ids: vec![b] }).unwrap();
    assert!(ed.doc().resources.get(sym).is_some());
    assert!(ed.doc().object(b).is_none());
    // The geometry is plain, in the drawing, and merged into the model of the level.
    assert_eq!(model(&ed, m).face_count(), 2);
    let top = model(&ed, m).bounds().unwrap();
    assert_eq!(top.1.y, 4000.0);
    // The last one takes the definition with it.
    ed.execute(&Command::Explode { ids: vec![a] }).unwrap();
    assert!(ed.doc().resources.get(sym).is_none());
    assert_eq!(model(&ed, m).face_count(), 3);
    assert!(ed.undo());
    assert!(ed.doc().resources.get(sym).is_some());
}

#[test]
fn a_component_in_a_component_and_the_limits_of_nesting() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let f = faces(&ed, m);
    let (inner, i1) = make_component(&mut ed, "Level 1", m, f);
    let mut last = i1;
    let mut last_sym = inner;
    // Nest up to the limit; one more is refused with a message and changes nothing.
    for level in 2..=component::MAX_COMPONENT_DEPTH {
        let out = ed
            .execute(&Command::MakeComponent {
                name: format!("Level {level}"),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![last],
                replace: true,
            })
            .unwrap();
        last_sym = ResourceId(out.created[0]);
        last = ObjectId(out.created[1]);
    }
    let leaves = component::leaves(&ed.doc().resources, last_sym).unwrap();
    assert_eq!(leaves.len(), 1);
    assert_eq!(
        component::definition_depth(&ed.doc().resources, last_sym),
        component::MAX_COMPONENT_DEPTH
    );
    let before = ed.doc().clone();
    invalid(
        ed.execute(&Command::MakeComponent {
            name: "Too deep".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![last],
            replace: true,
        }),
        "nested at most",
    );
    assert_eq!(ed.doc(), &before);
}

#[test]
fn a_component_cannot_contain_itself() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let f = faces(&ed, m);
    let (sym, a) = make_component(&mut ed, "A", m, f);
    let eg = ObjectId(
        ed.execute(&Command::EditComponent { id: a })
            .unwrap()
            .created[0],
    );
    // An instance of A inside A.
    ed.execute(&Command::AddObjectIn {
        parent: eg,
        shape: Shape::Component {
            symbol: sym,
            transform: component::raw(&DAffine3::IDENTITY),
        },
        class: None,
        attrs: Attributes::default(),
        id: None,
    })
    .unwrap();
    invalid(
        ed.execute(&Command::CloseComponent {
            group: eg,
            resolve: None,
        }),
        "contain itself",
    );
    // Nothing changed: the edit is still open.
    assert!(ed.doc().object(eg).is_some());
}

#[test]
fn groups_are_nested_to_a_limit() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let mut current = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![m],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    for _ in 1..component::MAX_GROUP_DEPTH {
        current = ObjectId(
            ed.execute(&Command::MakeGroup {
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![current],
                name: None,
            })
            .unwrap()
            .created[0],
        );
    }
    invalid(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![current],
            name: None,
        }),
        "nested at most",
    );
    // A group deep inside cannot get another level either.
    let leaf = ed.doc().leaves(&[current])[0];
    let one = model(&ed, leaf).edges().next().unwrap().0;
    invalid(
        ed.execute(&Command::MakeGroup {
            model: Some(leaf),
            faces: vec![],
            edges: vec![one],
            objects: vec![],
            name: None,
        }),
        "nested at most",
    );
}

#[test]
fn the_edit_context_enters_groups_and_components() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let (g, rest) = make_group(&mut ed, m, vec![f[0]], vec![]);
    let mut cx = EditContext::new();
    assert!(cx.is_root() && cx.container().is_none() && cx.path().is_empty());
    assert!(!cx.dimmed(ed.doc(), m));
    // Not a direct member of the root: m is, the model inside the group is not.
    assert!(cx.enter(&mut ed, rest[0]).is_err());
    cx.enter(&mut ed, g).unwrap();
    assert_eq!(cx.path(), vec![g]);
    assert_eq!(cx.container(), Some(g));
    assert!(cx.contains(ed.doc(), rest[0]) && !cx.contains(ed.doc(), m));
    assert!(cx.dimmed(ed.doc(), m) && !cx.dimmed(ed.doc(), rest[0]));
    cx.dim_rest = false;
    assert!(!cx.dimmed(ed.doc(), m));
    assert_eq!(cx.pick(ed.doc(), rest[0]), Some(rest[0]));
    assert_eq!(cx.pick(ed.doc(), m), None);
    assert_eq!(cx.raw_model(ed.doc()), Some(rest[0]));
    // Drawing in the context goes to the raw model of the group, with points of the drawing.
    let before = ed.undo_depth();
    cx.apply_world(
        &mut ed,
        &Edit::AddEdges {
            edges: vec![[[0.0, 0.0, 0.0], [0.0, 0.0, 500.0]]],
        },
    )
    .unwrap();
    assert_eq!(ed.undo_depth(), before + 1);
    assert_eq!(model(&ed, rest[0]).edge_count(), 5);
    assert_eq!(model(&ed, m).edge_count(), 4);
    assert_eq!(
        cx.to_local(ed.doc(), DVec3::new(1.0, 2.0, 3.0)),
        Some(DVec3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(
        cx.to_world(ed.doc(), DVec3::new(1.0, 2.0, 3.0)),
        Some(DVec3::new(1.0, 2.0, 3.0))
    );
    assert_eq!(cx.exit(&mut ed).unwrap(), Some(g));
    assert!(cx.is_root());
    assert_eq!(cx.exit(&mut ed).unwrap(), None);
}

#[test]
fn drawing_in_a_moved_component_converts_through_the_transform() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let f = faces(&ed, m);
    let (sym, a) = make_component(&mut ed, "Tile", m, f);
    ed.execute(&Command::MoveObjects {
        ids: vec![a],
        delta: glam::DVec2::new(5000.0, 0.0),
    })
    .unwrap();
    let mut cx = EditContext::new();
    cx.enter(&mut ed, a).unwrap();
    let eg = cx.container().unwrap();
    assert_ne!(eg, a);
    assert_eq!(cx.path(), vec![a]);
    // The point of the drawing (6000, 500, 0) is (1000, 500, 0) in the model.
    assert_eq!(
        cx.to_local(ed.doc(), DVec3::new(6000.0, 500.0, 0.0)),
        Some(DVec3::new(1000.0, 500.0, 0.0))
    );
    cx.apply_world(
        &mut ed,
        &Edit::AddEdges {
            edges: vec![[[6000.0, 500.0, 0.0], [5000.0, 500.0, 0.0]]],
        },
    )
    .unwrap();
    let inner = cx.raw_model(ed.doc()).unwrap();
    assert_eq!(model(&ed, inner).face_count(), 2);
    cx.exit(&mut ed).unwrap();
    let def = component::leaves(&ed.doc().resources, sym).unwrap();
    assert_eq!(def[0].model.face_count(), 2);
    // The definition is in its own coordinates: the model starts at 0, not at 5000.
    assert_eq!(def[0].model.local_bounds().unwrap().0.x, 0.0);
}

#[test]
fn a_level_with_no_model_gets_one_when_drawing() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![m],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    let n = add_shape(
        &mut ed,
        Shape::Circle {
            center: glam::DVec2::ZERO,
            radius: 1.0,
        },
    );
    ed.execute(&Command::SetParent {
        ids: vec![n],
        parent: Some(g),
    })
    .unwrap();
    ed.execute(&Command::SetParent {
        ids: vec![m],
        parent: None,
    })
    .unwrap();
    let mut cx = EditContext::new();
    cx.enter(&mut ed, g).unwrap();
    assert!(cx.raw_model(ed.doc()).is_none());
    let depth = ed.undo_depth();
    cx.apply_world(
        &mut ed,
        &Edit::AddEdges {
            edges: vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]],
        },
    )
    .unwrap();
    // The model and the edge are one undo step.
    assert_eq!(ed.undo_depth(), depth + 1);
    let made = cx.raw_model(ed.doc()).unwrap();
    assert_eq!(model(&ed, made).edge_count(), 1);
    assert!(ed.undo());
    assert!(cx.raw_model(ed.doc()).is_none());
    // A root level gets one too.
    cx.exit(&mut ed).unwrap();
    let mut empty = fresh();
    EditContext::new()
        .apply_world(
            &mut empty,
            &Edit::AddEdges {
                edges: vec![[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]],
            },
        )
        .unwrap();
    assert_eq!(empty.doc().objects.len(), 1);
}

#[test]
fn the_context_survives_undo_and_deleted_containers() {
    let mut ed = fresh();
    let m = two_squares(&mut ed);
    let f = faces(&ed, m);
    let (sym, a) = make_component(&mut ed, "Tile", m, vec![f[0]]);
    let _ = sym;
    let mut cx = EditContext::new();
    cx.enter_path(&mut ed, &[a]).unwrap();
    assert!(!cx.is_root());
    assert_eq!(cx.frames().len(), 1);
    // Undo the open: the edit group is gone, and the context drops it on the next question.
    assert!(ed.undo());
    assert!(cx.validate(ed.doc()));
    assert!(cx.is_root());
    assert!(!cx.validate(ed.doc()));
    // A failing step leaves the context as it was.
    let plain = add_shape(
        &mut ed,
        Shape::Circle {
            center: glam::DVec2::ZERO,
            radius: 1.0,
        },
    );
    assert!(cx.enter_path(&mut ed, &[a, plain]).is_err());
    assert!(cx.is_root());
    assert!(ed.doc().object(a).is_some());
    assert!(!kadakas_core::symbols::is_being_edited(
        ed.doc().object(a).unwrap()
    ));
    // Leave all.
    cx.enter(&mut ed, a).unwrap();
    cx.exit_all(&mut ed).unwrap();
    assert!(cx.is_root());
    let err = cx.enter(&mut ed, plain).unwrap_err();
    assert!(err.to_string().contains("only a group or a component"));
}

#[test]
fn nested_contexts_write_back_in_order() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let f = faces(&ed, m);
    let (inner, i1) = make_component(&mut ed, "Inner", m, f);
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Outer".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![i1],
            replace: true,
        })
        .unwrap();
    let (outer, o1) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
    let mut cx = EditContext::new();
    cx.enter(&mut ed, o1).unwrap();
    let eg = cx.container().unwrap();
    // The nested instance is an object in the edit group.
    let nested = ed.doc().children(eg)[0];
    assert!(matches!(
        ed.doc().object(nested).unwrap().shape,
        Shape::Component { .. }
    ));
    cx.enter(&mut ed, nested).unwrap();
    assert_eq!(cx.path(), vec![o1, nested]);
    // The outer edit group cannot close while the inner one is open.
    invalid(
        ed.execute(&Command::CloseComponent {
            group: eg,
            resolve: None,
        }),
        "still open",
    );
    let raw = cx.raw_model(ed.doc()).unwrap();
    edit(&mut ed, raw, square(1000.0, 0.0, 1000.0));
    cx.exit_all(&mut ed).unwrap();
    let area = |s| -> f64 {
        component::leaves(&ed.doc().resources, s)
            .unwrap()
            .iter()
            .map(|l| l.model.area())
            .sum()
    };
    assert_eq!(area(inner), 2_000_000.0);
    assert_eq!(area(outer), 2_000_000.0);
    assert_eq!(component::definition_depth(&ed.doc().resources, outer), 2);
}

#[test]
fn the_edit_group_of_a_component_is_not_a_2d_symbol_edit() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let f = faces(&ed, m);
    let (_, a) = make_component(&mut ed, "Tile", m, f);
    let eg = ObjectId(
        ed.execute(&Command::EditComponent { id: a })
            .unwrap()
            .created[0],
    );
    invalid(
        ed.execute(&Command::ExitSymbolEdit { group: eg }),
        "close_component",
    );
    invalid(
        ed.execute(&Command::CloseComponent {
            group: a,
            resolve: None,
        }),
        "not the edit group",
    );
    invalid(
        ed.execute(&Command::Explode { ids: vec![eg] }),
        "open for edit",
    );
    invalid(
        ed.execute(&Command::Explode { ids: vec![a] }),
        "open for edit",
    );
    invalid(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![a],
            name: None,
        }),
        "open for edit",
    );
    invalid(ed.execute(&Command::EditSymbol { id: a }), "not a symbol");
    // Deleting the edit group shows the instance again.
    ed.execute(&Command::DeleteObjects { ids: vec![eg] })
        .unwrap();
    assert!(!kadakas_core::symbols::is_being_edited(
        ed.doc().object(a).unwrap()
    ));
}

#[test]
fn instances_move_turn_and_the_definition_is_protected() {
    let mut ed = fresh();
    let m = add_model(&mut ed);
    edit(&mut ed, m, square(0.0, 0.0, 1000.0));
    let f = faces(&ed, m);
    let (sym, a) = make_component(&mut ed, "Tile", m, f);
    ed.execute(&Command::MoveObjects {
        ids: vec![a],
        delta: glam::DVec2::new(10.0, 20.0),
    })
    .unwrap();
    ed.execute(&Command::RotateObjects {
        ids: vec![a],
        center: glam::DVec2::ZERO,
        angle: std::f64::consts::FRAC_PI_2,
    })
    .unwrap();
    let Shape::Component { transform, .. } = &ed.doc().object(a).unwrap().shape else {
        panic!()
    };
    let p = component::affine(transform).transform_point3(DVec3::new(1000.0, 0.0, 0.0));
    assert!(
        (p - DVec3::new(-20.0, 1010.0, 0.0)).length() < 1e-9,
        "{p:?}"
    );
    // The definition is in use.
    assert!(matches!(
        ed.execute(&Command::DeleteResource {
            id: sym,
            reassign: None
        }),
        Err(CommandError::ResourceInUse(_, 1))
    ));
    // A component instance of a resource that is not a component.
    let hatch = ed
        .doc()
        .resources
        .iter()
        .find(|r| r.id != sym)
        .map(|r| r.id);
    assert!(hatch.is_none());
    let bad = Shape::Component {
        symbol: ResourceId::from_num(4242),
        transform: component::raw(&DAffine3::IDENTITY),
    };
    assert!(matches!(
        ed.execute(&Command::AddObject {
            shape: bad,
            layer: None,
            class: None,
            attrs: Attributes::default()
        }),
        Err(CommandError::NoResource(_))
    ));
    // A transform that cannot be inverted.
    let flat = Shape::Component {
        symbol: sym,
        transform: component::raw(&DAffine3::from_scale(DVec3::new(1.0, 1.0, 0.0))),
    };
    invalid(
        ed.execute(&Command::AddObject {
            shape: flat,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        }),
        "inverted",
    );
}
