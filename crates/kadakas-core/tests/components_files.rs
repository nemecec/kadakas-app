//! Components in the file, in the checks, in the plan, the 3D scene, snapping and in the limits
//! (MOD-07). The measurement of the limits is `perf_limits`.

use std::time::Instant;

use glam::{DAffine3, DVec2, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::component::{self, ComponentError, Part};
use kadakas_core::graphics::{GraphicResource, SymbolDef};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::resource::{Resource, ResourceBody, ResourceId};
use kadakas_core::snapping::Geometry;
use kadakas_core::surface::{Edit, SurfaceModel};
use kadakas_core::svg;

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

fn square_edges(x: f64, y: f64, w: f64) -> Edit {
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

fn cube_edges(s: f64) -> Edit {
    let c = |i: usize| {
        [
            if i & 1 == 1 { s } else { 0.0 },
            if i & 2 == 2 { s } else { 0.0 },
            if i & 4 == 4 { s } else { 0.0 },
        ]
    };
    let mut edges = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                edges.push([c(i), c(i | bit)]);
            }
        }
    }
    Edit::AddEdges { edges }
}

fn model_with(ed: &mut Editor, e: Edit) -> ObjectId {
    let m = add_shape(ed, Shape::SurfaceModel(Box::default()));
    ed.execute(&Command::EditSurface { id: m, edit: e })
        .unwrap();
    m
}

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("not a surface model: {}", other.kind()),
    }
}

/// A definition with `n` empty models, pushed straight into the resources.
fn push_symbol(ed: &mut Editor, name: &str, parts: Vec<Part>) -> ResourceId {
    let mut doc = ed.doc().clone();
    let id = ResourceId(doc.alloc_id());
    doc.resources.push(Resource {
        id,
        name: name.into(),
        body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
            parts,
            ..SymbolDef::default()
        }))),
    });
    *ed = Editor::new(doc);
    id
}

fn inst(symbol: ResourceId) -> Part {
    Part::new(Shape::Component {
        symbol,
        transform: component::raw(&DAffine3::IDENTITY),
    })
}

fn empty_model() -> Part {
    Part::new(Shape::SurfaceModel(Box::default()))
}

fn invalid(r: Result<kadakas_core::command::Outcome, CommandError>, needle: &str) {
    match r {
        Err(CommandError::Invalid(m)) => assert!(m.contains(needle), "{m} should contain {needle}"),
        other => panic!("expected an error with {needle:?}, got {other:?}"),
    }
}

