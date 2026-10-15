//! Issue commands (REV-04): undo and redo, change sets, snapshots as picture resources, limits.

use kadakas_core::change::{ChangeSet, Origin};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::issue::*;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::picture::header_only_sample;

fn editor() -> Editor {
    Editor::with_sequential_ids()
}

fn last(ed: &Editor) -> ChangeSet {
    ed.change_log().last().expect("a change set").clone()
}

fn new_issue(title: &str) -> Command {
    Command::AddIssue {
        issue: Box::new(Issue {
            title: title.into(),
            author: "Ann".into(),
            created: 1_000,
            modified: 1_000,
            ..Issue::default()
        }),
        pictures: vec![],
    }
}

fn add(ed: &mut Editor, title: &str) -> IssueId {
    IssueId(ed.execute(&new_issue(title)).unwrap().created[0])
}

fn comment(issue: IssueId, text: &str, time: i64) -> Command {
    Command::AddIssueComment {
        comment: Box::new(Comment {
            id: CommentId::default(),
            issue,
            author: "Bob".into(),
            origin: Origin::User,
            time,
            text: text.into(),
            viewpoint: None,
            edited: None,
        }),
    }
}

fn modify(id: IssueId, patch: IssuePatch, now: i64) -> Command {
    Command::ModifyIssue {
        id,
        patch: Box::new(patch),
        now,
    }
}

#[test]
fn add_modify_and_delete_are_undoable_steps_with_change_sets() {
    let mut ed = editor();
    let before = ed.doc().clone();
    let id = add(&mut ed, "Dead tree");
    let cs = last(&ed);
    assert_eq!(cs.op, "add_issue");
    assert_eq!(cs.added.issues, vec![id]);
    assert!(cs.touches("issues") && !cs.touches("objects"));
    assert_eq!(ed.doc().issues.get(id).unwrap().title, "Dead tree");

    ed.execute(&modify(
        id,
        IssuePatch {
            status: Some(IssueStatus::InProgress),
            assignee: Some("Cy".into()),
            ..IssuePatch::default()
        },
        2_000,
    ))
    .unwrap();
    let cs = last(&ed);
    assert_eq!(cs.changed.issues, vec![id]);
    let i = ed.doc().issues.get(id).unwrap();
    assert_eq!(
        (i.status, i.assignee.as_deref(), i.modified),
        (IssueStatus::InProgress, Some("Cy"), 2_000)
    );

    ed.execute(&comment(id, "I looked at it", 3_000)).unwrap();
    let cs = last(&ed);
    assert_eq!(cs.added.comments.len(), 1);
    assert_eq!(cs.changed.issues, vec![id], "a comment changes its issue");
    assert_eq!(ed.doc().issues.get(id).unwrap().modified, 3_000);

    ed.execute(&Command::DeleteIssue { id }).unwrap();
    let cs = last(&ed);
    assert_eq!(cs.removed.issues, vec![id]);
    assert_eq!(cs.removed.comments.len(), 1);
    assert!(ed.doc().issues.is_empty());

    // Undo all four steps, then redo them.
    let states: Vec<Document> = {
        let mut v = Vec::new();
        for _ in 0..4 {
            assert!(ed.undo());
            v.push(ed.doc().clone());
        }
        v
    };
    assert_eq!(ed.doc(), &before);
    assert_eq!(states[0].issues.comment_count(id), 1);
    assert_eq!(states[1].issues.comment_count(id), 0);
    assert_eq!(states[2].issues.get(id).unwrap().status, IssueStatus::Open);
    for _ in 0..4 {
        assert!(ed.redo());
    }
    assert!(ed.doc().issues.is_empty());
    assert!(ed.undo());
    assert_eq!(ed.doc().issues.comment_count(id), 1);
    let cs = last(&ed);
    assert_eq!(cs.op, "undo");
    assert_eq!(cs.added.issues, vec![id], "undo of a delete adds the issue");
}

#[test]
fn a_patch_that_changes_nothing_makes_no_step() {
    let mut ed = editor();
    let id = add(&mut ed, "A");
    let depth = ed.undo_depth();
    ed.execute(&modify(
        id,
        IssuePatch {
            title: Some("A".into()),
            ..IssuePatch::default()
        },
        9,
    ))
    .unwrap();
    assert_eq!(ed.undo_depth(), depth);
    assert_eq!(ed.doc().issues.get(id).unwrap().modified, 1_000);
}

