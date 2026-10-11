//! The annotation methods: dimensions, tags, legends, callouts, scale bars, north arrows, revision
//! clouds, styles and the update and error paths.

use kadakas_api::Session;
use kadakas_core::uid::AsUid;
use serde_json::{Value, json};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn err(s: &mut Session, method: &str, params: Value) -> String {
    s.handle(method, params).unwrap_err().message
}

fn exec(s: &mut Session, command: Value) -> Vec<kadakas_core::uid::Uid> {
    ok(s, "execute", json!({ "command": command }))["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_uid().unwrap())
        .collect()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> kadakas_core::uid::Uid {
    exec(
        s,
        json!({"op":"add_object","shape":{"kind":"line","a":a,"b":b}}),
    )[0]
}

fn item(s: &mut Session, id: kadakas_core::uid::Uid) -> Value {
    ok(s, "annotation.evaluate", json!({ "id": id }))
}

#[test]
fn dimensions_follow_objects_and_cover_every_mode() {
    let mut s = Session::with_sequential_ids();
    let ln = line(&mut s, [0.0, 0.0], [5000.0, 0.0]);
    let d = ok(
        &mut s,
        "dimension.add",
        json!({"mode":"horizontal","anchors":[{"object":ln,"vertex":0},{"object":ln,"vertex":1}]}),
    );
    assert_eq!(d["texts"], json!(["5000"]));
    exec(
        &mut s,
        json!({"op":"modify_object","id":ln,"shape":{"kind":"line","a":[0,0],"b":[8000,0]}}),
    );
    let id = d["id"].as_uid().unwrap();
    assert_eq!(item(&mut s, id)["computed"]["texts"], json!(["8000"]));
    // Edge and center anchors.
    ok(
        &mut s,
        "dimension.add",
        json!({"mode":"aligned","anchors":[{"object":ln,"edge":0,"t":0.25},{"object":ln,"vertex":1}]}),
    );
    let circle = exec(
        &mut s,
        json!({"op":"add_object","shape":{"kind":"circle","center":[0,0],"radius":1000}}),
    )[0];
    let arc = exec(
        &mut s,
        json!({"op":"add_object","shape":{"kind":"arc","center":[0,0],"radius":1000,"start":0,"end":1.5}}),
    )[0];
    for (params, want) in [
        (json!({"mode":"vertical","points":[[0,0],[0,3000]]}), "3000"),
        (
            json!({"mode":"aligned","points":[[0,0],[3000,4000]]}),
            "5000",
        ),
        (
            json!({"mode":"ordinate_vertical","points":[[0,0],[100,400]]}),
            "400",
        ),
        (
            json!({"mode":"angular","points":[[0,0],[1000,0],[0,1000]]}),
            "90.0",
        ),
        (
            json!({"mode":"radial","object":circle,"angle":1.0}),
            "R1000",
        ),
        (json!({"mode":"diameter","object":arc}), "2000"),
        (json!({"mode":"arc_length","object":arc}), "Arc 1500"),
    ] {
        let r = ok(&mut s, "dimension.add", params.clone());
        assert!(
            r["texts"][0].as_str().unwrap().contains(want),
            "{params}: {r}"
        );
    }
    let chain = ok(
        &mut s,
        "dimension.add",
        json!({"mode":"chain_horizontal","points":[[0,0],[1000,0],[3000,0]],"offset":4}),
    );
    assert_eq!(chain["texts"], json!(["1000", "2000"]));
    // Errors.
    assert!(
        err(&mut s, "dimension.add", json!({"points":[[0,0],[1,1]]})).contains("mode is required")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"wavy","points":[[0,0],[1,1]]})
        )
        .contains("unknown mode")
    );
    assert!(err(&mut s, "dimension.add", json!({"mode":"horizontal"})).contains("give points"));
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","anchors":"x"})
        )
        .contains("anchors is a list")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"radial","object":ln})
        )
        .contains("circle or an arc")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"radial","object":"00000000-0000-0000-0000-0000000003e7"})
        )
        .contains("does not exist")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","anchors":[{"vertex":0}]})
        )
        .contains("needs object")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","anchors":[{"object":ln}]})
        )
        .contains("vertex, edge or center")
    );
    assert!(err(&mut s, "dimension.add", json!({"mode":"horizontal","anchors":[{"object":ln,"vertex":9},{"object":ln,"vertex":0}]})).contains("no such part"));
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","points":[[0,0],[1,0]],"style":"nope"})
        )
        .contains("no dimension style")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","points":[[0,0],[1,0]],"layer":"Nope"})
        )
        .contains("no layer")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","points":[[0,0],[1,0]],"class":"Nope"})
        )
        .contains("no class")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","points":[[0,0],[1,0]],"viewport":"x"})
        )
        .contains("viewport is an object ID")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","points":[[0,0],[1,0]],"viewport":ln})
        )
        .contains("not a viewport")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"horizontal","points":[[0,0],[1,0]],"viewport":"00000000-0000-0000-0000-0000000003e7"})
        )
        .contains("does not exist")
    );
    // Update: a new mode name, new anchors, wrong object.
    let u = ok(
        &mut s,
        "annotation.update",
        json!({"id":id,"mode":"vertical","offset":12}),
    );
    assert_eq!(u["annotation"]["offset"], 12.0);
    ok(
        &mut s,
        "annotation.update",
        json!({"id":id,"anchors":[{"object":ln,"vertex":0},{"object":ln,"vertex":1}]}),
    );
    assert!(
        err(&mut s, "annotation.update", json!({"id":id,"mode":"wavy"})).contains("unknown mode")
    );
    assert!(
        err(
            &mut s,
            "annotation.update",
            json!({"id":id,"anchors":[{"vertex":1}]})
        )
        .contains("needs object")
    );
    assert!(
        err(&mut s, "annotation.update", json!({"id":ln,"offset":1})).contains("not an annotation")
    );
    assert!(
        err(
            &mut s,
            "annotation.update",
            json!({"id":"00000000-0000-0000-0000-00000000270f"})
        )
        .contains("does not exist")
    );
    assert!(err(&mut s, "annotation.update", json!({})).contains("params.id"));
}

