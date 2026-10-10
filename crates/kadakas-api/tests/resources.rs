//! Groups, symbols, patterns and the Resource Manager through the JSON-RPC methods.

use kadakas_api::{METHODS, Session};
use kadakas_core::uid::AsUid;
use serde_json::{Value, json};

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, (i64, String)> {
    s.handle(method, params).map_err(|e| (e.code, e.message))
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {e:?}"))
}

fn err(s: &mut Session, method: &str, params: Value) -> String {
    call(s, method, params).unwrap_err().1
}

fn exec(s: &mut Session, command: Value) -> Vec<kadakas_core::uid::Uid> {
    ok(s, "execute", json!({ "command": command }))["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_uid().unwrap())
        .collect()
}

fn circle(s: &mut Session, x: f64) -> kadakas_core::uid::Uid {
    exec(
        s,
        json!({"op": "add_object", "shape": {"kind": "circle", "center": [x, 0], "radius": 1}}),
    )[0]
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kadakas-api-resources-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn every_new_method_is_listed_and_routed() {
    let mut s = Session::with_sequential_ids();
    let listed: Vec<&str> = METHODS.iter().map(|(n, _)| *n).collect();
    for m in [
        "group.create",
        "group.enter",
        "symbol.place",
        "resource.import_from_file",
        "resource.purge_unused",
        "resource.list",
        "pattern.segments",
        "library.list",
    ] {
        assert!(listed.contains(&m), "{m}");
    }
    let d = ok(&mut s, "describe", json!({}));
    assert!(d["resources"]["examples"].as_array().unwrap().len() >= 5);
    // Every example is a known method.
    for e in d["resources"]["examples"].as_array().unwrap() {
        assert!(listed.contains(&e["method"].as_str().unwrap()), "{e}");
    }
}

#[test]
fn groups_select_move_rotate_copy_and_undo() {
    let mut s = Session::with_sequential_ids();
    let (a, b, c) = (
        circle(&mut s, 0.0),
        circle(&mut s, 10.0),
        circle(&mut s, 20.0),
    );
    let g = ok(
        &mut s,
        "group.create",
        json!({"ids": [a, b], "name": "pair"}),
    )["id"]
        .as_uid()
        .unwrap();
    assert_eq!(ok(&mut s, "selection.get", json!({}))["ids"], json!([g]));
    assert_eq!(
        ok(&mut s, "group.members", json!({"id": g}))["members"],
        json!([a, b])
    );
    // A click on a member selects the group. Outside the context the member is not a target of c.
    assert_eq!(
        ok(&mut s, "selection.click", json!({"id": a}))["ids"],
        json!([g])
    );
    assert_eq!(
        ok(&mut s, "selection.click", json!({"id": c, "add": true}))["ids"],
        json!([g, c])
    );
    // Enter the group: a click selects the member, and a click outside is refused.
    ok(&mut s, "group.enter", json!({"id": g}));
    assert_eq!(
        ok(&mut s, "group.context", json!({}))["context"],
        json!([g])
    );
    assert_eq!(
        ok(&mut s, "selection.click", json!({"id": a}))["ids"],
        json!([a])
    );
    assert!(err(&mut s, "selection.click", json!({"id": c})).contains("not inside"));
    assert!(err(&mut s, "group.enter", json!({"id": c})).contains("not a group"));
    assert_eq!(ok(&mut s, "group.exit", json!({}))["context"], json!([]));
    assert!(err(&mut s, "group.exit", json!({})).contains("not in a group"));
    assert_eq!(ok(&mut s, "selection.get", json!({}))["ids"], json!([g]));
    // Move, rotate and copy act on the whole group.
    ok(&mut s, "group.move", json!({"ids": [g], "delta": [0, 5]}));
    let center = |s: &mut Session, id: kadakas_core::uid::Uid| {
        ok(s, "objects.get", json!({"id": id}))["shape"]["center"].clone()
    };
    assert_eq!(center(&mut s, b), json!([10.0, 5.0]));
    ok(
        &mut s,
        "group.rotate",
        json!({"ids": [g], "center": [0, 0], "angle": std::f64::consts::FRAC_PI_2}),
    );
    let rotated = center(&mut s, b);
    assert!(
        (rotated[0].as_f64().unwrap() + 5.0).abs() < 1e-9
            && (rotated[1].as_f64().unwrap() - 10.0).abs() < 1e-9,
        "{rotated}"
    );
    let copy = ok(&mut s, "group.copy", json!({"ids": [g], "delta": [100, 0]}));
    assert_eq!(copy["members"].as_array().unwrap().len(), 2);
    assert_eq!(
        ok(&mut s, "objects.list", json!({"kind": "group"}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Undo the copy, the rotation and the move.
    for _ in 0..3 {
        ok(&mut s, "undo", json!({}));
    }
    assert_eq!(center(&mut s, b), json!([10.0, 0.0]));
    // Ungroup frees the members and selects them.
    assert_eq!(
        ok(&mut s, "group.ungroup", json!({"ids": [g]}))["members"],
        json!([a, b])
    );
    assert_eq!(ok(&mut s, "selection.get", json!({}))["ids"], json!([a, b]));
    assert!(err(&mut s, "group.ungroup", json!({"ids": [a]})).contains("not a group"));
    ok(&mut s, "selection.set", json!({"ids": []}));
    assert!(err(&mut s, "group.create", json!({})).contains("select objects"));
    ok(&mut s, "selection.set", json!({"ids": [a, b]}));
    let g2 = ok(&mut s, "group.create", json!({}))["id"]
        .as_uid()
        .unwrap();
    // Deleting a member that the context needs leaves the context.
    ok(&mut s, "group.enter", json!({"id": g2}));
    ok(&mut s, "undo", json!({}));
    assert_eq!(ok(&mut s, "group.context", json!({}))["context"], json!([]));
}

#[test]
fn symbols_place_edit_and_follow_the_definition() {
    let mut s = Session::with_sequential_ids();
    let sym = ok(&mut s, "symbol.create", json!({"name": "Bench", "items": [{"shape": {"kind": "rect", "min": [-500, -200], "max": [500, 200]}}]}))["id"].as_uid().unwrap();
    let a = ok(
        &mut s,
        "symbol.place",
        json!({"symbol": "bench", "at": [1000, 1000]}),
    )["id"]
        .as_uid()
        .unwrap();
    let b = ok(
        &mut s,
        "symbol.place",
        json!({"symbol": sym, "at": [5000, 0], "rotation": 1.0, "scale": 2.0, "scale_y": 1.0}),
    )["id"]
        .as_uid()
        .unwrap();
    let list = ok(&mut s, "symbol.list", json!({}));
    assert_eq!(list["symbols"][0]["instances"], 2);
    assert_eq!(list["symbols"][0]["kind"], "2d");
    // Bounds show in the document summary and the SVG.
    let svg = ok(&mut s, "render.svg", json!({}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(svg.contains("<rect") && svg.contains("<polygon"), "{svg}");
    // Change the definition: all instances follow.
    ok(
        &mut s,
        "resource.update",
        json!({"id": sym, "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 300}}]}),
    );
    let svg = ok(&mut s, "render.svg", json!({}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(svg.contains("<circle") && !svg.contains("<rect"), "{svg}");
    // symbol.set changes one instance.
    ok(
        &mut s,
        "symbol.set",
        json!({"id": a, "at": [0, 0], "scale_y": null, "scale": 3}),
    );
    assert_eq!(
        ok(&mut s, "objects.get", json!({"id": a}))["shape"]["scale"],
        3.0
    );
    assert!(
        err(
            &mut s,
            "symbol.set",
            json!({"id": "00000000-0000-0000-0000-00000000270f", "scale": 1})
        )
        .contains("does not exist")
    );
    // Edit in place, then end.
    let g = ok(&mut s, "symbol.edit", json!({"id": b}))["group"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "group.context", json!({}))["context"],
        json!([g])
    );
    let kids = ok(&mut s, "group.members", json!({"id": g}))["members"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(kids.len(), 1);
    exec(
        &mut s,
        json!({"op": "modify_object", "id": kids[0], "shape": {"kind": "circle", "center": [5000, 0], "radius": 900}}),
    );
    ok(&mut s, "symbol.edit_end", json!({}));
    assert_eq!(ok(&mut s, "group.context", json!({}))["context"], json!([]));
    assert_eq!(
        ok(&mut s, "symbol.get", json!({"id": "Bench"}))["items"][0]["shape"]["radius"],
        900.0
    );
    assert!(err(&mut s, "symbol.edit_end", json!({})).contains("not editing"));
    // Convert one instance to a group of plain objects.
    let groups = ok(&mut s, "symbol.convert_to_group", json!({"ids": [a]}))["groups"].clone();
    assert_eq!(groups.as_array().unwrap().len(), 1);
    assert_eq!(
        ok(&mut s, "objects.list", json!({"kind": "circle"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // From objects, with replace.
    let c1 = circle(&mut s, 7000.0);
    let made = ok(
        &mut s,
        "symbol.from_objects",
        json!({"name": "Dot", "ids": [c1], "replace": true}),
    );
    assert!(made["instance"].is_string());
    assert!(
        err(
            &mut s,
            "symbol.place",
            json!({"symbol": "Nope", "at": [0, 0]})
        )
        .contains("no symbol")
    );
    assert!(
        err(
            &mut s,
            "symbol.create",
            json!({"name": "Bad", "items": [{"shape": {"kind": "group"}}]})
        )
        .contains("is a group")
    );
    assert!(err(&mut s, "symbol.get", json!({"id": "Nope"})).contains("no symbol"));
}

#[test]
fn hatches_tiles_and_line_types_are_clipped_to_the_shape() {
    let mut s = Session::with_sequential_ids();
    let hatch = ok(&mut s, "resource.create", json!({"type": "hatch", "name": "Diag", "units": "world", "lines": [{"angle": 0, "spacing": 100}]}))["id"].as_uid().unwrap();
    let ring = json!([[0, 0], [1000, 0], [1000, 500], [0, 500]]);
    let r = ok(
        &mut s,
        "pattern.segments",
        json!({"pattern": "Diag", "ring": ring}),
    );
    // Total length times spacing is the area: 1000 x 500.
    let area = r["total_length"].as_f64().unwrap() * 100.0;
    assert!((area - 500_000.0).abs() < 0.05 * 500_000.0, "{area}");
    let holed = ok(
        &mut s,
        "pattern.segments",
        json!({"pattern": hatch, "ring": ring, "holes": [[[250, 100], [750, 100], [750, 400], [250, 400]]]}),
    );
    assert!(holed["total_length"].as_f64().unwrap() < r["total_length"].as_f64().unwrap());
    assert!(
        call(
            &mut s,
            "pattern.segments",
            json!({"pattern": hatch, "ring": [[0, 0], [null, 1]]})
        )
        .is_err()
    );
    // A tile needs a symbol: by name.
    ok(
        &mut s,
        "symbol.create",
        json!({"name": "Cross", "items": [{"shape": {"kind": "line", "a": [-20, 0], "b": [20, 0]}}, {"shape": {"kind": "line", "a": [0, -20], "b": [0, 20]}}]}),
    );
    ok(
        &mut s,
        "resource.create",
        json!({"type": "tile", "name": "Crosses", "symbol": "Cross", "width": 100, "height": 100, "units": "world"}),
    );
    let t = ok(
        &mut s,
        "pattern.segments",
        json!({"pattern": "Crosses", "ring": ring}),
    );
    assert!(t["count"].as_u64().unwrap() > 50);
    assert!(
        err(
            &mut s,
            "pattern.segments",
            json!({"pattern": "Cross", "ring": ring})
        )
        .contains("hatch or a tile")
    );
    let lt = ok(&mut s, "resource.create", json!({"type": "line_type", "name": "Dashed", "elements": [{"kind": "dash", "length": 3}, {"kind": "gap", "length": 1.5}]}))["id"].as_uid().unwrap();
    // Use them in attributes: a rect with a hatch and a dashed outline.
    let rect = exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1000, 500]}, "attrs": {"fill_pattern": hatch, "line_type": lt}}),
    )[0];
    let svg = ok(&mut s, "render.svg", json!({}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        svg.contains("data-pattern=\"fill\"") && svg.contains("data-pattern=\"line_type\""),
        "{svg}"
    );
    // The hatch is in use now. A delete is refused, and a purge keeps it.
    assert!(err(&mut s, "resource.delete", json!({"id": hatch})).contains("used by"));
    let purge = ok(&mut s, "resource.purge_unused", json!({"dry_run": true}));
    let names: Vec<&str> = purge["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Crosses", "Cross"]);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [rect]}}),
    );
    let purge = ok(&mut s, "resource.purge_unused", json!({}));
    assert_eq!(purge["removed"].as_array().unwrap().len(), 4);
    assert_eq!(
        ok(&mut s, "resource.list", json!({}))["resources"],
        json!([])
    );
    // One undo step brings all four back.
    ok(&mut s, "undo", json!({}));
    assert_eq!(
        ok(&mut s, "resource.list", json!({}))["resources"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(
        call(
            &mut s,
            "resource.create",
            json!({"type": "hatch", "name": "Bad", "lines": []})
        )
        .is_err()
    );
    assert!(
        err(
            &mut s,
            "resource.create",
            json!({"type": "wave", "name": "Bad"})
        )
        .contains("unknown type")
    );
}

#[test]
fn the_resource_manager_lists_renames_duplicates_and_draws_thumbnails() {
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "plant_style.create",
        json!({"name": "Oak", "spread": 8000, "height": 15000}),
    );
    ok(
        &mut s,
        "resource.create",
        json!({"type": "texture", "name": "Bark", "color": [90, 60, 30, 255], "size": [400, 400]}),
    );
    let hatch = ok(&mut s, "resource.create", json!({"type": "hatch", "name": "H", "lines": [{"angle": 45, "spacing": 2, "dash": [4, 1]}]}))["id"].as_uid().unwrap();
    let all = ok(&mut s, "resource.list", json!({"thumbnails": true}));
    assert_eq!(all["resources"].as_array().unwrap().len(), 3);
    assert!(
        all["resources"][0]["thumbnail"]
            .as_str()
            .unwrap()
            .starts_with("<svg")
    );
    let only = ok(&mut s, "resource.list", json!({"type": "texture"}));
    assert_eq!(only["resources"][0]["name"], "Bark");
    assert_eq!(
        ok(&mut s, "resource.list", json!({"search": "oak"}))["resources"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    ok(
        &mut s,
        "resource.rename",
        json!({"id": hatch, "name": "Dashed diagonal"}),
    );
    let dup = ok(
        &mut s,
        "resource.duplicate",
        json!({"id": "Dashed diagonal"}),
    )["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "resource.get", json!({"id": dup}))["name"],
        "Dashed diagonal copy"
    );
    ok(
        &mut s,
        "resource.duplicate",
        json!({"id": "Oak", "name": "Oak 2"}),
    );
    assert!(
        err(
            &mut s,
            "resource.rename",
            json!({"id": dup, "name": "dashed diagonal"})
        )
        .contains("exists already")
    );
    // Update a graphic resource: fields you give replace the old ones.
    let v = ok(
        &mut s,
        "resource.update",
        json!({"id": dup, "name": "Fine", "lines": [{"angle": 0, "spacing": 1}]}),
    );
    assert_eq!(v["lines"].as_array().unwrap().len(), 1);
    assert!(
        err(&mut s, "resource.update", json!({"id": "Oak", "name": "x"}))
            .contains("plant_style.update")
    );
    let svg = ok(
        &mut s,
        "resource.thumbnail",
        json!({"id": "Bark", "size": 32}),
    )["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(svg.contains("width=\"32\""));
    // Reassign on delete.
    ok(&mut s, "resource.delete", json!({"id": dup}));
    assert!(err(&mut s, "resource.get", json!({"id": dup})).contains("no resource"));
    assert!(call(&mut s, "resource.list", json!("x")).is_err());
}

#[test]
fn libraries_import_with_dependencies_and_follow_the_path_policy() {
    let mut src = Session::with_sequential_ids();
    let hatch = ok(
        &mut src,
        "resource.create",
        json!({"type": "hatch", "name": "Diag", "lines": [{"angle": 45, "spacing": 2}]}),
    )["id"]
        .as_uid()
        .unwrap();
    ok(
        &mut src,
        "symbol.create",
        json!({"name": "Dot", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 5}, "attrs": {"fill_pattern": hatch}}]}),
    );
    ok(
        &mut src,
        "resource.create",
        json!({"type": "texture", "name": "Bark", "color": [1, 2, 3, 255]}),
    );
    exec(&mut src, json!({"op": "add_class", "name": "Trees"}));
    exec(
        &mut src,
        json!({"op": "add_layer", "name": "Site", "elevation": 100}),
    );
    let file = tmp("lib.kad");
    let text = serde_json::to_string(&ok(&mut src, "document.get", json!({}))).unwrap();
    std::fs::write(&file, text).unwrap();

    let mut s = Session::with_sequential_ids();
    let listing = ok(&mut s, "resource.list_file", json!({"path": file}));
    assert_eq!(listing["resources"].as_array().unwrap().len(), 3);
    assert!(
        listing["classes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "Trees")
    );
    // Pick the symbol by name: the hatch comes along.
    let r = ok(
        &mut s,
        "resource.import_from_file",
        json!({"path": file, "items": [{"type": "symbol", "name": "Dot"}], "classes": ["Trees"], "layers": ["Site"]}),
    );
    let imported = r["imported"].as_array().unwrap();
    assert_eq!(imported.len(), 2);
    assert!(
        imported
            .iter()
            .any(|i| i["name"] == "Diag" && i["dependency"] == true && i["status"] == "added")
    );
    assert_eq!(r["classes_added"], json!(["Trees"]));
    assert_eq!(r["layers_added"], json!(["Site"]));
    assert_eq!(
        ok(&mut s, "resource.list", json!({}))["resources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let sym = ok(&mut s, "symbol.get", json!({"id": "Dot"}));
    let new_hatch = ok(&mut s, "resource.get", json!({"id": "Diag"}))["id"].clone();
    assert_eq!(sym["items"][0]["attrs"]["fill_pattern"], new_hatch);
    // One undo step.
    ok(&mut s, "undo", json!({}));
    assert_eq!(
        ok(&mut s, "resource.list", json!({}))["resources"],
        json!([])
    );
    // A name clash renames, or reuses, depending on the choice.
    ok(
        &mut s,
        "resource.create",
        json!({"type": "texture", "name": "Bark", "color": [9, 9, 9, 255]}),
    );
    let r = ok(
        &mut s,
        "resource.import_from_file",
        json!({"path": file, "items": ["Bark"]}),
    );
    assert_eq!(r["imported"][0]["status"], "renamed");
    assert_eq!(r["imported"][0]["name"], "Bark 2");
    let r = ok(
        &mut s,
        "resource.import_from_file",
        json!({"path": file, "items": ["Bark"], "on_conflict": "reuse"}),
    );
    assert_eq!(r["imported"][0]["status"], "reused");
    let r = ok(
        &mut s,
        "resource.import_from_file",
        json!({"path": file, "all": true, "on_conflict": "replace"}),
    );
    assert!(
        r["imported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["status"] == "replaced")
    );
    // Errors.
    assert!(
        err(&mut s, "resource.import_from_file", json!({"path": file}))
            .contains("name what to import")
    );
    assert!(
        call(
            &mut s,
            "resource.import_from_file",
            json!({"path": file, "items": ["Nope"]})
        )
        .is_err()
    );
    assert!(
        call(
            &mut s,
            "resource.import_from_file",
            json!({"path": file, "items": ["Nope"]})
        )
        .is_err()
    );
    assert!(
        call(
            &mut s,
            "resource.import_from_file",
            json!({"path": tmp("missing.kad"), "all": true})
        )
        .is_err()
    );
    assert!(call(&mut s, "resource.import_from_file", json!({"all": true})).is_err());
    assert!(
        call(
            &mut s,
            "resource.import_from_file",
            json!({"path": file, "all": true, "on_conflict": "x"})
        )
        .is_err()
    );
    // A restricted session refuses a file outside its folders.
    let mut restricted = Session::with_sequential_ids();
    restricted.paths =
        kadakas_api::PathPolicy::restricted([std::env::temp_dir().join("kadakas-nowhere-else")]);
    let e = err(
        &mut restricted,
        "resource.import_from_file",
        json!({"path": file, "all": true}),
    );
    assert!(e.contains("outside") || e.contains("cannot read"), "{e}");
    let mut allowed = Session::with_sequential_ids();
    allowed.paths = kadakas_api::PathPolicy::restricted([file.parent().unwrap().to_path_buf()]);
    ok(
        &mut allowed,
        "resource.import_from_file",
        json!({"path": file, "all": true}),
    );
    // Export a library from a document and read it back.
    let out = tmp("exported.kad");
    let e = ok(
        &mut s,
        "resource.export_to_file",
        json!({"path": out, "overwrite": true}),
    );
    assert!(e["count"].as_u64().unwrap() >= 2);
    let listing = ok(&mut s, "resource.list_file", json!({"path": out}));
    assert!(listing["resources"].as_array().unwrap().len() >= 2);
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn bad_parameters_are_refused_with_a_clear_message() {
    let mut s = Session::with_sequential_ids();
    let a = circle(&mut s, 0.0);
    let b = circle(&mut s, 1.0);
    let g = ok(&mut s, "group.create", json!({"ids": [a, b]}))["id"]
        .as_uid()
        .unwrap();
    let c = circle(&mut s, 5.0);
    let g2 = ok(&mut s, "group.create", json!({"ids": [c]}))["id"]
        .as_uid()
        .unwrap();
    ok(&mut s, "group.enter", json!({"id": g}));
    assert!(err(&mut s, "group.enter", json!({"id": g2})).contains("not inside"));
    assert!(err(&mut s, "group.members", json!({"id": a})).contains("not a group"));
    assert!(err(&mut s, "group.move", json!({"ids": [a]})).contains("missing 'delta'"));
    assert!(err(&mut s, "group.move", json!({"ids": [a], "delta": "x"})).contains("[x, y]"));
    assert!(
        err(
            &mut s,
            "group.rotate",
            json!({"ids": [a], "center": [0, 0], "angle": "x"})
        )
        .contains("number")
    );
    assert!(
        err(
            &mut s,
            "selection.click",
            json!({"id": "00000000-0000-0000-0000-00000000270f"})
        )
        .contains("does not exist")
    );
    assert!(err(&mut s, "group.create", json!({"ids": [a], "name": 5})).contains("text"));
    // Ctrl-click toggles a member in and out of the selection.
    ok(&mut s, "selection.click", json!({"id": a}));
    assert_eq!(
        ok(&mut s, "selection.click", json!({"id": b, "add": true}))["ids"],
        json!([a, b])
    );
    assert_eq!(
        ok(&mut s, "selection.click", json!({"id": b, "add": true}))["ids"],
        json!([a])
    );
    assert!(call(&mut s, "resource.list", json!(5)).is_err());
    let sym = ok(&mut s, "symbol.create", json!({"name": "S", "insertion": [10, 10], "items": [{"shape": {"kind": "circle", "center": [10, 10], "radius": 5}}]}))["id"].as_uid().unwrap();
    assert!(
        err(
            &mut s,
            "symbol.create",
            json!({"name": "S2", "insertion": "x"})
        )
        .contains("[x, y]")
    );
    ok(
        &mut s,
        "resource.create",
        json!({"type": "symbol", "name": "S3", "insertion": [1, 2]}),
    );
    assert!(
        err(
            &mut s,
            "resource.create",
            json!({"type": "symbol", "name": "S4", "insertion": 5})
        )
        .contains("[x, y]")
    );
    let inst = ok(&mut s, "symbol.place", json!({"symbol": sym, "at": [0, 0]}))["id"]
        .as_uid()
        .unwrap();
    let other = ok(&mut s, "symbol.create", json!({"name": "Other", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 5}}]}))["id"].as_uid().unwrap();
    ok(
        &mut s,
        "symbol.set",
        json!({"id": inst, "symbol": "Other", "scale_y": 2}),
    );
    assert_eq!(
        ok(&mut s, "objects.get", json!({"id": inst}))["shape"]["symbol"],
        other
    );
    ok(&mut s, "symbol.set", json!({"id": inst, "scale_y": 3}));
    assert_eq!(
        ok(&mut s, "objects.get", json!({"id": inst}))["shape"]["scale_y"],
        3.0
    );
    assert!(
        err(&mut s, "symbol.set", json!({"id": a, "scale": 2})).contains("not a symbol instance")
    );
    // Edit end with a group named.
    let eg = ok(&mut s, "symbol.edit", json!({"id": inst}))["group"].clone();
    ok(&mut s, "symbol.edit_end", json!({"group": eg}));
    // Updates and renames with bad types.
    assert!(err(&mut s, "resource.update", json!({"id": other, "name": 5})).contains("text"));
    assert!(
        err(
            &mut s,
            "resource.duplicate",
            json!({"id": other, "name": 5})
        )
        .contains("text")
    );
    assert!(
        err(
            &mut s,
            "resource.list_file",
            json!({"path": tmp("none.kad")})
        )
        .contains("cannot read")
    );
    assert!(
        err(
            &mut s,
            "pattern.segments",
            json!({"pattern": other, "ring": [[0, 0], [1, 0], [1, 1]]})
        )
        .contains("hatch or a tile")
    );
    let h = ok(
        &mut s,
        "resource.create",
        json!({"type": "hatch", "name": "H", "lines": [{"spacing": 1}]}),
    )["id"]
        .as_uid()
        .unwrap();
    assert!(
        call(
            &mut s,
            "pattern.segments",
            json!({"pattern": h, "ring": [[0, 0], [null, 0], [1, 1]]})
        )
        .is_err()
    );
    assert!(
        call(
            &mut s,
            "pattern.segments",
            json!({"pattern": h, "ring": "x"})
        )
        .is_err()
    );
    ok(
        &mut s,
        "resource.delete",
        json!({"id": h, "reassign": null}),
    );
    // A line type with a mark given by name.
    ok(
        &mut s,
        "resource.create",
        json!({"type": "line_type", "name": "Fence", "elements": [{"kind": "dash", "length": 2}, {"kind": "mark", "symbol": "Other"}, {"kind": "gap", "length": 1}]}),
    );
    assert_eq!(
        ok(&mut s, "resource.get", json!({"id": "Fence"}))["elements"][1]["symbol"],
        other
    );
    assert!(err(&mut s, "resource.get", json!({"id": true})).contains("no resource"));
}

#[test]
fn marks_cannot_make_marks_and_export_stays_small() {
    let mut s = Session::with_sequential_ids();
    let dot = ok(&mut s, "symbol.create", json!({"name": "Dot", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 50}}]}))["id"].as_uid().unwrap();
    let lt = ok(&mut s, "resource.create", json!({"type": "line_type", "name": "Fence", "units": "world", "elements": [{"kind": "dash", "length": 400}, {"kind": "mark", "symbol": dot}, {"kind": "gap", "length": 200}]}))["id"].as_uid().unwrap();
    // The class has the line type too, so a mark that inherits it would make marks on marks.
    exec(
        &mut s,
        json!({"op": "modify_class", "id": "00000000-0000-0000-0000-000000000000", "attrs": {"line_type": lt}}),
    );
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [6000, 0]}}),
    );
    let t = std::time::Instant::now();
    let svg = ok(&mut s, "render.svg", json!({}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        t.elapsed().as_secs() < 2 && svg.len() < 200_000,
        "{} bytes in {:?}",
        svg.len(),
        t.elapsed()
    );
    let out = tmp("mark.dxf");
    let r = ok(&mut s, "export.file", json!({"path": out, "format": "dxf"}));
    assert!(r["exported"].as_u64().unwrap() >= 1);
    assert!(std::fs::metadata(&out).unwrap().len() < 400_000);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn objects_drawn_while_editing_a_symbol_become_part_of_it() {
    let mut s = Session::with_sequential_ids();
    let sym = ok(&mut s, "symbol.create", json!({"name": "Bed", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 100}}]}))["id"].as_uid().unwrap();
    let inst = ok(
        &mut s,
        "symbol.place",
        json!({"symbol": sym, "at": [1000, 1000]}),
    )["id"]
        .as_uid()
        .unwrap();
    ok(&mut s, "symbol.edit", json!({"id": inst}));
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "line", "a": [900, 1000], "b": [1100, 1000]}}),
    );
    ok(&mut s, "symbol.edit_end", json!({}));
    let def = ok(&mut s, "symbol.get", json!({"id": sym}));
    assert_eq!(def["items"].as_array().unwrap().len(), 2);
    assert_eq!(def["items"][1]["shape"]["a"], json!([-100.0, 0.0]));
    // Move an object into a group and out again through the API.
    let (a, b) = (circle(&mut s, 0.0), circle(&mut s, 9.0));
    let g = ok(&mut s, "group.create", json!({"ids": [a]}))["id"]
        .as_uid()
        .unwrap();
    ok(&mut s, "group.move_into", json!({"ids": [b], "group": g}));
    assert_eq!(
        ok(&mut s, "group.members", json!({"id": g}))["members"],
        json!([a, b])
    );
    ok(&mut s, "group.move_out", json!({"ids": [b]}));
    assert_eq!(
        ok(&mut s, "group.members", json!({"id": g}))["members"],
        json!([a])
    );
    // A group you entered takes the new objects.
    ok(&mut s, "group.enter", json!({"id": g}));
    let n = circle(&mut s, 20.0);
    assert_eq!(ok(&mut s, "objects.get", json!({"id": n}))["parent"], g);
}

