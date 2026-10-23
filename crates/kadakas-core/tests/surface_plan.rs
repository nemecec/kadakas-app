//! Surface models and component instances as objects of the plan (MOD-13): the SVG, the snapping,
//! the 3D scene and the commands that move, turn, copy and delete them with the undo of each.

use glam::{DAffine3, DVec2, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh::{self, SURFACE_BACK, SURFACE_FRONT};
use kadakas_core::model::*;
use kadakas_core::snapping::{self, Context, Geometry, Kind, Reach, Settings, Smart};
use kadakas_core::surface::plan::{self, Detail, Outcome, PlanItem};
use kadakas_core::surface::{Edit, SurfaceModel};
use kadakas_core::svg;
use kadakas_core::visibility::VisMode;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn add(ed: &mut Editor, shape: Shape, class: Option<ClassId>, layer: Option<LayerId>) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer,
            class,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

/// A closed box of 1000 x 1000 x 500 mm with its corner at `(x, y, 0)`: a square pulled up, so
/// that the fronts of the faces look out.
fn cube(ed: &mut Editor, x: f64, y: f64) -> ObjectId {
    let id = add(ed, Shape::SurfaceModel(Box::default()), None, None);
    fill_cube(ed, id, x, y);
    id
}

fn fill_cube(ed: &mut Editor, id: ObjectId, x: f64, y: f64) {
    ed.execute(&Command::EditSurface {
        id,
        edit: square_edges(x, y, 1000.0),
    })
    .unwrap();
    let face = model(ed, id).faces().next().unwrap().0;
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::PushPull {
            face,
            distance: 500.0,
        },
    })
    .unwrap();
}

fn square_edges(x: f64, y: f64, s: f64) -> Edit {
    let p = [
        [x, y, 0.0],
        [x + s, y, 0.0],
        [x + s, y + s, 0.0],
        [x, y + s, 0.0],
    ];
    Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    }
}

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("{}", other.kind()),
    }
}

fn drawing(ed: &Editor, id: ObjectId) -> plan::PlanDrawing {
    match plan::plan_of_object(ed.doc(), ed.doc().object(id).unwrap()) {
        Some(Outcome::Drawing(d)) => d,
        other => panic!("{other:?}"),
    }
}

fn component(ed: &mut Editor, m: ObjectId) -> (kadakas_core::resource::ResourceId, ObjectId) {
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Block".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![m],
            replace: true,
        })
        .unwrap();
    (
        kadakas_core::resource::ResourceId(out.created[0]),
        ObjectId(out.created[1]),
    )
}

fn geometry(ed: &Editor, near: Option<(DVec2, f64)>) -> Geometry {
    Geometry::from_doc(ed.doc(), &[], Reach::default(), near)
}