#[test]
fn a_cube_in_a_component_stays_a_closed_solid_through_explode() {
    let mut ed = fresh();
    let m = model_with(&mut ed, cube_edges(1000.0));
    assert_eq!(model(&ed, m).face_count(), 6);
    let all: Vec<_> = model(&ed, m).faces().map(|(f, _)| f).collect();
    // A whole model becomes a component.
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Box".into(),
            model: Some(m),
            faces: all,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap();
    let (sym, i) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
    assert!(ed.doc().object(m).is_none());
    // Two more instances, side by side; explode them one after the other into one model.
    let b = ObjectId(
        ed.execute(&Command::CopyObjects {
            ids: vec![i],
            delta: DVec2::new(1000.0, 0.0),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::Explode { ids: vec![i, b] }).unwrap();
    assert!(ed.doc().resources.get(sym).is_none());
    let models: Vec<ObjectId> = ed
        .doc()
        .objects
        .values()
        .filter(|o| matches!(o.shape, Shape::SurfaceModel(_)))
        .map(|o| o.id)
        .collect();
    assert_eq!(models.len(), 1);
    // Two cubes that touch at a face: the shared face is two coplanar faces of two solids.
    let merged = model(&ed, models[0]);
    assert!(merged.problem().is_none());
    assert!(merged.check().problems.is_empty());
    // The face that the two cubes share exists once.
    assert_eq!(merged.face_count(), 11);
    assert_eq!(merged.area(), 11.0 * 1_000_000.0);
}

#[test]
fn the_file_keeps_groups_instances_and_definitions() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square_edges(0.0, 0.0, 1000.0));
    let n = model_with(&mut ed, square_edges(3000.0, 0.0, 1000.0));
    let f: Vec<_> = model(&ed, m).faces().map(|(f, _)| f).collect();
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Tile".into(),
            model: Some(m),
            faces: f,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap();
    let (sym, a) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
    ed.execute(&Command::MoveObjects {
        ids: vec![a],
        delta: DVec2::new(1.5, 2.5),
    })
    .unwrap();
    ed.execute(&Command::MakeGroup {
        model: None,
        faces: vec![],
        edges: vec![],
        objects: vec![a, n],
        name: Some("Yard".into()),
    })
    .unwrap();
    let mut text = Vec::new();
    ed.doc().write_pretty(&mut text).unwrap();
    let json = String::from_utf8(text.clone()).unwrap();
    assert!(json.contains("\"kind\": \"component\""));
    assert!(json.contains("\"parts\""));
    let back = Document::from_slice(&text).unwrap();
    assert_eq!(&back, ed.doc());
    assert_eq!(back.resources.symbol(sym).unwrap().parts.len(), 1);
    // The saved text of a document is the same after one more round.
    let mut again = Vec::new();
    back.write_pretty(&mut again).unwrap();
    assert_eq!(again, text);
    // A part that is not 3D content is refused when the file is read.
    let mut v: serde_json::Value = serde_json::from_slice(&text).unwrap();
    let res = v["resources"].as_array_mut().unwrap();
    let body = res.iter_mut().find(|r| r["name"] == "Tile").unwrap();
    let parts = body["body"]["items"].clone();
    let _ = parts;
    let target = body.pointer_mut("/body").unwrap();
    let key = if target.get("parts").is_some() {
        "parts"
    } else {
        "symbol"
    };
    assert_eq!(key, "parts");
    target["parts"][0]["shape"] =
        serde_json::json!({"kind": "circle", "center": [0, 0], "radius": 5});
    assert!(Document::from_slice(&serde_json::to_vec(&v).unwrap()).is_err());
}

#[test]
fn definitions_are_checked() {
    let mut ed = fresh();
    let try_add = |ed: &mut Editor, parts: Vec<Part>| {
        ed.execute(&Command::AddResource {
            name: format!("S{}", ed.doc().resources.iter().count()),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                parts,
                ..SymbolDef::default()
            }))),
        })
    };
    // Good: a model, a group with a model in it.
    let group = Part::new(Shape::Group { name: "g".into() });
    let mut in_group = empty_model();
    in_group.parent = Some(0);
    try_add(&mut ed, vec![group.clone(), in_group.clone()]).unwrap();
    // Not 3D content.
    invalid(
        try_add(
            &mut ed,
            vec![Part::new(Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0,
            })],
        ),
        "A component holds",
    );
    // A parent that is not before or not a group.
    let mut bad = empty_model();
    bad.parent = Some(0);
    invalid(
        try_add(&mut ed, vec![bad.clone()]),
        "inside a group that comes before",
    );
    invalid(
        try_add(&mut ed, vec![empty_model(), bad]),
        "inside a group that comes before",
    );
    // Groups too deep.
    let mut chain = vec![group.clone()];
    for i in 1..=component::MAX_GROUP_DEPTH {
        let mut g = group.clone();
        g.parent = Some(i as u32 - 1);
        chain.push(g);
    }
    invalid(try_add(&mut ed, chain), "nested at most");
    // Too many parts.
    invalid(
        try_add(
            &mut ed,
            (0..=component::MAX_PARTS).map(|_| empty_model()).collect(),
        ),
        "at most",
    );
    // A group name that is too long, a transform that cannot be inverted, a model with a problem.
    invalid(
        try_add(
            &mut ed,
            vec![Part::new(Shape::Group {
                name: "x".repeat(201),
            })],
        ),
        "200 characters",
    );
    invalid(
        try_add(
            &mut ed,
            vec![Part::new(Shape::Component {
                symbol: ResourceId::from_num(1),
                transform: [0.0; 12],
            })],
        ),
        "inverted",
    );
    let broken: SurfaceModel = serde_json::from_value(
        serde_json::json!({"v": 1, "next_id": 5, "vertices": [[1, 0, 0, 0]]}),
    )
    .unwrap();
    invalid(
        try_add(
            &mut ed,
            vec![Part::new(Shape::SurfaceModel(Box::new(broken)))],
        ),
        "has no edge",
    );
    let mut far = SurfaceModel::new();
    far.place(DAffine3::from_translation(DVec3::new(2e9, 0.0, 0.0)));
    invalid(
        try_add(&mut ed, vec![Part::new(Shape::SurfaceModel(Box::new(far)))]),
        "not valid",
    );
    // Attributes that are not finite.
    let mut nan = empty_model();
    nan.attrs.line_weight = Some(f64::NAN);
    invalid(try_add(&mut ed, vec![nan]), "not valid");
    // An instance of something that does not exist.
    assert!(try_add(&mut ed, vec![inst(ResourceId::from_num(9999))]).is_err());
    // Many inner parts are fine at the limit.
    let mut many: Vec<Part> = vec![group.clone()];
    many.extend((0..100).map(|_| {
        let mut p = empty_model();
        p.parent = Some(0);
        p
    }));
    try_add(&mut ed, many).unwrap();
}

