//! Agent API for the unit layout engine: lay units (pavers, tiles, boards, bricks) in a bond or on
//! a module lattice over any closed shape or a region you give.
//!
//! Methods: `unit_pattern.layout`, `unit_pattern.to_objects`, `unit_pattern.module_preset`,
//! `unit_pattern.module_from_shapes`, `unit_pattern.module_from_block`, `report.unit_pattern`.

use serde_json::{Map, Value, json};

use kadakas_core::DVec2;
use kadakas_core::command::Command;
use kadakas_core::curve::Vertex;
use kadakas_core::layout::object::{outline_of, pattern_of, region_of};
use kadakas_core::layout::plan::{self, Body, Group, Role};
use kadakas_core::layout::summary::{self, Summary, SummaryOptions, with_waste};
use kadakas_core::layout::{
    self, Bond, LayoutError, Limits, MAX_GAP_SEGMENTS, MAX_UNITS, Module, ModuleUnit, Pattern,
    Placement, Region, TOLERANCE, Unit, gaps,
};
use kadakas_core::model::{Attributes, ClassId, Document, LayerId, Object, ObjectId, Shape};

use crate::errors::bad;
use crate::{RpcError, Session};

pub const UNIT_PATTERN_LAYOUT: (&str, &str) = (
    "unit_pattern.layout",
    "Lay units (pavers, tiles, boards) over a closed shape and return the units and the gaps. params: {id | region: {outline: [vertices], holes?: [[vertices]]}, pattern?, origin? ([x, y]: where the pattern starts), angle? (degrees, counterclockwise), align_to_edge? (n: start at vertex n of the outline and run along the edge to vertex n+1; sets origin and angle), include? ([\"units\", \"gaps\"], default both), offset? and limit? (a page of the lists, default 0 and 1000, at most 20000; limit 0 gives the counts only), tolerance? (curve flattening, mm, default 1), and the count options of report.unit_pattern}. pattern is {bond? (running_bond | stack | herringbone | basket_weave), unit_length, unit_width, gap? (the joint width, default 0)} for the four bonds. Every whole unit has exactly unit_length by unit_width. Or a module: {kind: module, units: [{name, outline}], a: [x, y], b: [x, y], gap?}, or {preset: hexagon | octagon_square | i_shape, ...numbers, gap?} (see unit_pattern.module_preset). A closed polyline, rectangle, circle, landscape area or hardscape works as id. A hardscape with zones or courses (hardscape.split, hardscape.course) is laid group by group: the field (or its zones) and the courses; the reply then has groups: [{name, role (field | zone | course), layer_suffix, ...the counts of the summary}] next to the total in summary, every unit has a group, and notes lists what the plan could not do. A hardscape with a style uses the joint pattern of the style with its own origin and angle unless you give pattern, origin, angle or align_to_edge, and the pattern is laid inside its kerb border and outside its holes. The pattern starts at the middle of the box of the region without an origin. The unit is the pattern cell with half a gap taken off on every side, cut exactly to the outline minus the holes (arcs are cut into lines within the tolerance). Returns {summary: {bond, origin, angle_deg, whole_units, cut_units, slivers, unfillable_pieces, area_laid_m2, gap_area_m2, order_units, order_units_max, order_units_min, by_unit: [{name, whole_units, cut_units, ...}], ...}, units_total, units: [{kind (unit shape name), whole, area_m2, share (part of a full unit), outline: [[x, y]], holes: [[[x, y]]]}], gaps_total, gaps: [[[x, y], [x, y]]] (gap center lines), next_offset (null at the end)}. A cut unit that the edge splits in two pieces counts as two units. A layout of more than 1,000,000 units (all groups together) is an error that says how many units it would make.",
);
pub const UNIT_PATTERN_TO_OBJECTS: (&str, &str) = (
    "unit_pattern.to_objects",
    "Turn the units (and, with include, the gap center lines) of a layout into ordinary objects in one undo step: a closed polyline for every unit and a line for every gap segment, in one group. params: as unit_pattern.layout (id | region, pattern, origin, angle, align_to_edge, tolerance), plus include? ([\"units\"] by default; add \"gaps\"), layer? (default: the layer of the object, else the active layer), class?, group? (default true), name? (name of the group). A hole inside a cut unit becomes a closed polyline of its own. With a module pattern each unit polyline has the data field unit_type with the name of its shape. At most 100,000 objects in one call. A hardscape with zones or courses makes the units of all groups. Returns {group, units, gaps, objects}.",
);
pub const UNIT_PATTERN_MODULE_PRESET: (&str, &str) = (
    "unit_pattern.module_preset",
    "Build a module pattern from a preset. params: {preset: hexagon | octagon_square | i_shape, ...numbers in mm}. hexagon: side or across_flats. octagon_square: side (a regular octagon and the small square between four octagons). i_shape: length? (200), width? (165), notch? (25): a double-T interlocking paver, rows shifted by half a length (generic approximate dimensions). Returns {pattern: {kind: module, units, a, b}, note}. Use the pattern as the pattern of unit_pattern.layout, report.unit_pattern, or as the joints of a hardscape style (add gap as spacing).",
);
pub const UNIT_PATTERN_MODULE_FROM_SHAPES: (&str, &str) = (
    "unit_pattern.module_from_shapes",
    "Make a module pattern from closed shapes in the drawing (closed polylines, rectangles, circles). params: {ids? (default: the selection), names? ([text] one per shape; default unit 1, unit 2, ...), a? and b? ([x, y] the two repeat vectors)}. Without a and b the lattice is the box of the shapes (a stack): a = [width, 0], b = [0, height], and the reply says so. The shapes keep their place in the box; the corner of the box is the origin. The shapes must not overlap each other or their copies. Returns {pattern: {kind: module, units, a, b}, note?}.",
);
#[cfg(not(target_arch = "wasm32"))]
pub const UNIT_PATTERN_MODULE_FROM_BLOCK: (&str, &str) = (
    "unit_pattern.module_from_block",
    "Make a module pattern from the closed polylines of a block of a DXF file. params: {path, block, a? and b? ([x, y] repeat vectors; default the box of the shapes, as in module_from_shapes)}. The block goes through the DXF import (arcs from bulges, units from $INSUNITS, nested blocks exploded). The DXF layer of a polyline is the name of its unit shape. The path policy of import.file applies. Returns {pattern, note?, blocks?} (blocks lists the blocks of the file when the name is wrong, in the error).",
);
pub const REPORT_UNIT_PATTERN: (&str, &str) = (
    "report.unit_pattern",
    "Counts and quantities of unit patterns. params: {ids? (default: every hardscape with a joint pattern), pattern?, origin?, angle?, align_to_edge? (as unit_pattern.layout; with several ids they apply to each), sliver_share? (cut units under this part of a full unit are slivers, default 0.25), unfillable_share? (cut units under this part are too small to lay, default 0.05), min_width? (mm: cut units narrower than this cannot be laid, default off), exclude_unfillable? (default false: leave the unfillable pieces out of order_units), trim? (mm, default 0.1: a unit that crosses the edge by less than this counts as whole, so drawing noise makes no cut units; 0 turns it off), waste_percent? (no default: without it the order has no waste added), gap_length? (measure the length of the gap lines, default false), max_extra_joint_mm? (default 2: a basket weave whose joints get wider than the gap by more than this has a joint_warning in its summary; a much larger difference is refused), visible_only? (skip hardscapes on hidden layers, default false), tolerance? (curve flattening in mm, default 1; the DXF export has its own tolerance, default 1, and gives the same counts when the two are equal), format?: csv}. Returns {rows: [{id, name, whole_units, cut_units, slivers, unfillable_pieces, area_laid_m2, cut_area_m2, region_area_m2, gap_area_m2, gap_length_m?, order_units, order_units_max, order_units_min, order_units_with_waste, by_unit: [{name, whole_units, cut_units, ...}], ...} or {id, error}], totals: {..., complete}}. order_units is an upper bound: the whole units plus the cut units, every cut unit counted as one unit, no offcut used twice. order_units_min is a lower bound: the whole units plus the sum of the shares of the cut units, rounded up. waste_percent adds that percent to the total, rounded up, in whole numbers. Hardscapes are counted inside their kerb border and outside their holes. A hardscape with zones or courses has groups: [...] in its row (the counts of the field or each zone, and of each course, with its unit type; courses also have course: {length_m, corners, joints: {count, narrowest_mm, widest_mm}}) and notes; the row itself and the totals are the sums, and a stone that the course and the field share is one row in by_unit. CSV has a row per group. complete is false when a row is an error. The counts match the polylines on the layers <layer>-UNITS-WHOLE and <layer>-UNITS-CUT of export.file with patterns: \"units\".",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    let mut d = describe_base();
    if let (Some(a), Some(b)) = (d.as_object_mut(), crate::plan_api::describe().as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    d
}

fn describe_base() -> Value {
    json!({
        "unit_pattern": "The layout engine lays equal units in a bond (running_bond, stack, herringbone, basket_weave) or the unit shapes of a module over a closed shape with holes. The pattern is a set of cells that run at the centers of the joints; a unit is its cell moved inward by half the gap. So every whole unit of a bond is exactly unit_length by unit_width, for any ratio, and the gap is a real gap. Units are cut exactly to the outline minus the holes. Hardscapes use the engine for their joint pattern; their holes, origin, angle and align_to_edge are settings of the object (hardscape.add, hardscape.set).",
        "modules": "A module pattern is one or more closed shapes (unit types, each with a name; curves and concave outlines are allowed) and two repeat vectors a and b (the lattice). The shapes are the outlines at the centers of the joints; the gap moves each inward by half the gap. The shapes must not overlap each other or their copies on the neighbouring lattice points (a, b, a+b, a-b); spaces between shapes are allowed and show as gaps. Counts, reports and the DXF export are per unit type (report by_unit; DXF extended data KADAKAS_UNIT on every unit polyline; unit_type on the objects of unit_pattern.to_objects). The four bonds are built from the same machinery. Ways to define a module: JSON (pattern: {kind: module, units: [{name, outline}], a, b, gap}), a preset (unit_pattern.module_preset: hexagon, octagon_square, i_shape), closed shapes in the drawing (unit_pattern.module_from_shapes), or a block of a DXF file (unit_pattern.module_from_block). The same JSON is the joints of a hardscape style: hardscape_style.create {joints: {kind: module, units, a, b, spacing}}.",
        "generators": "A generator pattern (PAT-09) lays unit shapes that a generator made. The pattern stores the last result of the generator, so drawing, reports, unit_pattern.check, to_objects and the DXF export use the stored shapes and run no code; a document holds no program. In a call: pattern: {kind: generator, types: [\"name\", ...], rects: {kind: [type index per unit], data: [cx, cy, length, width, angle in degrees per unit]}, polygons: {kind: [...], start: [n + 1 entries], points: [x, y, ...]}, label? (who made it), params? (what it was made with), gap}, or the same inside pattern.generator: {label, params, units: {types, rects, polygons}}. In a hardscape style: joints: {kind: generator, generator: {label, params, units}, spacing}. The shapes are NOT clipped and are in drawing coordinates: like the cells of a bond, they are the outlines at the centers of the joints, and the gap moves each inward by half the gap. Kadakas does the rest as for every other kind: clipping to the outline and the holes (of each zone and course too), whole and cut units, counts per type (the full area of a type is its largest cell), unit_pattern.check, reports, to_objects and the DXF export. At most limits.pattern_units shapes. To make a result an agent runs a program with script.eval (it may call objects.get and the like for the region) and passes the typed arrays to hardscape_style.update or unit_pattern.layout in the same program; regeneration is the agent's job: run the program again with other parameters. An installed package will make results later (PLAT-15). Example programs are in docs/scripting-javascript.md (a random ashlar of three sizes and a radial fan).",
        "workflow_parking_lot": [
            {"step": 1, "method": "import.file", "params": {"path": "parking.dxf"}, "why": "Read the survey of the lot."},
            {"step": 2, "method": "document.summary", "params": {}, "why": "The classes list gives the ID of the class of the lot layer."},
            {"step": 3, "method": "objects.list", "params": {"class": "<class ID>"}, "why": "The lines, arcs and polylines of the lot outline, the islands and the stall lines."},
            {"step": 4, "method": "hardscape_style.create", "params": {"name": "Pavers 200x100", "thickness": 80, "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 5}, "border": {"width": 150, "material": "Kerb"}}},
            {"step": 5, "method": "region.from_curves", "params": {"ids": ["<outline and island ids>"], "tolerance": 2, "dry_run": true}, "why": "Joins loose curves into closed regions. Gaps up to the tolerance are closed and listed (use up to 10 mm); dead_ends say what does not close (position, length, nearest end). Read the regions: role area, island, or island? (doubtful: an island that lines join to the outline, or one that the closed shapes of the drawing call something else) and the warnings. Repeat with a fixed drawing or a larger tolerance until complete is true. Closed polylines can go to hardscape.from_shapes instead."},
            {"step": 6, "method": "region.from_curves", "params": {"ids": ["<outline and island ids>"], "tolerance": 2, "create": "hardscape", "style": "Pavers 200x100", "regions": [0]}, "why": "The lot becomes a hardscape; islands inside it become holes. Without regions it makes the regions with role area; list the regions you checked (an island? is only made when you list it)."},
            {"step": 7, "method": "hardscape.split", "params": {"id": "<hardscape id>", "lines": ["<stall line ids>", "<drive aisle line ids>"], "zones": [{"index": 0, "name": "Stalls", "angle": 90}]}, "why": "Zones for the stall rows and the drive aisle that share their edges exactly. Each zone can have its own pattern, origin and angle. Moving a followed line moves the zones."},
            {"step": 8, "method": "hardscape.course", "params": {"id": "<hardscape id>", "courses": [{"name": "Kerb", "along": "outline", "corner": "miter", "align_field": true}]}, "why": "A soldier course along the kerb. The field is cut by it with the joint kept, and starts at its edge."},
            {"step": 9, "method": "render.png", "params": {"view": "top"}, "why": "Look at the pattern before you count it."},
            {"step": 10, "method": "unit_pattern.check", "params": {"ids": ["<hardscape id>"]}, "why": "Overlaps, joints too narrow or too wide, small pieces, wrong sizes, units outside their area, coverage. Every finding has a position to draw."},
            {"step": 11, "method": "report.unit_pattern", "params": {"waste_percent": 5, "gap_length": true}, "why": "Whole, cut and sliver units, area laid, gap length and the order quantity, per zone and course (groups) and in total."},
            {"step": 12, "method": "export.file", "params": {"path": "parking-paving.dxf", "format": "dxf", "options": {"patterns": "blocks"}}, "why": "Whole units as INSERTs of one block per unit type, cut units as closed polylines, each zone and course on its own layers. patterns units writes every unit as a polyline. The counts equal report.unit_pattern."}
        ],
        "examples": [
            {"method": "unit_pattern.layout", "params": {"region": {"outline": [[0, 0], [6000, 0], [6000, 3000], [0, 3000]]}, "pattern": {"kind": "generator", "label": "ashlar", "types": ["300x200"], "rects": {"kind": [0, 0], "data": [150, 100, 300, 200, 0, 450, 100, 300, 200, 0]}, "gap": 10}, "origin": [0, 0], "limit": 0}},
            {"method": "unit_pattern.layout", "params": {"id": "00000000-0000-0000-0000-000000000010", "pattern": {"bond": "herringbone", "unit_length": 200, "unit_width": 100, "gap": 4}, "angle": 45, "include": ["units"], "limit": 100}},
            {"method": "unit_pattern.layout", "params": {"region": {"outline": [[0, 0], [8000, 0], [8000, 5000], [0, 5000]], "holes": [[[3000, 1500], [4500, 1500], [4500, 3000], [3000, 3000]]]}, "pattern": {"unit_length": 600, "unit_width": 300, "gap": 3, "bond": "stack"}, "origin": [0, 0]}},
            {"method": "unit_pattern.module_preset", "params": {"preset": "hexagon", "across_flats": 200}},
            {"method": "unit_pattern.layout", "params": {"id": "00000000-0000-0000-0000-000000000010", "pattern": {"preset": "i_shape", "length": 200, "width": 165, "notch": 25, "gap": 4}, "include": ["units"], "limit": 0}},
            {"method": "unit_pattern.module_from_shapes", "params": {"ids": ["00000000-0000-0000-0000-000000000011", "00000000-0000-0000-0000-000000000012"], "names": ["big", "small"], "a": [400, 0], "b": [0, 400]}},
            {"method": "unit_pattern.module_from_block", "params": {"path": "/data/tiles.dxf", "block": "MODULE_A"}},
            {"method": "unit_pattern.to_objects", "params": {"id": "00000000-0000-0000-0000-000000000010", "include": ["units", "gaps"], "name": "Paving units"}},
            {"method": "report.unit_pattern", "params": {"waste_percent": 5, "sliver_share": 0.25}}
        ],
    })
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !(method.starts_with("unit_pattern.") || method == "report.unit_pattern") {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "unit_pattern.layout" => layout_call(s, m),
        "unit_pattern.to_objects" => to_objects(s, m),
        "unit_pattern.module_preset" => module_preset(m),
        "unit_pattern.module_from_shapes" => module_from_shapes(s, m),
        #[cfg(not(target_arch = "wasm32"))]
        "unit_pattern.module_from_block" => module_from_block(s, m),
        "report.unit_pattern" => report(s, m),
        other => Err(RpcError::method_not_found(other)),
    })
}

/// What a call works on: the region, the pattern and where it starts.
struct Subject {
    /// The area of the whole subject (a hardscape inside its kerb border and outside its holes).
    region: Region,
    /// The parts that are laid on their own: the field, its zones and the courses of a hardscape
    /// (PAT-07, PAT-08). A subject without a plan has one group.
    groups: Vec<Group>,
    /// Things the person must read (a split line that stops short, ...).
    notes: Vec<String>,
    /// The layer and class of the object, when the subject is an object.
    layer: Option<LayerId>,
    class: Option<ClassId>,
}

impl Subject {
    fn planned(&self) -> bool {
        self.groups.len() > 1 || self.groups.iter().any(|g| g.role != Role::Field)
    }

    /// The names of the unit types of each group.
    fn names(&self) -> Vec<Vec<String>> {
        self.groups.iter().map(Group::kind_names).collect()
    }
}

fn number(v: &Value, what: &str) -> Result<f64, RpcError> {
    v.as_f64()
        .filter(|x| x.is_finite())
        .ok_or_else(|| bad(format!("'{what}' must be a number")))
}

fn point(v: &Value, what: &str) -> Result<DVec2, RpcError> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| bad(format!("'{what}' must be [x, y]")))?;
    Ok(DVec2::new(number(&a[0], what)?, number(&a[1], what)?))
}

