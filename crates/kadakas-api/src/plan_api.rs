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
use kadakas_core::layout::object::{outline_of, region_of};
use kadakas_core::layout::plan::{self, CourseDef, Plan, SplitLine, ZoneSpec};
use kadakas_core::model::{Attributes, ClassId, Document, LayerId, Object, ObjectId, Shape};
use kadakas_core::regions;

use crate::errors::bad;
use crate::{RpcError, Session};

pub const REGION_FROM_CURVES: (&str, &str) = (
    "region.from_curves",
    "Join loose lines, arcs, circles, rectangles and open or closed polylines into closed regions with holes (for example a parking lot drawn as separate lines and arcs). The curves are split where they cross; ends that lie within the tolerance of each other or of another curve are joined (a gap or a small overshoot is closed); dead ends are cut off; the areas that are left are traced. Nothing is guessed: a gap wider than the tolerance stays open, the area behind it is not made, and the dead ends are reported with their position, their length and the nearest other end, so that you can draw the place. A dead end is open (part of a shape that does not close) or a stub (a line that overshoots a corner or stops at a junction: harmless, but listed). params: {ids? (default: the selection), tolerance? (mm, default 1: the widest gap that is closed), create? (hardscape | landscape_area | polyline: make objects from the regions in one undo step; without it nothing is created), depths? (even | odd | all: which regions become objects; default even for hardscape and landscape_area, all for polyline; an even depth is an area to fill, an odd depth is an island inside a hole of an area), style? (hardscape style, ID or name, for create hardscape), name? (landscape area name), min_area_m2? (leave out smaller regions), layer?, class? (default: those of the first source object), delete_source? (delete the curves that only made created regions), dry_run?}. Returns {regions: [{index, outline: [vertices] (counterclockwise; a point on each arc is a vertex of kind point_on_arc), holes: [[vertices]], area_m2, depth, role (area | island), inside: [x, y], bbox: {lo, hi}, sources: [object IDs]}], closed_gaps: {count, listed: [{at, size_mm}]}, dead_ends: {count, listed: [{at, length_mm, nearest: {at, distance_mm}}]}, complete (no dead ends), skipped: [{id, reason}], ignored_short, duplicates, created: [{region, id, kind}], tolerance_mm, dry_run}. A region inside a hole of another region is an island: its own region has an odd depth, and the outer region lists it as a hole, so create hardscape (even depths) makes the lot with the island as a hole.",
);
pub const HARDSCAPE_SPLIT: (&str, &str) = (
    "hardscape.split",
    "Split a hardscape into zones with lines (stall rows, a driveway, a fire lane): the zones share their edges exactly, so there are no slivers and no overlaps, and the areas of the zones add up to the area of the hardscape. Each zone can have a pattern, origin, angle or alignment of its own. The split lines and the zone settings are data of the hardscape (undoable, they move with it). A split line that follows a drawn line, arc or polyline (source) is read again each time: move the line and the zones follow. params: {id, lines? ([object IDs or [vertices]]: replaces the split lines; [] removes them; an ID follows the object unless follow is false), follow? (default true), zones? ([{index? (the number of the zone in the reply, from 1... see zones in the reply: index counts from 0), seed? ([x, y]: a point inside the zone; the settings belong to the zone that holds it, also after a line moves), name?, pattern? (as the pattern of unit_pattern.layout), origin?, angle?, align_to_edge? (n: start at vertex n of the outline of the zone), align_to_split? (n: start at the first point of split line n and run along its first segment)}]: replaces the zone settings), tolerance? (mm, default 1: an end of a line this close to an edge joins it; the edge of the hardscape never moves)}. A zone without settings continues the pattern of the hardscape (the same lattice, cut at the zone edge). Returns {zones: [{index, name, area_m2, inside: [x, y], bbox: {lo, hi}, has_settings}], split_lines, area_m2, zones_area_m2, notes: [text]} . A split line that stops short of the edge, or cuts nothing, is a note with its position and the gap. Counts per zone and in total are in report.unit_pattern (groups), the check in unit_pattern.check, the DXF has layers per zone.",
);
pub const HARDSCAPE_COURSE: (&str, &str) = (
    "hardscape.course",
    "Add or replace courses along a path: a row of units along a polyline or a curve, on one side of the line or centered (a soldier course along a kerb, a border round an island). The course takes its area from the pattern next to it: the field is cut by the course with a joint of the gap kept, so they never overlap. The units are counted, reported and exported like the others (the unit type has the name of the stone, so a soldier course and the field of the same stone are added up). Corner rules: miter (the units of both legs are cut at the mitre line), butt (the first leg runs on to the end of the corner, the second butts against it), corner_unit (one unit of its own fills the corner). On a curve the units stay straight and are placed along the curve; the joints taper, and the reply and the report give the narrowest and the widest joint. params: {id, courses: [{name, path? ([vertices], curves allowed) | source? (a line, arc or polyline: followed, read again each time) | along? (outline | \"hole:N\": the edge of the hardscape or of its Nth hole, inside the kerb border), side? (left | right | center | inside | outside; default for outline: inside, for a hole: outside), offset? (mm to the left of the path), closed?, orientation? (soldier: the long side of the stone across the path (default) | stretcher: along it), unit_length? (mm, along the path) and unit_width? (mm, across) (default: from the stone of the style by orientation), gap? (default: the joint of the style), corner? (default miter), unit? (name of the unit type, default the size of the stone of the style), align_field? (true: the pattern of the hardscape starts at the inner edge of the course, so the first row of the field is whole and the joint to the course is the gap; sets the origin and the angle of the hardscape; needs a bond, not a module)}] (or the fields of one course directly in params)}. A course with the same name is replaced. Returns {courses: [{name, units, corners, length_m, joints: {count, narrowest_mm, widest_mm}}], notes}. A corner that turns by more than 150 degrees is refused.",
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
        m.get("id").ok_or_else(|| bad("give the 'id' of a hardscape"))?,
        "id",
    )?;
    let o = doc
        .object(id)
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    if !matches!(o.shape, Shape::Hardscape { .. }) {
        return Err(bad(format!("object {id} is a {}, not a hardscape", o.shape.kind())));
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
        ],
    )?;
    let doc = s.editor.doc();
    let ids: Vec<ObjectId> = match m.get("ids").filter(|v| !v.is_null()) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad("'ids' must be a list of object IDs"))?,
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
    let depths = match m.get("depths").and_then(Value::as_str) {
        None => {
            if create == Some("polyline") {
                "all"
            } else {
                "even"
            }
        }
        Some(d) if ["even", "odd", "all"].contains(&d) => d,
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
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(o) = doc.object(id) else {
            skipped.push(json!({"id": id, "reason": "no such object"}));
            continue;
        };
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
                "role": if r.depth % 2 == 0 { "area" } else { "island" },
                "inside": xy(r.inside),
                "bbox": {"lo": xy(r.lo), "hi": xy(r.hi)},
                "sources": sources_of(r),
            })
        })
        .collect();
    let mut out = json!({
        "regions": region_json,
        "closed_gaps": {
            "count": found.closed_gap_count,
            "listed": found.closed_gaps.iter().map(|g| json!({"at": xy(g.at), "size_mm": g.size})).collect::<Vec<_>>(),
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
        "tolerance_mm": tol,
        "dry_run": dry_run,
    });
    let Some(kind) = create else {
        out["created"] = json!([]);
        return Ok(out);
    };
    // Which regions become objects.
    let chosen: Vec<usize> = found
        .regions
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.area >= min_area
                && match depths {
                    "even" => r.depth % 2 == 0,
                    "odd" => r.depth % 2 == 1,
                    _ => true,
                }
        })
        .map(|(i, _)| i)
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
    let name = m.get("name").and_then(Value::as_str).unwrap_or("").to_string();
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
    if !commands.is_empty() {
        let mut next = doc.id_plan();
        let ids: Vec<ObjectId> = commands.iter().map(|_| ObjectId(next.take())).collect();
        for (&i, id) in chosen.iter().zip(&ids) {
            created.push(json!({"region": i, "id": id, "kind": kind}));
        }
        if delete_source {
            // Only curves that made created regions, and no region that stays on paper.
            let kept: std::collections::HashSet<ObjectId> = found
                .regions
                .iter()
                .enumerate()
                .filter(|(i, _)| !chosen.contains(i))
                .flat_map(|(_, r)| sources_of(r))
                .collect();
            let gone: Vec<ObjectId> = chosen
                .iter()
                .flat_map(|&i| sources_of(&found.regions[i]))
                .filter(|id| !kept.contains(id))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
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

fn plan_state(
    doc: &Document,
    o: &Object,
    max_units: usize,
) -> Result<plan::Planned, RpcError> {
    let Some(planned) = plan::groups_of(doc, o, kadakas_core::layout::TOLERANCE, max_units).map_err(bad)?
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
                && let Some(g) = pl.groups.iter().find(|g| g.role == plan::Role::Zone && g.name == z.name)
                    .or_else(|| pl.groups.iter().filter(|g| g.role != plan::Role::Course).nth(i))
            {
                v["outline"] = json!(
                    g.region.shapes.iter().map(|sh| sh.iter().map(|r| r.iter().map(|q| xy(*q)).collect::<Vec<_>>()).collect::<Vec<_>>()).collect::<Vec<_>>()
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
            set: [(plan::KEY.to_string(), serde_json::to_value(p).expect("json"))]
                .into_iter()
                .collect(),
            remove: Vec::new(),
        }
    };
    s.editor.execute(&cmd).map(|_| ()).map_err(RpcError::from)
}

// ------------------------------------------------------------------ hardscape.split

fn split(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    unknown_fields(m, &["id", "lines", "follow", "zones", "tolerance"])?;
    let doc = s.editor.doc();
    let o = hardscape(doc, m)?;
    let mut p = current_plan(o);
    let follow = flag(m, "follow", true)?;
    if let Some(v) = opt_number(m, "tolerance")? {
        p.tolerance = v;
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
                        .ok_or_else(|| bad(format!("line {}: object {id} does not exist", i + 1)))?;
                    let (points, _) = plan::path_of(&src.shape).ok_or_else(|| {
                        bad(format!(
                            "line {}: object {id} is a {}, not a line, arc or polyline",
                            i + 1,
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
                        .map_err(|e| bad(format!("line {}: {e}", i + 1)))?;
                    if points.len() < 2 {
                        return Err(bad(format!("line {} needs 2 or more points", i + 1)));
                    }
                    p.splits.push(SplitLine { source: None, points });
                }
                _ => {
                    return Err(bad(format!(
                        "line {} must be an object ID or a list of vertices",
                        i + 1
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
                .ok_or_else(|| bad(format!("zone {}: must be an object", i + 1)))?;
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
            .map_err(|e| bad(format!("zone {}: {}", i + 1, e.message)))?;
            let seed = match (z.get("index"), z.get("seed")) {
                (Some(_), Some(_)) => return Err(bad(format!("zone {}: give 'index' or 'seed', not both", i + 1))),
                (Some(ix), None) => {
                    let ix = ix
                        .as_u64()
                        .ok_or_else(|| bad(format!("zone {}: 'index' must be a number", i + 1)))?
                        as usize;
                    before
                        .zones
                        .get(ix)
                        .map(|zi| zi.inside)
                        .ok_or_else(|| {
                            bad(format!(
                                "zone {}: there is no zone {ix}; the lines make {} zones (numbered from 0)",
                                i + 1,
                                before.zones.len()
                            ))
                        })?
                }
                (None, Some(sd)) => point(sd, "seed")?,
                (None, None) => return Err(bad(format!("zone {}: give 'index' or 'seed'", i + 1))),
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
                align_to_edge: z.get("align_to_edge").and_then(Value::as_u64).map(|n| n as usize),
                align_to_split: z.get("align_to_split").and_then(Value::as_u64).map(|n| n as usize),
            });
        }
    }
    p.validate().map_err(bad)?;
    let after = plan_state(doc, &with_plan(o, &p), max_units)?;
    let region = region_of(doc, o, kadakas_core::layout::TOLERANCE).map_err(bad)?;
    let o = o.clone();
    store(s, &o, &p)?;
    let zones_area: f64 = after.zones.iter().map(|z| z.area).sum();
    Ok(json!({
        "zones": zones_json(&after, None),
        "split_lines": p.splits.len(),
        "area_m2": region.area / 1e6,
        "zones_area_m2": zones_area / 1e6,
        "notes": after.notes,
    }))
}

// ------------------------------------------------------------------ courses

/// The outline or a hole of a hardscape as a closed path running counterclockwise.
fn edge_path(doc: &Document, o: &Object, which: &str) -> Result<Vec<Vertex>, RpcError> {
    let (outer, holes) = outline_of(o).map_err(bad)?;
    let mut ring = if which == "outline" {
        outer
    } else {
        let n: usize = which
            .strip_prefix("hole:")
            .and_then(|n| n.trim().parse().ok())
            .ok_or_else(|| bad("'along' must be outline or hole:N"))?;
        holes.get(n).cloned().ok_or_else(|| {
            bad(format!(
                "the hardscape has no hole {n} (it has {})",
                holes.len()
            ))
        })?
    };
    let flat = curve::flatten(&ring, true, 1.0);
    if kadakas_core::geom::signed_area(&flat) < 0.0 {
        ring.reverse();
    }
    let _ = doc;
    Ok(ring)
}

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
    let mut offset = opt_number(c, "offset").map_err(fail)?.unwrap_or(0.0);
    let (path, closed, source, side);
    let border = kadakas_core::hardscape::data::style_of(doc, o)
        .and_then(|(_, st)| st.border)
        .map_or(0.0, |b| b.width);
    match (
        c.get("path").filter(|v| !v.is_null()),
        c.get("source").filter(|v| !v.is_null()),
        c.get("along").and_then(Value::as_str),
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
            path = edge_path(doc, o, which).map_err(fail)?;
            closed = true;
            source = None;
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
                Some(t) => Side::from_name(t)
                    .ok_or_else(|| fail(bad("'side' must be inside, outside, left, right or center")))?,
            };
            // The kerb border is not paved: the course starts at its inner edge.
            if border > 0.0 {
                offset += if outline { border } else { -border };
            }
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
    if let Some(c) = list.iter().find(|c| c.get("align_field").and_then(Value::as_bool) == Some(true)) {
        let name = c.get("name").and_then(Value::as_str).unwrap_or("");
        let def = p
            .courses
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| bad("align_field needs a named course"))?;
        let (path, closed) = plan::effective_path(doc, def, &mut Vec::new());
        let width = kadakas_core::layout::object::pattern_of(doc, o)
            .filter(|(pt, _)| pt.module.is_none())
            .map(|(pt, _)| pt.unit_width)
            .ok_or_else(|| bad("align_field needs a joint pattern of a bond (not a module)"))?;
        let spec = kadakas_core::layout::course::Spec {
            path,
            closed,
            side: def.side,
            offset: def.offset,
            along: def.along,
            depth: def.depth,
            gap: def.gap,
            corner: def.corner,
        };
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
                    set: [(plan::KEY.to_string(), serde_json::to_value(&p).expect("json"))]
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
        (Some(v), _) => serde_json::from_value(v.clone()).map_err(|_| bad("'ids' must be a list of object IDs"))?,
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
                set: [(plan::KEY.to_string(), serde_json::to_value(&p).expect("json"))]
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
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad("'ids' must be a list of object IDs"))?,
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
