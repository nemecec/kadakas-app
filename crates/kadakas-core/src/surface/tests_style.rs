//! Tests of soft, smooth and hidden edges (`SetEdgeStyle`, `SoftenEdges`, MOD-08).

use proptest::prelude::*;

use super::*;

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
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 500.0,
    })
    .unwrap();
    m
}

fn cylinder(n: usize) -> SurfaceModel {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                [300.0 * a.cos(), 300.0 * a.sin(), 0.0]
            })
            .collect(),
        closed: true,
        faceted: false,
    })
    .unwrap();
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 800.0,
    })
    .unwrap();
    m
}

fn all_edges(m: &SurfaceModel) -> Vec<EdgeId> {
    m.edges().map(|(id, _)| id).collect()
}

fn count(m: &SurfaceModel, key: &str) -> usize {
    m.edges()
        .filter(|(id, _)| flagged(m.edge_attrs(*id), key))
        .count()
}

#[test]
fn set_edge_style_sets_and_clears_the_flags_and_leaves_hidden_alone() {
    let mut m = box_model();
    let e = all_edges(&m)[0];
    let rev = m.rev();
    m.apply(&Edit::SetEdgeStyle {
        edges: vec![e],
        soft: Some(true),
        smooth: Some(true),
    })
    .unwrap();
    assert_eq!(m.rev(), rev + 1);
    assert!(m.edge_soft(e) && m.edge_smooth(e) && !m.edge_hidden(e));
    // Hide does not touch soft; Unsoften does not touch hidden.
    m.apply(&Edit::SetHidden {
        faces: Vec::new(),
        edges: vec![e],
        hidden: true,
    })
    .unwrap();
    assert!(m.edge_soft(e) && m.edge_hidden(e));
    m.apply(&Edit::SetEdgeStyle {
        edges: vec![e],
        soft: Some(false),
        smooth: Some(false),
    })
    .unwrap();
    assert!(!m.edge_soft(e) && !m.edge_smooth(e) && m.edge_hidden(e));
    m.apply(&Edit::SetEdgeStyle {
        edges: vec![e],
        soft: Some(true),
        smooth: None,
    })
    .unwrap();
    m.apply(&Edit::SetHidden {
        faces: Vec::new(),
        edges: vec![e],
        hidden: false,
    })
    .unwrap();
    assert!(m.edge_soft(e) && !m.edge_hidden(e), "Unhide leaves soft");
    // Only the smooth flag goes.
    m.apply(&Edit::SetEdgeStyle {
        edges: vec![e],
        soft: Some(false),
        smooth: None,
    })
    .unwrap();
    assert!(m.edge_attrs(e).is_none(), "no empty map is left");
    // The same flags again change nothing: no new revision.
    let rev = m.rev();
    m.apply(&Edit::SetEdgeStyle {
        edges: vec![e],
        soft: Some(false),
        smooth: None,
    })
    .unwrap();
    assert_eq!(m.rev(), rev);
    assert_eq!(
        m.apply(&Edit::SetEdgeStyle {
            edges: vec![EdgeId(999)],
            soft: Some(true),
            smooth: None,
        }),
        Err(SurfaceError::NoEdge(EdgeId(999)))
    );
    assert!(m.check().is_closed(), "flags change no geometry");
}

#[test]
fn a_soft_edge_between_coplanar_faces_does_not_merge_them() {
    let mut m = box_model();
    m.apply(&Edit::AddEdges {
        edges: vec![[[500.0, 0.0, 500.0], [500.0, 1000.0, 500.0]]],
    })
    .unwrap();
    let faces = m.face_count();
    let line = m
        .edges()
        .find(|(_, e)| {
            let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            a.x == 500.0 && b.x == 500.0 && a.z == 500.0 && b.z == 500.0
        })
        .unwrap()
        .0;
    // Without "soften coplanar" the edge stays as it is.
    m.apply(&Edit::SoftenEdges {
        edges: vec![line],
        angle: 45.0,
        smooth: true,
        coplanar: false,
    })
    .unwrap();
    assert!(m.edge_attrs(line).is_none());
    m.apply(&Edit::SoftenEdges {
        edges: vec![line],
        angle: 45.0,
        smooth: true,
        coplanar: true,
    })
    .unwrap();
    assert!(flagged(m.edge_attrs(line), ATTR_SOFT));
    assert_eq!(m.face_count(), faces, "the two halves stay two faces");
    assert_eq!(dihedral_of(&m, line), Some(0.0));
}