#[test]
fn errors_leave_the_document_alone() {
    let mut ed = editor();
    let id = add(&mut ed, "A");
    let depth = ed.undo_depth();
    let ghost = IssueId::from_num(999);
    assert_eq!(
        ed.execute(&Command::DeleteIssue { id: ghost }),
        Err(CommandError::NoIssue(ghost))
    );
    assert!(matches!(
        ed.execute(&modify(ghost, IssuePatch::default(), 1)),
        Err(CommandError::NoIssue(_))
    ));
    assert!(matches!(
        ed.execute(&comment(ghost, "x", 1)),
        Err(CommandError::NoIssue(_))
    ));
    assert!(matches!(
        ed.execute(&Command::EditIssueComment {
            id: CommentId::from_num(5),
            text: "x".into(),
            now: 1
        }),
        Err(CommandError::NoComment(_))
    ));
    assert!(matches!(
        ed.execute(&Command::DeleteIssueComment {
            id: CommentId::from_num(5)
        }),
        Err(CommandError::NoComment(_))
    ));
    for bad in [
        modify(
            id,
            IssuePatch {
                title: Some(" ".into()),
                ..IssuePatch::default()
            },
            2,
        ),
        modify(
            id,
            IssuePatch {
                due: Some("tomorrow".into()),
                ..IssuePatch::default()
            },
            2,
        ),
        comment(id, " ", 2),
        new_issue(&"t".repeat(MAX_TITLE_CHARS + 1)),
    ] {
        assert!(matches!(
            ed.execute(&bad),
            Err(CommandError::Invalid(_))
        ));
    }
    assert_eq!(ed.undo_depth(), depth);
    assert_eq!(ed.doc().issues.len(), 1);
}

#[test]
fn a_given_id_is_kept_and_a_repeat_is_refused() {
    let mut ed = editor();
    let mut issue = Issue {
        id: IssueId::from_num(77),
        title: "From a file".into(),
        ..Issue::default()
    };
    issue.viewpoints = vec![Viewpoint {
        id: ViewpointId::from_num(78),
        ..Viewpoint::default()
    }];
    let cmd = Command::AddIssue {
        issue: Box::new(issue),
        pictures: vec![],
    };
    let out = ed.execute(&cmd).unwrap();
    assert_eq!(
        out.created,
        vec![IssueId::from_num(77).0, ViewpointId::from_num(78).0]
    );
    assert!(matches!(ed.execute(&cmd), Err(CommandError::Invalid(_))));
}

#[test]
fn comments_are_edited_deleted_and_limited() {
    let mut ed = editor();
    let id = add(&mut ed, "A");
    let c = CommentId(ed.execute(&comment(id, "first", 5)).unwrap().created[0]);
    ed.execute(&Command::EditIssueComment {
        id: c,
        text: "changed".into(),
        now: 8,
    })
    .unwrap();
    let got = ed.doc().issues.comment(c).unwrap();
    assert_eq!((got.text.as_str(), got.edited), ("changed", Some(8)));
    let depth = ed.undo_depth();
    ed.execute(&Command::EditIssueComment {
        id: c,
        text: "changed".into(),
        now: 9,
    })
    .unwrap();
    assert_eq!(ed.undo_depth(), depth, "the same text is no change");
    ed.execute(&Command::DeleteIssueComment { id: c }).unwrap();
    assert_eq!(ed.doc().issues.comment_count(id), 0);
    ed.undo();
    assert_eq!(ed.doc().issues.comment_count(id), 1);

    // The bound on the thread.
    for n in 0..(MAX_COMMENTS - 1) {
        ed.execute(&comment(id, "x", 10 + n as i64)).unwrap();
    }
    let err = ed.execute(&comment(id, "one too many", 1)).unwrap_err();
    assert!(err.to_string().contains("comments"), "{err}");
    // A comment with a given ID twice is refused.
    let other = add(&mut ed, "B");
    let mut dup = match comment(other, "dup", 1) {
        Command::AddIssueComment { comment } => comment,
        _ => unreachable!(),
    };
    dup.id = c;
    assert!(
        ed.execute(&Command::AddIssueComment { comment: dup })
            .is_err()
    );
}