fn tolerance(m: &Map<String, Value>) -> Result<f64, RpcError> {
    match m.get("tolerance") {
        None | Some(Value::Null) => Ok(TOLERANCE),
        Some(v) => {
            let t = number(v, "tolerance")?;
            if t > 0.0 {
                Ok(t)
            } else {
                Err(bad("'tolerance' must be a positive number"))
            }
        }
    }
}

fn module_json(m: &Module) -> Value {
    json!({ "kind": "module", "units": m.units, "a": m.a, "b": m.b })
}

/// A module from the JSON fields `units`, `a` and `b`.
fn module_of(g: &Map<String, Value>) -> Result<Module, RpcError> {
    let units: Vec<ModuleUnit> = serde_json::from_value(
        g.get("units")
            .cloned()
            .ok_or_else(|| bad("a module pattern needs 'units': [{name, outline}]"))?,
    )
    .map_err(|e| bad(format!("'units' must be [{{name, outline}}]: {e}")))?;
    let vec2 = |k: &str| -> Result<[f64; 2], RpcError> {
        let p = point(
            g.get(k)
                .ok_or_else(|| bad(format!("a module pattern needs the repeat vector '{k}'")))?,
            k,
        )?;
        Ok([p.x, p.y])
    };
    Ok(Module {
        units,
        a: vec2("a")?,
        b: vec2("b")?,
    })
}

