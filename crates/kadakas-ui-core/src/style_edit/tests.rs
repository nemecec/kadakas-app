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
        assert!(!k.label().is_empty());
    }
    assert_eq!(StyleKind::from_type_name("hatch"), None);
    kadakas_l10n::with_language("et", || {
        assert_eq!(StyleKind::Dimension.label(), "Mõõtmestamise standard");
    });
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
    assert_eq!(dimension_samples(&s), ["12346", "37.3\u{b0}"]);
    s.unit = LengthUnit::M;
    s.precision = 2;
    s.angle_precision = 0;
    assert_eq!(dimension_samples(&s), ["12.35", "37\u{b0}"]);
}

#[test]
fn a_clean_editor_saves_nothing_and_an_edit_is_one_command() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Dimension);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    assert_eq!((e.id(), e.kind()), (id, StyleKind::Dimension));
    assert!(!e.is_dirty());
    assert_eq!(e.save_command(ed.doc()).unwrap(), None);
    e.dimension().unwrap().unit = LengthUnit::Ft;
    e.name = " Landscape ".into();
    assert!(e.is_dirty());
    let cmd = e.save_command(ed.doc()).unwrap().unwrap();
    run(&mut ed, cmd);
    e.mark_saved();
    assert!(!e.is_dirty());
    assert_eq!(e.name, "Landscape");
    let st = ed.doc().resources.dimension_style(id).unwrap();
    assert_eq!(st.unit, LengthUnit::Ft);
    assert_eq!(ed.doc().resources.get(id).unwrap().name, "Landscape");
    // One undo step undoes the whole save.
    assert!(ed.undo());
    assert_eq!(
        ed.doc().resources.dimension_style(id).unwrap().unit,
        LengthUnit::Mm
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
fn spaces_at_the_ends_of_the_name_are_not_a_change() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Text);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    e.name = "  Std ".into();
    assert!(!e.is_dirty());
    assert_eq!(e.save_command(ed.doc()).unwrap(), None);
}

#[test]
fn bad_drafts_are_refused_with_a_typed_reason() {
    let mut ed = Editor::default();
    let a = add(&mut ed, "A", StyleKind::Text);
    let b = add(&mut ed, "B", StyleKind::Text);
    let c = add(&mut ed, "A", StyleKind::Tag);
    let mut e = StyleEditor::open(ed.doc(), b).unwrap();
    e.name = "  ".into();
    assert_eq!(e.problem(ed.doc()), Some(Problem::NameMissing));
    e.name = "a".into();
    let taken = e.problem(ed.doc()).unwrap();
    assert_eq!(
        taken,
        Problem::NameTaken {
            kind: StyleKind::Text,
            name: "a".into()
        }
    );
    assert_eq!(e.save_command(ed.doc()), Err(taken.clone()));
    e.name = "B".into();
    e.text().unwrap().size_mm = f64::NAN;
    let Some(Problem::Invalid(m)) = e.problem(ed.doc()) else {
        panic!()
    };
    assert!(m.contains("size_mm"));
    e.revert();
    assert_eq!(e.problem(ed.doc()), None);
    // The same name in another kind is fine.
    let mut t = StyleEditor::open(ed.doc(), c).unwrap();
    t.name = "B".into();
    assert_eq!(t.problem(ed.doc()), None);
    let _ = a;
    // The texts are readable in both languages; the kind is a name, not an API id.
    assert_eq!(
        taken.text(),
        "The name \"a\" is already in use (Text style)."
    );
    assert_eq!(Problem::NameMissing.text(), "The style needs a name.");
    assert!(Problem::Invalid("x".into()).text().contains('x'));
    kadakas_l10n::with_language("et", || {
        assert_eq!(
            taken.text(),
            "Nimi \u{201e}a\u{201c} on juba kasutusel (Tekstistiil)."
        );
        assert_eq!(Problem::NameMissing.text(), "Stiilil peab olema nimi.");
        assert!(Problem::Invalid("x".into()).text().contains('x'));
    });
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
fn the_editor_reloads_a_clean_draft_and_follows_a_delete() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Text);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    assert_eq!(e.sync(ed.doc()), Follow::Same);
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
    run(&mut ed, Command::DeleteResource { id, reassign: None });
    assert_eq!(e.sync(ed.doc()), Follow::Gone);
}

