use super::*;
use crate::{METHODS, Session};

fn doc_with_layers() -> Session {
    let mut s = Session::with_sequential_ids();
    for name in ["Site", "Planting", "Trees", "trees"] {
        s.handle(
            "execute",
            json!({"command": {"op": "add_layer", "name": name}}),
        )
        .unwrap();
    }
    for name in ["Shrubs", "Hedge"] {
        s.handle(
            "execute",
            json!({"command": {"op": "add_class", "name": name}}),
        )
        .unwrap();
    }
    s
}

fn layer_id(s: &Session, name: &str) -> Value {
    json!(
        s.editor
            .doc()
            .layers
            .iter()
            .find(|l| l.name == name)
            .unwrap()
            .id
    )
}

#[test]
fn every_method_has_a_struct_and_every_struct_is_a_method() {
    let listed: std::collections::BTreeSet<&str> = METHODS.iter().map(|(n, _)| *n).collect();
    let missing: Vec<&str> = listed.iter().copied().filter(|m| !has(m)).collect();
    assert!(
        missing.is_empty(),
        "no parameter struct (crates/kadakas-api/src/params) for {missing:?}"
    );
    let extra: Vec<&str> = methods().filter(|m| !listed.contains(m)).collect();
    assert!(extra.is_empty(), "structs for no such method: {extra:?}");
}

#[test]
fn an_unknown_parameter_is_refused_with_the_nearest_name() {
    let mut s = doc_with_layers();
    let e = s
        .handle("objects.list", json!({"lyaer": "Site"}))
        .unwrap_err();
    assert_eq!(e.code, -32602);
    assert!(
        e.message.contains("unknown parameter 'lyaer'"),
        "{}",
        e.message
    );
    assert!(e.message.contains("did you mean 'layer'?"), "{}", e.message);
    assert!(
        e.message.contains("it takes: layer, class, kind"),
        "{}",
        e.message
    );
    let d = e.details.unwrap();
    assert_eq!(d["unknown"], json!(["lyaer"]));
    assert_eq!(d["suggestions"]["lyaer"], "layer");
    let e = s
        .handle("objects.list", json!({"kinds": "rect", "zzzzzz": 1}))
        .unwrap_err();
    assert!(e.message.contains("unknown parameters"), "{}", e.message);
    assert!(
        e.message.contains("'kinds' (did you mean 'kind'?)"),
        "{}",
        e.message
    );
    let e = s.handle("selection.get", json!({"all": true})).unwrap_err();
    assert!(
        e.message.contains("it takes no parameters"),
        "{}",
        e.message
    );
    s.handle("objects.list", json!({"kind": "rect"})).unwrap();
    s.handle("objects.list", Value::Null).unwrap();
    assert_eq!(s.handle("objects.lst", json!({})).unwrap_err().code, -32601);
}

#[test]
fn a_wrong_type_or_a_missing_parameter_names_the_parameter() {
    let mut s = doc_with_layers();
    let e = s.handle("file.open", json!({"path": 5})).unwrap_err();
    assert_eq!(e.code, -32602);
    assert!(e.message.contains("parameter 'path'"), "{}", e.message);
    assert!(e.message.contains("expected a string"), "{}", e.message);
    assert!(e.message.contains("got an integer"), "{}", e.message);
    assert_eq!(e.details.unwrap()["parameter"], "path");
    let e = s
        .handle("file.save", json!({"path": "x", "format": "glowing"}))
        .unwrap_err();
    assert!(e.message.contains("parameter 'format'"), "{}", e.message);
    assert!(e.message.contains("container, json"), "{}", e.message);
    let e = s.handle("file.open", json!({})).unwrap_err();
    assert!(e.message.contains("'path'"), "{}", e.message);
    assert_eq!(e.details.unwrap()["problem"], "missing");
    let e = s
        .handle("document.load", json!({"document": [1]}))
        .unwrap_err();
    assert!(e.message.contains("'document'"), "{}", e.message);
    assert!(e.message.contains("an object"), "{}", e.message);
}