/// The generator of a pattern from the call: `generator: {label?, params?, units: {types, rects?,
/// polygons?}}`, or the same fields at the top of the pattern (`label`, `params`, `types`, `rects`,
/// `polygons`).
fn generator_param(g: &Map<String, Value>) -> Result<layout::generator::GeneratorSpec, RpcError> {
    if let Some(v) = g.get("generator") {
        return serde_json::from_value(v.clone()).map_err(|e| {
            bad(format!(
                "'generator' must be {{label?, params?, units}}: {e}"
            ))
        });
    }
    let units: Map<String, Value> = ["types", "rects", "polygons"]
        .iter()
        .filter_map(|k| g.get(*k).map(|v| ((*k).to_string(), v.clone())))
        .collect();
    let units = serde_json::from_value(Value::Object(units)).map_err(|e| {
        bad(format!(
            "a generator pattern takes types, rects: {{kind, data}} and polygons: {{kind, start, points}}: {e}"
        ))
    })?;
    Ok(layout::generator::GeneratorSpec::new(
        g.get("label").and_then(Value::as_str).unwrap_or_default(),
        g.get("params").cloned().unwrap_or(Value::Null),
        units,
    ))
}

/// The pattern from the call, on top of a base (the pattern of the object).
fn pattern_param(m: &Map<String, Value>, base: Option<Pattern>) -> Result<Pattern, RpcError> {
    let given = match m.get("pattern") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_object()
                .ok_or_else(|| bad("'pattern' must be an object"))?,
        ),
    };
    let mut p = base.unwrap_or(Pattern::new(Bond::RunningBond, f64::NAN, f64::NAN, 0.0));
    if let Some(g) = given {
        let kind = g
            .get("bond")
            .or_else(|| g.get("kind"))
            .and_then(Value::as_str);
        let is_module = kind.is_some_and(|k| k.trim().eq_ignore_ascii_case("module"));
        let is_generator = kind.is_some_and(|k| k.trim().eq_ignore_ascii_case("generator"));
        if let Some(name) = g.get("preset").and_then(Value::as_str) {
            let num = |k: &str| g.get(k).and_then(Value::as_f64);
            let module = layout::module::preset(name, &num).map_err(bad)?;
            p.bond = Bond::Module;
            p.module = Some(module);
        } else if is_generator {
            p.bond = Bond::Generator;
            p.module = None;
            p.generator = Some(std::sync::Arc::new(generator_param(g)?));
        } else if is_module || g.contains_key("units") {
            p.bond = Bond::Module;
            p.module = Some(module_of(g)?);
        } else if let Some(k) = kind {
            p.bond = Bond::from_name(k).ok_or_else(|| {
                bad("'bond' must be running_bond, stack, herringbone, basket_weave, module or generator")
            })?;
            if p.bond == Bond::Module {
                return Err(bad(
                    "a module pattern needs 'units', 'a' and 'b' (or a preset)",
                ));
            }
            p.module = None;
            p.generator = None;
        }
        for (k, v) in g {
            match k.as_str() {
                "bond" | "kind" | "preset" | "units" | "a" | "b" => {}
                "label" | "params" | "types" | "rects" | "polygons" | "generator"
                    if is_generator => {}
                "unit_length" => p.unit_length = number(v, "unit_length")?,
                "unit_width" => p.unit_width = number(v, "unit_width")?,
                "gap" | "spacing" => p.gap = number(v, "gap")?,
                // The numbers of a preset.
                "side" | "across_flats" | "length" | "width" | "notch"
                    if g.contains_key("preset") => {}
                other => return Err(bad(format!("unknown pattern field '{other}'"))),
            }
        }
    }
    if !matches!(p.bond, Bond::Module | Bond::Generator) {
        if p.unit_length.is_nan() || p.unit_width.is_nan() {
            return Err(bad(
                "give a pattern with unit_length and unit_width (an object with a joint pattern needs none)",
            ));
        }
        if p.unit_length < Pattern::MIN_UNIT || p.unit_width < Pattern::MIN_UNIT {
            return Err(bad(format!(
                "units must be at least {} mm",
                Pattern::MIN_UNIT
            )));
        }
    }
    p.validate().map_err(bad)?;
    Ok(p)
}

