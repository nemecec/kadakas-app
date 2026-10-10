//! The tool listing of the MCP server.
//!
//! The default listing has one tool per domain (about twenty). A domain tool takes
//! `{method, params}` and its description lists the methods of the domain, taken from `METHODS`.
//! With `--mcp-flat` the server lists one tool per API method instead (about 280).

use kadakas_api::METHODS;
use serde_json::{Value, json};

/// Methods that the CLI itself answers (the document files of a headless session).
pub const FILE_METHODS: &[(&str, &str)] = &[
    ("file.save", "Save the document. params: {path}."),
    ("file.open", "Open a document. params: {path}."),
];

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
        summary: "The document and its history of changes. `execute` runs one command (add, modify, move or delete objects, and change layers, classes and settings: layers and classes have no methods of their own, use their commands). Also the project data, saved views of layer visibility, the document files and the change events (`events.poll`; MCP has no pushed messages).",
        prefixes: &[
            "execute",
            "document",
            "project",
            "saved_view",
            "file",
            "visibility",
            "events",
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
        summary: "Pictures of the drawing (SVG, PNG, hidden-line), the 3D view, and display rules: the northing/easting grid, project phases and data visualizations (color by a property).",
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
        summary: "Site models (terrain), contours, volumes, source data checks, survey point import, pads with retaining edges, the survey point of the georeference, and massing models (buildings).",
        prefixes: &["site", "massing", "pad", "survey", "georef"],
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
        summary: "Hardscape objects and styles, landscape areas and their styles.",
        prefixes: &[
            "hardscape",
            "hardscape_style",
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
        summary: "Resources (the Resource Manager and library files), symbols, groups, and hatch patterns.",
        prefixes: &["resource", "symbol", "group", "pattern", "library"],
        read_only: false,
        open_world: false,
    },
    Domain {
        name: "io",
        summary: "Import and export files: DXF, DWG (needs a converter), Shapefile, GeoJSON, IFC (IFC2x3, IFC4 and IFC4x3 import, IFC4 export), glTF, GLB, OBJ, and PDF, SVG and PNG of sheets.",
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
        summary: "Python scripts, Python plug-in objects (plugin.*) and sandboxed WebAssembly plug-ins (plugins.*).",
        prefixes: &["script", "plugin", "plugins"],
        read_only: false,
        open_world: false,
    },
];

/// The part of a method name before the first dot.
fn prefix_of(method: &str) -> &str {
    method.split('.').next().unwrap_or(method)
}

/// The domain of a method.
pub fn domain_of(method: &str) -> Option<&'static Domain> {
    let p = prefix_of(method);
    DOMAINS.iter().find(|d| d.prefixes.contains(&p))
}

/// Methods that need a message stream. MCP tools return one result, so they are not offered.
const STREAM_METHODS: &[&str] = &["events.subscribe", "events.unsubscribe"];

/// Every method that the server offers as a tool: the API methods and the file methods of the
/// CLI, except the ones that need a message stream.
pub fn all_methods() -> impl Iterator<Item = (&'static str, &'static str)> {
    METHODS
        .iter()
        .chain(FILE_METHODS.iter())
        .copied()
        .filter(|(m, _)| !STREAM_METHODS.contains(m))
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

/// One tool per domain.
pub fn domain_tools() -> Vec<Value> {
    DOMAINS
        .iter()
        .map(|d| {
            let methods = methods_of(d);
            let names: Vec<&str> = methods.iter().map(|(m, _)| *m).collect();
            let mut annotations = json!({"readOnlyHint": d.read_only});
            if !d.read_only {
                annotations["destructiveHint"] = json!(true);
            }
            if d.open_world {
                annotations["openWorldHint"] = json!(true);
            }
            json!({
                "name": d.name,
                "description": description(d, &methods),
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "method": {"type": "string", "enum": names, "description": "The method to call."},
                        "params": {"type": "object", "additionalProperties": true, "description": "The params of the method."}
                    },
                    "required": ["method"]
                },
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
    let mut tools: Vec<Value> = METHODS
        .iter()
        .filter(|(m, _)| !STREAM_METHODS.contains(m))
        .map(|(m, d)| json!({"name": flat_name(m), "description": d, "inputSchema": {"type": "object", "additionalProperties": true}}))
        .collect();
    for (m, d) in FILE_METHODS {
        tools.push(json!({"name": flat_name(m), "description": d, "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}}));
    }
    tools
}

/// The tool listing for `tools/list`.
pub fn tools(flat: bool) -> Value {
    json!(if flat { flat_tools() } else { domain_tools() })
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

/// Decides which API method a tool call runs.
pub fn resolve(flat: bool, name: &str, args: &Value) -> Call {
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
    let Some(wanted) = args.get("method").and_then(Value::as_str) else {
        return Call::Invalid(format!(
            "{name} needs params.method. Methods: {}",
            list(&methods)
        ));
    };
    match methods.iter().find(|(m, _)| *m == wanted) {
        Some((m, _)) => Call::Method(m, args.get("params").cloned().unwrap_or(Value::Null)),
        None => Call::Invalid(format!(
            "{name} has no method '{wanted}'. Methods: {}",
            list(&methods)
        )),
    }
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
            assert_eq!(
                owners.len(),
                1,
                "{m} is in {owners:?}: add its prefix to one domain"
            );
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
        let tools = domain_tools();
        assert!((15..=25).contains(&tools.len()), "{}", tools.len());
        let total: usize = tools
            .iter()
            .map(|t| {
                t["inputSchema"]["properties"]["method"]["enum"]
                    .as_array()
                    .unwrap()
                    .len()
            })
            .sum();
        assert_eq!(total, all_methods().count());
        for t in &tools {
            let text = t["description"].as_str().unwrap();
            for m in t["inputSchema"]["properties"]["method"]["enum"]
                .as_array()
                .unwrap()
            {
                assert!(text.contains(&format!("- {}:", m.as_str().unwrap())));
            }
        }
    }

    #[test]
    fn read_only_domains_hold_only_reads() {
        let tools = domain_tools();
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
            Call::Invalid(t) if t.contains("has no method 'plants.place'")
        ));
        assert_eq!(
            resolve(true, "objects_list", &json!({"kind": "x"})),
            Call::Method("objects.list", json!({"kind": "x"}))
        );
        assert_eq!(resolve(true, "objects", &json!({})), Call::UnknownTool);
        assert_eq!(flat_tools().len(), all_methods().count());
    }
}
