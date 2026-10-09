//! Agent API for drawing documentation: sheet layers, viewports, overrides, annotations,
//! title blocks, project data, saved views, and sheet output (PDF, SVG, PNG).
//!
//! Methods: `sheet.list|create|set|render`, `project.get|set`, `viewport.create|get|list|update|set_override|annotate`,
//! `title_block.create|update|resolve`, `saved_view.list|save|restore|delete`. Sheets are written by `export.file` with the format `pdf`, `sheet_svg` or `sheet_png`.
//! Viewports and title blocks are objects of kind `viewport` and `title_block` on a sheet layer,
//! so `execute` with `add_object`, `move_objects` and `delete_objects` works on them too.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Value};

use cadektor_core::command::Command;
use cadektor_core::model::*;
use cadektor_core::sheets::*;
use cadektor_core::DVec2;
use cadektor_render::{Projection, RenderMode, StandardView};

use crate::{RpcError, Session};

pub const SHEET_LIST: (&str, &str) = ("sheet.list", "Sheet layers with page size, number, title, and the IDs of their viewports and title blocks. Returns {sheets: [{layer, name, number, title, paper, orientation, width_mm, height_mm, viewports, title_blocks}]}.");
pub const SHEET_CREATE: (&str, &str) = ("sheet.create", "Add a sheet layer (paper space, mm, scale 1:1). params: {name, paper? (a0 a1 a2 a3 a4 letter legal tabloid arch_c arch_d arch_e, or {width_mm, height_mm}; default a3), orientation? (landscape | portrait), number?, title?}. Returns {layer}.");
pub const SHEET_SET: (&str, &str) = ("sheet.set", "Change the page of a sheet layer. params: {layer (ID or name), paper?, orientation?, number?, title?}.");
pub const SHEET_RENDER: (&str, &str) = ("sheet.render", "Draw a sheet. params: {sheet (layer ID or name), format? (svg | png; default svg), dpi? (png, default 100)}. Returns {svg} or {png_base64, width, height}. Viewports are drawn live from the model.");
pub const PROJECT_GET: (&str, &str) = ("project.get", "Project data for title blocks: name, client, address, drawn_by, date, revision.");
pub const PROJECT_SET: (&str, &str) = ("project.set", "Change project data. params: any of name, client, address, drawn_by, date, revision.");
pub const VIEWPORT_CREATE: (&str, &str) = ("viewport.create", "Add a viewport to a sheet layer. params: {sheet, layers? (design layer IDs or names; default all), min+max or at+size (frame on the sheet, mm; default the page inside the margins), scale? (denominator such as 100, or \"fit\" for the best standard scale; default fit), center? (model point at the middle of the frame; default the middle of the content), view? (plan | a 3D view from view.list; default plan), mode? (shaded | wireframe, 3D only), dpi? (3D only, default 150), crop? ({type: rect, min, max} | {type: polygon, points} in paper mm), show_frame?, name?}. 3D views are raster images. Returns the viewport with its id.");
pub const VIEWPORT_GET: (&str, &str) = ("viewport.get", "One viewport with all settings, overrides and annotations. params: {id}.");
pub const VIEWPORT_LIST: (&str, &str) = ("viewport.list", "Viewports. params: {sheet?}. Returns {viewports: [{id, sheet, name, scale, view, layers, min, max}]}.");
pub const VIEWPORT_UPDATE: (&str, &str) = ("viewport.update", "Change a viewport. params: {id, and any of name, min, max, center, scale (number or \"fit\"), layers, view, mode, dpi, crop (null removes it), show_frame, fit (true recomputes center and scale)}. Returns the viewport.");
pub const VIEWPORT_SET_OVERRIDE: (&str, &str) = ("viewport.set_override", "Visibility and graphic overrides in one viewport. params: {id, layers? ({layer: true | false | null}; true or false replaces the document visibility, null removes the override), classes? ({class: {visible?, pen?, fill?, line_weight?} | null}; a field set to null is removed; pen and fill are [r,g,b,a]; line_weight is mm on paper), reset? (true clears all overrides first)}. Precedence: the object's own attributes, then this override (only for attributes the object leaves to its class), then the class, then defaults. A viewport keeps its own visibility; it does not follow the document. Layer and class keys are IDs or names.");
pub const VIEWPORT_ANNOTATE: (&str, &str) = ("viewport.annotate", "Annotations of a viewport: objects in paper mm, measured from the lower left corner of the frame (they move with the viewport). params: {id, add? ([{kind: line, a, b} | {kind: text, at, text, height} | {kind: dimension, a, b, offset, text?} | {kind: shape, shape: {kind: rect | circle | arc | polyline | line | text, ...}}, each with optional class and attrs]), remove? ([annotation id]), clear? (bool)}. Returns {added: [annotation id], count}.");
pub const TITLE_BLOCK_CREATE: (&str, &str) = ("title_block.create", "Add a border, grid reference and title block to a sheet layer. It follows the page size. params: {sheet, margin? (mm, default 10), grid_columns? (0 to 26), grid_rows? (0 to 99), layout? (omit for the standard layout; or {width, height, cells: [{x, y, w, h, label, text, size}]} in mm from the lower left corner of the block), fields? ({name: text}, for {field.name})}. Cell text may use {project.name}, {project.client}, {project.address}, {project.drawn_by}, {project.date}, {project.revision}, {sheet.number}, {sheet.title}, {sheet.scale}, {sheet.paper}, {sheet.index}, {sheet.count} and {field.<name>}. Returns {id}.");
pub const TITLE_BLOCK_UPDATE: (&str, &str) = ("title_block.update", "Change a title block. params: {id, and any of margin, grid_columns, grid_rows, layout (null for the standard layout), fields}.");
pub const TITLE_BLOCK_RESOLVE: (&str, &str) = ("title_block.resolve", "The title block as drawn: border, grid labels and cells with their filled text. params: {id}.");
pub const SAVED_VIEW_LIST: (&str, &str) = ("saved_view.list", "Saved views. Returns {views: [{name, layers, classes, active_layer, active_class, zoom, pan, view, projection, mode}]}.");
pub const SAVED_VIEW_SAVE: (&str, &str) = ("saved_view.save", "Save the current view: the visibility of all layers and classes, the active layer and class, and the 3D view settings. params: {name, zoom? (px per mm), pan? ([x, y] model point at the middle)}. A view with the same name is replaced.");
pub const SAVED_VIEW_RESTORE: (&str, &str) = ("saved_view.restore", "Apply a saved view (one undo step): layer and class visibility, active layer and class, and the 3D view settings. params: {name}. Returns the view, including zoom and pan for the host to apply.");
pub const SAVED_VIEW_DELETE: (&str, &str) = ("saved_view.delete", "Delete a saved view. params: {name}.");

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "concepts": "A sheet layer is a page (paper space: mm, scale 1:1). A viewport is an object on a sheet layer that shows design layers at a scale. Viewports are drawn live from the model. Annotations live inside the viewport, in paper mm. A title block is a parametric object with a border, a grid reference and fields from the project and the sheet.",
        "export": "export.file {path, format, options?, overwrite?} with format pdf (options: sheets? [layer ID or name], default all sheet layers, one page each), sheet_svg (options: sheet) or sheet_png (options: sheet, dpi? default 150, at most 64 megapixels). The extension must be .pdf, .svg or .png. The path rules of export.file apply. At most 64 megapixels of 3D viewport images per page and per PDF.",
        "viewport_visibility": "A viewport stores the visibility of all design layers and classes when it is created. Later changes in the document do not reach it. Change it with viewport.set_override.",
        "override_precedence": "Inside a viewport (as in Vectorworks, a class override acts on attributes that are by class only): the object's own attributes, then the viewport class override, then the class attributes, then defaults (black, no fill, 0.25 mm). Visibility is the viewport's own.",
        "examples": [
            {"method": "sheet.create", "params": {"name": "Planting plan", "paper": "a1", "number": "L-101", "title": "Planting plan"}},
            {"method": "viewport.create", "params": {"sheet": "Planting plan", "scale": "fit", "view": "plan"}},
            {"method": "viewport.set_override", "params": {"id": 12, "classes": {"Existing": {"visible": false}, "Trees": {"pen": [0, 100, 0, 255], "line_weight": 0.5}}}},
            {"method": "viewport.annotate", "params": {"id": 12, "add": [{"kind": "text", "at": [20, 20], "text": "North", "height": 4}]}},
            {"method": "title_block.create", "params": {"sheet": "Planting plan", "grid_columns": 6, "grid_rows": 4}},
            {"method": "project.set", "params": {"name": "Riverside garden", "drawn_by": "NM", "date": "2026-10-10"}},
            {"method": "saved_view.save", "params": {"name": "Planting only"}},
            {"method": "export.file", "params": {"format": "pdf", "path": "/tmp/plans.pdf", "options": {"sheets": ["Planting plan"]}}}
        ],
    })
}