/// The pattern of the field `pattern` of a call, on top of a base pattern.
pub(crate) fn pattern_from_params(
    m: &Map<String, Value>,
    base: Option<Pattern>,
) -> Result<Pattern, RpcError> {
    pattern_param(m, base)
}

/// What the check needs of an object: its area, its groups (field, zones, courses) and the notes.
pub(crate) fn subject_for_check(
    doc: &Document,
    o: &Object,
    m: &Map<String, Value>,
    max_units: usize,
) -> Result<(Region, Vec<Group>, Vec<String>), String> {
    let sub = subject_of(doc, o, m, max_units).map_err(|e| e.message)?;
    Ok((sub.region, sub.groups, sub.notes))
}

fn region_param(v: &Value, tol: f64) -> Result<(Region, Vec<Vertex>), RpcError> {
    let o = v
        .as_object()
        .ok_or_else(|| bad("'region' must be {outline, holes?}"))?;
    let outline: Vec<Vertex> = serde_json::from_value(
        o.get("outline")
            .cloned()
            .ok_or_else(|| bad("the region needs an 'outline'"))?,
    )
    .map_err(bad)?;
    let holes: Vec<Vec<Vertex>> = match o.get("holes") {
        None | Some(Value::Null) => Vec::new(),
        Some(h) => serde_json::from_value(h.clone()).map_err(bad)?,
    };
    let region = Region::new(&outline, &holes, tol).map_err(bad)?;
    Ok((region, outline))
}

fn subject_of(
    doc: &Document,
    o: &Object,
    m: &Map<String, Value>,
    max_units: usize,
) -> Result<Subject, RpcError> {
    let tol = tolerance(m)?;
    let region = region_of(doc, o, tol).map_err(bad)?;
    let own = pattern_of(doc, o);
    let (own_pattern, own_place) = match own {
        Some((p, pl)) => (Some(p), Some(pl)),
        None => (None, None),
    };
    let pattern = pattern_param(m, own_pattern)?;
    let place = placement(m, Some((doc, o)), own_place, &[])?;
    let planned =
        plan::plan_groups(doc, o, region.clone(), pattern, place, max_units).map_err(bad)?;
    Ok(Subject {
        region,
        groups: planned.groups,
        notes: planned.notes,
        layer: Some(o.layer),
        class: Some(o.class),
    })
}

fn placement(
    m: &Map<String, Value>,
    object: Option<(&Document, &Object)>,
    base: Option<Placement>,
    outline: &[Vertex],
) -> Result<Placement, RpcError> {
    let mut place = base.unwrap_or_default();
    if let Some(v) = m.get("align_to_edge").filter(|v| !v.is_null()) {
        let i = v
            .as_u64()
            .ok_or_else(|| bad("'align_to_edge' must be a vertex number"))?
            as usize;
        place = match object {
            Some((doc, o)) => layout::object::align_placement(doc, o, i).map_err(bad)?,
            None => Placement::along_edge(outline, i).map_err(bad)?,
        };
    }
    if let Some(v) = m.get("origin").filter(|v| !v.is_null()) {
        place.origin = Some(point(v, "origin")?);
    }
    if let Some(v) = m.get("angle").filter(|v| !v.is_null()) {
        place.angle = number(v, "angle")?;
    }
    Ok(place)
}

