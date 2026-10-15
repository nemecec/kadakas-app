//! Agent API for the unit layout engine: lay units (pavers, tiles, boards, bricks) in a bond over
//! any closed shape or a region you give.
//!
//! Methods: `pattern.layout`, `pattern.to_objects`, `report.pattern`.

use serde_json::{Map, Value, json};

use kadakas_core::DVec2;
use kadakas_core::command::Command;
use kadakas_core::curve::Vertex;
use kadakas_core::layout::object::{outline_of, pattern_of};
use kadakas_core::layout::summary::{self, SummaryOptions};
use kadakas_core::layout::{
    self, Bond, MAX_GAP_SEGMENTS, MAX_UNITS, Pattern, Placement, Region, TOLERANCE, Unit, gaps,
};
use kadakas_core::model::{Attributes, ClassId, Document, LayerId, Object, ObjectId, Shape};

use crate::errors::bad;
use crate::{RpcError, Session};

pub const PATTERN_LAYOUT: (&str, &str) = (
    "pattern.layout",
    "Lay units (pavers, tiles, boards) in a bond over a closed shape and return the units and the gaps. params: {id | region: {outline: [vertices], holes?: [[vertices]]}, pattern?: {bond? (running_bond | stack | herringbone | basket_weave), unit_length, unit_width, gap? (the joint width, default 0)}, origin? ([x, y]: where the pattern starts), angle? (degrees, counterclockwise), align_to_edge? (n: start at vertex n of the outline and run along the edge to vertex n+1; sets origin and angle), include? ([\"units\", \"gaps\"], default both), offset? and limit? (page of the lists, default 0 and 1000, at most 20000), tolerance? (curve flattening, mm, default 1)}. A closed polyline, rectangle, circle, landscape area or hardscape works as id. A hardscape with a style uses the joint pattern of the style with its own origin and angle unless you give pattern, origin, angle or align_to_edge. The pattern starts at the middle of the box of the region without an origin. The unit is the pattern unit with the gap taken off, cut exactly to the outline minus the holes (arcs are cut into lines within the tolerance). Returns {summary: {bond, origin, angle_deg, whole_units, cut_units, slivers, area_laid_m2, gap_area_m2, gap_length_m, order_units, ...}, units_total, units: [{whole, area_m2, share (part of a full unit), outline: [[x, y]], holes: [[[x, y]]]}], gaps_total, gaps: [[[x, y], [x, y]]] (gap center lines), next_offset (null at the end)}. A cut unit that the edge splits in two pieces counts as two units. A layout of more than 1,000,000 units is an error that says how many units it would make.",
);
pub const PATTERN_TO_OBJECTS: (&str, &str) = (
    "pattern.to_objects",
    "Turn the units (and, with include, the gap center lines) of a layout into ordinary objects in one undo step: a closed polyline for every unit and a line for every gap segment, in one group. params: as pattern.layout (id | region, pattern, origin, angle, align_to_edge, tolerance), plus include? ([\"units\"] by default; add \"gaps\"), layer? (default: the layer of the object, else the active layer), class?, group? (default true), name? (name of the group). A hole inside a cut unit becomes a closed polyline of its own. At most 100,000 objects in one call. Returns {group, units, gaps, objects}.",
);
pub const REPORT_PATTERN: (&str, &str) = (
    "report.pattern",
    "Counts and quantities of unit patterns. params: {ids? (default: every hardscape with a joint pattern), pattern?, origin?, angle?, align_to_edge? (as pattern.layout; with several ids they apply to each), sliver_share? (cut units under this part of a full unit are slivers, default 0.25), waste_percent? (no default: without it the order has no waste added), tolerance? (curve flattening in mm, default 1; the DXF export uses its own tolerance, default 1, and gives the same counts when the two are equal), format?: csv}. Returns {rows: [{id, name, whole_units, cut_units, slivers, area_laid_m2, region_area_m2, gap_area_m2, gap_length_m, order_units, order_units_with_waste, ...} or {id, error}], totals: {whole_units, cut_units, slivers, area_laid_m2, gap_length_m, order_units, order_units_with_waste}}. The order is the whole units plus the cut units, every cut unit counted as one unit; waste_percent adds that percent on top, rounded up. Slivers are part of the cut units. The counts match the polylines on the layers <layer>-UNITS-WHOLE and <layer>-UNITS-CUT of export.file with patterns: \"units\".",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "pattern": "The layout engine lays equal units in a bond (running_bond, stack, herringbone, basket_weave) over a closed shape with holes. The unit is unit_length by unit_width, the gap is the joint width between units. Units are cut exactly to the outline minus the holes. Hardscapes use the engine for their joint pattern; their holes, origin, angle and align_to_edge are settings of the object (hardscape.add, hardscape.set).",
        "workflow_parking_lot": [
            {"step": 1, "method": "import.file", "params": {"path": "parking.dxf"}, "why": "Read the survey of the lot."},
            {"step": 2, "method": "query.objects", "params": {"kind": "polyline", "class": "LOT"}, "why": "Find the closed polylines of the lot outline and the islands on their layer."},
            {"step": 3, "method": "hardscape_style.create", "params": {"name": "Pavers 200x100", "thickness": 80, "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 5}}},
            {"step": 4, "method": "hardscape.from_shapes", "params": {"ids": ["<polyline ids>"], "style": "Pavers 200x100", "holes_from_inner": true}, "why": "The lot becomes a hardscape; islands inside it become holes."},
            {"step": 5, "method": "hardscape.set", "params": {"id": "<hardscape id>", "align_to_edge": 0}, "why": "Rows run along the first edge (the kerb); sets origin and angle."},
            {"step": 6, "method": "render.png", "params": {"view": "top"}, "why": "Look at the pattern before you count it."},
            {"step": 7, "method": "report.pattern", "params": {"waste_percent": 5}, "why": "Whole, cut and sliver units, area laid, gap length and the order quantity."},
            {"step": 8, "method": "export.file", "params": {"path": "parking-paving.dxf", "format": "dxf", "options": {"patterns": "units"}}, "why": "The units as closed polylines for the contractor. The counts equal report.pattern."}
        ],
        "examples": [
            {"method": "pattern.layout", "params": {"id": "00000000-0000-0000-0000-000000000010", "pattern": {"bond": "herringbone", "unit_length": 200, "unit_width": 100, "gap": 4}, "angle": 45, "include": ["units"], "limit": 100}},
            {"method": "pattern.layout", "params": {"region": {"outline": [[0, 0], [8000, 0], [8000, 5000], [0, 5000]], "holes": [[[3000, 1500], [4500, 1500], [4500, 3000], [3000, 3000]]]}, "pattern": {"unit_length": 600, "unit_width": 300, "gap": 3, "bond": "stack"}, "origin": [0, 0]}},
            {"method": "pattern.to_objects", "params": {"id": "00000000-0000-0000-0000-000000000010", "include": ["units", "gaps"], "name": "Paving units"}},
            {"method": "report.pattern", "params": {"waste_percent": 5, "sliver_share": 0.25}}
        ],
    })
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !matches!(
        method,
        "pattern.layout" | "pattern.to_objects" | "report.pattern"
    ) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(match p.as_object() {
        Some(m) => match method {
            "pattern.layout" => layout_call(s, m),
            "pattern.to_objects" => to_objects(s, m),
            _ => report(s, m),
        },
        None => Err(bad("params must be an object")),
    })
}

