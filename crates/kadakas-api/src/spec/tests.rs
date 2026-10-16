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
    s.handle(
        "execute",
        json!({"command": {"op": "add_class", "name": "Shrubs"}}),
    )
    .unwrap();
    s.handle(
        "execute",
        json!({"command": {"op": "add_class", "name": "Hedge"}}),
    )
    .unwrap();
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
fn the_text_of_the_table_is_read_with_types_defaults_and_includes() {
    let t = parse(
        "# a comment\n\
         a.b open\n  \
           x! num|str = 5 # a number or text\n  \
           kind enum(one,two) = one\n  \
           where layer\n  \
           where_to class[]\n  \
           pts point[]\n  \
           fill map(color)\n  \
           @one_of x,kind\n\
         a.c\n  \
           @include a.b\n  \
           y bool\n",
    )
    .unwrap();
    let b = t.get("a.b").unwrap();
    assert!(b.open);
    assert_eq!(b.params.len(), 6);
    let x = b.param("x").unwrap();
    assert!(x.required && x.default == Some(json!(5)) && x.doc == "a number or text");
    assert_eq!(x.ty, Ty::Union(vec![Ty::Num, Ty::Str]));
    assert_eq!(b.param("kind").unwrap().default, Some(json!("one")));
    assert_eq!(
        b.param("where_to").unwrap().ty,
        Ty::List(Box::new(Ty::Class))
    );
    assert_eq!(b.one_of, vec![vec!["x".to_string(), "kind".to_string()]]);
    // An include copies the parameters as optional ones, and the openness.
    let c = t.get("a.c").unwrap();
    assert!(c.open);
    assert_eq!(c.params.len(), 7);
    assert!(!c.param("x").unwrap().required);
    assert_eq!(t.methods().collect::<Vec<_>>(), vec!["a.b", "a.c"]);
}

#[test]
fn a_mistake_in_the_table_is_an_error_with_a_line_number() {
    for (text, part) in [
        ("m\n  x\n", "needs a type"),
        ("m\n  x wat\n", "unknown type"),
        ("m\n  x num\n  x num\n", "twice"),
        ("  x num\n", "before any method"),
        ("m\nm\n", "two entries"),
        ("m shut\n", "unknown flag"),
        ("m\n  @include nothing\n", "no entry"),
        ("m\n  x num\n  @one_of y\n", "no parameter"),
        ("m\n  x enum(a,,b)\n", "empty value"),
    ] {
        let e = parse(text)
            .err()
            .unwrap_or_else(|| panic!("{text:?} is fine"));
        assert!(e.contains(part), "{text:?}: {e}");
    }
    assert!(parse("m\n  x wat\n").unwrap_err().starts_with("line 2"));
}

#[test]
fn every_method_has_an_entry_and_every_entry_is_a_method() {
    let t = table();
    let listed: std::collections::BTreeSet<&str> = METHODS.iter().map(|(n, _)| *n).collect();
    let missing: Vec<&str> = listed
        .iter()
        .copied()
        .filter(|m| t.get(m).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "no entry in params.spec for {missing:?}"
    );
    let extra: Vec<&str> = t.methods().filter(|m| !listed.contains(m)).collect();
    assert!(
        extra.is_empty(),
        "params.spec names no such method: {extra:?}"
    );
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
    // Several at once, one without a near name.
    let e = s
        .handle("objects.list", json!({"kinds": "rect", "zzzzzz": 1}))
        .unwrap_err();
    assert!(e.message.contains("unknown parameters"), "{}", e.message);
    assert!(
        e.message.contains("'kinds' (did you mean 'kind'?)"),
        "{}",
        e.message
    );
    assert!(e.message.contains("'zzzzzz'"), "{}", e.message);
    // A method without parameters.
    let e = s.handle("selection.get", json!({"all": true})).unwrap_err();
    assert!(
        e.message.contains("it takes no parameters"),
        "{}",
        e.message
    );
    // The known names still work, and null is no parameters.
    s.handle("objects.list", json!({"kind": "rect"})).unwrap();
    s.handle("objects.list", Value::Null).unwrap();
    // A method that does not exist is still a method that does not exist.
    assert_eq!(s.handle("objects.lst", json!({})).unwrap_err().code, -32601);
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
fn json_schemas_have_names_types_required_enums_and_defaults() {
    let t = parse(
        "m\n  a! str # the a\n  b enum(x,y) = x\n  c layer\n  d point[]\n  e id|num\n  f obj\n  @one_of a,b\n\
         n open\n  q int\n",
    )
    .unwrap();
    let s = json_schema(t.get("m").unwrap());
    assert_eq!(s["type"], "object");
    assert_eq!(s["additionalProperties"], false);
    assert_eq!(s["required"], json!(["a"]));
    assert_eq!(s["properties"]["a"]["type"], "string");
    assert_eq!(s["properties"]["a"]["description"], "the a");
    assert_eq!(s["properties"]["b"]["enum"], json!(["x", "y"]));
    assert_eq!(s["properties"]["b"]["default"], "x");
    assert_eq!(s["properties"]["c"]["x-kadakas-ref"], "layer");
    assert!(
        s["properties"]["c"]["description"]
            .as_str()
            .unwrap()
            .contains("unique name")
    );
    assert_eq!(s["properties"]["d"]["type"], "array");
    assert_eq!(s["properties"]["d"]["items"]["maxItems"], 2);
    assert_eq!(s["properties"]["e"]["anyOf"].as_array().unwrap().len(), 2);
    assert_eq!(s["properties"]["f"]["type"], "object");
    assert_eq!(s["allOf"][0]["anyOf"][1]["required"], json!(["b"]));
    let n = json_schema(t.get("n").unwrap());
    assert_eq!(n["additionalProperties"], true);
    assert_eq!(n["properties"]["q"]["type"], "integer");
}

/// Every `{method, params}` in `describe` is an example for an agent: its names must be known.
fn examples(v: &Value, out: &mut Vec<(String, Value)>) {
    match v {
        Value::Object(m) => {
            if let (Some(Value::String(method)), Some(params)) = (m.get("method"), m.get("params"))
                && table().get(method).is_some()
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
fn every_example_of_describe_uses_known_parameters() {
    let mut s = Session::with_sequential_ids();
    let d = s.handle("describe", json!({})).unwrap();
    let mut found = Vec::new();
    examples(&d, &mut found);
    assert!(found.len() > 80, "{} examples", found.len());
    let mut bad = Vec::new();
    for (method, params) in &found {
        let unknown = unknown_names(method, params);
        if !unknown.is_empty() {
            bad.push(format!("{method}: {unknown:?}"));
        }
    }
    assert!(
        bad.is_empty(),
        "examples with unknown parameters:\n{}",
        bad.join("\n")
    );
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
fn a_name_added_earlier_in_the_batch_gets_a_clear_error_and_an_exact_name_wins() {
    let mut s = doc_with_layers();
    let e = s
        .handle(
            "execute",
            json!({"command": {"op": "batch", "commands": [
                {"op": "add_layer", "name": "Paving"},
                {"op": "add_object", "layer": "paving",
                 "shape": {"kind": "circle", "center": [0, 0], "radius": 10}},
            ]}}),
        )
        .unwrap_err();
    assert_eq!(e.code, -32602);
    assert!(
        e.message.contains("added earlier in this batch"),
        "{}",
        e.message
    );
    // The document is as it was.
    assert!(s.editor.doc().layers.iter().all(|l| l.name != "Paving"));
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
