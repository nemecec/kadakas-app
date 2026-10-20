//! Taking a part out of a surface model and drawing a model into another one (the base of MOD-07).

use glam::{DAffine3, DVec3};
use kadakas_core::surface::{EdgeId, Edit, FaceId, SurfaceError, SurfaceModel};

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

fn two_squares() -> SurfaceModel {
    let mut m = SurfaceModel::new();
    m.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    m.apply(&square(1000.0, 0.0, 1000.0)).unwrap();
    m
}

fn faces(m: &SurfaceModel) -> Vec<FaceId> {
    m.faces().map(|(f, _)| f).collect()
}

#[test]
fn select_takes_the_edges_of_faces_and_whole_curves() {
    let mut m = two_squares();
    let f = faces(&m);
    let sel = m.select(&[f[0]], &[]).unwrap();
    assert_eq!(sel.faces.len(), 1);
    assert_eq!(sel.edges.len(), 4);
    assert!(!sel.is_empty());
    assert!(!m.is_whole(&sel));
    let all = m.select(&f, &[]).unwrap();
    assert!(m.is_whole(&all));
    // An edge alone.
    let e = m.edges().next().unwrap().0;
    let one = m.select(&[], &[e]).unwrap();
    assert_eq!((one.faces.len(), one.edges.len()), (0, 1));
    // A curve goes whole when one of its edges is chosen.
    let chain: Vec<EdgeId> = m.edges().map(|(e, _)| e).take(2).collect();
    m.add_curve(chain.clone()).unwrap();
    let s = m.select(&[], &[chain[0]]).unwrap();
    assert!(s.edges.contains(&chain[1]));
    // Unknown ids.
    assert_eq!(
        m.select(&[FaceId(900)], &[]),
        Err(SurfaceError::NoFace(FaceId(900)))
    );
    assert_eq!(
        m.select(&[], &[EdgeId(900)]),
        Err(SurfaceError::NoEdge(EdgeId(900)))
    );
    assert!(m.select(&[], &[]).unwrap().is_empty());
}

#[test]
fn extract_copies_and_remove_takes_out() {
    let m = two_squares();
    let f = faces(&m);
    let sel = m.select(&[f[0]], &[]).unwrap();
    let e0 = *sel.edges.iter().next().unwrap();
    let part = m.extract(&sel);
    assert_eq!(part.face_count(), 1);
    assert_eq!(part.edge_count(), 4);
    assert_eq!(part.vertex_count(), 4);
    assert_eq!(part.rev(), 0);
    assert_eq!(part.next_id(), m.next_id());
    assert!(part.problem().is_none());
    assert!(part.edge(e0).is_some());
    let mut rest = m.clone();
    rest.remove(&sel).unwrap();
    // The face is gone; the edge that the other face uses stays.
    assert_eq!(rest.face_count(), 1);
    assert_eq!(rest.edge_count(), 4);
    assert!(rest.problem().is_none());
    // Removing nothing is allowed and changes nothing.
    let mut same = m.clone();
    same.remove(&Default::default()).unwrap();
    assert_eq!(same, m);
    // Removing a loose edge.
    let mut w = m.clone();
    w.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 0.0], [0.0, 0.0, 500.0]]],
    })
    .unwrap();
    let loose = w
        .edges()
        .find(|(_, e)| w.vertex(e.v[1]).unwrap().z != w.vertex(e.v[0]).unwrap().z)
        .unwrap()
        .0;
    let s = w.select(&[], &[loose]).unwrap();
    w.remove(&s).unwrap();
    assert_eq!(w.edge_count(), m.edge_count());
}

#[test]
fn extract_keeps_the_name_the_placement_and_whole_curves() {
    let mut m = two_squares();
    m.name = "Court".into();
    m.transform = DAffine3::from_translation(DVec3::new(10.0, 20.0, 30.0));
    let f = faces(&m);
    let sel = m.select(&[f[0]], &[]).unwrap();
    // Two edges of the first face that meet make a curve.
    let edges: Vec<EdgeId> = sel.edges.iter().copied().collect();
    let pair = edges
        .iter()
        .flat_map(|a| edges.iter().map(move |b| (*a, *b)))
        .find(|(a, b)| {
            a < b
                && m.edge(*a)
                    .unwrap()
                    .v
                    .iter()
                    .any(|v| m.edge(*b).unwrap().v.contains(v))
        })
        .unwrap();
    m.add_curve(vec![pair.0, pair.1]).unwrap();
    let part = m.extract(&m.select(&[f[0]], &[]).unwrap());
    assert_eq!(part.name, "Court");
    assert_eq!(part.transform, m.transform);
    assert_eq!(part.curves().count(), 1);
    // A curve with only some of its edges in the selection does not go.
    let partial = kadakas_core::surface::Selection {
        faces: Default::default(),
        edges: [pair.0].into_iter().collect(),
    };
    assert_eq!(m.extract(&partial).curves().count(), 0);
}

