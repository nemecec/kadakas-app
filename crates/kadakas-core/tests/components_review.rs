//! MOD-07 review items: a broken instance is marked and reported, the kind of a definition
//! matches its users, the limits hold for resources and files, group depth, explode keeps the
//! look of a model, two users on one definition, the origin of a definition made from objects.

use glam::{DAffine3, DVec2, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::component::{self, EditContext, PlanMark, Resolve};
use kadakas_core::graphics::{GraphicResource, SymbolDef};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::resource::{Resource, ResourceBody, ResourceId};
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

fn model_with(ed: &mut Editor, e: Edit) -> ObjectId {
    let m = add_shape(ed, Shape::SurfaceModel(Box::default()));
    ed.execute(&Command::EditSurface { id: m, edit: e })
        .unwrap();
    m
}

fn model_at(ed: &mut Editor, x: f64, attrs: Attributes) -> ObjectId {
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs,
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::EditSurface {
        id,
        edit: square(x, 0.0, 1000.0),
    })
    .unwrap();
    id
}

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("not a surface model: {}", other.kind()),
    }
}

fn symbol_body(def: SymbolDef) -> ResourceBody {
    ResourceBody::Graphic(Box::new(GraphicResource::Symbol(def)))
}

fn parts_def(parts: Vec<component::Part>) -> SymbolDef {
    SymbolDef {
        parts,
        ..SymbolDef::default()
    }
}

fn empty_model() -> component::Part {
    component::Part::new(Shape::SurfaceModel(Box::default()))
}

fn inst(symbol: ResourceId) -> component::Part {
    component::Part::new(Shape::Component {
        symbol,
        transform: component::raw(&DAffine3::IDENTITY),
    })
}

fn add_symbol(ed: &mut Editor, name: &str, def: SymbolDef) -> Result<ResourceId, CommandError> {
    ed.execute(&Command::AddResource {
        name: name.into(),
        body: symbol_body(def),
    })
    .map(|o| ResourceId(o.created[0]))
}

/// A resource pushed straight into the document (as a file would bring it).
fn push_symbol(doc: &mut Document, name: &str, def: SymbolDef) -> ResourceId {
    let id = ResourceId(doc.alloc_id());
    doc.resources.push(Resource {
        id,
        name: name.into(),
        body: symbol_body(def),
    });
    id
}

fn push_instance(doc: &mut Document, symbol: ResourceId) -> ObjectId {
    let id = ObjectId(doc.alloc_id());
    let layer = doc.active_layer;
    let class = doc.active_class;
    doc.objects.insert(
        id,
        Object::new(
            id,
            layer,
            class,
            Shape::Component {
                symbol,
                transform: component::raw(&DAffine3::IDENTITY),
            },
        ),
    );
    id
}

fn invalid<T: std::fmt::Debug>(r: Result<T, CommandError>, needle: &str) {
    match r {
        Err(CommandError::Invalid(m)) => assert!(m.contains(needle), "{m} should contain {needle}"),
        other => panic!("expected an error with {needle:?}, got {other:?}"),
    }
}

/// A component of one square with an instance.
fn tile(ed: &mut Editor) -> (ResourceId, ObjectId) {
    let m = model_with(ed, square(0.0, 0.0, 1000.0));
    let f: Vec<_> = model(ed, m).faces().map(|(f, _)| f).collect();
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
    (ResourceId(out.created[0]), ObjectId(out.created[1]))
}

// ---------------------------------------------------------------- 2: a broken instance