#[test]
fn expansion_reports_what_is_wrong() {
    let mut ed = fresh();
    let a = push_symbol(&mut ed, "A", vec![empty_model()]);
    let missing = ResourceId::from_num(31337);
    assert_eq!(
        component::leaves(&ed.doc().resources, missing).unwrap_err(),
        ComponentError::Missing(missing)
    );
    let leaves = component::leaves(&ed.doc().resources, a).unwrap();
    assert_eq!(leaves.len(), 1);
    assert!(component::instance_bounds(&ed.doc().resources, a, &DAffine3::IDENTITY).is_none());
    assert!(
        component::instance_bounds(&ed.doc().resources, missing, &DAffine3::IDENTITY).is_none()
    );
    // A cycle.
    let mut doc = ed.doc().clone();
    let b = ResourceId(doc.alloc_id());
    let c = ResourceId(doc.alloc_id());
    for (id, name, to) in [(b, "B", c), (c, "C", b)] {
        doc.resources.push(Resource {
            id,
            name: name.into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                parts: vec![inst(to)],
                ..SymbolDef::default()
            }))),
        });
    }
    assert!(matches!(
        component::leaves(&doc.resources, b),
        Err(ComponentError::Cycle(_))
    ));
    assert!(component::definition_depth(&doc.resources, b) >= 2);
    // A chain that is too deep.
    let mut ed2 = fresh();
    let mut last = push_symbol(&mut ed2, "L0", vec![empty_model()]);
    for i in 1..=component::MAX_COMPONENT_DEPTH + 1 {
        last = push_symbol(&mut ed2, &format!("L{i}"), vec![inst(last)]);
    }
    assert!(matches!(
        component::leaves(&ed2.doc().resources, last),
        Err(ComponentError::ComponentsTooDeep { .. })
    ));
    assert!(
        component::definition_depth(&ed2.doc().resources, last) > component::MAX_COMPONENT_DEPTH
    );
    // Too many models: three instances of a definition with the most that it may have, in one.
    let mut ed3 = fresh();
    let big = push_symbol(
        &mut ed3,
        "Big",
        (0..component::MAX_PARTS).map(|_| empty_model()).collect(),
    );
    let three = push_symbol(&mut ed3, "Three", vec![inst(big), inst(big), inst(big)]);
    assert_eq!(
        component::leaves(&ed3.doc().resources, big).unwrap().len(),
        component::MAX_PARTS
    );
    assert_eq!(
        component::leaves(&ed3.doc().resources, three).unwrap_err(),
        ComponentError::TooMany {
            max: component::MAX_LEAVES
        }
    );
    // The messages are plain.
    for e in [
        ComponentError::GroupsTooDeep { max: 3 },
        ComponentError::ComponentsTooDeep { max: 3 },
        ComponentError::TooMany { max: 3 },
        ComponentError::Cycle(a),
        ComponentError::Missing(a),
        ComponentError::NotAllowed("plant".into()),
        ComponentError::Invalid("x".into()),
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn only_surface_models_groups_and_components_can_be_parts() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square_edges(0.0, 0.0, 1000.0));
    let plant = add_shape(
        &mut ed,
        Shape::Circle {
            center: DVec2::ZERO,
            radius: 5.0,
        },
    );
    invalid(
        ed.execute(&Command::MakeComponent {
            name: "X".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![m, plant],
            replace: true,
        }),
        "circle cannot be part",
    );
    assert!(ed.doc().resources.is_empty());
    invalid(
        ed.execute(&Command::MakeComponent {
            name: "X".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![],
            replace: true,
        }),
        "choose",
    );
    invalid(
        ed.execute(&Command::MakeComponent {
            name: " ".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![m],
            replace: true,
        }),
        "needs a name",
    );
}

