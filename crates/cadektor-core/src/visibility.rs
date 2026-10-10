//! Visibility modes of layers and classes: visible, grey and invisible.
//!
//! A layer or class that is *grey* shows in a faded grey and cannot be picked or edited. It is
//! visible in the sense of `Layer::visible` and `Class::visible`, so exports and 3D views draw it
//! as usual. The grey flags live in `Drawing::grey`, so older files and tools that build layers
//! and classes by hand do not change.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::{ClassId, Document, LayerId, Object};

/// How a layer or class shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisMode {
    /// Drawn normally, can be picked and edited.
    Visible,
    /// Drawn in grey, cannot be picked or edited. Snaps only if the snapping option allows it.
    Grey,
    /// Not drawn.
    Invisible,
}

impl VisMode {
    pub fn id(self) -> &'static str {
        match self {
            VisMode::Visible => "visible",
            VisMode::Grey => "grey",
            VisMode::Invisible => "invisible",
        }
    }

    pub fn from_name(s: &str) -> Option<VisMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "visible" => Some(VisMode::Visible),
            "grey" | "gray" => Some(VisMode::Grey),
            "invisible" | "hidden" => Some(VisMode::Invisible),
            _ => None,
        }
    }
}

/// The layers and classes that are grey.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GreySet {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub layers: BTreeSet<LayerId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub classes: BTreeSet<ClassId>,
}

impl Document {
    /// The mode of a layer.
    pub fn layer_mode(&self, id: LayerId) -> Option<VisMode> {
        let l = self.layer(id)?;
        Some(if !l.visible {
            VisMode::Invisible
        } else if self.drawing.grey.layers.contains(&id) {
            VisMode::Grey
        } else {
            VisMode::Visible
        })
    }

    /// The mode of a class.
    pub fn class_mode(&self, id: ClassId) -> Option<VisMode> {
        let c = self.class(id)?;
        Some(if !c.visible {
            VisMode::Invisible
        } else if self.drawing.grey.classes.contains(&id) {
            VisMode::Grey
        } else {
            VisMode::Visible
        })
    }

