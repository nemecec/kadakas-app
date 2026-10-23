use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, ClassId};
use proptest::prelude::*;

use super::*;

fn editor() -> (Editor, ClassId) {
    let mut ed = Editor::with_sequential_ids();
    let c = ClassId(
        ed.execute(&Command::AddClass {
            name: "Trees".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    (ed, c)
}

fn cam() -> Camera {
    let mut c = Camera::standard(StandardView::RightIsometric, Projection::Perspective);
    c.target = Vec3::new(100.0, 200.0, 300.0);
    c.distance = 5000.0;
    c
}

macro_rules! run {
    ($ed:ident, $c:expr) => {{
        let c = $c;
        $ed.execute(&c.unwrap()).unwrap();
    }};
}

fn names(ed: &Editor) -> Vec<String> {
    ed.doc()
        .drawing
        .saved_views
        .iter()
        .map(|v| v.name.clone())
        .collect()
}

fn hide(ed: &mut Editor, c: ClassId) {
    ed.execute(&Command::ModifyClass {
        id: c,
        name: None,
        visible: Some(false),
        attrs: None,
    })
    .unwrap();
}

#[test]
fn new_names_are_free_and_numbered() {
    let (mut ed, _) = editor();
    assert_eq!(new_name(ed.doc()), "Scene 1");
    run!(ed, add(ed.doc(), "Scene 1", &cam()));
    run!(ed, add(ed.doc(), "scene 2", &cam()));
    assert_eq!(new_name(ed.doc()), "Scene 3");
}

#[test]
fn add_refuses_empty_and_taken_names() {
    let (mut ed, _) = editor();
    run!(ed, add(ed.doc(), " North ", &cam()));
    assert_eq!(names(&ed), ["North"]);
    assert_eq!(add(ed.doc(), "  ", &cam()), Err(Problem::Empty));
    assert_eq!(add(ed.doc(), "NORTH", &cam()), Err(Problem::Taken));
    let mut bad = cam();
    bad.distance = f32::NAN;
    assert_eq!(add(ed.doc(), "Bad", &bad), Err(Problem::BadCamera));
}

#[test]
fn a_scene_stores_camera_tags_layers_and_style() {
    let (mut ed, trees) = editor();
    ed.execute(&Command::UseRenderStyle {
        name: "hidden line".into(),
    })
    .unwrap();
    hide(&mut ed, trees);
    run!(ed, add(ed.doc(), "A", &cam()));
    let v = find(ed.doc(), "a").unwrap();
    assert_eq!(v.classes.get(&trees), Some(&false));
    assert_eq!(v.render_style.as_deref(), Some("Hidden Line"));
    let back = camera_from_saved(v.camera.as_ref().unwrap());
    assert!((back.target - cam().target).length() < 1e-3);
    assert_eq!(back.projection, Projection::Perspective);
    assert!(v.view.is_some() && v.zoom.is_none());
    let r = rows(ed.doc(), Some("A"));
    assert!(r[0].active && r[0].has_camera && r[0].hidden == 1);
    assert_eq!(r[0].style.as_deref(), Some("Hidden Line"));
}

#[test]
fn update_keeps_the_place_and_stores_the_present_state() {
    let (mut ed, trees) = editor();
    for n in ["A", "B", "C"] {
        run!(ed, add(ed.doc(), n, &cam()));
    }
    hide(&mut ed, trees);
    let mut moved = cam();
    moved.yaw += 1.0;
    assert!(is_modified(ed.doc(), "B", &moved));
    run!(ed, update(ed.doc(), "b", &moved));
    assert_eq!(names(&ed), ["A", "B", "C"]);
    assert!(!is_modified(ed.doc(), "B", &moved));
    assert_eq!(
        find(ed.doc(), "B").unwrap().classes.get(&trees),
        Some(&false)
    );
    assert_eq!(update(ed.doc(), "X", &moved), Err(Problem::NoSuchScene));
    assert!(!is_modified(ed.doc(), "X", &moved));
}

#[test]
fn rename_delete_and_move_are_one_undo_step_each() {
    let (mut ed, _) = editor();
    for n in ["A", "B", "C"] {
        run!(ed, add(ed.doc(), n, &cam()));
    }
    let base = ed.doc().drawing.saved_views.clone();
    run!(ed, rename(ed.doc(), "A", "First"));
    assert_eq!(names(&ed), ["First", "B", "C"]);
    assert_eq!(rename(ed.doc(), "B", "first"), Err(Problem::Taken));
    assert_eq!(rename(ed.doc(), "B", " "), Err(Problem::Empty));
    assert_eq!(rename(ed.doc(), "Q", "z"), Err(Problem::NoSuchScene));
    // A change of case only is a rename of the same scene.
    run!(ed, rename(ed.doc(), "B", "b"));
    ed.undo();
    ed.undo();
    assert_eq!(ed.doc().drawing.saved_views, base);
    run!(ed, move_to(ed.doc(), "C", 0));
    assert_eq!(names(&ed), ["C", "A", "B"]);
    ed.undo();
    assert_eq!(ed.doc().drawing.saved_views, base);
    run!(ed, delete(ed.doc(), "B"));
    assert_eq!(names(&ed), ["A", "C"]);
    ed.undo();
    assert_eq!(ed.doc().drawing.saved_views, base);
    ed.redo();
    assert_eq!(names(&ed), ["A", "C"]);
    assert_eq!(delete(ed.doc(), "B"), Err(Problem::NoSuchScene));
}

#[test]
fn move_by_stops_at_the_ends() {
    let (mut ed, _) = editor();
    for n in ["A", "B", "C"] {
        run!(ed, add(ed.doc(), n, &cam()));
    }
    assert_eq!(move_by(ed.doc(), "A", -1), Ok(None));
    assert_eq!(move_by(ed.doc(), "C", 1), Ok(None));
    let c = move_by(ed.doc(), "A", 1).unwrap().unwrap();
    ed.execute(&c).unwrap();
    assert_eq!(names(&ed), ["B", "A", "C"]);
    assert_eq!(move_by(ed.doc(), "Z", 1), Err(Problem::NoSuchScene));
    run!(ed, move_to(ed.doc(), "B", 99));
    assert_eq!(names(&ed), ["A", "C", "B"]);
}

#[test]
fn activating_restores_tags_and_style_in_one_undo_step() {
    let (mut ed, trees) = editor();
    ed.execute(&Command::UseRenderStyle {
        name: "Realistic".into(),
    })
    .unwrap();
    hide(&mut ed, trees);
    run!(ed, add(ed.doc(), "Dusk", &cam()));
    ed.execute(&Command::ModifyClass {
        id: trees,
        name: None,
        visible: Some(true),
        attrs: None,
    })
    .unwrap();
    ed.execute(&Command::UseRenderStyle {
        name: "Wireframe".into(),
    })
    .unwrap();
    let before = ed.doc().clone();
    let a = activate(ed.doc(), "dusk").unwrap();
    ed.execute(&a.command).unwrap();
    assert!(!ed.doc().class(trees).unwrap().visible);
    assert_eq!(active_style_name(ed.doc()).as_deref(), Some("Realistic"));
    assert!(matches!(a.goal, Goal::Camera(c) if (c.distance - 5000.0).abs() < 1e-2));
    ed.undo();
    assert_eq!(ed.doc().classes, before.classes);
    assert_eq!(
        ed.doc().drawing.presentation.active_style,
        before.drawing.presentation.active_style
    );
    assert_eq!(activate(ed.doc(), "none"), Err(Problem::NoSuchScene));
}

#[test]
fn a_view_of_the_app_becomes_a_standard_goal() {
    let (mut ed, _) = editor();
    let mk = |name: &str, view: Option<&str>, proj: Option<&str>| Command::SaveView {
        view: Box::new(SavedView {
            name: name.into(),
            view: view.map(Into::into),
            projection: proj.map(Into::into),
            ..SavedView::default()
        }),
    };
    for c in [
        mk("Front", Some("front"), Some("ortho")),
        mk("Plan", Some("plan"), None),
        mk("Nothing", None, None),
    ] {
        ed.execute(&c).unwrap();
    }
    assert_eq!(
        activate(ed.doc(), "Front").unwrap().goal,
        Goal::Standard(StandardView::Front, Some(Projection::Orthographic))
    );
    assert_eq!(
        activate(ed.doc(), "Plan").unwrap().goal,
        Goal::Standard(StandardView::Top, None)
    );
    assert_eq!(activate(ed.doc(), "Nothing").unwrap().goal, Goal::Stay);
    assert!(!rows(ed.doc(), None)[0].has_camera);
}

#[test]
fn the_nearest_standard_view_matches_the_standard_cameras() {
    for v in StandardView::ALL {
        if v == StandardView::Plan {
            continue;
        }
        let c = Camera::standard(v, Projection::Perspective);
        assert_eq!(nearest_standard(&c).yaw_pitch(), v.yaw_pitch(), "{v:?}");
    }
}

#[test]
fn styles_list_built_in_first_and_use_style_is_one_undo_step() {
    let (mut ed, _) = editor();
    let rows0 = styles(ed.doc());
    assert_eq!(rows0.len(), 5);
    assert!(rows0.iter().all(|r| r.builtin && !r.in_document));
    assert!(rows0.iter().find(|r| r.name == "Shaded").unwrap().active);
    assert!(use_style(ed.doc(), "Shaded").is_none());
    assert!(use_style(ed.doc(), "nope").is_none());
    let before = ed.doc().clone();
    let c = use_style(ed.doc(), "wireframe").unwrap();
    ed.execute(&c).unwrap();
    let r = styles(ed.doc());
    assert!(r.iter().find(|r| r.name == "Wireframe").unwrap().active);
    assert!(
        r.iter()
            .find(|r| r.name == "Wireframe")
            .unwrap()
            .in_document
    );
    ed.undo();
    assert_eq!(ed.doc().resources, before.resources);
    assert_eq!(ed.doc().drawing.presentation, before.drawing.presentation);
    // Own styles of the document come after the built-in ones.
    ed.execute(&Command::AddResource {
        name: "Afternoon".into(),
        body: kadakas_core::resource::ResourceBody::Render(Box::new(
            kadakas_core::present::RenderResource::RenderStyle(RenderStyle::default()),
        )),
    })
    .unwrap();
    let r = styles(ed.doc());
    assert_eq!(r.last().unwrap().name, "Afternoon");
    assert!(!r.last().unwrap().builtin);
    assert!(use_style(ed.doc(), "afternoon").is_some());
}

#[test]
fn the_transition_runs_from_start_to_goal_the_short_way() {
    let mut a = cam();
    a.yaw = 3.0;
    let mut b = cam();
    b.yaw = -3.0;
    b.distance = 20_000.0;
    let t = Transition::new(a, b, 10.0);
    let (c0, done0) = t.at(10.0);
    assert!(!done0 && (c0.yaw - 3.0).abs() < 1e-5);
    let (mid, done) = t.at(10.0 + TRANSITION_SECONDS / 2.0);
    assert!(!done);
    // The short way from 3.0 to -3.0 passes pi, not 0.
    assert!(mid.yaw.abs() > 3.0);
    assert!(mid.distance > 5000.0 && mid.distance < 20_000.0);
    let (end, done) = t.at(11.0);
    assert!(done && end == b && t.goal() == b);
    // A clock that runs back or is not a number never gives a wrong camera.
    assert_eq!(t.at(0.0).0.yaw, 3.0);
    assert!(t.at(f64::NAN).1);
}

#[test]
fn a_camera_that_looks_straight_down_gets_the_top_view_whatever_its_turn() {
    for yaw in [0.0f32, 1.0, 2.5, -2.0, 3.1] {
        let mut c = Camera::standard(StandardView::Top, Projection::Perspective);
        c.yaw = yaw;
        assert_eq!(nearest_standard(&c), StandardView::Top, "yaw {yaw}");
    }
}

#[test]
fn the_fov_comes_back_exactly_from_degrees() {
    let mut c = cam();
    c.fov_y = 108.02393_f32.to_radians();
    c.projection = Projection::Orthographic;
    assert_eq!(camera_from_saved(&camera_to_saved(&c)), c);
    // Many laps of turning give a valid number.
    c.yaw = 5.0e6;
    assert!(camera_to_saved(&c).is_valid());
}

#[test]
fn a_scene_stores_the_mode_of_its_style_and_marks_a_missing_style() {
    let (mut ed, _) = editor();
    run!(ed, add(ed.doc(), "A", &cam()));
    assert_eq!(find(ed.doc(), "A").unwrap().mode.as_deref(), Some("shaded"));
    ed.execute(&Command::UseRenderStyle {
        name: "Wireframe".into(),
    })
    .unwrap();
    run!(ed, update(ed.doc(), "A", &cam()));
    assert_eq!(
        find(ed.doc(), "A").unwrap().mode.as_deref(),
        Some("wireframe")
    );
    assert_eq!(active_mode(ed.doc()).id(), "wireframe");
    ed.execute(&Command::SaveView {
        view: Box::new(SavedView {
            name: "Lost".into(),
            render_style: Some("Nowhere".into()),
            ..SavedView::default()
        }),
    })
    .unwrap();
    let r = rows(ed.doc(), None);
    assert!(!r[0].style_missing && r[1].style_missing);
}

proptest! {
    #[test]
    fn the_transition_stays_finite_and_between_the_ends(
        y0 in -10.0f32..10.0, y1 in -10.0f32..10.0,
        d0 in 1.0f32..1.0e6, d1 in 1.0f32..1.0e6,
        t in 0.0f64..1.0,
    ) {
        let mut a = cam(); a.yaw = y0; a.distance = d0;
        let mut b = cam(); b.yaw = y1; b.distance = d1;
        let (c, _) = Transition::new(a, b, 0.0).at(t * TRANSITION_SECONDS);
        prop_assert!(c.distance.is_finite() && c.yaw.is_finite() && c.target.is_finite());
        prop_assert!(c.distance >= d0.min(d1) * 0.999 && c.distance <= d0.max(d1) * 1.001);
    }

    #[test]
    fn camera_survives_the_saved_form(
        yaw in -6.0f32..6.0, pitch in -1.5f32..1.5, dist in 1.0f32..1.0e6,
        x in -1.0e5f32..1.0e5, fov in 5.0f32..120.0, persp in any::<bool>(),
    ) {
        let c = Camera {
            target: Vec3::new(x, -x, x / 2.0), yaw, pitch, distance: dist,
            projection: if persp { Projection::Perspective } else { Projection::Orthographic },
            fov_y: fov.to_radians(),
        };
        let saved = camera_to_saved(&c);
        prop_assert!(saved.is_valid());
        let json = serde_json::to_string(&saved).unwrap();
        let back: ViewCamera = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&back, &saved);
        let c2 = camera_from_saved(&back);
        prop_assert_eq!(c2, c);
    }

    #[test]
    fn random_scene_edits_keep_names_unique_and_undo_exactly(
        ops in proptest::collection::vec((0u8..5, 0usize..4, 0usize..4), 1..24)
    ) {
        let (mut ed, _) = editor();
        let pool = ["a", "b", "c", "d"];
        for (op, i, j) in ops {
            let before = ed.doc().drawing.saved_views.clone();
            let name = pool[i];
            let cmd = match op {
                0 => add(ed.doc(), name, &cam()),
                1 => update(ed.doc(), name, &cam()),
                2 => rename(ed.doc(), name, pool[j]),
                3 => delete(ed.doc(), name),
                _ => move_to(ed.doc(), name, j),
            };
            let Ok(cmd) = cmd else { continue };
            if ed.execute(&cmd).is_ok() {
                let after = names(&ed);
                let mut lower: Vec<String> = after.iter().map(|n| n.to_lowercase()).collect();
                lower.sort();
                lower.dedup();
                prop_assert_eq!(lower.len(), after.len());
                // A command that changes nothing (Update with the same state) makes no step.
                if ed.doc().drawing.saved_views != before {
                    ed.undo();
                    prop_assert_eq!(&ed.doc().drawing.saved_views, &before);
                    ed.redo();
                    prop_assert_eq!(names(&ed), after);
                }
            }
        }
    }
}
