//! Agent API for regions from loose curves, zones, courses and the check of a pattern:
//! `region.from_curves`, `hardscape.split`, `hardscape.course`, `hardscape.course_remove`,
//! `hardscape.plan`, `hardscape.plan_clear` and `unit_pattern.check` (PAT-06, PAT-07, PAT-08,
//! PAT-10).
//!
//! Each method handles many items in one call: a script only coordinates.

use serde_json::{Map, Value, json};

use kadakas_core::DVec2;
use kadakas_core::command::Command;
use kadakas_core::curve::{self, Vertex};
use kadakas_core::hardscape::HardscapePatch;
use kadakas_core::landscape::LandscapePatch;
use kadakas_core::layout::check::{self, CheckOptions};
use kadakas_core::layout::course::{Corner, Side};
use kadakas_core::layout::object::region_of;
use kadakas_core::layout::plan::{self, CourseDef, Plan, SplitLine, ZoneSpec};
use kadakas_core::layout::summary::SummaryOptions;
use kadakas_core::model::{Attributes, ClassId, Document, LayerId, Object, ObjectId, Shape};
use kadakas_core::regions;

use crate::errors::bad;
use crate::{RpcError, Session};

pub const REGION_FROM_CURVES: (&str, &str) = (
    "region.from_curves",
    "Join loose lines, arcs, circles, rectangles and open or closed polylines into closed regions with holes (for example a parking lot drawn as separate lines and arcs). The curves are split where they cross; ends that lie within the tolerance of each other or of another curve are joined (a gap or a small overshoot is closed); dead ends are cut off; the areas that are left are traced. Nothing is guessed: a gap wider than the tolerance stays open, the area behind it is not made, and the dead ends are reported with their position, their length and the nearest other end, so that you can draw the place. A dead end is open (part of a shape that does not close) or a stub (a line that overshoots a corner or stops at a junction: harmless, but listed). params: {ids? (default: the selection), tolerance? (mm, default 1: the widest gap that is closed; use up to 10, above that closing a gap moves corners and warnings say so), create? (hardscape | landscape_area | polyline: make objects from the regions in one undo step; without it nothing is created), depths? (even | odd | all: which regions become objects; even means the regions with role area (the default for hardscape and landscape_area), odd the regions with role island, all every region including the doubtful ones (the default for polyline)), regions? ([region index or [x, y] point]: make exactly these regions, whatever their role), style? (hardscape style, ID or name, for create hardscape), name? (landscape area name), min_area_m2? (leave out smaller regions), layer?, class? (default: those of the first source object), delete_source? (delete a source only when every piece of it, also a piece that was cut off, lies in regions that are made and in no region that is left out; kept_sources lists the others with the reason), dry_run?}. Returns {regions: [{index, outline: [vertices] (counterclockwise; a point on each arc is a vertex of kind point_on_arc), holes: [[vertices]], area_m2, depth, role (area | island), inside: [x, y], bbox: {lo, hi}, sources: [object IDs]}], closed_gaps: {count, listed: [{at, size_mm}]}, dead_ends: {count, listed: [{at, length_mm, nearest: {at, distance_mm}}]}, complete (no dead ends), skipped: [{id, reason}], ignored_short, duplicates, created: [{region, id, kind}], tolerance_mm, dry_run}. A region inside a hole of another region is an island: its own region has an odd depth, and the outer region lists it as a hole, so create hardscape (the regions with role area) makes the lot with the island as a hole. An island that lines join to the outline (a stall line that ends on its kerb) is a face next to the lot, not a hole: its role is island? (also when the closed shapes of the drawing say a region is an island or an area against its lines), it has a doubt text, and create does not make it unless you list it in regions. The same holds for a circle, a closed shape or a closed chain of arcs inside the outline that lines end on (stall lines that run up to a round island). Look at the regions and warnings before you create.",
);
pub const HARDSCAPE_SPLIT: (&str, &str) = (
    "hardscape.split",
    "Split a hardscape (it needs a style with a joint pattern) into zones with lines (stall rows, a driveway, a fire lane): the zones share their edges exactly, so there are no slivers and no overlaps, and the areas of the zones add up to the area of the hardscape. Each zone can have a pattern, origin, angle or alignment of its own. The split lines and the zone settings are data of the hardscape (undoable, they move with it). A split line that follows a drawn line, arc or polyline (source) is read again each time: move the line and the zones follow. params: {id, lines? ([object IDs or [vertices]]: replaces the split lines; [] removes them; an ID follows the object unless follow is false), follow? (default true), zones? ([{index? (the index of the zone in the reply, counted from 0; the default name is zone N with N = index), seed? ([x, y]: a point inside the zone; the settings belong to the zone that holds it, also after a line moves), name?, pattern? (as the pattern of unit_pattern.layout), origin?, angle?, align_to_edge? (n: start at vertex n of the outline of the zone), align_to_split? (n: start at the first point of split line n and run along its first segment)}]: replaces the zone settings), tolerance? (mm, default 1: an end of a line this close to an edge joins it; the edge of the hardscape never moves)}. A zone without settings continues the pattern of the hardscape (the same lattice, cut at the zone edge). Returns {zones: [{index, name, area_m2, inside: [x, y], bbox: {lo, hi}, has_settings (a pattern, origin, angle or alignment of its own)}], split_lines, area_m2 (the hardscape), zones_area_m2 (the sum of the zones: the field), courses_area_m2 (what courses take: area less zones), zone_units, zone_edge_cuts ({whole_units_lost, extra_pieces}: the units that only the zone edges cut, when the zones continue the pattern), zone_edge_note, notes: [text]}. Every zone gets a setting with its name and a seed point, so a name stays with its place when a line moves. zone_units? (cut (default): a unit that a zone edge crosses is cut, and the two pieces touch with no joint, which is not buildable; centroid: the unit goes whole to the zone that holds its center, only for zones without a pattern of their own). A split line that stops short of the edge, or cuts nothing, is a note with its position and the gap. Counts per zone and in total are in report.unit_pattern (groups), the check in unit_pattern.check, the DXF has layers per zone.",
);
pub const HARDSCAPE_ZONE_SET: (&str, &str) = (
    "hardscape.zone_set",
    "Change the settings of one zone of a hardscape without touching the other zones (hardscape.split replaces them all). params: {id, zone (the index of the zone in the reply of hardscape.split or hardscape.plan, from 0), name?, pattern? (as the pattern of unit_pattern.layout), origin?, angle?, align_to_edge? (n: start at vertex n of the outline of the zone), align_to_split? (n: start at the first point of split line n), clear? ([pattern | origin | angle | align_to_edge | align_to_split]: remove these settings, so the zone takes the field's again)}. Settings that are not named stay as they are. Returns {zone, name, has_settings, zones: [{index, name, area_m2, has_settings}]}. One undo step. The Object Info panel of the app calls this method.",
);
pub const HARDSCAPE_COURSE: (&str, &str) = (
    "hardscape.course",
    "Add or replace courses along a path: a row of units along a polyline or a curve, on one side of the line or centered (a soldier course along a kerb, a border round an island). The course takes its area from the pattern next to it: the field is cut by the course with a joint of the gap kept, so they never overlap. The units are counted, reported and exported like the others (the unit type has the name of the stone, so a soldier course and the field of the same stone are added up). Corner rules: miter (the units of both legs are cut at the mitre line), butt (the first leg runs on to the end of the corner, the second butts against it), corner_unit (one unit of its own fills the corner). On a curve the units stay straight and are placed along the curve; the joints taper, and the reply and the report give the narrowest and the widest joint. params: {id, courses: [{name, path? ([vertices], curves allowed) | source? (a line, arc or polyline: followed, read again each time) | along? (outline | \"hole:N\": the edge of the hardscape or of its Nth hole, inside the kerb border), side? (left | right | center | inside | outside; default for outline: inside, for a hole: outside), offset? (mm to the left of the path), closed?, orientation? (soldier: the long side of the stone across the path (default) | stretcher: along it), unit_length? (mm, along the path) and unit_width? (mm, across) (default: from the stone of the style by orientation), gap? (default: the joint of the style), corner? (default miter), unit? (name of the unit type, default the size of the stone of the style), align_field? (true: the pattern of the hardscape starts at the inner edge of the course, so the first row of the field is whole and the joint to the course is the gap; sets the origin and the angle of the hardscape; needs a bond, not a module)}] (or the fields of one course directly in params)}. A course with the same name is replaced. Returns {courses: [{name, units, corners, length_m, joints: {count, narrowest_mm, widest_mm}}], notes}. A corner that turns by more than 150 degrees has no rule: on the inside both legs are cut at the bisector, on the outside both end flush and the wedge stays unpaved; a note says so. Corners under 30 degrees are always mitres. When the path is along: outline or hole:N the path is made again from the edge each time, so a change of the outline moves the course; a course that overlaps an earlier course is cut where they meet (a note says so).",
);
pub const HARDSCAPE_COURSE_REMOVE: (&str, &str) = (
    "hardscape.course_remove",
    "Remove courses from a hardscape. params: {id, names? ([text]), all? (true)}. The field gets its area back.",
);
pub const HARDSCAPE_PLAN: (&str, &str) = (
    "hardscape.plan",
    "Read the zones and courses of a hardscape: the plan (split lines, zone settings, courses), the zones as they are now (areas, a point inside, the outline), the courses (units, corners, length, joints) and the notes (a split line that stops short, a setting that no zone holds). params: {id, outlines? (true: include the outline of each zone)}. Returns {plan, zones: [{index, name, area_m2, inside, bbox, has_settings, outline?}], courses, groups: [{name, role, layer_suffix, area_m2}], notes, area_m2}.",
);
pub const HARDSCAPE_PLAN_CLEAR: (&str, &str) = (
    "hardscape.plan_clear",
    "Remove the zones and courses of a hardscape in one undo step. params: {id | ids, what? (all (default) | splits | zones | courses): splits removes the split lines and the zone settings, zones the zone settings only}.",
);
pub const UNIT_PATTERN_CHECK: (&str, &str) = (
    "unit_pattern.check",
    "Check the units of hardscapes (with their zones and courses) for the faults that cost money on site, with positions so that you can draw the places. The units are laid and tested as they are: overlaps between units (also a course against the field and a zone against a zone), joints narrower or wider than a limit, pieces under a minimum size, whole units of the wrong size, units that reach out of their area (for example over a zone edge), and the area covered against the area (a group that is much more open than its joints explain has missing units). params: {ids? (default: every hardscape with a joint pattern), visible_only?, overlap_mm2? (default 1: smaller overlaps do not count), overlap_depth_mm? (default 0.15: thinner overlaps do not count; a unit that crosses an edge by less than the trim is kept whole), min_joint_mm? (default: the gap less 1), max_joint_mm? (default: the widest joint of the pattern plus 1), min_share? (default 0.05: a cut unit with less of a full unit is a small piece), min_width_mm? (default off), size_tol_mm? (default 0.05), coverage_tol? (default 0.02), trim? (default 0.1), max_findings? (per kind, default 200, at most 10000; the counts are exact), pattern?, origin?, angle?, align_to_edge?, tolerance? (as unit_pattern.layout, to check a variant)}. Returns {objects: [{id, name, ok, units, counts: {overlap, joint_too_narrow, joint_too_wide, small_piece, wrong_size, outside_area, coverage}, findings: [{kind, group, other_group?, at: [x, y], size, message}], truncated: [kinds], coverage: [{group, role, region_area_m2, units_area_m2, units, open_share, expected_open_share}], area_m2, units_area_m2, notes}], totals: {units, counts, ok, complete}}. size is the overlap in mm2, the joint in mm, the share of a full unit, the area outside in mm2, the difference in mm2, or the open share. Units of two zones that meet at the zone edge have no joint by design and are not reported. An object that cannot be checked is a row with an error and complete is false.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "regions_from_curves": "region.from_curves joins loose lines, arcs and polylines into closed regions with holes. It closes gaps up to the tolerance and tells you what does not close: every dead end with its position, its length and the nearest other end. A region that is complete has depth 0 (an area to fill) or an even depth; an island inside a hole has an odd depth. With create it makes hardscapes (the lot with its islands as holes), landscape areas or polygons in one undo step.",
        "zones": "hardscape.split cuts a hardscape into zones that share their edges exactly. The split lines (drawn lines can be followed) and the settings of each zone (pattern, origin, angle, alignment) are data of the hardscape. report.unit_pattern gives the counts per zone (groups) and in total; the DXF export puts each zone on its own layers.",
        "courses": "hardscape.course lays a row of units along a path (a soldier course along a kerb). The field is cut by it with the gap kept. Corner rules: miter, butt, corner_unit. On curves the joints taper and the widest and narrowest joint are reported.",
        "check": "unit_pattern.check lays the units and tests them: overlaps, joints too narrow or too wide, small pieces, wrong sizes, units outside their area, and the area covered. Every finding has a position.",
    })
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !matches!(
        method,
        "region.from_curves"
            | "hardscape.split"
            | "hardscape.course"
            | "hardscape.course_remove"
            | "hardscape.zone_set"
            | "hardscape.plan"
            | "hardscape.plan_clear"
            | "unit_pattern.check"
    ) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "region.from_curves" => from_curves(s, m),
        "hardscape.split" => split(s, m),
        "hardscape.course" => course(s, m),
        "hardscape.course_remove" => course_remove(s, m),
        "hardscape.zone_set" => zone_set(s, m),
        "hardscape.plan" => plan_get(s, m),
        "hardscape.plan_clear" => plan_clear(s, m),
        "unit_pattern.check" => check_call(s, m),
        other => Err(RpcError::method_not_found(other)),
    })
}