    /// True if the object shows in grey: its layer or its class is grey.
    pub fn is_grey(&self, obj: &Object) -> bool {
        self.drawing.grey.layers.contains(&obj.layer)
            || self.drawing.grey.classes.contains(&obj.class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::model::{Attributes, FIRST_LAYER, NONE_CLASS, ObjectId, Shape};
    use glam::DVec2;

    /// Adds a layer and makes it the active one, so that the first layer may be grey.
    fn other_active(ed: &mut Editor) {
        let id = ed
            .execute(&Command::AddLayer {
                name: "Work".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::SetActive {
            layer: Some(crate::model::LayerId(id)),
            class: None,
        })
        .unwrap();
    }

    fn line(ed: &mut Editor) -> ObjectId {
        ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::X,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        )
    }

    #[test]
    fn modes_follow_the_command_and_undo() {
        let mut ed = Editor::default();
        let id = line(&mut ed);
        other_active(&mut ed);
        assert_eq!(ed.doc().layer_mode(FIRST_LAYER), Some(VisMode::Visible));
        ed.execute(&Command::SetVisibility {
            layers: vec![FIRST_LAYER],
            classes: vec![],
            mode: VisMode::Grey,
        })
        .unwrap();
        let o = ed.doc().object(id).unwrap();
        assert!(ed.doc().is_grey(o));
        assert!(ed.doc().is_visible(o), "grey objects are still drawn");
        assert!(!ed.doc().is_pickable(o), "but cannot be picked");
        assert_eq!(ed.doc().layer_mode(FIRST_LAYER), Some(VisMode::Grey));
        ed.execute(&Command::SetVisibility {
            layers: vec![FIRST_LAYER],
            classes: vec![],
            mode: VisMode::Invisible,
        })
        .unwrap();
        assert_eq!(ed.doc().layer_mode(FIRST_LAYER), Some(VisMode::Invisible));
        assert!(!ed.doc().is_visible(ed.doc().object(id).unwrap()));
        ed.undo();
        assert_eq!(ed.doc().layer_mode(FIRST_LAYER), Some(VisMode::Grey));
        ed.undo();
        assert_eq!(ed.doc().layer_mode(FIRST_LAYER), Some(VisMode::Visible));
        assert!(ed.doc().is_pickable(ed.doc().object(id).unwrap()));
    }

    #[test]
    fn classes_and_errors() {
        let mut ed = Editor::default();
        let id = line(&mut ed);
        ed.execute(&Command::SetVisibility {
            layers: vec![],
            classes: vec![NONE_CLASS],
            mode: VisMode::Grey,
        })
        .unwrap();
        assert!(ed.doc().is_grey(ed.doc().object(id).unwrap()));
        assert_eq!(ed.doc().class_mode(NONE_CLASS), Some(VisMode::Grey));
        ed.execute(&Command::SetVisibility {
            layers: vec![],
            classes: vec![NONE_CLASS],
            mode: VisMode::Visible,
        })
        .unwrap();
        assert!(ed.doc().drawing.grey.classes.is_empty());
        assert!(ed.doc().drawing.is_empty(), "no grey: nothing is saved");
        for bad in [
            Command::SetVisibility {
                layers: vec![],
                classes: vec![],
                mode: VisMode::Grey,
            },
            Command::SetVisibility {
                layers: vec![LayerId(99)],
                classes: vec![],
                mode: VisMode::Grey,
            },
            Command::SetVisibility {
                layers: vec![],
                classes: vec![ClassId(99)],
                mode: VisMode::Grey,
            },
        ] {
            assert!(ed.execute(&bad).is_err());
        }
        assert_eq!(ed.doc().layer_mode(LayerId(99)), None);
        assert_eq!(VisMode::from_name("Gray"), Some(VisMode::Grey));
        assert_eq!(VisMode::from_name("nope"), None);
        assert_eq!(VisMode::Invisible.id(), "invisible");
    }

    #[test]
    fn old_files_load_and_grey_survives_a_round_trip() {
        let mut ed = Editor::default();
        other_active(&mut ed);
        ed.execute(&Command::SetVisibility {
            layers: vec![FIRST_LAYER],
            classes: vec![],
            mode: VisMode::Grey,
        })
        .unwrap();
        let text = serde_json::to_string(ed.doc()).unwrap();
        let back = Document::from_json(&text).unwrap();
        assert_eq!(back.layer_mode(FIRST_LAYER), Some(VisMode::Grey));
        let mut plain = serde_json::to_value(Editor::default().doc()).unwrap();
        plain.as_object_mut().unwrap().remove("drawing");
        assert!(Document::from_value(plain).is_ok());
    }

    #[test]
    fn grey_objects_cannot_be_edited_and_the_active_layer_cannot_be_grey() {
        let mut ed = Editor::default();
        let id = line(&mut ed);
        let grey = |ed: &mut Editor, layers: Vec<crate::model::LayerId>, classes: Vec<ClassId>| {
            ed.execute(&Command::SetVisibility {
                layers,
                classes,
                mode: VisMode::Grey,
            })
        };
        // The active layer cannot go grey.
        let err = grey(&mut ed, vec![FIRST_LAYER], vec![]).unwrap_err();
        assert!(err.to_string().contains("active layer"), "{err}");
        other_active(&mut ed);
        grey(&mut ed, vec![FIRST_LAYER], vec![]).unwrap();
        // Commands refuse the objects of a grey layer.
        for cmd in [
            Command::MoveObjects {
                ids: vec![id],
                delta: DVec2::X,
            },
            Command::DeleteObjects { ids: vec![id] },
            Command::RotateObjects {
                ids: vec![id],
                center: DVec2::ZERO,
                angle: 1.0,
            },
        ] {
            let err = ed.execute(&cmd).unwrap_err();
            assert!(err.to_string().contains("grey"), "{err}");
        }
        // A grey layer cannot be made active, and nothing can be added to it.
        assert!(
            ed.execute(&Command::SetActive {
                layer: Some(FIRST_LAYER),
                class: None
            })
            .is_err()
        );
        assert!(
            ed.execute(&Command::AddObject {
                shape: Shape::Line {
                    a: DVec2::ZERO,
                    b: DVec2::X
                },
                layer: Some(FIRST_LAYER),
                class: None,
                attrs: Attributes::default(),
            })
            .is_err()
        );
        // Visible again: editable.
        ed.execute(&Command::SetVisibility {
            layers: vec![FIRST_LAYER],
            classes: vec![],
            mode: VisMode::Visible,
        })
        .unwrap();
        ed.execute(&Command::MoveObjects {
            ids: vec![id],
            delta: DVec2::X,
        })
        .unwrap();
        // A grey class: the objects in it cannot be edited.
        grey(&mut ed, vec![], vec![NONE_CLASS]).unwrap();
        assert!(
            ed.execute(&Command::MoveObjects {
                ids: vec![id],
                delta: DVec2::X
            })
            .unwrap_err()
            .to_string()
            .contains("grey class")
        );
    }
}