#[test]
fn an_instance_that_cannot_be_expanded_is_marked_and_reported_everywhere() {
    let mut doc = Editor::with_sequential_ids().doc().clone();
    // A missing definition, and a cycle.
    let gone = push_instance(&mut doc, ResourceId::from_num(31337));
    let b = ResourceId(doc.alloc_id());
    let c = ResourceId(doc.alloc_id());
    for (id, name, to) in [(b, "B", c), (c, "C", b)] {
        doc.resources.push(Resource {
            id,
            name: name.into(),
            body: symbol_body(parts_def(vec![inst(to)])),
        });
    }
    let looped = push_instance(&mut doc, b);
    let found = component::findings(&doc);
    assert!(found.iter().any(|f| f.object == gone), "{found:?}");
    assert!(found.iter().any(|f| f.object == looped), "{found:?}");
    // The plan, the SVG and the 3D scene draw a mark, not nothing.
    for id in [gone, looped] {
        let shape = &doc.object(id).unwrap().shape;
        assert!(matches!(
            component::plan_mark(&doc, shape),
            Some(PlanMark::Broken(_, why)) if !why.is_empty()
        ));
    }
    let svg_text = svg::render(&doc, 800);
    assert_eq!(svg_text.matches("data-problem=").count(), 2, "{svg_text}");
    let scene = mesh::build(&doc);
    assert_eq!(scene.lines.segment_count(), 6, "three axes for each");
    assert!(
        scene
            .lines
            .colors
            .iter()
            .all(|c| *c == component::WARNING_COLOR)
    );
    // A file with a missing definition loads, and says what is wrong. (A file with a cycle
    // between definitions is refused by the file check.)
    let mut only = Editor::with_sequential_ids().doc().clone();
    let gone = push_instance(&mut only, ResourceId::from_num(31337));
    let back = Document::from_json(&serde_json::to_string(&only).unwrap()).unwrap();
    let notes = back.load_notes().join("\n");
    assert!(notes.contains(&format!("object {gone}")), "{notes}");
    assert!(back.objects.contains_key(&gone), "nothing is dropped");
}

#[test]
fn a_part_with_a_look_of_its_own_keeps_it_in_the_scene() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square(0.0, 0.0, 1000.0));
    let red = Color([200, 10, 10, 255]);
    ed.execute(&Command::ModifyObject {
        id: m,
        shape: None,
        layer: None,
        class: None,
        attrs: Some(Attributes {
            fill: Some(red),
            ..Attributes::default()
        }),
    })
    .unwrap();
    let f: Vec<_> = model(&ed, m).faces().map(|(f, _)| f).collect();
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Red".into(),
            model: Some(m),
            faces: f,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap();
    let i = ObjectId(out.created[1]);
    // The instance has a fill of its own; the part still shows its own.
    ed.execute(&Command::ModifyObject {
        id: i,
        shape: None,
        layer: None,
        class: None,
        attrs: Some(Attributes {
            fill: Some(Color([10, 10, 200, 255])),
            ..Attributes::default()
        }),
    })
    .unwrap();
    let scene = mesh::build(ed.doc());
    assert!(!scene.mesh.colors.is_empty());
    assert!(
        scene.mesh.colors.iter().all(|c| c[..3] == [200, 10, 10]),
        "{:?}",
        scene.mesh.colors
    );
}

// ---------------------------------------------------------------- 3: kinds

#[test]
fn a_component_instance_needs_a_component_and_a_symbol_instance_a_2d_symbol() {
    let mut ed = fresh();
    let (comp, _) = tile(&mut ed);
    let flat = add_symbol(&mut ed, "Flat", SymbolDef::default()).unwrap();
    let component_of = |symbol| Shape::Component {
        symbol,
        transform: component::raw(&DAffine3::IDENTITY),
    };
    let symbol_of = |symbol| Shape::Symbol {
        symbol,
        at: DVec2::ZERO,
        rotation: 0.0,
        scale: 1.0,
        scale_y: None,
    };
    let add = |ed: &mut Editor, shape| {
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
    };
    invalid(add(&mut ed, component_of(flat)), "not a component");
    invalid(add(&mut ed, symbol_of(comp)), "3D component");
    // A component that has instances keeps its 3D parts.
    let body = symbol_body(SymbolDef::default());
    invalid(
        ed.execute(&Command::ModifyResource {
            id: comp,
            name: None,
            body: Some(body),
        }),
        "used as a component",
    );
    // A file that already has the mismatch: the instance stays, marked, with a finding.
    let mut doc = ed.doc().clone();
    let bad = push_instance(&mut doc, flat);
    let notes = component::findings(&doc);
    assert!(
        notes
            .iter()
            .any(|f| f.object == bad && f.message.contains("not a component")),
        "{notes:?}"
    );
    assert!(svg::render(&doc, 800).contains("data-problem"));
}