fn bad(e: impl std::fmt::Display) -> RpcError {
    RpcError { code: -32602, message: e.to_string() }
}

fn app(e: impl std::fmt::Display) -> RpcError {
    RpcError { code: 1, message: e.to_string() }
}

type Res = Result<Value, RpcError>;

const KNOWN: &[&str] = &[
    "sheet.list", "sheet.create", "sheet.set", "sheet.render", "project.get", "project.set", "viewport.create", "viewport.get", "viewport.list",
    "viewport.update", "viewport.set_override", "viewport.annotate", "title_block.create", "title_block.update", "title_block.resolve", "saved_view.list",
    "saved_view.save", "saved_view.restore", "saved_view.delete",
];

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Res> {
    if !KNOWN.contains(&method) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(run(s, method, &p))
}

fn run(s: &mut Session, method: &str, p: &Value) -> Res {
    match method {
        "sheet.list" => Ok(sheet_list(s.editor.doc())),
        "sheet.create" => sheet_create(s, p),
        "sheet.set" => sheet_set(s, p),
        "sheet.render" => sheet_render(s, p),
        "project.get" => Ok(serde_json::to_value(&s.editor.doc().drawing.project).unwrap()),
        "project.set" => project_set(s, p),
        "viewport.create" => viewport_create(s, p),
        "viewport.get" => {
            let (id, _) = object_id(p)?;
            viewport_json(s.editor.doc(), id)
        }
        "viewport.list" => viewport_list(s, p),
        "viewport.update" => viewport_update(s, p),
        "viewport.set_override" => viewport_override(s, p),
        "viewport.annotate" => viewport_annotate(s, p),
        "title_block.create" => title_block_create(s, p),
        "title_block.update" => title_block_update(s, p),
        "title_block.resolve" => title_block_resolve(s, p),
        "saved_view.list" => Ok(json!({"views": s.editor.doc().drawing.saved_views})),
        "saved_view.save" => saved_view_save(s, p),
        "saved_view.restore" => saved_view_restore(s, p),
        "saved_view.delete" => {
            let name = str_field(p, "name")?;
            s.editor.execute(&Command::DeleteView { name }).map_err(app)?;
            Ok(json!({"ok": true}))
        }
        _ => unreachable!("checked by KNOWN"),
    }
}

