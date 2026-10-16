//! Zones and courses of a hardscape (PAT-07, PAT-08): how one hardscape is divided into parts that
//! each have their own pattern, and the rows of units along its edges.
//!
//! The plan is the data of the hardscape (`data["pattern_plan"]`), so it moves, copies, undoes and
//! syncs with the object. It holds:
//!
//! - **split lines**: lines or polylines that cut the area into zones. A split line can follow a
//!   drawn object (`source`): the zones are made again from the current shape of that object each
//!   time, so a moved split line moves the zones. The points are the copy for when the object is
//!   gone.
//! - **zone settings**, each tied to a zone by a seed point (a point inside it): its own pattern,
//!   origin, angle or alignment. A zone without settings continues the pattern of the hardscape.
//! - **courses**: rows of units along a path (see [`super::course`]).
//!
//! [`plan_groups`] turns the hardscape and its plan into [`Group`]s: the field (what is left
//! after the courses), its zones and the courses. Each group is laid by the engine or by the course
//! code. Counts, reports, the check and the DXF export work on the groups, and the totals add
//! them up. The groups never overlap: the zones share their edges exactly and the field is cut by
//! the courses with a gap kept.

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::course::{self, Corner, Joints, Side};
use super::summary::{Desc, Summary, SummaryOptions, UnitCount, summarize_stream, with_waste};
use super::units::{LayoutError, Limits, Local, Placer, Stats, Unit, min_width};
use super::{Pattern, Placement, Region, zones};
use crate::curve::{self, Vertex, VertexKind};
use crate::model::{Document, Object, ObjectId, Shape};

/// The key in `Object::data` that holds the plan.
pub const KEY: &str = "pattern_plan";
/// Most zone settings of one hardscape.
pub const MAX_ZONES: usize = 1000;
/// Most courses of one hardscape.
pub const MAX_COURSES: usize = 100;
/// Flattening tolerance of the arcs of split lines, in mm.
pub const SPLIT_FLATTEN: f64 = 0.1;
/// Most zones that the split lines of one hardscape may make. Measured (`measure_zones`): 10,000
/// zones are cut in 19 ms and laid in 0.34 s, so time is not the reason for the bound; every zone
/// is a group with its own layers in the DXF and its own row in the report.
pub const MAX_ZONE_COUNT: usize = 1000;

/// A split line: a drawn object that it follows, and the points as a copy.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplitLine {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ObjectId>,
    pub points: Vec<Vertex>,
}

/// The settings of one zone, tied to the zone by a point inside it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoneSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A point inside the zone. The settings belong to the zone that holds it.
    pub seed: DVec2,
    /// A pattern of its own; without one the zone has the pattern of the hardscape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Pattern>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<DVec2>,
    /// The angle of the pattern in degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// Start at vertex n of the outline of the zone and run along the edge to the next vertex.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align_to_edge: Option<usize>,
    /// Start at the first point of split line n and run along its first segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align_to_split: Option<usize>,
}

impl ZoneSpec {
    /// True if the setting changes the pattern or where it starts.
    pub fn has_overrides(&self) -> bool {
        self.pattern.is_some()
            || self.origin.is_some()
            || self.angle.is_some()
            || self.align_to_edge.is_some()
            || self.align_to_split.is_some()
    }
}

/// A course along a path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CourseDef {
    pub name: String,
    /// A drawn line, arc or polyline that the course follows (the path is made again from it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ObjectId>,
    pub path: Vec<Vertex>,
    pub closed: bool,
    /// The edge of the hardscape that the course follows: `outline` or `hole:N`. The path is made
    /// again from the edge each time, inside the kerb border.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    pub side: Side,
    pub offset: f64,
    /// The unit size along the path (mm).
    pub along: f64,
    /// The unit size across the path (mm).
    pub depth: f64,
    pub gap: f64,
    pub corner: Corner,
    /// The name of the unit type in counts. Units of the same stone as the field use its name, so
    /// that they are added up with it.
    pub unit: String,
}

impl Default for CourseDef {
    fn default() -> Self {
        CourseDef {
            name: String::new(),
            source: None,
            path: Vec::new(),
            closed: false,
            edge: None,
            side: Side::Left,
            offset: 0.0,
            along: 100.0,
            depth: 200.0,
            gap: 5.0,
            corner: Corner::Miter,
            unit: String::new(),
        }
    }
}

/// What happens to a unit that a zone edge crosses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneUnits {
    /// The unit is cut at the zone edge: each zone gets its piece (the default). A zone edge has
    /// no joint, so the two pieces touch.
    #[default]
    Cut,
    /// The unit goes whole to the zone that holds its center, and is not cut by the zone edge.
    /// Only for zones that continue the pattern of the hardscape. Quantities have no false cuts;
    /// the zone edge becomes the line of the units, and units may reach over it.
    Centroid,
}

/// The plan of a hardscape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plan {
    pub splits: Vec<SplitLine>,
    pub zones: Vec<ZoneSpec>,
    pub courses: Vec<CourseDef>,
    /// The node tolerance of the zones (mm): a split line that ends this close to an edge joins it.
    pub tolerance: f64,
    pub zone_units: ZoneUnits,
}

