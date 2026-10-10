//! IDs are UUIDs: they survive text and JSON, they are unique in a document and between
//! documents, and every reference in a saved document resolves after it is loaded again.

use std::collections::HashSet;

use cadektor_core::command::{Command, Editor};
use cadektor_core::model::{Attributes, Document, ObjectId, Shape};
use cadektor_core::uid::Uid;
use glam::DVec2;
use proptest::prelude::*;

proptest! {
    #[test]
    fn text_and_json_round_trips(bits in any::<u128>()) {
        let u = Uid::from_u128(bits);
        let text = u.to_string();
        prop_assert_eq!(text.len(), 36);
        prop_assert_eq!(Uid::parse(&text), Some(u));
        prop_assert_eq!(Uid::parse(&text.to_uppercase()), Some(u));
        let json = serde_json::to_string(&u).unwrap();
        prop_assert_eq!(serde_json::from_str::<Uid>(&json).unwrap(), u);
    }

    #[test]
    fn text_order_is_numeric_order(a in any::<u128>(), b in any::<u128>()) {
        let (x, y) = (Uid::from_u128(a), Uid::from_u128(b));
        prop_assert_eq!(x.cmp(&y), x.to_string().cmp(&y.to_string()));
    }

    #[test]
    fn text_that_is_not_an_id_is_refused(s in "\\PC{0,50}") {
        if s.len() != 36 {
            prop_assert_eq!(Uid::parse(&s), None);
        }
    }

    /// Whatever commands run, all IDs differ, the saved document loads again equal, and every
    /// reference (layer, class, parent) resolves in the loaded document.
    #[test]
    fn documents_keep_unique_ids_and_resolving_references(
        steps in prop::collection::vec((0u8..5, -50.0f64..50.0), 1..40)
    ) {
        let mut ed = Editor::default();
        let mut objects: Vec<ObjectId> = Vec::new();
        for (kind, x) in steps {
            let cmd = match kind {
                0 => Command::AddObject {
                    shape: Shape::Circle { center: DVec2::new(x, 0.0), radius: 1.0 + x.abs() },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                },
                1 => Command::AddLayer { name: format!("L{x:.3}"), elevation: 0.0, scale: None, kind: None },
                2 => Command::AddClass { name: format!("C{x:.3}"), attrs: Attributes::default() },
                3 if objects.len() >= 2 => Command::Group { ids: objects[..2].to_vec(), name: None },
                _ => Command::AddObject {
                    shape: Shape::Line { a: DVec2::ZERO, b: DVec2::new(x, 1.0) },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                },
            };
            if let Ok(out) = ed.execute(&cmd) {
                if matches!(cmd, Command::AddObject { .. }) {
                    objects.push(ObjectId(out.created[0]));
                }
            }
        }
        let doc = ed.doc();
        let ids: Vec<Uid> = doc.objects.keys().map(|i| i.0)
            .chain(doc.layers.iter().map(|l| l.id.0))
            .chain(doc.classes.iter().map(|c| c.id.0))
            .collect();
        let unique: HashSet<&Uid> = ids.iter().collect();
        prop_assert_eq!(unique.len(), ids.len());
        let text = serde_json::to_string(doc).unwrap();
        let back = Document::from_json(&text).unwrap();
        prop_assert_eq!(&back, doc);
        for o in back.objects.values() {
            prop_assert!(back.layer(o.layer).is_some());
            prop_assert!(back.class(o.class).is_some());
            if let Some(p) = o.parent {
                prop_assert!(back.object(p).is_some());
            }
        }
    }
}

#[test]
fn two_documents_never_make_the_same_id() {
    let (mut a, mut b) = (Document::default(), Document::default());
    let from_a: HashSet<Uid> = (0..2000).map(|_| a.alloc_id()).collect();
    let from_b: HashSet<Uid> = (0..2000).map(|_| b.alloc_id()).collect();
    assert_eq!((from_a.len(), from_b.len()), (2000, 2000));
    assert!(from_a.is_disjoint(&from_b));
    // The same seed gives the same IDs (a document that is copied makes the same next IDs), and
    // another time gives other IDs.
    a.seed_ids(1_700_000_000_000, 5);
    b.seed_ids(1_700_000_000_000, 5);
    assert_eq!(a.alloc_id(), b.alloc_id());
    b.seed_ids(1_700_000_000_001, 5);
    assert_ne!(a.alloc_id(), b.alloc_id());
}
