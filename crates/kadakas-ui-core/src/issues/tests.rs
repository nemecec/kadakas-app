use kadakas_core::command::Editor;
use kadakas_core::issue::{IssueData, IssuePatch};
use kadakas_core::model::{Attributes, LayerId, Shape};

use super::*;

fn rect(ed: &mut Editor, x: f64) -> ObjectId {
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::new(x, 0.0),
                max: DVec2::new(x + 1000.0, 500.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    ObjectId(out.created[0])
}

fn add(ed: &mut Editor, draft: &Draft, now: i64) -> IssueId {
    let issue = draft.issue("Ann", Origin::User, now, vec![]);
    IssueId(
        ed.execute(&Command::AddIssue {
            issue: Box::new(issue),
            pictures: vec![],
        })
        .unwrap()
        .created[0],
    )
}

fn titled(t: &str) -> Draft {
    Draft {
        title: t.into(),
        ..Draft::default()
    }
}

#[test]
fn status_colors_and_marks_differ() {
    let all = IssueStatus::ALL;
    let colors: BTreeSet<[u8; 3]> = all.iter().map(|s| status_rgb(*s)).collect();
    assert_eq!(colors.len(), 4);
    let marks: Vec<Mark> = all.iter().map(|s| status_mark(*s)).collect();
    assert_eq!(
        marks,
        vec![Mark::Dot, Mark::Half, Mark::Check, Mark::Bar],
        "each status has a mark that is not a color"
    );
    let messages: BTreeSet<&str> = all.iter().map(|s| status_message(*s)).collect();
    assert_eq!(messages.len(), 4);
}

#[test]
fn the_list_hides_done_issues_until_asked_and_sorts() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(&mut ed, &titled("A"), 1_000);
    let b = add(&mut ed, &titled("B"), 2_000);
    let c = add(&mut ed, &titled("C"), 3_000);
    ed.execute(&Command::ModifyIssue {
        id: c,
        patch: Box::new(IssuePatch {
            status: Some(IssueStatus::Resolved),
            ..IssuePatch::default()
        }),
        now: 9_000,
    })
    .unwrap();
    let doc = ed.doc();
    let mut f = Filters::default();
    let ids = |f: &Filters| rows(doc, f, 0).iter().map(|r| r.id).collect::<Vec<_>>();
    assert_eq!(ids(&f), vec![b, a], "newest change first, done hidden");
    f.show_done = true;
    assert_eq!(ids(&f), vec![b, a, c], "done last");
    f.show_done = false;
    f.toggle_status(IssueStatus::Resolved);
    assert_eq!(ids(&f), vec![c], "a chosen status shows done issues");
    f.toggle_status(IssueStatus::Resolved);
    assert!(!f.is_active());
    f.text = " ".into();
    assert!(!f.is_active());
    f.text = "b".into();
    assert!(f.is_active());
    assert_eq!(ids(&f), vec![b]);
    f.clear();
    assert_eq!(f, Filters::default());
}

#[test]
fn the_list_filters_by_assignee_and_label_and_gives_the_facts_of_a_row() {
    let mut ed = Editor::with_sequential_ids();
    let o = rect(&mut ed, 0.0);
    let d = Draft {
        title: "Tree".into(),
        assignee: "Mari".into(),
        labels: "planting, urgent, Planting".into(),
        due: "2026-01-10".into(),
        objects: vec![o],
        ..Draft::default()
    };
    let id = add(&mut ed, &d, 5);
    add(&mut ed, &titled("Other"), 6);
    ed.execute(&Command::AddIssueComment {
        comment: Box::new(issue::Comment {
            id: Default::default(),
            issue: id,
            author: "A".into(),
            origin: Origin::User,
            time: 7,
            text: "x".into(),
            viewpoint: None,
            edited: None,
        }),
    })
    .unwrap();
    let doc = ed.doc();
    let f = Filters {
        assignee: Some("mari".into()),
        label: Some("URGENT".into()),
        ..Filters::default()
    };
    let r = rows(doc, &f, 1_900_000_000_000);
    assert_eq!(r.len(), 1);
    let r = &r[0];
    assert_eq!(
        (r.title.as_str(), r.comments, r.objects, r.labels.len()),
        ("Tree", 1, 1, 2)
    );
    assert_eq!(r.due_state, Due::Overdue);
    assert_eq!(r.assignee.as_deref(), Some("Mari"));
    assert_eq!((r.origin, r.priority.as_str()), (Origin::User, "normal"));
    assert_eq!(r.modified, 7);
}

