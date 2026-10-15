//! Issues and comments in the property model (REV-04): the cut into properties, the ops of a
//! change, the build of a document from a replica, and the checks.

use kadakas_core::command::{Command, Editor};
use kadakas_core::issue::{
    Comment, CommentId, Issue, IssueId, MAX_COMMENTS, Viewpoint, ViewpointId,
};
use kadakas_core::model::Document;
use kadakas_core::uid::Uid;
use kadakas_props::docsync::{
    diff_replicas, flatten, ops_from_change, ops_from_diff, replica_of, unflatten,
};
use kadakas_props::validate::validate_entity;
use kadakas_props::{Kind, Op, Replica, Target, Writer, materialize};
use serde_json::{Value, json};

fn issue(n: u64) -> Command {
    Command::AddIssue {
        issue: Box::new(Issue {
            id: IssueId::from_num(n),
            title: format!("Issue {n}"),
            created: n as i64,
            modified: n as i64,
            labels: vec!["a".into()],
            viewpoints: vec![
                Viewpoint {
                    id: ViewpointId::from_num(n * 10 + 2),
                    time: 2,
                    ..Viewpoint::default()
                },
                Viewpoint {
                    id: ViewpointId::from_num(n * 10 + 1),
                    time: 1,
                    ..Viewpoint::default()
                },
            ],
            ..Issue::default()
        }),
        pictures: vec![],
    }
}

fn comment(issue: u64, n: u64) -> Command {
    Command::AddIssueComment {
        comment: Box::new(Comment {
            id: CommentId::from_num(n),
            issue: IssueId::from_num(issue),
            author: "A".into(),
            origin: Default::default(),
            time: n as i64,
            text: "t".into(),
            viewpoint: None,
            edited: None,
        }),
    }
}

fn w(n: u64) -> Writer {
    Writer {
        client: Uid::from_num(n),
        op: 1,
    }
}

fn target(kind: Kind, n: u64) -> Target {
    Target::new(kind, Uid::from_num(n))
}

#[test]
fn viewpoints_are_one_property_each_and_come_back_in_order() {
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&issue(5)).unwrap();
    let doc = ed.doc().clone();
    let r = replica_of(&doc);
    let e = r.alive(target(Kind::Issue, 5)).unwrap();
    let props = e.visible();
    let keys: Vec<&String> = props
        .keys()
        .filter(|k| k.starts_with("viewpoints."))
        .collect();
    assert_eq!(keys.len(), 2, "{props:?}");
    // Back to JSON: the list is sorted by time, then ID.
    let whole = unflatten(Kind::Issue, &props);
    let list = whole["viewpoints"].as_array().unwrap();
    assert_eq!(list[0]["time"], 1);
    assert_eq!(list[1]["time"], 2);
    // A viewpoint without an ID stays in one property of its own.
    let mut m = serde_json::Map::new();
    m.insert("viewpoints".into(), json!([{"name": "no id"}]));
    let flat = flatten(Kind::Issue, m);
    assert!(flat.contains_key("viewpoints"));
    // A document with these things builds again.
    let m = materialize(&r, &Document::default());
    assert!(m.quarantined.is_empty(), "{:?}", m.quarantined);
    assert_eq!(m.doc.issues, doc.issues);
}

#[test]
fn a_change_becomes_ops_that_make_the_same_document_on_another_replica() {
    let mut ed = Editor::with_sequential_ids();
    let mut other = replica_of(ed.doc());
    for (n, cmd) in [issue(1), comment(1, 7), issue(2)].iter().enumerate() {
        let before = ed.revision();
        ed.execute(cmd).unwrap();
        let cs = ed
            .changes_since(before)
            .changes
            .last()
            .cloned()
            .unwrap()
            .clone();
        let ops = ops_from_change(&other, ed.doc(), &cs);
        assert!(!ops.is_empty(), "step {n}");
        other.apply_all(w(1), &ops);
    }
    let built = materialize(&other, &Document::default());
    assert_eq!(built.doc.issues, ed.doc().issues);
    // Delete the first issue with its comment: the ops delete both.
    let before = ed.revision();
    ed.execute(&Command::DeleteIssue {
        id: IssueId::from_num(1),
    })
    .unwrap();
    let cs = ed
        .changes_since(before)
        .changes
        .last()
        .cloned()
        .unwrap()
        .clone();
    let ops = ops_from_change(&other, ed.doc(), &cs);
    assert_eq!(
        ops.iter()
            .filter(|o| matches!(o, Op::Delete { .. }))
            .count(),
        2
    );
    other.apply_all(w(1), &ops);
    let built = materialize(&other, &Document::default());
    assert_eq!(built.doc.issues, ed.doc().issues);
    // The whole-document diff finds the same.
    let fresh = replica_of(&Document::default());
    let ops = ops_from_diff(&fresh, ed.doc());
    assert!(ops.iter().any(|o| o.target().kind == Kind::Issue));
    let mut copy = fresh.clone();
    copy.apply_all(w(2), &ops);
    assert_eq!(
        materialize(&copy, &Document::default()).doc.issues,
        ed.doc().issues
    );
    assert!(diff_replicas(&copy, &replica_of(ed.doc())).is_empty());
}