#[test]
fn schemas_come_from_the_structs() {
    let one = schema("file.save").unwrap();
    assert_eq!(one["type"], "object");
    assert_eq!(one["additionalProperties"], false);
    assert_eq!(one["required"], json!(["path"]));
    assert_eq!(one["properties"]["path"]["type"], "string");
    // Null is allowed for a parameter that is not given.
    assert_eq!(
        one["properties"]["format"]["enum"],
        json!(["container", "json", null])
    );
    assert!(one.get("$defs").is_none() && one.get("title").is_none());
    assert!(one["properties"]["path"]["description"].is_string());
}

#[test]
fn a_command_of_execute_can_name_a_layer_added_earlier_in_the_batch() {
    let mut s = Session::with_sequential_ids();
    let r = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_layer", "name": "Fresh"},
                {"op": "add_class", "name": "Newclass"},
                {"op": "add_object", "layer": "Fresh", "class": "newclass",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
            ]}}),
        )
        .unwrap();
    let id = r["created"].as_array().unwrap().last().unwrap().clone();
    let o = s.handle("objects.get", json!({"id": id})).unwrap();
    assert_eq!(o["layer"], layer_id(&s, "Fresh"));
    // One undo step.
    assert_eq!(s.editor.undo_depth(), 1);
    // A name that no command adds is still an error, and nothing of the batch stays.
    let e = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_layer", "name": "Other"},
                {"op": "add_object", "layer": "Nowhere",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
            ]}}),
        )
        .unwrap_err();
    assert!(
        e.message.contains("no layer is named \"Nowhere\""),
        "{}",
        e.message
    );
    assert!(s.editor.doc().layers.iter().all(|l| l.name != "Other"));
}

#[test]
fn nearest_finds_typing_mistakes_and_not_other_words() {
    let known = ["layer", "class", "kind", "spacing_x", "on_terrain"];
    assert_eq!(nearest("lyaer", known), Some("layer"));
    assert_eq!(nearest("layers", known), Some("layer"));
    assert_eq!(nearest("Layer", known), Some("layer"));
    assert_eq!(nearest("spacing", known), Some("spacing_x"));
    assert_eq!(nearest("onterrain", known), Some("on_terrain"));
    assert_eq!(nearest("elevation", known), None);
    assert_eq!(nearest("x", known), None);
}

