use super::*;

fn issue(title: &str) -> Issue {
    Issue {
        title: title.into(),
        ..Issue::default()
    }
}

#[test]
fn a_plain_issue_is_valid() {
    issue("Check the drain").validate().unwrap();
}

#[test]
fn bounds_are_refused_with_a_message() {
    let long = "x".repeat(MAX_TITLE_CHARS + 1);
    assert!(issue(&long).validate().unwrap_err().contains("title"));
    assert!(issue("  ").validate().is_err());
    let mut i = issue("a");
    i.description = "y".repeat(MAX_TEXT_CHARS + 1);
    assert!(i.validate().unwrap_err().contains("description"));
    let mut i = issue("a");
    i.labels = (0..=MAX_LABELS).map(|n| format!("l{n}")).collect();
    assert!(i.validate().unwrap_err().contains("labels"));
    let mut i = issue("a");
    i.labels = vec!["Tree".into(), "tree".into()];
    assert!(i.validate().unwrap_err().contains("twice"));
    let mut i = issue("a");
    i.labels = vec![String::new()];
    assert!(i.validate().is_err());
    let mut i = issue("a");
    i.labels = vec!["z".repeat(MAX_LABEL_CHARS + 1)];
    assert!(i.validate().is_err());
    let mut i = issue("a");
    i.objects = (0..=MAX_OBJECTS as u64).map(ObjectId::from_num).collect();
    assert!(i.validate().is_err());
    let mut i = issue("a");
    i.objects = vec![ObjectId::from_num(1), ObjectId::from_num(1)];
    assert!(i.validate().is_err());
    let mut i = issue("a");
    i.viewpoints = (0..=MAX_VIEWPOINTS as u64)
        .map(|n| Viewpoint {
            id: ViewpointId::from_num(n + 1),
            ..Viewpoint::default()
        })
        .collect();
    assert!(i.validate().is_err());
    let mut i = issue("a");
    i.viewpoints = vec![Viewpoint::default(), Viewpoint::default()];
    assert!(i.validate().unwrap_err().contains("same ID"));
    let mut i = issue("a");
    i.kind = " ".into();
    assert!(i.validate().is_err());
    let mut i = issue("a");
    i.assignee = Some(" ".into());
    assert!(i.validate().is_err());
    let mut i = issue("a\0b");
    assert!(i.validate().unwrap_err().contains("NUL"));
    i.title = "ok".into();
    i.created = -1;
    assert!(i.validate().is_err());
    i.created = 0;
    i.modified = i64::MAX;
    assert!(i.validate().is_err());
}