fn subject(s: &Session, m: &Map<String, Value>) -> Result<Subject, RpcError> {
    let doc = s.editor.doc();
    let max_units = s.limits().pattern_units.min(MAX_UNITS);
    match (m.get("id"), m.get("region")) {
        (Some(_), Some(_)) => Err(bad("give 'id' or 'region', not both")),
        (Some(id), None) => {
            let id: ObjectId = serde_json::from_value(id.clone()).map_err(bad)?;
            let o = doc
                .object(id)
                .ok_or_else(|| bad(format!("object {id} does not exist")))?;
            subject_of(doc, o, m, max_units)
        }
        (None, Some(r)) => {
            let (region, outline) = region_param(r, tolerance(m)?)?;
            let pattern = pattern_param(m, None)?;
            let place = placement(m, None, None, &outline)?;
            Ok(Subject {
                groups: vec![Group {
                    name: "field".into(),
                    slug: "FIELD".into(),
                    role: Role::Field,
                    region: region.clone(),
                    owner: None,
                    body: Body::Pattern { pattern, place },
                }],
                region,
                notes: Vec::new(),
                layer: None,
                class: None,
            })
        }
        (None, None) => Err(bad("give 'id' or 'region'")),
    }
}

fn include(m: &Map<String, Value>, default: &[&str]) -> Result<(bool, bool), RpcError> {
    let list: Vec<String> = match m.get("include") {
        None | Some(Value::Null) => default.iter().map(|s| s.to_string()).collect(),
        Some(v) => {
            serde_json::from_value(v.clone()).map_err(|_| bad("'include' must be a list"))?
        }
    };
    for k in &list {
        if k != "units" && k != "gaps" {
            return Err(bad(format!("'include' has units and gaps, not '{k}'")));
        }
    }
    Ok((
        list.iter().any(|k| k == "units"),
        list.iter().any(|k| k == "gaps"),
    ))
}

fn layout_error(e: LayoutError) -> RpcError {
    match e {
        LayoutError::TooMany(t) => RpcError::limit(format!(
            "{t}. Use larger units, a smaller region, or split the region"
        )),
        LayoutError::Scan(why) => RpcError::limit(why),
        LayoutError::Invalid(why) => bad(why),
    }
}

fn xy(p: DVec2) -> Value {
    json!([p.x, p.y])
}

fn unit_json(u: &Unit, names: &[String]) -> Value {
    json!({
        "kind": names.get(u.kind).cloned().unwrap_or_default(),
        "whole": u.whole,
        "area_m2": u.area / 1e6,
        "share": u.share,
        "outline": u.rings.first().map(|r| r.points.iter().map(|p| xy(*p)).collect::<Vec<_>>()).unwrap_or_default(),
        "holes": u.rings.iter().skip(1).map(|r| r.points.iter().map(|p| xy(*p)).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}

fn flag(m: &Map<String, Value>, k: &str) -> Result<bool, RpcError> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| bad(format!("'{k}' must be true or false"))),
    }
}

/// The count options of a call. `max_units` is the bound of the session.
pub(crate) fn summary_options(
    m: &Map<String, Value>,
    max_units: usize,
) -> Result<SummaryOptions, RpcError> {
    let mut o = SummaryOptions {
        max_units: max_units.min(MAX_UNITS),
        ..SummaryOptions::default()
    };
    let share = |k: &str| -> Result<Option<f64>, RpcError> {
        match m.get(k).filter(|v| !v.is_null()) {
            None => Ok(None),
            Some(v) => {
                let x = number(v, k)?;
                if (0.0..=1.0).contains(&x) {
                    Ok(Some(x))
                } else {
                    Err(bad(format!("'{k}' must be between 0 and 1")))
                }
            }
        }
    };
    if let Some(x) = share("sliver_share")? {
        o.sliver_share = x;
    }
    if let Some(x) = share("unfillable_share")? {
        o.unfillable_share = x;
    }
    if let Some(v) = m.get("min_width").filter(|v| !v.is_null()) {
        let x = number(v, "min_width")?;
        if x < 0.0 {
            return Err(bad("'min_width' must be zero or more"));
        }
        o.min_width = x;
    }
    if let Some(v) = m.get("trim").filter(|v| !v.is_null()) {
        let x = number(v, "trim")?;
        if !(0.0..=100.0).contains(&x) {
            return Err(bad("'trim' must be between 0 and 100 mm"));
        }
        o.trim = x;
    }
    if let Some(v) = m.get("max_extra_joint_mm").filter(|v| !v.is_null()) {
        let x = number(v, "max_extra_joint_mm")?;
        if !(0.0..=1000.0).contains(&x) {
            return Err(bad("'max_extra_joint_mm' must be between 0 and 1000"));
        }
        o.max_extra_joint_mm = x;
    }
    o.exclude_unfillable = flag(m, "exclude_unfillable")?;
    o.gap_length = flag(m, "gap_length")?;
    if let Some(v) = m.get("waste_percent").filter(|v| !v.is_null()) {
        let x = number(v, "waste_percent")?;
        if !(0.0..=100.0).contains(&x) {
            return Err(bad("'waste_percent' must be between 0 and 100"));
        }
        o.waste_percent = Some(x);
    }
    Ok(o)
}

/// The counts of every group, and of the whole subject. The units of all groups together stay
/// under the limit of the options.
fn summarize_all(
    sub: &Subject,
    opts: &SummaryOptions,
) -> Result<(Summary, Vec<Summary>), LayoutError> {
    let mut parts: Vec<Summary> = Vec::with_capacity(sub.groups.len());
    let mut left = opts.max_units;
    for g in &sub.groups {
        let o = SummaryOptions {
            max_units: left,
            ..*opts
        };
        let part = g.summarize(&o)?;
        left = left.saturating_sub(part.whole_units + part.cut_units);
        parts.push(part);
    }
    let total = if parts.len() == 1 {
        parts[0].clone()
    } else {
        plan::merge(&parts, sub.region.area, opts)
    };
    Ok((total, parts))
}

/// A group with its counts, as JSON.
fn group_json(g: &Group, sum: &Summary) -> Value {
    let mut v = serde_json::to_value(sum).expect("json");
    v["name"] = json!(g.name);
    v["role"] = json!(g.role);
    v["layer_suffix"] = json!(g.slug);
    if let Body::Course(c) = &g.body {
        // The area of a course is the strip of its units, not the area of the hardscape.
        let strip = c.length * c.depth / 1e6;
        v["region_area_m2"] = json!(strip);
        v["gap_area_m2"] = json!((strip - sum.area_laid_m2).max(0.0));
        v["course"] = json!({
            "length_m": c.length / 1000.0,
            "corners": c.corners,
            "joints": {
                "count": c.joints.count,
                "narrowest_mm": (c.joints.count > 0).then_some(c.joints.narrowest),
                "widest_mm": (c.joints.count > 0).then_some(c.joints.widest),
            },
        });
    }
    v
}