#[test]
fn absorb_welds_splits_and_forms_faces() {
    let mut a = SurfaceModel::new();
    a.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    let mut b = SurfaceModel::new();
    b.apply(&square(1000.0, 0.0, 1000.0)).unwrap();
    a.absorb(&b, None).unwrap();
    assert_eq!(a.face_count(), 2);
    assert_eq!(a.vertex_count(), 6);
    assert_eq!(a.edge_count(), 7);
    assert!(a.problem().is_none());
    // Placed: the second model is moved by a transform.
    let mut c = SurfaceModel::new();
    c.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    let mut d = SurfaceModel::new();
    d.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    c.absorb(
        &d,
        Some(DAffine3::from_translation(DVec3::new(1000.0, 0.0, 0.0))),
    )
    .unwrap();
    assert_eq!(c.face_count(), 2);
    assert_eq!(c.area(), 2_000_000.0);
    // Absorbing an empty model changes nothing.
    let before = c.clone();
    c.absorb(&SurfaceModel::new(), None).unwrap();
    assert_eq!(c, before);
}

#[test]
fn absorb_gives_back_attributes_and_the_side_of_faces() {
    let mut a = SurfaceModel::new();
    let mut b = SurfaceModel::new();
    b.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    let fb = faces(&b)[0];
    b.apply(&Edit::ReverseFaces { faces: vec![fb] }).unwrap();
    assert!(b.face_normal(fb).unwrap().z < 0.0);
    // Attributes through the parts constructor: a face with a material and an edge that is hidden.
    let text = serde_json::to_value(kadakas_core::model::Shape::SurfaceModel(Box::new(
        b.clone(),
    )))
    .unwrap();
    let mut text = text;
    text["faces"][0]["attrs"] = serde_json::json!({"material": "grass"});
    let edges = text["edges"].as_array_mut().unwrap();
    let first = edges[0].as_array().unwrap().clone();
    edges[0] = serde_json::json!([first[0], first[1], first[2], {"hidden": true}]);
    let kadakas_core::model::Shape::SurfaceModel(b) = serde_json::from_value(text).unwrap() else {
        panic!()
    };
    a.absorb(&b, None).unwrap();
    let fa = faces(&a)[0];
    assert!(a.face_normal(fa).unwrap().z < 0.0, "the side is kept");
    assert_eq!(
        a.face(fa).unwrap().attrs.get("material"),
        Some(&serde_json::json!("grass"))
    );
    assert_eq!(
        a.edges()
            .filter(|(e, _)| a.edge_attrs(*e).is_some_and(|x| x.contains_key("hidden")))
            .count(),
        1
    );
    // The change is in the log as one that cannot be replayed.
    assert!(
        a.log()
            .iter()
            .any(|e| matches!(e.intent, kadakas_core::surface::sync::Intent::Unlogged))
    );
    // A mirror turns the side of the faces over; the faces keep the side they looked to.
    let mut m = SurfaceModel::new();
    let mut s = SurfaceModel::new();
    s.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    m.absorb(&s, Some(DAffine3::from_scale(DVec3::new(1.0, 1.0, -1.0))))
        .unwrap();
    assert_eq!(m.face_count(), 1);
    assert!(m.problem().is_none());
}

#[test]
fn absorb_keeps_curves_that_are_found_again() {
    let mut src = SurfaceModel::new();
    src.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    let chain: Vec<EdgeId> = {
        let mut out = vec![];
        let first = src.edges().next().unwrap();
        out.push(first.0);
        let second = src
            .edges()
            .find(|(e, x)| *e != first.0 && x.v.iter().any(|v| first.1.v.contains(v)))
            .unwrap();
        out.push(second.0);
        out
    };
    src.add_curve(chain).unwrap();
    let mut dst = SurfaceModel::new();
    dst.absorb(&src, None).unwrap();
    assert_eq!(dst.curves().count(), 1);
    // A curve whose edge is cut by other geometry is no curve afterwards: nothing is lost but the identity.
    let mut dst2 = SurfaceModel::new();
    dst2.apply(&Edit::AddEdges {
        edges: vec![[[500.0, -500.0, 0.0], [500.0, 500.0, 0.0]]],
    })
    .unwrap();
    dst2.absorb(&src, None).unwrap();
    assert!(dst2.problem().is_none());
    assert!(dst2.curves().count() <= 1);
}

#[test]
fn absorb_refuses_what_the_edits_refuse() {
    let mut a = SurfaceModel::new();
    a.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    let before = a.clone();
    let mut b = SurfaceModel::new();
    b.apply(&square(0.0, 0.0, 1000.0)).unwrap();
    let r = a.absorb(&b, Some(DAffine3::from_scale(DVec3::splat(f64::NAN))));
    assert!(r.is_err());
    assert_eq!(a, before, "an error changes nothing");
}