fn number(v: &Value, what: &str) -> Result<f64, RpcError> {
    v.as_f64()
        .filter(|x| x.is_finite())
        .ok_or_else(|| bad(format!("'{what}' must be a number")))
}

fn opt_number(m: &Map<String, Value>, k: &str) -> Result<Option<f64>, RpcError> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => number(v, k).map(Some),
    }
}

/// A whole number of zero or more, or nothing.
fn whole_number(m: &Map<String, Value>, k: &str) -> Result<Option<usize>, RpcError> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(|n| Some(n as usize))
            .ok_or_else(|| bad(format!("'{k}' must be a whole number of zero or more"))),
    }
}

fn flag(m: &Map<String, Value>, k: &str, default: bool) -> Result<bool, RpcError> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| bad(format!("'{k}' must be true or false"))),
    }
}

fn point(v: &Value, what: &str) -> Result<DVec2, RpcError> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| bad(format!("'{what}' must be [x, y]")))?;
    Ok(DVec2::new(number(&a[0], what)?, number(&a[1], what)?))
}

fn xy(p: DVec2) -> Value {
    json!([p.x, p.y])
}

fn object_id(v: &Value, what: &str) -> Result<ObjectId, RpcError> {
    serde_json::from_value(v.clone()).map_err(|_| bad(format!("'{what}' must be an object ID")))
}