// ---------------------------------------------------------------- lookups

fn str_field(p: &Value, key: &str) -> Result<String, RpcError> {
    p.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| bad(format!("params.{key} is required (text)")))
}

fn object_id(p: &Value) -> Result<(ObjectId, ()), RpcError> {
    p.get("id").and_then(Value::as_u64).map(|i| (ObjectId(i), ())).ok_or_else(|| bad("params.id is required (an object ID)"))
}

/// A layer from an ID or a name.
fn layer_ref(doc: &Document, v: &Value) -> Result<LayerId, RpcError> {
    let found = match v {
        Value::Number(n) => n.as_u64().and_then(|i| doc.layer(LayerId(i))).map(|l| l.id),
        Value::String(t) => t
            .parse::<u64>()
            .ok()
            .and_then(|i| doc.layer(LayerId(i)))
            .or_else(|| doc.layers.iter().find(|l| l.name.eq_ignore_ascii_case(t.trim())))
            .map(|l| l.id),
        _ => None,
    };
    found.ok_or_else(|| bad(format!("no layer {v}. Use a layer ID or name")))
}

fn sheet_ref(doc: &Document, v: &Value) -> Result<LayerId, RpcError> {
    let id = layer_ref(doc, v)?;
    match doc.layer(id) {
        Some(l) if l.kind == LayerKind::Sheet => Ok(id),
        _ => Err(bad(format!("layer {id} is not a sheet layer"))),
    }
}

fn class_ref(doc: &Document, v: &Value) -> Result<ClassId, RpcError> {
    let found = match v {
        Value::Number(n) => n.as_u64().and_then(|i| doc.class(ClassId(i))).map(|c| c.id),
        Value::String(t) => t
            .parse::<u64>()
            .ok()
            .and_then(|i| doc.class(ClassId(i)))
            .or_else(|| doc.classes.iter().find(|c| c.name.eq_ignore_ascii_case(t.trim())))
            .map(|c| c.id),
        _ => None,
    };
    found.ok_or_else(|| bad(format!("no class {v}. Use a class ID or name")))
}

fn norm_view(name: &str) -> Result<String, RpcError> {
    StandardView::from_name(name).map(|v| v.id().to_string()).ok_or_else(|| {
        let all: Vec<&str> = StandardView::ALL.iter().map(|v| v.id()).collect();
        bad(format!("unknown view '{name}'. Views: {}", all.join(", ")))
    })
}

fn parse<T: for<'de> Deserialize<'de>>(v: Value) -> Result<T, RpcError> {
    serde_json::from_value(v).map_err(bad)
}