#[test]
fn soften_edges_softens_below_the_angle_and_unsoftens_above() {
    let mut m = cylinder(12);
    // Push/Pull made the 12 sides soft and smooth.
    assert_eq!((count(&m, ATTR_SOFT), count(&m, ATTR_SMOOTH)), (12, 12));
    let es = all_edges(&m);
    // 20 degrees: the sides (30 degrees apart) lose the flags, the rims (90) too.
    m.apply(&Edit::SoftenEdges {
        edges: es.clone(),
        angle: 20.0,
        smooth: true,
        coplanar: false,
    })
    .unwrap();
    assert_eq!(count(&m, ATTR_SOFT), 0);
    // 45 degrees: the sides again, not the rims; without smooth normals only soft.
    m.apply(&Edit::SoftenEdges {
        edges: es.clone(),
        angle: 45.0,
        smooth: false,
        coplanar: false,
    })
    .unwrap();
    assert_eq!((count(&m, ATTR_SOFT), count(&m, ATTR_SMOOTH)), (12, 0));
    // 180 degrees: all edges between two faces.
    m.apply(&Edit::SoftenEdges {
        edges: es.clone(),
        angle: 180.0,
        smooth: true,
        coplanar: false,
    })
    .unwrap();
    assert_eq!(count(&m, ATTR_SOFT), 36);
    for bad in [-1.0, 181.0, f64::NAN] {
        assert!(
            m.apply(&Edit::SoftenEdges {
                edges: es.clone(),
                angle: bad,
                smooth: true,
                coplanar: false
            })
            .is_err()
        );
    }
    // The angles: a side 30 degrees, a rim 90.
    let side = es
        .iter()
        .copied()
        .find(|e| {
            let ed = m.edge(*e).unwrap();
            m.vertex(ed.v[0]).unwrap().z != m.vertex(ed.v[1]).unwrap().z
        })
        .unwrap();
    assert!((dihedral_of(&m, side).unwrap() - 30.0).abs() < 1e-6);
}

#[test]
fn edge_styles_replay_on_another_model() {
    let m0 = cylinder(12);
    let mut m = m0.clone();
    m.apply(&Edit::SoftenEdges {
        edges: all_edges(&m0),
        angle: 10.0,
        smooth: true,
        coplanar: false,
    })
    .unwrap();
    let entry = m.log().last().unwrap().clone();
    let mut c = m0.clone();
    sync::replay_intent(&mut c, &entry.intent).unwrap();
    assert_eq!(count(&c, ATTR_SOFT), 0);
}

#[test]
fn a_moved_fold_keeps_its_soft_and_smooth_edges() {
    // Lifting one corner of the top folds it along a soft and smooth edge.
    let mut m = box_model();
    let corner = m
        .vertices()
        .find(|(_, p)| *p == glam::DVec3::new(1000.0, 1000.0, 500.0))
        .unwrap()
        .0;
    m.apply(&Edit::TransformVertices {
        vertices: vec![corner],
        faces: Vec::new(),
        edges: Vec::new(),
        matrix: glam::DAffine3::from_translation(glam::DVec3::Z * 200.0).to_cols_array(),
    })
    .unwrap();
    assert!(count(&m, ATTR_SOFT) >= 1);
    assert_eq!(count(&m, ATTR_SOFT), count(&m, ATTR_SMOOTH));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Soften Edges on all edges of a cylinder: an edge is soft exactly when its angle is below
    /// the limit, and the faces never change.
    #[test]
    fn soften_follows_the_angle(n in 3usize..40, angle in 0.0f64..180.0, smooth: bool) {
        let mut m = cylinder(n);
        let faces: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
        let es = all_edges(&m);
        m.apply(&Edit::SoftenEdges { edges: es.clone(), angle, smooth, coplanar: false }).unwrap();
        for e in es {
            let a = dihedral_of(&m, e).unwrap();
            prop_assert_eq!(flagged(m.edge_attrs(e), ATTR_SOFT), a < angle);
            prop_assert_eq!(flagged(m.edge_attrs(e), ATTR_SMOOTH), a < angle && smooth);
        }
        prop_assert_eq!(m.faces().map(|(id, _)| id).collect::<Vec<_>>(), faces);
    }
}