fn layout_call(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let sub = subject(s, m)?;
    let (want_units, want_gaps) = include(m, &["units", "gaps"])?;
    let offset = m.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = (m.get("limit").and_then(Value::as_u64).unwrap_or(1000) as usize).min(20_000);
    let opts = summary_options(m, s.limits().pattern_units)?;
    let (sum, parts) = summarize_all(&sub, &opts).map_err(layout_error)?;
    let all_names = sub.names();
    let planned = sub.planned();
    let mut out = json!({ "summary": sum });
    if planned {
        out["groups"] = json!(
            sub.groups
                .iter()
                .zip(&parts)
                .map(|(g, p)| group_json(g, p))
                .collect::<Vec<_>>()
        );
    }
    if !sub.notes.is_empty() {
        out["notes"] = json!(sub.notes);
    }
    let mut next: Option<usize> = None;
    if want_units {
        let mut page = Vec::new();
        let mut i = 0usize;
        let mut left = opts.max_units;
        for (gi, g) in sub.groups.iter().enumerate() {
            let limits = Limits {
                max_units: left,
                trim: opts.trim,
            };
            let before = i;
            g.for_each_unit(&limits, &mut |u| {
                if i >= offset && page.len() < limit {
                    let mut v = unit_json(u, &all_names[gi]);
                    if planned {
                        v["group"] = json!(g.name);
                    }
                    page.push(v);
                }
                i += 1;
            })
            .map_err(layout_error)?;
            left = left.saturating_sub(i - before);
        }
        if limit > 0 && offset + page.len() < i {
            next = Some(offset + page.len());
        }
        out["units_total"] = json!(i);
        out["units"] = json!(page);
    }
    if want_gaps {
        let segments = gap_segments(&sub, MAX_GAP_SEGMENTS)?;
        let total = segments.len();
        let page: Vec<Value> = segments
            .iter()
            .skip(offset)
            .take(limit)
            .map(|(a, b)| json!([xy(*a), xy(*b)]))
            .collect();
        if limit > 0 && offset + page.len() < total {
            next = Some(next.map_or(offset + page.len(), |n| n.min(offset + page.len())));
        }
        out["gaps_total"] = json!(total);
        out["gaps"] = json!(page);
    }
    out["next_offset"] = json!(next);
    Ok(out)
}

/// The gap center lines of all groups that have a pattern (a course has none).
fn gap_segments(sub: &Subject, max: usize) -> Result<Vec<layout::Segment>, RpcError> {
    let mut all: Vec<layout::Segment> = Vec::new();
    for g in &sub.groups {
        let Body::Pattern { pattern, place } = &g.body else {
            continue;
        };
        let got = gaps::gap_lines(
            &g.region,
            pattern,
            place,
            max.saturating_sub(all.len()),
            false,
        );
        if got.truncated {
            return Err(RpcError::limit(
                "the pattern has too many gap lines. Use larger units or a smaller region",
            ));
        }
        all.extend(got.segments);
    }
    Ok(all)
}

/// Most objects that `unit_pattern.to_objects` makes in one call.
const MAX_OBJECTS: usize = 100_000;

fn to_objects(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let sub = subject(s, m)?;
    let (want_units, want_gaps) = include(m, &["units"])?;
    let layer = match m.get("layer").filter(|v| !v.is_null()) {
        Some(v) => Some(LayerId(
            v.as_str()
                .and_then(kadakas_core::uid::Uid::parse)
                .ok_or_else(|| bad("'layer' must be an ID"))?,
        )),
        None => sub.layer,
    };
    let class = match m.get("class").filter(|v| !v.is_null()) {
        Some(v) => Some(ClassId(
            v.as_str()
                .and_then(kadakas_core::uid::Uid::parse)
                .ok_or_else(|| bad("'class' must be an ID"))?,
        )),
        None => sub.class,
    };
    let group = m.get("group").and_then(Value::as_bool).unwrap_or(true);
    let name = m.get("name").and_then(Value::as_str).map(str::to_string);
    // (shape, unit type)
    let mut shapes: Vec<(Shape, Option<String>)> = Vec::new();
    let (mut units, mut gap_count) = (0usize, 0usize);
    let all_names = sub.names();
    if want_units {
        let mut left = MAX_OBJECTS.min(s.limits().pattern_units);
        for (gi, g) in sub.groups.iter().enumerate() {
            let tag = match &g.body {
                Body::Pattern { pattern, .. } => pattern.module.is_some(),
                Body::Course(_) => false,
            };
            let limits = Limits::new(left);
            let before = units;
            g.for_each_unit(&limits, &mut |u| {
                units += 1;
                for r in &u.rings {
                    shapes.push((
                        Shape::Polyline {
                            points: kadakas_core::curve::corners(&r.points),
                            closed: true,
                        },
                        (tag && !r.hole)
                            .then(|| all_names[gi].get(u.kind).cloned().unwrap_or_default()),
                    ));
                }
            })
            .map_err(|e| match e {
                LayoutError::TooMany(t) => RpcError::limit(format!(
                    "{t}. One call makes at most {MAX_OBJECTS} objects: split the region"
                )),
                other => layout_error(other),
            })?;
            left = left.saturating_sub(units - before);
        }
    }
    if want_gaps {
        let segments = gap_segments(&sub, MAX_OBJECTS).map_err(|_| {
            RpcError::limit(format!(
                "the gaps are more than {MAX_OBJECTS} lines: split the region or leave the gaps out"
            ))
        })?;
        gap_count = segments.len();
        shapes.extend(
            segments
                .iter()
                .map(|(a, b)| (Shape::Line { a: *a, b: *b }, None)),
        );
    }
    if shapes.len() > MAX_OBJECTS {
        return Err(RpcError::limit(format!(
            "the pattern makes {} objects. One call makes at most {MAX_OBJECTS}: split the region",
            shapes.len()
        )));
    }
    if shapes.is_empty() {
        return Err(bad("the layout has no units"));
    }
    let n = shapes.len();
    let mut next = s.editor.doc().id_plan();
    let mut commands: Vec<Command> = Vec::with_capacity(n + 1);
    let mut ids: Vec<ObjectId> = Vec::with_capacity(n);
    for (shape, kind) in shapes {
        commands.push(Command::AddObject {
            shape,
            layer,
            class,
            attrs: Attributes::default(),
        });
        let id = ObjectId(next.take());
        ids.push(id);
        if let Some(k) = kind {
            let mut set = std::collections::BTreeMap::new();
            set.insert("unit_type".to_string(), json!(k));
            commands.push(Command::SetData {
                id,
                set: set.into_iter().collect(),
                remove: Vec::new(),
            });
        }
    }
    let group_id = if group && n > 1 {
        commands.push(Command::Group {
            ids: ids.clone(),
            name,
        });
        Some(ObjectId(next.take()))
    } else {
        None
    };
    s.editor
        .execute(&Command::Batch { commands })
        .map_err(RpcError::from)?;
    s.selection = group_id.map_or(ids, |g| vec![g]);
    Ok(json!({ "group": group_id, "units": units, "gaps": gap_count, "objects": n }))
}

