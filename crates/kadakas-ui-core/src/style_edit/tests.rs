use super::*;
use kadakas_core::command::Editor;

fn add(ed: &mut Editor, name: &str, kind: StyleKind) -> ResourceId {
    ResourceId(
        ed.execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::annot(kind.default_body()),
        })
        .unwrap()
        .created[0],
    )
}

fn run(ed: &mut Editor, c: Command) {
    ed.execute(&c).unwrap();
}

#[test]
fn the_kinds_map_to_type_names_and_bodies() {
    for k in StyleKind::ALL {
        assert_eq!(StyleKind::from_type_name(k.type_name()), Some(k));
        assert_eq!(kind_of(&k.default_body()), k);
        assert!(k.default_body().validate().is_ok(), "{k:?}");
        assert!(!k.default_name().is_empty());
    }
    assert_eq!(StyleKind::from_type_name("hatch"), None);
}

#[test]
fn choices_have_ids_and_the_lists_are_complete() {
    assert_eq!(id_of(&TERMINATORS, &Terminator::OpenArrow), "open_arrow");
    assert_eq!(id_of(&JUSTIFICATIONS, &Justify::Right), "right");
    assert_eq!(id_of(&FRAMES, &TagFrame::Underline), "underline");
    // The ids are the serde names, so the API and the window agree.
    for c in &TERMINATORS {
        assert_eq!(serde_json::to_value(c.value).unwrap(), c.id);
    }
    for c in &JUSTIFICATIONS {
        assert_eq!(serde_json::to_value(c.value).unwrap(), c.id);
    }
    for c in &FRAMES {
        assert_eq!(serde_json::to_value(c.value).unwrap(), c.id);
    }
    assert_eq!(length_units().len(), 6);
    assert_eq!(area_units()[3].1, "ac");
}

#[test]
fn optional_values_turn_on_and_off() {
    let mut v: Option<f64> = None;
    set_enabled(&mut v, true, 2.0);
    assert_eq!(v, Some(2.0));
    set_enabled(&mut v, true, 9.0);
    assert_eq!(v, Some(2.0));
    set_enabled(&mut v, false, 9.0);
    assert_eq!(v, None);
    set_enabled(&mut v, false, 9.0);
    assert_eq!(v, None);
    assert_eq!(default_tolerance().plus, 1.0);
}

#[test]
fn samples_follow_the_style() {
    let mut s = DimensionStyle::default();
    assert_eq!(
        dimension_samples(&s),
        ["12346", "87.65 m\u{b2}", "37.3\u{b0}"]
    );
    s.unit = LengthUnit::M;
    s.precision = 2;
    s.area_unit = AreaUnit::Ha;
    s.area_precision = 4;
    assert_eq!(dimension_samples(&s), ["12.35", "0.0088 ha", "37.3\u{b0}"]);
}

#[test]
fn a_clean_editor_saves_nothing_and_an_edit_is_one_command() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Dimension);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    assert_eq!((e.id(), e.kind()), (id, StyleKind::Dimension));
    assert!(!e.is_dirty());
    assert_eq!(e.save_command(ed.doc()).unwrap(), None);
    e.dimension().unwrap().area_unit = AreaUnit::Ac;
    e.name = " Landscape ".into();
    assert!(e.is_dirty());
    let cmd = e.save_command(ed.doc()).unwrap().unwrap();
    run(&mut ed, cmd);
    e.mark_saved();
    assert!(!e.is_dirty());
    assert_eq!(e.name, "Landscape");
    let st = ed.doc().resources.dimension_style(id).unwrap();
    assert_eq!(st.area_unit, AreaUnit::Ac);
    assert_eq!(ed.doc().resources.get(id).unwrap().name, "Landscape");
    // One undo step undoes the whole save.
    assert!(ed.undo());
    assert_eq!(
        ed.doc().resources.dimension_style(id).unwrap().area_unit,
        AreaUnit::M2
    );
    assert_eq!(ed.doc().resources.get(id).unwrap().name, "Std");
    // Only the name changed: the command carries no body.
    e.revert();
    e.name = "Other".into();
    let Some(Command::ModifyResource { name, body, .. }) = e.save_command(ed.doc()).unwrap() else {
        panic!()
    };
    assert_eq!((name.as_deref(), body.is_none()), (Some("Other"), true));
}