fn hardscape<'a>(doc: &'a Document, m: &Map<String, Value>) -> Result<&'a Object, RpcError> {
    let id = object_id(
        m.get("id")
            .ok_or_else(|| bad("give the 'id' of a hardscape"))?,
        "id",
    )?;
    let o = doc
        .object(id)
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    if !matches!(o.shape, Shape::Hardscape { .. }) {
        return Err(bad(format!(
            "object {id} is a {}, not a hardscape",
            o.shape.kind()
        )));
    }
    if plan::has_unreadable_data(o) {
        return Err(bad(format!(
            "object {id} has a zone and course plan that cannot be read. Clear it with hardscape.plan_clear"
        )));
    }
    Ok(o)
}

fn unknown_fields(m: &Map<String, Value>, known: &[&str]) -> Result<(), RpcError> {
    for k in m.keys() {
        if !known.contains(&k.as_str()) {
            return Err(bad(format!("unknown field '{k}'")));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ region.from_curves

fn vertices_json(v: &[Vertex]) -> Value {
    serde_json::to_value(v).expect("json")
}

fn from_curves(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    unknown_fields(
        m,
        &[
            "ids",
            "tolerance",
            "create",
            "depths",
            "style",
            "name",
            "min_area_m2",
            "layer",
            "class",
            "delete_source",
            "dry_run",
            "regions",
        ],
    )?;
    let doc = s.editor.doc();
    let ids: Vec<ObjectId> = match m.get("ids").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| bad("'ids' must be a list of object IDs"))?,
        None => s.selection.clone(),
    };
    if ids.is_empty() {
        return Err(bad("select lines, arcs or polylines, or give ids"));
    }
    let tol = opt_number(m, "tolerance")?.unwrap_or(1.0);
    if !(tol > 0.0 && tol <= 1000.0) {
        return Err(bad("'tolerance' must be between 0 and 1000 mm"));
    }
    let create = match m.get("create").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => Some(
            v.as_str()
                .filter(|k| ["hardscape", "landscape_area", "polyline"].contains(k))
                .ok_or_else(|| bad("'create' must be hardscape, landscape_area or polyline"))?,
        ),
    };
    let depths = match m.get("depths").filter(|v| !v.is_null()) {
        None => {
            if create == Some("polyline") {
                "all"
            } else {
                "even"
            }
        }
        Some(Value::String(d)) if ["even", "odd", "all"].contains(&d.as_str()) => {
            ["even", "odd", "all"]
                .into_iter()
                .find(|x| x == d)
                .unwrap_or("even")
        }
        Some(_) => return Err(bad("'depths' must be even, odd or all")),
    };
    let min_area = opt_number(m, "min_area_m2")?.unwrap_or(0.0) * 1e6;
    let dry_run = flag(m, "dry_run", false)?;
    let delete_source = flag(m, "delete_source", false)?;
    // The curves, with the object each comes from.
    let mut curves = Vec::new();
    let mut owner: Vec<ObjectId> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut used_objects: Vec<ObjectId> = Vec::new();
    // The closed shapes among the objects: they say what is inside what, as the outlines of
    // hardscape.from_shapes do.
    let mut loops: Vec<Vec<DVec2>> = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(o) = doc.object(id) else {
            skipped.push(json!({"id": id, "reason": "no such object"}));
            continue;
        };
        if let Ok((outer, _)) = kadakas_core::layout::object::outline_of(o)
            && !matches!(
                o.shape,
                Shape::Hardscape { .. } | Shape::LandscapeArea { .. }
            )
        {
            loops.push(kadakas_core::geom::open_ring(
                &curve::flatten(&outer, true, 0.5),
                1e-9,
            ));
        }
        match regions::curves_of(&o.shape) {
            Ok(cs) => {
                owner.extend(std::iter::repeat_n(id, cs.len()));
                curves.extend(cs);
                used_objects.push(id);
            }
            Err(e) => skipped.push(json!({"id": id, "reason": e})),
        }
    }
    if curves.is_empty() {
        return Err(bad("none of the objects has curves to join"));
    }
    let found = regions::from_curves(&curves, tol).map_err(|e| RpcError::limit(e.to_string()))?;
    let sources_of = |r: &regions::FoundRegion| -> Vec<ObjectId> {
        let mut v: Vec<ObjectId> = r.curves.iter().map(|&c| owner[c]).collect();
        v.sort();
        v.dedup();
        v
    };
    // The role of a region: its depth in the drawing says area or island. Two things make that
    // doubtful: a region that lines join to an enclosing region on all sides (a stall line that
    // ends on the kerb of an island), and closed shapes that say something else.
    let mut notes: Vec<String> = Vec::new();
    let roles: Vec<(&str, Option<String>)> = found
        .regions
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let inside_loops = loops
                .iter()
                .filter(|l| kadakas_core::geom::point_in_polygon(r.inside, l))
                .count();
            let by_shapes = (inside_loops >= 1).then(|| (inside_loops - 1) % 2);
            let why = if r.depth % 2 == 1 {
                match by_shapes {
                    Some(0) => Some(format!(
                        "region {i} is inside a hole of another region, but the closed shapes say it is an area"
                    )),
                    _ => None,
                }
            } else if r.enclosed {
                Some(format!(
                    "region {i} is enclosed on all sides by one other region and joined to it by lines: it may be an island that touches the outline"
                ))
            } else if r.closed_loop {
                Some(format!(
                    "region {i} is a closed shape (or a closed chain of curves) inside the outline of the drawing, and lines end on it: it may be an island that the stall lines run up to"
                ))
            } else if by_shapes == Some(1) {
                Some(format!(
                    "region {i} lies inside {inside_loops} closed shapes, which says it is an island, but its lines make it an area"
                ))
            } else {
                None
            };
            let role = match (&why, r.depth % 2) {
                (Some(_), _) => "island?",
                (None, 1) => "island",
                _ => "area",
            };
            if let Some(w) = &why {
                notes.push(format!("{w}. Check it, and list the regions to create with 'regions'"));
            }
            (role, why)
        })
        .collect();
    let region_json: Vec<Value> = found
        .regions
        .iter()
        .enumerate()
        .map(|(i, r)| {
            json!({
                "index": i,
                "outline": vertices_json(&r.outer),
                "holes": r.holes.iter().map(|h| vertices_json(h)).collect::<Vec<_>>(),
                "area_m2": r.area / 1e6,
                "depth": r.depth,
                "role": roles[i].0,
                "doubt": roles[i].1,
                "inside": xy(r.inside),
                "bbox": {"lo": xy(r.lo), "hi": xy(r.hi)},
                "sources": sources_of(r),
            })
        })
        .collect();
    if tol > 10.0 {
        notes.push(format!(
            "the tolerance of {tol} mm is more than 10 mm: gaps that wide are usually drawing errors, and closing them moves the corners"
        ));
    }
    if found.chained_nodes > 0 {
        notes.push(format!(
            "{} nodes joined points more than the tolerance apart through a chain of points: look at the closed gaps",
            found.chained_nodes
        ));
    }
    let mut out = json!({
        "regions": region_json,
        "closed_gaps": {
            "count": found.closed_gap_count,
            "total_movement_mm": found.node_movement,
            "chained_nodes": found.chained_nodes,
            "listed": found.closed_gaps.iter().map(|g| json!({"at": xy(g.at), "size_mm": g.size, "moved_mm": g.moved})).collect::<Vec<_>>(),
        },
        "dead_ends": {
            "count": found.dead_end_count,
            "open": found.open_dead_ends,
            "stubs": found.dead_end_count - found.open_dead_ends,
            "listed": found.dead_ends.iter().map(|d| json!({
                "at": xy(d.at),
                "kind": if d.open { "open" } else { "stub" },
                "length_mm": d.length,
                "nearest": d.nearest.map(|(p, dist)| json!({"at": xy(p), "distance_mm": dist})),
            })).collect::<Vec<_>>(),
        },
        "complete": found.open_dead_ends == 0,
        "skipped": skipped,
        "ignored_short": found.ignored_short,
        "duplicates": found.duplicates,
        "dropped_slivers": found.dropped_slivers,
        "dropped_sliver_list": found.dropped_sliver_list.iter()
            .map(|s| json!({"at": xy(s.at), "area_mm2": s.area}))
            .collect::<Vec<_>>(),
        "warnings": notes,
        "tolerance_mm": tol,
        "dry_run": dry_run,
    });
    let Some(kind) = create else {
        out["created"] = json!([]);
        return Ok(out);
    };
    // Which regions become objects: those listed, else those of the depths asked for.
    let chosen: Vec<usize> = match m.get("regions").filter(|v| !v.is_null()) {
        Some(list) => {
            let list = list.as_array().ok_or_else(|| {
                bad("'regions' must be a list of region numbers or [x, y] points")
            })?;
            let mut v: Vec<usize> = Vec::new();
            for item in list {
                let i = match item {
                    Value::Number(n) => n
                        .as_u64()
                        .map(|n| n as usize)
                        .filter(|n| *n < found.regions.len())
                        .ok_or_else(|| {
                            bad(format!(
                                "there is no region {item}: the drawing has {} regions (numbered from 0)",
                                found.regions.len()
                            ))
                        })?,
                    Value::Array(_) => {
                        let pt = point(item, "regions")?;
                        found
                            .regions
                            .iter()
                            .position(|r| {
                                let outer = curve::flatten(&r.outer, true, 0.5);
                                kadakas_core::geom::point_in_polygon(pt, &outer)
                                    && !r.holes.iter().any(|h| {
                                        kadakas_core::geom::point_in_polygon(
                                            pt,
                                            &curve::flatten(h, true, 0.5),
                                        )
                                    })
                            })
                            .ok_or_else(|| bad(format!("no region holds the point {item}")))?
                    }
                    _ => {
                        return Err(bad(
                            "'regions' must be a list of region numbers or [x, y] points",
                        ));
                    }
                };
                if !v.contains(&i) {
                    v.push(i);
                }
            }
            v
        }
        None => found
            .regions
            .iter()
            .enumerate()
            .filter(|(i, r)| {
                r.area >= min_area
                    && match depths {
                        "even" => roles[*i].0 == "area",
                        "odd" => roles[*i].0 == "island",
                        _ => true,
                    }
            })
            .map(|(i, _)| i)
            .collect(),
    };
    let left_out: Vec<usize> = (0..found.regions.len())
        .filter(|i| !chosen.contains(i))
        .collect();
    let first = used_objects.first().and_then(|id| doc.object(*id));
    let layer = match m.get("layer").filter(|v| !v.is_null()) {
        Some(v) => Some(LayerId(
            v.as_str()
                .and_then(kadakas_core::uid::Uid::parse)
                .ok_or_else(|| bad("'layer' must be an ID"))?,
        )),
        None => first.map(|o| o.layer),
    };
    let class = match m.get("class").filter(|v| !v.is_null()) {
        Some(v) => Some(ClassId(
            v.as_str()
                .and_then(kadakas_core::uid::Uid::parse)
                .ok_or_else(|| bad("'class' must be an ID"))?,
        )),
        None => first.map(|o| o.class),
    };
    let style = match m.get("style").filter(|v| !v.is_null()) {
        Some(v) => Some(crate::hardscape_api::hardscape_style_ref(doc, v)?),
        None => None,
    };
    let name = m
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut commands: Vec<Command> = Vec::new();
    for &i in &chosen {
        let r = &found.regions[i];
        let holes = (!r.holes.is_empty()).then(|| r.holes.clone());
        commands.push(match kind {
            "hardscape" => Command::AddHardscape {
                boundary: Some(r.outer.clone()),
                patch: Box::new(HardscapePatch {
                    style,
                    holes,
                    ..Default::default()
                }),
                layer,
                class,
                attrs: Attributes::default(),
            },
            "landscape_area" => Command::AddLandscapeArea {
                boundary: r.outer.clone(),
                name: name.clone(),
                patch: Box::new(LandscapePatch {
                    holes,
                    ..Default::default()
                }),
                layer,
                class,
                attrs: Attributes::default(),
            },
            _ => Command::AddObject {
                shape: Shape::Polyline {
                    points: r.outer.clone(),
                    closed: true,
                },
                layer,
                class,
                attrs: Attributes::default(),
            },
        });
    }
    let mut created: Vec<Value> = Vec::new();
    let mut kept: Vec<Value> = Vec::new();
    if !commands.is_empty() {
        let mut next = doc.id_plan();
        let ids: Vec<ObjectId> = commands.iter().map(|_| ObjectId(next.take())).collect();
        for (&i, id) in chosen.iter().zip(&ids) {
            created.push(json!({"region": i, "id": id, "kind": kind}));
        }
        if delete_source {
            // A source goes only if every piece of it, also the pieces that were cut off, lies in
            // regions that are made, and in no region that is left out.
            let made: std::collections::HashSet<usize> = chosen
                .iter()
                .flat_map(|&i| found.regions[i].edges.iter().copied())
                .collect();
            let other: std::collections::HashSet<usize> = left_out
                .iter()
                .flat_map(|&i| found.regions[i].edges.iter().copied())
                .collect();
            let mut gone: Vec<ObjectId> = Vec::new();
            for &id in &used_objects {
                let mine: Vec<usize> = (0..owner.len()).filter(|&c| owner[c] == id).collect();
                let reason = if mine.iter().any(|&c| found.lost_curves[c]) {
                    Some("a piece of it is a dead end or too short, so it is not all in a region")
                } else if mine
                    .iter()
                    .any(|&c| found.curve_edges[c].iter().any(|e| other.contains(e)))
                {
                    Some("it also bounds a region that was not made")
                } else if mine.iter().any(|&c| {
                    found.curve_edges[c].is_empty()
                        || found.curve_edges[c].iter().any(|e| !made.contains(e))
                }) {
                    Some("a piece of it is not in a region that was made")
                } else {
                    None
                };
                match reason {
                    None => gone.push(id),
                    Some(why) => kept.push(json!({"id": id, "reason": why})),
                }
            }
            if !gone.is_empty() {
                commands.push(Command::DeleteObjects { ids: gone });
            }
        }
        if !dry_run {
            s.editor
                .execute(&Command::Batch { commands })
                .map_err(RpcError::from)?;
            s.prune_selection();
            s.selection = ids;
        }
    }
    out["created"] = json!(created);
    out["kept_sources"] = json!(kept);
    Ok(out)
}