#[test]
fn styles_resources_and_text_annotations() {
    let mut s = Session::with_sequential_ids();
    for ty in ["text_style", "dimension_style", "tag_style", "notes_db"] {
        assert_eq!(
            ok(&mut s, &format!("{ty}.list"), json!({}))["items"],
            json!([])
        );
    }
    let st = ok(
        &mut s,
        "dimension_style.create",
        json!({"name":"Metric","unit":"m","precision":2,"show_unit":true}),
    )["id"]
        .as_uid()
        .unwrap();
    let other = ok(&mut s, "dimension_style.create", json!({"name":"Other"}))["id"]
        .as_uid()
        .unwrap();
    let d = ok(
        &mut s,
        "dimension.add",
        json!({"mode":"horizontal","points":[[0,0],[2500,0]],"style":"metric"}),
    );
    assert_eq!(d["texts"], json!(["2.50 m"]));
    let got = ok(&mut s, "dimension_style.get", json!({"id":"Metric"}));
    assert_eq!(got["used_by"], 1);
    let upd = ok(
        &mut s,
        "dimension_style.update",
        json!({"id":st,"precision":1,"name":"Metric 1"}),
    );
    assert_eq!(upd["precision"], 1);
    assert!(err(&mut s, "dimension_style.delete", json!({"id":st})).contains("used by"));
    ok(
        &mut s,
        "dimension_style.delete",
        json!({"id":st,"reassign":other}),
    );
    assert!(err(&mut s, "dimension_style.get", json!({"id":st})).contains("no dimension style"));
    assert!(
        err(&mut s, "dimension_style.create", json!({"unit":"m"})).contains("name is required")
    );
    assert!(
        err(
            &mut s,
            "dimension_style.create",
            json!({"name":"B","unit":"parsec"})
        )
        .len()
            > 5
    );
    assert!(err(&mut s, "dimension_style.get", json!({"id":false})).contains("no dimension style"));

    let t = ok(
        &mut s,
        "annotation.text",
        json!({"at":[0,0],"text":"Hello"}),
    )["id"]
        .as_uid()
        .unwrap();
    ok(
        &mut s,
        "text_style.create",
        json!({"name":"Big","size_mm":6}),
    );
    assert_eq!(
        ok(
            &mut s,
            "annotation.apply_text_style",
            json!({"ids":[t],"style":"Big"})
        )["changed"],
        1
    );
    assert!(item(&mut s, t)["annotation"]["style"].is_string());
    assert_eq!(item(&mut s, t)["computed"]["text"], "Hello");
    let plain = exec(
        &mut s,
        json!({"op":"add_object","shape":{"kind":"text","at":[0,0],"text":"p","height":300}}),
    )[0];
    assert_eq!(
        ok(
            &mut s,
            "annotation.apply_text_style",
            json!({"ids":[plain],"style":"Big"})
        )["changed"],
        1
    );
    let rect = exec(
        &mut s,
        json!({"op":"add_object","shape":{"kind":"rect","min":[0,0],"max":[1,1]}}),
    )[0];
    assert!(
        err(
            &mut s,
            "annotation.apply_text_style",
            json!({"ids":[rect],"style":"Big"})
        )
        .contains("not text")
    );
    assert!(
        err(
            &mut s,
            "annotation.apply_text_style",
            json!({"ids":["00000000-0000-0000-0000-0000000003e7"],"style":"Big"})
        )
        .contains("does not exist")
    );
    assert!(
        err(
            &mut s,
            "annotation.apply_text_style",
            json!({"ids":"x","style":"Big"})
        )
        .contains("list of object IDs")
    );
    assert!(
        err(
            &mut s,
            "annotation.apply_text_style",
            json!({"ids":[t],"style":"Nope"})
        )
        .contains("no text style")
    );
    let arrow = ok(&mut s, "north_arrow.add", json!({"at":[0,0]}))["id"]
        .as_uid()
        .unwrap();
    assert!(
        err(
            &mut s,
            "annotation.apply_text_style",
            json!({"ids":[arrow],"style":"Big"})
        )
        .contains("no text style")
    );

    ok(
        &mut s,
        "notes_db.create",
        json!({"name":"General","notes":{"N1":"Verify on site"}}),
    );
    ok(
        &mut s,
        "notes_db.update",
        json!({"id":"General","notes":{"N1":"Changed"}}),
    );
    let nd = ok(&mut s, "notes_db.get", json!({"id":"General"}));
    assert_eq!(nd["notes"][0]["text"], "Changed");
    ok(&mut s, "notes_db.delete", json!({"id":"General"}));
    assert!(
        err(
            &mut s,
            "tag_style.create",
            json!({"name":"bad","lines":["{open"]})
        )
        .len()
            > 3
    );
    assert!(err(&mut s, "text_style.update", json!({"id":"nope"})).contains("no text style"));
    assert!(
        err(
            &mut s,
            "text_style.delete",
            json!({"id":"Big","reassign":"nope"})
        )
        .contains("no text style")
    );
}