/// Every `{method, params}` in `describe` is an example for an agent: its struct must accept it.
fn examples(v: &Value, out: &mut Vec<(String, Value)>) {
    match v {
        Value::Object(m) => {
            if let (Some(Value::String(method)), Some(params)) = (m.get("method"), m.get("params"))
                && has(method)
            {
                out.push((method.clone(), params.clone()));
            }
            m.values().for_each(|x| examples(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| examples(x, out)),
        _ => {}
    }
}

#[test]
fn every_example_of_describe_is_accepted_by_its_struct() {
    let mut s = Session::with_sequential_ids();
    let d = s.handle("describe", json!({})).unwrap();
    let mut found = Vec::new();
    examples(&d, &mut found);
    assert!(!found.is_empty(), "no examples");
    let mut bad = Vec::new();
    for (method, p) in &found {
        if let Err(e) = check_without_document(method, p.clone()) {
            bad.push(format!("{method} {p}: {}", e.message));
        }
    }
    assert!(
        bad.is_empty(),
        "examples that the struct refuses:\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_name_of_a_layer_or_a_class_becomes_its_id_in_every_parameter_of_that_type() {
    let mut s = doc_with_layers();
    let planting = layer_id(&s, "Planting");
    // A name, with the case and the spaces of a person.
    let r = s
        .handle("objects.list", json!({"layer": " planting "}))
        .unwrap();
    assert!(r.is_array() || r.is_object());
    let p = prepare(
        s.editor.doc(),
        "objects.list",
        json!({"layer": "Planting", "class": "shrubs"}),
    )
    .unwrap();
    assert_eq!(p["layer"], planting);
    let shrubs = s
        .editor
        .doc()
        .classes
        .iter()
        .find(|c| c.name == "Shrubs")
        .unwrap()
        .id;
    assert_eq!(p["class"], json!(shrubs));
    // An ID stays as it is, also one that names nothing: the method decides.
    let same = "01a143c3-1f03-7000-8000-0034f0676be8";
    let p = prepare(s.editor.doc(), "objects.list", json!({"layer": same})).unwrap();
    assert_eq!(p["layer"], same);
    // The exact name wins over a name that differs by the case ("Trees" and "trees").
    let p = prepare(s.editor.doc(), "objects.list", json!({"layer": "trees"})).unwrap();
    assert_eq!(p["layer"], layer_id(&s, "trees"));
    let p = prepare(s.editor.doc(), "objects.list", json!({"layer": "Trees"})).unwrap();
    assert_eq!(p["layer"], layer_id(&s, "Trees"));
    // A name that differs by the case from two layers is ambiguous, and the error lists them.
    let e = prepare(s.editor.doc(), "objects.list", json!({"layer": "TREES"})).unwrap_err();
    assert_eq!(e.code, -32602);
    assert!(e.message.contains("matches 2 layers"), "{}", e.message);
    assert!(e.message.contains("\"Trees\"") && e.message.contains("\"trees\""));
    assert_eq!(
        e.details.unwrap()["candidates"].as_array().unwrap().len(),
        2
    );
    // An unknown name is an error that lists the layers, with the nearest one.
    let e = prepare(s.editor.doc(), "objects.list", json!({"layer": "Plantin"})).unwrap_err();
    assert_eq!(e.code, -32602);
    assert!(
        e.message.contains("no layer is named \"Plantin\""),
        "{}",
        e.message
    );
    assert!(
        e.message.contains("Did you mean \"Planting\"?"),
        "{}",
        e.message
    );
    assert!(e.message.contains("\"Site\""), "{}", e.message);
    let e = prepare(s.editor.doc(), "objects.list", json!({"class": "Shrub"})).unwrap_err();
    assert!(
        e.message.contains("no class is named \"Shrub\""),
        "{}",
        e.message
    );
    assert!(e.message.contains("\"Hedge\""), "{}", e.message);
    // A list of layers.
    let p = prepare(
        s.editor.doc(),
        "visibility.set",
        json!({"mode": "grey", "layers": ["Site", "Planting"]}),
    )
    .unwrap();
    assert_eq!(p["layers"], json!([layer_id(&s, "Site"), planting]));
}

#[test]
fn a_command_of_execute_takes_names_too() {
    let mut s = doc_with_layers();
    let planting = layer_id(&s, "Planting");
    let r = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_object", "layer": "Planting", "class": "Hedge",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
                {"op": "add_object", "layer": "Site",
                 "shape": {"kind": "circle", "center": [5, 0], "radius": 10}},
            ]}}),
        )
        .unwrap();
    let id = r["created"][0].clone();
    let o = s.handle("objects.get", json!({"id": id})).unwrap();
    assert_eq!(o["layer"], planting);
    let e = s
        .handle(
            "execute",
            json!({"command": {"op": "add_object", "layer": "Nowhere",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}}}),
        )
        .unwrap_err();
    assert!(
        e.message.contains("no layer is named \"Nowhere\""),
        "{}",
        e.message
    );
}

#[test]
fn a_sheet_parameter_takes_only_sheet_layers() {
    let mut s = doc_with_layers();
    let sheet = s.handle("sheet.create", json!({"name": "A-101"})).unwrap()["layer"].clone();
    let p = prepare(s.editor.doc(), "viewport.list", json!({"sheet": "a-101"})).unwrap();
    assert_eq!(p["sheet"], sheet);
    let e = prepare(
        s.editor.doc(),
        "viewport.list",
        json!({"sheet": "Planting"}),
    )
    .unwrap_err();
    assert!(
        e.message.contains("no sheet layer is named \"Planting\""),
        "{}",
        e.message
    );
    assert!(e.message.contains("A-101"), "{}", e.message);
}

