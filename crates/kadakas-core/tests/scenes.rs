//! Scenes (MOD-10) in the core: the camera and the render style of a saved view, the commands
//! `RenameView`, `MoveView` and `UseRenderStyle`, the restore of a style, undo, and the file
//! form. A scene is a saved view, so these are saved view tests.

use kadakas_core::command::{Command, Editor};
use kadakas_core::present::{self, RenderMode};
use kadakas_core::sheets::{SavedView, ViewCamera};
use proptest::prelude::*;

fn camera() -> ViewCamera {
    ViewCamera {
        target: [10.0, 20.0, 30.0],
        yaw: 0.25,
        pitch: -0.5,
        distance: 7000.0,
        fov: 35.0,
        projection: "perspective".into(),
    }
}

fn save(ed: &mut Editor, name: &str, style: Option<&str>, cam: Option<ViewCamera>) {
    ed.execute(&Command::SaveView {
        view: Box::new(SavedView {
            name: name.into(),
            render_style: style.map(Into::into),
            camera: cam,
            ..SavedView::default()
        }),
    })
    .unwrap();
}

fn names(ed: &Editor) -> Vec<String> {
    ed.doc()
        .drawing
        .saved_views
        .iter()
        .map(|v| v.name.clone())
        .collect()
}

fn style_mode(ed: &Editor) -> Option<RenderMode> {
    let id = ed.doc().drawing.presentation.active_style?;
    present::render_style(ed.doc(), id).map(|s| s.mode)
}

#[test]
fn the_camera_and_style_are_saved_in_json_and_old_files_have_neither() {
    let v = SavedView {
        name: "A".into(),
        camera: Some(camera()),
        render_style: Some("Realistic".into()),
        ..SavedView::default()
    };
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["camera"]["distance"], 7000.0);
    assert_eq!(json["render_style"], "Realistic");
    assert_eq!(serde_json::from_value::<SavedView>(json).unwrap(), v);
    let old: SavedView = serde_json::from_str(r#"{"name": "Old", "view": "front"}"#).unwrap();
    assert!(old.camera.is_none() && old.render_style.is_none());
    let plain = serde_json::to_value(&old).unwrap();
    assert!(plain.get("camera").is_none() && plain.get("render_style").is_none());
}

#[test]
fn a_camera_must_be_usable() {
    let mut ed = Editor::with_sequential_ids();
    for bad in [
        ViewCamera {
            distance: 0.0,
            ..camera()
        },
        ViewCamera {
            distance: f64::NAN,
            ..camera()
        },
        ViewCamera {
            fov: 0.5,
            ..camera()
        },
        ViewCamera {
            fov: 171.0,
            ..camera()
        },
        ViewCamera {
            yaw: f64::INFINITY,
            ..camera()
        },
        ViewCamera {
            target: [1.0e12, 0.0, 0.0],
            ..camera()
        },
        ViewCamera {
            projection: "fisheye".into(),
            ..camera()
        },
    ] {
        assert!(!bad.is_valid());
        let r = ed.execute(&Command::SaveView {
            view: Box::new(SavedView {
                name: "Bad".into(),
                camera: Some(bad),
                ..SavedView::default()
            }),
        });
        assert!(r.is_err());
    }
    assert!(names(&ed).is_empty());
    save(
        &mut ed,
        "Good",
        None,
        Some(ViewCamera {
            projection: "ortho".into(),
            ..camera()
        }),
    );
    assert_eq!(names(&ed), ["Good"]);
}

#[test]
fn use_render_style_adds_a_built_in_style_once_and_is_one_undo_step() {
    let mut ed = Editor::with_sequential_ids();
    let before = ed.doc().clone();
    ed.execute(&Command::UseRenderStyle {
        name: " hidden line ".into(),
    })
    .unwrap();
    assert_eq!(style_mode(&ed), Some(RenderMode::HiddenLine));
    assert_eq!(ed.doc().resources.len(), 1);
    ed.execute(&Command::UseRenderStyle {
        name: "Shaded".into(),
    })
    .unwrap();
    ed.execute(&Command::UseRenderStyle {
        name: "Hidden Line".into(),
    })
    .unwrap();
    assert_eq!(ed.doc().resources.len(), 2, "the style is not added twice");
    assert!(ed.undo() && ed.undo() && ed.undo());
    assert_eq!(ed.doc().resources, before.resources);
    assert_eq!(ed.doc().drawing.presentation, before.drawing.presentation);
    assert!(!ed.undo() || ed.doc().resources.is_empty());
    let e = ed
        .execute(&Command::UseRenderStyle {
            name: "Nope".into(),
        })
        .unwrap_err();
    assert!(e.to_string().contains("no render style"));
    assert!(ed.doc().resources.is_empty());
}

#[test]
fn a_style_of_the_document_wins_over_the_built_in_one_of_the_same_name() {
    let mut ed = Editor::with_sequential_ids();
    let own = present::RenderStyle {
        mode: RenderMode::Wireframe,
        ..present::RenderStyle::default()
    };
    ed.execute(&Command::AddResource {
        name: "Shaded".into(),
        body: kadakas_core::resource::ResourceBody::Render(Box::new(
            present::RenderResource::RenderStyle(own),
        )),
    })
    .unwrap();
    ed.execute(&Command::UseRenderStyle {
        name: "shaded".into(),
    })
    .unwrap();
    assert_eq!(style_mode(&ed), Some(RenderMode::Wireframe));
    assert_eq!(ed.doc().resources.len(), 1);
}