#[test]
fn due_dates() {
    let day = 86_400_000;
    let d = time::from_date("2026-06-10").unwrap();
    assert_eq!(due_state(None, IssueStatus::Open, 0), Due::None);
    assert_eq!(due_state(Some("bad"), IssueStatus::Open, 0), Due::None);
    assert_eq!(
        due_state(Some("2026-06-10"), IssueStatus::Open, d - 10 * day),
        Due::Later
    );
    assert_eq!(
        due_state(Some("2026-06-10"), IssueStatus::Open, d - 2 * day),
        Due::Soon
    );
    assert_eq!(
        due_state(Some("2026-06-10"), IssueStatus::Open, d + day / 2),
        Due::Soon
    );
    assert_eq!(
        due_state(Some("2026-06-10"), IssueStatus::Open, d + day),
        Due::Overdue
    );
    assert_eq!(
        due_state(Some("2026-06-10"), IssueStatus::Closed, d + 9 * day),
        Due::Later
    );
}

#[test]
fn pins_follow_the_point_the_object_or_the_plan_view() {
    let mut ed = Editor::with_sequential_ids();
    let o = rect(&mut ed, 2000.0);
    let by_point = add(
        &mut ed,
        &Draft {
            title: "Point".into(),
            point: Some([10.0, 20.0, 30.0]),
            ..Draft::default()
        },
        1,
    );
    let by_object = add(&mut ed, &Draft::for_objects(&[o]).tap_title("Object"), 2);
    let none = add(&mut ed, &titled("Nowhere"), 3);
    let gone = add(
        &mut ed,
        &Draft::for_objects(&[ObjectId::from_num(9999)]).tap_title("Gone"),
        4,
    );
    let doc = ed.doc();
    let pins = pins(doc, &Filters::default());
    let find = |id: IssueId| pins.iter().find(|p| p.id == id);
    assert_eq!(find(by_point).unwrap().at, [10.0, 20.0, 30.0]);
    assert_eq!(find(by_object).unwrap().at, [2500.0, 250.0, 0.0]);
    assert!(find(none).is_none() && find(gone).is_none());
    assert_eq!(find(by_point).unwrap().status, IssueStatus::Open);
}

#[test]
fn a_pin_of_a_layer_at_a_height_is_at_that_height() {
    let mut ed = Editor::with_sequential_ids();
    let layer = LayerId(
        ed.execute(&Command::AddLayer {
            name: "Up".into(),
            elevation: 1500.0,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0],
    );
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(100.0, 100.0),
            },
            layer: Some(layer),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    add(
        &mut ed,
        &Draft::for_objects(&[ObjectId(out.created[0])]).tap_title("Up"),
        1,
    );
    let p = pins(ed.doc(), &Filters::default());
    assert_eq!(p[0].at[2], 1500.0);
}

#[test]
fn a_caller_with_the_ground_puts_the_pin_on_it() {
    let mut ed = Editor::with_sequential_ids();
    let o = rect(&mut ed, 2000.0);
    add(
        &mut ed,
        &Draft::for_objects(&[o]).tap_title("On the ground"),
        1,
    );
    let p = pins_at(ed.doc(), &Filters::default(), &|_, at| at.x * 2.0);
    assert_eq!(p[0].at, [2500.0, 250.0, 5000.0]);
}

trait TapTitle {
    fn tap_title(self, t: &str) -> Self;
}

impl TapTitle for Draft {
    fn tap_title(mut self, t: &str) -> Draft {
        self.title = t.into();
        self
    }
}

#[test]
fn badges_show_the_count_of_many_objects() {
    assert_eq!(badge(0), None);
    assert_eq!(badge(1), None);
    assert_eq!(badge(2).as_deref(), Some("2"));
    assert_eq!(badge(99).as_deref(), Some("99"));
    assert_eq!(badge(100).as_deref(), Some("99+"));
    assert_eq!(badge(5000).as_deref(), Some("99+"));
}