// ------------------------------------------------------------------ plan helpers

fn current_plan(o: &Object) -> Plan {
    plan::data(o).unwrap_or_default()
}

/// A copy of the object with this plan, to lay the groups before anything is stored.
fn with_plan(o: &Object, p: &Plan) -> Object {
    let mut c = o.clone();
    c.data
        .insert(plan::KEY.into(), serde_json::to_value(p).expect("json"));
    c
}

fn plan_state(doc: &Document, o: &Object, max_units: usize) -> Result<plan::Planned, RpcError> {
    let Some(planned) =
        plan::groups_of(doc, o, kadakas_core::layout::TOLERANCE, max_units).map_err(bad)?
    else {
        return Err(bad(
            "the hardscape has no joint pattern: give it a style with joints first (hardscape.set)",
        ));
    };
    Ok(planned)
}

fn zones_json(p: &plan::Planned, with_outline: Option<&plan::Planned>) -> Vec<Value> {
    p.zones
        .iter()
        .enumerate()
        .map(|(i, z)| {
            let mut v = json!({
                "index": i,
                "name": z.name,
                "area_m2": z.area / 1e6,
                "inside": xy(z.inside),
                "bbox": {"lo": xy(z.lo), "hi": xy(z.hi)},
                "has_settings": z.has_setting,
            });
            if let Some(pl) = with_outline
                && let Some(g) = pl
                    .groups
                    .iter()
                    .find(|g| g.role == plan::Role::Zone && g.name == z.name)
                    .or_else(|| {
                        pl.groups
                            .iter()
                            .filter(|g| g.role != plan::Role::Course)
                            .nth(i)
                    })
            {
                v["outline"] = json!(
                    g.region
                        .shapes
                        .iter()
                        .map(|sh| sh
                            .iter()
                            .map(|r| r.iter().map(|q| xy(*q)).collect::<Vec<_>>())
                            .collect::<Vec<_>>())
                        .collect::<Vec<_>>()
                );
            }
            v
        })
        .collect()
}