#[test]
fn describe_gives_the_schemas_on_request() {
    let mut s = Session::with_sequential_ids();
    let d = s.handle("describe", json!({})).unwrap();
    assert!(
        d.get("method_schemas").is_none(),
        "the default answer stays small"
    );
    let one = s
        .handle("describe", json!({"method": "plants.place"}))
        .unwrap();
    assert_eq!(one["method"], "plants.place");
    assert!(one["doc"].as_str().unwrap().contains("Place plants"));
    assert_eq!(one["params"]["properties"]["mode"]["enum"][0], "single");
    assert!(
        one["params"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("mode"))
    );
    let e = s
        .handle("describe", json!({"method": "plants.plce"}))
        .unwrap_err();
    assert!(
        e.message.contains("did you mean 'plants.place'?"),
        "{}",
        e.message
    );
    let all = s.handle("describe", json!({"schemas": true})).unwrap();
    let m = all["method_schemas"].as_object().unwrap();
    assert_eq!(m.len(), METHODS.len());
    assert_eq!(m["plants.place"], one["params"]);
}

#[test]
fn one_id_is_a_list_of_one_and_feedback_attach_takes_false() {
    let s = doc_with_layers();
    let id = "00000000-0000-0000-0000-000000000001";
    let p = prepare(s.editor.doc(), "wall.report", json!({"id": id})).unwrap();
    assert_eq!(p, json!({"ids": [id]}));
    // A method that has an `id` of its own keeps it.
    let p = prepare(s.editor.doc(), "objects.get", json!({"id": id})).unwrap();
    assert_eq!(p, json!({"id": id}));
    // Both given: `ids` wins and `id` is an unknown parameter.
    assert!(
        prepare(
            s.editor.doc(),
            "wall.report",
            json!({"id": id, "ids": [id]})
        )
        .is_err()
    );
    for attach in [json!(false), json!({"log": false})] {
        prepare(
            s.editor.doc(),
            "feedback.submit",
            json!({"kind": "bug", "title": "t", "attach": attach}),
        )
        .unwrap();
    }
}

#[test]
fn a_name_added_earlier_in_the_batch_works_and_an_exact_name_wins() {
    let mut s = doc_with_layers();
    // A layer that an earlier command of the batch adds can be named by a later one.
    let r = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_layer", "name": "Paving"},
                {"op": "add_object", "layer": "paving",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
            ]}}),
        )
        .unwrap();
    let paving = layer_id(&s, "Paving");
    let created = r["created"].as_array().unwrap().last().unwrap().clone();
    let o = s.handle("objects.get", json!({"id": created})).unwrap();
    assert_eq!(o["layer"], paving);
    // Two layers that differ in case: the exact name picks one, another case is ambiguous.
    let lower = layer_id(&s, "trees");
    let r = s
        .handle(
            "execute",
            json!({"command": {"op": "add_object", "layer": "trees",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}}}),
        )
        .unwrap();
    let o = s
        .handle("objects.get", json!({"id": r["created"][0]}))
        .unwrap();
    assert_eq!(o["layer"], lower);
    let e = s
        .handle(
            "execute",
            json!({"command": {"op": "add_object", "layer": "TREES",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}}}),
        )
        .unwrap_err();
    assert!(e.message.contains("matches 2 layers"), "{}", e.message);
}

#[test]
fn enums_ignore_the_case_and_take_a_hyphen_for_an_underscore() {
    let doc = Session::with_sequential_ids();
    let p = prepare(
        doc.editor.doc(),
        "render_style.create",
        json!({"name": "x", "mode": "Hidden-Line"}),
    )
    .unwrap();
    assert_eq!(p["mode"], "hidden_line");
    let e = prepare(
        doc.editor.doc(),
        "render_style.create",
        json!({"name": "x", "mode": "sketch"}),
    )
    .unwrap_err();
    assert!(e.message.contains("parameter 'mode'"), "{}", e.message);
    assert!(e.message.contains("shaded"), "{}", e.message);
    assert_eq!(e.details.unwrap()["problem"], "wrong_value");
}