#[test]
fn a_group_in_a_component_survives_the_round_trip_of_edit_in_place() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square_edges(0.0, 0.0, 1000.0));
    let n = model_with(&mut ed, square_edges(3000.0, 0.0, 1000.0));
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![n],
            name: Some("inner".into()),
        })
        .unwrap()
        .created[0],
    );
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Pair".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![m, g],
            replace: true,
        })
        .unwrap();
    let (sym, i) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
    let def = ed.doc().resources.symbol(sym).unwrap().clone();
    assert_eq!(def.parts.len(), 3);
    assert!(
        matches!(def.parts[1].shape, Shape::Group { .. })
            || matches!(def.parts[0].shape, Shape::Group { .. })
    );
    // Open and close without a change: the definition is the same.
    let eg = ObjectId(
        ed.execute(&Command::EditComponent { id: i })
            .unwrap()
            .created[0],
    );
    assert_eq!(ed.doc().with_members(&[eg]).len(), 4);
    ed.execute(&Command::CloseComponent { group: eg }).unwrap();
    assert_eq!(ed.doc().resources.symbol(sym).unwrap(), &def);
    // Explode: the group comes back as a group with its model; the other model is the level's.
    ed.execute(&Command::Explode { ids: vec![i] }).unwrap();
    let groups = ed
        .doc()
        .objects
        .values()
        .filter(|o| matches!(o.shape, Shape::Group { .. }))
        .count();
    assert_eq!(groups, 1);
    assert_eq!(
        ed.doc()
            .objects
            .values()
            .filter(|o| matches!(o.shape, Shape::SurfaceModel(_)))
            .count(),
        2
    );
}

#[test]
fn a_moved_instance_keeps_its_definition_numbers_through_edit_in_place() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square_edges(0.0, 0.0, 1000.0));
    let f: Vec<_> = model(&ed, m).faces().map(|(f, _)| f).collect();
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Tile".into(),
            model: Some(m),
            faces: f,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap();
    let (sym, i) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
    ed.execute(&Command::MoveObjects {
        ids: vec![i],
        delta: DVec2::new(2500.0, 4000.0),
    })
    .unwrap();
    let before = ed.doc().resources.symbol(sym).unwrap().clone();
    let eg = ObjectId(
        ed.execute(&Command::EditComponent { id: i })
            .unwrap()
            .created[0],
    );
    ed.execute(&Command::CloseComponent { group: eg }).unwrap();
    assert_eq!(ed.doc().resources.symbol(sym).unwrap(), &before);
    // The edit group sits at the instance: its model is in the drawing.
    let eg = ObjectId(
        ed.execute(&Command::EditComponent { id: i })
            .unwrap()
            .created[0],
    );
    let inner = ed.doc().children(eg)[0];
    assert_eq!(
        model(&ed, inner).bounds().unwrap().0,
        DVec3::new(2500.0, 4000.0, 0.0)
    );
}

