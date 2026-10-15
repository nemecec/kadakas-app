use kadakas_core::change::Origin;
use kadakas_core::command::{Command, Editor};
use kadakas_core::issue::{IssueData, ViewSpec, ViewpointId};
use kadakas_core::model::{ClassId, LayerId};

use super::*;

fn issue(n: u64, title: &str) -> Issue {
    Issue {
        id: IssueId::from_num(n),
        title: title.into(),
        created: n as i64,
        ..Issue::default()
    }
}

fn store() -> Issues {
    let mut a = issue(1, "Dead tree");
    a.assignee = Some("Ann".into());
    a.labels = vec!["Planting".into()];
    a.objects = vec![ObjectId::from_num(10)];
    let mut b = issue(2, "Broken sprinkler head");
    b.status = IssueStatus::Resolved;
    b.assignee = Some("bo".into());
    b.description = "near the Gate".into();
    b.origin = Origin::Agent;
    b.viewpoints = vec![Viewpoint {
        id: ViewpointId::from_num(1),
        selection: vec![ObjectId::from_num(20)],
        ..Viewpoint::default()
    }];
    let mut c = issue(3, "Wall too low");
    c.status = IssueStatus::Closed;
    c.labels = vec!["hardscape".into(), "planting".into()];
    Issues::from(IssueData {
        issues: vec![a, b, c],
        comments: vec![],
    })
}

fn ids(v: &[&Issue]) -> Vec<u64> {
    v.iter()
        .map(|i| {
            let n = i.id.0.to_string();
            u64::from_str_radix(n.rsplit('-').next().unwrap(), 16).unwrap()
        })
        .collect()
}

#[test]
fn an_empty_filter_shows_all_in_order() {
    let s = store();
    assert_eq!(ids(&select(&s, &Filter::default())), vec![1, 2, 3]);
}

#[test]
fn the_filters_work_alone_and_together() {
    let s = store();
    let f = |f: Filter| ids(&select(&s, &f));
    assert_eq!(
        f(Filter {
            status: vec![IssueStatus::Resolved, IssueStatus::Closed],
            ..Filter::default()
        }),
        vec![2, 3]
    );
    assert_eq!(
        f(Filter {
            open_only: true,
            ..Filter::default()
        }),
        vec![1]
    );
    assert_eq!(
        f(Filter {
            assignee: Some("ANN".into()),
            ..Filter::default()
        }),
        vec![1]
    );
    assert_eq!(
        f(Filter {
            label: Some("PLANTING".into()),
            ..Filter::default()
        }),
        vec![1, 3]
    );
    assert_eq!(
        f(Filter {
            object: Some(ObjectId::from_num(20)),
            ..Filter::default()
        }),
        vec![2],
        "a viewpoint selection counts"
    );
    assert_eq!(
        f(Filter {
            object: Some(ObjectId::from_num(10)),
            ..Filter::default()
        }),
        vec![1]
    );
    assert_eq!(
        f(Filter {
            text: Some("gate".into()),
            ..Filter::default()
        }),
        vec![2],
        "the description counts"
    );
    assert_eq!(
        f(Filter {
            text: Some("TREE".into()),
            ..Filter::default()
        }),
        vec![1]
    );
    assert_eq!(
        f(Filter {
            origin: Some(Origin::Agent),
            ..Filter::default()
        }),
        vec![2]
    );
    assert_eq!(
        f(Filter {
            label: Some("planting".into()),
            status: vec![IssueStatus::Closed],
            ..Filter::default()
        }),
        vec![3]
    );
    assert!(
        f(Filter {
            assignee: Some("nobody".into()),
            ..Filter::default()
        })
        .is_empty()
    );
}

#[test]
fn facets_list_what_is_in_use() {
    let (assignees, labels) = facets(&store());
    assert_eq!(assignees, vec!["Ann", "bo"]);
    assert_eq!(labels, vec!["hardscape", "Planting"]);
}