fn course_json(p: &plan::Planned) -> Vec<Value> {
    p.courses
        .iter()
        .map(|c| {
            json!({
                "name": c.name,
                "units": c.units,
                "corners": c.corners,
                "length_m": c.length / 1000.0,
                "joints": {
                    "count": c.joints.count,
                    "narrowest_mm": (c.joints.count > 0).then_some(c.joints.narrowest),
                    "widest_mm": (c.joints.count > 0).then_some(c.joints.widest),
                },
            })
        })
        .collect()
}

fn store(s: &mut Session, o: &Object, p: &Plan) -> Result<(), RpcError> {
    let cmd = if p.is_empty() {
        Command::SetData {
            id: o.id,
            set: Default::default(),
            remove: vec![plan::KEY.to_string()],
        }
    } else {
        Command::SetData {
            id: o.id,
            set: [(
                plan::KEY.to_string(),
                serde_json::to_value(p).expect("json"),
            )]
            .into_iter()
            .collect(),
            remove: Vec::new(),
        }
    };
    s.editor.execute(&cmd).map(|_| ()).map_err(RpcError::from)
}

// ------------------------------------------------------------------ hardscape.split

fn split(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    unknown_fields(
        m,
        &["id", "lines", "follow", "zones", "tolerance", "zone_units"],
    )?;
    let doc = s.editor.doc();
    let o = hardscape(doc, m)?;
    let mut p = current_plan(o);
    let follow = flag(m, "follow", true)?;
    if let Some(v) = opt_number(m, "tolerance")? {
        p.tolerance = v;
    }
    if let Some(u) = m.get("zone_units").filter(|v| !v.is_null()) {
        p.zone_units = match u.as_str() {
            Some("cut") => plan::ZoneUnits::Cut,
            Some("centroid") => plan::ZoneUnits::Centroid,
            _ => return Err(bad("'zone_units' must be cut or centroid")),
        };
    }
    if let Some(lines) = m.get("lines").filter(|v| !v.is_null()) {
        let list = lines
            .as_array()
            .ok_or_else(|| bad("'lines' must be a list of object IDs or vertex lists"))?;
        p.splits = Vec::with_capacity(list.len());
        for (i, item) in list.iter().enumerate() {
            match item {
                Value::String(_) => {
                    let id = object_id(item, "lines")?;
                    let src = doc
                        .object(id)
                        .ok_or_else(|| bad(format!("line {}: object {id} does not exist", i)))?;
                    let (points, _) = plan::path_of(&src.shape).ok_or_else(|| {
                        bad(format!(
                            "line {}: object {id} is a {}, not a line, arc or polyline",
                            i,
                            src.shape.kind()
                        ))
                    })?;
                    p.splits.push(SplitLine {
                        source: follow.then_some(id),
                        points,
                    });
                }
                Value::Array(_) => {
                    let points: Vec<Vertex> = serde_json::from_value(item.clone())
                        .map_err(|e| bad(format!("line {}: {e}", i)))?;
                    if points.len() < 2 {
                        return Err(bad(format!("line {} needs 2 or more points", i)));
                    }
                    p.splits.push(SplitLine {
                        source: None,
                        points,
                    });
                }
                _ => {
                    return Err(bad(format!(
                        "line {} must be an object ID or a list of vertices",
                        i
                    )));
                }
            }
        }
    }
    // The zones that the lines make, to tie settings to them by number.
    let max_units = s.limits().pattern_units;
    let mut probe = p.clone();
    probe.zones.clear();
    let before = plan_state(doc, &with_plan(o, &probe), max_units)?;
    if let Some(zs) = m.get("zones").filter(|v| !v.is_null()) {
        let list = zs
            .as_array()
            .ok_or_else(|| bad("'zones' must be a list of zone settings"))?;
        p.zones = Vec::with_capacity(list.len());
        for (i, item) in list.iter().enumerate() {
            let z = item
                .as_object()
                .ok_or_else(|| bad(format!("zone {}: must be an object", i)))?;
            unknown_fields(
                z,
                &[
                    "index",
                    "seed",
                    "name",
                    "pattern",
                    "origin",
                    "angle",
                    "align_to_edge",
                    "align_to_split",
                ],
            )
            .map_err(|e| bad(format!("zone {}: {}", i, e.message)))?;
            let seed = match (z.get("index"), z.get("seed")) {
                (Some(_), Some(_)) => {
                    return Err(bad(format!("zone {}: give 'index' or 'seed', not both", i)));
                }
                (Some(ix), None) => {
                    let ix = ix
                        .as_u64()
                        .ok_or_else(|| bad(format!("zone {}: 'index' must be a number", i)))?
                        as usize;
                    before
                        .zones
                        .get(ix)
                        .map(|zi| zi.inside)
                        .ok_or_else(|| {
                            bad(format!(
                                "zone {}: there is no zone {ix}; the lines make {} zones (numbered from 0)",
                                i,
                                before.zones.len()
                            ))
                        })?
                }
                (None, Some(sd)) => point(sd, "seed")?,
                (None, None) => return Err(bad(format!("zone {}: give 'index' or 'seed'", i))),
            };
            let pattern = match z.get("pattern").filter(|v| !v.is_null()) {
                Some(v) => {
                    let mut pm = Map::new();
                    pm.insert("pattern".into(), v.clone());
                    Some(crate::pattern_api::pattern_from_params(&pm, None)?)
                }
                None => None,
            };
            p.zones.push(ZoneSpec {
                name: z.get("name").and_then(Value::as_str).map(str::to_string),
                seed,
                pattern,
                origin: match z.get("origin").filter(|v| !v.is_null()) {
                    Some(v) => Some(point(v, "origin")?),
                    None => None,
                },
                angle: opt_number(z, "angle")?,
                align_to_edge: whole_number(z, "align_to_edge")?,
                align_to_split: whole_number(z, "align_to_split")?,
            });
        }
    }
    p.validate().map_err(bad)?;
    let mut after = plan_state(doc, &with_plan(o, &p), max_units)?;
    // Every zone gets a setting with its name and a seed, so that its name stays with it when a
    // line moves. A setting without a pattern, origin, angle or alignment changes nothing else.
    if after.zones.len() > 1 {
        for z in &after.zones {
            if z.spec.is_none() {
                p.zones.push(ZoneSpec {
                    name: Some(z.name.clone()),
                    seed: z.inside,
                    ..Default::default()
                });
            }
        }
        p.validate().map_err(bad)?;
        after = plan_state(doc, &with_plan(o, &p), max_units)?;
    }
    let region = region_of(doc, o, kadakas_core::layout::TOLERANCE).map_err(bad)?;
    let o = o.clone();
    store(s, &o, &p)?;
    let zones_area: f64 = after.zones.iter().map(|z| z.area).sum();
    let cuts = after.zone_edge_cuts(&SummaryOptions::default());
    Ok(json!({
        "zones": zones_json(&after, None),
        "split_lines": p.splits.len(),
        "area_m2": region.area / 1e6,
        "zones_area_m2": zones_area / 1e6,
        "courses_area_m2": ((region.area - zones_area) / 1e6).max(0.0),
        "zone_units": p.zone_units,
        "zone_edge_cuts": cuts.map(|c| json!({"whole_units_lost": c.whole_lost, "extra_pieces": c.extra_pieces})),
        "zone_edge_note": "units of two zones meet at the zone edge with no joint: that is not buildable. Put the zone edge in a joint of the pattern, or use zone_units centroid to give each unit whole to the zone of its center",
        "notes": after.notes,
    }))
}