#[test]
fn bad_drafts_are_refused_with_a_reason() {
    let mut ed = Editor::default();
    let a = add(&mut ed, "A", StyleKind::Text);
    let b = add(&mut ed, "B", StyleKind::Text);
    let c = add(&mut ed, "A", StyleKind::Tag);
    let mut e = StyleEditor::open(ed.doc(), b).unwrap();
    e.name = "  ".into();
    assert!(e.problem(ed.doc()).unwrap().contains("name"));
    e.name = "a".into();
    assert!(e.problem(ed.doc()).unwrap().contains("exists"));
    assert!(e.save_command(ed.doc()).is_err());
    e.name = "B".into();
    e.text().unwrap().size_mm = f64::NAN;
    assert!(e.problem(ed.doc()).unwrap().contains("size_mm"));
    e.revert();
    assert_eq!(e.problem(ed.doc()), None);
    // The same name in another type is fine.
    let mut t = StyleEditor::open(ed.doc(), c).unwrap();
    t.name = "B".into();
    assert_eq!(t.problem(ed.doc()), None);
    let _ = a;
}

#[test]
fn open_refuses_other_resources() {
    let mut ed = Editor::default();
    assert!(
        StyleEditor::open(ed.doc(), ResourceId(kadakas_core::uid::Uid::from_u128(99))).is_none()
    );
    let id = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Rec".into(),
            body: ResourceBody::RecordFormat(Default::default()),
        })
        .unwrap()
        .created[0],
    );
    assert!(StyleEditor::open(ed.doc(), id).is_none());
}

#[test]
fn the_editor_follows_the_document() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Text);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    assert_eq!(e.sync(ed.doc()), Follow::Same);
    // Another change reaches a clean editor.
    run(
        &mut ed,
        Command::ModifyResource {
            id,
            name: Some("New".into()),
            body: Some(ResourceBody::annot(AnnotResource::TextStyle(TextStyle {
                size_mm: 7.0,
                ..TextStyle::default()
            }))),
        },
    );
    assert_eq!(e.sync(ed.doc()), Follow::Reloaded);
    assert_eq!(e.name, "New");
    assert_eq!(e.text().unwrap().size_mm, 7.0);
    assert!(!e.is_dirty());
    // An edit is kept when the document changes under it.
    e.text().unwrap().bold = true;
    assert!(ed.undo());
    assert_eq!(e.sync(ed.doc()), Follow::Conflict);
    assert!(e.text().unwrap().bold);
    assert!(e.is_dirty());
    assert_eq!(e.sync(ed.doc()), Follow::Same);
    // Saving writes the draft over the new content.
    let cmd = e.save_command(ed.doc()).unwrap().unwrap();
    run(&mut ed, cmd);
    e.mark_saved();
    assert_eq!(e.sync(ed.doc()), Follow::Same);
    // Deleted.
    run(&mut ed, Command::DeleteResource { id, reassign: None });
    assert_eq!(e.sync(ed.doc()), Follow::Gone);
}

#[test]
fn tag_lines_add_remove_and_move() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Tag", StyleKind::Tag);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    assert_eq!(e.rows(), 1);
    assert!(e.add_row());
    assert_eq!((e.rows(), e.selected), (2, 1));
    e.tag().unwrap().lines[1] = "{IdCode}".into();
    assert!(e.add_row());
    e.tag().unwrap().lines[2] = "{Botanical}".into();
    assert!(e.move_row(2, -1));
    assert_eq!(e.selected, 1);
    assert_eq!(
        e.tag().unwrap().lines,
        ["{Kind}", "{Botanical}", "{IdCode}"]
    );
    assert!(!e.move_row(0, -1));
    assert!(!e.move_row(2, 1));
    assert!(!e.move_row(7, 1));
    assert!(!e.move_row(1, 0));
    assert!(e.remove_row(0));
    assert!(e.remove_row(0));
    assert!(e.remove_row(0));
    // Always one line.
    assert_eq!(e.tag().unwrap().lines, [""]);
    assert!(!e.remove_row(5));
    assert_eq!(e.selected, 0);
    // A limit.
    while e.add_row() {}
    assert_eq!(e.rows(), MAX_TAG_LINES);
    // The unfinished line makes the style invalid only by its braces.
    e.tag().unwrap().lines[0] = "{Open".into();
    assert!(e.problem(ed.doc()).unwrap().contains("braces"));
}