impl Default for Plan {
    fn default() -> Self {
        Plan {
            splits: Vec::new(),
            zones: Vec::new(),
            courses: Vec::new(),
            tolerance: zones::DEFAULT_TOLERANCE,
            zone_units: ZoneUnits::Cut,
        }
    }
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.splits.is_empty() && self.zones.is_empty() && self.courses.is_empty()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.zones.len() > MAX_ZONES {
            return Err(format!("a hardscape has at most {MAX_ZONES} zone settings"));
        }
        if self.splits.len() > zones::MAX_LINES {
            return Err(format!(
                "a hardscape has at most {} split lines",
                zones::MAX_LINES
            ));
        }
        if self.courses.len() > MAX_COURSES {
            return Err(format!("a hardscape has at most {MAX_COURSES} courses"));
        }
        if !(self.tolerance.is_finite() && self.tolerance > 0.0 && self.tolerance <= 100.0) {
            return Err("the tolerance must be between 0 and 100 mm".into());
        }
        let mut names = std::collections::HashSet::new();
        for c in &self.courses {
            if c.name.trim().is_empty() {
                return Err("a course needs a name".into());
            }
            if !names.insert(c.name.to_lowercase()) {
                return Err(format!("two courses are called '{}'", c.name));
            }
            course::Spec {
                path: c.path.clone(),
                closed: c.closed,
                side: c.side,
                offset: c.offset,
                along: c.along,
                depth: c.depth,
                gap: c.gap,
                corner: c.corner,
            }
            .validate()
            .or_else(|e| {
                // A course that follows a drawing may have no copy of the path yet.
                if (c.source.is_some() || c.edge.is_some()) && c.path.is_empty() {
                    Ok(())
                } else {
                    Err(format!("course '{}': {e}", c.name))
                }
            })?;
        }
        for (i, z) in self.zones.iter().enumerate() {
            if !z.seed.is_finite() {
                return Err(format!("zone setting {} has no finite seed point", i));
            }
            if z.angle.is_some_and(|a| !a.is_finite()) || z.origin.is_some_and(|o| !o.is_finite()) {
                return Err(format!("zone setting {}: numbers must be finite", i));
            }
            if let Some(p) = &z.pattern {
                p.validate()
                    .map_err(|e| format!("zone setting {}: {e}", i))?;
                if p.bond != super::Bond::Module
                    && (p.unit_length < Pattern::MIN_UNIT || p.unit_width < Pattern::MIN_UNIT)
                {
                    return Err(format!(
                        "zone setting {}: units must be at least {} mm",
                        i,
                        Pattern::MIN_UNIT
                    ));
                }
            }
            if z.align_to_split.is_some_and(|n| n >= self.splits.len()) {
                return Err(format!(
                    "zone setting {}: align_to_split names a split line that does not exist",
                    i
                ));
            }
        }
        if self
            .splits
            .iter()
            .flat_map(|s| &s.points)
            .any(|v| !v.is_finite())
        {
            return Err("numbers must be finite".into());
        }
        Ok(())
    }
}

/// The plan of an object, if it has one that can be read.
pub fn data(obj: &Object) -> Option<Plan> {
    if !matches!(obj.shape, Shape::Hardscape { .. }) {
        return None;
    }
    serde_json::from_value(obj.data.get(KEY)?.clone()).ok()
}

/// True if the object has a plan that cannot be read. Such objects are left alone.
pub fn has_unreadable_data(obj: &Object) -> bool {
    obj.data.get(KEY).is_some_and(|v| !v.is_null()) && data(obj).is_none()
}

/// Moves the positions of the plan with the object.
pub fn translate_data(obj: &mut Object, delta: DVec2) {
    let Some(mut p) = data(obj) else { return };
    for s in &mut p.splits {
        for v in &mut s.points {
            v.p += delta;
        }
    }
    for z in &mut p.zones {
        z.seed += delta;
        if let Some(o) = &mut z.origin {
            *o += delta;
        }
    }
    for c in &mut p.courses {
        for v in &mut c.path {
            v.p += delta;
        }
    }
    obj.data.insert(
        KEY.into(),
        serde_json::to_value(&p).expect("a plan is JSON"),
    );
}

/// An angle in degrees (the direction of the rows of a pattern) after a transform.
pub fn turn_angle(xf: &crate::edit2d::Xf, degrees: f64) -> f64 {
    let d = xf.m * DVec2::from_angle(degrees.to_radians());
    d.y.atan2(d.x).to_degrees()
}

/// Moves the positions of the plan with a transform of the object (turn, mirror, scale). A mirror
/// puts a course on the other side of its path.
pub fn transform_data(obj: &mut Object, xf: &crate::edit2d::Xf) {
    let Some(mut p) = data(obj) else { return };
    for s in &mut p.splits {
        for v in &mut s.points {
            v.p = xf.apply(v.p);
        }
    }
    for z in &mut p.zones {
        z.seed = xf.apply(z.seed);
        if let Some(o) = &mut z.origin {
            *o = xf.apply(*o);
        }
        // The rows of the pattern turn with the drawing.
        z.angle = z.angle.map(|a| turn_angle(xf, a));
    }
    for c in &mut p.courses {
        for v in &mut c.path {
            v.p = xf.apply(v.p);
        }
        // A path that is copied is mirrored, so the course goes to the other side of it. A course
        // that follows the edge of the hardscape is made again from the (mirrored) edge, which
        // always runs counterclockwise: its side stays. A course that follows a drawn line takes
        // the line as it is then.
        if xf.is_reflection() && c.edge.is_none() && c.source.is_none() {
            c.side = match c.side {
                Side::Left => Side::Right,
                Side::Right => Side::Left,
                Side::Center => Side::Center,
            };
            c.offset = -c.offset;
        }
    }
    obj.data.insert(
        KEY.into(),
        serde_json::to_value(&p).expect("a plan is JSON"),
    );
}

/// The vertices of a drawn object that a split line or a course can follow: a line, an arc or a
/// polyline. `None` for another shape.
pub fn path_of(shape: &Shape) -> Option<(Vec<Vertex>, bool)> {
    match shape {
        Shape::Line { a, b } => Some((vec![Vertex::corner(*a), Vertex::corner(*b)], false)),
        Shape::Polyline { points, closed } => Some((points.clone(), *closed)),
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = if end - start >= std::f64::consts::TAU - 1e-12 {
                std::f64::consts::TAU
            } else {
                (end - start).rem_euclid(std::f64::consts::TAU)
            };
            if sweep <= 0.0 || sweep >= std::f64::consts::TAU - 1e-12 {
                return None;
            }
            let at = |t: f64| *center + DVec2::from_angle(start + sweep * t) * *radius;
            Some((
                vec![
                    Vertex::corner(at(0.0)),
                    Vertex::new(at(0.5), VertexKind::PointOnArc),
                    Vertex::corner(at(1.0)),
                ],
                false,
            ))
        }
        _ => None,
    }
}