fn paper_from(p: &Value) -> Result<Option<PaperSize>, RpcError> {
    let custom = |w: &Value, h: &Value| -> Result<PaperSize, RpcError> {
        match (w.as_f64(), h.as_f64()) {
            (Some(w), Some(h)) => Ok(PaperSize::Custom { width_mm: w, height_mm: h }),
            _ => Err(bad("a custom paper needs width_mm and height_mm (numbers)")),
        }
    };
    match p.get("paper") {
        None | Some(Value::Null) => match (p.get("width_mm"), p.get("height_mm")) {
            (None, None) => Ok(None),
            (Some(w), Some(h)) => custom(w, h).map(Some),
            _ => Err(bad("give both width_mm and height_mm")),
        },
        Some(Value::String(t)) => PaperSize::from_id(t).map(Some).ok_or_else(|| {
            let all: Vec<&str> = PaperSize::NAMED.iter().map(|p| p.id()).collect();
            bad(format!("unknown paper '{t}'. Use {} or {{width_mm, height_mm}}", all.join(", ")))
        }),
        Some(o @ Value::Object(m)) => {
            if let Some(c) = m.get("custom") {
                custom(&c["width_mm"], &c["height_mm"]).map(Some)
            } else {
                custom(&o["width_mm"], &o["height_mm"]).map(Some)
            }
        }
        Some(_) => Err(bad("paper must be a size name or {width_mm, height_mm}")),
    }
}

fn orientation_from(p: &Value) -> Result<Option<Orientation>, RpcError> {
    match p.get("orientation") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Orientation::from_id(t).map(Some).ok_or_else(|| bad("orientation is landscape or portrait")),
        Some(_) => Err(bad("orientation is landscape or portrait")),
    }
}

fn opt_str(p: &Value, key: &str) -> Option<String> {
    p.get(key).and_then(Value::as_str).map(str::to_string)
}

// ---------------------------------------------------------------- sheets and project

fn sheet_json(doc: &Document, layer: &Layer) -> Value {
    let sheet = doc.sheet(layer.id).unwrap_or_else(|| Sheet::new("", layer.name.clone()));
    let size = sheet.size_mm();
    let on_sheet = |want: &str| -> Vec<ObjectId> { doc.objects.values().filter(|o| o.layer == layer.id && o.shape.kind() == want).map(|o| o.id).collect() };
    json!({
        "layer": layer.id, "name": layer.name, "number": sheet.number, "title": sheet.title,
        "paper": sheet.paper, "orientation": sheet.orientation, "width_mm": size.x, "height_mm": size.y,
        "viewports": on_sheet("viewport"), "title_blocks": on_sheet("title_block"),
    })
}

fn sheet_list(doc: &Document) -> Value {
    json!({"sheets": doc.sheet_layers().map(|l| sheet_json(doc, l)).collect::<Vec<_>>()})
}

fn sheet_create(s: &mut Session, p: &Value) -> Res {
    let name = str_field(p, "name")?;
    let cmd = Command::AddSheet { name, paper: paper_from(p)?, orientation: orientation_from(p)?, number: opt_str(p, "number"), title: opt_str(p, "title") };
    let out = s.editor.execute(&cmd).map_err(app)?;
    Ok(json!({"layer": out.created[0]}))
}

fn sheet_set(s: &mut Session, p: &Value) -> Res {
    let layer = sheet_ref(s.editor.doc(), p.get("layer").unwrap_or(&Value::Null))?;
    let cmd = Command::SetSheet { layer, paper: paper_from(p)?, orientation: orientation_from(p)?, number: opt_str(p, "number"), title: opt_str(p, "title") };
    s.editor.execute(&cmd).map_err(app)?;
    let doc = s.editor.doc();
    Ok(sheet_json(doc, doc.layer(layer).expect("checked")))
}

fn project_set(s: &mut Session, p: &Value) -> Res {
    let cmd = Command::SetProject {
        name: opt_str(p, "name"),
        client: opt_str(p, "client"),
        address: opt_str(p, "address"),
        drawn_by: opt_str(p, "drawn_by"),
        date: opt_str(p, "date"),
        revision: opt_str(p, "revision"),
    };
    s.editor.execute(&cmd).map_err(app)?;
    Ok(serde_json::to_value(&s.editor.doc().drawing.project).unwrap())
}

// ---------------------------------------------------------------- viewports

fn viewport_json(doc: &Document, id: ObjectId) -> Res {
    let (o, vp) = find_viewport(doc, id)?;
    let mut v = serde_json::to_value(vp).unwrap();
    v["id"] = json!(o.id);
    v["sheet"] = json!(o.layer);
    Ok(v)
}

fn find_viewport(doc: &Document, id: ObjectId) -> Result<(&Object, &Viewport), RpcError> {
    let o = doc.object(id).ok_or_else(|| app(format!("object {id} does not exist")))?;
    match &o.shape {
        Shape::Viewport(v) => Ok((o, v)),
        _ => Err(bad(format!("object {id} is not a viewport"))),
    }
}