#[test]
fn an_optional_parameter_sent_as_null_stays_null_and_a_required_one_may_not_be() {
    let doc = Session::with_sequential_ids();
    // null clears: the method gets the null.
    let p = prepare(
        doc.editor.doc(),
        "landscape_area.set",
        json!({"id": "00000000-0000-0000-0000-000000000001", "style": null}),
    )
    .unwrap();
    assert!(p["style"].is_null() && p.as_object().unwrap().contains_key("style"));
    // An optional parameter with null is as if it were not there, and stays null for the method.
    let p = prepare(doc.editor.doc(), "objects.list", json!({"kind": null})).unwrap();
    assert_eq!(p, json!({"kind": null}));
    // A required parameter may not be null.
    let e = prepare(doc.editor.doc(), "file.open", json!({"path": null})).unwrap_err();
    assert!(e.message.contains("'path'"), "{}", e.message);
    // A required parameter that may be null must be given.
    prepare(doc.editor.doc(), "grid.set", json!({"spacing": null})).unwrap();
    let e = prepare(doc.editor.doc(), "grid.set", json!({})).unwrap_err();
    assert!(e.message.contains("missing 'spacing'"), "{}", e.message);
}

#[test]
fn numbers_keep_the_form_the_caller_wrote() {
    let doc = Session::with_sequential_ids();
    let p = prepare(
        doc.editor.doc(),
        "group.rotate",
        json!({"center": [0, 0], "angle": 2}),
    )
    .unwrap();
    assert_eq!(p["angle"].to_string(), "2");
    assert_eq!(p["center"].to_string(), "[0,0]");
    let p = prepare(
        doc.editor.doc(),
        "group.rotate",
        json!({"center": [0.5, 1], "angle": 2.5}),
    )
    .unwrap();
    assert_eq!(p["angle"].to_string(), "2.5");
}

#[test]
fn a_union_of_parameter_sets_checks_the_set_that_type_names() {
    let doc = Session::with_sequential_ids();
    let hatch = json!({"type": "hatch", "name": "H", "lines": [{"angle": 45, "spacing": 2}]});
    let p = prepare(doc.editor.doc(), "resource.create", hatch).unwrap();
    assert_eq!(p["lines"][0]["spacing"], 2);
    // A field of another type is refused, with the nearest name of this type.
    let e = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "tile", "name": "T", "symbol": "S", "width": 1, "height": 1, "widht": 2}),
    )
    .unwrap_err();
    assert_eq!(e.code, -32602);
    assert!(e.message.contains("'widht'"), "{}", e.message);
    assert!(e.message.contains("did you mean 'width'?"), "{}", e.message);
    let e = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "hatch", "name": "H", "lines": [{"angle": 45}]}),
    )
    .unwrap_err();
    assert!(
        e.message.contains("missing 'lines[0].spacing'"),
        "{}",
        e.message
    );
    let e = prepare(doc.editor.doc(), "resource.create", json!({"name": "H"})).unwrap_err();
    assert!(e.message.contains("missing 'type'"), "{}", e.message);
    let e = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "wave", "name": "H"}),
    )
    .unwrap_err();
    assert!(e.message.contains("parameter 'type'"), "{}", e.message);
    assert!(e.message.contains("one of symbol"), "{}", e.message);
    // The schema is an object at the root, with the five sets.
    let schema = schema("resource.create").unwrap();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["oneOf"].as_array().unwrap().len(), 5);
}