#[test]
fn a_remote_rename_survives_a_local_change_of_another_field() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Text);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    // The person turns on Bold. Meanwhile another user renames the style and changes the size.
    e.text().unwrap().bold = true;
    run(
        &mut ed,
        Command::ModifyResource {
            id,
            name: Some("Renamed".into()),
            body: Some(ResourceBody::annot(AnnotResource::TextStyle(TextStyle {
                size_mm: 6.0,
                ..TextStyle::default()
            }))),
        },
    );
    assert_eq!(e.sync(ed.doc()), Follow::Merged);
    assert_eq!(e.name, "Renamed");
    assert!(e.text().unwrap().bold);
    assert_eq!(e.text().unwrap().size_mm, 6.0);
    assert!(e.is_dirty());
    assert_eq!(e.sync(ed.doc()), Follow::Same);
    // Saving writes both: the new name stays and the size stays.
    let cmd = e.save_command(ed.doc()).unwrap().unwrap();
    let Command::ModifyResource { name, body, .. } = &cmd else {
        panic!()
    };
    assert!(name.is_none() && body.is_some());
    run(&mut ed, cmd);
    e.mark_saved();
    let st = ed.doc().resources.text_style(id).unwrap();
    assert!(st.bold && st.size_mm == 6.0);
    assert_eq!(ed.doc().resources.get(id).unwrap().name, "Renamed");
}

#[test]
fn a_change_of_the_same_field_on_both_sides_is_a_conflict_and_mine_wins() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Text);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    e.text().unwrap().size_mm = 5.0;
    e.name = "Mine".into();
    run(
        &mut ed,
        Command::ModifyResource {
            id,
            name: Some("Theirs".into()),
            body: Some(ResourceBody::annot(AnnotResource::TextStyle(TextStyle {
                size_mm: 9.0,
                italic: true,
                ..TextStyle::default()
            }))),
        },
    );
    assert_eq!(e.sync(ed.doc()), Follow::Conflict);
    assert_eq!(e.name, "Mine");
    assert_eq!(e.text().unwrap().size_mm, 5.0);
    // The other field came in.
    assert!(e.text().unwrap().italic);
    // The same change on both sides is no conflict.
    let id2 = add(&mut ed, "Two", StyleKind::Text);
    let mut e2 = StyleEditor::open(ed.doc(), id2).unwrap();
    e2.text().unwrap().bold = true;
    run(
        &mut ed,
        Command::ModifyResource {
            id: id2,
            name: None,
            body: Some(ResourceBody::annot(AnnotResource::TextStyle(TextStyle {
                bold: true,
                ..TextStyle::default()
            }))),
        },
    );
    assert_eq!(e2.sync(ed.doc()), Follow::Merged);
    assert!(!e2.is_dirty());
}

#[test]
fn an_undo_under_an_edited_draft_merges_too() {
    let mut ed = Editor::default();
    let id = add(&mut ed, "Std", StyleKind::Tag);
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    e.tag().unwrap().size_mm = 4.0;
    assert!(ed.undo());
    // The resource is gone now.
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
    let Some(Problem::Invalid(m)) = e.problem(ed.doc()) else {
        panic!()
    };
    assert!(m.contains("braces"));
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
    let Some(Problem::Invalid(m)) = e.problem(ed.doc()) else {
        panic!()
    };
    assert!(m.contains("two notes"));
    e.notes().unwrap().notes[2].key = "note-3".into();
    e.notes().unwrap().notes[1].text = "Mulch 50 mm".into();
    assert!(e.move_row(1, 1));
    assert_eq!(e.selected, 2);
    let cmd = e.save_command(ed.doc()).unwrap().unwrap();
    run(&mut ed, cmd);
    e.mark_saved();
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
    assert!(te.note_warnings(ed.doc()).is_empty());
}

