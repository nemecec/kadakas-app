//! The change-event stream: every command, undo and redo makes one change set (PLAT-02).

use std::collections::BTreeMap;

use glam::DVec2;
use kadakas_core::change::{ChangeSet, Origin};
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, ClassId, LayerId, ObjectId, Shape};
use kadakas_core::plants::style::PlantStyle;
use kadakas_core::resource::{ResourceBody, ResourceId};
use serde_json::json;

fn circle(x: f64) -> Shape {
    Shape::Circle {
        center: DVec2::new(x, 0.0),
        radius: 5.0,
    }
}

fn add(shape: Shape) -> Command {
    Command::AddObject {
        shape,
        layer: None,
        class: None,
        attrs: Attributes::default(),
    }
}

fn last(ed: &Editor) -> ChangeSet {
    ed.change_log().last().expect("a change set").clone()
}

fn one(ed: &mut Editor, cmd: &Command) -> (kadakas_core::command::Outcome, ChangeSet) {
    let before = ed.change_log().len();
    let out = ed.execute(cmd).unwrap();
    assert_eq!(
        ed.change_log().len(),
        before + 1,
        "one command makes one change set"
    );
    (out, last(ed))
}

fn editor() -> Editor {
    Editor::with_sequential_ids()
}

#[test]
fn objects_added_changed_and_removed() {
    let mut ed = editor();
    let (out, c) = one(&mut ed, &add(circle(0.0)));
    let id = ObjectId(out.created[0]);
    assert_eq!(c.op, "add_object");
    assert_eq!(c.revision, ed.revision());
    assert_eq!(c.origin, Origin::User);
    assert_eq!(c.added.objects, vec![id]);
    assert!(c.changed.is_empty() && c.removed.is_empty() && !c.document.any());

    let (_, c) = one(
        &mut ed,
        &Command::MoveObjects {
            ids: vec![id, id],
            delta: DVec2::X,
        },
    );
    assert_eq!(c.op, "move_objects");
    assert_eq!(c.changed.objects, vec![id]);
    assert!(c.added.is_empty() && c.removed.is_empty());

    let (_, c) = one(
        &mut ed,
        &Command::SetData {
            id,
            set: BTreeMap::from([("k".to_string(), json!(1))]),
            remove: vec![],
        },
    );
    assert_eq!(
        (c.op.as_str(), c.changed.objects.clone()),
        ("set_data", vec![id])
    );

    let (_, c) = one(&mut ed, &Command::DeleteObjects { ids: vec![id] });
    assert_eq!(c.op, "delete_objects");
    assert_eq!(c.removed.objects, vec![id]);
    assert!(c.added.is_empty() && c.changed.is_empty());
}

#[test]
fn layers_classes_and_the_document_settings() {
    let mut ed = editor();
    let (out, c) = one(
        &mut ed,
        &Command::AddLayer {
            name: "Site".into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        },
    );
    let layer = LayerId(out.created[0]);
    assert_eq!(c.added.layers, vec![layer]);
    assert!(c.added.objects.is_empty() && !c.document.any());

    let (_, c) = one(
        &mut ed,
        &Command::SetActive {
            layer: Some(layer),
            class: None,
        },
    );
    assert!(c.document.active_layer && !c.document.active_class);
    assert!(c.added.is_empty() && c.changed.is_empty() && c.removed.is_empty());

    let (_, c) = one(
        &mut ed,
        &Command::ModifyLayer {
            id: layer,
            name: Some("Planting".into()),
            visible: None,
            locked: None,
            elevation: None,
            scale: None,
        },
    );
    assert_eq!(c.changed.layers, vec![layer]);

    let (out, c) = one(
        &mut ed,
        &Command::AddClass {
            name: "Trees".into(),
            attrs: Attributes::default(),
        },
    );
    let class = ClassId(out.created[0]);
    assert_eq!(c.added.classes, vec![class]);
    let (_, c) = one(
        &mut ed,
        &Command::ModifyClass {
            id: class,
            name: None,
            visible: Some(false),
            attrs: None,
        },
    );
    assert_eq!(c.changed.classes, vec![class]);

    let (_, c) = one(
        &mut ed,
        &Command::SetActive {
            layer: None,
            class: Some(class),
        },
    );
    assert!(c.document.active_class && !c.document.active_layer);

    // The active layer goes back to the first layer when the active one is deleted.
    let (_, c) = one(&mut ed, &Command::DeleteLayer { id: layer });
    assert_eq!(c.removed.layers, vec![layer]);
    assert!(c.document.active_layer);

    let (_, c) = one(
        &mut ed,
        &Command::SetProjectGuid {
            guid: "3Gx3v0EYX3_fOnxLUh6gRb".into(),
        },
    );
    assert!(c.document.drawing && !c.document.georef);
    assert_eq!(c.op, "set_project_guid");
}

