//! Property tests: any sequence of commands, undos and redos behaves like a
//! simple model of a history, and the document can always be saved and
//! reopened.

use std::collections::BTreeMap;

use cadektor_core::command::{Command, Editor};
use cadektor_core::model::{Attributes, ClassId, Color, Document, LayerId, ObjectId, Shape, Vertex, VertexKind};
use cadektor_core::DVec2;
use proptest::prelude::*;
use serde_json::{json, Value};

/// A coordinate: usually small, sometimes huge, sometimes not a number.
fn coord() -> impl Strategy<Value = f64> {
    prop_oneof![
        20 => -1e4..1e4f64,
        1 => Just(1e308),
        1 => Just(-1e308),
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
    ]
}

fn point() -> impl Strategy<Value = DVec2> {
    (coord(), coord()).prop_map(|(x, y)| DVec2::new(x, y))
}

/// A vertex of any kind. Radii are sometimes negative or not a number, which commands must refuse.
fn vertex() -> impl Strategy<Value = Vertex> {
    let p = || (-1e4..1e4f64, -1e4..1e4f64).prop_map(|(x, y)| DVec2::new(x, y));
    let kind = prop_oneof![
        4 => Just(VertexKind::Corner),
        4 => (0.0..2000.0f64).prop_map(|radius| VertexKind::Arc { radius }),
        1 => prop_oneof![Just(-5.0), Just(f64::NAN), Just(f64::INFINITY), Just(1e308)].prop_map(|radius| VertexKind::Arc { radius }),
        2 => Just(VertexKind::PointOnArc),
        2 => Just(VertexKind::Bezier),
        2 => Just(VertexKind::Cubic),
    ];
    (p(), kind).prop_map(|(p, kind)| Vertex { p, kind })
}

fn shape() -> impl Strategy<Value = Shape> {
    let p = || (-1e4..1e4f64, -1e4..1e4f64).prop_map(|(x, y)| DVec2::new(x, y));
    prop_oneof![
        8 => (p(), p()).prop_map(|(a, b)| Shape::Line { a, b }),
        8 => (p(), -10.0..500.0f64).prop_map(|(center, radius)| Shape::Circle { center, radius }),
        4 => (p(), 1.0..5000.0f64).prop_map(|(at, spread)| Shape::Plant { at, species: "Oak".into(), spread, height: 1.0 }),
        2 => prop::collection::vec(p(), 0..6)
            .prop_map(|pts| Shape::Hardscape { boundary: cadektor_core::curve::corners(&pts), material: "Gravel".into(), thickness: None, elevation: None }),
        4 => prop::collection::vec(vertex(), 0..7).prop_map(|boundary| Shape::Hardscape { boundary, material: "Gravel".into(), thickness: None, elevation: None }),
        4 => (prop::collection::vec(vertex(), 0..7), any::<bool>())
            .prop_map(|(points, closed)| Shape::Polyline { points, closed }),
        2 => prop::collection::vec(vertex(), 0..7).prop_map(|boundary| Shape::LandscapeArea { boundary, name: "Lawn".into() }),
        4 => (p(), 1.0..100.0f64, -10.0..10.0f64, -10.0..10.0f64)
            .prop_map(|(center, radius, start, end)| Shape::Arc { center, radius, start, end }),
        1 => (point(), point()).prop_map(|(a, b)| Shape::Line { a, b }),
        1 => (point(), prop_oneof![Just(5.0), Just(1e308), Just(f64::NAN)])
            .prop_map(|(center, radius)| Shape::Circle { center, radius }),
    ]
}

fn attrs() -> impl Strategy<Value = Attributes> {
    (
        prop::option::of(any::<[u8; 4]>().prop_map(Color)),
        prop::option::of(prop_oneof![0.0..5.0f64, Just(f64::NAN)]),
    )
        .prop_map(|(pen, line_weight)| Attributes { pen, fill: None, line_weight })
}

