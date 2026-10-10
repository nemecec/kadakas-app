//! Applying the effects of a tool to a document. Any UI program can use this, so it does not need
//! to know how a new object becomes a member of the group you are in, or how it gets selected.

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Document, ObjectId};

use crate::tools::Effect;

/// The command to run for `cmd` while you are in the group `context`: a new object becomes a
/// member of the group (also in the edit group of a symbol, so it is part of the symbol after you
/// exit). Other commands, and objects for another layer, are unchanged.
pub fn in_context(doc: &Document, cmd: Command, context: Option<ObjectId>) -> Command {
    let group = context.and_then(|g| doc.object(g).map(|o| (g, o.layer)));
    match (&cmd, group) {
        (
            Command::AddObject {
                shape,
                layer,
                class,
                attrs,
            },
            Some((parent, group_layer)),
        ) if layer.unwrap_or(doc.active_layer) == group_layer => Command::AddObjectIn {
            parent,
            shape: shape.clone(),
            class: *class,
            attrs: attrs.clone(),
            id: None,
        },
        _ => cmd,
    }
}

/// True for the commands that add a new object. The new object becomes the selection.
fn adds_object(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::AddObject { .. }
            | Command::AddObjectIn { .. }
            | Command::AddHardscape { .. }
            | Command::AddLandscapeArea { .. }
    )
}

/// Applies the effects of a tool, in order.
///
/// - A command runs in the group `context` and, when it adds objects, selects them.
/// - After each command, objects that are gone leave the selection.
/// - A selection effect replaces the selection.
///
/// Returns the messages for the status line: the notices of the tools and the errors of commands.
pub fn apply(
    effects: Vec<Effect>,
    editor: &mut Editor,
    selection: &mut Vec<ObjectId>,
    context: Option<ObjectId>,
) -> Vec<String> {
    let mut messages = Vec::new();
    for effect in effects {
        match effect {
            Effect::SetSelection(s) => *selection = s,
            Effect::Notice(m) => messages.push(m),
            Effect::Exec(cmd) => {
                let cmd = in_context(editor.doc(), cmd, context);
                match editor.execute(&cmd) {
                    Ok(out) => {
                        let doc = editor.doc();
                        selection.retain(|id| doc.object(*id).is_some());
                        if adds_object(&cmd) && !out.created.is_empty() {
                            *selection = out.created.into_iter().map(ObjectId).collect();
                        }
                    }
                    Err(e) => messages.push(format!("Error: {e}")),
                }
            }
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::model::{Attributes, Shape};

    fn add_group(ed: &mut Editor) -> ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::Group {
                    name: String::new(),
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        ObjectId(out.created[0])
    }

    fn line() -> Command {
        Command::AddObject {
            shape: Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        }
    }

    #[test]
    fn a_new_object_is_selected() {
        let mut ed = Editor::default();
        let mut sel = vec![];
        let msgs = apply(vec![Effect::Exec(line())], &mut ed, &mut sel, None);
        assert!(msgs.is_empty());
        assert_eq!(sel.len(), 1);
        assert!(ed.doc().object(sel[0]).is_some());
    }

    #[test]
    fn inside_a_group_the_new_object_becomes_a_member() {
        let mut ed = Editor::default();
        let g = add_group(&mut ed);
        let mut sel = vec![];
        apply(vec![Effect::Exec(line())], &mut ed, &mut sel, Some(g));
        let made = ed.doc().object(sel[0]).unwrap();
        assert_eq!(made.parent, Some(g));
        // Without the context the same command makes a loose object.
        apply(vec![Effect::Exec(line())], &mut ed, &mut sel, None);
        assert_eq!(ed.doc().object(sel[0]).unwrap().parent, None);
    }

    #[test]
    fn notices_errors_and_selection_effects() {
        let mut ed = Editor::default();
        let mut sel = vec![];
        let missing = ObjectId(kadakas_core::uid::Uid::from_num(99));
        let msgs = apply(
            vec![
                Effect::Notice("hello".into()),
                Effect::SetSelection(vec![missing]),
                Effect::Exec(Command::MoveObjects {
                    ids: vec![missing],
                    delta: DVec2::X,
                }),
            ],
            &mut ed,
            &mut sel,
            None,
        );
        assert_eq!(msgs[0], "hello");
        assert!(msgs[1].starts_with("Error: "), "{msgs:?}");
        assert_eq!(msgs.len(), 2);
        // The selection effect ran in order, and the object is gone anyway.
        assert_eq!(sel, vec![missing]);
    }

    #[test]
    fn objects_that_are_gone_leave_the_selection() {
        let mut ed = Editor::default();
        let mut sel = vec![];
        apply(vec![Effect::Exec(line())], &mut ed, &mut sel, None);
        let id = sel[0];
        apply(
            vec![Effect::Exec(Command::DeleteObjects { ids: vec![id] })],
            &mut ed,
            &mut sel,
            None,
        );
        assert!(sel.is_empty());
    }
}
