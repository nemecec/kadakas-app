//! More tests of the modeling tools: scaled models, the edges that Push/Pull leaves out of the
//! inference, the selection of connected geometry, and Intersect Faces.

use super::*;

#[test]
fn intersect_actions_group_the_parts_by_model_and_run_as_one_step() {
    use kadakas_core::command::Command;
    use kadakas_core::surface::{Edit, IntersectScope};
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::AddObject {
        shape: Shape::SurfaceModel(Box::default()),
        layer: None,
        class: None,
        attrs: Default::default(),
    })
    .unwrap();
    let id = *ed.doc().objects.keys().next().unwrap();
    // A square, and a line through it.
    let p = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    let mut edges: Vec<[[f64; 3]; 2]> = (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect();
    edges.push([[500.0, 500.0, -500.0], [500.0, 500.0, 500.0]]);
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges { edges },
    })
    .unwrap();
    let m = model_of(ed.doc(), id).unwrap();
    assert_eq!(m.check().not_intersected(), 1);
    let face = m.faces().next().unwrap().0;
    let mut sel = SubSelection::default();
    // Nothing chosen: no action for With Selection, nor for With Model without a model.
    assert!(intersect_actions(&sel, IntersectScope::Selection, &[id]).is_empty());
    assert!(intersect_actions(&sel, IntersectScope::Model, &[]).is_empty());
    // With Model on a chosen model: the whole model.
    assert_eq!(
        intersect_actions(&sel, IntersectScope::Model, &[id]),
        vec![Action::Intersect {
            model: id,
            scope: IntersectScope::Model,
            faces: vec![],
            edges: vec![],
        }]
    );
    sel.set([Item::Face(id, face)]);
    let actions = intersect_actions(&sel, IntersectScope::Selection, &[]);
    assert_eq!(
        actions,
        vec![Action::Intersect {
            model: id,
            scope: IntersectScope::Selection,
            faces: vec![face],
            edges: vec![],
        }]
    );
    let (_, notes) = run_actions_noted(&mut ed, &actions, false).unwrap();
    assert!(
        notes.iter().any(|n| n.contains("it is split there")),
        "{notes:?}"
    );
    assert_eq!(model_of(ed.doc(), id).unwrap().check().not_intersected(), 0);
    // One undo step takes it back.
    ed.undo();
    assert_eq!(model_of(ed.doc(), id).unwrap().check().not_intersected(), 1);
}

#[test]
fn a_typed_distance_is_in_world_mm_even_when_the_model_is_scaled() {
    use glam::DAffine3;
    use kadakas_core::command::Command;
    use kadakas_core::surface::Edit;
    let mut ed = Editor::with_sequential_ids();
    let mut m = SurfaceModel::new();
    m.transform = DAffine3::from_scale(DVec3::splat(2.0));
    ed.execute(&Command::AddObject {
        shape: Shape::SurfaceModel(Box::new(m)),
        layer: None,
        class: None,
        attrs: Default::default(),
    })
    .unwrap();
    let id = *ed.doc().objects.keys().next().unwrap();
    let p = [
        [0.0, 0.0, 0.0],
        [500.0, 0.0, 0.0],
        [500.0, 500.0, 0.0],
        [0.0, 500.0, 0.0],
    ];
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let face = model_of(ed.doc(), id).unwrap().faces().next().unwrap().0;
    assert!((exec::local_distance(ed.doc(), id, face, 400.0) - 200.0).abs() < 1e-9);
    assert!((exec::local_distance(ed.doc(), id, face, -400.0) + 200.0).abs() < 1e-9);
    run_actions(
        &mut ed,
        &[Action::PushPull {
            model: id,
            face,
            distance: 400.0,
        }],
        false,
    )
    .unwrap();
    let (lo, hi) = model_of(ed.doc(), id).unwrap().bounds().unwrap();
    assert!((hi.z - lo.z - 400.0).abs() < 1e-9, "{lo:?} {hi:?}");
}

#[test]
fn push_pull_leaves_its_edges_out_of_the_inference_and_a_tiny_distance_says_so() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    let face = r.model().faces().next().unwrap().0;
    let model = r.model_id();
    r.tools.choose(ModelerTool::PushPull);
    r.click_inf(
        on_face(v(500.0, 500.0, 0.0), face),
        down_ray(v(500.0, 500.0, 0.0)),
    );
    let ex = r.tools.excluded();
    assert_eq!(ex.faces.len(), 1);
    assert_eq!(ex.edges.len(), 4, "the four edges of the face");
    assert!(ex.edges.iter().all(|(m, _)| *m == model));
    // Clicking again at the same height: a distance of nothing is said, not dropped.
    let out = r.click_inf(
        free(v(500.0, 500.0, 0.0)),
        pull_ray(v(-5000.0, 500.0, 0.0), v(500.0, 500.0, 0.0)),
    );
    assert!(out.actions.is_empty() && out.message.is_some(), "{out:?}");
}

#[test]
fn a_double_click_selects_an_edge_with_its_faces_and_a_triple_click_everything_connected() {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    // A second, separate square in the same model.
    r.click(v(5000.0, 0.0, 0.0));
    r.click(v(6000.0, 1000.0, 0.0));
    let model = r.model_id();
    let doc = r.ed.doc();
    let m = model_of(doc, model).unwrap();
    let first_edge = m
        .edges()
        .find(|(_, e)| m.vertex(e.v[0]).unwrap().x < 1500.0)
        .unwrap()
        .0;
    let items = select::with_neighbors(doc, Item::Edge(model, first_edge));
    assert_eq!(items.len(), 2, "the edge and its face: {items:?}");
    assert!(matches!(items[1], Item::Face(..)));
    let all = select::connected(doc, Item::Edge(model, first_edge));
    // The first square only: 4 edges and a face. The other square is not connected.
    assert_eq!(all.len(), 5, "{all:?}");
    let face = m
        .faces()
        .find(|(_, f)| m.vertex(f.outer[0]).unwrap().x < 1500.0)
        .unwrap()
        .0;
    assert_eq!(select::connected(doc, Item::Face(model, face)), all);
    assert_eq!(
        select::with_neighbors(doc, Item::Face(model, face)).len(),
        5
    );
}