fn data() -> impl Strategy<Value = BTreeMap<String, Value>> {
    let key = prop::sample::select(vec!["a", "b", "c"]).prop_map(String::from);
    let value = prop_oneof![Just(json!(1)), Just(json!("x")), Just(json!({"n": [1, 2]}))];
    prop::collection::btree_map(key, value, 0..3)
}

/// Commands with small IDs, so they often hit existing objects and layers.
fn command() -> impl Strategy<Value = Command> {
    let oid = || (0u64..12).prop_map(ObjectId);
    let lid = || (0u64..12).prop_map(LayerId);
    let cid = || (0u64..12).prop_map(ClassId);
    let ids = || prop::collection::vec(oid(), 0..4);
    let leaf = prop_oneof![
        4 => (shape(), prop::option::of(lid()), prop::option::of(cid()), attrs())
            .prop_map(|(shape, layer, class, attrs)| Command::AddObject { shape, layer, class, attrs }),
        2 => ids().prop_map(|ids| Command::DeleteObjects { ids }),
        2 => (ids(), prop_oneof![-100.0..100.0f64, Just(1e308)])
            .prop_map(|(ids, d)| Command::MoveObjects { ids, delta: DVec2::splat(d) }),
        2 => (oid(), prop::option::of(shape()), prop::option::of(lid()), prop::option::of(cid()), prop::option::of(attrs()))
            .prop_map(|(id, shape, layer, class, attrs)| Command::ModifyObject { id, shape, layer, class, attrs }),
        3 => (oid(), 0usize..8, prop::option::of(vertex().prop_map(|v| v.kind)), prop::option::of(point()))
            .prop_map(|(id, index, kind, p)| Command::SetVertex { id, index, kind, p }),
        2 => (oid(), data(), prop::collection::vec(prop::sample::select(vec!["a", "b", "z"]).prop_map(String::from), 0..3))
            .prop_map(|(id, set, remove)| Command::SetData { id, set, remove }),
        1 => "[a-z]{1,6}".prop_map(|name| Command::AddLayer { name, elevation: 0.0, scale: None, kind: None }),
        1 => (lid(), any::<bool>()).prop_map(|(id, locked)| Command::ModifyLayer {
            id, name: None, visible: None, locked: Some(locked), elevation: None, scale: None
        }),
        1 => lid().prop_map(|id| Command::DeleteLayer { id }),
        1 => (lid(), prop_oneof![Just(10.0), Just(f64::NAN)]).prop_map(|(id, e)| Command::ModifyLayer {
            id, name: None, visible: None, locked: None, elevation: Some(e), scale: None
        }),
        1 => ("[a-z]{1,6}", attrs()).prop_map(|(name, attrs)| Command::AddClass { name, attrs }),
        1 => (cid(), any::<bool>(), prop::option::of(attrs()))
            .prop_map(|(id, visible, attrs)| Command::ModifyClass { id, name: None, visible: Some(visible), attrs }),
        1 => lid().prop_map(|id| Command::SetActive { layer: Some(id), class: None }),
        1 => cid().prop_map(|id| Command::SetActive { layer: None, class: Some(id) }),
    ];
    leaf.prop_recursive(2, 8, 4, |inner| prop::collection::vec(inner, 0..4).prop_map(|commands| Command::Batch { commands }))
}

#[derive(Clone, Debug)]
enum Step {
    Do(Command),
    Undo,
    Redo,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        6 => command().prop_map(Step::Do),
        2 => Just(Step::Undo),
        1 => Just(Step::Redo),
    ]
}

/// Document content without the ID counter. Undo keeps the counter on
/// purpose, so IDs are never reused. Objects are a JSON array, so the
/// z-order is part of the comparison.
fn content(doc: &Document) -> Value {
    let mut v = serde_json::to_value(doc).unwrap();
    v.as_object_mut().unwrap().remove("next_id");
    v
}

/// The document can be saved and the file reopens to the same document.
fn assert_saveable(doc: &Document) -> Result<(), TestCaseError> {
    let json = serde_json::to_string(doc).unwrap();
    let back = Document::from_json(&json);
    prop_assert!(back.is_ok(), "saved document does not reopen: {:?}", back.err());
    prop_assert_eq!(&back.unwrap(), doc);
    Ok(())
}