fn viewport_list(s: &Session, p: &Value) -> Res {
    let doc = s.editor.doc();
    let sheet = match p.get("sheet") {
        None | Some(Value::Null) => None,
        Some(v) => Some(sheet_ref(doc, v)?),
    };
    let rows: Vec<Value> = doc
        .objects
        .values()
        .filter(|o| sheet.is_none_or(|l| o.layer == l))
        .filter_map(|o| match &o.shape {
            Shape::Viewport(v) => Some(json!({"id": o.id, "sheet": o.layer, "name": v.name, "scale": v.scale, "view": v.view, "layers": v.layers, "min": v.min, "max": v.max, "annotations": v.annotations.len()})),
            _ => None,
        })
        .collect();
    Ok(json!({"viewports": rows}))
}

fn layer_list(doc: &Document, v: &Value) -> Result<Vec<LayerId>, RpcError> {
    let arr = v.as_array().ok_or_else(|| bad("layers must be a list of layer IDs or names"))?;
    let mut out = Vec::new();
    for x in arr {
        let id = layer_ref(doc, x)?;
        if doc.layer(id).is_some_and(|l| l.kind != LayerKind::Design) {
            return Err(bad(format!("layer {id} is a sheet layer. A viewport shows design layers")));
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CreateViewport {
    min: Option<DVec2>,
    max: Option<DVec2>,
    at: Option<DVec2>,
    size: Option<DVec2>,
    center: Option<DVec2>,
    view: Option<String>,
    mode: Option<String>,
    dpi: Option<f64>,
    name: Option<String>,
    crop: Option<Crop>,
    show_frame: Option<bool>,
}

fn scale_from(p: &Value) -> Result<Option<f64>, RpcError> {
    match p.get("scale") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) if t.eq_ignore_ascii_case("fit") => Ok(None),
        Some(v) => v.as_f64().map(Some).ok_or_else(|| bad("scale is a number such as 100 (for 1:100) or \"fit\"")),
    }
}

fn viewport_create(s: &mut Session, p: &Value) -> Res {
    let doc = s.editor.doc();
    let sheet = sheet_ref(doc, p.get("sheet").unwrap_or(&Value::Null))?;
    let c: CreateViewport = parse(p.clone())?;
    let page = doc.sheet(sheet).expect("checked").size_mm();
    let layers = match p.get("layers") {
        None | Some(Value::Null) => doc.layers.iter().filter(|l| l.kind == LayerKind::Design).map(|l| l.id).collect(),
        Some(v) => layer_list(doc, v)?,
    };
    let (min, max) = match (c.min, c.max, c.at, c.size) {
        (Some(a), Some(b), _, _) => (a, b),
        (_, _, Some(a), Some(sz)) => (a, a + sz),
        (None, None, None, None) => (DVec2::new(15.0, 55.0), page - DVec2::splat(15.0)),
        _ => return Err(bad("give min and max, or at and size")),
    };
    let mut vp = Viewport::new(min, max, DVec2::ZERO, scale_from(p)?.unwrap_or(100.0), layers);
    if let Some(v) = &c.view {
        vp.view = norm_view(v)?;
    }
    if let Some(m) = c.mode {
        vp.mode = m;
    }
    if let Some(d) = c.dpi {
        vp.dpi = d;
    }
    vp.name = c.name.unwrap_or_default();
    vp.crop = c.crop;
    vp.show_frame = c.show_frame.unwrap_or(false);
    vp.snapshot_visibility(doc);
    let mut fitted = vp.clone();
    fitted.fit(doc);
    vp.center = c.center.unwrap_or(fitted.center);
    if scale_from(p)?.is_none() {
        vp.scale = fitted.scale;
    }
    let cmd = Command::AddObject { shape: Shape::Viewport(Box::new(vp)), layer: Some(sheet), class: None, attrs: Attributes::default() };
    let out = s.editor.execute(&cmd).map_err(app)?;
    viewport_json(s.editor.doc(), ObjectId(out.created[0]))
}

/// Applies `f` to a copy of the viewport and stores the result as one undo step.
fn change_viewport(s: &mut Session, id: ObjectId, f: impl FnOnce(&Document, &mut Viewport) -> Result<(), RpcError>) -> Res {
    let doc = s.editor.doc();
    let (_, vp) = find_viewport(doc, id)?;
    let mut vp = vp.clone();
    f(doc, &mut vp)?;
    s.editor.execute(&Command::ModifyObject { id, shape: Some(Shape::Viewport(Box::new(vp))), layer: None, class: None, attrs: None }).map_err(app)?;
    viewport_json(s.editor.doc(), id)
}

fn viewport_update(s: &mut Session, p: &Value) -> Res {
    let (id, _) = object_id(p)?;
    let p = p.clone();
    change_viewport(s, id, move |doc, vp| {
        // Merge the given keys into the viewport JSON, so one validation path covers all fields.
        let mut v = serde_json::to_value(&*vp).unwrap();
        let obj = v.as_object_mut().expect("a viewport is an object");
        for key in ["name", "min", "max", "center", "mode", "dpi", "crop", "show_frame"] {
            if let Some(x) = p.get(key) {
                obj.insert(key.into(), x.clone());
            }
        }
        if let Some(x) = p.get("view") {
            obj.insert("view".into(), json!(norm_view(x.as_str().ok_or_else(|| bad("view must be text"))?)?));
        }
        if let Some(x) = p.get("layers") {
            obj.insert("layers".into(), json!(layer_list(doc, x)?));
        }
        if let Some(sc) = scale_from(&p)? {
            obj.insert("scale".into(), json!(sc));
        }
        *vp = parse(v)?;
        let fit = p.get("fit").and_then(Value::as_bool).unwrap_or(false) || matches!(p.get("scale"), Some(Value::String(t)) if t.eq_ignore_ascii_case("fit"));
        if fit {
            vp.fit(doc);
        }
        Ok(())
    })
}

fn viewport_override(s: &mut Session, p: &Value) -> Res {
    let (id, _) = object_id(p)?;
    let p = p.clone();
    change_viewport(s, id, move |doc, vp| {
        if p.get("reset").and_then(Value::as_bool).unwrap_or(false) {
            vp.layer_overrides.clear();
            vp.class_overrides.clear();
        }
        if let Some(m) = p.get("layers") {
            for (k, v) in m.as_object().ok_or_else(|| bad("layers is an object {layer: true | false | null}"))? {
                let l = layer_ref(doc, &json!(k))?;
                match v {
                    Value::Null => {
                        vp.layer_overrides.remove(&l);
                    }
                    Value::Bool(b) => {
                        vp.layer_overrides.insert(l, *b);
                    }
                    _ => return Err(bad("a layer override is true, false or null")),
                }
            }
        }
        if let Some(m) = p.get("classes") {
            for (k, v) in m.as_object().ok_or_else(|| bad("classes is an object {class: {...} | null}"))? {
                let c = class_ref(doc, &json!(k))?;
                if v.is_null() {
                    vp.class_overrides.remove(&c);
                    continue;
                }
                let fields = v.as_object().ok_or_else(|| bad("a class override is an object or null"))?;
                let mut cur = serde_json::to_value(vp.class_overrides.get(&c).cloned().unwrap_or_default()).unwrap();
                let cur_map = cur.as_object_mut().expect("an override is an object");
                for (f, x) in fields {
                    if !["visible", "pen", "fill", "line_weight"].contains(&f.as_str()) {
                        return Err(bad(format!("unknown override field '{f}'. Use visible, pen, fill, line_weight")));
                    }
                    if x.is_null() {
                        cur_map.remove(f);
                    } else {
                        cur_map.insert(f.clone(), x.clone());
                    }
                }
                let ov: ClassOverride = parse(cur)?;
                vp.class_overrides.insert(c, ov);
            }
        }
        Ok(())
    })
}

#[derive(Deserialize)]
struct NewAnnotation {
    #[serde(default)]
    class: Option<Value>,
    #[serde(default)]
    attrs: Attributes,
    #[serde(flatten)]
    item: AnnotationItem,
}

fn viewport_annotate(s: &mut Session, p: &Value) -> Res {
    let (id, _) = object_id(p)?;
    let p = p.clone();
    let mut added = Vec::new();
    let out = change_viewport(s, id, |doc, vp| {
        if p.get("clear").and_then(Value::as_bool).unwrap_or(false) {
            vp.annotations.clear();
        }
        if let Some(r) = p.get("remove") {
            let ids: Vec<u32> = parse(r.clone())?;
            if let Some(missing) = ids.iter().find(|i| !vp.annotations.iter().any(|a| a.id == **i)) {
                return Err(bad(format!("the viewport has no annotation {missing}")));
            }
            vp.annotations.retain(|a| !ids.contains(&a.id));
        }
        if let Some(list) = p.get("add") {
            let items: Vec<NewAnnotation> = parse(list.clone())?;
            for n in items {
                let class = match &n.class {
                    Some(c) => class_ref(doc, c)?,
                    None => NONE_CLASS,
                };
                let aid = vp.next_annotation_id();
                vp.annotations.push(Annotation { id: aid, class, attrs: n.attrs, item: n.item });
                added.push(aid);
            }
        }
        Ok(())
    })?;
    Ok(json!({"added": added, "count": out["annotations"].as_array().map_or(0, Vec::len), "viewport": out}))
}

// ---------------------------------------------------------------- title blocks

fn find_title_block(doc: &Document, id: ObjectId) -> Result<(&Object, &TitleBlock), RpcError> {
    let o = doc.object(id).ok_or_else(|| app(format!("object {id} does not exist")))?;
    match &o.shape {
        Shape::TitleBlock(t) => Ok((o, t)),
        _ => Err(bad(format!("object {id} is not a title block"))),
    }
}

fn title_block_from(base: TitleBlock, p: &Value) -> Result<TitleBlock, RpcError> {
    let mut v = serde_json::to_value(&base).unwrap();
    let obj = v.as_object_mut().expect("a title block is an object");
    for key in ["margin", "grid_columns", "grid_rows", "fields", "offset"] {
        if let Some(x) = p.get(key) {
            obj.insert(key.into(), x.clone());
        }
    }
    match p.get("layout") {
        None => {}
        Some(Value::Null) => {
            obj.remove("layout");
        }
        Some(x) => {
            obj.insert("layout".into(), x.clone());
        }
    }
    parse(v)
}

fn title_block_create(s: &mut Session, p: &Value) -> Res {
    let sheet = sheet_ref(s.editor.doc(), p.get("sheet").unwrap_or(&Value::Null))?;
    let tb = title_block_from(TitleBlock::default(), p)?;
    let cmd = Command::AddObject { shape: Shape::TitleBlock(tb), layer: Some(sheet), class: None, attrs: Attributes::default() };
    let out = s.editor.execute(&cmd).map_err(app)?;
    Ok(json!({"id": out.created[0]}))
}

fn title_block_update(s: &mut Session, p: &Value) -> Res {
    let (id, _) = object_id(p)?;
    let (_, tb) = find_title_block(s.editor.doc(), id)?;
    let tb = title_block_from(tb.clone(), p)?;
    s.editor.execute(&Command::ModifyObject { id, shape: Some(Shape::TitleBlock(tb.clone())), layer: None, class: None, attrs: None }).map_err(app)?;
    Ok(serde_json::to_value(&tb).unwrap())
}

fn title_block_resolve(s: &Session, p: &Value) -> Res {
    let (id, _) = object_id(p)?;
    let doc = s.editor.doc();
    let (o, tb) = find_title_block(doc, id)?;
    let page = doc.sheet(o.layer).ok_or_else(|| app("the title block is not on a sheet layer"))?.size_mm();
    let g = title_block_geometry(tb, &Fields::of(doc, o.layer, tb), page);
    Ok(json!({
        "border": [g.border.0, g.border.1],
        "block": [g.block.0, g.block.1],
        "grid_labels": g.grid_labels.iter().map(|(at, t)| json!({"at": at, "text": t})).collect::<Vec<_>>(),
        "cells": g.cells.iter().map(|(a, b, label, text, size)| json!({"min": a, "max": b, "label": label, "text": text, "size": size})).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------- saved views

fn saved_view_save(s: &mut Session, p: &Value) -> Res {
    let name = str_field(p, "name")?;
    let doc = s.editor.doc();
    let layers: BTreeMap<LayerId, bool> = doc.layers.iter().map(|l| (l.id, l.visible)).collect();
    let classes: BTreeMap<ClassId, bool> = doc.classes.iter().map(|c| (c.id, c.visible)).collect();
    let view = SavedView {
        name,
        layers,
        classes,
        active_layer: Some(doc.active_layer),
        active_class: Some(doc.active_class),
        zoom: p.get("zoom").and_then(Value::as_f64),
        pan: p.get("pan").map(|v| parse::<DVec2>(v.clone())).transpose()?,
        view: Some(s.view.view.id().to_string()),
        projection: Some(s.view.projection.id().to_string()),
        mode: Some(s.view.mode.id().to_string()),
    };
    s.editor.execute(&Command::SaveView { view: Box::new(view.clone()) }).map_err(app)?;
    Ok(serde_json::to_value(view).unwrap())
}

fn saved_view_restore(s: &mut Session, p: &Value) -> Res {
    let name = str_field(p, "name")?;
    s.editor.execute(&Command::RestoreView { name: name.clone() }).map_err(app)?;
    let view = s.editor.doc().drawing.saved_view(&name).cloned().expect("restored");
    s.apply_saved_view(&view);
    Ok(serde_json::to_value(view).unwrap())
}

impl Session {
    /// Sets the 3D view settings of a saved view, and keeps the view for the host.
    /// The host takes it with `take_applied_view` and applies zoom and pan.
    pub fn apply_saved_view(&mut self, v: &SavedView) {
        let mut next = self.view;
        if let Some(x) = v.view.as_deref().and_then(StandardView::from_name) {
            next.view = x;
        }
        if let Some(x) = v.projection.as_deref().and_then(Projection::from_name) {
            next.projection = x;
        }
        if let Some(x) = v.mode.as_deref().and_then(RenderMode::from_name) {
            next.mode = x;
        }
        if next != self.view {
            self.view = next;
            self.view_changed = true;
        }
        self.applied_view = Some(v.clone());
        self.prune_selection();
    }

    /// The saved view that `saved_view.restore` applied, once. The host sets its zoom and pan.
    pub fn take_applied_view(&mut self) -> Option<SavedView> {
        self.applied_view.take()
    }
}

// ---------------------------------------------------------------- output

#[cfg(not(target_arch = "wasm32"))]
fn sheet_render(s: &mut Session, p: &Value) -> Res {
    let layer = sheet_ref(s.editor.doc(), p.get("sheet").unwrap_or(&Value::Null))?;
    let format = opt_str(p, "format").unwrap_or_else(|| "svg".into());
    let page = s.sheet_page(layer)?;
    match format.as_str() {
        "svg" => Ok(json!({"svg": cadektor_print::svg::page_svg(&page), "width_mm": page.size.x, "height_mm": page.size.y})),
        "png" => {
            let dpi = p.get("dpi").and_then(Value::as_f64).unwrap_or(100.0);
            let bytes = cadektor_print::png::page_png(&page, dpi).map_err(app)?;
            use base64::Engine;
            let (w, h) = png_size(&bytes);
            Ok(json!({"png_base64": base64::engine::general_purpose::STANDARD.encode(bytes), "width": w, "height": h}))
        }
        _ => Err(bad("format is svg or png")),
    }
}

#[cfg(target_arch = "wasm32")]
fn sheet_render(_: &mut Session, _: &Value) -> Res {
    Err(app("sheet output is not available in this build"))
}

#[cfg(not(target_arch = "wasm32"))]
impl Session {
    /// The display list of a sheet. It is rebuilt only when the document changed.
    pub fn sheet_page(&mut self, layer: LayerId) -> Result<std::sync::Arc<cadektor_print::Page>, RpcError> {
        let revision = self.editor.revision();
        self.sheet_cache.page(self.editor.doc(), revision, layer).map_err(app)
    }
}

/// Width and height from the header of a PNG file.
#[cfg(not(target_arch = "wasm32"))]
fn png_size(b: &[u8]) -> (u32, u32) {
    let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    (be(16), be(20))
}

/// The sheet formats of `export.file`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn is_sheet_format(f: &str) -> bool {
    matches!(f, "pdf" | "sheet_svg" | "sheet_png")
}

#[cfg(not(target_arch = "wasm32"))]
impl Session {
    /// `export.file` with the format `pdf`, `sheet_svg` or `sheet_png`. The path rules are those of
    /// the other formats: `check_path` for agents, and `overwrite` to replace a file in a restricted session.
    pub(crate) fn export_sheet_file(&mut self, format: &str, path: std::path::PathBuf, options: &Value, overwrite: bool, checked: bool) -> Res {
        use cadektor_print as print;
        let ext = match format {
            "pdf" => "pdf",
            "sheet_svg" => "svg",
            _ => "png",
        };
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
            return Err(bad(format!("the file name must end with .{ext}")));
        }
        if path.is_dir() {
            return Err(bad(format!("{} is a folder", path.display())));
        }
        let path = if checked { self.check_path(&path, true, &[ext]).map_err(app)? } else { path };
        if checked && self.paths.is_restricted() && path.exists() && !overwrite {
            return Err(app(format!("{} exists: pass \"overwrite\": true to replace it", path.display())));
        }
        let options = if options.is_null() { json!({}) } else { options.clone() };
        let doc = self.editor.doc();
        let (bytes, extra) = if format == "pdf" {
            let ids: Option<Vec<LayerId>> = match options.get("sheets") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_array().ok_or_else(|| bad("sheets is a list of sheet layer IDs or names"))?.iter().map(|x| sheet_ref(doc, x)).collect::<Result<_, _>>()?),
            };
            let list = print::select_sheets(doc, ids.as_deref()).map_err(bad)?;
            let revision = self.editor.revision();
            let bytes = print::export_pdf(self.editor.doc(), revision, &mut self.sheet_cache, &list).map_err(app)?;
            (bytes, json!({"pages": list.len()}))
        } else {
            let layer = sheet_ref(doc, options.get("sheet").unwrap_or(&Value::Null))?;
            let page = self.sheet_page(layer)?;
            if ext == "svg" {
                (print::svg::page_svg(&page).into_bytes(), json!({}))
            } else {
                let dpi = options.get("dpi").and_then(Value::as_f64).unwrap_or(150.0);
                let bytes = print::png::page_png(&page, dpi).map_err(bad)?;
                let (w, h) = png_size(&bytes);
                (bytes, json!({"width": w, "height": h}))
            }
        };
        print::write_file(&path, &bytes).map_err(app)?;
        let mut out = json!({"path": path.display().to_string(), "bytes": bytes.len()});
        out.as_object_mut().expect("object").extend(extra.as_object().cloned().unwrap_or_default());
        Ok(out)
    }
}