/// What a group is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The pattern of the hardscape, laid on what the courses leave.
    Field,
    Zone,
    Course,
}

/// The units of a course, as made.
#[derive(Clone, Debug)]
pub struct CourseBody {
    pub pieces: Vec<course::Piece>,
    /// The unit type of each piece (an index into `kinds`).
    pub piece_kind: Vec<usize>,
    /// The name and the area of each unit type; the first is the regular unit.
    pub kinds: Vec<(String, f64)>,
    pub joints: Joints,
    pub corners: usize,
    pub length: f64,
    pub along: f64,
    pub depth: f64,
    pub gap: f64,
    /// The area that the course takes from the field (see [`course::Course::band`]).
    pub band: Vec<Vec<Vec<DVec2>>>,
}

#[derive(Clone, Debug)]
pub enum Body {
    Pattern { pattern: Pattern, place: Placement },
    Course(Box<CourseBody>),
}

/// A zone whose units are the units of a shared lattice that have their center in it.
///
/// The lattice is laid once for all the zones that share it. Every unit goes to one zone by a
/// grid over the boxes of the zones, and each zone keeps its own list of units. The lists live
/// as long as the zones do.
#[derive(Clone, Debug)]
pub struct Owner {
    /// The zone, in `zones`.
    pub index: usize,
    /// All zones that share the lattice, in the order that decides a center on an edge.
    pub zones: Arc<Vec<Region>>,
    shared: Arc<Shared>,
}

/// What the zones of one lattice share.
#[derive(Debug, Default)]
struct Shared {
    /// The laid lattice, for each trim limit that was asked.
    laid: Mutex<HashMap<u64, Arc<Laid>>>,
}

/// A lattice that was laid and given out to its zones.
#[derive(Debug)]
struct Laid {
    per_zone: Vec<Vec<Unit>>,
    stats: Stats,
    total: usize,
}

/// A uniform grid over the boxes of some regions: the regions that can hold a point.
struct ZoneGrid {
    lo: DVec2,
    cell: DVec2,
    n: usize,
    cells: Vec<Vec<u32>>,
}

impl ZoneGrid {
    fn new(zones: &[Region]) -> Self {
        let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
        for z in zones {
            lo = lo.min(z.lo);
            hi = hi.max(z.hi);
        }
        let n = ((zones.len() as f64).sqrt().ceil() as usize).clamp(1, 256);
        let cell = ((hi - lo) / n as f64).max(DVec2::splat(1e-6));
        let mut g = ZoneGrid {
            lo,
            cell,
            n,
            cells: vec![Vec::new(); n * n],
        };
        for (i, z) in zones.iter().enumerate() {
            let (x0, y0) = g.at(z.lo);
            let (x1, y1) = g.at(z.hi);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    g.cells[y * n + x].push(i as u32);
                }
            }
        }
        g
    }

    fn at(&self, p: DVec2) -> (usize, usize) {
        let q = ((p - self.lo) / self.cell).floor();
        let c = |v: f64| (v.max(0.0) as usize).min(self.n - 1);
        (c(q.x), c(q.y))
    }

    /// The first zone that contains the point, else the nearest.
    fn zone_of(&self, zones: &[Region], p: DVec2) -> usize {
        let (x, y) = self.at(p);
        if let Some(&i) = self.cells[y * self.n + x]
            .iter()
            .find(|&&i| zones::region_contains(&zones[i as usize], p))
        {
            return i as usize;
        }
        let dist = |z: &Region| {
            z.rings()
                .flat_map(|r| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()])))
                .map(|(a, b)| crate::geom::dist_to_segment(p, a, b))
                .fold(f64::MAX, f64::min)
        };
        (0..zones.len())
            .min_by(|&a, &b| dist(&zones[a]).total_cmp(&dist(&zones[b])))
            .unwrap_or(0)
    }
}

impl Owner {
    /// The owner of zone `index` among `zones`.
    pub fn new(index: usize, zones: Arc<Vec<Region>>) -> Self {
        Owner {
            index,
            zones,
            shared: Arc::default(),
        }
    }

    /// The same lattice for another zone of the set.
    pub fn sibling(&self, index: usize) -> Self {
        Owner {
            index,
            zones: self.zones.clone(),
            shared: self.shared.clone(),
        }
    }

    /// The units of the zone, and the stats of the whole lattice. The lattice is laid when the
    /// first zone asks and not again.
    fn units(
        &self,
        region: &Region,
        pattern: &Pattern,
        place: &Placement,
        limits: &Limits,
    ) -> Result<Arc<Laid>, LayoutError> {
        let key = limits.trim.to_bits();
        let mut map = self.shared.laid.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = map.get(&key) {
            if l.total > limits.max_units {
                return Err(LayoutError::TooMany(super::units::TooMany {
                    estimated: l.total as u64,
                    max: limits.max_units,
                }));
            }
            return Ok(l.clone());
        }
        let grid = ZoneGrid::new(&self.zones);
        let mut per_zone: Vec<Vec<Unit>> = vec![Vec::new(); self.zones.len()];
        let mut total = 0usize;
        let stats = super::for_each_unit(region, pattern, place, limits, &mut |u| {
            let Some(ring) = u.rings.first() else { return };
            let c = ring.points.iter().copied().sum::<DVec2>() / ring.points.len() as f64;
            per_zone[grid.zone_of(&self.zones, c)].push(u.clone());
            total += 1;
        })?;
        let laid = Arc::new(Laid {
            per_zone,
            stats,
            total,
        });
        map.insert(key, laid.clone());
        Ok(laid)
    }
}