#[test]
fn a_changed_or_removed_note_key_warns_while_callouts_use_it() {
    use kadakas_core::annot::{Annot, Callout, Leader};
    let mut ed = Editor::default();
    let id = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Notes".into(),
            body: ResourceBody::annot(AnnotResource::NotesDatabase(NotesDb {
                notes: vec![
                    Note {
                        key: "N1".into(),
                        text: "One".into(),
                    },
                    Note {
                        key: "N2".into(),
                        text: "Two".into(),
                    },
                ],
            })),
        })
        .unwrap()
        .created[0],
    );
    let callout = |note: &str| Callout {
        at: glam::DVec2::ZERO,
        text: String::new(),
        note: Some(note.into()),
        notes_db: Some(id),
        style: None,
        leaders: vec![Leader {
            points: vec![glam::DVec2::ZERO],
            anchor: None,
        }],
        shoulder_mm: 5.0,
        terminator: Terminator::Arrow,
        detached: false,
    };
    for note in ["n1", "N1", "N2"] {
        run(
            &mut ed,
            Command::AddObject {
                shape: Shape::Annotation(Box::new(Annot::Callout(callout(note)))),
                layer: None,
                class: None,
                attrs: Default::default(),
            },
        );
    }
    // A callout of another database does not count.
    let mut other = callout("N1");
    other.notes_db = None;
    other.note = None;
    run(
        &mut ed,
        Command::AddObject {
            shape: Shape::Annotation(Box::new(Annot::Callout(other))),
            layer: None,
            class: None,
            attrs: Default::default(),
        },
    );
    let mut e = StyleEditor::open(ed.doc(), id).unwrap();
    assert!(e.note_warnings(ed.doc()).is_empty());
    // A new text keeps the key: no warning. A new key warns, with the number of callouts.
    e.notes().unwrap().notes[0].text = "Changed".into();
    assert!(e.note_warnings(ed.doc()).is_empty());
    e.notes().unwrap().notes[0].key = "N9".into();
    assert_eq!(
        e.note_warnings(ed.doc()),
        [NoteWarning {
            key: "N1".into(),
            callouts: 2
        }]
    );
    // Removing a note nobody uses is fine; removing one in use warns.
    e.revert();
    e.notes().unwrap().notes.push(Note {
        key: "N3".into(),
        text: String::new(),
    });
    e.mark_saved();
    e.notes().unwrap().notes.remove(2);
    assert!(e.note_warnings(ed.doc()).is_empty());
    e.notes().unwrap().notes.remove(1);
    assert_eq!(e.note_warnings(ed.doc())[0].callouts, 1);
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
fn the_first_style_of_a_kind_is_found() {
    let mut ed = Editor::default();
    assert_eq!(first_of(ed.doc(), StyleKind::Dimension), None);
    let a = add(&mut ed, "A", StyleKind::Dimension);
    add(&mut ed, "B", StyleKind::Dimension);
    add(&mut ed, "T", StyleKind::Text);
    assert_eq!(first_of(ed.doc(), StyleKind::Dimension), Some(a));
}

#[test]
fn the_default_styles_include_the_area_tag_style_once() {
    let mut ed = Editor::default();
    let cmds = missing_default_styles(ed.doc());
    assert_eq!(cmds.len(), 4);
    for c in cmds {
        run(&mut ed, c);
    }
    assert!(missing_default_styles(ed.doc()).is_empty());
    let id = ed
        .doc()
        .resources
        .find_annot("tag_style", "Area")
        .expect("the Area tag style");
    assert_eq!(
        ed.doc().resources.tag_style(id).unwrap().lines,
        ["{AREA(Area)}"]
    );
    // A document with its own tag style still gets the Area style.
    let mut ed = Editor::default();
    add(&mut ed, "Mine", StyleKind::Tag);
    let names: Vec<String> = missing_default_styles(ed.doc())
        .iter()
        .map(|c| match c {
            Command::AddResource { name, .. } => name.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(names, ["Text 3 mm", "Dimension 2.5 mm", "Area"]);
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
        set_enabled(&mut s.tolerance, true, default_tolerance());
        set_enabled(&mut s.round_to, true, 5.0);
    }
    let cmd = e.save_command(ed.doc()).unwrap().unwrap();
    run(&mut ed, cmd);
    let s = ed.doc().resources.dimension_style(id).unwrap();
    assert_eq!(s.unit, LengthUnit::FtIn);
    assert!(s.tolerance.is_some() && s.round_to == Some(5.0));
    // A negative rounding is refused before the command runs.
    e.mark_saved();
    e.dimension().unwrap().round_to = Some(-1.0);
    assert!(e.save_command(ed.doc()).is_err());
}