#[test]
fn notes_get_free_keys_and_keep_their_order() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Notes", StyleKind::Notes);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    assert_eq!(e.rows(), 0);
    assert!(e.add_row());
    assert!(e.add_row());
    e.notes().unwrap().notes[0].key = "first".into();
    assert!(e.add_row());
    let keys: Vec<_> = e
        .notes()
        .unwrap()
        .notes
        .iter()
        .map(|n| n.key.clone())
        .collect();
    assert_eq!(keys, ["first", "note-2", "note-1"]);
    // Two notes with one key are refused.
    e.notes().unwrap().notes[2].key = "NOTE-2".into();
    assert!(e.problem(ed.doc()).unwrap().contains("two notes"));
    e.notes().unwrap().notes[2].key = "note-3".into();
    e.notes().unwrap().notes[1].text = "Mulch 50 mm".into();
    assert!(e.move_row(1, 1));
    assert_eq!(e.selected, 2);
    let cmd = e.save_command(ed.doc()).unwrap().unwrap();
    run(&mut ed, cmd);
    assert_eq!(ed.doc().resources.notes_db(id).unwrap().notes.len(), 3);
    assert!(e.remove_row(2));
    assert_eq!(e.selected, 1);
    // Other kinds have no rows.
    let t = add(&mut ed, "T", StyleKind::Text);
    let mut te = StyleEditor::open(ed.doc(), t).unwrap();
    assert_eq!(te.rows(), 0);
    assert!(!te.add_row());
    assert!(!te.remove_row(0));
    assert!(!te.move_row(0, 1));
    assert!(te.dimension().is_none() && te.tag().is_none() && te.notes().is_none());
    assert!(te.text().is_some());
}

#[test]
fn the_notes_limit_stops_adding() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Notes", StyleKind::Notes);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    e.notes().unwrap().notes = (0..MAX_NOTES)
        .map(|i| Note {
            key: format!("k{i}"),
            text: String::new(),
        })
        .collect();
    assert!(!e.add_row());
    assert_eq!(e.rows(), MAX_NOTES);
}

#[test]
fn new_styles_get_free_names() {
    let mut ed = Editor::default();
    for k in StyleKind::ALL {
        let c = create_command(ed.doc(), k);
        run(&mut ed, c);
        let c = create_command(ed.doc(), k);
        let Command::AddResource { name, .. } = &c else {
            panic!()
        };
        assert_eq!(name, &format!("{} 2", k.default_name()));
        run(&mut ed, c);
        let c = create_command(ed.doc(), k);
        let Command::AddResource { name, .. } = &c else {
            panic!()
        };
        assert_eq!(name, &format!("{} 3", k.default_name()));
    }
}

#[test]
fn a_dimension_standard_edits_every_field_and_saves() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Dimension);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    {
        let s = e.dimension().unwrap();
        s.unit = LengthUnit::FtIn;
        s.terminator = Terminator::Tick;
        s.prefix = "~".into();
        s.area_round_to = Some(0.5);
        set_enabled(&mut s.tolerance, true, default_tolerance());
        set_enabled(&mut s.round_to, true, 5.0);
    }
    let cmd = e.save_command(ed.doc()).unwrap().unwrap();
    run(&mut ed, cmd);
    let s = ed.doc().resources.dimension_style(id).unwrap();
    assert_eq!(s.unit, LengthUnit::FtIn);
    assert!(s.tolerance.is_some() && s.round_to == Some(5.0));
    // A negative increment is refused before the command runs.
    e.mark_saved();
    e.dimension().unwrap().area_round_to = Some(-1.0);
    assert!(e.save_command(ed.doc()).is_err());
}
