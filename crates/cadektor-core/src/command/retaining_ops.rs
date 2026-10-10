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
    fn retaining_sync(&mut self, pad: ObjectId) {
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