fn module_preset(m: &Map<String, Value>) -> Result<Value, RpcError> {
    let name = m.get("preset").and_then(Value::as_str).ok_or_else(|| {
        bad(format!(
            "give 'preset': {}",
            layout::module::PRESETS.join(" | ")
        ))
    })?;
    for k in m.keys() {
        if !["preset", "side", "across_flats", "length", "width", "notch"].contains(&k.as_str()) {
            return Err(bad(format!("unknown field '{k}'")));
        }
    }
    let num = |k: &str| m.get(k).and_then(Value::as_f64);
    let module = layout::module::preset(name, &num).map_err(bad)?;
    module.compile(0.0).map_err(bad)?;
    Ok(json!({
        "pattern": module_json(&module),
        "note": "add gap (or spacing in a hardscape style) for the joints",
    }))
}

fn module_from_shapes(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let doc = s.editor.doc();
    let ids: Vec<ObjectId> = match m.get("ids").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(bad)?,
        None => s.selection.clone(),
    };
    if ids.is_empty() {
        return Err(bad("select closed shapes or give ids"));
    }
    let names: Vec<String> = match m.get("names").filter(|v| !v.is_null()) {
        Some(v) => {
            serde_json::from_value(v.clone()).map_err(|_| bad("'names' must be a list of text"))?
        }
        None => Vec::new(),
    };
    if !names.is_empty() && names.len() != ids.len() {
        return Err(bad("give one name per shape, or no names"));
    }
    let mut units = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let o = doc
            .object(*id)
            .ok_or_else(|| bad(format!("object {id} does not exist")))?;
        let (outline, _) = outline_of(o).map_err(|e| bad(format!("object {id}: {e}")))?;
        units.push(ModuleUnit {
            name: names
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("unit {}", i + 1)),
            outline,
        });
    }
    finish_module(units, m)
}

fn finish_module(units: Vec<ModuleUnit>, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let vec = |k: &str| -> Result<Option<[f64; 2]>, RpcError> {
        match m.get(k).filter(|v| !v.is_null()) {
            None => Ok(None),
            Some(v) => {
                let p = point(v, k)?;
                Ok(Some([p.x, p.y]))
            }
        }
    };
    let (module, note) = layout::module::from_outlines(units, vec("a")?, vec("b")?).map_err(bad)?;
    module.compile(0.0).map_err(bad)?;
    Ok(json!({ "pattern": module_json(&module), "note": note }))
}

#[cfg(not(target_arch = "wasm32"))]
fn module_from_block(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let path = m
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("give 'path' of a DXF file"))?;
    let block = m
        .get("block")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("give the name of the 'block'"))?;
    for k in m.keys() {
        if !["path", "block", "a", "b"].contains(&k.as_str()) {
            return Err(bad(format!("unknown field '{k}'")));
        }
    }
    let bytes = s.read_dxf_for_module(path.into())?;
    let outlines =
        kadakas_io::dxf::block::block_outlines(&bytes, block).map_err(|e| bad(e.to_string()))?;
    let units = outlines
        .into_iter()
        .map(|b| ModuleUnit {
            name: b.layer,
            outline: b.outline,
        })
        .collect();
    finish_module(units, m)
}

fn label(doc: &Document, o: &Object) -> String {
    kadakas_core::hardscape::data::style_of(doc, o)
        .and_then(|(id, _)| doc.resources.get(id).map(|r| r.name.clone()))
        .unwrap_or_else(|| o.shape.kind().to_string())
}

/// The product of a row: the style (by ID, with its name for display), or a pattern given in the
/// call.
#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    pub id: Option<String>,
    pub name: String,
}

/// The product of an object whose pattern comes from its style, or `custom` when the call gives a
/// pattern.
fn product_of(doc: &Document, o: &Object, custom: bool) -> Product {
    if custom {
        return Product {
            id: None,
            name: "custom".into(),
        };
    }
    Product {
        id: kadakas_core::hardscape::data::style_id(o).map(|id| id.to_string()),
        name: label(doc, o),
    }
}

/// The sums of several summaries: one JSON for the total row. The waste is applied to the total
/// order, not to each row. `items` are the pattern label (the style, or `custom`) and the summary.
/// The top-level counts add the units of different products, so they are for the order of one
/// product only; `by_unit` never adds different products together: it is keyed by the pattern
/// label and the unit type (a bond names its unit type by its size, `200x100`).
pub fn totals_json(items: &[(Product, &Summary)], errors: usize, opts: &SummaryOptions) -> Value {
    let sums: Vec<&Summary> = items.iter().map(|(_, s)| *s).collect();
    let sum = |f: &dyn Fn(&Summary) -> usize| sums.iter().map(|s| f(s)).sum::<usize>();
    let order = sum(&|s| s.order_units);
    let mut by_unit: Vec<(Product, summary::UnitCount)> = Vec::new();
    for (product, s) in items {
        for u in &s.by_unit {
            match by_unit.iter_mut().find(|(p, x)| {
                x.name == u.name
                    && if p.id.is_some() || product.id.is_some() {
                        p.id == product.id
                    } else {
                        p.name == product.name
                    }
            }) {
                Some((_, x)) => {
                    x.whole_units += u.whole_units;
                    x.cut_units += u.cut_units;
                    x.slivers += u.slivers;
                    x.unfillable_pieces += u.unfillable_pieces;
                    x.area_laid_m2 += u.area_laid_m2;
                    x.order_units += u.order_units;
                    x.cut_share += u.cut_share;
                }
                None => by_unit.push((product.clone(), u.clone())),
            }
        }
    }
    let by_unit: Vec<Value> = by_unit
        .into_iter()
        .map(|(product, mut u)| {
            u.set_min();
            u.order_units_with_waste = opts.waste_percent.map(|w| with_waste(u.order_units, w));
            let mut v = serde_json::to_value(&u).expect("json");
            v["pattern"] = json!(product.name);
            v["pattern_id"] = json!(product.id);
            v
        })
        .collect();
    let products: std::collections::BTreeSet<String> = by_unit
        .iter()
        .map(|u| {
            u["pattern_id"]
                .as_str()
                .unwrap_or(u["pattern"].as_str().unwrap_or(""))
                .to_string()
        })
        .collect();
    let gap = sums.iter().map(|s| s.gap_length_m).collect::<Vec<_>>();
    json!({
        "whole_units": sum(&|s| s.whole_units),
        "cut_units": sum(&|s| s.cut_units),
        "slivers": sum(&|s| s.slivers),
        "unfillable_pieces": sum(&|s| s.unfillable_pieces),
        "area_laid_m2": sums.iter().map(|s| s.area_laid_m2).sum::<f64>(),
        "cut_area_m2": sums.iter().map(|s| s.cut_area_m2).sum::<f64>(),
        "gap_length_m": if !gap.is_empty() && gap.iter().all(Option::is_some) { json!(gap.iter().flatten().sum::<f64>()) } else { Value::Null },
        "order_units": order,
        "order_units_max": sum(&|s| s.order_units_max),
        "order_units_min": sum(&|s| s.order_units_min),
        "waste_percent": opts.waste_percent,
        "order_units_with_waste": opts.waste_percent.map(|w| with_waste(order, w)),
        "mixed_products": products.len() > 1,
        "by_unit": by_unit,
        "complete": errors == 0,
        "errors": errors,
    })
}

