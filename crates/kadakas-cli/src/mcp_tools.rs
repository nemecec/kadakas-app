//! The tool listing of the MCP server.
//!
//! The default listing has one tool per domain (about twenty). A domain tool takes
//! `{method, params}` and its description lists the methods of the domain, taken from `METHODS`.
//! With `--mcp-flat` the server lists one tool per API method instead (about 280).

use kadakas_api::METHODS;
use serde_json::{Value, json};

/// One domain tool.
pub struct Domain {
    pub name: &'static str,
    pub summary: &'static str,
    /// The part of a method name before the dot, or the whole name of a method without a dot.
    pub prefixes: &'static [&'static str],
    /// True if no method of the domain changes the document or the session.
    pub read_only: bool,
    /// True if methods of the domain reach out to the network.
    pub open_world: bool,
}

pub const DOMAINS: &[Domain] = &[
    Domain {
        name: "describe",
        summary: "Start here. Lists every method with a short description, the command examples and the units (mm, Y axis up).",
        prefixes: &["describe"],
        read_only: true,
        open_world: false,
    },
    Domain {
        name: "document",
        summary: "The document and its history of changes. `execute` runs one command (add, modify, move or delete objects, and change layers, classes and settings: layers and classes have no methods of their own, use their commands). Also the project data, saved views of layer visibility, the document files and the change events (`events.poll`; MCP has no pushed messages). Also collaboration (`collab.*`: connect to a shared document, sync, presence, per-user undo).",
        prefixes: &[
            "execute",
            "document",
            "project",
            "saved_view",
            "file",
            "visibility",
            "events",
            "collab",
        ],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "objects",
        summary: "Read objects of the drawing.",
        prefixes: &["objects"],
        read_only: true,
        open_world: false,
    },
    Domain {
        name: "edit_2d",
        summary: "Edit 2D geometry: rotate, scale, mirror, duplicate array (linear, rectangular, circular), offset, fillet, chamfer, trim, extend, split, and surface operations (add, subtract, clip, intersect, compose). Also snap queries.",
        prefixes: &["edit", "surface", "snap"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "selection_undo",
        summary: "The selection of the session, and undo and redo.",
        prefixes: &["selection", "undo", "redo"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "reports",
        summary: "Read-only reports: plant schedules, areas, existing trees, hardscape and landscape quantities.",
        prefixes: &["report"],
        read_only: true,
        open_world: false,
    },
    Domain {
        name: "render_view",
        summary: "Pictures of the drawing (SVG, PNG, hidden-line; render.png and render.svg take a `compare` option that tints what changed since a version, see the review domain), the 3D view, and display rules: the northing/easting grid, project phases and data visualizations (color by a property).",
        prefixes: &[
            "render",
            "view",
            "view_rules",
            "grid",
            "phase",
            "visualization",
        ],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "materials_styles",
        summary: "Materials, render styles and the sun.",
        prefixes: &["material", "render_style", "sun"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "plants",
        summary: "Plant styles (a catalog of plants), planting, plant tags and tree surveys.",
        prefixes: &["plant_style", "plants", "plant_tag", "tree"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "site",
        summary: "Site models (terrain), contours, volumes, source data checks, survey point import, pads with retaining edges, texture beds, and the survey point of the georeference.",
        prefixes: &["site", "pad", "grade_limit", "bed", "survey", "georef"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "model_3d",
        summary: "3D modeling objects: massing models (buildings), extrudes, sweeps (a profile turned around an axis, spirals) and 3D polygons, with Model > Extrude and Model > Sweep from 2D shapes.",
        prefixes: &["massing", "extrude", "sweep", "polygon3d"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "mesh",
        summary: "Surface models, the edge-and-face geometry of the 3D modeler: read vertices, edges and faces (with paging), the closed-solid report, bounds, faces near a point and a ray pick, and edit them in single undo steps: draw edges and curves, erase, Push/Pull, Follow Me, Offset, Intersect Faces, move, rotate, scale, copy, paint, hide, soften (also every edge under an angle, with the count of edges it changed), the solid tools (union, subtract, intersect, trim, split), groups and components. mesh.batch runs up to 50 edits as one undo step, all or nothing. mesh.render gives a picture of the result. A refusal has a stable error_id. describe.mesh has the model, the limits and examples.",
        prefixes: &["mesh", "mesh3d"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "grading",
        summary: "Grade objects, spot elevations, landscape walls and section profiles of the ground.",
        prefixes: &["grade", "spot", "wall", "wall_style", "section"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "hardscape_landscape",
        summary: "Hardscape objects and styles (holes, pattern origin and angle, hardscape.from_shapes for closed outlines), landscape areas and their styles. Regions from loose lines and arcs (region.from_curves: closes gaps, reports what does not close). Zones and courses of a hardscape (hardscape.split, hardscape.course, hardscape.plan). Unit patterns (pavers, tiles) over any closed shape: unit_pattern.layout, unit_pattern.to_objects, unit_pattern.check (overlaps, joints, small pieces, sizes, coverage), unit_pattern.module_* (module patterns from presets, shapes or a DXF block); the counts are in report.unit_pattern (per zone and course, and in total); export.file with patterns blocks writes whole units as INSERTs.",
        prefixes: &[
            "region",
            "hardscape",
            "hardscape_style",
            "unit_pattern",
            "landscape_area",
            "landscape_area_style",
        ],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "irrigation",
        summary: "Irrigation design: outlets, lines, valves, controllers, the catalog, layout helpers, hydraulic calculation and reports.",
        prefixes: &["irrigation"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "annotation",
        summary: "Dimensions, callouts, data tags, legends, north arrows, scale bars, revision clouds, text, and their styles and notes databases.",
        prefixes: &[
            "dimension",
            "callout",
            "data_tag",
            "legend",
            "north_arrow",
            "scale_bar",
            "revision_cloud",
            "annotation",
            "revision",
            "text_style",
            "dimension_style",
            "tag_style",
            "notes_db",
        ],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "sheets_print",
        summary: "Sheet layers, viewports, section viewports and title blocks. PDF, SVG and PNG output of sheets goes through the io tool (export.file).",
        prefixes: &["sheet", "viewport", "title_block", "section_viewport"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "data_worksheets",
        summary: "Record formats and records on objects, the Data Manager (data maps) and worksheets with database blocks.",
        prefixes: &["record_format", "record", "data_manager", "worksheet"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "resources_symbols",
        summary: "Resources (the Resource Manager and library files), symbols, groups and hatch patterns.",
        prefixes: &["resource", "symbol", "group", "pattern", "library"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "review",
        summary: "What the person sees and what the agent shows: view.get (the camera, zoom, visible box, selection, active layer and visible layers and classes of the person), view.zoom, view.pan and view.camera (move the view of the person with a short animation; the person wins while they navigate and can step back with Back), and view.highlight and view.highlight_clear (point at objects or a place with a short text; the agent shows like a collaborator). They work only in the app (kadakas app --agent-port), never change the document, and describe.review.agent_view has the rules and examples. agent.activity lists the steps of the agent (each call or script run that changed the document, with a summary and the objects it touched; it only reads, and the person reverts a step in the app; describe.review.agent_activity). Review issues and named versions. Issues (BCF terms): issue.create (from objects, a point or the current view, with a viewpoint and a snapshot picture), issue.list (filter by status, assignee, label, object, text), issue.get (viewpoints and the comment thread), issue.update, issue.comment, issue.resolve, issue.delete (the work of somebody else needs the consent of the person: confirm), and BCF files: issue.export_bcf and issue.import_bcf (.bcfzip, BCF 3.0 and 2.1; an issue with the same GUID and other content is a conflict that the person decides). Issues made by an agent have the origin agent; describe.review.issues has the model, the limits and examples. Named versions of the document and the diff between them: save a version, list, read, restore (one undo step, the current state is saved first), delete, and compare two versions or a version and the current document (added, removed and changed things by ID, with the changed property paths and old and new values). The visual compare is the `compare` option of render.png, render.svg and sheet.render (domains render_view and sheets): added objects green, changed orange, removed objects as red ghosts, or both sides next to each other; describe.review.compare has the options. dxf.compare compares two DXF or DWG files without importing them: a JSON diff, and an overlay picture (png, svg or pdf) with optional revision clouds (describe.review.dxf_compare).",
        prefixes: &["version", "dxf", "issue", "agent"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "io",
        summary: "Import and export files: DXF, DWG (needs a converter), Shapefile, GeoJSON, IFC (IFC2x3, IFC4 and IFC4x3 import, IFC4 export), glTF, GLB, OBJ, and PDF, SVG and PNG of sheets. import.update reads a new revision of an imported DXF or DWG drawing and lists what changed (added, removed, changed, conflicts with the edits of the person) without changing the document; import.apply then applies the accepted changes as one undo step with a version before and after (describe.io.update).",
        prefixes: &["import", "export"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "gis",
        summary: "Coordinate systems, map services (WMS, WMTS, WFS), geo images and the tile cache. Uses the network for allowed addresses only.",
        prefixes: &["gis"],
        read_only: false,
        open_world: true,
    },
    Domain {
        name: "scripting",
        summary: "JavaScript programs that run inside the session (script.eval: the program calls the methods of this API, the whole run is one undo step, mode read for checks and reports; it can reach files and the network as far as the methods it calls can), Python scripts, Python plug-in objects (plugin.*) and sandboxed WebAssembly plug-ins (plugins.*).",
        prefixes: &["script", "plugin", "plugins"],
        read_only: false,
        open_world: true,
    },
];

/// The part of a method name before the first dot.
fn prefix_of(method: &str) -> &str {
    method.split('.').next().unwrap_or(method)
}

/// Methods that belong to another domain than their prefix says. The methods with which an agent
/// reads and drives what the person sees (AGR-02, AGR-03) are part of the review: a person
/// reviews what the agent does, and the agent points at it.
const METHOD_DOMAIN: &[(&str, &str)] = &[
    ("view.get", "review"),
    ("view.zoom", "review"),
    ("view.pan", "review"),
    ("view.camera", "review"),
    ("view.highlight", "review"),
    ("view.highlight_clear", "review"),
];

/// The domain of a method.
pub fn domain_of(method: &str) -> Option<&'static Domain> {
    if let Some((_, name)) = METHOD_DOMAIN.iter().find(|(m, _)| *m == method) {
        return DOMAINS.iter().find(|d| d.name == *name);
    }
    let p = prefix_of(method);
    DOMAINS.iter().find(|d| d.prefixes.contains(&p))
}

/// Methods that need a message stream. MCP tools return one result, so they are not offered.
const STREAM_METHODS: &[&str] = &["events.subscribe", "events.unsubscribe"];

/// Every method that the server offers as a tool: the API methods, except the ones that need a
/// message stream and the feedback methods (the `feedback` tool covers them in both modes).
pub fn all_methods() -> impl Iterator<Item = (&'static str, &'static str)> {
    METHODS
        .iter()
        .copied()
        .filter(|(m, _)| !STREAM_METHODS.contains(m) && !m.starts_with("feedback."))
}

/// The methods of one domain with their descriptions.
fn methods_of(domain: &Domain) -> Vec<(&'static str, &'static str)> {
    all_methods()
        .filter(|(m, _)| domain_of(m).is_some_and(|d| d.name == domain.name))
        .collect()
}

/// The description of a domain tool: what it covers, how to call it, and the methods.
fn description(domain: &Domain, methods: &[(&str, &str)]) -> String {
    let mut text = format!(
        "{} Call with {{method, params}}; params are the params of the method.\n\nMethods:",
        domain.summary
    );
    for (m, d) in methods {
        text.push_str(&format!("\n- {m}: {d}"));
    }
    text
}

/// The JSON Schema of the params of a method, from its parameter struct in the API: the same
/// struct that makes the API refuse a parameter it does not take. A method without an entry takes
/// any object. `compact` leaves out the descriptions (the domain tools list the schemas of all
/// their methods, and the text of the methods is in their description).
pub fn method_schema(method: &str, compact: bool) -> Value {
    let Some(mut schema) = kadakas_api::params::schema(method) else {
        return json!({"type": "object", "additionalProperties": true});
    };
    if let Some(o) = schema.as_object_mut() {
        o.remove("$schema");
    }
    if compact {
        strip_descriptions(&mut schema);
    }
    schema
}

/// Removes the `description` of every schema node. The names of the properties are not schema
/// keywords (a parameter may be called `description`), and the values of `default`, `enum` and
/// `const` are data.
fn strip_descriptions(v: &mut Value) {
    match v {
        Value::Object(m) => {
            if m.get("description").is_some_and(Value::is_string) {
                m.remove("description");
            }
            for (key, child) in m.iter_mut() {
                match key.as_str() {
                    "default" | "enum" | "const" => {}
                    "properties" => {
                        if let Value::Object(props) = child {
                            props.values_mut().for_each(strip_descriptions);
                        }
                    }
                    _ => strip_descriptions(child),
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(strip_descriptions),
        _ => {}
    }
}

/// One tool per domain. With `schemas` the tool lists the JSON Schema of the params of each of
/// its methods (about 100 KB more in all); without, `describe {method}` gives one on request.
pub fn domain_tools(schemas: bool) -> Vec<Value> {
    DOMAINS
        .iter()
        .map(|d| {
            let methods = methods_of(d);
            let names: Vec<&str> = methods.iter().map(|(m, _)| *m).collect();
            // The schema of the params depends on the method: one `if` for each.
            let by_method: Vec<Value> = names
                .iter()
                .filter(|_| schemas)
                .map(|m| {
                    json!({
                        "if": {"properties": {"method": {"const": m}}, "required": ["method"]},
                        "then": {"properties": {"params": method_schema(m, true)}},
                    })
                })
                .collect();
            let mut annotations = json!({"readOnlyHint": d.read_only});
            if !d.read_only {
                annotations["destructiveHint"] = json!(true);
            }
            if d.open_world {
                annotations["openWorldHint"] = json!(true);
            }
            let mut input = json!({
                "type": "object",
                "properties": {
                    "method": {"type": "string", "enum": names, "description": "The method to call."},
                    "params": {"type": "object", "additionalProperties": true, "description": "The params of the method. A name the method does not take is an error that names the nearest known one; describe {method} gives its schema."}
                },
                "required": ["method"],
            });
            if schemas {
                input["allOf"] = json!(by_method);
            }
            if d.name == "describe" {
                // `describe {}` is enough: no method is needed.
                input = json!({
                    "type": "object",
                    "properties": {
                        "method": {"type": "string", "description": "Optional. A method name, for example hardscape.course: gives that method with the JSON Schema of its params. Leave it out for the list of all methods."},
                        "docs": {"type": "boolean", "description": "true: the full doc of every method in the list (about 140 KB more); by default the list has the first sentence, and the full text is in the description of each tool."},
                        "schemas": {"type": "boolean", "description": "true: add the JSON Schema of the params of every method (large)."},
                        "schema": {"type": "boolean", "description": "true: add the JSON Schema of the commands of execute (about 30 KB)."},
                        "params": {"type": "object", "additionalProperties": true, "description": "Only for the long form {method: \"describe\", params: {...}}."}
                    },
                });
            }
            json!({
                "name": d.name,
                "description": description(d, &methods),
                "inputSchema": input,
                "annotations": annotations,
            })
        })
        .collect()
}

/// Maps API method names to flat tool names: `objects.list` -> `objects_list`.
pub fn flat_name(method: &str) -> String {
    method.replace('.', "_")
}

/// One tool per method (the old listing, `--mcp-flat`).
pub fn flat_tools() -> Vec<Value> {
    METHODS
        .iter()
        .filter(|(m, _)| !STREAM_METHODS.contains(m) && !m.starts_with("feedback."))
        .map(|(m, d)| json!({"name": flat_name(m), "description": d, "inputSchema": method_schema(m, false)}))
        .collect()
}

/// The `feedback` tool (API-47). It is in both modes, right after `describe`, so an agent sees it
/// before it has a reason to give up on a task.
pub fn feedback_tool() -> Value {
    json!({
        "name": "feedback",
        "description": "Tell the Kadakas team what is missing or broken. You must call this with kind feature_request when a task needs a feature that Kadakas lacks, and with kind bug when a method fails in a way it must not. Do not work around it silently. Give a short title, and in body what you wanted and what happened. A bug report attaches the failing objects, a picture of the view and the recent calls by default (attach: {excerpt?, screenshot?, calls?, log?: false} removes one; attach: {objects: [id]} or {selection: true} names objects). Attachments stay on this computer until a human approves them (status needs_review); you cannot send them. Texts are scrubbed of paths, tokens and e-mail addresses. Equal reports merge into a counter. action: submit (default), list, get (needs id) or withdraw (needs id).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["submit", "list", "get", "withdraw"], "description": "Default: submit."},
                "kind": {"type": "string", "enum": ["bug", "feature_request", "question"], "description": "submit: bug, feature_request or question."},
                "title": {"type": "string", "maxLength": 120, "description": "submit: one short line."},
                "body": {"type": "string", "maxLength": 4000, "description": "submit: what happened, or what you need."},
                "wanted": {"type": "string", "maxLength": 2000, "description": "submit: what you wanted to do."},
                "tried": {"type": "string", "maxLength": 2000, "description": "submit: what you did."},
                "method": {"type": "string", "description": "submit: the API method. A bug report takes the last failed call if you leave it out."},
                "error": {"type": "string", "description": "submit: the error text."},
                "severity": {"type": "string", "enum": ["low", "medium", "high"]},
                "attach": {"type": "object", "additionalProperties": true, "description": "submit: what to attach. Fields excerpt, screenshot, calls, log (false removes), objects (ids), selection (bool), view (render.png params)."},
                "id": {"type": "string", "description": "get, withdraw: the report id, for example fb-1."}
            }
        },
        "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false},
    })
}

/// The tool listing for `tools/list`.
pub fn tools(flat: bool, schemas: bool) -> Value {
    let mut list = if flat {
        flat_tools()
    } else {
        domain_tools(schemas)
    };
    list.insert(1.min(list.len()), feedback_tool());
    json!(list)
}

/// What a `tools/call` asks for.
#[derive(Debug, PartialEq)]
pub enum Call {
    /// No tool of this name.
    UnknownTool,
    /// The tool exists, but the call is wrong. The text says why.
    Invalid(String),
    /// Run this method with these params.
    Method(&'static str, Value),
}

/// The `feedback` tool: `action` picks the method (default: submit); the other arguments are its
/// params.
fn resolve_feedback(args: &Value) -> Call {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("submit");
    let method = match action {
        "submit" => "feedback.submit",
        "list" => "feedback.list",
        "get" => "feedback.get",
        "withdraw" => "feedback.withdraw",
        other => {
            return Call::Invalid(format!(
                "feedback has no action '{other}'. Actions: submit, list, get, withdraw"
            ));
        }
    };
    let mut params = args.clone();
    if let Some(o) = params.as_object_mut() {
        o.remove("action");
    }
    Call::Method(method, params)
}

/// Decides which API method a tool call runs.
pub fn resolve(flat: bool, name: &str, args: &Value) -> Call {
    if name == "feedback" {
        return resolve_feedback(args);
    }
    if flat {
        return match all_methods().map(|(m, _)| m).find(|m| flat_name(m) == name) {
            Some(m) => Call::Method(m, args.clone()),
            None => Call::UnknownTool,
        };
    }
    let Some(domain) = DOMAINS.iter().find(|d| d.name == name) else {
        return Call::UnknownTool;
    };
    let methods = methods_of(domain);
    if domain.name == "describe" {
        return resolve_describe(args);
    }
    let Some(wanted) = args.get("method").and_then(Value::as_str) else {
        return Call::Invalid(format!(
            "{name} needs params.method. Methods: {}",
            list(&methods)
        ));
    };
    match methods.iter().find(|(m, _)| *m == wanted) {
        Some((m, _)) => Call::Method(m, args.get("params").cloned().unwrap_or(Value::Null)),
        None => match all_methods().find(|(m, _)| *m == wanted) {
            // The method exists in another tool: say which.
            Some((m, _)) => Call::Invalid(format!(
                "{m} is a method of the tool '{t}', not of '{name}'. Call the tool '{t}' with {{\"method\": \"{m}\", \"params\": {{...}}}}.",
                t = tool_of(m)
            )),
            None => Call::Invalid(format!(
                "{name} has no method '{wanted}'. Methods: {}",
                list(&methods)
            )),
        },
    }
}

/// The `describe` tool works with no method: `describe {}` lists the methods, `describe
/// {schemas: true}` adds the schemas, and `describe {method: "hardscape.course"}` gives one method
/// with its parameter schema. The long form `describe {method: "describe", params: {...}}` works
/// too.
fn resolve_describe(args: &Value) -> Call {
    match args.get("method").and_then(Value::as_str) {
        Some("describe") => Call::Method(
            "describe",
            args.get("params").cloned().unwrap_or(Value::Null),
        ),
        Some(other) => {
            if all_methods().any(|(m, _)| m == other) {
                let mut params = args
                    .get("params")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                params.insert("method".into(), json!(other));
                Call::Method("describe", Value::Object(params))
            } else {
                Call::Invalid(format!(
                    "'{other}' is no method. Call describe with {{}} for the list of methods, with {{schemas: true}} for the list with the parameter schemas, or with {{method: <a method name>}} for one method."
                ))
            }
        }
        None => {
            // No method: the other arguments are the params of describe.
            let mut params = args.as_object().cloned().unwrap_or_default();
            if let Some(Value::Object(inner)) = params.remove("params") {
                params.extend(inner);
            }
            Call::Method(
                "describe",
                if params.is_empty() {
                    Value::Null
                } else {
                    Value::Object(params)
                },
            )
        }
    }
}

/// Names the MCP tool of every method in the result of `describe`: `tool` on each entry of the
/// method list, and on the one method that `describe {method}` returns. In domain mode it is the
/// domain tool (the call is `{method, params}` to that tool), in flat mode the tool named like the
/// method.
pub fn name_tools(flat: bool, docs: bool, result: &mut Value) {
    let tool = |m: &str| -> String {
        if flat {
            flat_name(m)
        } else {
            tool_of(m).to_string()
        }
    };
    if let Some(list) = result.get_mut("methods").and_then(Value::as_array_mut) {
        for entry in list {
            if let Some(name) = entry["name"].as_str().map(str::to_string) {
                entry["tool"] = json!(tool(&name));
            }
            // The full text is in the description of the tool and in `describe {method}`, so the
            // list carries the first sentence only (about 140 KB less).
            if !docs && let Some(doc) = entry["doc"].as_str().map(summary) {
                entry["doc"] = json!(doc);
            }
        }
    }
    if result.get("methods").is_none()
        && let Some(name) = result["method"].as_str().map(str::to_string)
    {
        result["tool"] = json!(tool(&name));
    }
    if result.get("methods").is_some() {
        result["tools"] = json!(if flat {
            "Each method is a tool of its own: the tool name is the method name with the dots as underscores (objects.list -> objects_list). Every method in `methods` has its `tool`."
        } else {
            "Each tool is a domain and takes {method, params}. Every method in `methods` has its `tool`: call that tool, for example the tool hardscape_landscape with {method: \"hardscape.course\", params: {...}}. A call to another tool is an error that names the right one."
        });
        if !docs {
            result["docs"] = json!(
                "The doc of each method here is its first sentence. The full text with the params is in the description of its tool, in describe {method: <name>} (with the JSON Schema of the params), or here with describe {docs: true}."
            );
        }
    }
}

/// The first sentence of a method doc (up to its `params:` list), at most 200 characters.
fn summary(doc: &str) -> String {
    let cut = [doc.find(". "), doc.find(" params:")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(doc.len());
    let first = doc[..cut].trim_end_matches('.');
    if first.chars().count() > 200 {
        let mut t: String = first.chars().take(200).collect();
        t.push_str("...");
        t
    } else {
        format!("{first}.")
    }
}

/// The name of the tool that holds a method, in domain mode.
pub fn tool_of(method: &str) -> &'static str {
    domain_of(method).map_or("describe", |d| d.name)
}

fn list(methods: &[(&str, &str)]) -> String {
    methods
        .iter()
        .map(|(m, _)| *m)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_belongs_to_exactly_one_domain() {
        for (m, _) in all_methods() {
            let owners: Vec<&str> = DOMAINS
                .iter()
                .filter(|d| d.prefixes.contains(&prefix_of(m)))
                .map(|d| d.name)
                .collect();
            let moved = METHOD_DOMAIN.iter().any(|(name, _)| *name == m);
            assert_eq!(
                owners.len(),
                1,
                "{m} is in {owners:?}: add its prefix to one domain"
            );
            assert!(domain_of(m).is_some(), "{m} has a domain");
            if !moved {
                assert_eq!(domain_of(m).map(|d| d.name), Some(owners[0]));
            }
        }
        // No prefix is dead: each one names a method.
        for d in DOMAINS {
            for p in d.prefixes {
                assert!(
                    all_methods().any(|(m, _)| prefix_of(m) == *p),
                    "{} lists the prefix {p}, which no method has",
                    d.name
                );
            }
        }
    }

    #[test]
    fn the_domain_listing_is_short_and_complete() {
        let tools = domain_tools(true);
        assert!((15..=25).contains(&tools.len()), "{}", tools.len());
        // The describe tool takes any method name (it needs none), so it has no list.
        let listed = |t: &Value| -> Vec<String> {
            t["inputSchema"]["properties"]["method"]["enum"]
                .as_array()
                .map(|a| a.iter().map(|m| m.as_str().unwrap().to_string()).collect())
                .unwrap_or_else(|| vec!["describe".to_string()])
        };
        let total: usize = tools.iter().map(|t| listed(t).len()).sum();
        assert_eq!(total, all_methods().count());
        for t in &tools {
            let text = t["description"].as_str().unwrap();
            for m in listed(t) {
                assert!(text.contains(&format!("- {m}:")));
            }
        }
    }

    #[test]
    fn read_only_domains_hold_only_reads() {
        let tools = domain_tools(true);
        let by = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap();
        assert_eq!(by("describe")["annotations"], json!({"readOnlyHint": true}));
        assert_eq!(by("objects")["annotations"]["readOnlyHint"], true);
        assert_eq!(by("reports")["annotations"]["readOnlyHint"], true);
        assert_eq!(by("plants")["annotations"]["readOnlyHint"], false);
        assert_eq!(by("plants")["annotations"]["destructiveHint"], true);
        assert_eq!(by("gis")["annotations"]["openWorldHint"], true);
        // The read-only domains have no method that writes.
        for d in DOMAINS.iter().filter(|d| d.read_only) {
            for (m, _) in methods_of(d) {
                assert!(
                    !["add", "set", "create", "delete", "update", "load", "save"]
                        .iter()
                        .any(|w| m.ends_with(w)),
                    "{m}"
                );
            }
        }
    }

    #[test]
    fn the_feedback_tool_is_in_both_modes_and_resolves() {
        for flat in [false, true] {
            let list = tools(flat, false);
            let t = &list.as_array().unwrap()[1];
            assert_eq!(t["name"], "feedback", "flat={flat}");
            let d = t["description"].as_str().unwrap();
            assert!(d.contains("feature_request") && d.contains("bug"));
            assert!(
                t["inputSchema"]["properties"]["kind"]["enum"]
                    .as_array()
                    .unwrap()
                    .len()
                    == 3
            );
            // Only one tool of that name, and the methods are not tools of their own.
            let names: Vec<&str> = list
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap())
                .collect();
            assert_eq!(names.iter().filter(|n| **n == "feedback").count(), 1);
            assert!(!names.contains(&"feedback_submit"));
        }
        // The domain listing has no domain for the feedback methods.
        assert!(domain_tools(true).iter().all(|t| t["name"] != "feedback"));
        let args = json!({"kind": "bug", "title": "t"});
        for flat in [false, true] {
            assert_eq!(
                resolve(flat, "feedback", &args),
                Call::Method("feedback.submit", args.clone())
            );
        }
        assert_eq!(
            resolve(false, "feedback", &json!({"action": "get", "id": "fb-1"})),
            Call::Method("feedback.get", json!({"id": "fb-1"}))
        );
        assert_eq!(
            resolve(true, "feedback", &json!({"action": "list"})),
            Call::Method("feedback.list", json!({}))
        );
        assert_eq!(
            resolve(false, "feedback", &json!({"action": "withdraw", "id": "x"})),
            Call::Method("feedback.withdraw", json!({"id": "x"}))
        );
        assert!(matches!(
            resolve(false, "feedback", &json!({"action": "approve"})),
            Call::Invalid(t) if t.contains("no action")
        ));
        assert_eq!(
            resolve(true, "feedback_submit", &json!({})),
            Call::UnknownTool
        );
    }

    #[test]
    fn calls_resolve_to_methods() {
        assert_eq!(
            resolve(
                false,
                "objects",
                &json!({"method": "objects.list", "params": {"kind": "plant"}})
            ),
            Call::Method("objects.list", json!({"kind": "plant"}))
        );
        assert_eq!(
            resolve(false, "document", &json!({"method": "execute"})),
            Call::Method("execute", Value::Null)
        );
        assert_eq!(
            resolve(
                false,
                "document",
                &json!({"method": "file.save", "params": {"path": "a"}})
            ),
            Call::Method("file.save", json!({"path": "a"}))
        );
        assert_eq!(resolve(false, "execute", &json!({})), Call::UnknownTool);
        assert!(
            matches!(resolve(false, "objects", &json!({})), Call::Invalid(t) if t.contains("needs params.method"))
        );
        assert!(matches!(
            resolve(false, "objects", &json!({"method": "plants.place"})),
            Call::Invalid(t) if t.contains("method of the tool 'plants'")
        ));
        assert_eq!(
            resolve(true, "objects_list", &json!({"kind": "x"})),
            Call::Method("objects.list", json!({"kind": "x"}))
        );
        assert_eq!(resolve(true, "objects", &json!({})), Call::UnknownTool);
        assert_eq!(flat_tools().len(), all_methods().count());
    }

    #[test]
    fn the_tool_schemas_come_from_the_parameter_structs() {
        // A flat tool: the schema of the method, with required names, values and descriptions.
        let flat = flat_tools();
        let place = flat.iter().find(|t| t["name"] == "plants_place").unwrap();
        let schema = &place["inputSchema"];
        assert_eq!(schema["required"], json!(["mode"]));
        assert_eq!(schema["additionalProperties"], false);
        assert!(
            schema["properties"]["mode"]["enum"]
                .as_array()
                .unwrap()
                .contains(&json!("grid"))
        );
        assert_eq!(schema["properties"]["layer"]["x-kadakas-ref"], "layer");
        assert!(schema.get("$schema").is_none());
        // Every flat tool has a schema of an object.
        assert!(flat.iter().all(|t| t["inputSchema"]["type"] == "object"));
        // A domain tool: one `if` for each method, without the descriptions.
        let plain = domain_tools(false);
        let plants = plain.iter().find(|t| t["name"] == "plants").unwrap();
        assert!(plants["inputSchema"].get("allOf").is_none());
        let domains = domain_tools(true);
        let plants = domains.iter().find(|t| t["name"] == "plants").unwrap();
        let all = plants["inputSchema"]["allOf"].as_array().unwrap();
        let one = all
            .iter()
            .find(|c| c["if"]["properties"]["method"]["const"] == "plants.place")
            .unwrap();
        let params = &one["then"]["properties"]["params"];
        assert_eq!(params["required"], json!(["mode"]));
        assert!(params["properties"]["layer"].get("description").is_none());
        assert!(
            plants["inputSchema"]["properties"]["params"]["description"]
                .as_str()
                .unwrap()
                .contains("nearest")
        );
        // A method without an entry takes any object.
        assert_eq!(
            method_schema("no.such", false)["additionalProperties"],
            true
        );
    }

    #[test]
    fn stripping_keeps_a_property_that_is_called_description() {
        let mut v = json!({
            "type": "object",
            "description": "gone",
            "properties": {
                "description": {"type": "string", "description": "gone too"},
                "title": {"type": "string", "default": {"description": "data"}}
            },
            "required": ["description"]
        });
        strip_descriptions(&mut v);
        assert!(v.get("description").is_none());
        assert_eq!(v["properties"]["description"], json!({"type": "string"}));
        assert_eq!(v["properties"]["title"]["default"]["description"], "data");
        let s = method_schema("issue.create", true);
        assert!(s["properties"].get("description").is_some(), "{s}");
    }

    #[test]
    fn the_method_schemas_are_opt_in_in_domain_mode() {
        let size = |flat, schemas| tools(flat, schemas).to_string().len();
        assert!(size(false, false) < size(false, true));
        assert!(size(false, false) * 3 < size(true, false) * 2);
        for t in domain_tools(false) {
            assert!(t["inputSchema"].get("allOf").is_none());
        }
    }

    #[test]
    fn a_wrong_tool_names_the_right_one_and_describe_needs_no_method() {
        let Call::Invalid(t) = resolve(
            false,
            "hardscape_landscape",
            &json!({"method": "view.zoom", "params": {}}),
        ) else {
            panic!("a method of another tool is refused");
        };
        assert!(
            t.contains("tool 'review'") && t.contains("view.zoom"),
            "{t}"
        );
        let Call::Invalid(t) = resolve(false, "objects", &json!({"method": "no.such"})) else {
            panic!();
        };
        assert!(t.contains("has no method"), "{t}");
        // describe without method, with schemas, and with a method name.
        assert_eq!(
            resolve(false, "describe", &json!({})),
            Call::Method("describe", Value::Null)
        );
        assert_eq!(
            resolve(false, "describe", &json!({"schemas": true})),
            Call::Method("describe", json!({"schemas": true}))
        );
        assert_eq!(
            resolve(false, "describe", &json!({"method": "describe"})),
            Call::Method("describe", Value::Null)
        );
        assert_eq!(
            resolve(false, "describe", &json!({"method": "hardscape.course"})),
            Call::Method("describe", json!({"method": "hardscape.course"}))
        );
        assert!(matches!(
            resolve(false, "describe", &json!({"method": "nonsense"})),
            Call::Invalid(_)
        ));
        // The tool does not require a method.
        let tools = domain_tools(false);
        let d = tools.iter().find(|t| t["name"] == "describe").unwrap();
        assert!(d["inputSchema"].get("required").is_none());
        // The list names the tool of each method.
        let mut result = json!({"methods": [{"name": "view.zoom", "doc": "x"}, {"name": "objects.list", "doc": "y"}]});
        name_tools(false, false, &mut result);
        assert_eq!(result["methods"][0]["tool"], "review");
        assert_eq!(result["methods"][1]["tool"], "objects");
        assert!(
            result["tools"]
                .as_str()
                .unwrap()
                .contains("names the right one")
        );
        assert_eq!(result["methods"][0]["doc"], "x.");
        let long = "Lay units over a shape. params: {id, pattern?}. Returns {x}.";
        assert_eq!(summary(long), "Lay units over a shape.");
        let mut full = json!({"methods": [{"name": "view.zoom", "doc": long}]});
        name_tools(false, true, &mut full);
        assert_eq!(full["methods"][0]["doc"], long);
        assert!(full.get("docs").is_none());
        let mut one = json!({"method": "view.zoom", "doc": "x", "params": {}});
        name_tools(true, true, &mut one);
        assert_eq!(one["tool"], "view_zoom");
    }
}