// ------------------------------------------------------------------ hardscape.zone_set

fn zone_set(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    unknown_fields(
        m,
        &[
            "id",
            "zone",
            "name",
            "pattern",
            "origin",
            "angle",
            "align_to_edge",
            "align_to_split",
            "clear",
        ],
    )?;
    let doc = s.editor.doc();
    let o = hardscape(doc, m)?;
    let index = whole_number(m, "zone")?.ok_or_else(|| bad("give 'zone'"))?;
    let max_units = s.limits().pattern_units;
    let mut p = current_plan(o);
    let before = plan_state(doc, o, max_units)?;
    let info = before.zones.get(index).ok_or_else(|| {
        bad(format!(
            "there is no zone {index}; the hardscape has {} zones (numbered from 0)",
            before.zones.len()
        ))
    })?;
    let i = match info.spec.filter(|i| *i < p.zones.len()) {
        Some(i) => i,
        None => {
            p.zones.push(ZoneSpec {
                seed: info.inside,
                ..Default::default()
            });
            p.zones.len() - 1
        }
    };
    let clear: Vec<String> = match m.get("clear").filter(|v| !v.is_null()) {
        None => Vec::new(),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| bad("'clear' must be a list of setting names"))?,
    };
    for c in &clear {
        if ![
            "pattern",
            "origin",
            "angle",
            "align_to_edge",
            "align_to_split",
        ]
        .contains(&c.as_str())
        {
            return Err(bad(format!("cannot clear '{c}'")));
        }
        if m.contains_key(c.as_str()) {
            return Err(bad(format!("'{c}' is set and cleared in one call")));
        }
    }
    let new_pattern = match m.get("pattern").filter(|v| !v.is_null()) {
        Some(v) => {
            let mut pm = Map::new();
            pm.insert("pattern".into(), v.clone());
            Some(crate::pattern_api::pattern_from_params(&pm, None)?)
        }
        None => None,
    };
    let new_origin = match m.get("origin").filter(|v| !v.is_null()) {
        Some(v) => Some(point(v, "origin")?),
        None => None,
    };
    let spec = &mut p.zones[i];
    if let Some(n) = m.get("name").and_then(Value::as_str) {
        let n = n.trim();
        spec.name = (!n.is_empty()).then(|| n.to_string());
    }
    if let Some(pt) = new_pattern {
        spec.pattern = Some(pt);
    }
    if let Some(o) = new_origin {
        spec.origin = Some(o);
    }
    if let Some(a) = opt_number(m, "angle")? {
        spec.angle = Some(a);
    }
    if let Some(n) = whole_number(m, "align_to_edge")? {
        spec.align_to_edge = Some(n);
        spec.align_to_split = None;
    }
    if let Some(n) = whole_number(m, "align_to_split")? {
        spec.align_to_split = Some(n);
        spec.align_to_edge = None;
    }
    for c in &clear {
        match c.as_str() {
            "pattern" => spec.pattern = None,
            "origin" => spec.origin = None,
            "angle" => spec.angle = None,
            "align_to_edge" => spec.align_to_edge = None,
            _ => spec.align_to_split = None,
        }
    }
    p.validate().map_err(bad)?;
    let after = plan_state(doc, &with_plan(o, &p), max_units)?;
    let o = o.clone();
    store(s, &o, &p)?;
    let zone = after.zones.get(index);
    Ok(json!({
        "zone": index,
        "name": zone.map(|z| z.name.clone()),
        "has_settings": zone.is_some_and(|z| z.has_setting),
        "zones": zones_json(&after, None),
        "notes": after.notes,
    }))
}

// ------------------------------------------------------------------ courses

