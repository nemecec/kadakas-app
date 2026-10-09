//! JSON-RPC 2.0 API. The CLI serves it on stdio, and the desktop app serves
//! it to AI agents. Each request is one JSON object on one line.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::command::{Command, Editor};
use crate::model::*;

/// An open document with its history and selection.
#[derive(Default)]
pub struct Session {
    pub editor: Editor,
    pub selection: Vec<ObjectId>,
    replaced: bool,
}

#[derive(Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    fn invalid_params(e: impl std::fmt::Display) -> Self {
        RpcError { code: -32602, message: e.to_string() }
    }
    fn app(e: impl std::fmt::Display) -> Self {
        RpcError { code: 1, message: e.to_string() }
    }
}

fn params<T: for<'de> Deserialize<'de>>(p: Value) -> Result<T, RpcError> {
    let p = if p.is_null() { json!({}) } else { p };
    serde_json::from_value(p).map_err(RpcError::invalid_params)
}

#[derive(Deserialize)]
struct ObjectFilter {
    layer: Option<LayerId>,
    class: Option<ClassId>,
    kind: Option<String>,
}

/// Method names with a short description, for discovery by agents.
pub const METHODS: &[(&str, &str)] = &[
    ("describe", "List methods and command examples."),
    ("execute", "Run one command. params: {command: Command}. Returns {created: [id]}."),
    ("undo", "Undo the last command. Returns {done: bool}."),
    ("redo", "Redo the last undone command. Returns {done: bool}."),
    ("document.summary", "Layers, classes, object counts and drawing bounds."),
    ("document.get", "The full document as JSON."),
    ("document.load", "Replace the document. params: {document}. Clears history."),
    ("objects.list", "Objects. params: {layer?, class?, kind?}."),
    ("objects.get", "One object. params: {id}."),
    ("selection.get", "Selected object IDs."),
    ("selection.set", "Select objects. params: {ids: [id]}."),
    ("render.svg", "SVG image of visible objects. params: {width?: px}."),
    ("report.plants", "Plant schedule: count by species."),
    ("report.areas", "Areas of landscape areas and hardscapes, in m²."),
];

impl Session {
    /// A session for an opened document, with empty history and selection.
    pub fn with_document(doc: Document) -> Self {
        Session { editor: Editor::new(doc), selection: Vec::new(), replaced: false }
    }

    /// Replaces the document, as `document.load` does. Sets the `replaced` flag.
    pub fn replace_document(&mut self, doc: Document) {
        *self = Session { replaced: true, ..Session::with_document(doc) };
    }

    /// True once after the document was replaced by `document.load` (or
    /// `replace_document`). A host that tracks a file path, tool state or
    /// view should poll this after `handle` and reset that state.
    pub fn take_replaced(&mut self) -> bool {
        std::mem::take(&mut self.replaced)
    }