/// What a call works on: the region, the pattern and where it starts.
struct Subject {
    region: Region,
    pattern: Pattern,
    place: Placement,
    /// The layer and class of the object, when the subject is an object.
    layer: Option<LayerId>,
    class: Option<ClassId>,
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

/// The pattern from the call, on top of a base (the pattern of the object).
fn pattern_param(m: &Map<String, Value>, base: Option<Pattern>) -> Result<Pattern, RpcError> {
    let given = match m.get("pattern") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_object()
                .ok_or_else(|| bad("'pattern' must be an object"))?,
        ),
    };
    let mut p = base.unwrap_or(Pattern {
        bond: Bond::RunningBond,
        unit_length: f64::NAN,
        unit_width: f64::NAN,
        gap: 0.0,
    });
    if let Some(g) = given {
        for (k, v) in g {
            match k.as_str() {
                "bond" | "kind" => {
                    p.bond = Bond::from_name(v.as_str().unwrap_or("")).ok_or_else(|| {
                        bad("'bond' must be running_bond, stack, herringbone or basket_weave")
                    })?
                }
                "unit_length" => p.unit_length = number(v, "unit_length")?,
                "unit_width" => p.unit_width = number(v, "unit_width")?,
                "gap" | "spacing" => p.gap = number(v, "gap")?,
                other => return Err(bad(format!("unknown pattern field '{other}'"))),
            }
        }
    }
    if p.unit_length.is_nan() || p.unit_width.is_nan() {
        return Err(bad(
            "give a pattern with unit_length and unit_width (an object with a joint pattern needs none)",
        ));
    }
    p.validate().map_err(bad)?;
    if p.unit_length < Pattern::MIN_UNIT || p.unit_width < Pattern::MIN_UNIT {
        return Err(bad(format!(
            "units must be at least {} mm",
            Pattern::MIN_UNIT
        )));
    }
    Ok(p)
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