/// The summary of the pattern of one object, or why it has none. For a hardscape with zones or
/// courses it is the sum of all parts.
pub fn object_summary(
    doc: &Document,
    o: &Object,
    m: &Map<String, Value>,
    opts: &SummaryOptions,
) -> Result<Summary, String> {
    object_summary_groups(doc, o, m, opts).map(|r| r.0)
}

/// Like [`object_summary`], with the counts of each part (for a hardscape with zones or courses;
/// the list is empty for a plain one) and the notes of the plan.
pub fn object_summary_groups(
    doc: &Document,
    o: &Object,
    m: &Map<String, Value>,
    opts: &SummaryOptions,
) -> Result<(Summary, Vec<Value>, Vec<String>), String> {
    let sub = subject_of(doc, o, m, opts.max_units).map_err(|e| e.message)?;
    let (total, parts) = summarize_all(&sub, opts).map_err(|e| e.to_string())?;
    let groups = if sub.planned() {
        sub.groups
            .iter()
            .zip(&parts)
            .map(|(g, p)| group_json(g, p))
            .collect()
    } else {
        Vec::new()
    };
    Ok((total, groups, sub.notes))
}

/// The pattern block of one hardscape for `report.hardscape`: the summary as JSON, or an error.
/// `None` if the hardscape has no joint pattern.
pub fn block_for(
    doc: &Document,
    o: &Object,
    opts: &SummaryOptions,
) -> Option<Result<Summary, String>> {
    pattern_of(doc, o)?;
    Some(object_summary(doc, o, &Map::new(), opts))
}

/// The summary of the pattern of every hardscape (or the ids), as report rows and totals. A hardscape
/// or an id that cannot be counted is a row with an error, and the totals say `complete: false`.
pub fn report_rows(
    doc: &Document,
    ids: Option<Vec<ObjectId>>,
    m: &Map<String, Value>,
    opts: &SummaryOptions,
) -> Result<(Vec<Value>, Value), RpcError> {
    let visible_only = flag(m, "visible_only")?;
    let list: Vec<ObjectId> = ids.unwrap_or_else(|| {
        doc.objects
            .values()
            .filter(|o| matches!(o.shape, Shape::Hardscape { .. }) && pattern_of(doc, o).is_some())
            .filter(|o| !visible_only || doc.is_visible(o))
            .map(|o| o.id)
            .collect()
    });
    let mut rows = Vec::new();
    let mut good: Vec<(Product, Summary)> = Vec::new();
    let mut errors = 0;
    for id in list {
        let Some(o) = doc.object(id) else {
            errors += 1;
            rows.push(json!({"id": id, "error": "no such object"}));
            continue;
        };
        match object_summary_groups(doc, o, m, opts) {
            Ok((sum, groups, notes)) => {
                let mut row = serde_json::to_value(&sum).expect("json");
                row["id"] = json!(id);
                row["name"] = json!(label(doc, o));
                if !groups.is_empty() {
                    row["groups"] = json!(groups);
                }
                if !notes.is_empty() {
                    row["notes"] = json!(notes);
                }
                rows.push(row);
                good.push((product_of(doc, o, m.contains_key("pattern")), sum));
            }
            Err(e) => {
                errors += 1;
                rows.push(json!({"id": id, "name": label(doc, o), "error": e}));
            }
        }
    }
    let refs: Vec<(Product, &Summary)> = good.iter().map(|(p, s)| (p.clone(), s)).collect();
    Ok((rows, totals_json(&refs, errors, opts)))
}

fn report(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids: Option<Vec<ObjectId>> = match m.get("ids").filter(|v| !v.is_null()) {
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(bad)?),
        None => None,
    };
    let opts = summary_options(m, s.limits().pattern_units)?;
    let (rows, totals) = report_rows(s.editor.doc(), ids, m, &opts)?;
    let mut out = json!({ "rows": rows, "totals": totals });
    if m.get("format").and_then(Value::as_str) == Some("csv") {
        out["csv"] = json!(csv(&rows));
    }
    Ok(out)
}

fn csv(rows: &[Value]) -> String {
    let mut table = vec![
        [
            "Id",
            "Name",
            "Bond",
            "Unit type",
            "Whole units",
            "Cut units",
            "Slivers",
            "Unfillable",
            "Area laid (m2)",
            "Gap length (m)",
            "Order units",
            "Order with waste",
        ]
        .map(String::from)
        .to_vec(),
    ];
    let text = |r: &Value, k: &str| match &r[k] {
        Value::Null => String::new(),
        Value::String(x) => x.clone(),
        Value::Number(n) => n.as_f64().map_or(n.to_string(), |f| {
            if f.fract() == 0.0 {
                format!("{f:.0}")
            } else {
                format!("{f:.3}")
            }
        }),
        other => other.to_string(),
    };
    let mut flat: Vec<Value> = Vec::new();
    for r in rows {
        flat.push(r.clone());
        for g in r["groups"].as_array().into_iter().flatten() {
            let mut g = g.clone();
            g["id"] = r["id"].clone();
            g["name"] = json!(format!(
                "{} / {}",
                r["name"].as_str().unwrap_or(""),
                g["name"].as_str().unwrap_or("")
            ));
            flat.push(g);
        }
    }
    for r in &flat {
        let units = r["by_unit"].as_array().cloned().unwrap_or_default();
        if units.len() > 1 {
            for u in &units {
                table.push(vec![
                    text(r, "id"),
                    text(r, "name"),
                    text(r, "bond"),
                    text(u, "name"),
                    text(u, "whole_units"),
                    text(u, "cut_units"),
                    text(u, "slivers"),
                    text(u, "unfillable_pieces"),
                    text(u, "area_laid_m2"),
                    String::new(),
                    text(u, "order_units"),
                    String::new(),
                ]);
            }
        }
        table.push(
            [
                "id",
                "name",
                "bond",
                "",
                "whole_units",
                "cut_units",
                "slivers",
                "unfillable_pieces",
                "area_laid_m2",
                "gap_length_m",
                "order_units",
                "order_units_with_waste",
            ]
            .iter()
            .map(|k| {
                if k.is_empty() {
                    if units.len() > 1 {
                        "all".to_string()
                    } else {
                        String::new()
                    }
                } else {
                    text(r, k)
                }
            })
            .collect(),
        );
    }
    kadakas_core::csv::write_rows(&table)
}

#[cfg(test)]
mod tests;