/// A part of a hardscape that is laid on its own.
#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    /// The name as a layer suffix: letters, digits and dashes.
    pub slug: String,
    pub role: Role,
    /// Where the units are cut to.
    pub region: Region,
    pub body: Body,
    /// Set when the units are laid over a bigger region and the group keeps those whose center
    /// is in its zone.
    pub owner: Option<Owner>,
}

fn number_text(x: f64) -> String {
    let r = (x * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}

/// The name of the unit type of a corner unit.
fn corner_name(poly: &[DVec2]) -> String {
    let (a, b, c) = (poly[0], poly[1], poly[2]);
    let (s1, s2) = (a.distance(b), b.distance(c));
    let angle = (a - b).angle_to(c - b).abs().to_degrees();
    if (angle - 90.0).abs() < 0.05 {
        format!("corner {}x{}", number_text(s1), number_text(s2))
    } else {
        format!(
            "corner {}x{} at {} degrees",
            number_text(s1),
            number_text(s2),
            number_text(angle)
        )
    }
}

impl Group {
    /// The names of the unit types, in the order of `Unit::kind`.
    pub fn kind_names(&self) -> Vec<String> {
        match &self.body {
            Body::Pattern { pattern, .. } => pattern
                .compile()
                .map(|c| c.kinds.iter().map(|k| k.name.clone()).collect())
                .unwrap_or_default(),
            Body::Course(c) => c.kinds.iter().map(|k| k.0.clone()).collect(),
        }
    }

    /// The area of the group, in mm2: its zone when units are given by center.
    pub fn area(&self) -> f64 {
        match &self.owner {
            Some(o) => o.zones[o.index].area,
            None => self.region.area,
        }
    }

    pub fn pattern(&self) -> Option<&Pattern> {
        match &self.body {
            Body::Pattern { pattern, .. } => Some(pattern),
            Body::Course(_) => None,
        }
    }

    /// The gap between units that the group is laid with, in mm.
    pub fn gap(&self) -> f64 {
        match &self.body {
            Body::Pattern { pattern, .. } => pattern.gap,
            Body::Course(c) => c.gap,
        }
    }

    /// The widest joint of the group, in mm.
    pub fn widest_joint(&self) -> f64 {
        match &self.body {
            Body::Pattern { pattern, .. } => pattern.widest_joint(),
            Body::Course(c) => {
                if c.joints.count > 0 {
                    c.joints.widest.max(c.gap)
                } else {
                    c.gap
                }
            }
        }
    }

    /// Lays the units and calls `f` for each. Same limits as [`super::for_each_unit`].
    pub fn for_each_unit(
        &self,
        limits: &Limits,
        f: &mut dyn FnMut(&Unit),
    ) -> Result<Stats, LayoutError> {
        match &self.body {
            Body::Pattern { pattern, place } => {
                let Some(owner) = &self.owner else {
                    return super::for_each_unit(&self.region, pattern, place, limits, f);
                };
                let laid = owner.units(&self.region, pattern, place, limits)?;
                let mut stats = laid.stats.clone();
                let mine = &laid.per_zone[owner.index];
                for u in mine {
                    f(u);
                }
                stats.units = mine.len();
                stats.region_area = owner.zones[owner.index].area;
                Ok(stats)
            }
            Body::Course(c) => {
                if c.pieces.len() > limits.max_units {
                    return Err(LayoutError::TooMany(super::units::TooMany {
                        estimated: c.pieces.len() as u64,
                        max: limits.max_units,
                    }));
                }
                let extent = c.along.max(c.depth);
                let local = Local::new(&self.region, DVec2::ZERO, 0.0, extent);
                let stats = Stats {
                    origin: DVec2::ZERO,
                    angle: 0.0,
                    full_unit_area: c.kinds.first().map_or(0.0, |k| k.1),
                    kinds: c.kinds.clone(),
                    region_area: self.region.area,
                    units: 0,
                    dust_pieces: 0,
                    trimmed: 0,
                };
                let mut placer = Placer::new(
                    local,
                    DVec2::ZERO,
                    DVec2::X,
                    limits.trim.max(0.0),
                    limits.max_units,
                    c.pieces.len() as u64,
                    stats,
                );
                for (pc, &ki) in c.pieces.iter().zip(&c.piece_kind) {
                    let area = crate::geom::polygon_area(&pc.poly);
                    // A unit that a mitre or a corner rule cut is a cut unit, also when the region
                    // does not touch it.
                    let cut_by_rule = area < pc.full_area * (1.0 - 1e-9);
                    placer.place(
                        &pc.poly,
                        ki,
                        area,
                        c.kinds[ki].1,
                        min_width(&pc.poly),
                        &mut |u| {
                            if cut_by_rule && u.whole {
                                let mut v = u.clone();
                                v.whole = false;
                                f(&v)
                            } else {
                                f(u)
                            }
                        },
                    )?;
                }
                Ok(placer.stats)
            }
        }
    }

    /// The counts of the group.
    pub fn summarize(&self, opts: &SummaryOptions) -> Result<Summary, LayoutError> {
        match &self.body {
            Body::Pattern { pattern, place } => {
                if self.owner.is_none() {
                    return super::summary::summarize(&self.region, pattern, place, opts);
                }
                let limits = Limits {
                    max_units: opts.max_units,
                    trim: opts.trim,
                };
                summarize_stream(Desc::of(pattern), self.area(), opts, None, &mut |f| {
                    self.for_each_unit(&limits, f)
                })
            }
            Body::Course(c) => {
                let limits = Limits {
                    max_units: opts.max_units,
                    trim: opts.trim,
                };
                let desc = Desc {
                    bond: "course",
                    unit_length_mm: Some(c.along),
                    unit_width_mm: Some(c.depth),
                    gap_mm: c.gap,
                    widest_joint_mm: self.widest_joint(),
                    narrowest_joint_mm: (c.joints.count > 0).then_some(c.joints.narrowest),
                };
                summarize_stream(desc, self.region.area, opts, None, &mut |f| {
                    self.for_each_unit(&limits, f)
                })
            }
        }
    }
}

/// The parts of a zone that the report and the API list.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneInfo {
    pub name: String,
    pub area: f64,
    /// A point inside the zone.
    pub inside: DVec2,
    pub lo: DVec2,
    pub hi: DVec2,
    /// True if a zone setting of the plan changes the pattern of this zone (pattern, origin,
    /// angle or alignment).
    pub has_setting: bool,
    /// The zone setting of the plan that is tied to this zone (its place in the plan), if any.
    pub spec: Option<usize>,
}