#[test]
fn a_click_hits_the_nearest_pin_in_range() {
    let a = IssueId::from_num(1);
    let b = IssueId::from_num(2);
    let screens = [(a, DVec2::new(100.0, 100.0)), (b, DVec2::new(108.0, 100.0))];
    assert_eq!(pick_pin(&screens, DVec2::new(101.0, 100.0), 12.0), Some(a));
    assert_eq!(pick_pin(&screens, DVec2::new(107.0, 100.0), 12.0), Some(b));
    assert_eq!(pick_pin(&screens, DVec2::new(300.0, 100.0), 12.0), None);
    assert_eq!(pick_pin(&[], DVec2::ZERO, 12.0), None);
}

#[test]
fn a_form_checks_its_fields_and_makes_an_issue() {
    assert_eq!(titled(" ").check(), Err(DraftError::TitleEmpty));
    assert_eq!(
        titled(&"x".repeat(MAX_TITLE_CHARS + 1)).check(),
        Err(DraftError::TitleTooLong)
    );
    let mut d = titled("Ok");
    d.due = "soon".into();
    assert_eq!(d.check(), Err(DraftError::DueNotDate));
    d.due = "2026-12-24".into();
    d.labels = (0..=MAX_LABELS)
        .map(|n| format!("l{n}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(d.check(), Err(DraftError::TooManyLabels));
    d.labels = "x".repeat(MAX_LABEL_CHARS + 1);
    assert_eq!(d.check(), Err(DraftError::LabelTooLong));
    d.labels = " a , b ,, A ".into();
    assert_eq!(d.label_list(), vec!["a", "b"]);
    d.description = "z".repeat(issue::MAX_TEXT_CHARS + 1);
    let e = d.check().unwrap_err();
    assert!(matches!(e, DraftError::Other(_)));
    assert_eq!(e.message(), "uic-issues-error-other");
    d.description = "  Text  ".into();
    d.check().unwrap();
    d.assignee = " Mari ".into();
    d.kind = " ".into();
    d.priority = "high".into();
    d.objects = vec![ObjectId::from_num(3)];
    d.point = Some([1.0, 2.0, 3.0]);
    let i = d.issue("Ann", Origin::User, 55, vec![]);
    assert_eq!(i.title, "Ok");
    assert_eq!(i.description, "Text");
    assert_eq!(i.kind, "issue", "an empty type is the default");
    assert_eq!(i.priority, "high");
    assert_eq!(i.assignee.as_deref(), Some("Mari"));
    assert_eq!(i.due.as_deref(), Some("2026-12-24"));
    assert_eq!((i.author.as_str(), i.created, i.modified), ("Ann", 55, 55));
    assert_eq!(i.labels, vec!["a", "b"]);
    assert_eq!(i.point, Some([1.0, 2.0, 3.0]));
    i.validate().unwrap();
    let again = Draft::default().issue("", Origin::Agent, 0, vec![]);
    assert_eq!(
        (again.assignee, again.due, again.origin),
        (None, None, Origin::Agent)
    );
    for e in [
        DraftError::TitleEmpty,
        DraftError::TitleTooLong,
        DraftError::DueNotDate,
        DraftError::TooManyLabels,
        DraftError::LabelTooLong,
    ] {
        assert!(e.message().starts_with("uic-issues-error-"));
    }
    assert_eq!(
        Draft::at_point([1.0, 2.0, 0.0]).point,
        Some([1.0, 2.0, 0.0])
    );
    assert!(Draft::for_objects(&[ObjectId::from_num(1)]).with_viewpoint);
}

#[test]
fn opening_an_issue_selects_its_objects_and_restores_the_layers() {
    let mut ed = Editor::with_sequential_ids();
    let a = rect(&mut ed, 0.0);
    let b = rect(&mut ed, 5000.0);
    let layer = ed.doc().layers[0].id;
    // The viewpoint is made with the layer visible.
    let vp = capture(
        ed.doc(),
        ViewSpec::Plan {
            center: [1.0, 2.0],
            zoom: 0.5,
        },
        &[a, ObjectId::from_num(404)],
        9,
        "Overview",
    );
    assert!(vp.layers[&layer]);
    assert_eq!(vp.name, "Overview");
    let issue = Draft {
        title: "T".into(),
        objects: vec![a, b, ObjectId::from_num(405)],
        ..Draft::default()
    }
    .issue("Ann", Origin::User, 1, vec![]);
    // With the layer hidden afterwards, the plan has a command.
    ed.execute(&Command::SetVisibility {
        layers: vec![layer],
        classes: vec![],
        mode: kadakas_core::visibility::VisMode::Invisible,
    })
    .unwrap();
    let plan = open_plan(ed.doc(), &issue, Some(&vp));
    assert_eq!(
        plan.selection,
        vec![a],
        "the selection of the viewpoint, objects that exist"
    );
    assert_eq!(plan.missing, 1);
    assert_eq!(plan.commands.len(), 1);
    assert!(matches!(plan.view, Some(ViewSpec::Plan { .. })));
    ed.execute(&Command::Batch {
        commands: plan.commands,
    })
    .unwrap();
    assert!(ed.doc().layer(layer).unwrap().visible);
    // Without a viewpoint the objects of the issue are selected and nothing else changes.
    let plan = open_plan(ed.doc(), &issue, None);
    assert_eq!(plan.selection, vec![a, b]);
    assert!(plan.commands.is_empty() && plan.view.is_none());
    // A viewpoint without a selection falls back to the objects of the issue.
    let empty = Viewpoint::default();
    assert_eq!(
        open_plan(ed.doc(), &issue, Some(&empty)).selection,
        vec![a, b]
    );
}

#[test]
fn the_viewpoint_that_opens_is_the_asked_one_then_one_with_a_view() {
    let mk = |n: u64, view: bool| Viewpoint {
        id: ViewpointId::from_num(n),
        view: view.then_some(ViewSpec::Plan {
            center: [0.0, 0.0],
            zoom: 1.0,
        }),
        ..Viewpoint::default()
    };
    let mut i = Issue::default();
    assert!(viewpoint_of(&i, None).is_none());
    i.viewpoints = vec![mk(1, false), mk(2, true), mk(3, true)];
    assert_eq!(viewpoint_of(&i, None).unwrap().id, ViewpointId::from_num(2));
    assert_eq!(
        viewpoint_of(&i, Some(ViewpointId::from_num(3))).unwrap().id,
        ViewpointId::from_num(3)
    );
    assert_eq!(
        viewpoint_of(&i, Some(ViewpointId::from_num(99)))
            .unwrap()
            .id,
        ViewpointId::from_num(2)
    );
    i.viewpoints = vec![mk(1, false)];
    assert_eq!(viewpoint_of(&i, None).unwrap().id, ViewpointId::from_num(1));
}

#[test]
fn the_plan_view_goes_into_a_viewpoint_and_comes_back() {
    let (lo, hi) = (DVec2::new(10.0, 20.0), DVec2::new(810.0, 620.0));
    let v = View {
        origin: DVec2::new(123.0, 456.0),
        zoom: 0.25,
    };
    let spec = plan_spec(&v, lo, hi);
    let back = plan_view(&spec, lo, hi).unwrap();
    assert!((back.origin - v.origin).length() < 1e-9);
    assert_eq!(back.zoom, v.zoom);
    // The model point in the middle stays in the middle of another window.
    let other = plan_view(&spec, DVec2::ZERO, DVec2::new(100.0, 100.0)).unwrap();
    let ViewSpec::Plan { center, .. } = spec else {
        unreachable!()
    };
    let mid = other.model_of(DVec2::new(50.0, 50.0));
    assert!((mid.x - center[0]).abs() < 1e-9 && (mid.y - center[1]).abs() < 1e-9);
    // The zoom stays inside the limits; a camera is not a plan view.
    let wild = ViewSpec::Plan {
        center: [0.0, 0.0],
        zoom: 1e9,
    };
    assert_eq!(plan_view(&wild, lo, hi).unwrap().zoom, MAX_ZOOM);
    let cam = ViewSpec::Camera {
        target: [0.0; 3],
        yaw: 0.0,
        pitch: 0.0,
        distance: 1.0,
        perspective: false,
    };
    assert!(plan_view(&cam, lo, hi).is_none());
}

#[test]
fn an_empty_document_has_no_rows_and_no_pins() {
    let doc = Document::default();
    assert!(rows(&doc, &Filters::default(), 0).is_empty());
    assert!(pins(&doc, &Filters::default()).is_empty());
    let mut d = Document::default();
    d.issues = IssueData::default().into();
    assert!(pins(&d, &Filters::default()).is_empty());
}