#[test]
fn tags_legends_callouts_scale_bars_and_revisions() {
    let mut s = Session::with_sequential_ids();
    let style = ok(
        &mut s,
        "plant_style.create",
        json!({"name":"Oak","id_code":"QR","spread":3000,"height":8000}),
    )["id"]
        .as_uid()
        .unwrap();
    exec(
        &mut s,
        json!({"op":"add_plants","style":style,"positions":[[1000,1000],[6000,1000]]}),
    );
    ok(
        &mut s,
        "tag_style.create",
        json!({"name":"Plant tag","lines":["{IdCode} {Kind}","{ROUND(Area, 1)} m2"],"frame":"box"}),
    );
    let r = ok(
        &mut s,
        "data_tag.tag_all",
        json!({"style":"Plant tag","criteria":"Kind = \"plant\""}),
    );
    assert_eq!(r["tagged"], 2);
    let again = ok(
        &mut s,
        "data_tag.tag_all",
        json!({"style":"Plant tag","criteria":"Kind = \"plant\"","layer":"00000000-0000-0000-0000-000000000001"}),
    );
    assert_eq!(again["tagged"], 0);
    assert!(
        err(
            &mut s,
            "data_tag.tag_all",
            json!({"style":"Plant tag","offset":"x"})
        )
        .contains("offset")
    );
    assert!(
        err(
            &mut s,
            "data_tag.tag_all",
            json!({"style":"Plant tag","criteria":"Kind ="})
        )
        .len()
            > 3
    );
    let tags = ok(&mut s, "annotation.list", json!({"type":"data_tag"}));
    assert_eq!(tags["items"].as_array().unwrap().len(), 2);
    assert_eq!(tags["items"][0]["computed"]["lines"][0], "QR plant");
    assert!(err(&mut s, "annotation.list", json!({"type":"bogus"})).contains("unknown type"));
    assert!(err(&mut s, "annotation.list", json!({"layer":"Nope"})).contains("no layer"));

    let plant = ok(&mut s, "objects.list", json!({"kind":"plant"}));
    let pid = plant["objects"]
        .as_array()
        .or(plant.as_array())
        .map(|a| a[0]["id"].as_uid().unwrap())
        .unwrap();
    let one = ok(
        &mut s,
        "data_tag.add",
        json!({"style":"Plant tag","object":pid,"offset":[3,4]}),
    );
    assert!(one["lines"].is_array());
    assert_eq!(one["object_exists"], true);
    assert!(err(&mut s, "data_tag.add", json!({"object":pid})).contains("style is required"));
    assert!(err(&mut s, "data_tag.add", json!({"style":"Plant tag"})).contains("params.object"));
    assert!(
        err(
            &mut s,
            "data_tag.add",
            json!({"style":"Plant tag","object":"00000000-0000-0000-0000-0000000003e7"})
        )
        .contains("cannot be tagged")
    );
    assert!(
        err(
            &mut s,
            "data_tag.add",
            json!({"style":"Plant tag","object":pid,"offset":"x"})
        )
        .contains("offset")
    );
    assert!(
        err(&mut s, "data_tag.add", json!({"style":"nope","object":pid})).contains("no tag style")
    );

    let legend = ok(
        &mut s,
        "legend.add",
        json!({"at":[0,-5000],"title":"Plants","criteria":"Kind = \"plant\"","group_by":"Style"}),
    );
    assert_eq!(legend["rows"], json!([{"label":"Oak","count":2}]));

    // Scale bars follow the scale of the layer, of the viewport or of their own.
    exec(
        &mut s,
        json!({"op":"modify_layer","id":"00000000-0000-0000-0000-000000000001","scale":50}),
    );
    let bar = ok(
        &mut s,
        "scale_bar.add",
        json!({"at":[0,0],"segment":5,"segments":4}),
    );
    assert_eq!(bar["length_mm"], 20000.0);
    assert_eq!(bar["length_paper_mm"], 400.0);
    let own = ok(
        &mut s,
        "scale_bar.add",
        json!({"at":[0,0],"segment":10,"segments":1,"scale":200}),
    );
    assert!((own["length_paper_mm"].as_f64().unwrap() - 50.0).abs() < 1e-6);
    let sheet = ok(&mut s, "sheet.create", json!({"name":"S","paper":"a3"}))["layer"]
        .as_uid()
        .unwrap();
    let vp = ok(
        &mut s,
        "viewport.create",
        json!({"sheet":sheet,"scale":250,"view":"plan"}),
    )["id"]
        .as_uid()
        .unwrap();
    let inside = ok(&mut s, "scale_bar.add", json!({"at":[20,20],"viewport":vp}));
    assert!((inside["length_paper_mm"].as_f64().unwrap() - 80.0).abs() < 1e-6);
    let d = ok(
        &mut s,
        "dimension.add",
        json!({"mode":"horizontal","points":[[20,20],[60,20]],"viewport":vp}),
    );
    assert_eq!(d["id"]["viewport"], vp);
    assert!(err(&mut s, "dimension.add", json!({"mode":"horizontal","anchors":[{"object":pid,"center":true},{"object":pid,"center":true}],"viewport":vp})).contains("paper mm"));
    let tag_in_vp = ok(
        &mut s,
        "data_tag.add",
        json!({"style":"Plant tag","object":pid,"viewport":vp}),
    );
    assert!(tag_in_vp["id"]["viewport"].is_string());
    ok(
        &mut s,
        "annotation.text",
        json!({"at":[20,60],"text":"Notes","layer":sheet}),
    );

    // Callouts with a leader to an object and notes from a database.
    let ln = line(&mut s, [0.0, 0.0], [1000.0, 0.0]);
    ok(
        &mut s,
        "notes_db.create",
        json!({"name":"General","notes":{"N1":"Verify on site"}}),
    );
    let c = ok(
        &mut s,
        "callout.add",
        json!({"at":[3000,3000],"leaders":[{"object":ln,"vertex":1}],"note":"N1","notes_db":"General"}),
    );
    assert_eq!(c["text"], "Verify on site");
    ok(
        &mut s,
        "notes_db.update",
        json!({"id":"General","notes":{"N1":"Changed"}}),
    );
    let cid = c["id"].as_uid().unwrap();
    assert_eq!(item(&mut s, cid)["computed"]["text"], "Changed");
    exec(
        &mut s,
        json!({"op":"move_objects","ids":[ln],"delta":[0,500]}),
    );
    assert_eq!(
        item(&mut s, cid)["annotation"]["leaders"][0]["points"][0],
        json!([1000.0, 500.0])
    );
    ok(
        &mut s,
        "callout.add",
        json!({"at":[3000,3000],"text":"Plain","tip":[0,0]}),
    );
    ok(
        &mut s,
        "callout.add",
        json!({"at":[3000,3000],"text":"Edge","tip":[0,0],"object":ln,"edge":0,"t":0.3}),
    );
    ok(
        &mut s,
        "callout.add",
        json!({"at":[3000,3000],"text":"Mid","leaders":[{"object":ln,"points":[[1,1],[2,2]]},{"points":[[5,5]]}]}),
    );
    assert!(err(&mut s, "callout.add", json!({"at":[0,0],"text":"x"})).contains("give leaders"));
    assert!(
        err(&mut s, "callout.add", json!({"at":[0,0],"leaders":"x"})).contains("leaders is a list")
    );
    assert!(err(&mut s, "callout.add", json!({"at":[0,0],"leaders":[3]})).contains("a leader is"));

    assert_eq!(
        ok(
            &mut s,
            "north_arrow.add",
            json!({"at":[0,0],"rotation_deg":15,"arrow_style":"compass"})
        )["rotation_deg"],
        15.0
    );
    assert!(err(&mut s, "north_arrow.add", json!({"at":[0,0],"style":3})).contains("arrow_style"));
    ok(
        &mut s,
        "revision_cloud.add",
        json!({"points":[[0,0],[1000,0],[1000,1000]],"revision":"A","description":"Moved path"}),
    );
    ok(
        &mut s,
        "revision_cloud.add",
        json!({"points":[[0,0],[500,0],[500,500]],"revision":"A"}),
    );
    let revs = ok(&mut s, "revision.list", json!({}));
    assert_eq!(revs["revisions"][0]["clouds"].as_array().unwrap().len(), 2);
    assert!(err(&mut s, "revision_cloud.add", json!({"points":"x"})).contains("revision_cloud"));

    // The tagged plant leaves: its tags go. Style deletion needs a reassignment.
    exec(&mut s, json!({"op":"delete_objects","ids":[pid]}));
    assert!(
        ok(&mut s, "annotation.list", json!({"type":"data_tag"}))["items"]
            .as_array()
            .unwrap()
            .len()
            <= 2
    );
    assert!(err(&mut s, "tag_style.delete", json!({"id":"Plant tag"})).contains("used by"));
}

