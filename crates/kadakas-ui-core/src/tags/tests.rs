use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, Color, FIRST_LAYER, Shape};

use super::*;

fn tag(ed: &mut Editor, name: &str, attrs: Attributes) -> ClassId {
    ClassId(
        ed.execute(&Command::AddClass {
            name: name.into(),
            attrs,
        })
        .unwrap()
        .created[0],
    )
}

fn line(ed: &mut Editor, class: Option<ClassId>) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
            layer: None,
            class,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

#[test]
fn untagged_comes_first_and_the_rows_count_objects_and_show_the_active_tag() {
    let mut ed = Editor::with_sequential_ids();
    let red = tag(
        &mut ed,
        "Trees",
        Attributes {
            pen: Some(Color([200, 10, 10, 255])),
            ..Attributes::default()
        },
    );
    let plain = tag(&mut ed, "Walls", Attributes::default());
    line(&mut ed, Some(red));
    line(&mut ed, Some(red));
    line(&mut ed, None);
    ed.execute(&set_active_tag(plain)).unwrap();
    let r = rows(ed.doc());
    assert_eq!(r.tags.len(), 3);
    assert!(r.tags[0].untagged && r.tags[0].id == NONE_CLASS);
    assert_eq!(r.tags[0].color, None, "Untagged has no swatch");
    assert_eq!(r.tags[0].objects, 1);
    assert!(!r.tags[0].active);
    assert_eq!(r.tags[1].name, "Trees");
    assert_eq!(r.tags[1].color, Some([200, 10, 10]));
    assert_eq!(r.tags[1].objects, 2);
    assert_eq!(r.tags[2].name, "Walls");
    assert_eq!(r.tags[2].color, None);
    assert!(r.tags[2].active && r.tags[2].visible);
    assert_eq!(r.layers.len(), 1);
    assert!(r.layers[0].active && r.layers[0].visible && !r.layers[0].locked);
}

#[test]
fn the_swatch_takes_the_pen_then_the_fill_and_ignores_clear_colors() {
    let fill = Attributes {
        pen: Some(Color::NONE),
        fill: Some(Color([1, 2, 3, 255])),
        ..Attributes::default()
    };
    assert_eq!(swatch(&fill), Some([1, 2, 3]));
    let pen = Attributes {
        pen: Some(Color([9, 8, 7, 255])),
        fill: Some(Color([1, 2, 3, 255])),
        ..Attributes::default()
    };
    assert_eq!(swatch(&pen), Some([9, 8, 7]));
    assert_eq!(swatch(&Attributes::default()), None);
}

#[test]
fn the_eye_hides_and_shows_a_tag_and_a_layer_with_the_existing_commands() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees", Attributes::default());
    let o = line(&mut ed, Some(t));
    let cmd = toggle_tag(ed.doc(), t).unwrap();
    assert!(matches!(
        cmd,
        Command::ModifyClass { visible: Some(false), .. }
    ));
    ed.execute(&cmd).unwrap();
    assert!(!ed.doc().is_visible(ed.doc().object(o).unwrap()));
    assert!(!rows(ed.doc()).tags[1].visible);
    ed.execute(&toggle_tag(ed.doc(), t).unwrap()).unwrap();
    assert!(ed.doc().is_visible(ed.doc().object(o).unwrap()));
    // Layers work the same way.
    ed.execute(&toggle_layer(ed.doc(), FIRST_LAYER).unwrap()).unwrap();
    assert!(!ed.doc().is_visible(ed.doc().object(o).unwrap()));
    assert!(!rows(ed.doc()).layers[0].visible);
    // Show All brings back every hidden tag and layer in one step.
    ed.execute(&toggle_tag(ed.doc(), t).unwrap()).unwrap();
    let depth = ed.undo_depth();
    ed.execute(&show_all(ed.doc()).unwrap()).unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert!(ed.doc().is_visible(ed.doc().object(o).unwrap()));
    assert!(show_all(ed.doc()).is_none());
    assert!(toggle_tag(ed.doc(), ClassId::from_num(99)).is_none());
    assert!(toggle_layer(ed.doc(), LayerId::from_num(99)).is_none());
}