#[test]
fn the_plan_snapping_and_the_3d_scene_show_instances() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square_edges(0.0, 0.0, 1000.0));
    let f: Vec<_> = model(&ed, m).faces().map(|(f, _)| f).collect();
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Tile".into(),
            model: Some(m),
            faces: f,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap();
    let (sym, i) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
    let svg_text = svg::render(ed.doc(), 800);
    assert!(svg_text.contains("data-kind=\"component\""));
    let mut g = Geometry::default();
    g.add_shape(&ed.doc().object(i).unwrap().shape);
    assert!(g.points.iter().any(|p| p.p == DVec2::ZERO));
    let scene = mesh::build(ed.doc());
    assert!(scene.mesh.positions.len() >= 4);
    // Mirrored.
    let Shape::Component { transform, .. } = ed.doc().object(i).unwrap().shape.clone() else {
        panic!()
    };
    let flipped = component::raw(
        &(DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)) * component::affine(&transform)),
    );
    ed.execute(&Command::ModifyObject {
        id: i,
        shape: Some(Shape::Component {
            symbol: sym,
            transform: flipped,
        }),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    assert!(mesh::build(ed.doc()).mesh.positions.len() >= 4);
    // An instance of an empty definition draws nothing.
    let mut e2 = fresh();
    let empty = push_symbol(&mut e2, "E", vec![empty_model()]);
    let eid = add_shape(
        &mut e2,
        Shape::Component {
            symbol: empty,
            transform: component::raw(&DAffine3::IDENTITY),
        },
    );
    assert!(!svg::render(e2.doc(), 800).contains("data-kind=\"component\""));
    assert!(mesh::build(e2.doc()).mesh.positions.is_empty());
    assert_eq!(
        e2.doc().object_bounds(e2.doc().object(eid).unwrap()),
        (DVec2::ZERO, DVec2::ZERO)
    );
    // The shape on its own.
    let s = Shape::Component {
        symbol: sym,
        transform: component::raw(&DAffine3::from_translation(DVec3::new(1.0, 2.0, 3.0))),
    };
    assert_eq!(s.kind(), "component");
    assert_eq!(s.bounds(), (DVec2::new(1.0, 2.0), DVec2::new(1.0, 2.0)));
    let mut t = s.clone();
    t.translate3(DVec3::new(10.0, 10.0, 10.0));
    assert_eq!(t.bounds().0, DVec2::new(11.0, 12.0));
    let Shape::Component { transform, .. } = &t else {
        panic!()
    };
    assert_eq!(transform[11], 13.0);
    assert!(s.is_finite() && s.is_in_range());
    assert!(
        Shape::Component {
            symbol: sym,
            transform: [f64::NAN; 12]
        }
        .problem()
        .is_some()
    );
    assert!(
        !Shape::Component {
            symbol: sym,
            transform: [f64::NAN; 12]
        }
        .is_finite()
    );
}

#[test]
fn scale_and_mirror_in_the_plan_apply_to_instances() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square_edges(0.0, 0.0, 1000.0));
    let f: Vec<_> = model(&ed, m).faces().map(|(f, _)| f).collect();
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Tile".into(),
            model: Some(m),
            faces: f,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap();
    let i = ObjectId(out.created[1]);
    let t = kadakas_core::edit2d::Transform::Scale {
        center: DVec2::new(100.0, 0.0),
        sx: 2.0,
        sy: None,
    };
    ed.execute(&Command::TransformObjects {
        ids: vec![i],
        transform: t,
        copy: false,
    })
    .unwrap();
    let Shape::Component { transform, .. } = &ed.doc().object(i).unwrap().shape else {
        panic!()
    };
    let p = component::affine(transform).transform_point3(DVec3::new(1000.0, 0.0, 500.0));
    // About (100, 0) by 2: x -> 2 (x - 100) + 100, and the heights scale too.
    assert!(
        (p - DVec3::new(1900.0, 0.0, 1000.0)).length() < 1e-9,
        "{p:?}"
    );
}