/// Curve code never panics, and finite vertices with small coordinates give finite results.
fn assert_curves_are_sane(doc: &Document) -> Result<(), TestCaseError> {
    for o in doc.objects.values() {
        let (lo, hi) = o.shape.bounds();
        let area = o.shape.area();
        if let Some(vs) = o.shape.vertices()
            && !vs.is_empty()
            && vs.iter().all(|v| v.p.abs().max_element() < 1e5 && !matches!(v.kind, VertexKind::Arc { radius } if radius > 1e6))
        {
            prop_assert!(lo.is_finite() && hi.is_finite(), "bounds {lo:?} {hi:?}");
            prop_assert!(area.is_none_or(|a| a.is_finite() && a >= 0.0), "area {area:?} of {:?}", o.shape);
            // The drawing in SVG has only finite numbers.
            let mut ed = Editor::default();
            let add = Command::AddObject { shape: o.shape.clone(), layer: None, class: None, attrs: Attributes::default() };
            if ed.execute(&add).is_ok() {
                let svg = cadektor_core::svg::render(ed.doc(), 200);
                prop_assert!(!svg.contains("NaN") && !svg.contains("inf"), "{svg}");
            }
        }
    }
    Ok(())
}

proptest! {

    #[test]
    fn undo_all_then_redo_all(cmds in prop::collection::vec(command(), 0..25)) {
        let mut ed = Editor::default();
        let mut states = vec![content(ed.doc())];
        for c in &cmds {
            let before = content(ed.doc());
            match ed.execute(c) {
                Ok(_) if content(ed.doc()) != before => states.push(content(ed.doc())),
                Ok(_) => {}
                // A failed command leaves the document unchanged.
                Err(_) => prop_assert_eq!(content(ed.doc()), before),
            }
            assert_saveable(ed.doc())?;
            assert_curves_are_sane(ed.doc())?;
        }
        for s in states.iter().rev().skip(1) {
            prop_assert!(ed.undo());
            prop_assert_eq!(&content(ed.doc()), s);
        }
        prop_assert!(!ed.undo());
        for s in states.iter().skip(1) {
            prop_assert!(ed.redo());
            prop_assert_eq!(&content(ed.doc()), s);
        }
        prop_assert!(!ed.redo());
    }

    /// Undo, redo and new commands in any order match a plain history list,
    /// and a new command clears the redo steps.
    #[test]
    fn interleaved_undo_redo_and_new_commands(steps in prop::collection::vec(step(), 0..40)) {
        let mut ed = Editor::default();
        let mut states = vec![content(ed.doc())];
        let mut pos = 0usize;
        for s in &steps {
            match s {
                Step::Do(c) => {
                    let before = content(ed.doc());
                    match ed.execute(c) {
                        Ok(_) if content(ed.doc()) != before => {
                            states.truncate(pos + 1);
                            states.push(content(ed.doc()));
                            pos += 1;
                        }
                        Ok(_) => {}
                        Err(_) => prop_assert_eq!(content(ed.doc()), before),
                    }
                }
                Step::Undo => {
                    prop_assert_eq!(ed.undo(), pos > 0);
                    pos = pos.saturating_sub(1);
                }
                Step::Redo => {
                    let can = pos + 1 < states.len();
                    prop_assert_eq!(ed.redo(), can);
                    if can {
                        pos += 1;
                    }
                }
            }
            prop_assert_eq!(&content(ed.doc()), &states[pos]);
            prop_assert_eq!(ed.can_undo(), pos > 0);
            prop_assert_eq!(ed.can_redo(), pos + 1 < states.len());
            assert_saveable(ed.doc())?;
        }
    }

    #[test]
    fn document_json_roundtrip(cmds in prop::collection::vec(command(), 0..15)) {
        let mut ed = Editor::default();
        for c in &cmds {
            let _ = ed.execute(c);
        }
        let json = serde_json::to_string(&ed.doc()).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&back, ed.doc());
    }
}