#[test]
fn restore_sets_the_style_in_the_same_undo_step_and_skips_a_missing_style() {
    let mut ed = Editor::with_sequential_ids();
    save(&mut ed, "Lines", Some("Dashed Hidden Line"), None);
    save(&mut ed, "Gone", Some("Deleted style"), None);
    save(&mut ed, "NoStyle", None, None);
    let before = ed.doc().clone();
    ed.execute(&Command::RestoreView {
        name: "lines".into(),
    })
    .unwrap();
    assert_eq!(style_mode(&ed), Some(RenderMode::DashedHiddenLine));
    assert!(ed.undo());
    assert_eq!(ed.doc().resources, before.resources);
    assert_eq!(ed.doc().drawing, before.drawing);
    ed.execute(&Command::RestoreView {
        name: "Lines".into(),
    })
    .unwrap();
    let after = ed.doc().drawing.presentation.clone();
    // A style that does not exist leaves the style as it is, and a view without a style too.
    ed.execute(&Command::RestoreView {
        name: "Gone".into(),
    })
    .unwrap();
    ed.execute(&Command::RestoreView {
        name: "NoStyle".into(),
    })
    .unwrap();
    assert_eq!(ed.doc().drawing.presentation, after);
    assert!(
        ed.execute(&Command::RestoreView {
            name: "Missing".into()
        })
        .is_err()
    );
}

#[test]
fn rename_keeps_the_place_and_refuses_taken_and_empty_names() {
    let mut ed = Editor::with_sequential_ids();
    for n in ["a", "b", "c"] {
        save(&mut ed, n, None, None);
    }
    let rename = |name: &str, new: &str| Command::RenameView {
        name: name.into(),
        new_name: new.into(),
    };
    ed.execute(&rename("B", "  Middle ")).unwrap();
    assert_eq!(names(&ed), ["a", "Middle", "c"]);
    ed.execute(&rename("c", "C")).unwrap();
    assert_eq!(names(&ed), ["a", "Middle", "C"]);
    assert!(
        ed.execute(&rename("a", "MIDDLE"))
            .unwrap_err()
            .to_string()
            .contains("exists")
    );
    assert!(ed.execute(&rename("a", "   ")).is_err());
    assert!(ed.execute(&rename("zz", "x")).is_err());
    assert_eq!(names(&ed), ["a", "Middle", "C"]);
    assert!(ed.undo() && ed.undo());
    assert_eq!(names(&ed), ["a", "b", "c"]);
}

#[test]
fn move_view_sets_the_order() {
    let mut ed = Editor::with_sequential_ids();
    for n in ["a", "b", "c", "d"] {
        save(&mut ed, n, None, None);
    }
    let mv = |name: &str, to: usize| Command::MoveView {
        name: name.into(),
        to,
    };
    ed.execute(&mv("d", 0)).unwrap();
    assert_eq!(names(&ed), ["d", "a", "b", "c"]);
    ed.execute(&mv("D", 2)).unwrap();
    assert_eq!(names(&ed), ["a", "b", "d", "c"]);
    ed.execute(&mv("a", usize::MAX)).unwrap();
    assert_eq!(names(&ed), ["b", "d", "c", "a"]);
    assert!(ed.execute(&mv("x", 0)).is_err());
    assert!(ed.undo() && ed.undo() && ed.undo());
    assert_eq!(names(&ed), ["a", "b", "c", "d"]);
}

#[test]
fn commands_are_json_for_agents() {
    for text in [
        r#"{"op": "rename_view", "name": "a", "new_name": "b"}"#,
        r#"{"op": "move_view", "name": "a", "to": 2}"#,
        r#"{"op": "use_render_style", "name": "Realistic"}"#,
    ] {
        let c: Command = serde_json::from_str(text).unwrap();
        let back = serde_json::to_value(&c).unwrap();
        assert_eq!(
            back,
            serde_json::from_str::<serde_json::Value>(text).unwrap()
        );
    }
}

proptest! {
    #[test]
    fn random_view_edits_keep_names_unique_and_undo_exactly(
        ops in proptest::collection::vec((0u8..4, 0usize..4, 0usize..5), 1..30)
    ) {
        let mut ed = Editor::with_sequential_ids();
        let pool = ["a", "b", "c", "d"];
        for (op, i, j) in ops {
            let before = ed.doc().drawing.saved_views.clone();
            let name = pool[i].to_string();
            let cmd = match op {
                0 => Command::SaveView { view: Box::new(SavedView { name, ..SavedView::default() }) },
                1 => Command::RenameView { name, new_name: pool[j % 4].into() },
                2 => Command::MoveView { name, to: j },
                _ => Command::DeleteView { name },
            };
            if ed.execute(&cmd).is_ok() {
                let after = names(&ed);
                let mut lower: Vec<String> = after.iter().map(|n| n.to_lowercase()).collect();
                lower.sort();
                lower.dedup();
                prop_assert_eq!(lower.len(), after.len());
                if ed.doc().drawing.saved_views != before {
                    prop_assert!(ed.undo());
                    prop_assert_eq!(&ed.doc().drawing.saved_views, &before);
                    prop_assert!(ed.redo());
                    prop_assert_eq!(names(&ed), after);
                }
            }
        }
    }
}