#[test]
fn json_schemas_have_names_types_required_enums_defaults_and_references() {
    let s = schema("plants.place").unwrap();
    assert_eq!(s["type"], "object");
    assert_eq!(s["additionalProperties"], false);
    assert!(s["required"].as_array().unwrap().contains(&json!("mode")));
    assert_eq!(s["properties"]["mode"]["enum"][0], "single");
    assert_eq!(s["properties"]["layer"]["x-kadakas-ref"], "layer");
    assert_eq!(s["properties"]["class"]["x-kadakas-ref"], "class");
    assert_eq!(s["properties"]["at"]["maxItems"], 2);
    assert!(s["properties"]["layer"]["description"].is_string());
    assert!(s.get("$defs").is_none() && s.get("title").is_none());
    // A parameter that may be null says so; an optional one says it too (null is "not given").
    let g = schema("grid.set").unwrap();
    assert_eq!(g["required"], json!(["spacing"]));
    let any = g["properties"]["spacing"]["anyOf"].as_array().unwrap();
    assert!(any.contains(&json!({"type": "null"})));
    assert_eq!(
        g["properties"]["labels"]["type"],
        json!(["boolean", "null"])
    );
    assert_eq!(g["properties"]["labels"]["default"], true);
    // At least one of id and ids.
    let s = schema("irrigation.set").unwrap();
    assert_eq!(s["allOf"][0]["anyOf"][1]["required"], json!(["ids"]));
}

#[test]
fn at_least_one_of_a_group_is_needed() {
    let doc = Session::with_sequential_ids();
    let e = prepare(doc.editor.doc(), "irrigation.set", json!({"arc": 5})).unwrap_err();
    assert_eq!(e.code, -32602);
    assert!(e.message.contains("id, ids"), "{}", e.message);
    prepare(
        doc.editor.doc(),
        "irrigation.set",
        json!({"ids": ["00000000-0000-0000-0000-000000000001"], "arc": 5}),
    )
    .unwrap();
}

#[test]
fn a_nested_list_names_the_item_that_is_wrong() {
    let doc = Session::with_sequential_ids();
    let e = prepare(
        doc.editor.doc(),
        "survey.import_points",
        json!({"text": "1,2,3", "columns": 5}),
    )
    .unwrap_err();
    assert!(e.message.contains("'columns'"), "{}", e.message);
    let e = prepare(
        doc.editor.doc(),
        "plants.place",
        json!({"mode": "path", "points": [[0, 0], [1]]}),
    )
    .unwrap_err();
    assert!(e.message.contains("'points[1]'"), "{}", e.message);
    assert!(e.message.contains("[x, y]"), "{}", e.message);
    // A method without a struct passes through (the dispatcher says it does not exist).
    assert!(prepare(doc.editor.doc(), "color.nothing", json!({})).is_ok());
}

#[test]
fn a_nested_unknown_name_and_a_wrong_tag_are_named() {
    let doc = Session::with_sequential_ids();
    let e = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "hatch", "name": "H", "lines": [{"spacing": 2, "bogus": 1}]}),
    )
    .unwrap_err();
    assert!(
        e.message.contains("unknown parameter 'lines[0].bogus'"),
        "{}",
        e.message
    );
    assert!(e.message.contains("it takes:"), "{}", e.message);
    let e = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": 5, "name": "H"}),
    )
    .unwrap_err();
    assert_eq!(e.code, -32602);
    let e = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "texture", "name": "T", "color": [300, 0, 0]}),
    )
    .unwrap_err();
    assert!(e.message.contains("'color'"), "{}", e.message);
    assert!(e.message.contains("300"), "{}", e.message);
    let e = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "texture", "name": "T", "color": [1, 2]}),
    )
    .unwrap_err();
    assert!(e.message.contains("'color'"), "{}", e.message);
    // A point with three numbers where a 3D point is allowed, and with four where it is not.
    prepare(
        doc.editor.doc(),
        "polygon3d.add",
        json!({"points": [[0, 0, 1], [1, 0], [1, 1, 2]]}),
    )
    .unwrap();
    let e = prepare(
        doc.editor.doc(),
        "polygon3d.add",
        json!({"points": [[0, 0, 1, 2], [1, 0], [1, 1, 2]]}),
    )
    .unwrap_err();
    assert!(e.message.contains("'points[0]'"), "{}", e.message);
}