/// The groups of a hardscape and what to tell the person about them.
#[derive(Clone, Debug, Default)]
pub struct Planned {
    pub groups: Vec<Group>,
    /// Things the person must read: a split line that stops short, a setting that no zone holds.
    pub notes: Vec<String>,
    pub zones: Vec<ZoneInfo>,
    /// The split lines as they are now (followed objects read again).
    pub splits: Vec<Vec<DVec2>>,
    /// The courses: the name, the units, the corners and the joints.
    pub courses: Vec<CourseInfo>,
    /// The field, its pattern and its start, when all zones continue it: to count the units that
    /// are cut only by zone edges.
    pub lattice: Option<(Region, Pattern, Placement)>,
}

/// Units that only the zone edges cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneEdgeCuts {
    /// Whole units of the lattice that a zone edge turned into pieces.
    pub whole_lost: usize,
    /// Pieces more than the lattice has: the units that a zone edge splits in two.
    pub extra_pieces: usize,
}

impl Planned {
    /// How many units the zone edges cut, compared with the same field as one area. `None` unless
    /// the field has two zones or more that all continue the pattern of the hardscape.
    pub fn zone_edge_cuts(&self, opts: &SummaryOptions) -> Option<ZoneEdgeCuts> {
        let (region, pattern, place) = self.lattice.as_ref()?;
        let whole = super::summary::summarize(region, pattern, place, opts).ok()?;
        let (mut w, mut c) = (0usize, 0usize);
        for g in self.groups.iter().filter(|g| g.role == Role::Zone) {
            let s = g.summarize(opts).ok()?;
            w += s.whole_units;
            c += s.cut_units;
        }
        Some(ZoneEdgeCuts {
            whole_lost: whole.whole_units.saturating_sub(w),
            extra_pieces: c.saturating_sub(whole.cut_units),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CourseInfo {
    pub name: String,
    pub units: usize,
    pub corners: usize,
    pub length: f64,
    pub joints: Joints,
}

/// A name as a layer suffix.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "X".into() } else { out }
}

/// The split lines as flat polylines, followed objects read again.
pub fn effective_splits(
    doc: &Document,
    plan: &Plan,
    tol: f64,
    notes: &mut Vec<String>,
) -> Vec<Vec<DVec2>> {
    plan.splits
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (pts, closed) = match s.source.and_then(|id| doc.object(id)) {
                Some(o) => match path_of(&o.shape) {
                    Some(p) => p,
                    None => {
                        notes.push(format!(
                            "split line {} follows an object that is not a line, arc or polyline: the saved points are used",
                            i
                        ));
                        (s.points.clone(), false)
                    }
                },
                None => {
                    if let Some(id) = s.source {
                        notes.push(format!(
                            "split line {} followed the object {id}, which is gone: the saved points are used",
                            i
                        ));
                    }
                    (s.points.clone(), false)
                }
            };
            // Arcs of a split line are cut finely: the tolerance of the zones is a distance to join.
            let mut flat = curve::flatten(&pts, closed, SPLIT_FLATTEN.min(tol));
            if closed && let Some(f) = flat.first().copied() {
                flat.push(f);
            }
            flat
        })
        .collect()
}

/// The outline or a hole of a hardscape as a path that runs counterclockwise, and how far the
/// kerb border moves the course from it (to the left of the path).
pub fn edge_path(doc: &Document, obj: &Object, which: &str) -> Result<(Vec<Vertex>, f64), String> {
    let (outer, holes) = super::object::outline_of(obj)?;
    let outline = which == "outline";
    let mut ring = if outline {
        outer
    } else {
        let n: usize = which
            .strip_prefix("hole:")
            .and_then(|n| n.trim().parse().ok())
            .ok_or_else(|| "'along' must be outline or hole:N".to_string())?;
        holes
            .get(n)
            .cloned()
            .ok_or_else(|| format!("the hardscape has no hole {n} (it has {})", holes.len()))?
    };
    let flat = curve::flatten(&ring, true, 1.0);
    if crate::geom::signed_area(&flat) < 0.0 {
        ring.reverse();
    }
    // The kerb border is not paved: the course starts at its inner edge.
    let border = crate::hardscape::data::style_of(doc, obj)
        .and_then(|(_, s)| s.border)
        .map_or(0.0, |b| b.width);
    Ok((ring, if outline { border } else { -border }))
}

/// The course path now: the edge of the hardscape or a followed object read again, else the saved
/// path. The last number is what the kerb border adds to the offset.
pub fn effective_path(
    doc: &Document,
    obj: &Object,
    c: &CourseDef,
    notes: &mut Vec<String>,
) -> (Vec<Vertex>, bool, f64) {
    if let Some(which) = &c.edge {
        match edge_path(doc, obj, which) {
            Ok((p, off)) => return (p, true, off),
            Err(e) => notes.push(format!(
                "course '{}' follows the {which} of the hardscape, which cannot be read now ({e}): the saved path is used",
                c.name
            )),
        }
    }
    if let Some(id) = c.source {
        match doc.object(id).and_then(|o| path_of(&o.shape)) {
            Some((p, closed)) => return (p, closed, 0.0),
            None => notes.push(format!(
                "course '{}' followed the object {id}, which is gone or no longer a line, arc or polyline: the saved path is used",
                c.name
            )),
        }
    }
    (c.path.clone(), c.closed, 0.0)
}

/// The geometry input of a course, as it is now.
pub fn course_spec(
    doc: &Document,
    obj: &Object,
    c: &CourseDef,
    notes: &mut Vec<String>,
) -> course::Spec {
    let (path, closed, extra) = effective_path(doc, obj, c, notes);
    course::Spec {
        path,
        closed,
        side: c.side,
        offset: c.offset + extra,
        along: c.along,
        depth: c.depth,
        gap: c.gap,
        corner: c.corner,
    }
}

/// The union of shapes (each an outline and holes).
fn union_shapes(shapes: &[Vec<Vec<DVec2>>]) -> Vec<Vec<Vec<DVec2>>> {
    let subject: Vec<Vec<[f64; 2]>> = shapes
        .iter()
        .flatten()
        .map(|r| r.iter().map(|p| [p.x, p.y]).collect())
        .collect();
    let empty: Vec<Vec<[f64; 2]>> = Vec::new();
    subject
        .overlay(&empty, OverlayRule::Union, FillRule::NonZero)
        .into_iter()
        .map(|s| {
            s.into_iter()
                .map(|r| r.into_iter().map(|p| DVec2::new(p[0], p[1])).collect())
                .collect()
        })
        .collect()
}

fn subtract(region: &Region, band: &[Vec<Vec<DVec2>>]) -> Result<Option<Region>, String> {
    if band.is_empty() {
        return Ok(Some(region.clone()));
    }
    let ring = |r: &Vec<DVec2>| r.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
    let subject: Vec<Vec<[f64; 2]>> = region.rings().map(ring).collect();
    let clip: Vec<Vec<[f64; 2]>> = band.iter().flatten().map(ring).collect();
    let shapes = subject.overlay(&clip, OverlayRule::Difference, FillRule::EvenOdd);
    let shapes: Vec<Vec<Vec<DVec2>>> = shapes
        .into_iter()
        .map(|s| {
            s.into_iter()
                .map(|r| r.into_iter().map(|p| DVec2::new(p[0], p[1])).collect())
                .collect()
        })
        .collect();
    if shapes.is_empty() {
        return Ok(None);
    }
    Region::from_shapes(shapes).map(Some).map_err(|e| {
        format!("the area that is left when the courses are taken off cannot be made: {e}")
    })
}

/// Makes the groups of a hardscape. `region` is where the pattern goes (the outline inside the
/// kerb border, without the holes), `pattern` and `place` are the pattern and the start of the
/// hardscape with the overrides of the call. `max_units` bounds the units of a course.
pub fn plan_groups(
    doc: &Document,
    obj: &Object,
    region: Region,
    pattern: Pattern,
    place: Placement,
    max_units: usize,
) -> Result<Planned, String> {
    if has_unreadable_data(obj) {
        return Err(format!(
            "the zones and courses of object {} cannot be read (data pattern_plan). Clear them with hardscape.plan_clear",
            obj.id
        ));
    }
    let plan = data(obj).unwrap_or_default();
    let mut out = Planned::default();
    if plan.is_empty() {
        out.groups.push(Group {
            name: "field".into(),
            slug: "FIELD".into(),
            role: Role::Field,
            region,
            owner: None,
            body: Body::Pattern { pattern, place },
        });
        return Ok(out);
    }
    plan.validate()?;
    let tol = plan.tolerance;
    // The pattern starts where it would start on the whole area, so that the zones and the field
    // continue one lattice (a zone with its own settings starts where it says).
    let (origin, angle) = place.resolve(&region);
    let place = Placement {
        origin: Some(origin),
        angle,
    };
    // Courses first: they take their area from the field. A course is cut by the courses before it.
    let mut bands: Vec<Vec<Vec<DVec2>>> = Vec::new();
    let mut course_groups: Vec<Group> = Vec::new();
    let mut budget = max_units;
    for c in &plan.courses {
        let spec = course_spec(doc, obj, c, &mut out.notes);
        let laid = course::lay(&spec, budget).map_err(|e| format!("course '{}': {e}", c.name))?;
        for n in &laid.notes {
            out.notes.push(format!("course '{}': {n}", c.name));
        }
        // Where this course meets the courses before it, its units are cut.
        let mut course_region = Some(region.clone());
        if !bands.is_empty() {
            let overlap: f64 = laid
                .band
                .iter()
                .flat_map(|a| bands.iter().map(move |b| super::shapes_overlap(a, b)))
                .sum();
            if overlap > 1.0 {
                out.notes.push(format!(
                    "course '{}' overlaps an earlier course by {:.3} m2: its units are cut where they meet, with the joint kept",
                    c.name,
                    overlap / 1e6
                ));
                course_region = subtract(&region, &bands)?;
            }
        }
        let Some(course_region) = course_region else {
            out.notes.push(format!(
                "course '{}' lies entirely inside the earlier courses: it has no units",
                c.name
            ));
            out.courses.push(CourseInfo {
                name: c.name.clone(),
                units: 0,
                corners: laid.corners,
                length: laid.length,
                joints: laid.joints,
            });
            continue;
        };
        budget = budget.saturating_sub(laid.pieces.len());
        let unit = if c.unit.trim().is_empty() {
            super::bond::size_name(c.along, c.depth)
        } else {
            c.unit.clone()
        };
        let mut kinds = vec![(unit, c.along * c.depth)];
        let mut piece_kind = Vec::with_capacity(laid.pieces.len());
        for p in &laid.pieces {
            if p.corner_unit {
                let name = corner_name(&p.poly);
                let idx = match kinds.iter().position(|k| k.0 == name) {
                    Some(i) => i,
                    None => {
                        kinds.push((name, p.full_area));
                        kinds.len() - 1
                    }
                };
                piece_kind.push(idx);
            } else {
                piece_kind.push(0);
            }
        }
        out.courses.push(CourseInfo {
            name: c.name.clone(),
            units: laid.pieces.len(),
            corners: laid.corners,
            length: laid.length,
            joints: laid.joints,
        });
        bands = union_shapes(&[bands, laid.band.clone()].concat());
        course_groups.push(Group {
            name: c.name.clone(),
            slug: format!("COURSE-{}", slug(&c.name)),
            role: Role::Course,
            region: course_region,
            owner: None,
            body: Body::Course(Box::new(CourseBody {
                pieces: laid.pieces,
                piece_kind,
                kinds,
                joints: laid.joints,
                corners: laid.corners,
                length: laid.length,
                along: c.along,
                depth: c.depth,
                gap: c.gap,
                band: laid.band,
            })),
        });
    }
    let field = subtract(&region, &bands)?;
    if let Some(field) = field {
        if plan.splits.is_empty() {
            out.groups.push(Group {
                name: "field".into(),
                slug: "FIELD".into(),
                role: Role::Field,
                region: field,
                owner: None,
                body: Body::Pattern { pattern, place },
            });
        } else {
            out.splits = effective_splits(doc, &plan, tol, &mut out.notes);
            let split = zones::split(&field, &out.splits, tol)?;
            if split.zones.len() > MAX_ZONE_COUNT {
                return Err(format!(
                    "the split lines make {} zones, more than the limit of {MAX_ZONE_COUNT}. Use fewer lines",
                    split.zones.len()
                ));
            }
            for e in &split.loose_ends {
                out.notes.push(format!(
                    "split line {} stops inside the area at ({:.1}, {:.1}), {:.1} mm from the nearest edge or line: it cuts nothing there",
                    e.line, e.at.x, e.at.y, e.gap
                ));
            }
            for i in &split.unused_lines {
                out.notes.push(format!(
                    "split line {} cuts nothing: it does not cross the area",
                    i
                ));
            }
            // Settings by seed.
            let mut spec_of: Vec<Option<usize>> = vec![None; split.zones.len()];
            for (si, z) in plan.zones.iter().enumerate() {
                let hit = split.zones.iter().position(|zone| {
                    zones::region_contains(
                        &Region {
                            shapes: vec![zone.shape.clone()],
                            area: zone.area,
                            lo: zone.lo,
                            hi: zone.hi,
                        },
                        z.seed,
                    )
                });
                match hit {
                    Some(zi) if spec_of[zi].is_none() => spec_of[zi] = Some(si),
                    Some(zi) => out.notes.push(format!(
                        "zone setting {} and {} are both tied to zone {}: the first one is used",
                        spec_of[zi].unwrap_or(0) + 1,
                        si,
                        zi
                    )),
                    None => out.notes.push(format!(
                        "zone setting {}{} is tied to ({:.0}, {:.0}), which is in no zone now: it is not used",
                        si,
                        z.name.as_ref().map(|n| format!(" ({n})")).unwrap_or_default(),
                        z.seed.x,
                        z.seed.y
                    )),
                }
            }
            let single = split.zones.len() == 1;
            // Zones that continue the pattern and give each unit to the zone of its center.
            let by_center: Vec<bool> = (0..split.zones.len())
                .map(|zi| {
                    plan.zone_units == ZoneUnits::Centroid
                        && !single
                        && !spec_of[zi].is_some_and(|i| plan.zones[i].has_overrides())
                })
                .collect();
            let center_zones: Vec<Region> = split
                .zones
                .iter()
                .zip(&by_center)
                .filter(|(_, c)| **c)
                .map(|(z, _)| Region::from_shapes(vec![z.shape.clone()]))
                .collect::<Result<_, _>>()?;
            let center_zones = Arc::new(center_zones);
            // The lattice of the centered zones: the field less the zones that are cut.
            let cut_shapes: Vec<Vec<Vec<DVec2>>> = split
                .zones
                .iter()
                .zip(&by_center)
                .filter(|(_, c)| !**c)
                .map(|(z, _)| z.shape.clone())
                .collect();
            let center_region = if center_zones.is_empty() {
                None
            } else if cut_shapes.is_empty() {
                Some(field.clone())
            } else {
                subtract(&field, &cut_shapes)?
            };
            let mut center_index = 0usize;
            let mut first_owner: Option<Owner> = None;
            for (zi, zone) in split.zones.iter().enumerate() {
                let spec = spec_of[zi].map(|i| &plan.zones[i]);
                let name = spec
                    .and_then(|s| s.name.clone())
                    .unwrap_or_else(|| format!("zone {zi}"));
                let zregion = Region::from_shapes(vec![zone.shape.clone()])?;
                let mut zpattern = pattern.clone();
                let mut zplace = place;
                if let Some(s) = spec {
                    if let Some(p) = &s.pattern {
                        zpattern = p.clone();
                    }
                    if let Some(n) = s.align_to_edge {
                        let outline = curve::corners(&zone.shape[0]);
                        zplace = Placement::along_edge(&outline, n)
                            .map_err(|e| format!("zone '{name}': {e}"))?;
                    }
                    if let Some(n) = s.align_to_split {
                        let line = out.splits.get(n).ok_or_else(|| {
                            format!("zone '{name}': split line {} does not exist", n)
                        })?;
                        if line.len() < 2 {
                            return Err(format!("zone '{name}': split line {} is empty", n));
                        }
                        let d = line[1] - line[0];
                        zplace = Placement {
                            origin: Some(line[0]),
                            angle: d.y.atan2(d.x).to_degrees(),
                        };
                    }
                    if let Some(o) = s.origin {
                        zplace.origin = Some(o);
                    }
                    if let Some(a) = s.angle {
                        zplace.angle = a;
                    }
                }
                zpattern
                    .validate()
                    .map_err(|e| format!("zone '{name}': {e}"))?;
                out.zones.push(ZoneInfo {
                    name: name.clone(),
                    area: zone.area,
                    inside: zone.inside,
                    lo: zone.lo,
                    hi: zone.hi,
                    has_setting: spec.is_some_and(ZoneSpec::has_overrides),
                    spec: spec_of[zi],
                });
                let (region_of_group, owner) = if by_center[zi] {
                    let o = match &first_owner {
                        Some(f) => f.sibling(center_index),
                        None => Owner::new(center_index, center_zones.clone()),
                    };
                    if first_owner.is_none() {
                        first_owner = Some(o.clone());
                    }
                    center_index += 1;
                    (center_region.clone().unwrap_or(zregion), Some(o))
                } else {
                    (zregion, None)
                };
                out.groups.push(Group {
                    slug: if single {
                        "FIELD".into()
                    } else {
                        format!("ZONE-{}", slug(&name))
                    },
                    name: if single && spec.is_none() {
                        "field".into()
                    } else {
                        name
                    },
                    role: if single { Role::Field } else { Role::Zone },
                    region: region_of_group,
                    owner,
                    body: Body::Pattern {
                        pattern: zpattern,
                        place: zplace,
                    },
                });
            }
            let all_continue = !single
                && plan.zone_units == ZoneUnits::Cut
                && out.zones.iter().all(|z| !z.has_setting);
            if all_continue {
                out.lattice = Some((field.clone(), pattern.clone(), place));
            }
        }
    } else {
        out.notes
            .push("the courses take the whole area: no field is left".into());
    }
    out.groups.extend(course_groups);
    // Zone names are layer suffixes: two zones with one slug would share layers.
    let mut used = std::collections::HashSet::<String>::new();
    for g in &mut out.groups {
        let base = g.slug.clone();
        let mut n = 2;
        while !used.insert(g.slug.clone()) {
            g.slug = format!("{base}-{n}");
            n += 1;
        }
    }
    Ok(out)
}

/// The groups of a hardscape with a joint pattern, as the report and the export lay them. `None`
/// if the hardscape has no joint pattern. `tolerance` flattens curves; `max_units` bounds the
/// units of the courses.
pub fn groups_of(
    doc: &Document,
    obj: &Object,
    tolerance: f64,
    max_units: usize,
) -> Result<Option<Planned>, String> {
    let Some((pattern, place)) = super::object::pattern_of(doc, obj) else {
        return Ok(None);
    };
    let region = super::object::region_of(doc, obj, tolerance)?;
    plan_groups(doc, obj, region, pattern, place, max_units).map(Some)
}

/// Adds the summaries of the groups of one hardscape into one. `region_area` is the area of the
/// hardscape (a course lies inside it and is not added).
pub fn merge(parts: &[Summary], region_area_mm2: f64, opts: &SummaryOptions) -> Summary {
    let first = &parts[0];
    let mut s = first.clone();
    let mut by_unit: Vec<UnitCount> = Vec::new();
    s.whole_units = 0;
    s.cut_units = 0;
    s.slivers = 0;
    s.unfillable_pieces = 0;
    s.dust_pieces = 0;
    s.trimmed_units = 0;
    s.area_laid_m2 = 0.0;
    s.cut_area_m2 = 0.0;
    s.order_units = 0;
    s.order_units_max = 0;
    let mut gap_len: Option<f64> = Some(0.0);
    for p in parts {
        s.whole_units += p.whole_units;
        s.cut_units += p.cut_units;
        s.slivers += p.slivers;
        s.unfillable_pieces += p.unfillable_pieces;
        s.dust_pieces += p.dust_pieces;
        s.trimmed_units += p.trimmed_units;
        s.area_laid_m2 += p.area_laid_m2;
        s.cut_area_m2 += p.cut_area_m2;
        s.order_units += p.order_units;
        s.order_units_max += p.order_units_max;
        s.widest_joint_mm = s.widest_joint_mm.max(p.widest_joint_mm);
        s.narrowest_joint_mm = match (s.narrowest_joint_mm, p.narrowest_joint_mm) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if s.joint_warning.is_none() {
            s.joint_warning = p.joint_warning.clone();
        }
        if p.bond != s.bond {
            s.bond = "mixed";
        }
        if p.unit_length_mm != s.unit_length_mm || p.unit_width_mm != s.unit_width_mm {
            s.unit_length_mm = None;
            s.unit_width_mm = None;
        }
        gap_len = match (gap_len, p.gap_length_m) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        for u in &p.by_unit {
            match by_unit.iter_mut().find(|x| x.name == u.name) {
                Some(x) => {
                    x.whole_units += u.whole_units;
                    x.cut_units += u.cut_units;
                    x.slivers += u.slivers;
                    x.unfillable_pieces += u.unfillable_pieces;
                    x.area_laid_m2 += u.area_laid_m2;
                    x.order_units += u.order_units;
                    x.cut_share += u.cut_share;
                }
                None => by_unit.push(u.clone()),
            }
        }
    }
    for u in &mut by_unit {
        u.set_min();
        u.order_units_with_waste = opts.waste_percent.map(|w| with_waste(u.order_units, w));
    }
    let cut_share: f64 = by_unit.iter().map(|u| u.cut_share).sum();
    s.order_units_min = s.whole_units + (cut_share - 1e-9).max(0.0).ceil() as usize;
    s.order_units_with_waste = opts.waste_percent.map(|w| with_waste(s.order_units, w));
    s.gap_length_m = gap_len;
    s.region_area_m2 = region_area_mm2 / 1e6;
    s.gap_area_m2 = (s.region_area_m2 - s.area_laid_m2).max(0.0);
    s.unit_area_m2 = (by_unit.len() == 1).then(|| by_unit[0].unit_area_m2);
    s.by_unit = by_unit;
    s
}

#[cfg(test)]
mod tests;
