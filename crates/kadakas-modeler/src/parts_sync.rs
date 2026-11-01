//! The selected faces and edges and the API (AGR-09). The editor keeps the picked parts in a
//! `SubSelection`. The session keeps them as plain ids (`Session::parts`), which is what
//! `selection.get` reports and `selection.set` can change. This module keeps the two in step:
//! a change by `selection.set` goes to the editor, and a pick of the person goes to the session.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use kadakas_api::{PartSelection, Session};
use kadakas_core::surface::{EdgeId, FaceId};
use kadakas_ui_core::modeling::{Item, SubSelection};

/// What the editor showed to the session last.
#[derive(Default)]
pub struct PartsSync {
    mirrored: Option<u64>,
}

fn key(sub: &SubSelection) -> u64 {
    let mut h = DefaultHasher::new();
    sub.hash(&mut h);
    h.finish()
}

impl PartsSync {
    /// Takes the parts that `selection.set` gave, once. Call it before the editor reads its
    /// selection.
    pub fn apply(&mut self, session: &mut Session, sub: &mut SubSelection) {
        if let Some(parts) = session.take_parts_change() {
            sub.set(
                parts
                    .faces()
                    .map(|(m, f)| Item::Face(m, FaceId(f)))
                    .chain(parts.edges().map(|(m, e)| Item::Edge(m, EdgeId(e)))),
            );
            self.mirrored = Some(key(sub));
        }
    }

    /// Shows the parts of the editor to the session, when they changed.
    pub fn mirror(&mut self, session: &mut Session, sub: &SubSelection) {
        let now = key(sub);
        if self.mirrored == Some(now) {
            return;
        }
        self.mirrored = Some(now);
        session.parts = PartSelection::new(
            sub.faces().into_iter().map(|(m, f)| (m, f.0)),
            sub.edges().into_iter().map(|(m, e)| (m, e.0)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::model::ObjectId;
    use serde_json::json;

    fn model(s: &mut Session) -> (String, u32, u32) {
        let id = s.handle("mesh.create", json!({"name": "M"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let rect = json!([
            [[0, 0, 0], [100, 0, 0]],
            [[100, 0, 0], [100, 100, 0]],
            [[100, 100, 0], [0, 100, 0]],
            [[0, 100, 0], [0, 0, 0]]
        ]);
        s.handle("mesh.add_edges", json!({"id": id, "edges": rect}))
            .unwrap();
        let first = |s: &mut Session, m: &str| {
            s.handle(m, json!({"id": id})).unwrap()["items"][0]["id"]
                .as_u64()
                .unwrap() as u32
        };
        let (f, e) = (first(s, "mesh.faces"), first(s, "mesh.edges"));
        (id, f, e)
    }

    #[test]
    fn a_pick_of_the_person_shows_in_selection_get() {
        let mut s = Session::with_sequential_ids();
        let (id, f, e) = model(&mut s);
        let m: ObjectId = serde_json::from_value(json!(id)).unwrap();
        let (mut sub, mut sync) = (SubSelection::default(), PartsSync::default());
        sync.mirror(&mut s, &sub);
        assert_eq!(
            s.handle("selection.get", json!({})).unwrap(),
            json!({"ids": [], "faces": {}, "edges": {}})
        );
        sub.set([Item::Face(m, FaceId(f)), Item::Edge(m, EdgeId(e))]);
        sync.mirror(&mut s, &sub);
        let got = s.handle("selection.get", json!({})).unwrap();
        assert_eq!(got["faces"], json!({id.clone(): [f]}));
        assert_eq!(got["edges"], json!({id.clone(): [e]}));
        sub.clear();
        sync.mirror(&mut s, &sub);
        assert_eq!(
            s.handle("selection.get", json!({})).unwrap()["faces"],
            json!({})
        );
    }

    #[test]
    fn selection_set_reaches_the_editor_once_and_the_editor_keeps_it() {
        let mut s = Session::with_sequential_ids();
        let (id, f, e) = model(&mut s);
        let (mut sub, mut sync) = (SubSelection::default(), PartsSync::default());
        s.handle(
            "selection.set",
            json!({"faces": {id.clone(): [f]}, "edges": {id.clone(): [e]}}),
        )
        .unwrap();
        sync.apply(&mut s, &mut sub);
        assert_eq!((sub.faces().len(), sub.edges().len()), (1, 1));
        // The mirror does not undo what the call set.
        sync.mirror(&mut s, &sub);
        assert_eq!(
            s.handle("selection.get", json!({})).unwrap()["faces"],
            json!({id.clone(): [f]})
        );
        // Nothing is taken twice: a pick after that stays.
        sub.clear();
        sync.apply(&mut s, &mut sub);
        assert!(sub.is_empty());
        sync.mirror(&mut s, &sub);
        assert!(s.parts.is_empty());
        // ids alone clear the parts of the editor.
        s.handle("selection.set", json!({"faces": {id.clone(): [f]}}))
            .unwrap();
        sync.apply(&mut s, &mut sub);
        assert_eq!(sub.len(), 1);
        s.handle("selection.set", json!({"ids": []})).unwrap();
        sync.apply(&mut s, &mut sub);
        assert!(sub.is_empty());
    }
}