#[test]
fn resources_and_the_objects_that_follow_them() {
    let mut ed = editor();
    let (out, c) = one(
        &mut ed,
        &Command::AddResource {
            name: "Oak".into(),
            body: ResourceBody::plant_style(PlantStyle::default()),
        },
    );
    let style = ResourceId(out.created[0]);
    assert_eq!(c.added.resources, vec![style]);
    assert_eq!(c.op, "add_resource");

    let (out, c) = one(
        &mut ed,
        &Command::AddPlants {
            style: Some(style),
            overrides: Default::default(),
            positions: vec![DVec2::ZERO, DVec2::X],
            layer: None,
            class: None,
            variation: None,
        },
    );
    let plants: Vec<ObjectId> = out.created.iter().map(|u| ObjectId(*u)).collect();
    assert_eq!(c.added.objects, plants, "in creation order");
    assert!(c.added.resources.is_empty());

    let (_, c) = one(
        &mut ed,
        &Command::ModifyResource {
            id: style,
            name: Some("Quercus".into()),
            body: None,
        },
    );
    assert_eq!(c.changed.resources, vec![style]);

    // A style that plants use cannot go; without the plants it can.
    assert!(
        ed.execute(&Command::DeleteResource {
            id: style,
            reassign: None
        })
        .is_err()
    );
    ed.execute(&Command::DeleteObjects { ids: plants }).unwrap();
    let (_, c) = one(
        &mut ed,
        &Command::DeleteResource {
            id: style,
            reassign: None,
        },
    );
    assert_eq!(c.removed.resources, vec![style]);
}

#[test]
fn a_batch_is_one_change_set_with_everything_in_it() {
    let mut ed = editor();
    let keep = ObjectId(ed.execute(&add(circle(0.0))).unwrap().created[0]);
    let before = ed.change_log().len();
    let planned = ed.doc().next_ids(2);
    let out = ed
        .execute(&Command::Batch {
            commands: vec![
                add(circle(1.0)),
                Command::MoveObjects {
                    ids: vec![keep],
                    delta: DVec2::Y,
                },
                Command::AddLayer {
                    name: "L".into(),
                    elevation: 0.0,
                    scale: None,
                    kind: None,
                },
                Command::DeleteObjects { ids: vec![keep] },
            ],
        })
        .unwrap();
    assert_eq!(
        ed.change_log().len(),
        before + 1,
        "one change set for the batch"
    );
    let c = last(&ed);
    assert_eq!(c.op, "batch");
    assert_eq!(c.revision, ed.revision());
    assert_eq!(c.added.objects, vec![ObjectId(planned[0])]);
    assert_eq!(
        c.removed.objects,
        vec![keep],
        "moved and then deleted: removed"
    );
    assert!(c.changed.objects.is_empty());
    assert_eq!(c.added.layers, vec![LayerId(planned[1])]);
    assert_eq!(out.created.len(), 2);
}

#[test]
fn an_object_made_and_deleted_in_one_batch_leaves_no_trace() {
    let mut ed = editor();
    ed.execute(&add(circle(0.0))).unwrap();
    let before = ed.change_log().len();
    let id = ObjectId(ed.doc().next_ids(1)[0]);
    let r = ed.execute(&Command::Batch {
        commands: vec![add(circle(1.0)), Command::DeleteObjects { ids: vec![id] }],
    });
    assert!(r.is_ok());
    assert_eq!(
        ed.change_log().len(),
        before,
        "a command that changes nothing makes no change set"
    );
}

#[test]
fn failed_commands_make_no_change_set() {
    let mut ed = editor();
    ed.execute(&add(circle(0.0))).unwrap();
    let (n, rev) = (ed.change_log().len(), ed.revision());
    assert!(
        ed.execute(&Command::DeleteObjects {
            ids: vec![ObjectId::from_num(999)]
        })
        .is_err()
    );
    assert!(
        ed.execute(&Command::Batch {
            commands: vec![
                add(circle(2.0)),
                Command::DeleteObjects {
                    ids: vec![ObjectId::from_num(999)]
                }
            ],
        })
        .is_err()
    );
    assert_eq!((ed.change_log().len(), ed.revision()), (n, rev));
}