#[test]
fn numbers_must_be_finite() {
    let mut i = issue("a");
    i.point = Some([0.0, f64::NAN, 0.0]);
    assert!(i.validate().is_err());
    i.point = Some([1e12, 0.0, 0.0]);
    assert!(i.validate().is_err());
    i.point = Some([1.0, 2.0, 3.0]);
    i.validate().unwrap();
    for bad in [
        ViewSpec::Plan {
            center: [f64::INFINITY, 0.0],
            zoom: 1.0,
        },
        ViewSpec::Plan {
            center: [0.0, 0.0],
            zoom: 0.0,
        },
        ViewSpec::Plan {
            center: [1e12, 0.0],
            zoom: 1.0,
        },
        ViewSpec::Camera {
            target: [0.0; 3],
            yaw: f64::NAN,
            pitch: 0.0,
            distance: 1.0,
            perspective: false,
        },
        ViewSpec::Camera {
            target: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            distance: 0.0,
            perspective: true,
        },
        ViewSpec::Camera {
            target: [1e12, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 1.0,
            perspective: true,
        },
    ] {
        i.viewpoints = vec![Viewpoint {
            id: ViewpointId::from_num(1),
            view: Some(bad),
            ..Viewpoint::default()
        }];
        assert!(i.validate().is_err());
    }
}

#[test]
fn viewpoint_bounds() {
    let mut v = Viewpoint {
        id: ViewpointId::from_num(1),
        ..Viewpoint::default()
    };
    v.name = "n".repeat(MAX_NAME_CHARS + 1);
    assert!(v.validate().is_err());
    v.name.clear();
    v.selection = (0..=MAX_OBJECTS as u64).map(ObjectId::from_num).collect();
    assert!(v.validate().is_err());
    v.selection.clear();
    v.layers = (0..=MAX_VISIBILITY as u64)
        .map(|n| (LayerId::from_num(n), true))
        .collect();
    assert!(v.validate().is_err());
    v.layers.clear();
    v.snapshot = Some("abc".into());
    assert!(v.validate().is_err());
    v.snapshot = Some("a".repeat(64));
    v.validate().unwrap();
}

#[test]
fn dates_and_hashes() {
    assert!(is_date("2026-02-28"));
    assert!(!is_date("2026-02-29"));
    assert!(is_date("2028-02-29"));
    assert!(!is_date("2026-13-01"));
    assert!(!is_date("2026-04-31"));
    assert!(!is_date("2026-4-01"));
    assert!(!is_date("abcd-ef-gh"));
    assert!(!is_date("0000-01-01"));
    assert!(is_date("2100-01-31") && !is_date("2100-02-29"));
    assert!(is_hash(&"0".repeat(64)));
    assert!(!is_hash(&"A".repeat(64)));
    assert!(!is_hash("00"));
}

#[test]
fn comments_are_checked() {
    let c = Comment {
        id: CommentId::default(),
        issue: IssueId::from_num(1),
        author: "a".into(),
        origin: Origin::Agent,
        time: 5,
        text: "hello".into(),
        viewpoint: None,
        edited: None,
    };
    c.validate().unwrap();
    let mut b = c.clone();
    b.text = " ".into();
    assert!(b.validate().is_err());
    b.text = "x".repeat(MAX_TEXT_CHARS + 1);
    assert!(b.validate().is_err());
    let mut b = c.clone();
    b.author = "x".repeat(MAX_NAME_CHARS + 1);
    assert!(b.validate().is_err());
    let mut b = c.clone();
    b.edited = Some(-5);
    assert!(b.validate().is_err());
}

#[test]
fn status_names_round_trip() {
    for s in IssueStatus::ALL {
        assert_eq!(IssueStatus::parse(s.name()), Some(s));
        assert_eq!(serde_json::to_value(s).unwrap(), s.name());
    }
    assert_eq!(IssueStatus::parse("nope"), None);
    assert!(IssueStatus::Closed.is_done() && !IssueStatus::InProgress.is_done());
}

#[test]
fn patch_applies_and_clears() {
    let mut i = issue("a");
    i.assignee = Some("bo".into());
    i.due = Some("2026-01-01".into());
    i.point = Some([1.0, 2.0, 3.0]);
    let p = IssuePatch {
        title: Some("b".into()),
        description: Some("d".into()),
        kind: Some("error".into()),
        status: Some(IssueStatus::Closed),
        priority: Some("high".into()),
        assignee: Some(String::new()),
        labels: Some(vec!["x".into()]),
        due: Some(String::new()),
        clear_point: true,
        objects: Some(vec![ObjectId::from_num(7)]),
        ..IssuePatch::default()
    };
    let n = p.apply(&i);
    assert_eq!(n.title, "b");
    assert_eq!(n.kind, "error");
    assert_eq!(n.status, IssueStatus::Closed);
    assert_eq!((n.assignee, n.due, n.point), (None, None, None));
    assert_eq!(n.objects, vec![ObjectId::from_num(7)]);
    let q = IssuePatch {
        point: Some([4.0, 5.0, 6.0]),
        ..IssuePatch::default()
    };
    assert_eq!(q.apply(&i).point, Some([4.0, 5.0, 6.0]));
    assert_eq!(IssuePatch::default().apply(&i), i);
}

#[test]
fn the_store_finds_orders_and_repairs() {
    let mut a = issue("a");
    a.id = IssueId::from_num(1);
    a.viewpoints = vec![Viewpoint {
        id: ViewpointId::from_num(1),
        snapshot: Some("b".repeat(64)),
        ..Viewpoint::default()
    }];
    let mut b = issue("b");
    b.id = IssueId::from_num(2);
    let mk = |n: u64, issue: u64, time: i64| Comment {
        id: CommentId::from_num(n),
        issue: IssueId::from_num(issue),
        author: String::new(),
        origin: Origin::User,
        time,
        text: "t".into(),
        viewpoint: None,
        edited: None,
    };
    let mut s = Issues::from(IssueData {
        issues: vec![a.clone(), b, a.clone()],
        comments: vec![mk(2, 1, 20), mk(1, 1, 10), mk(3, 9, 1), mk(1, 1, 10)],
    });
    assert!(!s.is_empty());
    let notes = s.repair();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(s.len(), 2);
    assert_eq!(s.comment_count(IssueId::from_num(1)), 2);
    let times: Vec<i64> = s
        .comments_of(IssueId::from_num(1))
        .iter()
        .map(|c| c.time)
        .collect();
    assert_eq!(times, vec![10, 20]);
    assert!(s.get(IssueId::from_num(1)).is_some() && s.get(IssueId::from_num(5)).is_none());
    assert!(s.comment(CommentId::from_num(2)).is_some());
    assert!(s.uses_snapshot(&"b".repeat(64)) && !s.uses_snapshot("x"));
    assert_eq!(s.snapshots(), vec!["b".repeat(64).as_str()]);
    assert!(s.approx_bytes() > 0);
    let c = s.clone();
    assert_eq!(c, s);
    s.data_mut().issues.pop();
    assert_ne!(c, s, "a change copies the data");
    assert!(Issues::default().is_empty());
}

#[test]
fn repair_notes_issues_over_the_limits() {
    let mut s = Issues::from(IssueData {
        issues: vec![Issue {
            id: IssueId::from_num(1),
            title: "t".repeat(MAX_TITLE_CHARS + 5),
            ..Issue::default()
        }],
        comments: vec![],
    });
    let notes = s.repair();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("limits"));
    assert_eq!(s.len(), 1, "nothing is dropped");
}

#[test]
fn json_round_trip_skips_empty_fields() {
    let mut i = issue("a");
    i.id = IssueId::from_num(3);
    let v = serde_json::to_value(&i).unwrap();
    assert!(v.get("labels").is_none() && v.get("viewpoints").is_none());
    assert_eq!(v["type"], "issue");
    let back: Issue = serde_json::from_value(v).unwrap();
    assert_eq!(back, i);
    assert!(is_unset(Uid::NIL) && !is_unset(Uid::from_num(2)));
}
