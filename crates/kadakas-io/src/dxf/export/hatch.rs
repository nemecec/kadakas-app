//! Hatch fills as HATCH entities (IOP-27).
//!
//! An object with a closed outline and a hatch fill (`fill_pattern`) is written as a HATCH entity
//! with a pattern: the boundary path, the name of the pattern and its definition lines. A pattern
//! that is a standard pattern at some scale and turn (`ANSI31`, `AR-B816`, ...) gets that name,
//! the scale in code 41 and the turn in code 52. Any other hatch gets a name made from its own
//! name, and the lines. A CAD program draws the lines of the entity, so every program shows the
//! same.
//!
//! A hatch with a color or a weight of its own on a line cannot be a HATCH (a pattern line has
//! none). It stays as plain LINE entities, as before. So does a tile.

use kadakas_core::DVec2;
use kadakas_core::curve::Vertex;
use kadakas_core::graphics::{HatchDef, HatchLine, PatternUnits};
use kadakas_core::model::{Object, Shape};
use kadakas_core::pattern::Decor;

use super::{Ctx, W, bulged_vertices, common};
use crate::dxf::hatch::{self, PatLine};

/// One boundary path: vertices with bulges.
type Path = Vec<(DVec2, f64)>;

/// The boundary paths of an object: the outline first, then holes. `None` for a shape that has no
/// closed outline.
fn boundary(cx: &Ctx, o: &Object) -> Option<Vec<Path>> {
    let tol = cx.tol;
    let ring = |pts: &[Vertex]| {
        let (verts, _) = bulged_vertices(pts, true, tol);
        (verts.len() >= 2).then_some(verts)
    };
    match &o.shape {
        Shape::Polyline {
            points,
            closed: true,
        } => {
            let mut paths = vec![ring(points)?];
            // The islands that the import found in the hatch are holes of this one HATCH.
            let outer: Vec<DVec2> = paths[0].iter().map(|(p, _)| *p).collect();
            let ids: Vec<kadakas_core::uid::Uid> = o
                .data
                .get("hatch_islands")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            for id in ids.into_iter().take(1000) {
                let Some(Shape::Polyline {
                    points: hole,
                    closed: true,
                }) = cx
                    .doc
                    .object(kadakas_core::model::ObjectId(id))
                    .map(|x| &x.shape)
                else {
                    continue;
                };
                // The objects of the export are moved to the real-world place, the islands are not yet.
                let Some(mut r) = ring(hole) else { continue };
                for (p, _) in &mut r {
                    *p += cx.shift;
                }
                if r.first()
                    .is_some_and(|(p, _)| kadakas_core::geom::point_in_polygon(*p, &outer))
                {
                    paths.push(r);
                }
            }
            Some(paths)
        }
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            Some(vec![
                [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
                    .iter()
                    .map(|p| (*p, 0.0))
                    .collect(),
            ])
        }
        // Two half circles, each one an arc with a bulge of 1.
        Shape::Circle { center, radius } => Some(vec![vec![
            (*center + DVec2::new(*radius, 0.0), 1.0),
            (*center - DVec2::new(*radius, 0.0), 1.0),
        ]]),
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            let mut paths = vec![ring(boundary)?];
            if let Some(d) = kadakas_core::landscape::data(o) {
                paths.extend(d.holes.iter().filter_map(|h| ring(h)));
            }
            Some(paths)
        }
        _ => None,
    }
}

/// The name of the pattern in the file: made of capital letters, digits, `_` and `-`.
fn file_name(resource: &str) -> String {
    let s: String = resource
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .take(31)
        .collect();
    if s.chars().all(|c| c == '_') {
        "KADAKAS".to_string()
    } else {
        s
    }
}