#[test]
fn bad_issues_and_comments_are_quarantined_and_orphans_wait() {
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&issue(1)).unwrap();
    ed.execute(&comment(1, 5)).unwrap();
    let mut r = replica_of(ed.doc());
    // An issue that is not valid (a title too long), a comment of an issue that is not there, a
    // comment that points to a viewpoint that is not there, and a thing that does not read.
    r.insert_baseline(
        target(Kind::Issue, 9),
        serde_json::from_value(json!({"title": "x".repeat(500), "type": "issue", "status": "open", "priority": "normal"})).unwrap(),
    );
    r.insert_baseline(
        target(Kind::Issue, 10),
        serde_json::from_value(json!({"title": 5})).unwrap(),
    );
    r.insert_baseline(
        target(Kind::Comment, 20),
        serde_json::from_value(json!({"issue": IssueId::from_num(404), "text": "orphan"})).unwrap(),
    );
    r.insert_baseline(
        target(Kind::Comment, 21),
        serde_json::from_value(json!({"issue": IssueId::from_num(1), "text": "dangling view", "viewpoint": ViewpointId::from_num(404)})).unwrap(),
    );
    r.insert_baseline(
        target(Kind::Comment, 22),
        serde_json::from_value(json!({"issue": IssueId::from_num(1), "text": " "})).unwrap(),
    );
    let m = materialize(&r, &Document::default());
    let bad: Vec<Target> = m.quarantined.iter().map(|q| q.target).collect();
    for n in [9, 10] {
        assert!(bad.contains(&target(Kind::Issue, n)), "{bad:?}");
    }
    for n in [20, 22] {
        assert!(bad.contains(&target(Kind::Comment, n)), "{bad:?}");
    }
    assert_eq!(m.doc.issues.len(), 1);
    let kept = m.doc.issues.comment(CommentId::from_num(21)).unwrap();
    assert_eq!(
        kept.viewpoint, None,
        "a view that is gone is dropped, the text stays"
    );
    assert!(m.doc.issues.comment(CommentId::from_num(20)).is_none());
}

#[test]
fn a_thread_over_the_bound_is_cut_in_the_build_and_kept_in_the_replica() {
    let ed = Editor::with_sequential_ids();
    let mut r = replica_of(ed.doc());
    r.insert_baseline(
        target(Kind::Issue, 1),
        serde_json::from_value(
            json!({"title": "T", "type": "issue", "status": "open", "priority": "normal"}),
        )
        .unwrap(),
    );
    for n in 0..(MAX_COMMENTS as u64 + 3) {
        r.insert_baseline(
            target(Kind::Comment, 100 + n),
            serde_json::from_value(json!({"issue": IssueId::from_num(1), "text": "t", "time": n}))
                .unwrap(),
        );
    }
    let m = materialize(&r, &Document::default());
    assert_eq!(
        m.doc.issues.comment_count(IssueId::from_num(1)),
        MAX_COMMENTS
    );
    assert_eq!(m.quarantined.len(), 3);
}

#[test]
fn validate_entity_checks_issues_and_comments() {
    let id = Uid::from_num(1);
    let ok_issue = json!({"title": "T", "type": "issue", "status": "open", "priority": "normal"});
    let props = |v: Value| v.as_object().unwrap().clone();
    assert_eq!(
        validate_entity(Kind::Issue, id, &props(ok_issue.clone())),
        Ok(())
    );
    let mut bad = ok_issue.clone();
    bad["status"] = json!("nonsense");
    assert!(validate_entity(Kind::Issue, id, &props(bad)).is_err());
    let mut bad = ok_issue;
    bad["title"] = json!("");
    assert!(validate_entity(Kind::Issue, id, &props(bad)).is_err());
    let c = json!({"issue": IssueId::from_num(2), "text": "hello"});
    assert_eq!(validate_entity(Kind::Comment, id, &props(c)), Ok(()));
    let c = json!({"issue": IssueId::from_num(2), "text": ""});
    assert!(validate_entity(Kind::Comment, id, &props(c)).is_err());
    let c = json!({"text": "no issue"});
    assert!(validate_entity(Kind::Comment, id, &props(c)).is_err());
}

#[test]
fn concurrent_edits_of_different_fields_and_comments_all_stay() {
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&issue(1)).unwrap();
    let base = replica_of(ed.doc());
    let (mut a, mut b) = (base.clone(), base);
    let t = target(Kind::Issue, 1);
    let set = |prop: &str, v: Value| Op::Set {
        target: t,
        prop: prop.into(),
        value: v,
    };
    let comment_op = |n: u64, text: &str| Op::Create {
        target: target(Kind::Comment, n),
        props: serde_json::from_value(
            json!({"issue": IssueId::from_num(1), "text": text, "time": n}),
        )
        .unwrap(),
        merge: false,
    };
    // User 1 changes the status and comments; user 2 changes the assignee and comments.
    let u1 = [set("status", json!("resolved")), comment_op(50, "from one")];
    let u2 = [set("assignee", json!("Bo")), comment_op(51, "from two")];
    // The server order is user 1, then user 2; both replicas apply the same order.
    for r in [&mut a, &mut b] {
        r.apply_all(w(1), &u1);
        r.apply_all(w(2), &u2);
    }
    assert_eq!(a.canonical_text(), b.canonical_text());
    let m = materialize(&a, &Document::default());
    let i = m.doc.issues.get(IssueId::from_num(1)).unwrap();
    assert_eq!(i.status.name(), "resolved");
    assert_eq!(i.assignee.as_deref(), Some("Bo"));
    assert_eq!(m.doc.issues.comment_count(IssueId::from_num(1)), 2);
    // Delete wins over an edit: after the issue is deleted its comments wait, not shown.
    a.apply(w(1), &Op::Delete { target: t });
    let m = materialize(&a, &Document::default());
    assert!(m.doc.issues.is_empty());
    assert!(
        m.quarantined
            .iter()
            .any(|q| q.target.kind == Kind::Comment && q.reason.contains("issue"))
    );
    let _ = Replica::new();
}