    pub fn handle(&mut self, method: &str, p: Value) -> Result<Value, RpcError> {
        let doc = &self.editor.doc;
        Ok(match method {
            "describe" => json!({
                "methods": METHODS.iter().map(|(m, d)| json!({"name": m, "doc": d})).collect::<Vec<_>>(),
                "units": "millimeters, Y axis up, angles in radians",
                "vertex_kinds": "Polyline points and landscape/hardscape boundaries are vertices. [x, y] is a corner. Other kinds are {p: [x, y], kind: corner | arc (with radius, a fillet) | point_on_arc | bezier | cubic}.",
                "command_examples": command_examples(),
            }),
            "execute" => {
                #[derive(Deserialize)]
                struct P {
                    command: Command,
                }
                let P { command } = params(p)?;
                let out = self.editor.execute(&command).map_err(RpcError::app)?;
                self.prune_selection();
                serde_json::to_value(out).unwrap()
            }
            "undo" => {
                let done = self.editor.undo();
                self.prune_selection();
                json!({ "done": done })
            }
            "redo" => {
                let done = self.editor.redo();
                self.prune_selection();
                json!({ "done": done })
            }
            "document.summary" => {
                let mut kinds = BTreeMap::<&str, usize>::new();
                for o in doc.objects.values() {
                    *kinds.entry(o.shape.kind()).or_default() += 1;
                }
                let bounds = crate::svg::drawing_bounds(doc).map(|(a, b)| json!({"min": a, "max": b}));
                json!({
                    "layers": doc.layers,
                    "classes": doc.classes,
                    "active_layer": doc.active_layer,
                    "active_class": doc.active_class,
                    "object_count": doc.objects.len(),
                    "objects_by_kind": kinds,
                    "bounds": bounds,
                    "can_undo": self.editor.can_undo(),
                    "can_redo": self.editor.can_redo(),
                })
            }
            "document.get" => serde_json::to_value(doc).unwrap(),
            "document.load" => {
                #[derive(Deserialize)]
                struct P {
                    document: Value,
                }
                let P { document } = params(p)?;
                let document = Document::from_value(document).map_err(|e| match e {
                    LoadError::Parse(_) => RpcError::invalid_params(e),
                    e => RpcError::app(e),
                })?;
                self.replace_document(document);
                json!({ "ok": true })
            }
            "objects.list" => {
                let f: ObjectFilter = params(p)?;
                let list: Vec<&Object> = doc
                    .objects
                    .values()
                    .filter(|o| f.layer.is_none_or(|l| o.layer == l))
                    .filter(|o| f.class.is_none_or(|c| o.class == c))
                    .filter(|o| f.kind.as_deref().is_none_or(|k| o.shape.kind() == k))
                    .collect();
                serde_json::to_value(list).unwrap()
            }
            "objects.get" => {
                #[derive(Deserialize)]
                struct P {
                    id: ObjectId,
                }
                let P { id } = params(p)?;
                let o = doc.object(id).ok_or_else(|| RpcError::app(format!("object {id} does not exist")))?;
                serde_json::to_value(o).unwrap()
            }
            "selection.get" => json!({ "ids": self.selection }),
            "selection.set" => {
                #[derive(Deserialize)]
                struct P {
                    ids: Vec<ObjectId>,
                }
                let P { ids } = params(p)?;
                if let Some(bad) = ids.iter().find(|id| doc.object(**id).is_none()) {
                    return Err(RpcError::app(format!("object {bad} does not exist")));
                }
                self.selection = ids;
                json!({ "ids": self.selection })
            }
            "render.svg" => {
                #[derive(Deserialize)]
                struct P {
                    width: Option<u32>,
                }
                let P { width } = params(p)?;
                json!({ "svg": crate::svg::render(doc, width.unwrap_or(800).clamp(16, 8192)) })
            }
            "report.plants" => {
                let mut rows = BTreeMap::<&str, usize>::new();
                for o in doc.objects.values() {
                    if let Shape::Plant { species, .. } = &o.shape {
                        *rows.entry(species).or_default() += 1;
                    }
                }
                json!({ "rows": rows.iter().map(|(s, n)| json!({"species": s, "count": n})).collect::<Vec<_>>() })
            }
            "report.areas" => {
                let rows: Vec<Value> = doc
                    .objects
                    .values()
                    .filter_map(|o| {
                        let name = match &o.shape {
                            Shape::LandscapeArea { name, .. } => name,
                            Shape::Hardscape { material, .. } => material,
                            _ => return None,
                        };
                        let m2 = o.shape.area().unwrap_or(0.0) / 1e6;
                        Some(json!({"id": o.id, "kind": o.shape.kind(), "name": name, "area_m2": m2}))
                    })
                    .collect();
                json!({ "rows": rows })
            }
            _ => return Err(RpcError { code: -32601, message: format!("unknown method: {method}") }),
        })
    }

    /// Drops selected IDs that no longer exist.
    pub fn prune_selection(&mut self) {
        let doc = &self.editor.doc;
        self.selection.retain(|id| doc.object(*id).is_some());
    }