fn course_from(
    doc: &Document,
    o: &Object,
    c: &Map<String, Value>,
    default_name: Option<&str>,
) -> Result<CourseDef, RpcError> {
    unknown_fields(
        c,
        &[
            "name",
            "path",
            "source",
            "along",
            "side",
            "offset",
            "closed",
            "orientation",
            "unit_length",
            "unit_width",
            "gap",
            "corner",
            "unit",
            "align_field",
        ],
    )?;
    let name = c
        .get("name")
        .and_then(Value::as_str)
        .or(default_name)
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| bad("a course needs a 'name'"))?
        .to_string();
    let fail = |e: RpcError| bad(format!("course '{name}': {}", e.message));
    // The stone of the style.
    let joints = kadakas_core::hardscape::data::style_of(doc, o).and_then(|(_, st)| st.joints);
    let bond = joints.as_ref().filter(|j| j.module().is_none());
    let soldier = match c.get("orientation").and_then(Value::as_str) {
        None | Some("soldier") => true,
        Some("stretcher") => false,
        Some(_) => return Err(fail(bad("'orientation' must be soldier or stretcher"))),
    };
    let sizes = |what: &str| -> Result<(f64, f64), RpcError> {
        let b = bond.ok_or_else(|| {
            bad(format!(
                "give {what}: the style has no bond to take the stone from"
            ))
        })?;
        Ok(if soldier {
            (b.unit_width, b.unit_length)
        } else {
            (b.unit_length, b.unit_width)
        })
    };
    let along = match opt_number(c, "unit_length").map_err(fail)? {
        Some(v) => v,
        None => sizes("'unit_length' and 'unit_width'").map_err(fail)?.0,
    };
    let depth = match opt_number(c, "unit_width").map_err(fail)? {
        Some(v) => v,
        None => sizes("'unit_length' and 'unit_width'").map_err(fail)?.1,
    };
    let gap = match opt_number(c, "gap").map_err(fail)? {
        Some(v) => v,
        None => joints.as_ref().map_or(5.0, |j| j.spacing),
    };
    let corner = match c.get("corner").and_then(Value::as_str) {
        None => Corner::Miter,
        Some(n) => Corner::from_name(n)
            .ok_or_else(|| fail(bad("'corner' must be miter, butt or corner_unit")))?,
    };
    let unit = match c.get("unit").and_then(Value::as_str) {
        Some(u) => u.to_string(),
        None => match bond {
            Some(b) => kadakas_core::layout::bond::size_name(b.unit_length, b.unit_width),
            None => String::new(),
        },
    };
    let side_text = c.get("side").and_then(Value::as_str);
    let offset = opt_number(c, "offset").map_err(fail)?.unwrap_or(0.0);
    let (path, closed, source, side);
    let mut edge: Option<String> = None;
    match (
        c.get("path").filter(|v| !v.is_null()),
        c.get("source").filter(|v| !v.is_null()),
        match c.get("along").filter(|v| !v.is_null()) {
            None => None,
            Some(Value::String(s)) => Some(s.as_str()),
            Some(_) => return Err(fail(bad("'along' must be outline or hole:N"))),
        },
    ) {
        (Some(p), None, None) => {
            path = serde_json::from_value::<Vec<Vertex>>(p.clone())
                .map_err(|e| fail(bad(format!("'path': {e}"))))?;
            closed = flag(c, "closed", false).map_err(fail)?;
            source = None;
            side = match side_text {
                None => Side::Left,
                Some(t) => Side::from_name(t)
                    .ok_or_else(|| fail(bad("'side' must be left, right or center")))?,
            };
        }
        (None, Some(src), None) => {
            let id = object_id(src, "source").map_err(fail)?;
            let so = doc
                .object(id)
                .ok_or_else(|| fail(bad(format!("object {id} does not exist"))))?;
            let (pts, cl) = plan::path_of(&so.shape).ok_or_else(|| {
                fail(bad(format!(
                    "object {id} is a {}, not a line, arc or polyline",
                    so.shape.kind()
                )))
            })?;
            path = pts;
            closed = cl;
            source = Some(id);
            side = match side_text {
                None => Side::Left,
                Some(t) => Side::from_name(t)
                    .ok_or_else(|| fail(bad("'side' must be left, right or center")))?,
            };
        }
        (None, None, Some(which)) => {
            // The path is made again from the edge each time; this copy is for when it cannot be.
            path = plan::edge_path(doc, o, which).map_err(|e| fail(bad(e)))?.0;
            closed = true;
            source = None;
            edge = Some(which.to_string());
            let outline = which == "outline";
            // The ring runs counterclockwise: the left is inside it. A course goes on the side of
            // the paving: inside the outline, outside a hole.
            side = match side_text {
                None => {
                    if outline {
                        Side::Left
                    } else {
                        Side::Right
                    }
                }
                Some("inside") => Side::Left,
                Some("outside") => Side::Right,
                Some(t) => Side::from_name(t).ok_or_else(|| {
                    fail(bad("'side' must be inside, outside, left, right or center"))
                })?,
            };
        }
        _ => {
            return Err(fail(bad(
                "give one of 'path', 'source' or 'along' (outline | hole:N)",
            )));
        }
    }
    Ok(CourseDef {
        name,
        source,
        edge,
        path,
        closed,
        side,
        offset,
        along,
        depth,
        gap,
        corner,
        unit,
    })
}

fn course(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let doc = s.editor.doc();
    let o = hardscape(doc, m)?;
    let list: Vec<Map<String, Value>> = match m.get("courses").filter(|v| !v.is_null()) {
        Some(v) => v
            .as_array()
            .ok_or_else(|| bad("'courses' must be a list"))?
            .iter()
            .map(|c| {
                c.as_object()
                    .cloned()
                    .ok_or_else(|| bad("a course must be an object"))
            })
            .collect::<Result<_, _>>()?,
        None => {
            let mut c = m.clone();
            c.remove("id");
            vec![c]
        }
    };
    if list.is_empty() {
        return Err(bad("give at least one course"));
    }
    let mut p = current_plan(o);
    for c in &list {
        let def = course_from(doc, o, c, None)?;
        match p
            .courses
            .iter_mut()
            .find(|x| x.name.eq_ignore_ascii_case(&def.name))
        {
            Some(slot) => *slot = def,
            None => p.courses.push(def),
        }
    }
    p.validate().map_err(bad)?;
    // The field starts from the edge of the course, so that its first row is whole.
    let mut aligned: Option<(DVec2, f64)> = None;
    if let Some(c) = list
        .iter()
        .find(|c| c.get("align_field").and_then(Value::as_bool) == Some(true))
    {
        let name = c.get("name").and_then(Value::as_str).unwrap_or("");
        let def = p
            .courses
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| bad("align_field needs a named course"))?;
        let width = kadakas_core::layout::object::pattern_of(doc, o)
            .filter(|(pt, _)| pt.module.is_none())
            .map(|(pt, _)| pt.unit_width)
            .ok_or_else(|| bad("align_field needs a joint pattern of a bond (not a module)"))?;
        let spec = plan::course_spec(doc, o, def, &mut Vec::new());
        aligned = kadakas_core::layout::course::field_origin(&spec, width);
    }
    let planned = plan_state(doc, &with_plan(o, &p), s.limits().pattern_units)?;
    let o = o.clone();
    if let Some((origin, angle)) = aligned {
        let cmd = Command::Batch {
            commands: vec![
                Command::SetHardscape {
                    ids: vec![o.id],
                    patch: HardscapePatch {
                        origin: Some(origin),
                        angle: Some(angle),
                        ..Default::default()
                    },
                },
                Command::SetData {
                    id: o.id,
                    set: [(
                        plan::KEY.to_string(),
                        serde_json::to_value(&p).expect("json"),
                    )]
                    .into_iter()
                    .collect(),
                    remove: Vec::new(),
                },
            ],
        };
        s.editor.execute(&cmd).map_err(RpcError::from)?;
    } else {
        store(s, &o, &p)?;
    }
    Ok(json!({
        "courses": course_json(&planned),
        "aligned": aligned.map(|(origin, angle)| json!({"origin": xy(origin), "angle_deg": angle})),
        "notes": planned.notes,
    }))
}

fn course_remove(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    unknown_fields(m, &["id", "names", "all"])?;
    let doc = s.editor.doc();
    let o = hardscape(doc, m)?;
    let mut p = current_plan(o);
    let before = p.courses.len();
    if flag(m, "all", false)? {
        p.courses.clear();
    } else {
        let names: Vec<String> = serde_json::from_value(
            m.get("names")
                .cloned()
                .ok_or_else(|| bad("give 'names' or 'all': true"))?,
        )
        .map_err(|_| bad("'names' must be a list of text"))?;
        for n in &names {
            if !p.courses.iter().any(|c| c.name.eq_ignore_ascii_case(n)) {
                return Err(bad(format!("the hardscape has no course '{n}'")));
            }
        }
        p.courses
            .retain(|c| !names.iter().any(|n| c.name.eq_ignore_ascii_case(n)));
    }
    store(s, &o.clone(), &p)?;
    Ok(json!({ "removed": before - p.courses.len(), "courses": p.courses.len() }))
}

