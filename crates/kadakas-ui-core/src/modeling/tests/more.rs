//! More tests of the modeling tools: scaled models, the edges that Push/Pull leaves out of the
//! inference, and the selection of connected geometry.

use super::*;

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