    /// Handles one JSON-RPC 2.0 request line and returns the response line.
    /// Returns `None` for a notification (a request without an id).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error_response(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        let id = req.get("id").cloned();
        let Some(method) = req.get("method").and_then(Value::as_str) else {
            return Some(error_response(id.unwrap_or(Value::Null), -32600, "missing method"));
        };
        let result = self.handle(method, req.get("params").cloned().unwrap_or(Value::Null));
        let id = id?;
        Some(match result {
            Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}).to_string(),
            Err(e) => error_response(id, e.code, &e.message),
        })
    }
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

fn command_examples() -> Value {
    json!([
        {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [5000, 3000]}},
        {"op": "add_object", "shape": {"kind": "plant", "at": [1000, 1000], "species": "Acer campestre", "spread": 4000, "height": 8000}},
        {"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0,0],[3000,0],[3000,1200],[0,1200]], "material": "Granite setts"}},
        {"op": "add_object", "shape": {"kind": "polyline", "closed": true, "points": [[0,0], {"p": [3000,0], "kind": "arc", "radius": 400}, {"p": [3000,2000], "kind": "arc", "radius": 400}, [0,2000]]}},
        {"op": "set_vertex", "id": 2, "index": 1, "kind": {"kind": "arc", "radius": 250}},
        {"op": "move_objects", "ids": [2], "delta": [100, 0]},
        {"op": "add_layer", "name": "Planting", "elevation": 0},
        {"op": "batch", "commands": [{"op": "delete_objects", "ids": [2]}]}
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut Session, line: &str) -> Value {
        serde_json::from_str(&s.handle_line(line).unwrap()).unwrap()
    }

    #[test]
    fn rpc_roundtrip() {
        let mut s = Session::default();
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":1,"method":"execute","params":{"command":{"op":"add_object","shape":{"kind":"plant","at":[0,0],"species":"Oak","spread":3000,"height":9000}}}}"#);
        let id = r["result"]["created"][0].as_u64().unwrap();
        let r = call(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"report.plants"}"#);
        assert_eq!(r["result"]["rows"][0]["count"], 1);
        call(&mut s, &format!(r#"{{"id":3,"method":"selection.set","params":{{"ids":[{id}]}}}}"#));
        call(&mut s, r#"{"id":4,"method":"undo"}"#);
        let r = call(&mut s, r#"{"id":5,"method":"selection.get"}"#);
        assert_eq!(r["result"]["ids"], json!([]));
    }

    #[test]
    fn rpc_errors() {
        let mut s = Session::default();
        assert_eq!(call(&mut s, "{nope")["error"]["code"], -32700);
        assert_eq!(call(&mut s, r#"{"id":1}"#)["error"]["code"], -32600);
        assert_eq!(call(&mut s, r#"{"id":1,"method":"x"}"#)["error"]["code"], -32601);
        assert_eq!(call(&mut s, r#"{"id":1,"method":"objects.get","params":{}}"#)["error"]["code"], -32602);
        assert_eq!(call(&mut s, r#"{"id":1,"method":"objects.get","params":{"id":9}}"#)["error"]["code"], 1);
        assert!(s.handle_line(r#"{"method":"undo"}"#).is_none());
    }

    fn ok(s: &mut Session, method: &str, params: Value) -> Value {
        let r = call(s, &json!({"id": 1, "method": method, "params": params}).to_string());
        assert!(r.get("error").is_none(), "{r}");
        r["result"].clone()
    }

    fn fail(s: &mut Session, method: &str, params: Value) -> (i64, String) {
        let r = call(s, &json!({"id": 1, "method": method, "params": params}).to_string());
        (r["error"]["code"].as_i64().unwrap(), r["error"]["message"].as_str().unwrap().to_string())
    }

    /// Runs one `execute` call and returns the created IDs.
    fn exec(s: &mut Session, command: Value) -> Vec<u64> {
        let r = ok(s, "execute", json!({"command": command}));
        r["created"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
    }

    fn xy(v: &Value) -> (f64, f64) {
        (v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
    }

    fn circle_cmd(x: f64) -> Value {
        json!({"op": "add_object", "shape": {"kind": "circle", "center": [x, 0], "radius": 1}})
    }

    #[test]
    fn describe_lists_every_method_and_every_method_is_routed() {
        let mut s = Session::default();
        let d = ok(&mut s, "describe", json!(null));
        let names: Vec<&str> = d["methods"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
        let expected: Vec<&str> = METHODS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, expected);
        assert!(d["units"].as_str().unwrap().contains("millimeters"));
        assert_eq!(d["command_examples"].as_array().unwrap().len(), 8);

        // Every advertised method is handled: none of them is "unknown method".
        for (name, _) in METHODS {
            let r = call(&mut s, &json!({"id": 1, "method": name, "params": {}}).to_string());
            let code = r["error"]["code"].as_i64().unwrap_or(0);
            assert_ne!(code, -32601, "{name} is advertised but not routed");
        }
    }

    #[test]
    fn execute_reports_app_errors_and_bad_params() {
        let mut s = Session::default();
        let (code, msg) = fail(
            &mut s,
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 0}}}),
        );
        assert_eq!((code, msg.as_str()), (1, "invalid value: radius must be positive"));

        let (code, msg) = fail(&mut s, "execute", json!({"command": {"op": "explode"}}));
        assert_eq!(code, -32602);
        assert!(msg.contains("explode"), "{msg}");

        let (code, _) = fail(&mut s, "execute", json!({}));
        assert_eq!(code, -32602);

        let (code, msg) = fail(&mut s, "execute", json!({"command": {"op": "delete_objects", "ids": [42]}}));
        assert_eq!((code, msg.as_str()), (1, "object 42 does not exist"));
        assert!(ok(&mut s, "document.summary", json!({}))["object_count"] == 0);
    }

    #[test]
    fn undo_and_redo_report_whether_anything_changed() {
        let mut s = Session::default();
        assert_eq!(ok(&mut s, "undo", json!(null)), json!({"done": false}));
        assert_eq!(ok(&mut s, "redo", json!(null)), json!({"done": false}));

        exec(&mut s, circle_cmd(0.0));
        assert_eq!(ok(&mut s, "undo", json!(null)), json!({"done": true}));
        assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 0);
        assert_eq!(ok(&mut s, "redo", json!(null)), json!({"done": true}));
        assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 1);
        assert_eq!(ok(&mut s, "redo", json!(null)), json!({"done": false}));
    }

    #[test]
    fn document_summary_counts_kinds_and_bounds() {
        let mut s = Session::default();
        let empty = ok(&mut s, "document.summary", json!({}));
        assert!(empty["bounds"].is_null());
        assert_eq!(empty["can_undo"], false);
        assert_eq!(empty["layers"].as_array().unwrap().len(), 1);
        assert_eq!(empty["active_class"], 0);

        exec(&mut s, json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [5, 3]}}));
        exec(&mut s, json!({"op": "add_object", "shape": {"kind": "plant", "at": [10, 10], "species": "Oak", "spread": 4, "height": 8}}));
        exec(&mut s, json!({"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": "Oak", "spread": 2, "height": 8}}));
        let sum = ok(&mut s, "document.summary", json!({}));
        assert_eq!(sum["object_count"], 3);
        assert_eq!(sum["objects_by_kind"], json!({"plant": 2, "rect": 1}));
        assert_eq!(sum["can_undo"], true);
        assert_eq!(sum["can_redo"], false);
        assert_eq!(xy(&sum["bounds"]["min"]), (-1.0, -1.0));
        assert_eq!(xy(&sum["bounds"]["max"]), (12.0, 12.0));
    }

    #[test]
    fn document_get_returns_the_whole_document() {
        let mut s = Session::default();
        exec(&mut s, circle_cmd(0.0));
        let d = ok(&mut s, "document.get", json!(null));
        assert_eq!(d["format_version"], FORMAT_VERSION);
        assert_eq!(d["objects"].as_array().unwrap().len(), 1);
        assert_eq!(d["objects"][0]["shape"]["kind"], "circle");
        assert_eq!(d["layers"][0]["name"], "Layer 1");
    }

    #[test]
    fn document_load_replaces_document_and_clears_history() {
        let mut s = Session::default();
        let id = exec(&mut s, circle_cmd(0.0))[0];
        ok(&mut s, "selection.set", json!({"ids": [id]}));

        let blank = serde_json::to_value(Document::default()).unwrap();
        assert_eq!(ok(&mut s, "document.load", json!({"document": blank})), json!({"ok": true}));
        let sum = ok(&mut s, "document.summary", json!({}));
        assert_eq!(sum["object_count"], 0);
        assert_eq!(sum["can_undo"], false);
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": []}));
    }

    #[test]
    fn document_load_rejects_newer_format_and_bad_shape() {
        let mut s = Session::default();
        let mut newer = serde_json::to_value(Document::default()).unwrap();
        newer["format_version"] = json!(FORMAT_VERSION + 1);
        let (code, msg) = fail(&mut s, "document.load", json!({"document": newer}));
        assert_eq!(code, 1);
        assert_eq!(msg, format!("file format {} is newer than this app", FORMAT_VERSION + 1));

        let (code, _) = fail(&mut s, "document.load", json!({"document": {"format_version": 1}}));
        assert_eq!(code, -32602);
        let (code, _) = fail(&mut s, "document.load", json!({}));
        assert_eq!(code, -32602);
    }

    #[test]
    fn objects_list_filters_by_layer_class_and_kind() {
        let mut s = Session::default();
        let layer = exec(&mut s, json!({"op": "add_layer", "name": "Planting"}))[0];
        let class = exec(&mut s, json!({"op": "add_class", "name": "Trees"}))[0];
        let c1 = exec(&mut s, circle_cmd(0.0))[0];
        exec(&mut s, json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1, 1]}}));
        let c3 = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 1}, "layer": layer, "class": class}),
        )[0];
        exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": "Oak", "spread": 1, "height": 1}, "layer": layer}),
        );

        let ids = |v: Value| -> Vec<u64> { v.as_array().unwrap().iter().map(|o| o["id"].as_u64().unwrap()).collect() };
        assert_eq!(ids(ok(&mut s, "objects.list", json!({}))).len(), 4);
        assert_eq!(ids(ok(&mut s, "objects.list", json!({"layer": layer}))).len(), 2);
        assert_eq!(ids(ok(&mut s, "objects.list", json!({"class": class}))), vec![c3]);
        assert_eq!(ids(ok(&mut s, "objects.list", json!({"kind": "circle"}))), vec![c1, c3]);
        assert_eq!(ids(ok(&mut s, "objects.list", json!({"layer": layer, "kind": "circle"}))), vec![c3]);
        assert!(ids(ok(&mut s, "objects.list", json!({"kind": "text"}))).is_empty());
        assert_eq!(ids(ok(&mut s, "objects.list", json!({"layer": null}))).len(), 4);
    }

    #[test]
    fn objects_get_returns_object_or_error() {
        let mut s = Session::default();
        let id = exec(&mut s, circle_cmd(3.0))[0];
        let o = ok(&mut s, "objects.get", json!({"id": id}));
        assert_eq!(o["id"], id);
        assert_eq!(o["layer"], 1);
        assert_eq!(o["shape"]["radius"], 1.0);

        assert_eq!(fail(&mut s, "objects.get", json!({"id": 999})), (1, "object 999 does not exist".into()));
        assert_eq!(fail(&mut s, "objects.get", json!({})).0, -32602);
    }

    #[test]
    fn selection_is_validated_and_pruned_when_objects_go_away() {
        let mut s = Session::default();
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": []}));
        let a = exec(&mut s, circle_cmd(0.0))[0];
        let b = exec(&mut s, circle_cmd(5.0))[0];
        assert_eq!(ok(&mut s, "selection.set", json!({"ids": [a, b]})), json!({"ids": [a, b]}));

        // An unknown ID is rejected and the selection stays as it was.
        assert_eq!(fail(&mut s, "selection.set", json!({"ids": [a, 999]})), (1, "object 999 does not exist".into()));
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": [a, b]}));

        // Deleting a selected object drops it from the selection.
        exec(&mut s, json!({"op": "delete_objects", "ids": [a]}));
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": [b]}));

        // Undo that brings the object back, but the old selection is not restored.
        ok(&mut s, "undo", json!(null));
        assert_eq!(ok(&mut s, "selection.get", json!({})), json!({"ids": [b]}));
    }

    #[test]
    fn render_svg_width_is_clamped() {
        let mut s = Session::default();
        exec(&mut s, circle_cmd(0.0));
        let width_of = |svg: &str| -> String {
            let start = svg.find(r#"width=""#).unwrap() + 7;
            svg[start..].split('"').next().unwrap().to_string()
        };
        let render = |s: &mut Session, p: Value| ok(s, "render.svg", p)["svg"].as_str().unwrap().to_string();

        assert_eq!(width_of(&render(&mut s, json!({}))), "800");
        assert_eq!(width_of(&render(&mut s, json!({"width": 300}))), "300");
        assert_eq!(width_of(&render(&mut s, json!({"width": 0}))), "16");
        assert_eq!(width_of(&render(&mut s, json!({"width": 1}))), "16");
        assert_eq!(width_of(&render(&mut s, json!({"width": 100000}))), "8192");
        assert!(render(&mut s, json!({})).contains(r#"data-id="#));
        assert_eq!(fail(&mut s, "render.svg", json!({"width": "wide"})).0, -32602);
    }

    #[test]
    fn report_plants_counts_by_species_sorted() {
        let mut s = Session::default();
        assert_eq!(ok(&mut s, "report.plants", json!({})), json!({"rows": []}));
        for species in ["Oak", "Acer", "Oak"] {
            exec(&mut s, json!({"op": "add_object", "shape": {"kind": "plant", "at": [0, 0], "species": species, "spread": 1, "height": 1}}));
        }
        exec(&mut s, circle_cmd(0.0));
        assert_eq!(
            ok(&mut s, "report.plants", json!({})),
            json!({"rows": [{"species": "Acer", "count": 1}, {"species": "Oak", "count": 2}]})
        );
    }

    #[test]
    fn report_areas_gives_square_meters() {
        let mut s = Session::default();
        let meadow = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "landscape_area", "boundary": [[0, 0], [2000, 0], [2000, 3000], [0, 3000]], "name": "Meadow"}}),
        )[0];
        let setts = exec(
            &mut s,
            json!({"op": "add_object", "shape": {"kind": "hardscape", "boundary": [[0, 0], [3000, 0], [0, 2000]], "material": "Setts"}}),
        )[0];
        exec(&mut s, json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [9000, 9000]}}));

        let rows = ok(&mut s, "report.areas", json!({}))["rows"].clone();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 2, "rects are not areas for this report");
        assert_eq!(rows[0]["id"], meadow);
        assert_eq!(rows[0]["kind"], "landscape_area");
        assert_eq!(rows[0]["name"], "Meadow");
        assert!((rows[0]["area_m2"].as_f64().unwrap() - 6.0).abs() < 1e-9);
        assert_eq!(rows[1]["id"], setts);
        assert_eq!(rows[1]["kind"], "hardscape");
        assert_eq!(rows[1]["name"], "Setts");
        assert!((rows[1]["area_m2"].as_f64().unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn notifications_and_bad_request_ids() {
        let mut s = Session::default();
        // A request without an id is a notification and gets no reply, even on error.
        assert!(s.handle_line(r#"{"method":"nope"}"#).is_none());
        // A missing method is reported with a null id when there is no id.
        let r: Value = serde_json::from_str(&s.handle_line(r#"{"params":{}}"#).unwrap()).unwrap();
        assert_eq!(r["error"]["code"], -32600);
        assert!(r["id"].is_null());
        // A parse error has a null id.
        let r: Value = serde_json::from_str(&s.handle_line("[1,").unwrap()).unwrap();
        assert_eq!(r["error"]["code"], -32700);
        assert!(r["error"]["message"].as_str().unwrap().starts_with("parse error"));
    }

    #[test]
    fn load_with_stale_next_id_does_not_reuse_ids() {
        let mut doc = serde_json::to_value(Document::default()).unwrap();
        doc["objects"] = json!([{"id": 2, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        // next_id stays at 2, the ID the object above already uses.
        let mut s = Session::default();
        ok(&mut s, "document.load", json!({"document": doc}));
        let new_id = exec(&mut s, circle_cmd(5.0))[0];
        assert_ne!(new_id, 2);
    }

    #[test]
    fn command_examples_parse() {
        for ex in command_examples().as_array().unwrap() {
            serde_json::from_value::<Command>(ex.clone()).unwrap();
        }
    }

    #[test]
    fn document_load_signals_that_the_document_was_replaced() {
        let mut s = Session::default();
        assert!(!s.take_replaced());
        exec(&mut s, circle_cmd(0.0));
        assert!(!s.take_replaced(), "normal edits do not replace the document");
        ok(&mut s, "document.load", json!({"document": serde_json::to_value(Document::default()).unwrap()}));
        assert!(s.take_replaced());
        assert!(!s.take_replaced(), "the flag is cleared when read");

        // A refused load keeps the old document and does not set the flag.
        exec(&mut s, circle_cmd(0.0));
        let mut newer = serde_json::to_value(Document::default()).unwrap();
        newer["format_version"] = json!(FORMAT_VERSION + 1);
        fail(&mut s, "document.load", json!({"document": newer}));
        assert!(!s.take_replaced());
        assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 1);
    }

    #[test]
    fn document_load_rejects_invalid_documents_and_repairs_next_id() {
        let mut s = Session::default();
        let mut bad = serde_json::to_value(Document::default()).unwrap();
        bad["active_layer"] = json!(42);
        assert_eq!(fail(&mut s, "document.load", json!({"document": bad})).0, 1);

        let mut doc = serde_json::to_value(Document::default()).unwrap();
        doc.as_object_mut().unwrap().remove("next_id");
        doc["objects"] = json!([{"id": 2, "layer": 1, "class": 0, "shape": {"kind": "circle", "center": [0, 0], "radius": 1}}]);
        ok(&mut s, "document.load", json!({"document": doc}));
        assert_ne!(exec(&mut s, circle_cmd(1.0))[0], 2);
    }

    #[test]
    fn replace_document_and_prune_selection_are_public() {
        let mut s = Session::default();
        let id = exec(&mut s, circle_cmd(0.0))[0];
        s.selection = vec![ObjectId(id), ObjectId(999)];
        s.prune_selection();
        assert_eq!(s.selection, vec![ObjectId(id)]);
        s.replace_document(Document::default());
        assert!(s.selection.is_empty() && s.take_replaced());
        assert!(Session::with_document(Document::default()).editor.doc.objects.is_empty());
    }

    #[test]
    fn execute_set_data_over_rpc() {
        let mut s = Session::default();
        let id = exec(&mut s, circle_cmd(0.0))[0];
        exec(&mut s, json!({"op": "set_data", "id": id, "set": {"Pset": {"a": 1}}}));
        assert_eq!(ok(&mut s, "objects.get", json!({"id": id}))["data"], json!({"Pset": {"a": 1}}));
        exec(&mut s, json!({"op": "set_data", "id": id, "remove": ["Pset"]}));
        assert!(ok(&mut s, "objects.get", json!({"id": id})).get("data").is_none());
    }
}