#[test]
fn a_mixed_2d_and_3d_symbol_serves_both_kinds_of_instance() {
    let mut ed = fresh();
    let mut def = parts_def(vec![empty_model()]);
    def.items.push(kadakas_core::graphics::SymbolItem {
        shape: Shape::Circle {
            center: DVec2::ZERO,
            radius: 100.0,
        },
        attrs: Attributes::default(),
        class: None,
    });
    let mixed = add_symbol(&mut ed, "Mixed", def.clone()).unwrap();
    let add = |ed: &mut Editor, shape| {
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
    };
    add(
        &mut ed,
        Shape::Symbol {
            symbol: mixed,
            at: DVec2::ZERO,
            rotation: 0.0,
            scale: 1.0,
            scale_y: None,
        },
    )
    .unwrap();
    add(
        &mut ed,
        Shape::Component {
            symbol: mixed,
            transform: component::raw(&DAffine3::IDENTITY),
        },
    )
    .unwrap();
    let change = |ed: &mut Editor, def: SymbolDef| {
        ed.execute(&Command::ModifyResource {
            id: mixed,
            name: None,
            body: Some(symbol_body(def)),
        })
    };
    // Losing the 3D side or the 2D side is refused while the other kind of instance exists.
    invalid(
        change(
            &mut ed,
            SymbolDef {
                parts: vec![],
                ..def.clone()
            },
        ),
        "used as a component",
    );
    invalid(
        change(
            &mut ed,
            SymbolDef {
                items: vec![],
                ..def.clone()
            },
        ),
        "used as a symbol",
    );
    assert!(component::findings(ed.doc()).is_empty());
}

// ---------------------------------------------------------------- 4: limits

#[test]
fn resources_are_held_to_the_limits_when_they_are_added_and_changed() {
    let mut ed = fresh();
    let big = add_symbol(
        &mut ed,
        "Big",
        parts_def((0..component::MAX_PARTS).map(|_| empty_model()).collect()),
    )
    .unwrap();
    // 3 x 10,000 models is over 20,000.
    invalid(
        add_symbol(
            &mut ed,
            "Three",
            parts_def(vec![inst(big), inst(big), inst(big)]),
        ),
        "at most",
    );
    assert!(ed.doc().resources.iter().all(|r| r.name != "Three"));
    // A nested definition that grows after the outer one was made: the change is refused.
    let small = add_symbol(&mut ed, "Small", parts_def(vec![empty_model()])).unwrap();
    let outer = add_symbol(
        &mut ed,
        "Outer",
        parts_def(vec![inst(small), inst(small), inst(small)]),
    )
    .unwrap();
    let before = ed.doc().clone();
    let r = ed.execute(&Command::ModifyResource {
        id: small,
        name: None,
        body: Some(symbol_body(parts_def(
            (0..component::MAX_PARTS).map(|_| empty_model()).collect(),
        ))),
    });
    invalid(r, "Outer");
    assert_eq!(ed.doc(), &before, "nothing changed");
    assert!(component::leaves(&ed.doc().resources, outer).is_ok());
    // Nesting: a chain of MAX_COMPONENT_DEPTH is fine, one more is refused.
    let mut last = add_symbol(&mut ed, "L1", parts_def(vec![empty_model()])).unwrap();
    for i in 2..=component::MAX_COMPONENT_DEPTH {
        last = add_symbol(&mut ed, &format!("L{i}"), parts_def(vec![inst(last)])).unwrap();
    }
    invalid(
        add_symbol(&mut ed, "Deeper", parts_def(vec![inst(last)])),
        "nested at most",
    );
}

#[test]
fn a_file_with_a_definition_over_the_limits_loads_with_a_finding() {
    let mut ed = fresh();
    let mut doc = ed.doc().clone();
    let big = push_symbol(
        &mut doc,
        "Big",
        parts_def((0..component::MAX_PARTS).map(|_| empty_model()).collect()),
    );
    let three = push_symbol(
        &mut doc,
        "Three",
        parts_def(vec![inst(big), inst(big), inst(big)]),
    );
    let i = push_instance(&mut doc, three);
    let text = serde_json::to_string(&doc).unwrap();
    let back = Document::from_json(&text).unwrap();
    let notes = back.load_notes().join("\n");
    assert!(notes.contains("'Three'"), "{notes}");
    assert!(back.objects.contains_key(&i));
    assert!(matches!(
        component::plan_mark(&back, &back.object(i).unwrap().shape),
        Some(PlanMark::Broken(..))
    ));
    ed = Editor::new(back);
    assert!(!component::findings(ed.doc()).is_empty());
}

