//! A pad with a retaining edge keeps its wall in step (see `pad.retaining_edge` in the API).
//!
//! The pad has the data key `retaining_edge` = `{wall, top_offset, mode}`, and its wall has the key
//! `retaining_pad`. When the pad moves or changes, the wall follows its outline (and, for a pad that is
//! filled up, its top follows the level of the pad). When the pad is deleted, its wall goes too.

use super::*;
use crate::site::ModifierKind;
use crate::uid::AsUid;

/// The data key of the pad.
pub const PAD_KEY: &str = "retaining_edge";
/// The data key of the wall.
pub const WALL_KEY: &str = "retaining_pad";

impl Tx<'_> {
    pub(super) fn retaining_after(&mut self, cmd: &Command) -> Result<(), CommandError> {
        match cmd {
            Command::DeleteObjects { ids } => {
                let gone: HashSet<ObjectId> = ids.iter().copied().collect();
                let walls: Vec<ObjectId> = self
                    .doc
                    .objects
                    .values()
                    .filter(|o| {
                        o.data
                            .get(WALL_KEY)
                            .and_then(AsUid::as_uid)
                            .is_some_and(|p| gone.contains(&ObjectId(p)))
                            && !gone.contains(&o.id)
                    })
                    .map(|o| o.id)
                    .collect();
                if !walls.is_empty() {
                    self.run(
                        &Command::DeleteObjects { ids: walls },
                        &mut Outcome::default(),
                    )?;
                }
            }
            Command::MoveObjects { ids, .. } => {
                for id in ids.clone() {
                    self.retaining_sync(id);
                }
            }
            Command::ModifyObject { id, .. } | Command::SetVertex { id, .. } => {
                self.retaining_sync(*id)
            }
            _ => {}
        }
        Ok(())
    }

    /// Makes the wall of a pad follow the pad.
    pub(super) fn retaining_sync(&mut self, pad: ObjectId) {
        let Some(o) = self.doc.object(pad) else {
            return;
        };
        let Shape::SiteModifier {
            modifier: ModifierKind::Pad { elevation, .. },
            boundary,
        } = &o.shape
        else {
            return;
        };
        let Some(link) = o.data.get(PAD_KEY) else {
            return;
        };
        let Some(wall) = link.get("wall").and_then(AsUid::as_uid).map(ObjectId) else {
            return;
        };
        let top_offset = link
            .get("top_offset")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let fill = link.get("mode").and_then(Value::as_str) != Some("cut");
        let (level, boundary) = (*elevation, boundary.clone());
        let ok = self.doc.object(wall).is_some_and(|w| {
            w.data.get(WALL_KEY).and_then(AsUid::as_uid) == Some(pad.0)
                && matches!(w.shape, Shape::Polyline { .. })
        });
        if !ok {
            return;
        }
        self.touch(wall);
        let w = self.doc.object_mut(wall).expect("checked");
        w.shape = Shape::Polyline {
            points: boundary,
            closed: true,
        };
        if fill
            && let Some(top) = w
                .data
                .get_mut("landscape_wall")
                .and_then(|d| d.get_mut("top"))
        {
            *top = serde_json::json!({"mode": "fixed", "elevation": level + top_offset});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Vertex;
    use glam::DVec2;
    use serde_json::json;

    fn ring(x: f64) -> Vec<Vertex> {
        [(0.0, 0.0), (4000.0, 0.0), (4000.0, 3000.0), (0.0, 3000.0)]
            .iter()
            .map(|&(a, b)| Vertex::corner(DVec2::new(a + x, b)))
            .collect()
    }

    fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
        ObjectId(
            ed.execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap()
            .created[0],
        )
    }

    fn set(ed: &mut Editor, id: ObjectId, key: &str, value: Value) {
        ed.execute(&Command::SetData {
            id,
            set: [(key.to_string(), value)].into_iter().collect(),
            remove: vec![],
        })
        .unwrap();
    }

    /// A pad at level 2000 with its wall (a polyline with a fixed top), linked both ways.
    fn pad_with_wall(mode: &str) -> (Editor, ObjectId, ObjectId) {
        let mut ed = Editor::with_sequential_ids();
        let pad = add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::Pad {
                    elevation: 2000.0,
                    slope: None,
                },
                boundary: ring(0.0),
            },
        );
        let wall = add(
            &mut ed,
            Shape::Polyline {
                points: ring(0.0),
                closed: true,
            },
        );
        let uid = |id: ObjectId| serde_json::to_value(id).unwrap();
        set(
            &mut ed,
            pad,
            PAD_KEY,
            json!({"wall": uid(wall), "top_offset": 300.0, "mode": mode}),
        );
        set(&mut ed, wall, WALL_KEY, uid(pad));
        set(
            &mut ed,
            wall,
            "landscape_wall",
            json!({"top": {"mode": "fixed", "elevation": 0.0}}),
        );
        (ed, pad, wall)
    }

    fn wall_top(ed: &Editor, wall: ObjectId) -> Value {
        ed.doc().object(wall).unwrap().data["landscape_wall"]["top"].clone()
    }

    #[test]
    fn the_wall_follows_a_pad_that_is_moved_or_changed() {
        let (mut ed, pad, wall) = pad_with_wall("fill");
        ed.execute(&Command::MoveObjects {
            ids: vec![pad],
            delta: DVec2::new(1000.0, 0.0),
        })
        .unwrap();
        let Shape::Polyline { points, closed } = &ed.doc().object(wall).unwrap().shape else {
            panic!("the wall is a polyline");
        };
        assert!(*closed);
        assert_eq!(points[0].p, DVec2::new(1000.0, 0.0));
        // A filled pad: the top of the wall is the level of the pad plus the offset.
        assert_eq!(
            wall_top(&ed, wall),
            json!({"mode": "fixed", "elevation": 2300.0})
        );
        // A new level of the pad moves the top.
        ed.execute(&Command::ModifyObject {
            id: pad,
            shape: Some(Shape::SiteModifier {
                modifier: ModifierKind::Pad {
                    elevation: 2500.0,
                    slope: None,
                },
                boundary: ring(1000.0),
            }),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
        assert_eq!(
            wall_top(&ed, wall),
            json!({"mode": "fixed", "elevation": 2800.0})
        );
        // A moved corner of the pad moves the outline of the wall.
        ed.execute(&Command::SetVertex {
            id: pad,
            index: 2,
            kind: None,
            p: Some(DVec2::new(6000.0, 3500.0)),
        })
        .unwrap();
        let Shape::Polyline { points, .. } = &ed.doc().object(wall).unwrap().shape else {
            panic!("the wall is a polyline");
        };
        assert_eq!(points[2].p, DVec2::new(6000.0, 3500.0));
    }

    #[test]
    fn a_pad_that_is_cut_leaves_the_top_of_the_wall_alone() {
        let (mut ed, pad, wall) = pad_with_wall("cut");
        ed.execute(&Command::MoveObjects {
            ids: vec![pad],
            delta: DVec2::new(500.0, 0.0),
        })
        .unwrap();
        let Shape::Polyline { points, .. } = &ed.doc().object(wall).unwrap().shape else {
            panic!("the wall is a polyline");
        };
        assert_eq!(points[0].p, DVec2::new(500.0, 0.0));
        assert_eq!(
            wall_top(&ed, wall),
            json!({"mode": "fixed", "elevation": 0.0})
        );
    }

    #[test]
    fn a_wall_that_is_not_linked_back_or_not_a_polyline_stays() {
        let (mut ed, pad, wall) = pad_with_wall("fill");
        ed.execute(&Command::SetData {
            id: wall,
            set: Default::default(),
            remove: vec![WALL_KEY.into()],
        })
        .unwrap();
        ed.execute(&Command::MoveObjects {
            ids: vec![pad],
            delta: DVec2::new(700.0, 0.0),
        })
        .unwrap();
        let Shape::Polyline { points, .. } = &ed.doc().object(wall).unwrap().shape else {
            panic!("the wall is a polyline");
        };
        assert_eq!(points[0].p, DVec2::ZERO);
        // An object that is no pad, or a pad without a link, is left alone.
        let line = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(1.0, 0.0),
            },
        );
        ed.execute(&Command::MoveObjects {
            ids: vec![line],
            delta: DVec2::new(1.0, 0.0),
        })
        .unwrap();
        let bare = add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::GradeLimit,
                boundary: ring(0.0),
            },
        );
        ed.execute(&Command::MoveObjects {
            ids: vec![bare],
            delta: DVec2::new(1.0, 0.0),
        })
        .unwrap();
    }

    #[test]
    fn the_wall_goes_with_its_pad() {
        let (mut ed, pad, wall) = pad_with_wall("fill");
        ed.execute(&Command::DeleteObjects { ids: vec![pad] })
            .unwrap();
        assert!(ed.doc().object(wall).is_none());
        // Deleting the wall alone leaves the pad.
        let (mut ed, pad, wall) = pad_with_wall("fill");
        ed.execute(&Command::DeleteObjects { ids: vec![wall] })
            .unwrap();
        assert!(ed.doc().object(pad).is_some());
        // Both in one call: nothing is deleted twice.
        let (mut ed, pad, wall) = pad_with_wall("fill");
        ed.execute(&Command::DeleteObjects {
            ids: vec![pad, wall],
        })
        .unwrap();
        assert!(ed.doc().objects.is_empty());
    }
}
