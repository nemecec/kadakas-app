//! Records, data maps and worksheets through the JSON-RPC methods.

use cadektor_api::Session;
use serde_json::{Value, json};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn err(s: &mut Session, method: &str, params: Value) -> String {
    s.handle(method, params).unwrap_err().message
}

fn exec(s: &mut Session, command: Value) -> u64 {
    ok(s, "execute", json!({ "command": command }))["created"][0]
        .as_u64()
        .unwrap()
}

fn plant(s: &mut Session, x: f64, species: &str) -> u64 {
    exec(
        s,
        json!({"op":"add_object","shape":{"kind":"plant","at":[x,0],"species":species,"spread":2000,"height":3000}}),
    )
}

fn rect(s: &mut Session, x: f64) -> u64 {
    exec(
        s,
        json!({"op":"add_object","shape":{"kind":"rect","min":[x,0],"max":[x + 1000.0, 1000]}}),
    )
}

#[test]
fn record_formats_attach_set_get_and_detach() {
    let mut s = Session::default();
    assert_eq!(
        ok(&mut s, "record_format.list", json!({}))["formats"],
        json!([])
    );
    let id = ok(
        &mut s,
        "record_format.create",
        json!({"name":"Soil","fields":[
            {"name":"Depth","type":"dimension"},
            {"name":"Kind","type":"choice","choices":["clay","sand"]},
            {"name":"Note","type":"text"}]}),
    )["id"]
        .as_u64()
        .unwrap();
    let got = ok(&mut s, "record_format.get", json!({"id":"soil"}));
    assert_eq!(got["fields"].as_array().unwrap().len(), 3);
    assert_eq!(got["used_by"], 0);
    // Bad field lists and types.
    assert!(
        err(
            &mut s,
            "record_format.create",
            json!({"name":"X","fields":"nope"})
        )
        .contains("fields")
    );
    assert!(
        err(
            &mut s,
            "record_format.create",
            json!({"name":"X","fields":[{"name":"A","type":"unknown"}]})
        )
        .contains("unknown type")
    );
    assert!(err(&mut s, "record_format.get", json!({"id":999})).contains("no record format"));
    assert!(err(&mut s, "record_format.get", json!({"id":true})).contains("no record format"));

    let a = rect(&mut s, 0.0);
    let b = rect(&mut s, 5000.0);
    let n = ok(
        &mut s,
        "record.attach",
        json!({"ids":[a,b],"record":id,"values":{"Depth":300,"Kind":"clay"}}),
    );
    assert_eq!(n["objects"], 2);
    let rec = ok(&mut s, "record.get", json!({"id":a}));
    assert_eq!(rec["records"]["Soil"]["Depth"], 300.0);
    ok(
        &mut s,
        "record.set",
        json!({"ids":[a],"record":"Soil","values":{"Depth":450}}),
    );
    assert_eq!(
        ok(&mut s, "record.get", json!({"id":a}))["records"]["Soil"]["Depth"],
        450.0
    );
    assert_eq!(
        ok(&mut s, "record_format.get", json!({"id":id}))["used_by"],
        2
    );
    assert!(
        err(&mut s, "record.attach", json!({"ids":[777],"record":id})).contains("does not exist")
    );
    assert!(err(&mut s, "record.get", json!({"id":777})).contains("does not exist"));
    assert!(
        err(&mut s, "record.attach", json!({"ids":[a],"record":"Nope"}))
            .contains("no record format")
    );

    let upd = ok(
        &mut s,
        "record_format.update",
        json!({"id":id,"name":"Soils","fields":[{"name":"Depth","type":"dimension"}]}),
    );
    assert_eq!(upd["name"], "Soils");
    ok(&mut s, "record.detach", json!({"ids":[a,b],"record":id}));
    assert_eq!(
        ok(&mut s, "record.get", json!({"id":a}))["records"],
        json!({})
    );
    ok(&mut s, "record_format.delete", json!({"id":id}));
    assert_eq!(
        ok(&mut s, "record_format.list", json!({}))["formats"],
        json!([])
    );
    assert!(err(&mut s, "record_format.delete", json!({"id":id})).contains("no record format"));
}