#[test]
fn new_tags_get_a_free_name_and_names_are_checked() {
    let mut ed = Editor::with_sequential_ids();
    assert_eq!(new_tag_name(ed.doc()), "Tag 1");
    tag(&mut ed, "Tag 1", Attributes::default());
    tag(&mut ed, "Tag 2", Attributes::default());
    assert_eq!(new_tag_name(ed.doc()), "Tag 3");
    let c = new_tag(ed.doc(), "  Paths ").unwrap();
    assert!(matches!(&c, Command::AddClass { name, .. } if name == "Paths"));
    assert_eq!(new_tag(ed.doc(), "   "), Err(NameProblem::Empty));
    assert_eq!(new_tag(ed.doc(), "tag 1"), Err(NameProblem::Taken));
    ed.execute(&c).unwrap();
    assert_eq!(new_tag(ed.doc(), "PATHS"), Err(NameProblem::Taken));
}

#[test]
fn a_tag_is_renamed_but_untagged_keeps_its_name() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees", Attributes::default());
    let other = tag(&mut ed, "Walls", Attributes::default());
    let c = rename_tag(ed.doc(), t, "Big trees").unwrap();
    ed.execute(&c).unwrap();
    assert_eq!(ed.doc().class(t).unwrap().name, "Big trees");
    // The same name with other letters is fine for the tag itself.
    assert!(rename_tag(ed.doc(), t, "BIG TREES").is_ok());
    assert_eq!(rename_tag(ed.doc(), t, "walls"), Err(NameProblem::Taken));
    assert_eq!(rename_tag(ed.doc(), t, ""), Err(NameProblem::Empty));
    assert_eq!(rename_tag(ed.doc(), NONE_CLASS, "x"), Err(NameProblem::Untagged));
    assert_eq!(
        rename_tag(ed.doc(), ClassId::from_num(99), "x"),
        Err(NameProblem::NoSuchTag)
    );
    let _ = other;
}

#[test]
fn deleting_a_tag_counts_its_objects_and_moves_them_to_untagged() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees", Attributes::default());
    let a = line(&mut ed, Some(t));
    let b = line(&mut ed, Some(t));
    let plan = delete_tag(ed.doc(), t).unwrap();
    assert_eq!(plan.objects, 2, "the window asks before this runs");
    ed.execute(&plan.command).unwrap();
    assert!(ed.doc().class(t).is_none());
    for id in [a, b] {
        assert_eq!(ed.doc().object(id).unwrap().class, NONE_CLASS);
    }
    let t2 = tag(&mut ed, "Empty", Attributes::default());
    assert_eq!(delete_tag(ed.doc(), t2).unwrap().objects, 0, "no question for an empty tag");
    assert_eq!(delete_tag(ed.doc(), NONE_CLASS), Err(NameProblem::Untagged));
    assert_eq!(
        delete_tag(ed.doc(), ClassId::from_num(99)),
        Err(NameProblem::NoSuchTag)
    );
}

#[test]
fn the_tag_of_a_selection_is_one_mixed_or_none_and_can_be_changed_in_one_step() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees", Attributes::default());
    let a = line(&mut ed, Some(t));
    let b = line(&mut ed, None);
    assert_eq!(tag_of(ed.doc(), &[]), SelectionTag::None);
    assert_eq!(tag_of(ed.doc(), &[a]), SelectionTag::One(t));
    assert_eq!(tag_of(ed.doc(), &[a, b]), SelectionTag::Mixed);
    let depth = ed.undo_depth();
    let cmd = set_tag(ed.doc(), &[a, b], t).unwrap();
    ed.execute(&cmd).unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert_eq!(tag_of(ed.doc(), &[a, b]), SelectionTag::One(t));
    // Nothing to do when they have it.
    assert!(set_tag(ed.doc(), &[a, b], t).is_none());
    // A tag that does not exist is refused.
    assert!(set_tag(ed.doc(), &[a], ClassId::from_num(99)).is_none());
    ed.undo();
    assert_eq!(tag_of(ed.doc(), &[a, b]), SelectionTag::Mixed);
}

#[test]
fn a_group_gives_its_tag_to_its_members() {
    let mut ed = Editor::with_sequential_ids();
    let t = tag(&mut ed, "Trees", Attributes::default());
    let a = line(&mut ed, None);
    let b = line(&mut ed, None);
    let g = ObjectId(
        ed.execute(&Command::Group {
            ids: vec![a, b],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&set_tag(ed.doc(), &[g], t).unwrap()).unwrap();
    for id in [g, a, b] {
        assert_eq!(ed.doc().object(id).unwrap().class, t, "{id:?}");
    }
}

#[test]
fn sheet_layers_are_not_listed() {
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::AddLayer {
        name: "Sheet".into(),
        elevation: 0.0,
        scale: None,
        kind: Some(LayerKind::Sheet),
    })
    .unwrap();
    assert_eq!(rows(ed.doc()).layers.len(), 1);
}