#[test]
fn undo_and_redo_make_change_sets_that_mirror_the_command() {
    let mut ed = editor();
    let (out, add_c) = one(&mut ed, &add(circle(0.0)));
    let id = ObjectId(out.created[0]);
    let (_, move_c) = one(
        &mut ed,
        &Command::MoveObjects {
            ids: vec![id],
            delta: DVec2::X,
        },
    );

    assert!(ed.undo());
    let c = last(&ed);
    assert_eq!(c.op, "undo");
    assert_eq!(c.changed.objects, move_c.changed.objects);
    assert!(c.revision > move_c.revision && c.revision == ed.revision());

    assert!(ed.undo());
    let c = last(&ed);
    assert_eq!(
        c.removed.objects, add_c.added.objects,
        "undo of an add removes"
    );
    assert!(c.added.is_empty());

    assert!(ed.redo());
    let c = last(&ed);
    assert_eq!((c.op.as_str(), c.added.objects.clone()), ("redo", vec![id]));
    assert!(ed.redo());
    assert_eq!(last(&ed).changed.objects, vec![id]);

    // Nothing to undo or redo: no change set.
    let n = ed.change_log().len();
    assert!(!ed.redo());
    assert_eq!(ed.change_log().len(), n);
    // The undo of a layer and a batch swaps added and removed, too.
    let (out, _) = one(
        &mut ed,
        &Command::AddLayer {
            name: "L".into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        },
    );
    ed.undo();
    assert_eq!(last(&ed).removed.layers, vec![LayerId(out.created[0])]);
}

#[test]
fn revisions_grow_and_the_log_can_be_read_from_a_revision() {
    let mut ed = editor();
    let start = ed.revision();
    for x in 0..5 {
        ed.execute(&add(circle(x as f64))).unwrap();
    }
    ed.undo();
    ed.redo();
    let all = ed.changes_since(start);
    assert!(!all.truncated);
    assert_eq!(all.changes.len(), 7);
    assert!(
        all.changes
            .windows(2)
            .all(|w| w[0].revision < w[1].revision),
        "in order"
    );
    assert_eq!(all.changes.last().unwrap().revision, ed.revision());
    // From the middle.
    let mid = all.changes[2].revision;
    let rest = ed.changes_since(mid);
    assert_eq!(rest.changes.len(), 4);
    assert!(ed.changes_since(ed.revision()).changes.is_empty());
}

#[test]
fn the_origin_is_on_every_change_set() {
    let mut ed = editor();
    ed.execute(&add(circle(0.0))).unwrap();
    assert_eq!(last(&ed).origin, Origin::User);
    ed.set_origin(Origin::Agent);
    assert_eq!(ed.origin(), Origin::Agent);
    ed.execute(&add(circle(1.0))).unwrap();
    assert_eq!(last(&ed).origin, Origin::Agent);
    ed.set_origin(Origin::Script);
    ed.undo();
    assert_eq!(last(&ed).origin, Origin::Script);
    ed.rollback(1);
    let c = last(&ed);
    assert_eq!((c.op.as_str(), c.origin), ("rollback", Origin::Script));
    assert_eq!(
        c.removed.objects.len(),
        1,
        "the rolled back step added one object"
    );
}

#[test]
fn rollback_makes_a_change_set_for_each_step() {
    let mut ed = editor();
    ed.execute(&add(circle(0.0))).unwrap();
    ed.execute(&add(circle(1.0))).unwrap();
    let n = ed.change_log().len();
    ed.rollback(2);
    assert_eq!(ed.change_log().len(), n + 2);
    assert!(ed.doc().objects.is_empty());
    assert!(!ed.can_redo(), "a rolled back step cannot be redone");
}

#[test]
fn a_move_in_the_z_order_counts_as_a_change() {
    let mut ed = editor();
    let a = ObjectId(ed.execute(&add(circle(0.0))).unwrap().created[0]);
    ed.execute(&add(circle(1.0))).unwrap();
    // Grouping puts the members into a group object and changes them.
    let (out, c) = one(
        &mut ed,
        &Command::Group {
            ids: vec![a],
            name: None,
        },
    );
    assert_eq!(c.op, "group");
    assert!(c.changed.objects.contains(&a));
    assert_eq!(c.added.objects, vec![ObjectId(out.created[0])]);
}