#[test]
fn the_svg_draws_faces_and_edges_and_says_how_much() {
    let mut ed = Editor::default();
    let id = cube(&mut ed, 0.0, 0.0);
    let text = svg::render(ed.doc(), 800);
    assert!(text.contains(r#"data-kind="surface_model""#), "{text}");
    assert!(text.contains(r#"data-detail="full""#));
    assert!(text.contains(r#"data-faces="2" data-edges="8""#));
    // The top and the bottom are filled paths in the colours of the fronts and backs.
    assert!(text.contains("rgba(236,236,230,1.000)"), "{text}");
    assert!(text.contains("rgba(148,168,199,1.000)"));
    // A soft edge goes out of the picture.
    let soft = model(&ed, id)
        .edges()
        .find(|(_, e)| {
            let m = model(&ed, id);
            m.vertex(e.v[0]).unwrap().z == 500.0 && m.vertex(e.v[1]).unwrap().z == 500.0
        })
        .map(|(e, _)| e)
        .unwrap();
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetEdgeStyle {
            edges: vec![soft],
            soft: Some(true),
            smooth: None,
        },
    })
    .unwrap();
    assert!(
        svg::render(ed.doc(), 800).contains(r#"data-faces="2" data-edges="7""#),
        "soft edges are not drawn"
    );
}

#[test]
fn an_instance_draws_the_same_as_the_model_and_a_broken_one_a_cross() {
    let mut ed = Editor::default();
    let id = cube(&mut ed, 0.0, 0.0);
    let before = drawing(&ed, id);
    let (_, inst) = component(&mut ed, id);
    assert_eq!(drawing(&ed, inst), before);
    // Move the instance: the drawing moves with it.
    ed.execute(&Command::MoveObjects {
        ids: vec![inst],
        delta: v(5000.0, 0.0),
    })
    .unwrap();
    let moved = drawing(&ed, inst);
    assert_eq!(moved.bounds(), Some((v(5000.0, 0.0), v(6000.0, 1000.0))));
    assert_eq!(
        (moved.faces(), moved.edges()),
        (before.faces(), before.edges())
    );
    // An instance of a definition that is gone is a cross in the warning colour.
    let mut doc = ed.doc().clone();
    let Shape::Component { transform, .. } = doc.object(inst).unwrap().shape.clone() else {
        panic!()
    };
    doc.object_mut(inst).unwrap().shape = Shape::Component {
        symbol: kadakas_core::resource::ResourceId::from_num(99),
        transform,
    };
    let Some(Outcome::Broken(p, why)) = plan::plan_of_object(&doc, doc.object(inst).unwrap())
    else {
        panic!()
    };
    assert_eq!(p, v(transform[9], transform[10]));
    assert!(!why.is_empty());
    let text = svg::render(&doc, 800);
    assert!(text.contains("data-problem="), "{text}");
}

#[test]
fn a_part_of_a_definition_in_a_class_that_is_off_is_not_drawn() {
    let mut ed = Editor::default();
    let part_class = ClassId(
        ed.execute(&Command::AddClass {
            name: "Parts".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let m = add(
        &mut ed,
        Shape::SurfaceModel(Box::default()),
        Some(part_class),
        None,
    );
    fill_cube(&mut ed, m, 0.0, 0.0);
    let (_, inst) = component(&mut ed, m);
    // The instance is in the first class; the part keeps "Parts".
    ed.execute(&Command::ModifyObject {
        id: inst,
        shape: None,
        layer: None,
        class: Some(ed.doc().active_class),
        attrs: None,
    })
    .unwrap();
    assert_eq!(drawing(&ed, inst).faces(), 2);
    let scene_tris = |ed: &Editor| mesh::build(ed.doc()).mesh.triangle_count();
    assert!(scene_tris(&ed) > 0);
    ed.execute(&Command::ModifyClass {
        id: part_class,
        name: None,
        visible: Some(false),
        attrs: None,
    })
    .unwrap();
    let d = drawing(&ed, inst);
    assert!(d.items.is_empty());
    assert_eq!(scene_tris(&ed), 0, "the 3D scene leaves the part out too");
    // Nothing to snap to either.
    assert!(geometry(&ed, None).points.is_empty());
}

#[test]
fn snapping_finds_vertices_edge_middles_and_face_centres() {
    let mut ed = Editor::default();
    let id = cube(&mut ed, 0.0, 0.0);
    let g = geometry(&ed, None);
    let has = |p: DVec2, k: Kind| g.points.iter().any(|q| q.p == p && q.kind == k);
    assert!(has(v(0.0, 0.0), Kind::Endpoint));
    assert!(has(v(1000.0, 1000.0), Kind::Endpoint));
    assert!(has(v(500.0, 0.0), Kind::Midpoint));
    assert!(has(v(500.0, 500.0), Kind::Center));
    assert_eq!(g.edges.len(), 8);
    // The pointer lands on a vertex, a middle and a face centre.
    let ctx = Context {
        tol: 20.0,
        grid_step: 0.0,
        anchor: None,
        prior_direction: None,
    };
    let settings = Settings::default();
    let at = |p: DVec2| snapping::snap(p, &ctx, &settings, &g, &Smart::default());
    let s = at(v(5.0, -8.0)).unwrap();
    assert_eq!((s.point, s.kind), (v(0.0, 0.0), Kind::Endpoint));
    let s = at(v(510.0, 7.0)).unwrap();
    assert_eq!((s.point, s.kind), (v(500.0, 0.0), Kind::Midpoint));
    let s = at(v(495.0, 505.0)).unwrap();
    assert_eq!((s.point, s.kind), (v(500.0, 500.0), Kind::Center));
    // Only what is near the pointer.
    let near = geometry(&ed, Some((v(1000.0, 1000.0), 30.0)));
    assert!(
        near.points
            .iter()
            .all(|p| p.p.distance(v(1000.0, 1000.0)) <= 30.0 + 1e-9)
    );
    assert!(!near.points.is_empty());
    // Hidden edges and soft edges give nothing.
    let all: Vec<_> = model(&ed, id).edges().map(|(e, _)| e).collect();
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetEdgeStyle {
            edges: all,
            soft: Some(true),
            smooth: None,
        },
    })
    .unwrap();
    let g = geometry(&ed, None);
    assert!(g.edges.is_empty());
    assert!(g.points.iter().all(|p| p.kind == Kind::Center));
}

#[test]
fn an_instance_and_its_class_follow_the_visibility_of_other_objects() {
    let mut ed = Editor::default();
    let class = ClassId(
        ed.execute(&Command::AddClass {
            name: "Sheds".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let m = add(
        &mut ed,
        Shape::SurfaceModel(Box::default()),
        Some(class),
        None,
    );
    fill_cube(&mut ed, m, 0.0, 0.0);
    let on = |ed: &Editor| {
        let o = ed.doc().object(m).unwrap();
        (
            ed.doc().is_visible(o),
            ed.doc().is_pickable(o),
            !geometry(ed, None).points.is_empty(),
            mesh::build(ed.doc()).mesh.triangle_count() > 0,
            svg::render(ed.doc(), 800).contains("surface_model"),
        )
    };
    assert_eq!(on(&ed), (true, true, true, true, true));
    ed.execute(&Command::SetVisibility {
        layers: vec![],
        classes: vec![class],
        mode: VisMode::Grey,
    })
    .unwrap();
    // Grey: drawn, cannot be picked, not snapped to.
    let (visible, pickable, snap, tris, drawn) = on(&ed);
    assert!(visible && !pickable && !snap && tris && drawn);
    ed.execute(&Command::SetVisibility {
        layers: vec![],
        classes: vec![class],
        mode: VisMode::Invisible,
    })
    .unwrap();
    assert_eq!(on(&ed), (false, false, false, false, false));
    ed.execute(&Command::SetVisibility {
        layers: vec![],
        classes: vec![class],
        mode: VisMode::Visible,
    })
    .unwrap();
    assert_eq!(on(&ed), (true, true, true, true, true));
}

#[test]
fn move_turn_copy_and_delete_are_one_undo_step_each() {
    let mut ed = Editor::default();
    let id = cube(&mut ed, 0.0, 0.0);
    let steps = |ed: &Editor| ed.doc().objects.len();
    let start = model(&ed, id).clone();
    // Move.
    ed.execute(&Command::MoveObjects {
        ids: vec![id],
        delta: v(2500.0, 100.0),
    })
    .unwrap();
    assert_eq!(
        model(&ed, id).bounds().unwrap().0,
        DVec3::new(2500.0, 100.0, 0.0)
    );
    assert_eq!(
        drawing(&ed, id).bounds(),
        Some((v(2500.0, 100.0), v(3500.0, 1100.0)))
    );
    ed.undo();
    assert_eq!(model(&ed, id), &start);
    // Turn a quarter about the origin: x and y swap with a sign.
    ed.execute(&Command::RotateObjects {
        ids: vec![id],
        center: DVec2::ZERO,
        angle: std::f64::consts::FRAC_PI_2,
    })
    .unwrap();
    let (lo, hi) = drawing(&ed, id).bounds().unwrap();
    assert!((lo - v(-1000.0, 0.0)).length() < 1e-6 && (hi - v(0.0, 1000.0)).length() < 1e-6);
    ed.undo();
    assert_eq!(model(&ed, id), &start);
    // Copy.
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![id],
            delta: v(3000.0, 0.0),
        })
        .unwrap();
    assert_eq!(steps(&ed), 2);
    let copy = ObjectId(out.created[0]);
    assert_eq!(
        drawing(&ed, copy).bounds(),
        Some((v(3000.0, 0.0), v(4000.0, 1000.0)))
    );
    ed.undo();
    assert_eq!(steps(&ed), 1);
    // Delete.
    ed.execute(&Command::DeleteObjects { ids: vec![id] })
        .unwrap();
    assert_eq!(steps(&ed), 0);
    ed.undo();
    assert_eq!(model(&ed, id), &start);
    // A group of two models moves as one.
    let other = cube(&mut ed, 5000.0, 0.0);
    let g = ObjectId(
        ed.execute(&Command::Group {
            ids: vec![id, other],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::MoveObjects {
        ids: vec![g],
        delta: v(0.0, 700.0),
    })
    .unwrap();
    assert_eq!(model(&ed, id).bounds().unwrap().0.y, 700.0);
    assert_eq!(model(&ed, other).bounds().unwrap().0.y, 700.0);
    ed.undo();
    assert_eq!(model(&ed, other).bounds().unwrap().0.y, 0.0);
}

#[test]
fn an_instance_is_moved_turned_and_copied_through_its_transform() {
    let mut ed = Editor::default();
    let m = cube(&mut ed, 0.0, 0.0);
    let (sym, inst) = component(&mut ed, m);
    let shape = |ed: &Editor| ed.doc().object(inst).unwrap().shape.clone();
    let start = shape(&ed);
    ed.execute(&Command::MoveObjects {
        ids: vec![inst],
        delta: v(100.0, 200.0),
    })
    .unwrap();
    let Shape::Component { symbol, transform } = shape(&ed) else {
        panic!()
    };
    assert_eq!(symbol, sym);
    assert_eq!(&transform[9..11], &[100.0, 200.0]);
    ed.execute(&Command::RotateObjects {
        ids: vec![inst],
        center: v(100.0, 200.0),
        angle: std::f64::consts::PI,
    })
    .unwrap();
    let (lo, hi) = drawing(&ed, inst).bounds().unwrap();
    assert!((lo - v(-900.0, -800.0)).length() < 1e-6, "{lo}");
    assert!((hi - v(100.0, 200.0)).length() < 1e-6, "{hi}");
    ed.undo();
    ed.undo();
    assert_eq!(shape(&ed), start);
    // A copy shares the definition.
    let out = ed
        .execute(&Command::CopyObjects {
            ids: vec![inst],
            delta: v(2000.0, 0.0),
        })
        .unwrap();
    let Shape::Component { symbol, .. } = ed.doc().object(ObjectId(out.created[0])).unwrap().shape
    else {
        panic!()
    };
    assert_eq!(symbol, sym);
    // The transform of the instance is a plain affine map.
    let Shape::Component { transform, .. } = shape(&ed) else {
        panic!()
    };
    assert_eq!(
        kadakas_core::component::affine(&transform),
        DAffine3::IDENTITY
    );
}

#[test]
fn the_3d_scene_keeps_soft_edges_apart_and_draws_the_others() {
    let mut ed = Editor::default();
    let id = cube(&mut ed, 0.0, 0.0);
    let scene = mesh::build(ed.doc());
    assert_eq!(scene.lines.segment_count(), 12);
    assert!(scene.soft_edges.is_empty());
    let e: Vec<_> = model(&ed, id).edges().map(|(e, _)| e).take(3).collect();
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetEdgeStyle {
            edges: e.clone(),
            soft: Some(true),
            smooth: None,
        },
    })
    .unwrap();
    let fifth = model(&ed, id).edges().nth(5).unwrap().0;
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::SetHidden {
            faces: vec![],
            edges: vec![fifth],
            hidden: true,
        },
    })
    .unwrap();
    let scene = mesh::build(ed.doc());
    assert_eq!(scene.lines.segment_count(), 8);
    assert_eq!(scene.soft_edges.len(), 4);
}

#[test]
fn a_small_model_is_drawn_in_full_and_the_numbers_say_so() {
    let mut ed = Editor::default();
    let id = cube(&mut ed, 0.0, 0.0);
    let d = drawing(&ed, id);
    assert_eq!(d.detail, Detail::Full);
    let colors: Vec<[u8; 4]> = d
        .items
        .iter()
        .filter_map(|i| match i {
            PlanItem::Face(f) => Some(f.color),
            _ => None,
        })
        .collect();
    assert_eq!(colors, [SURFACE_BACK, SURFACE_FRONT]);
}