#[test]
fn viewpoints_keep_snapshots_as_picture_resources() {
    let mut ed = editor();
    let id = add(&mut ed, "A");
    let pic = header_only_sample(1);
    let hash = pic.sha256.clone();
    let vp = Viewpoint {
        name: "Overview".into(),
        time: 5,
        view: Some(ViewSpec::Plan {
            center: [1.0, 2.0],
            zoom: 0.5,
        }),
        snapshot: Some(hash.clone()),
        ..Viewpoint::default()
    };
    let out = ed
        .execute(&Command::AddIssueViewpoint {
            issue: id,
            viewpoint: Box::new(vp.clone()),
            picture: Some(Box::new(pic.clone())),
            now: 6,
        })
        .unwrap();
    let vid = ViewpointId(out.created[0]);
    assert!(ed.doc().resources.find_image(&hash).is_some());
    assert!(ed.doc().issues.uses_snapshot(&hash));
    let cs = last(&ed);
    assert_eq!(cs.changed.issues, vec![id]);
    assert_eq!(cs.added.resources.len(), 1);

    // A second viewpoint with the same picture shares the resource.
    let out2 = ed
        .execute(&Command::AddIssueViewpoint {
            issue: id,
            viewpoint: Box::new(vp.clone()),
            picture: Some(Box::new(pic.clone())),
            now: 7,
        })
        .unwrap();
    assert_eq!(ed.doc().resources.len(), 1);
    // A comment may refer to a viewpoint of its issue.
    let mut c = match comment(id, "see the view", 8) {
        Command::AddIssueComment { comment } => comment,
        _ => unreachable!(),
    };
    c.viewpoint = Some(vid);
    ed.execute(&Command::AddIssueComment { comment: c }).unwrap();
    let mut bad = match comment(id, "no such view", 8) {
        Command::AddIssueComment { comment } => comment,
        _ => unreachable!(),
    };
    bad.viewpoint = Some(ViewpointId::from_num(4242));
    assert!(ed.execute(&Command::AddIssueComment { comment: bad }).is_err());

    // Deleting one viewpoint keeps the picture (the other uses it) and unlinks the comment.
    ed.execute(&Command::DeleteIssueViewpoint {
        issue: id,
        viewpoint: vid,
        now: 9,
    })
    .unwrap();
    assert!(ed.doc().resources.find_image(&hash).is_some());
    assert_eq!(ed.doc().issues.comments_of(id)[0].viewpoint, None);
    // Deleting the last one removes the picture. Undo brings both back.
    ed.execute(&Command::DeleteIssueViewpoint {
        issue: id,
        viewpoint: ViewpointId(out2.created[0]),
        now: 10,
    })
    .unwrap();
    assert!(ed.doc().resources.find_image(&hash).is_none());
    ed.undo();
    assert!(ed.doc().resources.find_image(&hash).is_some());
    ed.redo();
    // Deleting the issue with no viewpoint left changes no resource.
    ed.execute(&Command::DeleteIssue { id }).unwrap();
    assert_eq!(ed.doc().resources.len(), 0);
}

#[test]
fn a_snapshot_needs_its_picture_and_must_be_a_small_png() {
    let mut ed = editor();
    let pic = header_only_sample(2);
    let with_snapshot = |hash: &str| Issue {
        title: "A".into(),
        viewpoints: vec![Viewpoint {
            snapshot: Some(hash.into()),
            ..Viewpoint::default()
        }],
        ..Issue::default()
    };
    let missing = Command::AddIssue {
        issue: Box::new(with_snapshot(&pic.sha256)),
        pictures: vec![],
    };
    let err = ed.execute(&missing).unwrap_err();
    assert!(err.to_string().contains("no picture"), "{err}");
    // A picture that is not a PNG, one that is too big, one with a bad hash.
    let mut jpeg = pic.clone();
    jpeg.format = kadakas_core::picture::PictureFormat::Jpeg;
    let cmd = |p| Command::AddIssue {
        issue: Box::new(with_snapshot(&pic.sha256)),
        pictures: vec![p],
    };
    assert!(ed.execute(&cmd(jpeg)).is_err());
    let mut huge = pic.clone();
    huge.width = 3000;
    huge.height = 3000;
    assert!(ed.execute(&cmd(huge)).is_err());
    let mut bad = pic.clone();
    bad.sha256 = "0".repeat(64);
    assert!(ed.execute(&cmd(bad)).is_err());
    assert!(ed.doc().issues.is_empty() && ed.doc().resources.len() == 0);
    // The good one works in one step.
    ed.execute(&cmd(pic.clone())).unwrap();
    assert_eq!(ed.doc().resources.len(), 1);
    ed.undo();
    assert_eq!(ed.doc().resources.len(), 0);
    assert!(ed.doc().issues.is_empty());
}

#[test]
fn a_plant_picture_with_the_same_hash_is_not_removed_with_the_issue() {
    use kadakas_core::resource::ResourceBody;
    let mut ed = editor();
    let pic = header_only_sample(3);
    ed.execute(&Command::AddResource {
        name: "Mine".into(),
        body: ResourceBody::Image(Box::new(pic.clone())),
    })
    .unwrap();
    let style = ed
        .execute(&Command::AddResource {
            name: "Oak".into(),
            body: ResourceBody::PlantStyle(Box::default()),
        })
        .unwrap()
        .created[0];
    ed.execute(&Command::SetPlantPicture {
        style: kadakas_core::resource::ResourceId(style),
        picture: Some(Box::new(pic.clone())),
    })
    .unwrap();
    let issue = Issue {
        title: "A".into(),
        viewpoints: vec![Viewpoint {
            snapshot: Some(pic.sha256.clone()),
            ..Viewpoint::default()
        }],
        ..Issue::default()
    };
    let id = IssueId(
        ed.execute(&Command::AddIssue {
            issue: Box::new(issue),
            pictures: vec![pic.clone()],
        })
        .unwrap()
        .created[0],
    );
    let n = ed.doc().resources.len();
    ed.execute(&Command::DeleteIssue { id }).unwrap();
    assert_eq!(ed.doc().resources.len(), n, "the plant style still uses it");
}