#[test]
fn a_batch_with_names_refuses_what_is_no_command_and_takes_nothing_of_it() {
    let mut s = Session::with_sequential_ids();
    let before = s.editor.doc().layers.len();
    let e = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_layer", "name": "A"},
                {"op": "add_object", "layer": "A",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
                5,
            ]}}),
        )
        .unwrap_err();
    assert_eq!(e.code, -32602);
    assert_eq!(
        s.editor.doc().layers.len(),
        before,
        "nothing of the batch stays"
    );
    // A nested batch is read command by command too.
    s.handle(
        "execute",
        json!({"command": {"op": "batch", "commands": [
            {"op": "add_layer", "name": "B"},
            {"op": "batch", "commands": [
                {"op": "add_object", "layer": "b",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
            ]},
        ]}}),
    )
    .unwrap();
    assert_eq!(s.editor.undo_depth(), 1);
}

#[test]
fn whole_numbers_may_be_written_with_a_fraction_of_zero() {
    let doc = Session::with_sequential_ids();
    let p = prepare(
        doc.editor.doc(),
        "render.png",
        json!({"width": 800.0, "height": 600}),
    )
    .unwrap();
    // Written back as integers.
    assert_eq!(p["width"].to_string(), "800");
    assert_eq!(p["height"].to_string(), "600");
    let e = prepare(doc.editor.doc(), "render.png", json!({"width": 2.5})).unwrap_err();
    assert!(e.message.contains("'width'"), "{}", e.message);
    assert!(e.message.contains("2.5"), "{}", e.message);
    let e = prepare(doc.editor.doc(), "render.png", json!({"width": 1e30})).unwrap_err();
    assert!(e.message.contains("'width'"), "{}", e.message);
    let e = prepare(doc.editor.doc(), "render.png", json!({"width": u64::MAX})).unwrap_err();
    assert!(e.message.contains("'width'"), "{}", e.message);
    // The components of a color too.
    let p = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "texture", "name": "T", "color": [255.0, 0, 10.0]}),
    )
    .unwrap();
    assert_eq!(p["color"].to_string(), "[255,0,10]");
    let e = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "texture", "name": "T", "color": [256.0, 0, 0]}),
    )
    .unwrap_err();
    assert!(e.message.contains("'color'"), "{}", e.message);
    // A number that the handlers truncate stays a number.
    prepare(
        doc.editor.doc(),
        "site.check",
        json!({"id": "a", "max_issues": 2.5}),
    )
    .unwrap();
}

#[test]
fn the_type_of_a_resource_ignores_the_case() {
    let doc = Session::with_sequential_ids();
    let p = prepare(
        doc.editor.doc(),
        "resource.create",
        json!({"type": "Line-Type", "name": "L", "elements": [{"kind": "dash", "length": 1}]}),
    )
    .unwrap();
    assert_eq!(p["type"], "line_type");
}