// ---------------------------------------------------------------- 5: group depth

/// `levels` groups in each other: returns the outermost and the innermost.
fn nested_groups(ed: &mut Editor, levels: usize) -> (ObjectId, ObjectId) {
    let mut innermost = None;
    let mut top = add_shape(
        ed,
        Shape::Circle {
            center: DVec2::ZERO,
            radius: 5.0,
        },
    );
    for _ in 0..levels {
        top = ObjectId(
            ed.execute(&Command::Group {
                ids: vec![top],
                name: None,
            })
            .unwrap()
            .created[0],
        );
        innermost.get_or_insert(top);
    }
    (top, innermost.unwrap())
}

#[test]
fn the_2d_group_command_stops_at_the_depth_limit() {
    let mut ed = fresh();
    let (top, _) = nested_groups(&mut ed, component::MAX_GROUP_DEPTH);
    let before = ed.doc().clone();
    invalid(
        ed.execute(&Command::Group {
            ids: vec![top],
            name: None,
        }),
        "nested at most",
    );
    assert_eq!(ed.doc(), &before);
}

#[test]
fn edit_and_explode_respect_the_group_depth_limit() {
    let mut ed = fresh();
    let (_, inner) = nested_groups(&mut ed, component::MAX_GROUP_DEPTH);
    let mut g = component::Part::new(Shape::Group { name: "g".into() });
    g.parent = None;
    let mut m = empty_model();
    m.parent = Some(0);
    let sym = add_symbol(&mut ed, "G", parts_def(vec![g, m])).unwrap();
    let i = ObjectId(
        ed.execute(&Command::AddObjectIn {
            parent: inner,
            shape: Shape::Component {
                symbol: sym,
                transform: component::raw(&DAffine3::IDENTITY),
            },
            class: None,
            attrs: Attributes::default(),
            id: None,
        })
        .unwrap()
        .created[0],
    );
    let before = ed.doc().clone();
    let r = ed.execute(&Command::EditComponent { id: i });
    assert!(r.is_err(), "edit: {r:?}");
    let r = ed.execute(&Command::Explode { ids: vec![i] });
    assert!(r.is_err(), "explode: {r:?}");
    assert_eq!(ed.doc(), &before, "nothing changed");
}

// ---------------------------------------------------------------- 6: explode keeps the look

#[test]
fn explode_keeps_the_fill_of_a_model_that_has_one() {
    let mut ed = fresh();
    let base = model_at(&mut ed, 0.0, Attributes::default());
    let red = Attributes {
        fill: Some(Color([200, 10, 10, 255])),
        ..Attributes::default()
    };
    let painted = model_at(&mut ed, 3000.0, red.clone());
    let plain = model_at(&mut ed, 6000.0, Attributes::default());
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![painted, plain],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::Explode { ids: vec![g] }).unwrap();
    let models: Vec<&Object> = ed
        .doc()
        .objects
        .values()
        .filter(|o| matches!(o.shape, Shape::SurfaceModel(_)))
        .collect();
    // The plain one merged into the base; the painted one stayed apart, still red.
    assert_eq!(models.len(), 2, "{models:?}");
    assert!(
        models
            .iter()
            .any(|o| o.id == base && o.attrs.fill.is_none())
    );
    assert!(models.iter().any(|o| o.attrs.fill == red.fill));
    let merged = model(&ed, base);
    assert_eq!(merged.face_count(), 2);
    assert!(ed.doc().object(plain).is_none() || ed.doc().object(plain).unwrap().parent.is_none());
}

// ---------------------------------------------------------------- 7: two users, one definition