#[test]
fn the_issue_count_is_bounded() {
    let ed = editor();
    let issues: Vec<Issue> = (0..MAX_ISSUES as u64)
        .map(|n| Issue {
            id: IssueId::from_num(n + 10),
            title: "x".into(),
            ..Issue::default()
        })
        .collect();
    let mut doc = ed.doc().clone();
    doc.issues = IssueData {
        issues,
        comments: vec![],
    }
    .into();
    let mut ed = Editor::new(doc);
    let err = ed.execute(&new_issue("one too many")).unwrap_err();
    assert!(err.to_string().contains("at most"), "{err}");
}

#[test]
fn issues_are_saved_and_loaded_and_older_files_have_none() {
    let mut ed = editor();
    let id = add(&mut ed, "Saved");
    ed.execute(&comment(id, "a note", 4)).unwrap();
    let text = serde_json::to_string(ed.doc()).unwrap();
    let back = Document::from_json(&text).unwrap();
    assert_eq!(&back, ed.doc());
    assert_eq!(back.issues.comment_count(id), 1);
    // A document without issues writes no `issues` field.
    let plain = serde_json::to_value(Document::default()).unwrap();
    assert!(plain.get("issues").is_none());
    // A file with an orphan comment loads, and says what it repaired.
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["issues"]["issues"] = serde_json::json!([]);
    let loaded = Document::from_json(&v.to_string()).unwrap();
    assert!(loaded.issues.is_empty());
    assert!(
        loaded.load_notes().iter().any(|n| n.contains("comment")),
        "{:?}",
        loaded.load_notes()
    );
}

#[test]
fn objects_in_an_issue_are_ids_not_checked_against_the_document() {
    let mut ed = editor();
    let issue = Issue {
        title: "About a tree".into(),
        objects: vec![ObjectId::from_num(12345)],
        ..Issue::default()
    };
    ed.execute(&Command::AddIssue {
        issue: Box::new(issue),
        pictures: vec![],
    })
    .unwrap();
}

#[test]
fn a_batch_is_one_undo_step() {
    let mut ed = editor();
    let batch = Command::Batch {
        commands: vec![new_issue("one"), new_issue("two")],
    };
    ed.execute(&batch).unwrap();
    assert_eq!(ed.doc().issues.len(), 2);
    assert_eq!(last(&ed).added.issues.len(), 2);
    ed.undo();
    assert!(ed.doc().issues.is_empty());
}

#[test]
fn remote_application_lists_issue_changes() {
    let mut ed = editor();
    let id = add(&mut ed, "Mine");
    let mut theirs = ed.doc().clone();
    let other = IssueId::from_num(500);
    let d = theirs.issues.data_mut();
    d.issues.push(Issue {
        id: other,
        title: "Theirs".into(),
        ..Issue::default()
    });
    d.issues[0].title = "Mine, edited".into();
    d.comments.push(Comment {
        id: CommentId::from_num(600),
        issue: id,
        author: String::new(),
        origin: Origin::Remote,
        time: 1,
        text: "hi".into(),
        viewpoint: None,
        edited: None,
    });
    let cs = ed.apply_remote(theirs).expect("a change");
    assert_eq!(cs.added.issues, vec![other]);
    assert_eq!(cs.changed.issues, vec![id]);
    assert_eq!(cs.added.comments, vec![CommentId::from_num(600)]);
    assert_eq!(cs.origin, Origin::Remote);
}

#[test]
fn the_change_set_filter_knows_issues() {
    let mut ed = editor();
    add(&mut ed, "A");
    let cs = last(&ed);
    let only = cs.only(&["issues"]).unwrap();
    assert_eq!(only.added.issues.len(), 1);
    assert!(cs.only(&["objects"]).is_none());
    assert!(cs.added.len() >= 1 && !cs.added.is_empty());
}

#[test]
fn the_undo_budget_counts_issue_steps() {
    let mut ed = editor();
    let before = ed.history_bytes();
    add(&mut ed, "A");
    assert!(ed.history_bytes() > before);
}
