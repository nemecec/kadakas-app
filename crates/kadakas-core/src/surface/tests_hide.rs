//! Tests of Hide and Unhide (`Edit::SetHidden`, MOD-05).

use glam::{DAffine3, DVec3};

use super::*;

type P = [f64; 3];

fn poly(m: &mut SurfaceModel, pts: &[P]) {
    let edges = (0..pts.len())
        .map(|i| [pts[i], pts[(i + 1) % pts.len()]])
        .collect();
    m.apply(&Edit::AddEdges { edges }).unwrap();
}

fn square(x0: f64, w: f64) -> Vec<P> {
    vec![
        [x0, 0.0, 0.0],
        [x0 + w, 0.0, 0.0],
        [x0 + w, w, 0.0],
        [x0, w, 0.0],
    ]
}

/// Two squares side by side that share one edge: two faces, seven edges.
fn pair() -> SurfaceModel {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 1000.0));
    poly(&mut m, &square(1000.0, 1000.0));
    assert_eq!((m.face_count(), m.edge_count()), (2, 7));
    m
}

fn hide(m: &mut SurfaceModel, faces: &[FaceId], edges: &[EdgeId], hidden: bool) {
    m.apply(&Edit::SetHidden {
        faces: faces.to_vec(),
        edges: edges.to_vec(),
        hidden,
    })
    .unwrap();
}

#[test]
fn a_face_and_an_edge_are_hidden_and_shown_again() {
    let mut m = pair();
    let f = m.faces().next().unwrap().0;
    let e = m.edges().next().unwrap().0;
    let rev = m.rev();
    hide(&mut m, &[f], &[e], true);
    assert!(m.face_hidden(f) && m.edge_hidden(e));
    assert_eq!((m.hidden_faces(), m.hidden_edges()), (vec![f], vec![e]));
    assert_eq!(m.rev(), rev + 1, "one edit");
    // The geometry is the same: nothing merged, nothing went.
    assert_eq!((m.face_count(), m.edge_count()), (2, 7));
    hide(&mut m, &[f], &[e], false);
    assert!(!m.face_hidden(f) && !m.edge_hidden(e));
    assert!(m.hidden_faces().is_empty() && m.hidden_edges().is_empty());
    assert!(
        m.edge_attrs(e).is_none(),
        "an edge with no other attribute has no map"
    );
    assert_eq!(m.rev(), rev + 2);
}

#[test]
fn hiding_what_is_hidden_already_changes_nothing() {
    let mut m = pair();
    let f = m.faces().next().unwrap().0;
    hide(&mut m, &[f], &[], true);
    let rev = m.rev();
    hide(&mut m, &[f], &[], true);
    assert_eq!(m.rev(), rev, "no edit, no revision");
    hide(&mut m, &[], &[], true);
    assert_eq!(m.rev(), rev);
}

#[test]
fn an_unknown_id_is_refused_and_the_model_stays() {
    let mut m = pair();
    let before = m.clone();
    for edit in [
        Edit::SetHidden {
            faces: vec![FaceId(999)],
            edges: vec![],
            hidden: true,
        },
        Edit::SetHidden {
            faces: vec![],
            edges: vec![EdgeId(999)],
            hidden: true,
        },
    ] {
        assert!(m.apply(&edit).is_err());
        assert_eq!(m, before);
    }
}

#[test]
fn a_hidden_edge_does_not_merge_faces_and_a_soft_edge_is_a_different_mark() {
    let mut m = pair();
    let shared = m
        .edges()
        .map(|(id, _)| id)
        .find(|id| {
            m.faces()
                .filter(|(_, f)| f.loops().any(|l| l.len() >= 3))
                .count()
                == 2
                && m.edge(*id).is_some_and(|e| {
                    let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
                    (a.x - 1000.0).abs() < 1e-9 && (b.x - 1000.0).abs() < 1e-9
                })
        })
        .unwrap();
    hide(&mut m, &[], &[shared], true);
    assert_eq!(m.face_count(), 2, "the faces stay two");
    // The hidden edge is only a mark: the model is still valid and the faces still agree.
    assert!(m.check().is_valid());
    let attrs = m.edge_attrs(shared).unwrap();
    assert_eq!(attrs.get("hidden"), Some(&serde_json::Value::Bool(true)));
    assert!(!attrs.contains_key("soft"));
}

#[test]
fn the_mark_stays_when_the_model_moves() {
    let mut m = pair();
    let f = m.faces().next().unwrap().0;
    hide(&mut m, &[f], &[], true);
    let all: Vec<VertexId> = m.vertices().map(|(id, _)| id).collect();
    m.apply(&Edit::TransformVertices {
        vertices: all,
        faces: vec![],
        edges: vec![],
        matrix: DAffine3::from_translation(DVec3::new(0.0, 0.0, 50.0)).to_cols_array(),
    })
    .unwrap();
    assert_eq!(m.hidden_faces().len(), 1, "a move keeps the attribute");
}

#[test]
fn the_edit_is_in_the_log_and_replays_on_another_model() {
    let base = pair();
    let mut mine = base.clone();
    let f = mine.faces().next().unwrap().0;
    let e = mine.edges().next().unwrap().0;
    hide(&mut mine, &[f], &[e], true);
    assert!(matches!(
        mine.log().last().unwrap().intent,
        sync::Intent::HiddenAt { hidden: true, .. }
    ));
    // The same model with ids that differ: the intent finds the face and the edge by position.
    let mut other = base.clone();
    sync::replay_intent(&mut other, &mine.log().last().unwrap().intent).unwrap();
    assert_eq!(other.hidden_faces().len(), 1);
    assert_eq!(other.hidden_edges().len(), 1);
    assert_eq!(other.hidden_faces(), mine.hidden_faces());
    // The points that the intent holds are not on the model any more: the replay says so.
    let mut empty = SurfaceModel::new();
    assert!(sync::replay_intent(&mut empty, &mine.log().last().unwrap().intent).is_err());
    // Unhide replays too.
    hide(&mut mine, &[f], &[e], false);
    let intent = mine.log().last().unwrap().intent.clone();
    sync::replay_intent(&mut other, &intent).unwrap();
    assert!(other.hidden_faces().is_empty() && other.hidden_edges().is_empty());
}

#[test]
fn the_same_edits_give_the_same_model() {
    let build = || {
        let mut m = pair();
        let ids: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
        hide(&mut m, &ids[..1], &[], true);
        let e = m.edges().next().unwrap().0;
        hide(&mut m, &[], &[e], true);
        m
    };
    assert_eq!(build(), build());
    assert_eq!(
        serde_json::to_string(&build()).unwrap(),
        serde_json::to_string(&build()).unwrap()
    );
}

#[test]
fn a_hidden_face_survives_a_save() {
    let mut m = pair();
    let f = m.faces().next().unwrap().0;
    hide(&mut m, &[f], &[], true);
    let text = serde_json::to_string(&m).unwrap();
    let back: SurfaceModel = serde_json::from_str(&text).unwrap();
    assert_eq!(back.hidden_faces(), vec![f]);
}