#[test]
fn a_definition_changed_during_an_edit_is_a_conflict_the_person_resolves() {
    let setup = || {
        let mut ed = fresh();
        let (sym, i) = tile(&mut ed);
        let g = ObjectId(
            ed.execute(&Command::EditComponent { id: i })
                .unwrap()
                .created[0],
        );
        // Somebody else changes the definition now.
        let mut other = parts_def(vec![empty_model(), empty_model()]);
        other.parts[0] = ed.doc().resources.symbol(sym).unwrap().parts[0].clone();
        ed.execute(&Command::ModifyResource {
            id: sym,
            name: None,
            body: Some(symbol_body(other)),
        })
        .unwrap();
        (ed, sym, i, g)
    };
    // No answer: refused, nothing changes.
    let (mut ed, sym, i0, g) = setup();
    let before = ed.doc().clone();
    invalid(
        ed.execute(&Command::CloseComponent {
            group: g,
            resolve: None,
        }),
        "conflict",
    );
    assert_eq!(ed.doc(), &before);
    // Keep mine as a new component: the instance uses it, the other version stays.
    let out = ed
        .execute(&Command::CloseComponent {
            group: g,
            resolve: Some(Resolve::KeepAsNew),
        })
        .unwrap();
    let mine = ResourceId(*out.created.last().unwrap());
    assert_ne!(mine, sym);
    assert_eq!(ed.doc().resources.get(mine).unwrap().name, "Tile#1");
    // The new definition holds my edit (the one square), the old one the other change.
    let mine_parts = &ed.doc().resources.symbol(mine).unwrap().parts;
    assert_eq!(mine_parts.len(), 1);
    assert!(
        matches!(&mine_parts[0].shape, Shape::SurfaceModel(m) if m.face_count() == 1),
        "{mine_parts:?}"
    );
    assert_eq!(ed.doc().resources.symbol(sym).unwrap().parts.len(), 2);
    assert!(matches!(
        &ed.doc().object(i0).unwrap().shape,
        Shape::Component { symbol, .. } if *symbol == mine
    ));
    assert!(ed.doc().object(g).is_none());
    // Discard: the other version stays and the edit is gone.
    let (mut ed, sym, i, g) = setup();
    ed.execute(&Command::CloseComponent {
        group: g,
        resolve: Some(Resolve::Discard),
    })
    .unwrap();
    assert_eq!(ed.doc().resources.symbol(sym).unwrap().parts.len(), 2);
    assert!(ed.doc().object(g).is_none());
    assert!(!kadakas_core::symbols::is_being_edited(
        ed.doc().object(i).unwrap()
    ));
    // Without a change by somebody else the close is as before.
    let mut ed = fresh();
    let (sym, i) = tile(&mut ed);
    let g = ObjectId(
        ed.execute(&Command::EditComponent { id: i })
            .unwrap()
            .created[0],
    );
    ed.execute(&Command::CloseComponent {
        group: g,
        resolve: None,
    })
    .unwrap();
    assert_eq!(ed.doc().resources.symbol(sym).unwrap().parts.len(), 1);
}

// ---------------------------------------------------------------- 10: a singular transform

#[test]
fn a_surface_model_that_cannot_be_inverted_has_no_local_coordinates() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square(0.0, 0.0, 1000.0));
    let ctx = EditContext::new();
    assert!(ctx.to_local(ed.doc(), DVec3::new(5.0, 5.0, 0.0)).is_some());
    let mut doc = ed.doc().clone();
    if let Shape::SurfaceModel(s) = &mut doc.object_mut(m).unwrap().shape {
        s.transform = DAffine3::from_scale(DVec3::new(1.0, 0.0, 1.0));
    }
    assert!(ctx.to_local(&doc, DVec3::new(5.0, 5.0, 0.0)).is_none());
}

// ---------------------------------------------------------------- 12: the origin

#[test]
fn a_component_made_from_objects_has_its_origin_at_the_corner_of_their_bounds() {
    let mut ed = fresh();
    let m = model_with(&mut ed, square(50_000.0, 60_000.0, 1000.0));
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Far".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![m],
            replace: true,
        })
        .unwrap();
    let (sym, i) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
    let Shape::Component { transform, .. } = &ed.doc().object(i).unwrap().shape else {
        panic!()
    };
    assert_eq!(&transform[9..], &[50_000.0, 60_000.0, 0.0]);
    // The instance is where the objects were.
    let (lo, hi) =
        component::instance_bounds(&ed.doc().resources, sym, &component::affine(transform))
            .unwrap();
    assert!(
        lo.distance(DVec3::new(50_000.0, 60_000.0, 0.0)) < 1e-6,
        "{lo}"
    );
    assert!(
        hi.distance(DVec3::new(51_000.0, 61_000.0, 0.0)) < 1e-6,
        "{hi}"
    );
    // The part of the definition is placed by a move back to the corner.
    let Shape::SurfaceModel(part) = &ed.doc().resources.symbol(sym).unwrap().parts[0].shape else {
        panic!()
    };
    assert!(
        part.transform
            .translation
            .distance(DVec3::new(-50_000.0, -60_000.0, 0.0))
            < 1e-6
    );
}

