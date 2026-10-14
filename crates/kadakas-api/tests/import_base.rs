//! `import.file` with the option `base` (IOP-22): the reply names what was kept, a drawing over the
//! cap is refused with the choices, and a link survives a save and a load.

use kadakas_api::Session;
use kadakas_core::provenance::{self, LayerSource};
use serde_json::{Value, json};

fn drawing(n: usize) -> String {
    let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
    for k in 0..n {
        s.push_str(&format!(
            "0\nPOINT\n5\n{:X}\n8\nP\n10\n{k}\n20\n0\n",
            k + 16
        ));
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

fn temp(name: &str) -> kadakas_sys::tmp::PrivateTempDir {
    kadakas_sys::tmp::PrivateTempDir::new(name).unwrap()
}

fn layer_source(s: &Session) -> Option<LayerSource> {
    s.editor
        .doc()
        .layers
        .iter()
        .find_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
}

fn import(
    s: &mut Session,
    path: &std::path::Path,
    options: Value,
) -> Result<Value, kadakas_api::RpcError> {
    s.handle("import.file", json!({"path": path, "options": options}))
}

#[test]
fn the_reply_says_which_base_was_used() {
    let dir = temp("kadakas-api-base");
    let path = dir.path().join("site.dxf");
    std::fs::write(&path, drawing(5)).unwrap();
    for (options, want) in [
        (json!({}), "auto"),
        (json!({"base": "auto"}), "auto"),
        (json!({"base": "none"}), "none"),
        (json!({"base": "link"}), "link"),
    ] {
        let mut s = Session::with_sequential_ids();
        let reply = import(&mut s, &path, options.clone()).unwrap();
        assert_eq!(reply["base"], want, "{options}");
        assert_eq!(reply["report"]["base"], want);
        let link = layer_source(&s).and_then(|l| l.link);
        assert_eq!(link.is_some(), want == "link", "{options}");
    }
    // Without source tracking there is no update data to name.
    let mut s = Session::with_sequential_ids();
    let reply = import(&mut s, &path, json!({"track_source": false})).unwrap();
    assert_eq!(reply["base"], Value::Null);
}

#[test]
fn a_drawing_over_the_cap_is_refused_with_the_choices_and_the_import_is_not_done() {
    let dir = temp("kadakas-api-base-cap");
    let path = dir.path().join("big.dxf");
    let n = provenance::MAX_BASE_RECS + 1;
    std::fs::write(&path, drawing(n)).unwrap();
    let mut s = Session::with_sequential_ids();
    let before = s.editor.doc().clone();
    let e = import(&mut s, &path, json!({})).unwrap_err();
    assert_eq!(e.kind, "validation");
    for want in [n.to_string(), "\"link\"".into(), "\"none\"".into()] {
        assert!(e.message.contains(&want), "{}", e.message);
    }
    assert_eq!(e.reason, Some(kadakas_api::REASON_BASE_TOO_MANY));
    assert_eq!(
        e.details,
        Some(json!({"entities": n, "cap": provenance::MAX_BASE_RECS}))
    );
    assert_eq!(s.editor.doc(), &before, "nothing was imported");
    // The same for an explicit auto, and a bad value is a bad parameter.
    assert!(import(&mut s, &path, json!({"base": "auto"})).is_err());
    let e = import(&mut s, &path, json!({"base": "sometimes"})).unwrap_err();
    assert_eq!(e.kind, "invalid_params");
}

#[test]
fn a_link_survives_save_and_load_and_the_describe_text_names_the_option() {
    let dir = temp("kadakas-api-base-save");
    let path = dir.path().join("site.dxf");
    let text = drawing(8);
    std::fs::write(&path, &text).unwrap();
    let mut s = Session::with_sequential_ids();
    import(&mut s, &path, json!({"base": "link"})).unwrap();
    let link = layer_source(&s).unwrap().link.unwrap();
    assert_eq!(link.bytes, text.len() as u64);
    // Save with the streaming writer, load again.
    let file = dir.path().join("doc.kad");
    kadakas_api::files::write_document_file(&file, s.editor.doc()).unwrap();
    let loaded = kadakas_api::files::read_document(&file).unwrap();
    assert!(loaded.load_notes().is_empty(), "{:?}", loaded.load_notes());
    let again = loaded
        .layers
        .iter()
        .find_map(|l| LayerSource::of(l.extra.get(provenance::LAYER_KEY)?))
        .unwrap();
    assert_eq!(again.link, Some(link));
    // The method list tells agents about the option.
    let described = s.handle("describe", json!({})).unwrap().to_string();
    assert!(
        described.contains("\"base\""),
        "describe has the option base"
    );
}

#[test]
fn a_lowered_session_limit_gives_the_same_choice_and_the_numbers_travel_in_data() {
    let dir = temp("kadakas-api-base-low");
    let path = dir.path().join("site.dxf");
    std::fs::write(&path, drawing(11)).unwrap();
    let mut s = Session::with_sequential_ids();
    s.set_limits(kadakas_api::SessionLimits {
        base_records: 10,
        ..Default::default()
    });
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "import.file",
        "params": {"path": path}});
    let line = s.handle_line(&req.to_string()).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    let data = &reply["error"]["data"];
    assert_eq!(data["kind"], "validation");
    assert_eq!(
        (data["entities"].as_u64(), data["cap"].as_u64()),
        (Some(11), Some(10))
    );
    // The choices work.
    let ok = import(&mut s, &path, json!({"base": "none"})).unwrap();
    assert_eq!(ok["base"], "none");
    // The limit can be lowered but never raised above the cap of the document.
    let mut high = Session::with_sequential_ids();
    high.set_limits(kadakas_api::SessionLimits {
        base_records: usize::MAX,
        ..Default::default()
    });
    let big = dir.path().join("big.dxf");
    std::fs::write(&big, drawing(provenance::MAX_BASE_RECS + 1)).unwrap();
    assert!(import(&mut high, &big, json!({})).is_err());
}