/// The names that the `from_name` functions of the program accept, for the parameters that the
/// structs read as enums: nothing that worked may be refused.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_other_names_of_the_program_are_still_accepted() {
    let doc = Session::with_sequential_ids();
    let ok = |method: &str, p: Value| {
        prepare(doc.editor.doc(), method, p.clone())
            .unwrap_or_else(|e| panic!("{method} {p}: {}", e.message));
    };
    for name in [
        "dxf",
        "shp",
        "shapefile",
        "geojson",
        "json",
        ".DXF",
        " Shp ",
    ] {
        if kadakas_io::Format::from_name(name).is_some() {
            ok("import.file", json!({"path": "a", "format": name}));
            if !name.to_ascii_lowercase().contains("sh") {
                ok("export.file", json!({"path": "a", "format": name}));
            }
        }
    }
    for name in ["container", "zip", "kad", "json", "KAD"] {
        ok("file.save", json!({"path": "a", "format": name}));
    }
    for name in [
        "shaded",
        "opengl",
        "textured",
        "wireframe",
        "hidden_line",
        "hiddenline",
        "dashed_hidden",
    ] {
        assert!(
            kadakas_core::present::RenderMode::from_name(name).is_some(),
            "{name}"
        );
        ok("view.set", json!({"mode": name}));
    }
    for name in ["ortho", "orthographic", "parallel", "perspective", "persp"] {
        assert!(
            kadakas_render::Projection::from_name(name).is_some(),
            "{name}"
        );
        ok("view.set", json!({"projection": name}));
    }
    for name in [
        "existing",
        "exist",
        "retained",
        "new",
        "proposed",
        "demolished",
        "demolish",
        "demo",
        "removed",
    ] {
        assert!(
            kadakas_core::viewrules::Phase::from_name(name).is_some(),
            "{name}"
        );
        ok("phase.set", json!({"ids": ["a"], "phase": name}));
    }
    for name in ["flat", "gable", "gabled", "hip", "hipped"] {
        assert!(
            kadakas_core::massing::RoofStyle::from_name(name).is_some(),
            "{name}"
        );
        ok(
            "massing.add",
            json!({"footprint": [[0, 0], [1, 0], [0, 1]], "roof": name}),
        );
    }
    for name in ["visible", "grey", "gray", "invisible", "hidden"] {
        assert!(
            kadakas_core::visibility::VisMode::from_name(name).is_some(),
            "{name}"
        );
        ok("visibility.set", json!({"mode": name}));
    }
    for name in ["rename", "reuse", "skip", "replace", "overwrite"] {
        assert!(
            kadakas_core::library::Conflict::from_name(name).is_some(),
            "{name}"
        );
        ok(
            "resource.import_from_file",
            json!({"path": "a", "on_conflict": name}),
        );
    }
    for name in [
        "auto",
        ",",
        "comma",
        ";",
        "semicolon",
        "tab",
        "|",
        "pipe",
        " ",
        "space",
        "whitespace",
    ] {
        assert!(
            kadakas_core::survey::Delimiter::from_name(name).is_some(),
            "{name}"
        );
        ok(
            "survey.import_points",
            json!({"text": "a", "delimiter": name}),
        );
    }
    // A unit is a name or a factor.
    for unit in [
        json!("feet"),
        json!("Inches"),
        json!("yd"),
        json!("usft"),
        json!(0.3048),
    ] {
        ok("survey.import_points", json!({"text": "a", "unit": unit}));
    }
    // A name that nothing knows is still refused.
    let e = prepare(doc.editor.doc(), "view.set", json!({"mode": "sketch"})).unwrap_err();
    assert!(e.message.contains("'mode'"), "{}", e.message);
}

#[test]
fn a_method_of_the_list_without_a_struct_is_an_error_of_the_program() {
    // The coverage test makes sure that none is missing; the check itself must not let one pass.
    let doc = Session::with_sequential_ids();
    assert!(!has("no.such.method"));
    assert!(prepare(doc.editor.doc(), "no.such.method", json!({"x": 1})).is_ok());
}

#[test]
fn a_nested_batch_whose_commands_is_not_a_list_says_wrong_type() {
    let mut s = Session::with_sequential_ids();
    let e = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_layer", "name": "A"},
                {"op": "add_object", "layer": "A",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
                {"op": "batch", "commands": "x"},
            ]}}),
        )
        .unwrap_err();
    assert_eq!(e.code, -32602);
    assert!(!e.message.contains("missing"), "{}", e.message);
    assert!(e.message.contains("sequence"), "{}", e.message);
}

#[test]
fn the_names_of_a_batch_follow_the_layers_that_the_batch_makes_and_renames() {
    let mut s = Session::with_sequential_ids();
    let r = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_layer", "name": "One"},
                {"op": "add_object", "layer": "One",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
                {"op": "add_layer", "name": "Two"},
                {"op": "add_object", "layer": "two",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
                {"op": "modify_layer", "id": "One", "name": "Uno"},
                {"op": "add_object", "layer": "Uno",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
            ]}}),
        )
        .unwrap_err();
    // `id` of modify_layer is an ID, not a name: the batch is refused whole.
    assert_eq!(r.code, -32602);
    assert!(s.editor.doc().layers.iter().all(|l| l.name != "One"));
    let ok = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_layer", "name": "One"},
                {"op": "add_object", "layer": "One",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
                {"op": "add_layer", "name": "Two"},
                {"op": "add_object", "layer": "two",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
            ]}}),
        )
        .unwrap();
    assert_eq!(ok["created"].as_array().unwrap().len(), 4);
}