fn subject_of(doc: &Document, o: &Object, m: &Map<String, Value>) -> Result<Subject, RpcError> {
    let tol = tolerance(m)?;
    let (outline, _) = outline_of(o).map_err(bad)?;
    let region = layout::object::region_of(o, tol).map_err(bad)?;
    let own = pattern_of(doc, o);
    let pattern = pattern_param(m, own.map(|x| x.0))?;
    let place = placement(m, &outline, own.map(|x| x.1))?;
    Ok(Subject {
        region,
        pattern,
        place,
        layer: Some(o.layer),
        class: Some(o.class),
    })
}

fn placement(
    m: &Map<String, Value>,
    outline: &[Vertex],
    base: Option<Placement>,
) -> Result<Placement, RpcError> {
    let mut place = base.unwrap_or_default();
    if let Some(v) = m.get("align_to_edge").filter(|v| !v.is_null()) {
        let i = v
            .as_u64()
            .ok_or_else(|| bad("'align_to_edge' must be a vertex number"))?;
        place = Placement::along_edge(outline, i as usize).map_err(bad)?;
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
    match (m.get("id"), m.get("region")) {
        (Some(_), Some(_)) => Err(bad("give 'id' or 'region', not both")),
        (Some(id), None) => {
            let id: ObjectId = serde_json::from_value(id.clone()).map_err(bad)?;
            let o = doc
                .object(id)
                .ok_or_else(|| bad(format!("object {id} does not exist")))?;
            subject_of(doc, o, m)
        }
        (None, Some(r)) => {
            let (region, outline) = region_param(r, tolerance(m)?)?;
            let pattern = pattern_param(m, None)?;
            let place = placement(m, &outline, None)?;
            Ok(Subject {
                region,
                pattern,
                place,
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

fn too_many(e: layout::TooMany) -> RpcError {
    RpcError::limit(format!(
        "{e}. Use larger units, a smaller region, or split the region"
    ))
}

fn xy(p: DVec2) -> Value {
    json!([p.x, p.y])
}

fn unit_json(u: &Unit) -> Value {
    json!({
        "whole": u.whole,
        "area_m2": u.area / 1e6,
        "share": u.share,
        "outline": u.rings.first().map(|r| r.points.iter().map(|p| xy(*p)).collect::<Vec<_>>()).unwrap_or_default(),
        "holes": u.rings.iter().skip(1).map(|r| r.points.iter().map(|p| xy(*p)).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}

fn summary_options(m: &Map<String, Value>) -> Result<SummaryOptions, RpcError> {
    let mut o = SummaryOptions::default();
    if let Some(v) = m.get("sliver_share").filter(|v| !v.is_null()) {
        let x = number(v, "sliver_share")?;
        if !(0.0..=1.0).contains(&x) {
            return Err(bad("'sliver_share' must be between 0 and 1"));
        }
        o.sliver_share = x;
    }
    if let Some(v) = m.get("waste_percent").filter(|v| !v.is_null()) {
        let x = number(v, "waste_percent")?;
        if !(0.0..=100.0).contains(&x) {
            return Err(bad("'waste_percent' must be between 0 and 100"));
        }
        o.waste_percent = Some(x);
    }
    Ok(o)
}

fn layout_call(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let sub = subject(s, m)?;
    let (want_units, want_gaps) = include(m, &["units", "gaps"])?;
    let offset = m.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = (m.get("limit").and_then(Value::as_u64).unwrap_or(1000) as usize).min(20_000);
    let opts = summary_options(m)?;
    let sum = summary::summarize(&sub.region, &sub.pattern, &sub.place, &opts).map_err(too_many)?;
    let mut out = json!({ "summary": sum });
    let mut next: Option<usize> = None;
    if want_units {
        let mut page = Vec::new();
        let mut i = 0usize;
        layout::for_each_unit(&sub.region, &sub.pattern, &sub.place, MAX_UNITS, &mut |u| {
            if i >= offset && page.len() < limit {
                page.push(unit_json(u));
            }
            i += 1;
        })
        .map_err(too_many)?;
        if offset + page.len() < i {
            next = Some(offset + page.len());
        }
        out["units_total"] = json!(i);
        out["units"] = json!(page);
    }
    if want_gaps {
        let g = gaps::gap_lines(
            &sub.region,
            &sub.pattern,
            &sub.place,
            MAX_GAP_SEGMENTS,
            false,
        );
        if g.truncated {
            return Err(RpcError::limit(
                "the pattern has too many gap lines. Use larger units or a smaller region",
            ));
        }
        let total = g.segments.len();
        let page: Vec<Value> = g
            .segments
            .iter()
            .skip(offset)
            .take(limit)
            .map(|(a, b)| json!([xy(*a), xy(*b)]))
            .collect();
        if offset + page.len() < total {
            next = Some(next.map_or(offset + page.len(), |n| n.min(offset + page.len())));
        }
        out["gaps_total"] = json!(total);
        out["gaps"] = json!(page);
    }
    out["next_offset"] = json!(next);
    Ok(out)
}

/// Most objects that `pattern.to_objects` makes in one call.
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
    let mut shapes: Vec<Shape> = Vec::new();
    let (mut units, mut gap_count) = (0usize, 0usize);
    if want_units {
        let mut over = false;
        layout::for_each_unit(
            &sub.region,
            &sub.pattern,
            &sub.place,
            MAX_OBJECTS,
            &mut |u| {
                if over {
                    return;
                }
                units += 1;
                for r in &u.rings {
                    shapes.push(Shape::Polyline {
                        points: kadakas_core::curve::corners(&r.points),
                        closed: true,
                    });
                }
                over = shapes.len() > MAX_OBJECTS;
            },
        )
        .map_err(|e| {
            RpcError::limit(format!(
                "{e}. One call makes at most {MAX_OBJECTS} objects: split the region"
            ))
        })?;
    }
    if want_gaps {
        let g = gaps::gap_lines(&sub.region, &sub.pattern, &sub.place, MAX_OBJECTS, false);
        if g.truncated {
            return Err(RpcError::limit(format!(
                "the gaps are more than {MAX_OBJECTS} lines: split the region or leave the gaps out"
            )));
        }
        gap_count = g.segments.len();
        shapes.extend(g.segments.iter().map(|(a, b)| Shape::Line { a: *a, b: *b }));
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
    let mut commands: Vec<Command> = shapes
        .into_iter()
        .map(|shape| Command::AddObject {
            shape,
            layer,
            class,
            attrs: Attributes::default(),
        })
        .collect();
    let ids: Vec<ObjectId> = (0..n).map(|_| ObjectId(next.take())).collect();
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

fn label(doc: &Document, o: &Object) -> String {
    kadakas_core::hardscape::data::style_of(doc, o)
        .and_then(|(id, _)| doc.resources.get(id).map(|r| r.name.clone()))
        .unwrap_or_else(|| o.shape.kind().to_string())
}

/// The summary of the pattern of every hardscape (or the ids), as report rows and totals.
pub fn report_rows(
    doc: &Document,
    ids: Option<Vec<ObjectId>>,
    m: &Map<String, Value>,
) -> Result<(Vec<Value>, Value), RpcError> {
    let opts = summary_options(m)?;
    let explicit = ids.is_some();
    let list: Vec<ObjectId> = ids.unwrap_or_else(|| {
        doc.objects
            .values()
            .filter(|o| matches!(o.shape, Shape::Hardscape { .. }) && pattern_of(doc, o).is_some())
            .map(|o| o.id)
            .collect()
    });
    let mut rows = Vec::new();
    let mut tot = (0usize, 0usize, 0usize, 0.0f64, 0.0f64, 0usize, 0usize, true);
    for id in list {
        let Some(o) = doc.object(id) else {
            rows.push(json!({"id": id, "error": "no such object"}));
            continue;
        };
        let sub = match subject_of(doc, o, m) {
            Ok(x) => x,
            Err(e) if explicit => {
                rows.push(json!({"id": id, "name": label(doc, o), "error": e.message}));
                continue;
            }
            Err(_) => continue,
        };
        match summary::summarize(&sub.region, &sub.pattern, &sub.place, &opts) {
            Ok(sum) => {
                tot.0 += sum.whole_units;
                tot.1 += sum.cut_units;
                tot.2 += sum.slivers;
                tot.3 += sum.area_laid_m2;
                tot.4 += sum.gap_length_m.unwrap_or(0.0);
                tot.5 += sum.order_units;
                tot.6 += sum.order_units_with_waste.unwrap_or(0);
                let mut row = serde_json::to_value(&sum).expect("json");
                row["id"] = json!(id);
                row["name"] = json!(label(doc, o));
                rows.push(row);
            }
            Err(e) => {
                tot.7 = false;
                rows.push(json!({
                    "id": id,
                    "name": label(doc, o),
                    "error": format!("{e}"),
                    "estimated_units": e.estimated,
                }));
            }
        }
    }
    let totals = json!({
        "whole_units": tot.0,
        "cut_units": tot.1,
        "slivers": tot.2,
        "area_laid_m2": tot.3,
        "gap_length_m": tot.4,
        "order_units": tot.5,
        "waste_percent": opts.waste_percent,
        "order_units_with_waste": opts.waste_percent.map(|_| tot.6),
        "complete": tot.7,
    });
    Ok((rows, totals))
}

fn report(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids: Option<Vec<ObjectId>> = match m.get("ids").filter(|v| !v.is_null()) {
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(bad)?),
        None => None,
    };
    let (rows, totals) = report_rows(s.editor.doc(), ids, m)?;
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
            "Whole units",
            "Cut units",
            "Slivers",
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
    for r in rows {
        table.push(
            [
                "id",
                "name",
                "bond",
                "whole_units",
                "cut_units",
                "slivers",
                "area_laid_m2",
                "gap_length_m",
                "order_units",
                "order_units_with_waste",
            ]
            .iter()
            .map(|k| text(r, k))
            .collect(),
        );
    }
    kadakas_core::csv::write_rows(&table)
}

/// The pattern block of one hardscape for `report.hardscape`: the summary, or an error.
pub fn block_for(doc: &Document, o: &Object, m: &Map<String, Value>) -> Option<Value> {
    pattern_of(doc, o)?;
    let opts = summary_options(m).ok()?;
    let sub = subject_of(doc, o, &Map::new()).ok()?;
    Some(
        match summary::summarize(&sub.region, &sub.pattern, &sub.place, &opts) {
            Ok(sum) => serde_json::to_value(sum).expect("json"),
            Err(e) => json!({ "error": format!("{e}"), "estimated_units": e.estimated }),
        },
    )
}

#[cfg(test)]
mod tests;