#[test]
fn area_dimensions_show_the_area_and_follow_the_shape() {
    let mut s = Session::with_sequential_ids();
    let rect = exec(
        &mut s,
        json!({"op":"add_object","shape":{"kind":"rect","min":[0,0],"max":[4000,5000]}}),
    )[0];
    let style = ok(
        &mut s,
        "dimension_style.create",
        json!({"name":"Areas","area_unit":"ft2","area_precision":0}),
    )["id"]
        .clone();
    let plain = ok(
        &mut s,
        "dimension.add",
        json!({"mode":"area","object":rect}),
    );
    assert_eq!(plain["texts"], json!(["20.00 m\u{b2}"]));
    assert_eq!(plain["values"], json!([20.0e6]));
    let styled = ok(
        &mut s,
        "dimension.add",
        json!({"mode":"area","object":rect,"style":style}),
    );
    assert_eq!(styled["texts"], json!(["215 ft\u{b2}"]));
    // The shape changes: both labels follow.
    exec(
        &mut s,
        json!({"op":"modify_object","id":rect,"shape":{"kind":"rect","min":[0,0],"max":[1000,1000]}}),
    );
    let id = plain["id"].as_uid().unwrap();
    assert_eq!(
        item(&mut s, id)["computed"]["texts"],
        json!(["1.00 m\u{b2}"])
    );
    let id = styled["id"].as_uid().unwrap();
    assert_eq!(
        item(&mut s, id)["computed"]["texts"],
        json!(["11 ft\u{b2}"])
    );
    // Errors.
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"area","points":[[0,0]]})
        )
        .contains("needs object")
    );
    let ln = line(&mut s, [0.0, 0.0], [1.0, 1.0]);
    assert!(
        err(&mut s, "dimension.add", json!({"mode":"area","object":ln})).contains("closed shape")
    );
    assert!(
        err(
            &mut s,
            "dimension.add",
            json!({"mode":"area","object":rect,"style":"nope"})
        )
        .contains("nope")
    );
}