fn doc_with(layers: &[(&str, bool)]) -> (Document, Vec<LayerId>) {
    let mut ed = Editor::with_sequential_ids();
    let mut ids = Vec::new();
    for (name, visible) in layers {
        let out = ed
            .execute(&Command::AddLayer {
                name: (*name).into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap();
        let id = LayerId(out.created[0]);
        if !*visible {
            ed.execute(&Command::SetVisibility {
                layers: vec![id],
                classes: vec![],
                mode: VisMode::Invisible,
            })
            .unwrap();
        }
        ids.push(id);
    }
    (ed.doc().clone(), ids)
}

#[test]
fn visibility_is_captured_and_put_back() {
    let (doc, ids) = doc_with(&[("A", true), ("B", false), ("C", true)]);
    let (layers, classes) = capture_visibility(&doc);
    assert!(layers[&ids[0]]);
    assert!(!layers[&ids[1]]);
    assert_eq!(classes.len(), doc.classes.len());
    let vp = Viewpoint {
        layers: layers.clone(),
        classes,
        ..Viewpoint::default()
    };
    // The document is as the viewpoint says: no commands.
    assert!(visibility_commands(&doc, &vp).is_empty());
    // Change the document, then put it back with the commands.
    let mut ed = Editor::new(doc.clone());
    ed.execute(&Command::SetVisibility {
        layers: vec![ids[0]],
        classes: vec![],
        mode: VisMode::Invisible,
    })
    .unwrap();
    ed.execute(&Command::SetVisibility {
        layers: vec![ids[1]],
        classes: vec![],
        mode: VisMode::Visible,
    })
    .unwrap();
    let cmds = visibility_commands(ed.doc(), &vp);
    assert_eq!(cmds.len(), 2, "one to show, one to hide");
    ed.execute(&Command::Batch { commands: cmds }).unwrap();
    for id in &ids {
        assert_eq!(
            ed.doc().layer(*id).unwrap().visible,
            doc.layer(*id).unwrap().visible
        );
    }
    // Layers and classes that are gone are left alone.
    let mut vp2 = vp.clone();
    vp2.layers.insert(LayerId::from_num(777), false);
    vp2.classes.insert(ClassId::from_num(777), false);
    assert!(visibility_commands(ed.doc(), &vp2).is_empty());
    // A class that differs.
    let mut vp3 = vp;
    vp3.classes.insert(doc.classes[0].id, false);
    let cmds = visibility_commands(ed.doc(), &vp3);
    assert_eq!(cmds.len(), 1);
}

#[test]
fn the_pin_goes_to_the_point_then_an_object_then_a_plan_view() {
    let mut i = issue(1, "x");
    let pos = |o: ObjectId| (o == ObjectId::from_num(5)).then_some([5.0, 6.0, 7.0]);
    assert_eq!(pin_position(&i, &pos), None);
    i.viewpoints = vec![
        Viewpoint {
            id: ViewpointId::from_num(1),
            view: Some(ViewSpec::Camera {
                target: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                distance: 1.0,
                perspective: false,
            }),
            ..Viewpoint::default()
        },
        Viewpoint {
            id: ViewpointId::from_num(2),
            view: Some(ViewSpec::Plan {
                center: [1.0, 2.0],
                zoom: 1.0,
            }),
            ..Viewpoint::default()
        },
    ];
    assert_eq!(pin_position(&i, &pos), Some([1.0, 2.0, 0.0]));
    i.objects = vec![ObjectId::from_num(4), ObjectId::from_num(5)];
    assert_eq!(pin_position(&i, &pos), Some([5.0, 6.0, 7.0]));
    i.point = Some([9.0, 9.0, 9.0]);
    assert_eq!(pin_position(&i, &pos), Some([9.0, 9.0, 9.0]));
}

#[test]
fn done_follows_the_status() {
    let s = store();
    assert!(!is_done(&s, IssueId::from_num(1)));
    assert!(is_done(&s, IssueId::from_num(2)));
    assert!(!is_done(&s, IssueId::from_num(99)));
}

#[test]
fn deleting_the_work_of_others_needs_consent() {
    let mut mine = issue(1, "x");
    mine.origin = Origin::Agent;
    assert_eq!(consent_reason(&mine, &[], Origin::Agent, ""), None);
    assert!(
        consent_reason(&mine, &[], Origin::User, "")
            .unwrap()
            .contains("agent")
    );
    let c = Comment {
        id: kadakas_core::issue::CommentId::from_num(1),
        issue: mine.id,
        author: String::new(),
        origin: Origin::User,
        time: 0,
        text: "t".into(),
        viewpoint: None,
        edited: None,
    };
    let why = consent_reason(&mine, &[&c], Origin::Agent, "").unwrap();
    assert!(why.contains("1 comment"), "{why}");
    // The same origin with another author is somebody else.
    let mut alice = issue(5, "y");
    alice.origin = Origin::Agent;
    alice.author = "Alice".into();
    assert_eq!(consent_reason(&alice, &[], Origin::Agent, " alice "), None);
    assert!(
        consent_reason(&alice, &[], Origin::Agent, "Bob")
            .unwrap()
            .contains("Alice")
    );
    assert_eq!(
        consent_reason(&alice, &[], Origin::Agent, ""),
        None,
        "no name, nobody to ask for"
    );
    let c2 = Comment {
        author: "Bob".into(),
        origin: Origin::Agent,
        ..c.clone()
    };
    assert!(
        consent_reason(&alice, &[&c2], Origin::Agent, "Alice")
            .unwrap()
            .contains("1 comment")
    );
    let own = Comment {
        origin: Origin::Agent,
        ..c
    };
    assert_eq!(consent_reason(&mine, &[&own], Origin::Agent, ""), None);
}