#[test]
fn paper_symbols_and_insert_options() {
    let mut s = Session::with_sequential_ids();
    let arrow = ok(&mut s, "symbol.create", json!({"name": "North", "units": "paper", "rotate_on_insert": true, "items": [{"shape": {"kind": "line", "a": [0, 0], "b": [0, 10]}}]}))["id"].as_uid().unwrap();
    let id = ok(
        &mut s,
        "symbol.place",
        json!({"symbol": arrow, "at": [0, 0], "rotate_to": [100, 0]}),
    )["id"]
        .as_uid()
        .unwrap();
    assert!(
        (ok(&mut s, "objects.get", json!({"id": id}))["shape"]["rotation"]
            .as_f64()
            .unwrap())
        .abs()
            < 1e-9
    );
    // On a 1:100 layer the 10 mm line is 1000 mm long.
    let summary = ok(&mut s, "document.summary", json!({}));
    let (lo, hi) = (&summary["bounds"]["min"], &summary["bounds"]["max"]);
    let len = ((hi[0].as_f64().unwrap() - lo[0].as_f64().unwrap()).powi(2)
        + (hi[1].as_f64().unwrap() - lo[1].as_f64().unwrap()).powi(2))
    .sqrt();
    assert!(len > 900.0, "{len}");
    // The class of the definition applies on insert.
    let class = exec(&mut s, json!({"op": "add_class", "name": "Notes"}))[0];
    ok(
        &mut s,
        "resource.update",
        json!({"id": arrow, "insert_class": class}),
    );
    let id2 = ok(
        &mut s,
        "symbol.place",
        json!({"symbol": arrow, "at": [0, 0]}),
    )["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "objects.get", json!({"id": id2}))["class"],
        class
    );
}