fn plan_get(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    unknown_fields(m, &["id", "outlines"])?;
    let doc = s.editor.doc();
    let o = hardscape(doc, m)?;
    let p = current_plan(o);
    let planned = plan_state(doc, o, s.limits().pattern_units)?;
    let outlines = flag(m, "outlines", false)?;
    Ok(json!({
        "plan": p,
        "zones": zones_json(&planned, outlines.then_some(&planned)),
        "courses": course_json(&planned),
        "groups": planned.groups.iter().map(|g| json!({
            "name": g.name,
            "role": g.role,
            "layer_suffix": g.slug,
            "area_m2": g.region.area / 1e6,
        })).collect::<Vec<_>>(),
        "notes": planned.notes,
        "zone_edge_cuts": planned.zone_edge_cuts(&SummaryOptions::default()).map(|c| json!({"whole_units_lost": c.whole_lost, "extra_pieces": c.extra_pieces})),
        "area_m2": region_of(doc, o, kadakas_core::layout::TOLERANCE).map(|r| r.area / 1e6).unwrap_or(0.0),
    }))
}

fn plan_clear(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    unknown_fields(m, &["id", "ids", "what"])?;
    let what = m.get("what").and_then(Value::as_str).unwrap_or("all");
    if !["all", "splits", "zones", "courses"].contains(&what) {
        return Err(bad("'what' must be all, splits, zones or courses"));
    }
    let ids: Vec<ObjectId> = match (m.get("ids"), m.get("id")) {
        (Some(v), _) => serde_json::from_value(v.clone())
            .map_err(|_| bad("'ids' must be a list of object IDs"))?,
        (None, Some(v)) => vec![object_id(v, "id")?],
        _ => return Err(bad("give 'id' or 'ids'")),
    };
    let doc = s.editor.doc();
    let mut commands = Vec::new();
    for id in &ids {
        let mut one = Map::new();
        one.insert("id".into(), json!(id));
        let o = hardscape(doc, &one)?;
        let mut p = current_plan(o);
        match what {
            "all" => p = Plan::default(),
            "splits" => {
                p.splits.clear();
                p.zones.clear();
            }
            "zones" => p.zones.clear(),
            _ => p.courses.clear(),
        }
        commands.push(if p.is_empty() {
            Command::SetData {
                id: *id,
                set: Default::default(),
                remove: vec![plan::KEY.to_string()],
            }
        } else {
            Command::SetData {
                id: *id,
                set: [(
                    plan::KEY.to_string(),
                    serde_json::to_value(&p).expect("json"),
                )]
                .into_iter()
                .collect(),
                remove: Vec::new(),
            }
        });
    }
    s.editor
        .execute(&Command::Batch { commands })
        .map_err(RpcError::from)?;
    Ok(json!({ "ids": ids, "cleared": what }))
}

// ------------------------------------------------------------------ unit_pattern.check

fn check_options(m: &Map<String, Value>, max_units: usize) -> Result<CheckOptions, RpcError> {
    let mut o = CheckOptions {
        max_units: max_units.min(check::MAX_UNITS),
        ..Default::default()
    };
    let num = |k: &str, lo: f64, hi: f64| -> Result<Option<f64>, RpcError> {
        match opt_number(m, k)? {
            None => Ok(None),
            Some(x) if (lo..=hi).contains(&x) => Ok(Some(x)),
            Some(_) => Err(bad(format!("'{k}' must be between {lo} and {hi}"))),
        }
    };
    if let Some(x) = num("overlap_mm2", 0.0, 1e9)? {
        o.overlap_mm2 = x;
    }
    if let Some(x) = num("overlap_depth_mm", 0.0, 1000.0)? {
        o.overlap_depth_mm = x;
    }
    o.min_joint_mm = num("min_joint_mm", 0.0, 1000.0)?;
    o.max_joint_mm = num("max_joint_mm", 0.0, 10_000.0)?;
    if let Some(x) = num("min_share", 0.0, 1.0)? {
        o.min_share = x;
    }
    if let Some(x) = num("min_width_mm", 0.0, 10_000.0)? {
        o.min_width_mm = x;
    }
    if let Some(x) = num("size_tol_mm", 0.0, 1000.0)? {
        o.size_tol_mm = x;
    }
    if let Some(x) = num("coverage_tol", 0.0, 1.0)? {
        o.coverage_tol = x;
    }
    if let Some(x) = num("trim", 0.0, 100.0)? {
        o.trim = x;
    }
    if let Some(x) = num("max_findings", 0.0, 10_000.0)? {
        o.max_findings = x as usize;
    }
    Ok(o)
}

fn check_call(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let max_units = s.limits().pattern_units;
    let opts = check_options(m, max_units)?;
    let doc = s.editor.doc();
    let visible_only = flag(m, "visible_only", false)?;
    let ids: Vec<ObjectId> = match m.get("ids").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| bad("'ids' must be a list of object IDs"))?,
        None => doc
            .objects
            .values()
            .filter(|o| {
                matches!(o.shape, Shape::Hardscape { .. })
                    && kadakas_core::layout::object::pattern_of(doc, o).is_some()
            })
            .filter(|o| !visible_only || doc.is_visible(o))
            .map(|o| o.id)
            .collect(),
    };
    let mut rows: Vec<Value> = Vec::new();
    let mut totals: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let (mut units, mut errors) = (0usize, 0usize);
    let mut left = opts.max_units;
    for id in ids {
        let Some(o) = doc.object(id) else {
            errors += 1;
            rows.push(json!({"id": id, "error": "no such object"}));
            continue;
        };
        let name = plan_label(doc, o);
        let sub = crate::pattern_api::subject_for_check(doc, o, m, left);
        let result = sub.and_then(|(region, groups, notes)| {
            let o2 = CheckOptions {
                max_units: left,
                ..opts.clone()
            };
            check::check(&groups, &region, &o2)
                .map(|r| (r, notes))
                .map_err(|e| e.to_string())
        });
        match result {
            Ok((r, notes)) => {
                units += r.units;
                left = left.saturating_sub(r.units);
                for (k, n) in &r.counts {
                    *totals.entry(k.clone()).or_default() += n;
                }
                let mut row = serde_json::to_value(&r).expect("json");
                row["id"] = json!(id);
                row["name"] = json!(name);
                if !notes.is_empty() {
                    row["notes"] = json!(notes);
                }
                rows.push(row);
            }
            Err(e) => {
                errors += 1;
                rows.push(json!({"id": id, "name": name, "error": e}));
            }
        }
    }
    let ok = errors == 0 && totals.values().all(|n| *n == 0);
    Ok(json!({
        "objects": rows,
        "totals": {"units": units, "counts": totals, "ok": ok, "complete": errors == 0, "errors": errors},
    }))
}

fn plan_label(doc: &Document, o: &Object) -> String {
    kadakas_core::hardscape::data::style_of(doc, o)
        .and_then(|(id, _)| doc.resources.get(id).map(|r| r.name.clone()))
        .unwrap_or_else(|| o.shape.kind().to_string())
}

#[cfg(test)]
mod tests;