// ---------------------------------------------------------------- 11: object info

#[test]
fn object_info_gives_name_position_rotation_and_scale() {
    let mut ed = fresh();
    let (sym, i) = tile(&mut ed);
    let xf = DAffine3::from_translation(DVec3::new(1.0, 2.0, 3.0))
        * DAffine3::from_rotation_z(90f64.to_radians())
        * DAffine3::from_scale(DVec3::new(2.0, 3.0, 4.0));
    let shape = Shape::Component {
        symbol: sym,
        transform: component::raw(&xf),
    };
    let info = component::instance_info(ed.doc(), &shape).unwrap();
    assert_eq!(info.name, "Tile");
    assert!(info.position.distance(DVec3::new(1.0, 2.0, 3.0)) < 1e-9);
    assert!((info.rotation_deg - 90.0).abs() < 1e-9);
    assert!(info.scale.distance(DVec3::new(2.0, 3.0, 4.0)) < 1e-9);
    assert!(component::instance_info(ed.doc(), &ed.doc().object(i).unwrap().shape).is_some());
}

// ---------------------------------------------------------------- 1: screen equals export

#[test]
fn the_plan_mark_is_the_box_that_the_svg_draws() {
    let mut ed = fresh();
    let (_, i) = tile(&mut ed);
    let Some(PlanMark::Box([lo, _, hi, _])) =
        component::plan_mark(ed.doc(), &ed.doc().object(i).unwrap().shape)
    else {
        panic!("a box")
    };
    assert_eq!((lo, hi), (DVec2::new(0.0, 0.0), DVec2::new(1000.0, 1000.0)));
    // The SVG draws the faces and edges of the parts (MOD-13). The box of that drawing is the
    // plan mark, which stays for bounds and picking.
    let Some(kadakas_core::surface::plan::Outcome::Drawing(d)) =
        kadakas_core::surface::plan::plan_of_object(ed.doc(), ed.doc().object(i).unwrap())
    else {
        panic!("a drawing")
    };
    assert_eq!(d.bounds(), Some((lo, hi)));
    let text = svg::render(ed.doc(), 800);
    assert!(
        text.contains(r#"data-kind="component""#)
            && text.contains(&format!(
                r#"data-faces="{}" data-edges="{}""#,
                d.faces(),
                d.edges()
            )),
        "{text}"
    );
}

// ---------------------------------------------------------------- review round 2

fn open_edit(ed: &mut Editor, i: ObjectId) -> ObjectId {
    ObjectId(
        ed.execute(&Command::EditComponent { id: i })
            .unwrap()
            .created[0],
    )
}

#[test]
fn emptying_a_component_and_closing_is_refused() {
    let mut ed = fresh();
    let (sym, i) = tile(&mut ed);
    // A second instance must not end up as a placeholder.
    let other = ObjectId(
        ed.execute(&Command::CopyObjects {
            ids: vec![i],
            delta: DVec2::new(5000.0, 0.0),
        })
        .unwrap()
        .created[0],
    );
    let g = open_edit(&mut ed, i);
    let kids = ed.doc().with_members(&[g]);
    ed.execute(&Command::DeleteObjects {
        ids: kids.into_iter().filter(|k| *k != g).collect(),
    })
    .unwrap();
    let before = ed.doc().clone();
    for resolve in [None, Some(Resolve::Overwrite), Some(Resolve::KeepAsNew)] {
        invalid(
            ed.execute(&Command::CloseComponent { group: g, resolve }),
            "would be empty",
        );
        assert_eq!(ed.doc(), &before);
    }
    assert_eq!(ed.doc().resources.symbol(sym).unwrap().parts.len(), 1);
    assert!(component::findings(ed.doc()).is_empty());
    // Discard is always possible and keeps the old content.
    ed.execute(&Command::CloseComponent {
        group: g,
        resolve: Some(Resolve::Discard),
    })
    .unwrap();
    assert!(component::findings(ed.doc()).is_empty());
    assert!(ed.doc().object(other).is_some());
}

#[test]
fn discard_throws_the_edit_away_also_without_a_conflict() {
    let mut ed = fresh();
    let (sym, i) = tile(&mut ed);
    let before = ed.doc().clone();
    let g = open_edit(&mut ed, i);
    // Draw something in the edit.
    let model = ed
        .doc()
        .children(g)
        .into_iter()
        .find(|c| matches!(ed.doc().object(*c).unwrap().shape, Shape::SurfaceModel(_)))
        .unwrap();
    ed.execute(&Command::EditSurface {
        id: model,
        edit: square(5000.0, 0.0, 1000.0),
    })
    .unwrap();
    ed.execute(&Command::CloseComponent {
        group: g,
        resolve: Some(Resolve::Discard),
    })
    .unwrap();
    assert_eq!(ed.doc().resources.symbol(sym).unwrap().parts.len(), 1);
    let Shape::SurfaceModel(m) = &ed.doc().resources.symbol(sym).unwrap().parts[0].shape else {
        panic!()
    };
    assert_eq!(m.face_count(), 1, "the drawing in the edit is gone");
    // The same document as before the edit.
    assert_eq!(ed.doc().resources, before.resources);
    assert_eq!(
        ed.doc().objects.keys().collect::<Vec<_>>(),
        before.objects.keys().collect::<Vec<_>>()
    );
    assert!(!kadakas_core::symbols::is_being_edited(
        ed.doc().object(i).unwrap()
    ));
}

#[test]
fn an_edit_without_a_saved_version_is_never_written_without_an_answer() {
    let mut ed = fresh();
    let (sym, i) = tile(&mut ed);
    let g = open_edit(&mut ed, i);
    // An edit group from a build before the version was kept.
    let mut doc = ed.doc().clone();
    doc.object_mut(g)
        .unwrap()
        .data
        .get_mut(kadakas_core::symbols::EDIT_GROUP_KEY)
        .and_then(|v| v.as_object_mut())
        .unwrap()
        .remove("version");
    let mut ed = Editor::new(doc);
    let before = ed.doc().clone();
    invalid(
        ed.execute(&Command::CloseComponent {
            group: g,
            resolve: None,
        }),
        "conflict",
    );
    assert_eq!(ed.doc(), &before);
    // The person says overwrite: it is written.
    ed.execute(&Command::CloseComponent {
        group: g,
        resolve: Some(Resolve::Overwrite),
    })
    .unwrap();
    assert!(ed.doc().object(g).is_none());
    assert_eq!(ed.doc().resources.symbol(sym).unwrap().parts.len(), 1);
}

#[test]
fn many_definitions_on_one_base_are_checked_in_one_pass() {
    // 5,000 definitions that each use a base of 10,000 models: the old check walked 50 million
    // models for one change of the base.
    let mut doc = Editor::with_sequential_ids().doc().clone();
    let base = push_symbol(
        &mut doc,
        "Base",
        parts_def((0..component::MAX_PARTS).map(|_| empty_model()).collect()),
    );
    for k in 0..5000 {
        push_symbol(&mut doc, &format!("D{k}"), parts_def(vec![inst(base)]));
    }
    let mut ed = Editor::new(doc);
    let t = std::time::Instant::now();
    ed.execute(&Command::ModifyResource {
        id: base,
        name: None,
        body: Some(symbol_body(parts_def(
            (0..component::MAX_PARTS - 1)
                .map(|_| empty_model())
                .collect(),
        ))),
    })
    .unwrap();
    let took = t.elapsed();
    assert!(took.as_secs_f64() < 3.0, "took {took:?}");
    // And the check still finds the real problem: two of them in one definition are too many.
    let mut parts = vec![inst(base), inst(base), inst(base)];
    parts.truncate(3);
    invalid(
        add_symbol(&mut ed, "Three", parts_def(parts)),
        "over a limit",
    );
}

#[test]
fn the_plan_mark_follows_the_definition_inside_and_outside_a_frame() {
    let mut ed = fresh();
    let (sym, i) = tile(&mut ed);
    let shape = ed.doc().object(i).unwrap().shape.clone();
    let boxed = |ed: &Editor| match component::plan_mark(ed.doc(), &shape) {
        Some(PlanMark::Box([lo, _, hi, _])) => (lo, hi),
        other => panic!("{other:?}"),
    };
    assert_eq!(boxed(&ed).1, DVec2::new(1000.0, 1000.0));
    let g = open_edit(&mut ed, i);
    let model = ed
        .doc()
        .children(g)
        .into_iter()
        .find(|c| matches!(ed.doc().object(*c).unwrap().shape, Shape::SurfaceModel(_)))
        .unwrap();
    ed.execute(&Command::EditSurface {
        id: model,
        edit: square(1000.0, 0.0, 1000.0),
    })
    .unwrap();
    ed.execute(&Command::CloseComponent {
        group: g,
        resolve: None,
    })
    .unwrap();
    assert_eq!(boxed(&ed).1, DVec2::new(2000.0, 1000.0), "outside a frame");
    {
        let _frame = component::plan_frame();
        assert_eq!(boxed(&ed).1, DVec2::new(2000.0, 1000.0));
        assert_eq!(boxed(&ed).1, DVec2::new(2000.0, 1000.0), "read again");
    }
    assert!(ed.undo());
    assert_eq!(boxed(&ed).1, DVec2::new(1000.0, 1000.0), "after the undo");
    assert!(component::definition_version(ed.doc().resources.symbol(sym).unwrap()) > 0);
    // Many instances in one frame are fast.
    let big = {
        let mut doc = Editor::with_sequential_ids().doc().clone();
        let d = push_symbol(
            &mut doc,
            "Big",
            parts_def((0..5000).map(|_| empty_model()).collect()),
        );
        let ids: Vec<ObjectId> = (0..5000).map(|_| push_instance(&mut doc, d)).collect();
        (doc, ids)
    };
    let t = std::time::Instant::now();
    let _frame = component::plan_frame();
    for id in &big.1 {
        component::plan_mark(&big.0, &big.0.object(*id).unwrap().shape);
    }
    assert!(t.elapsed().as_secs_f64() < 3.0, "took {:?}", t.elapsed());
}

#[test]
fn a_styled_model_is_never_the_raw_model_of_a_level() {
    let mut ed = fresh();
    let red = Attributes {
        fill: Some(Color([200, 10, 10, 255])),
        ..Attributes::default()
    };
    // The styled model has the lower id.
    let styled = model_at(&mut ed, 0.0, red.clone());
    assert_eq!(
        component::raw_model(ed.doc(), None, ed.doc().active_layer),
        None
    );
    let plain = model_at(&mut ed, 3000.0, Attributes::default());
    assert_eq!(
        component::raw_model(ed.doc(), None, ed.doc().active_layer),
        Some(plain)
    );
    // Exploding a group of one plain model and one styled model into a level that has only the
    // styled one: the plain geometry does not weld into it.
    let a = model_at(&mut ed, 6000.0, Attributes::default());
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![a],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::DeleteObjects { ids: vec![plain] })
        .unwrap();
    ed.execute(&Command::Explode { ids: vec![g] }).unwrap();
    assert_eq!(
        model(&ed, styled).face_count(),
        1,
        "the styled model is as it was"
    );
    assert_eq!(model(&ed, styled).edge_count(), 4);
    assert!(ed.doc().object(a).is_some(), "the plain model stands apart");
    assert_eq!(ed.doc().object(a).unwrap().parent, None);
}

#[test]
fn a_definition_over_a_limit_that_came_from_outside_is_reported_and_named() {
    let mut ed = fresh();
    let mut doc = ed.doc().clone();
    let base = push_symbol(&mut doc, "Base", parts_def(vec![empty_model()]));
    let big = push_symbol(
        &mut doc,
        "Big",
        parts_def((0..component::MAX_PARTS).map(|_| empty_model()).collect()),
    );
    let outer = push_symbol(
        &mut doc,
        "Outer",
        parts_def(vec![inst(base), inst(big), inst(big), inst(big)]),
    );
    let _ = outer;
    // Other users' document arrives with the problem: it is noted.
    assert!(ed.apply_remote(doc).is_some());
    let notes = ed.doc().load_notes().join("\n");
    assert!(notes.contains("'Outer'"), "{notes}");
    // A local edit of the shared base gets a message that names the cause.
    let r = ed.execute(&Command::ModifyResource {
        id: base,
        name: None,
        body: Some(symbol_body(parts_def(vec![empty_model(), empty_model()]))),
    });
    invalid(r, "which is over a limit");
}
