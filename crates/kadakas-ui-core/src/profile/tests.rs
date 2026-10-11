use super::*;
use crate::tools::Tool;
use serde_json::Value;

fn chord(s: &str) -> Chord {
    Chord::parse(s).unwrap()
}

fn parse(text: &str) -> Parsed {
    Profile::parse(text, "fallback").unwrap()
}

const MIN: &str = r#"{"format":1,"name":"Mine"}"#;

#[test]
fn a_minimal_file_gets_all_the_defaults() {
    let p = parse(MIN);
    assert!(p.problems.is_empty());
    let p = p.profile;
    assert_eq!(p.id, "fallback");
    assert_eq!(p.name, "Mine");
    assert!(!p.builtin && p.description.is_empty() && p.units.is_none());
    assert_eq!(p.keymap, Keymap::defaults());
    assert!(p.aliases.is_empty() && p.terms.is_empty());
    assert_eq!(p.tool_sets, layout::default_tool_sets());
    assert_eq!(p.menus, layout::default_menus());
    assert_eq!(p.display_name(), "Mine");
    assert_eq!(p.display_description(), "");
}

#[test]
fn files_that_are_no_profile_are_refused() {
    let err = |t: &str| Profile::parse(t, "x").unwrap_err();
    assert!(matches!(err("nope"), ParseError::NotJson(_)));
    assert_eq!(err("[]"), ParseError::NotAnObject);
    assert_eq!(err(r#"{"name":"a"}"#), ParseError::NoFormat);
    assert_eq!(err(r#"{"format":0,"name":"a"}"#), ParseError::BadFormat(0));
    assert_eq!(err(r#"{"format":2,"name":"a"}"#), ParseError::BadFormat(2));
    assert_eq!(err(r#"{"format":1}"#), ParseError::NoName);
    assert_eq!(err(r#"{"format":1,"name":"  "}"#), ParseError::NoName);
    let long = format!(r#"{{"format":1,"name":"{}"}}"#, "n".repeat(MAX_NAME + 1));
    assert_eq!(err(&long), ParseError::NameTooLong);
    for e in [
        ParseError::NotJson("x".into()),
        ParseError::NotAnObject,
        ParseError::NoFormat,
        ParseError::BadFormat(3),
        ParseError::NoName,
        ParseError::NameTooLong,
    ] {
        assert!(!e.text().starts_with("uic-"), "{e:?}");
    }
}

#[test]
fn ids_come_from_the_file_or_the_fallback() {
    let p = parse(r#"{"format":1,"name":"a","id":"my-id"}"#);
    assert_eq!(p.profile.id, "my-id");
    assert!(p.problems.is_empty());
    let p = parse(r#"{"format":1,"name":"a","id":"Bad Id"}"#);
    assert_eq!(p.profile.id, "fallback");
    assert_eq!(p.problems, vec![Problem::BadId("Bad Id".into())]);
}

#[test]
fn the_keymap_section_merges_with_the_defaults() {
    let p = parse(
        r#"{"format":1,"name":"a","keymap":{"tool.line":["G"],"edit.redo":[],"tool.rect":"Alt+R"}}"#,
    );
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    let k = p.profile.keymap;
    assert_eq!(k.action_for(chord("G")), Some(Action::ToolLine));
    assert_eq!(k.action_for(chord("2")), None);
    assert!(k.chords(Action::Redo).is_empty());
    assert_eq!(k.action_for(chord("Alt+R")), Some(Action::ToolRect));
    // Untouched actions keep their defaults.
    assert_eq!(k.action_for(chord("6")), Some(Action::ToolCircle));
}

#[test]
fn a_named_action_gives_up_its_old_keys_before_others_take_them() {
    // Line takes the old key of Rectangle; Rectangle takes a new one. No conflict.
    let p = parse(r#"{"format":1,"name":"a","keymap":{"tool.line":"4","tool.rect":"Q"}}"#);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert_eq!(
        p.profile.keymap.action_for(chord("4")),
        Some(Action::ToolLine)
    );
}

#[test]
fn a_chord_taken_from_an_action_that_is_not_named_is_reported() {
    let p = parse(r#"{"format":1,"name":"a","keymap":{"tool.circle":"2"}}"#);
    assert_eq!(
        p.problems,
        vec![Problem::ChordMoved {
            chord: "2".into(),
            from: "tool.line".into(),
            to: "tool.circle".into()
        }]
    );
    assert!(p.problems[0].text().contains("2"));
    assert!(p.profile.keymap.chords(Action::ToolLine).is_empty());
}

#[test]
fn bad_parts_are_replaced_by_defaults_and_listed() {
    let p = parse(
        r#"{"format":1,"name":"a",
        "keymap":{"nope":"A","tool.line":"Hyper+Q","tool.rect":5},
        "aliases":{"PL":"tool.polyline","bad alias!!":"tool.line","X":"nope","Y":7},
        "tool_sets":[{"id":"s","name":"S","tools":["tool.line","tool.line","nope","edit.undo"]},
                      {"id":"s"},{"id":"BAD"},5,{"id":"unnamed"},{"id":"n","name":""}],
        "menus":[{"id":"file","items":["file.new","zzz",7]},{"id":"m","name":"M","items":3}],
        "units":{"length":"cubits"},
        "terms":{"en-US":"panels-class = Layer { $zzz }\n","xx":"a = b","et":"nope = x\n"}}"#,
    );
    use Problem::*;
    let want = [
        UnknownAction("nope".into()),
        BadChord {
            action: "tool.line".into(),
            text: "Hyper+Q".into(),
        },
        BadChord {
            action: "tool.rect".into(),
            text: "5".into(),
        },
        BadAlias("bad alias!!".into()),
        UnknownAction("nope".into()),
        UnknownAction("7".into()),
        DuplicateTool {
            set: "s".into(),
            tool: "tool.line".into(),
        },
        UnknownTool {
            set: "s".into(),
            tool: "nope".into(),
        },
        UnknownTool {
            set: "s".into(),
            tool: "edit.undo".into(),
        },
        DuplicateId("s".into()),
        BadId("BAD".into()),
        WrongType("tools".into()),
        NeedsName("unnamed".into()),
        BadName("n".into()),
        NeedsName("n".into()),
        UnknownItem {
            menu: "file".into(),
            item: "zzz".into(),
        },
        UnknownItem {
            menu: "file".into(),
            item: "".into(),
        },
        WrongType("m.items".into()),
    ];
    for w in &want {
        assert!(p.problems.contains(w), "missing {w:?} in {:?}", p.problems);
    }
    assert!(p.problems.iter().any(|x| matches!(x, BadUnits(_))));
    assert!(p.problems.iter().any(|x| matches!(
        x,
        Terms { lang, problem: kadakas_l10n::OverlayProblem::UnknownVariable { .. } } if lang == "en-US"
    )));
    assert!(p.problems.contains(&TermsLanguage("xx".into())));
    for x in &p.problems {
        assert!(!x.text().starts_with("uic-"), "{x:?}");
    }
    let q = p.profile;
    assert_eq!(q.keymap.action_for(chord("2")), Some(Action::ToolLine));
    assert_eq!(q.aliases.len(), 1);
    assert_eq!(q.tool_sets.len(), 3);
    assert_eq!(q.tool_sets[0].tools, vec![Tool::Line]);
    assert!(q.units.is_none());
    // The program filters the messages when it sets the overlay, so both texts stay in the
    // profile. They are not dropped, because they can be read.
    assert!(q.terms.contains_key("en-US") && q.terms.contains_key("et"));
}

#[test]
fn sections_of_the_wrong_type_use_the_default() {
    let p = parse(
        r#"{"format":1,"name":"a","keymap":3,"aliases":[],"tool_sets":{},"menus":"x","terms":1,"units":null}"#,
    );
    assert_eq!(p.problems.len(), 5);
    assert!(
        p.problems
            .iter()
            .all(|x| matches!(x, Problem::WrongType(_)))
    );
    assert_eq!(p.profile.keymap, Keymap::defaults());
    let p = parse(r#"{"format":1,"name":"a","terms":{"et":5}}"#);
    assert_eq!(p.problems, vec![Problem::WrongType("terms.et".into())]);
}

#[test]
fn a_terms_text_with_a_syntax_error_is_dropped() {
    let p = parse(r#"{"format":1,"name":"a","terms":{"et":"a = {\n"}}"#);
    assert!(p.profile.terms.is_empty());
    assert!(matches!(
        p.problems[0],
        Problem::Terms {
            problem: kadakas_l10n::OverlayProblem::Syntax(_),
            ..
        }
    ));
    let t = p.problems[0].text();
    assert!(t.contains("et"));
    let u = Problem::Terms {
        lang: "et".into(),
        problem: kadakas_l10n::OverlayProblem::UnknownLanguage("et".into()),
    };
    assert!(!u.text().is_empty());
    let m = Problem::Terms {
        lang: "et".into(),
        problem: kadakas_l10n::OverlayProblem::UnknownMessage("zz".into()),
    };
    assert!(m.text().contains("zz"));
}

#[test]
fn units_are_read() {
    let p = parse(r#"{"format":1,"name":"a","units":{"length":"ft"}}"#);
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert!(p.profile.units.is_some());
}

#[test]
fn a_profile_survives_a_round_trip_through_its_file() {
    for original in Profile::builtins() {
        let again = parse(&original.to_json());
        assert!(again.problems.is_empty(), "{:?}", again.problems);
        let mut back = again.profile;
        back.builtin = true;
        assert_eq!(back, original, "{}", original.id);
    }
    let mut p = parse(r#"{"format":1,"name":"a","units":{"length":"ft"},"menus":[{"id":"m","name":"Mine","items":["tool.line","-"]}]}"#)
        .profile;
    p.set_alias("zz", Action::Undo);
    let back = parse(&p.to_json()).profile;
    assert_eq!(back, p);
    assert_eq!(back.menus[0].name.as_deref(), Some("Mine"));
}

#[test]
fn the_vectorworks_profile_is_the_default_workspace() {
    let p = Profile::vectorworks();
    assert!(p.builtin && p.id == VECTORWORKS_ID);
    assert_eq!(p.keymap, Keymap::defaults());
    assert_eq!(p.tool_sets, layout::default_tool_sets());
    assert_eq!(p.menus, layout::default_menus());
    assert!(p.aliases.is_empty() && p.terms.is_empty() && p.units.is_none());
}

#[test]
fn the_vectorworks_file_shows_the_whole_format() {
    // `UPDATE_PROFILES=1 cargo test -p kadakas-ui-core` writes the file again.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/profiles/vectorworks.json");
    let mut p = Profile::vectorworks();
    p.description = "The default workspace.".into();
    let text = p.to_json();
    if std::env::var_os("UPDATE_PROFILES").is_some() {
        std::fs::write(path, text.clone() + "\n").unwrap();
    }
    let file: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut want: Value = serde_json::from_str(&text).unwrap();
    // The description of the file is English text for readers of the file.
    want["description"] = file["description"].clone();
    assert_eq!(file, want, "run with UPDATE_PROFILES=1");
}

#[test]
fn the_autocad_profile_is_clean() {
    let b = Profile::builtins();
    let p = &b[1];
    assert!(p.builtin && p.id == AUTOCAD_ID);
    // Reading the shipped file gives no problem at all.
    let again = parse(AUTOCAD_JSON);
    assert!(again.problems.is_empty(), "{:?}", again.problems);
    let k = &p.keymap;
    // Letters go to the command field, so no plain letter or digit runs a tool.
    for key in ["L", "C", "O", "F", "Z", "V", "T", "2", "X"] {
        assert_eq!(k.action_for(chord(key)), None, "{key}");
    }
    assert_eq!(k.action_for(chord("Home")), Some(Action::ZoomFit));
    for a in Action::ALL {
        for c in k.chords(*a) {
            let plain = !c.command && !c.ctrl && !c.alt && !c.shift;
            let letter = c.key.name().len() == 1 || c.key.name().starts_with("Num");
            assert!(
                !(plain && letter),
                "{} has the plain key {}",
                a.id(),
                c.to_text()
            );
        }
    }
    assert_eq!(k.action_for(chord("Cmd/Ctrl+9")), Some(Action::CommandLine));
    for (alias, action) in [
        ("PL", Action::ToolPolyline),
        ("REC", Action::ToolRect),
        ("CO", Action::CmdDuplicateArray),
        ("RO", Action::ToolRotate),
        ("TR", Action::ToolTrim),
        ("EX", Action::ToolExtend),
        ("MI", Action::ToolMirror),
        ("M", Action::ToolSelect),
    ] {
        assert_eq!(p.aliases.get(alias), Some(&action), "{alias}");
    }
    // No two key actions of the profile share a chord (the lookup cannot hold two).
    let mut n = 0;
    for a in Action::ALL {
        n += k.chords(*a).len();
    }
    let distinct: std::collections::HashSet<Chord> = Action::ALL
        .iter()
        .flat_map(|a| k.chords(*a).to_vec())
        .collect();
    assert_eq!(n, distinct.len());
    // Every tool is reachable in the tool sets, once.
    let mut tools: Vec<Tool> = p.tool_sets.iter().flat_map(|s| s.tools.clone()).collect();
    assert_eq!(tools.len(), 29);
    tools.dedup();
    assert_eq!(tools.len(), 29);
    assert_eq!(p.tool_sets[0].title(), "Draw");
    assert_eq!(p.menus[2].title(), "Draw");
    // The terms are good for both languages and change the text.
    for tag in ["en-US", "et"] {
        let text = &p.terms[tag];
        assert!(kadakas_l10n::check_overlay(tag, text).is_empty(), "{tag}");
        let ids = kadakas_l10n::message_variables(tag).unwrap();
        for id in parse_ids(text) {
            assert!(ids.contains_key(&id), "{tag}: {id}");
        }
    }
    assert_eq!(p.overlay().len(), 2);
}

fn parse_ids(ftl: &str) -> Vec<String> {
    ftl.lines()
        .filter(|l| !l.starts_with(['#', ' ']) && l.contains('='))
        .map(|l| l.split('=').next().unwrap().trim().to_string())
        .collect()
}

#[test]
fn builtin_names_and_descriptions_are_translated() {
    for p in Profile::builtins() {
        let en = p.display_name();
        let about = p.display_description();
        assert!(!en.starts_with("uic-") && !about.starts_with("uic-"));
        kadakas_l10n::with_language("et", || {
            assert_ne!(p.display_name(), en);
            assert_ne!(p.display_description(), about);
        });
    }
    let mut odd = Profile::vectorworks();
    odd.id = "other".into();
    odd.name = "Other".into();
    odd.description = "D".into();
    assert_eq!(odd.display_name(), "Other");
    assert_eq!(odd.display_description(), "D");
}

#[test]
fn editing_a_profile() {
    let mut p = parse(MIN).profile;
    // A free chord.
    assert_eq!(p.chord_conflict(Action::ToolLine, chord("Q")), None);
    // A chord of another action is a conflict; taking it moves it.
    assert_eq!(
        p.chord_conflict(Action::ToolLine, chord("4")),
        Some(Action::ToolRect)
    );
    assert_eq!(p.chord_conflict(Action::ToolLine, chord("2")), None);
    let moved = p.set_chords(Action::ToolLine, &[chord("4"), chord("4"), chord("Q")]);
    assert_eq!(moved, vec![(chord("4"), Action::ToolRect)]);
    assert_eq!(p.keymap.chords(Action::ToolLine), [chord("4"), chord("Q")]);
    // Reset gives the default back and takes it from the new holder.
    let moved = p.reset_chords(Action::ToolRect, &Keymap::defaults());
    assert_eq!(moved, vec![(chord("4"), Action::ToolLine)]);
    assert_eq!(p.keymap.chords(Action::ToolRect), [chord("4")]);

    assert_eq!(p.set_alias("pl", Action::ToolPolyline), Some(None));
    assert_eq!(
        p.set_alias("PL", Action::ToolLine),
        Some(Some(Action::ToolPolyline))
    );
    assert_eq!(p.set_alias("!!", Action::ToolLine), None);
    assert!(p.remove_alias("PL") && !p.remove_alias("PL"));

    assert!(p.move_tool(0, 0, 2));
    assert_eq!(p.tool_sets[0].tools[2], Tool::Select);
    assert!(!p.move_tool(9, 0, 1) && !p.move_tool(0, 0, 99));
}

#[test]
fn the_set_lists_builtin_and_user_profiles() {
    let user = parse(r#"{"format":1,"name":"Vectorworks-like","id":"vectorworks"}"#).profile;
    let mut set = ProfileSet::new(vec![user]);
    assert_eq!(set.all().len(), 3);
    assert!(set.all()[0].builtin && set.all()[1].builtin && !set.all()[2].builtin);
    // The clashing id and name of the user profile were changed.
    assert_eq!(set.all()[2].id, "vectorworks-2");
    assert_eq!(set.all()[2].name, "Vectorworks-like 2");

    assert_eq!(set.find("autocad").unwrap().id, AUTOCAD_ID);
    assert_eq!(set.find("AutoCAD-like").unwrap().id, AUTOCAD_ID);
    assert_eq!(set.find("autocad_like").unwrap().id, AUTOCAD_ID);
    assert_eq!(set.find("vectorworks-2").unwrap().id, "vectorworks-2");
    assert!(set.find("nope").is_none() && set.get("nope").is_none());

    // A built-in profile cannot be edited, renamed or removed.
    assert!(set.user_mut(AUTOCAD_ID).is_none());
    assert_eq!(set.rename(AUTOCAD_ID, "x"), Err(RenameError::ReadOnly));
    assert_eq!(set.rename("nope", "x"), Err(RenameError::Unknown));
    assert!(!set.remove(AUTOCAD_ID) && !set.remove("nope"));
    // Editing a built-in profile makes a user copy first.
    let copy = set.editable(AUTOCAD_ID).unwrap();
    assert_ne!(copy, AUTOCAD_ID);
    assert_eq!(set.get(&copy).unwrap().name, "Copy of AutoCAD-like");
    assert!(!set.get(&copy).unwrap().builtin);
    assert_eq!(
        set.get(&copy).unwrap().aliases,
        set.get(AUTOCAD_ID).unwrap().aliases
    );
    // A user profile is its own answer.
    assert_eq!(set.editable(&copy).unwrap(), copy);
    assert!(set.editable("nope").is_none());
    set.user_mut(&copy).unwrap().set_alias("QQ", Action::Undo);
    assert!(set.get(&copy).unwrap().aliases.contains_key("QQ"));

    // A second copy gets another name and id.
    let copy2 = set.duplicate(AUTOCAD_ID).unwrap();
    assert_eq!(set.get(&copy2).unwrap().name, "Copy of AutoCAD-like 2");
    assert!(set.duplicate("nope").is_none());

    assert_eq!(set.rename(&copy, "  "), Err(RenameError::BadName));
    assert_eq!(
        set.rename(&copy, &"x".repeat(MAX_NAME + 1)),
        Err(RenameError::BadName)
    );
    assert_eq!(
        set.rename(&copy, "autocad-LIKE"),
        Err(RenameError::NameTaken)
    );
    assert_eq!(set.rename(&copy, "Mine"), Ok(()));
    assert_eq!(set.rename(&copy, "Mine"), Ok(()));
    assert_eq!(set.get(&copy).unwrap().name, "Mine");

    assert!(set.remove(&copy2));
    assert!(set.get(&copy2).is_none());
}

#[test]
fn a_profile_is_imported_with_a_free_id_and_name() {
    let mut set = ProfileSet::new(vec![]);
    let text = Profile::builtins()[1].to_json();
    let (id, problems) = set.import(&text).unwrap();
    assert!(problems.is_empty());
    assert_eq!(id, "autocad-2");
    let p = set.get(&id).unwrap();
    assert!(!p.builtin);
    assert_eq!(p.name, "AutoCAD-like 2");
    assert_eq!(p.aliases, set.get(AUTOCAD_ID).unwrap().aliases);
    let (id2, _) = set.import(r#"{"format":1,"name":"Z","keymap":3}"#).unwrap();
    assert_eq!(id2, "imported");
    assert!(matches!(set.import("x"), Err(ParseError::NotJson(_))));
}

#[test]
fn names_and_ids_are_made_from_text() {
    assert_eq!(slug("AutoCAD-like"), "autocad-like");
    assert_eq!(slug("  My  Profile!! "), "my-profile");
    assert_eq!(slug("%%%"), "profile");
    assert_eq!(slug(&"a".repeat(40)).len(), 24);
    assert_eq!(slug("aaaaaaaaaaaaaaaaaaaaaaa-b"), "aaaaaaaaaaaaaaaaaaaaaaa");
    let set = ProfileSet::new(vec![]);
    assert_eq!(set.unique_id("new thing"), "new-thing");
    assert_eq!(set.unique_id(&"v".repeat(40)).len(), 24);
    assert_eq!(set.unique_name("  "), "Profile");
    assert_eq!(set.unique_name("Fresh"), "Fresh");
    assert!(layout::valid_id(&set.unique_id("vectorworks")));
    let long = ProfileSet::new(vec![
        parse(r#"{"format":1,"name":"x","id":"vvvvvvvvvvvvvvvvvvvvvvvv"}"#).profile,
    ]);
    assert_eq!(
        long.unique_id("vvvvvvvvvvvvvvvvvvvvvvvv"),
        "vvvvvvvvvvvvvvvvvvvvvv-2"
    );
}

#[test]
fn overlay_pairs_are_listed() {
    let p = parse(r#"{"format":1,"name":"a","terms":{"et":"common-ok = Okei\n"}}"#).profile;
    assert_eq!(p.overlay(), vec![("et", "common-ok = Okei\n")]);
}

#[test]
fn every_message_of_the_builtin_terms_may_be_replaced() {
    for p in Profile::builtins() {
        for (tag, text) in &p.terms {
            let ids = parse_ids(text);
            assert!(!ids.is_empty());
            for id in ids {
                assert!(
                    kadakas_l10n::is_overlayable(&id),
                    "{}: {tag}: {id} is not overlayable",
                    p.id
                );
            }
            assert!(text.len() < kadakas_l10n::MAX_TEXT);
        }
        // Both languages replace the same messages.
        if p.terms.len() == 2 {
            assert_eq!(parse_ids(&p.terms["en-US"]), parse_ids(&p.terms["et"]));
        }
    }
}

/// The labels that sit next to each other in Object Info and in the Navigation palette must
/// read differently, with and without the terms of any built-in profile.
#[test]
fn neighbor_labels_stay_different_under_every_overlay() {
    let groups: [&[&str]; 4] = [
        // Object Info: the layer and the class of an object.
        &["panels-layer", "panels-class"],
        // Navigation: the lists of design layers, classes and sheet layers.
        &[
            "panels-design-layers",
            "panels-classes",
            "sheets-ui-sheet-layers",
        ],
        &[
            "panels-add-design-layer",
            "panels-add-class",
            "sheets-ui-add-sheet-layer",
        ],
        &["io-dialog-design-layer", "io-dialog-class-from-attribute"],
    ];
    for p in Profile::builtins() {
        for lang in kadakas_l10n::languages() {
            kadakas_l10n::with_language(lang.tag, || {
                kadakas_l10n::with_overlay(&p.overlay(), || {
                    for g in groups {
                        let texts: Vec<String> = g
                            .iter()
                            .map(|id| kadakas_l10n::translate(id, &[]))
                            .collect();
                        let distinct: std::collections::HashSet<&String> = texts.iter().collect();
                        assert_eq!(
                            distinct.len(),
                            texts.len(),
                            "{} {}: {g:?} {texts:?}",
                            p.id,
                            lang.tag
                        );
                    }
                });
            });
        }
    }
}

#[test]
fn big_files_and_long_lists_are_limited() {
    let big = format!(
        r#"{{"format":1,"name":"a","description":"{}"}}"#,
        " ".repeat(MAX_FILE_BYTES)
    );
    assert_eq!(Profile::parse(&big, "x").unwrap_err(), ParseError::TooLarge);
    assert!(!ParseError::TooLarge.text().starts_with("uic-"));

    let long = "d".repeat(MAX_DESCRIPTION + 5);
    let p = parse(&format!(
        r#"{{"format":1,"name":"a","description":"{long}"}}"#
    ));
    assert_eq!(p.profile.description.chars().count(), MAX_DESCRIPTION);
    assert_eq!(p.problems, vec![Problem::TooLong("description".into())]);
    assert!(!p.problems[0].text().starts_with("uic-"));

    let aliases: Vec<String> = (0..MAX_ALIASES + 3)
        .map(|i| format!(r#""A{i}":"tool.line""#))
        .collect();
    let menus: Vec<String> = (0..MAX_MENUS + 2)
        .map(|i| format!(r#"{{"id":"m{i}","name":"M","items":[]}}"#))
        .collect();
    let sets: Vec<String> = (0..MAX_TOOL_SETS + 2)
        .map(|i| format!(r#"{{"id":"s{i}","name":"S","tools":[]}}"#))
        .collect();
    let items: Vec<String> = (0..MAX_ITEMS + 2).map(|_| r#""-""#.to_string()).collect();
    let text = format!(
        r#"{{"format":1,"name":"a","aliases":{{{}}},"menus":[{},{{"id":"big","name":"B","items":[{}]}}],"tool_sets":[{}]}}"#,
        aliases.join(","),
        menus.join(","),
        items.join(","),
        sets.join(",")
    );
    let p = parse(&text);
    assert_eq!(p.profile.aliases.len(), MAX_ALIASES);
    assert_eq!(p.profile.menus.len(), MAX_MENUS);
    assert_eq!(p.profile.tool_sets.len(), MAX_TOOL_SETS);
    for part in ["aliases", "menus", "tool_sets"] {
        assert!(
            p.problems.contains(&Problem::TooMany(part.into())),
            "{part}"
        );
    }
    let one = parse(&format!(
        r#"{{"format":1,"name":"a","menus":[{{"id":"big","name":"B","items":[{}]}}]}}"#,
        items.join(",")
    ));
    assert_eq!(one.profile.menus[0].items.len(), MAX_ITEMS);
    assert!(one.problems.contains(&Problem::TooMany("big.items".into())));
    assert!(!one.problems[0].text().starts_with("uic-"));
}

#[test]
fn a_copy_remembers_its_base_and_reset_goes_back_to_it() {
    let mut set = ProfileSet::new(vec![]);
    let copy = set.duplicate(AUTOCAD_ID).unwrap();
    assert_eq!(
        set.get(&copy).unwrap().based_on.as_deref(),
        Some(AUTOCAD_ID)
    );
    // The file keeps the base.
    let back = parse(&set.get(&copy).unwrap().to_json()).profile;
    assert_eq!(back.based_on.as_deref(), Some(AUTOCAD_ID));
    // Change a key, then reset: the key of the AutoCAD-like profile comes back, not the
    // Vectorworks-like default.
    set.user_mut(&copy)
        .unwrap()
        .set_chords(Action::ZoomFit, &[chord("Q")]);
    let moved = set.reset_chords(&copy, Action::ZoomFit).unwrap();
    assert!(moved.is_empty());
    assert_eq!(
        set.get(&copy).unwrap().keymap.chords(Action::ZoomFit),
        [chord("Home")]
    );
    // A copy of a copy follows the chain up to the built-in profile.
    let copy2 = set.duplicate(&copy).unwrap();
    assert_eq!(set.base_keymap(&copy2), set.get(AUTOCAD_ID).unwrap().keymap);
    // A broken or missing base gives the defaults.
    set.user_mut(&copy2).unwrap().based_on = Some("gone".into());
    assert_eq!(set.base_keymap(&copy2), Keymap::defaults());
    set.user_mut(&copy2).unwrap().based_on = None;
    assert_eq!(set.base_keymap("nope"), Keymap::defaults());
    assert!(set.reset_chords(AUTOCAD_ID, Action::ZoomFit).is_none());
    // A bad `based_on` in a file is dropped.
    assert!(
        parse(r#"{"format":1,"name":"a","based_on":"Bad Id"}"#)
            .profile
            .based_on
            .is_none()
    );
}

#[test]
fn a_builtin_keymap_can_be_replaced_and_is_copied() {
    let mut set = ProfileSet::new(vec![]);
    let (km, w) = Keymap::with_overrides(r#"{"tool.line": "G"}"#);
    assert!(w.is_empty());
    assert!(set.set_builtin_keymap(VECTORWORKS_ID, km.clone()));
    assert!(!set.set_builtin_keymap("nope", km.clone()));
    assert_eq!(set.get(VECTORWORKS_ID).unwrap().keymap, km);
    let copy = set.duplicate(VECTORWORKS_ID).unwrap();
    assert_eq!(
        set.get(&copy).unwrap().keymap.action_for(chord("G")),
        Some(Action::ToolLine)
    );
    // Resetting an action in the copy gives the keys that work in the base.
    assert_eq!(set.base_keymap(&copy), km);
}

#[test]
fn valid_ids_are_kept_and_others_are_made_from_the_text() {
    let set = ProfileSet::new(vec![]);
    assert_eq!(set.unique_id("my_profile"), "my_profile");
    assert_eq!(set.unique_id("My Profile"), "my-profile");
    assert_eq!(slug("a_b"), "a_b");
    assert_eq!(slug("___"), "profile");
    let mut set = ProfileSet::new(vec![]);
    let p = parse(r#"{"format":1,"name":"x","id":"user_one"}"#).profile;
    assert_eq!(set.add_user(p.clone()), "user_one");
    assert_eq!(set.add_user(p), "user_one-2");
}

#[test]
fn the_autocad_profile_has_the_3d_aliases_and_the_model_menu() {
    let b = Profile::builtins();
    let p = &b[1];
    for (alias, action) in [
        ("EXTRUDE", Action::CmdExtrude),
        ("EXT", Action::CmdExtrude),
        ("REVOLVE", Action::CmdSweep),
        ("REV", Action::CmdSweep),
        ("3DPOLY", Action::ToolPolygon3d),
        ("3P", Action::ToolPolygon3d),
    ] {
        assert_eq!(p.aliases.get(alias), Some(&action), "{alias}");
    }
    assert!(p.menus.iter().any(|m| m.id == "model"));
    assert!(
        p.tool_sets
            .iter()
            .any(|s| s.tools.contains(&Tool::Polygon3d))
    );
    // The terms follow the family: Sweep is Revolve, the 3D Polygon is a 3D Polyline.
    kadakas_l10n::with_language("en-US", || {
        kadakas_l10n::with_overlay(&p.overlay(), || {
            assert_eq!(layout::Cmd::Sweep.label(), "Revolve...");
            assert_eq!(Tool::Polygon3d.name(), "3D Polyline");
        });
    });
}