/// A point that lies inside the outline and outside the holes, found along horizontal lines.
fn seed_point(paths: &[Vec<DVec2>]) -> Option<DVec2> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in paths.iter().flatten() {
        lo = lo.min(p.y);
        hi = hi.max(p.y);
    }
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    for frac in [0.5, 0.37, 0.63, 0.25, 0.75, 0.13, 0.87] {
        let y = lo + (hi - lo) * frac;
        let mut xs: Vec<f64> = Vec::new();
        for path in paths {
            let n = path.len();
            for i in 0..n {
                let (a, b) = (path[i], path[(i + 1) % n]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        // The widest stretch between an odd and the next crossing is inside.
        let best = xs
            .as_chunks::<2>()
            .0
            .iter()
            .max_by(|p, q| (p[1] - p[0]).total_cmp(&(q[1] - q[0])));
        if let Some(c) = best.filter(|c| c[1] - c[0] > 0.0) {
            return Some(DVec2::new((c[0] + c[1]) / 2.0, y));
        }
    }
    None
}

/// The definition of a hatch in model mm: paper units are multiplied by the scale of the layer.
fn in_model_mm(def: &HatchDef, scale: f64) -> HatchDef {
    let f = def.units.factor(scale);
    HatchDef {
        units: PatternUnits::World,
        lines: def
            .lines
            .iter()
            .map(|l| HatchLine {
                spacing: l.spacing * f,
                offset: l.offset * f,
                origin: l.origin * f,
                dash: l.dash.iter().map(|d| d * f).collect(),
                ..l.clone()
            })
            .collect(),
    }
}

/// Writes the HATCH entity of the fill pattern of `o`, if it can be one. Returns `true` when it
/// did, and the caller leaves out the LINE entities of the pattern.
pub(super) fn write(
    w: &mut W,
    cx: &mut Ctx,
    o: &Object,
    layer: &str,
    hidden: bool,
    fill: Option<(kadakas_core::resource::ResourceId, f64)>,
    decor: &Decor,
) -> bool {
    let Some((id, scale)) = fill else {
        return false;
    };
    let doc = cx.doc;
    let Some(def) = doc.resources.hatch(id) else {
        return false;
    };
    // Nothing is drawn (the pattern is too dense or the outline too small): nothing to write.
    if decor.fill.is_empty()
        || def
            .lines
            .iter()
            .any(|l| l.color.is_some() || l.weight.is_some())
    {
        return false;
    }
    let Some(paths) = boundary(cx, o) else {
        return false;
    };
    let model = in_model_mm(def, scale);
    let unit = cx.unit;
    // The name of the resource keeps the name of a standard pattern ("ANSI31 x2 90 deg"): its first word.
    let first_word = doc
        .resources
        .get(id)
        .and_then(|r| r.name.split_whitespace().next().map(str::to_owned));
    let named = hatch::identify_as(&model, first_word.as_deref());
    let (name, scale_code, angle_code, kind) = match &named {
        Some(n) => (n.name.to_string(), n.scale / unit, n.angle, 1),
        None => (
            file_name(doc.resources.get(id).map_or("KADAKAS", |r| r.name.as_str())),
            1.0,
            0.0,
            2,
        ),
    };
    let flat: Vec<Vec<DVec2>> = paths
        .iter()
        .map(|p| p.iter().map(|(v, _)| *v).collect())
        .collect();

    common(w, "HATCH", layer, o, hidden);
    w.pair(100, "AcDbHatch");
    w.num(10, 0.0);
    w.num(20, 0.0);
    w.num(30, 0.0);
    w.num(210, 0.0);
    w.num(220, 0.0);
    w.num(230, 1.0);
    w.pair(2, &name);
    w.pair(70, 0);
    w.pair(71, 0);
    w.pair(91, paths.len());
    for (i, path) in paths.iter().enumerate() {
        // 1 outer path, 2 polyline, 16 outermost: a hole is a polyline path inside the outer one.
        w.pair(92, if i == 0 { 3 } else { 18 });
        w.pair(72, i32::from(path.iter().any(|(_, b)| *b != 0.0)));
        w.pair(73, 1);
        w.pair(93, path.len());
        for (p, bulge) in path {
            cx.pt(w, 10, *p);
            if path.iter().any(|(_, b)| *b != 0.0) {
                w.num(42, *bulge);
            }
        }
        w.pair(97, 0);
    }
    w.pair(75, 0);
    w.pair(76, kind);
    w.num(52, angle_code);
    w.num(41, scale_code);
    w.pair(77, 0);
    w.pair(78, model.lines.len());
    let shift = cx.shift;
    for l in &model.lines {
        let p = PatLine::from(l);
        w.num(53, p.angle);
        w.num(43, (p.base.x + shift.x) / unit);
        w.num(44, (p.base.y + shift.y) / unit);
        // The offset is a vector, not a point: no shift.
        w.num(45, p.offset.x / unit);
        w.num(46, p.offset.y / unit);
        w.pair(79, p.dashes.len());
        for d in &p.dashes {
            w.num(49, d / unit);
        }
    }
    match seed_point(&flat) {
        Some(s) => {
            w.pair(98, 1);
            cx.pt(w, 10, s);
        }
        None => w.pair(98, 0),
    }
    *cx.hatch_names.entry(name).or_default() += 1;
    true
}