#[test]
fn deleting_and_reassigning_a_definition() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square_edges(0.0, 0.0, 1000.0));
    let f: Vec<_> = model(&ed, m).faces().map(|(f, _)| f).collect();
    let (s1, i) = {
        let out = ed
            .execute(&Command::MakeComponent {
                name: "A".into(),
                model: Some(m),
                faces: f,
                edges: vec![],
                objects: vec![],
                replace: true,
            })
            .unwrap();
        (ResourceId(out.created[0]), ObjectId(out.created[1]))
    };
    let n = model_with(&mut ed, square_edges(0.0, 5000.0, 1000.0));
    let f: Vec<_> = model(&ed, n).faces().map(|(f, _)| f).collect();
    let s2 = ResourceId(
        ed.execute(&Command::MakeComponent {
            name: "B".into(),
            model: Some(n),
            faces: f,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap()
        .created[0],
    );
    assert_eq!(kadakas_core::symbols::instance_ids(ed.doc(), s1), vec![i]);
    assert!(kadakas_core::symbols::is_used_by_objects(ed.doc(), s1));
    assert_eq!(kadakas_core::library::usage(ed.doc(), s1).objects, vec![i]);
    ed.execute(&Command::DeleteResource {
        id: s1,
        reassign: Some(s2),
    })
    .unwrap();
    let Shape::Component { symbol, .. } = &ed.doc().object(i).unwrap().shape else {
        panic!()
    };
    assert_eq!(*symbol, s2);
    assert!(ed.doc().resources.get(s1).is_none());
}

/// The measurement behind the limits of nesting (see `component`): the cost of a walk of a deep
/// chain of groups and of components, and of a wide expansion. The limits are far below the cost
/// that a person would notice, and far above what a model needs.
#[test]
fn perf_limits() {
    // Groups: the walk of a chain of MAX_GROUP_DEPTH groups.
    let mut ed = fresh();
    let m = model_with(&mut ed, square_edges(0.0, 0.0, 1000.0));
    let mut current = m;
    let t = Instant::now();
    for _ in 0..component::MAX_GROUP_DEPTH {
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
    let make = t.elapsed();
    let t = Instant::now();
    ed.execute(&Command::MoveObjects {
        ids: vec![current],
        delta: DVec2::new(1.0, 1.0),
    })
    .unwrap();
    let mv = t.elapsed();
    let t = Instant::now();
    let chain = ed.doc().ancestors(m);
    let anc = t.elapsed();
    assert_eq!(chain.len(), component::MAX_GROUP_DEPTH);
    // Components: a chain of MAX_COMPONENT_DEPTH definitions, and the expansion of the top one.
    let mut ed2 = fresh();
    let mut last = push_symbol(&mut ed2, "L0", vec![empty_model()]);
    for i in 1..component::MAX_COMPONENT_DEPTH {
        last = push_symbol(&mut ed2, &format!("L{i}"), vec![inst(last)]);
    }
    let t = Instant::now();
    let leaves = component::leaves(&ed2.doc().resources, last).unwrap();
    let chain_walk = t.elapsed();
    assert_eq!(leaves.len(), 1);
    // The widest expansion: MAX_LEAVES models.
    let mut ed3 = fresh();
    let big = push_symbol(
        &mut ed3,
        "Big",
        (0..component::MAX_PARTS).map(|_| empty_model()).collect(),
    );
    let two = push_symbol(&mut ed3, "Two", vec![inst(big), inst(big)]);
    let t = Instant::now();
    let wide = component::leaves(&ed3.doc().resources, two).unwrap();
    let wide_walk = t.elapsed();
    assert_eq!(wide.len(), component::MAX_LEAVES);
    eprintln!(
        "perf_limits (debug build): {} groups make {:?}, move {:?}, ancestors {:?}; {} components deep walk {:?}; {} leaves walk {:?}",
        component::MAX_GROUP_DEPTH,
        make,
        mv,
        anc,
        component::MAX_COMPONENT_DEPTH,
        chain_walk,
        wide.len(),
        wide_walk
    );
    // Generous bounds for a debug build on a slow machine; the numbers are in the documentation.
    assert!(
        make.as_secs_f64() < 5.0
            && mv.as_secs_f64() < 1.0
            && chain_walk.as_secs_f64() < 1.0
            && wide_walk.as_secs_f64() < 2.0
    );
}