#[test]
fn data_maps_are_created_checked_evaluated_and_applied() {
    let mut s = Session::default();
    assert_eq!(
        ok(&mut s, "data_manager.list", json!({}))["maps"],
        json!([])
    );
    let props = ok(&mut s, "data_manager.properties", json!({}));
    assert!(props["builtin"].as_array().unwrap().len() > 3);
    assert!(
        props["ifc_types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "area")
    );
    ok(
        &mut s,
        "record_format.create",
        json!({"name":"Cost","fields":[{"name":"Area","type":"area"}]}),
    );
    assert!(
        ok(&mut s, "data_manager.properties", json!({}))["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["record"] == "Cost")
    );
    assert!(
        err(
            &mut s,
            "data_manager.create",
            json!({"name":"M","template":"nope"})
        )
        .contains("unknown template")
    );
    let landscape = ok(
        &mut s,
        "data_manager.create",
        json!({"name":"Landscape IFC","template":"landscape"}),
    )["id"]
        .as_u64()
        .unwrap();
    let rules = json!([{"kinds":["rect"],"source":{"type":"property","name":"Area"},
        "target":{"type":"record_field","record":"Cost","field":"Area"}}]);
    let map = ok(
        &mut s,
        "data_manager.create",
        json!({"name":"Takeoff","entities":[{"kind":"rect","ifc_class":"IfcSlab"}],"rules":rules}),
    )["id"]
        .as_u64()
        .unwrap();
    let got = ok(&mut s, "data_manager.get", json!({"id":"takeoff"}));
    assert_eq!(got["rules"].as_array().unwrap().len(), 1);
    assert_eq!(
        ok(&mut s, "data_manager.list", json!({}))["maps"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // A record on the rect receives the value by `apply`.
    let r = rect(&mut s, 0.0);
    rect(&mut s, 3000.0);
    plant(&mut s, 9000.0, "Oak");
    ok(&mut s, "record.attach", json!({"ids":[r],"record":"Cost"}));
    let chk = ok(&mut s, "data_manager.check", json!({"id":map}));
    assert!(chk["problems"].is_array());
    let ev = ok(&mut s, "data_manager.evaluate", json!({"id":map}));
    assert_eq!(ev["total"], 3);
    let ev = ok(
        &mut s,
        "data_manager.evaluate",
        json!({"id":map,"ids":[r],"limit":5}),
    );
    assert_eq!(ev["objects"][0]["ifc_class"], "IfcSlab");
    assert!(
        err(
            &mut s,
            "data_manager.evaluate",
            json!({"id":map,"ids":[999]})
        )
        .contains("does not exist")
    );
    let limited = ok(
        &mut s,
        "data_manager.evaluate",
        json!({"id":landscape,"limit":1}),
    );
    assert_eq!(limited["objects"].as_array().unwrap().len(), 1);
    let applied = ok(&mut s, "data_manager.apply", json!({"id":map}));
    assert!(applied["changed"].as_u64().unwrap() >= 1, "{applied}");
    assert_eq!(
        ok(&mut s, "data_manager.apply", json!({"id":map}))["changed"],
        0
    );
    let upd = ok(
        &mut s,
        "data_manager.update",
        json!({"id":map,"name":"Takeoff 2","rules":[]}),
    );
    assert_eq!(upd["name"], "Takeoff 2");
    assert_eq!(upd["rules"], json!([]));
    let upd = ok(
        &mut s,
        "data_manager.update",
        json!({"id":map,"entities":[]}),
    );
    assert_eq!(upd["entities"], json!([]));
    assert_eq!(
        ok(&mut s, "data_manager.apply", json!({"id":map}))["changed"],
        0
    );
    ok(&mut s, "data_manager.delete", json!({"id":map}));
    assert!(err(&mut s, "data_manager.get", json!({"id":map})).contains("no data map"));
    assert!(err(&mut s, "data_manager.check", json!({"id":map})).contains("no data map"));
}

#[test]
fn worksheets_hold_cells_databases_and_export() {
    let mut s = Session::default();
    assert_eq!(
        ok(&mut s, "worksheet.list", json!({}))["worksheets"],
        json!([])
    );
    let templates = ok(&mut s, "worksheet.templates", json!({}));
    let first = templates["templates"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        err(
            &mut s,
            "worksheet.create",
            json!({"name":"X","template":"nope"})
        )
        .contains("unknown template")
    );
    assert!(
        err(
            &mut s,
            "worksheet.create",
            json!({"name":"X","cells":{"A1":[1,2]}})
        )
        .contains("cell input")
    );
    let t = ok(
        &mut s,
        "worksheet.create",
        json!({"name":"From template","template":first}),
    )["id"]
        .as_u64()
        .unwrap();
    assert!(ok(&mut s, "worksheet.get", json!({"id":t}))["definition"].is_object());
    let w = ok(
        &mut s,
        "worksheet.create",
        json!({"name":"Sums","cells":{"A1":1,"B1":2.5,"C1":"=A1+B1","D1":true,"E1":"text","F1":null}}),
    )["id"]
        .as_u64()
        .unwrap();
    let ev = ok(&mut s, "worksheet.evaluate", json!({"id":w}));
    assert_eq!(ev["cells"]["C1"], 3.5);
    assert_eq!(ev["errors"], 0);
    assert!(!ev["text"].is_null());

    ok(
        &mut s,
        "worksheet.set_cell",
        json!({"id":"Sums","address":"A2","input":"=C1*2","decimals":2,"unit":"m"}),
    );
    ok(
        &mut s,
        "worksheet.set_cell",
        json!({"id":w,"address":"a2","decimals":1}),
    );
    ok(
        &mut s,
        "worksheet.set_cell",
        json!({"id":w,"address":"A3","input":"=1/0"}),
    );
    let n = ok(
        &mut s,
        "worksheet.set_cells",
        json!({"id":w,"cells":{"B2":"x","B3":4}}),
    );
    assert_eq!(n["cells"], 2);
    let ev = ok(&mut s, "worksheet.evaluate", json!({"id":w}));
    assert!(ev["errors"].as_u64().unwrap() >= 1);
    assert!(
        err(
            &mut s,
            "worksheet.set_cell",
            json!({"id":w,"address":"A1","input":{"a":1}})
        )
        .contains("cell input")
    );
    assert!(
        err(
            &mut s,
            "worksheet.set_cell",
            json!({"id":w,"address":"??","input":"1"})
        )
        .len()
            > 3
    );

    plant(&mut s, 0.0, "Oak");
    plant(&mut s, 3000.0, "Birch");
    let db = json!({"anchor":"A6","criteria":"Kind = \"plant\"",
        "columns":[{"header":"Species","expr":"Species"},{"header":"Count","expr":"Count","summary":"sum"}]});
    let idx = ok(
        &mut s,
        "worksheet.set_database",
        json!({"id":w,"database":db}),
    );
    assert_eq!(idx["index"], 0);
    ok(
        &mut s,
        "worksheet.set_database",
        json!({"id":w,"index":0,"database":db}),
    );
    assert!(
        err(
            &mut s,
            "worksheet.set_database",
            json!({"id":w,"index":5,"database":db})
        )
        .contains("does not exist")
    );
    let ev = ok(&mut s, "worksheet.evaluate", json!({"id":w}));
    assert_eq!(ev["blocks"][0]["matched"], 2);
    let csv = ok(&mut s, "worksheet.export", json!({"id":w,"format":"CSV"}));
    assert!(csv["csv"].as_str().unwrap().contains("Oak"));
    let xlsx = ok(&mut s, "worksheet.export", json!({"id":w,"format":"xlsx"}));
    assert!(xlsx["xlsx_base64"].as_str().unwrap().len() > 100);
    assert!(
        err(&mut s, "worksheet.export", json!({"id":w,"format":"pdf"})).contains("unknown format")
    );

    let placed = ok(
        &mut s,
        "worksheet.place",
        json!({"id":w,"at":[0,0],"text_height":3}),
    );
    let table = placed["id"].as_u64().unwrap();
    assert!(placed["size"][0].as_f64().unwrap() > 0.0);
    assert!(
        err(
            &mut s,
            "worksheet.place",
            json!({"id":w,"at":[0,0],"text_height":-1})
        )
        .contains("positive")
    );
    let second = ok(&mut s, "worksheet.place", json!({"id":t,"at":[100,100]}))["id"]
        .as_u64()
        .unwrap();
    ok(
        &mut s,
        "worksheet.set_cell",
        json!({"id":w,"address":"H1","input":"a long text that widens the column"}),
    );
    assert_eq!(
        ok(&mut s, "worksheet.fit", json!({"ids":[table]}))["fitted"],
        1
    );
    assert_eq!(ok(&mut s, "worksheet.fit", json!({}))["fitted"], 2);
    let r = rect(&mut s, 20000.0);
    assert!(err(&mut s, "worksheet.fit", json!({"ids":[r]})).contains("not a worksheet table"));

    assert!(
        err(
            &mut s,
            "worksheet.remove_database",
            json!({"id":w,"index":3})
        )
        .contains("does not exist")
    );
    ok(
        &mut s,
        "worksheet.remove_database",
        json!({"id":w,"index":0}),
    );
    ok(&mut s, "worksheet.rename", json!({"id":w,"name":"Renamed"}));
    let list = ok(&mut s, "worksheet.list", json!({}));
    assert!(
        list["worksheets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "Renamed")
    );
    assert!(err(&mut s, "worksheet.delete", json!({"id":"renamed"})).contains("used by"));
    let tables = vec![table, second];
    ok(
        &mut s,
        "execute",
        json!({"command":{"op":"delete_objects","ids":tables}}),
    );
    ok(&mut s, "worksheet.delete", json!({"id":"renamed"}));
    assert!(err(&mut s, "worksheet.get", json!({"id":w})).contains("no worksheet"));
    assert!(ok(&mut s, "describe", json!({}))["data"]["worksheet"]["templates"].is_array());
}
