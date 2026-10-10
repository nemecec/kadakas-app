//! Builds the hydraulic network from the irrigation objects of a document.
//!
//! # Topology from geometry
//!
//! Points that lie within the tolerance of each other (default 50 mm) are one node. The points are
//! every vertex of every irrigation line, and the positions of POCs and valves. So lines connect
//! when their ends (or vertices) meet, and a valve or POC sits on a node when it is at the end of
//! a line. Each line segment is a pipe. An outlet connects to the nearest node within the
//! tolerance. An outlet with no node close by has no water.
//!
//! A drip line uses `floor(length / emitter spacing)` emitters (at least 1) of `emitter_flow` l/h.
//! Their flow counts at the last point of the line, so draw drip lines from the supply end.

use std::collections::HashMap;

use cadektor_core::DVec2;
use cadektor_core::ground::GroundSource;
use cadektor_core::irrigation::{IrrigationShape, LineType};
use cadektor_core::model::{Document, Object};
use cadektor_core::uid::Uid;

use crate::catalog::{Catalog, HeadKind};
use crate::network::{
    Demand, DripInfo, Edge, Network, Node, NodeId, Poc, ValveSpec, Warning, WarningKind, solve,
};

/// Default snapping tolerance in mm.
pub const DEFAULT_TOLERANCE: f64 = 50.0;
/// Default emitter spacing of a drip line in mm.
pub const DEFAULT_EMITTER_SPACING: f64 = 300.0;
/// Default emitter flow of a drip line in l/h.
pub const DEFAULT_EMITTER_FLOW: f64 = 2.0;
/// Lowest pressure at the emitters of a drip line in kPa.
pub const DRIP_MIN_KPA: f64 = 70.0;
/// Hazen-Williams coefficient when a material is not in the catalog.
pub const DEFAULT_C: f64 = 140.0;
/// Diameter in mm of the links between outlets and nodes (no loss).
const LINK_DIAMETER: f64 = 1000.0;

/// An outlet with the values from the catalog.
#[derive(Clone, Debug, PartialEq)]
pub struct OutletInfo {
    pub id: Uid,
    pub at: DVec2,
    pub model: String,
    pub kind: Option<HeadKind>,
    pub arc: f64,
    pub direction: f64,
    /// The radius in mm: the own radius of the outlet, else the one of the model.
    pub radius: Option<f64>,
    /// Flow in l/min. Zero when the model is unknown.
    pub flow_lpm: f64,
    pub min_kpa: f64,
    pub price: f64,
    pub node: Option<NodeId>,
}

/// An irrigation line with the values from the catalog.
#[derive(Clone, Debug, PartialEq)]
pub struct LineInfo {
    pub id: Uid,
    pub line_type: LineType,
    pub size: String,
    pub material: String,
    pub length_m: f64,
    pub inner_diameter_mm: Option<f64>,
    pub price_per_m: f64,
    /// Total flow of the emitters in l/min (drip lines only).
    pub emitter_flow_lpm: f64,
    /// Emitter spacing in mm (drip lines only).
    pub emitter_spacing: Option<f64>,
    /// The points of the line as drawn.
    pub points: Vec<DVec2>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValveInfo {
    pub id: Uid,
    pub zone: u32,
    pub name: String,
    pub kv: f64,
    pub controller: Option<String>,
    pub station: Option<u32>,
    pub node: NodeId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PocInfo {
    pub id: Uid,
    pub name: String,
    pub static_kpa: f64,
    pub available_lpm: f64,
    pub supply_loss_kpa: f64,
    pub meter_loss_kpa: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControllerInfo {
    pub id: Uid,
    pub name: String,
    pub stations: u32,
}

/// The network and the facts about the objects.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extraction {
    pub network: Network,
    pub outlets: Vec<OutletInfo>,
    pub lines: Vec<LineInfo>,
    pub valves: Vec<ValveInfo>,
    pub pocs: Vec<PocInfo>,
    pub controllers: Vec<ControllerInfo>,
    /// Problems found while reading the objects (catalog gaps, spray angles, controllers).
    pub warnings: Vec<Warning>,
}

struct Dsu(Vec<usize>);

impl Dsu {
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut c = x;
        while self.0[c] != r {
            let next = self.0[c];
            self.0[c] = r;
            c = next;
        }
        r
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            // The lower index stays the root, so results do not depend on the order of unions.
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// Groups points that are within `tol` of each other. Returns the group number of each point
/// (numbered by first appearance).
pub fn cluster(points: &[DVec2], tol: f64) -> Vec<usize> {
    let tol = tol.max(1e-9);
    let cell = |p: DVec2| ((p.x / tol).floor() as i64, (p.y / tol).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, p) in points.iter().enumerate() {
        grid.entry(cell(*p)).or_default().push(i);
    }
    let mut dsu = Dsu((0..points.len()).collect());
    for (i, p) in points.iter().enumerate() {
        let (cx, cy) = cell(*p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = grid.get(&(cx + dx, cy + dy)) {
                    for &j in v {
                        if j > i && p.distance(points[j]) <= tol {
                            dsu.union(i, j);
                        }
                    }
                }
            }
        }
    }
    let mut ids: HashMap<usize, usize> = HashMap::new();
    (0..points.len())
        .map(|i| {
            let r = dsu.find(i);
            let next = ids.len();
            *ids.entry(r).or_insert(next)
        })
        .collect()
}

fn warn(out: &mut Vec<Warning>, kind: WarningKind, object: Uid, message: String) {
    out.push(Warning {
        kind,
        object: Some(object),
        zone: None,
        message,
    });
}

/// Options of the extraction.
#[derive(Clone, Copy)]
pub struct ExtractOptions<'a> {
    /// Snapping tolerance in mm.
    pub tolerance: f64,
    /// Allowance for fittings as a percentage of the pipe losses.
    pub fittings_pct: f64,
    /// The ground (site model): nodes take their height from it, and water has to climb.
    pub ground: Option<&'a dyn GroundSource>,
}

impl Default for ExtractOptions<'_> {
    fn default() -> Self {
        ExtractOptions {
            tolerance: DEFAULT_TOLERANCE,
            fittings_pct: 0.0,
            ground: None,
        }
    }
}

/// A grid of points for neighbor queries within one tolerance.
struct PointGrid {
    cell: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl PointGrid {
    fn new(points: &[DVec2], tol: f64) -> PointGrid {
        let cell = tol.max(1e-9);
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, p) in points.iter().enumerate() {
            cells.entry(Self::key(cell, *p)).or_default().push(i);
        }
        PointGrid { cell, cells }
    }

    fn key(cell: f64, p: DVec2) -> (i64, i64) {
        ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64)
    }

    /// Indexes of the points in the 3 x 3 cells around `p`.
    fn near(&self, p: DVec2) -> impl Iterator<Item = usize> + '_ {
        let (cx, cy) = Self::key(self.cell, p);
        (-1..=1).flat_map(move |dx| {
            (-1..=1).flat_map(move |dy| {
                self.cells
                    .get(&(cx + dx, cy + dy))
                    .into_iter()
                    .flat_map(|v| v.iter().copied())
            })
        })
    }
}

/// Adds a vertex to a line wherever one of the `taps` (heads, line ends, valves, POCs) lies
/// within `tol` of the inside of a segment, so that they connect in the middle of a segment.
fn split_at_taps(pts: &[DVec2], taps: &[DVec2], grid: &PointGrid, tol: f64) -> Vec<DVec2> {
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let len = ab.length();
        let mut found: Vec<(f64, DVec2)> = vec![];
        if len > 2.0 * tol {
            // Look at the cells along the segment (a cap keeps very long segments cheap).
            let steps = ((len / tol).ceil() as usize).clamp(1, 20_000);
            let mut seen: Vec<usize> = vec![];
            for s in 0..=steps {
                seen.extend(grid.near(a + ab * (s as f64 / steps as f64)));
            }
            seen.sort_unstable();
            seen.dedup();
            for q in seen {
                let t = (taps[q] - a).dot(ab) / (len * len);
                if t <= 0.0 || t >= 1.0 {
                    continue;
                }
                let foot = a + ab * t;
                if taps[q].distance(foot) <= tol
                    && taps[q].distance(a) > tol
                    && taps[q].distance(b) > tol
                {
                    found.push((t, foot));
                }
            }
            found.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut last = a;
            for (_, foot) in found {
                if foot.distance(last) > tol && foot.distance(b) > tol {
                    out.push(foot);
                    last = foot;
                }
            }
        }
        out.push(b);
    }
    out
}

/// Reads the irrigation objects of the document into a network (no ground, no fittings).
pub fn extract(doc: &Document, catalog: &Catalog, tol: f64) -> Extraction {
    extract_with(
        doc,
        catalog,
        &ExtractOptions {
            tolerance: tol,
            ..ExtractOptions::default()
        },
    )
}

/// Reads the irrigation objects of the document into a network.
pub fn extract_with(doc: &Document, catalog: &Catalog, opts: &ExtractOptions) -> Extraction {
    let tol = opts.tolerance;
    let mut ex = Extraction::default();
    ex.network.fittings_pct = opts.fittings_pct;
    let objs: Vec<(&Object, &IrrigationShape)> = doc
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            cadektor_core::model::Shape::Irrigation(i) => Some((o, i.as_ref())),
            _ => None,
        })
        .collect();

    // Points that tap a line in the middle of a segment: heads, line ends, valves and POCs.
    let mut taps: Vec<DVec2> = vec![];
    for (_, i) in &objs {
        match i {
            IrrigationShape::Line { points, .. } => {
                taps.extend([points[0], points[points.len() - 1]]);
            }
            IrrigationShape::Controller { .. } => {}
            other => taps.push(other.anchor()),
        }
    }
    let tap_grid = PointGrid::new(&taps, tol);

    // Points that become nodes, and who owns each point.
    enum Owner {
        LineVertex,
        Poc(usize),
        Valve(usize),
    }
    let mut points: Vec<DVec2> = vec![];
    let mut owners: Vec<Owner> = vec![];
    // The vertices of each line (with the added ones), and where they start in `points`.
    let mut line_points: Vec<Vec<DVec2>> = vec![];
    let mut line_starts: Vec<usize> = vec![];
    // An object on the layer of each point, for the height of the ground.
    let mut reps: Vec<&Object> = vec![];
    for (o, i) in &objs {
        match i {
            IrrigationShape::Line {
                points: pts,
                line_type,
                ..
            } => {
                // A drip line is one pipe with its emitters spread along it.
                let pts = if *line_type == LineType::Drip || pts.len() < 2 {
                    pts.clone()
                } else {
                    split_at_taps(pts, &taps, &tap_grid, tol)
                };
                line_starts.push(points.len());
                for p in &pts {
                    points.push(*p);
                    owners.push(Owner::LineVertex);
                    reps.push(o);
                }
                line_points.push(pts);
            }
            IrrigationShape::Poc {
                at,
                name,
                static_pressure,
                available_flow,
                supply_loss,
                meter_loss,
            } => {
                ex.pocs.push(PocInfo {
                    id: o.id.0,
                    name: name.clone(),
                    static_kpa: *static_pressure,
                    available_lpm: *available_flow,
                    supply_loss_kpa: supply_loss.unwrap_or(0.0),
                    meter_loss_kpa: meter_loss.unwrap_or(0.0),
                });
                points.push(*at);
                owners.push(Owner::Poc(ex.pocs.len() - 1));
                reps.push(o);
            }
            IrrigationShape::Valve {
                at,
                zone,
                name,
                kv,
                controller,
                station,
            } => {
                ex.valves.push(ValveInfo {
                    id: o.id.0,
                    zone: *zone,
                    name: name.clone(),
                    kv: *kv,
                    controller: controller.clone(),
                    station: *station,
                    node: 0,
                });
                points.push(*at);
                owners.push(Owner::Valve(ex.valves.len() - 1));
                reps.push(o);
            }
            IrrigationShape::Controller { name, stations, .. } => {
                ex.controllers.push(ControllerInfo {
                    id: o.id.0,
                    name: name.clone(),
                    stations: *stations,
                })
            }
            IrrigationShape::Outlet { .. } => {}
        }
    }

    // One node per cluster.
    let groups = cluster(&points, tol);
    let n_nodes = groups.iter().max().map_or(0, |m| m + 1);
    for _ in 0..n_nodes {
        ex.network.add_node(Node::default());
    }
    let mut node_rep: Vec<Option<(usize, DVec2)>> = vec![None; n_nodes];
    for (k, owner) in owners.iter().enumerate() {
        if node_rep[groups[k]].is_none() {
            node_rep[groups[k]] = Some((k, points[k]));
        }
        let node = &mut ex.network.nodes[groups[k]];
        match owner {
            Owner::LineVertex => {}
            Owner::Poc(i) => {
                let p = &ex.pocs[*i];
                if node.poc.is_none() {
                    node.poc = Some(Poc {
                        static_kpa: p.static_kpa,
                        available_lpm: p.available_lpm,
                        supply_loss_kpa: p.supply_loss_kpa,
                        meter_loss_kpa: p.meter_loss_kpa,
                    });
                    node.object = node.object.or(Some(p.id));
                    node.label = if p.name.is_empty() {
                        format!("POC #{}", p.id)
                    } else {
                        p.name.clone()
                    };
                }
            }
            Owner::Valve(i) => {
                let v = &mut ex.valves[*i];
                v.node = groups[k];
                if node.valve.is_none() {
                    node.valve = Some(ValveSpec {
                        zone: v.zone,
                        kv: v.kv,
                    });
                    if node.object.is_none() {
                        node.object = Some(v.id);
                    }
                    if node.poc.is_none() {
                        node.label = if v.name.is_empty() {
                            format!("Valve zone {}", v.zone)
                        } else {
                            v.name.clone()
                        };
                    }
                }
            }
        }
    }
    if let Some(g) = opts.ground {
        for (ni, rep) in node_rep.iter().enumerate() {
            if let Some((k, p)) = rep {
                ex.network.nodes[ni].z_m = g.elevation(reps[*k], *p).map(|z| z / 1000.0);
            }
        }
    }

    // Pipes. A drip line is one pipe with emitters along it.
    let mut line_no = 0usize;
    for (o, i) in &objs {
        let IrrigationShape::Line {
            points: orig,
            line_type,
            size,
            material,
            emitter_spacing,
            emitter_flow,
        } = i
        else {
            continue;
        };
        let id = o.id.0;
        let pts = &line_points[line_no];
        let base = line_starts[line_no];
        line_no += 1;
        let pipe = catalog.pipe(size, material);
        let c = catalog.material_c(material);
        if pipe.is_none() {
            warn(
                &mut ex.warnings,
                WarningKind::Catalog,
                id,
                format!(
                    "pipe {material} {size} is not in the catalog; it is calculated without loss"
                ),
            );
        }
        if c.is_none() {
            warn(
                &mut ex.warnings,
                WarningKind::Catalog,
                id,
                format!(
                    "material {material} has no roughness coefficient; C = {DEFAULT_C} is used"
                ),
            );
        }
        let length_mm: f64 = orig.windows(2).map(|w| w[0].distance(w[1])).sum();
        let (spacing, flow_lph) = (
            emitter_spacing.unwrap_or(DEFAULT_EMITTER_SPACING),
            emitter_flow.unwrap_or(DEFAULT_EMITTER_FLOW),
        );
        let emitters = if *line_type == LineType::Drip {
            (length_mm / spacing).floor().max(1.0)
        } else {
            0.0
        };
        let emitter_lpm = emitters * flow_lph / 60.0;
        ex.lines.push(LineInfo {
            id,
            line_type: *line_type,
            size: size.clone(),
            material: material.clone(),
            length_m: length_mm / 1000.0,
            inner_diameter_mm: pipe.map(|p| p.inner_diameter),
            price_per_m: pipe.map_or(0.0, |p| p.price),
            emitter_flow_lpm: emitter_lpm,
            emitter_spacing: (*line_type == LineType::Drip).then_some(spacing),
            points: orig.clone(),
        });
        let d = pipe.map_or(LINK_DIAMETER, |p| p.inner_diameter);
        let c = c.unwrap_or(DEFAULT_C);
        if *line_type == LineType::Drip {
            let (a, b) = (groups[base], groups[base + pts.len() - 1]);
            if a != b {
                ex.network.add_edge(Edge {
                    a,
                    b,
                    length_m: length_mm / 1000.0,
                    inner_diameter_mm: d,
                    c,
                    object: Some(id),
                    through: false,
                    drip: Some(DripInfo {
                        flow_lpm: emitter_lpm,
                        emitters: emitters as usize,
                        min_kpa: DRIP_MIN_KPA,
                    }),
                });
            }
            continue;
        }
        for k in 0..pts.len().saturating_sub(1) {
            let (a, b) = (groups[base + k], groups[base + k + 1]);
            if a != b {
                ex.network.add_edge(Edge {
                    a,
                    b,
                    length_m: pts[k].distance(pts[k + 1]) / 1000.0,
                    inner_diameter_mm: d,
                    c,
                    object: Some(id),
                    through: *line_type == LineType::Mainline,
                    drip: None,
                });
            }
        }
    }

    // Outlets attach to the nearest node within the tolerance.
    let point_grid = PointGrid::new(&points, tol);
    for (o, i) in &objs {
        let IrrigationShape::Outlet {
            at,
            model,
            arc,
            direction,
            radius,
            ..
        } = i
        else {
            continue;
        };
        let id = o.id.0;
        let head = catalog.head(model);
        let mut flow = 0.0;
        let mut min_kpa = 0.0;
        let mut price = 0.0;
        match head {
            None => warn(
                &mut ex.warnings,
                WarningKind::Catalog,
                id,
                format!("head model {model} is not in the catalog; it uses no water here"),
            ),
            Some(h) => {
                flow = h.flow_at_arc(*arc);
                min_kpa = h.min_required();
                price = h.price;
                if h.kind.flow_scales_with_arc()
                    && (*arc < h.min_arc - 1e-9 || *arc > h.max_arc + 1e-9)
                {
                    warn(
                        &mut ex.warnings,
                        WarningKind::ArcRange,
                        id,
                        format!(
                            "the arc {arc:.0} degrees is outside {:.0} to {:.0} degrees of {}",
                            h.min_arc, h.max_arc, h.name
                        ),
                    );
                }
            }
        }
        let z_m = opts
            .ground
            .and_then(|g| g.elevation(o, *at))
            .map(|z| z / 1000.0);
        let node = ex.network.add_node(Node {
            object: Some(id),
            label: format!("Outlet #{id} ({model})"),
            demand: (flow > 0.0).then_some(Demand {
                flow_lpm: flow,
                min_kpa,
            }),
            z_m,
            ..Node::default()
        });
        // The nearest point within the tolerance.
        let near = point_grid
            .near(*at)
            .filter(|&k| points[k].distance(*at) <= tol)
            .min_by(|&a, &b| {
                points[a]
                    .distance(*at)
                    .total_cmp(&points[b].distance(*at))
                    .then(a.cmp(&b))
            });
        if let Some(k) = near {
            ex.network.add_edge(Edge {
                a: groups[k],
                b: node,
                length_m: 0.0,
                inner_diameter_mm: LINK_DIAMETER,
                c: DEFAULT_C,
                object: None,
                through: false,
                drip: None,
            });
        }
        ex.outlets.push(OutletInfo {
            id,
            at: *at,
            model: model.clone(),
            kind: head.map(|h| h.kind),
            arc: *arc,
            direction: *direction,
            radius: radius.or(head.map(|h| h.radius)),
            flow_lpm: flow,
            min_kpa,
            price,
            node: Some(node),
        });
    }

    check_controllers(&mut ex);
    ex
}

/// Valves that name a controller need an existing controller and a free station in its range.
fn check_controllers(ex: &mut Extraction) {
    let mut used: Vec<(String, u32, Uid)> = vec![];
    let mut out = vec![];
    for v in &ex.valves {
        let Some(name) = &v.controller else { continue };
        let Some(c) = ex
            .controllers
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
        else {
            warn(
                &mut out,
                WarningKind::Controller,
                v.id,
                format!(
                    "the valve of zone {} names controller '{name}', which does not exist",
                    v.zone
                ),
            );
            continue;
        };
        let Some(st) = v.station else { continue };
        if st == 0 || st > c.stations {
            warn(
                &mut out,
                WarningKind::Controller,
                v.id,
                format!(
                    "station {st} is outside 1 to {} of controller '{}'",
                    c.stations, c.name
                ),
            );
        } else if let Some(other) = used
            .iter()
            .find(|(n, s, _)| n.eq_ignore_ascii_case(name) && *s == st)
        {
            warn(
                &mut out,
                WarningKind::Controller,
                v.id,
                format!(
                    "station {st} of controller '{}' is also used by object {}",
                    c.name, other.2
                ),
            );
        } else {
            used.push((name.clone(), st, v.id));
        }
    }
    ex.warnings.extend(out);
}

/// Everything a calculation produces.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub catalog: Catalog,
    pub extraction: Extraction,
    pub solution: crate::network::Solution,
}

impl Analysis {
    /// All warnings: from reading the objects, then from the solver.
    pub fn warnings(&self) -> Vec<&Warning> {
        self.extraction
            .warnings
            .iter()
            .chain(self.solution.all_warnings())
            .collect()
    }

    /// The zone of each outlet object that has water (by object ID).
    pub fn outlet_zones(&self) -> HashMap<Uid, u32> {
        let mut m = HashMap::new();
        for z in &self.solution.zones {
            for o in &z.outlets {
                if let Some(id) = o.object {
                    m.insert(id, z.zone);
                }
            }
        }
        m
    }
}

/// Reads the document and solves the network with the catalog of the document.
pub fn analyze(doc: &Document, tol: f64) -> Analysis {
    analyze_with(
        doc,
        &ExtractOptions {
            tolerance: tol,
            ..ExtractOptions::default()
        },
    )
}

/// Like `analyze`, with a fittings allowance and the ground of a site model.
pub fn analyze_with(doc: &Document, opts: &ExtractOptions) -> Analysis {
    let catalog = Catalog::of_document(doc);
    let extraction = extract_with(doc, &catalog, opts);
    let solution = solve(&extraction.network);
    Analysis {
        catalog,
        extraction,
        solution,
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::{Attributes, Shape};

    pub fn add(ed: &mut Editor, s: IrrigationShape) -> cadektor_core::uid::Uid {
        ed.execute(&Command::AddObject {
            shape: Shape::Irrigation(Box::new(s)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0]
    }

    pub fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    pub fn line(points: &[DVec2], t: LineType, size: &str, material: &str) -> IrrigationShape {
        IrrigationShape::Line {
            points: points.to_vec(),
            line_type: t,
            size: size.into(),
            material: material.into(),
            emitter_spacing: None,
            emitter_flow: None,
        }
    }

    pub fn outlet(at: DVec2, model: &str, arc: f64) -> IrrigationShape {
        IrrigationShape::Outlet {
            at,
            model: model.into(),
            arc,
            direction: 0.0,
            radius: None,
            zone: None,
        }
    }

    pub fn poc(at: DVec2, kpa: f64, lpm: f64) -> IrrigationShape {
        IrrigationShape::Poc {
            at,
            name: "Meter".into(),
            static_pressure: kpa,
            available_flow: lpm,
            supply_loss: None,
            meter_loss: None,
        }
    }

    pub fn valve(at: DVec2, zone: u32, kv: f64) -> IrrigationShape {
        IrrigationShape::Valve {
            at,
            zone,
            name: String::new(),
            kv,
            controller: None,
            station: None,
        }
    }

    /// POC (0,0) -> 20 m PVC 50 mainline -> valve zone 1 at (20 m, 0) -> 30 m PE 32 lateral
    /// to (50 m, 0). Four Spray-4.0 heads (full circle, 8 l/min) at the end of the lateral:
    /// two on the end point, two on a short PE 25 branch.
    pub fn small_zone() -> Editor {
        let mut ed = Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 400.0, 100.0));
        add(
            &mut ed,
            line(
                &[v(0.0, 0.0), v(20_000.0, 0.0)],
                LineType::Mainline,
                "50",
                "PVC",
            ),
        );
        add(&mut ed, valve(v(20_000.0, 0.0), 1, 6.0));
        add(
            &mut ed,
            line(
                &[v(20_000.0, 0.0), v(50_000.0, 0.0)],
                LineType::Lateral,
                "32",
                "PE",
            ),
        );
        add(
            &mut ed,
            line(
                &[v(50_000.0, 0.0), v(50_000.0, 10_000.0)],
                LineType::Lateral,
                "25",
                "PE",
            ),
        );
        add(&mut ed, outlet(v(50_000.0, 0.0), "Spray-4.0", 360.0));
        add(&mut ed, outlet(v(50_000.0, 0.0), "Spray-4.0", 360.0));
        add(&mut ed, outlet(v(50_000.0, 10_000.0), "Spray-4.0", 360.0));
        add(&mut ed, outlet(v(50_010.0, 10_010.0), "Spray-4.0", 360.0));
        ed
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::hydraulics::{pipe_loss_kpa, valve_loss_kpa};
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::{Attributes, Shape};

    #[test]
    fn clustering_merges_close_points() {
        let pts = [
            v(0.0, 0.0),
            v(30.0, 0.0),
            v(100.0, 0.0),
            v(120.0, 0.0),
            v(1000.0, 1000.0),
        ];
        let g = cluster(&pts, 50.0);
        assert_eq!(g, [0, 0, 1, 1, 2]);
        // Chains of close points merge.
        let g = cluster(&[v(0.0, 0.0), v(40.0, 0.0), v(80.0, 0.0)], 50.0);
        assert_eq!(g, [0, 0, 0]);
        assert!(cluster(&[], 50.0).is_empty());
        // Negative coordinates and a zero tolerance still work.
        assert_eq!(cluster(&[v(-1.0, -1.0), v(-1.0, -1.0)], 0.0), [0, 0]);
    }

    /// The zone of `small_zone`, by hand (Hazen-Williams, see `hydraulics`).
    /// Four heads at 8 l/min give 32 l/min.
    /// * Mainline PVC 50 (inner 45.2 mm, C 150), 20 m, 32 l/min.
    /// * Valve kv 6, 32 l/min = 1.92 m3/h: 100 * (1.92 / 6)^2 = 10.24 kPa.
    /// * Lateral PE 32 (inner 26.2, C 140), 30 m, 32 l/min.
    /// * Branch PE 25 (inner 20.4, C 140), 10 m, 16 l/min (two heads).
    #[test]
    fn small_zone_matches_formulas() {
        let ed = small_zone();
        let a = analyze(ed.doc(), DEFAULT_TOLERANCE);
        assert!(a.warnings().is_empty(), "{:?}", a.warnings());
        assert_eq!(a.extraction.outlets.len(), 4);
        let z = a.solution.zone(1).unwrap();
        assert_eq!(z.flow_lpm, 32.0);
        let p_valve = 400.0 - pipe_loss_kpa(32.0, 45.2, 150.0, 20.0) - valve_loss_kpa(32.0, 6.0);
        assert!((valve_loss_kpa(32.0, 6.0) - 10.24).abs() < 1e-9);
        let p_end = p_valve - pipe_loss_kpa(32.0, 26.2, 140.0, 30.0);
        let p_far = p_end - pipe_loss_kpa(16.0, 20.4, 140.0, 10.0);
        let by_id =
            |id: cadektor_core::uid::Uid| z.outlets.iter().find(|o| o.object == Some(id)).unwrap();
        let ids: Vec<cadektor_core::uid::Uid> = a.extraction.outlets.iter().map(|o| o.id).collect();
        assert!((by_id(ids[0]).pressure_kpa - p_end).abs() < 1e-9);
        assert!((by_id(ids[2]).pressure_kpa - p_far).abs() < 1e-9);
        // The two far heads are critical (the first of them on a tie).
        assert_eq!(z.outlets[z.critical.unwrap()].object, Some(ids[2]));
        assert!(p_far > 140.0 && p_far < p_end);
        assert_eq!(a.outlet_zones().len(), 4);
        assert!(a.outlet_zones().values().all(|z| *z == 1));
    }

    #[test]
    fn lines_connect_through_the_tolerance() {
        let mut ed = Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 300.0, 50.0));
        add(&mut ed, valve(v(10_000.0, 0.0), 2, 5.0));
        // The ends are 40 mm off the valve: connected with the default tolerance, not with 10.
        add(
            &mut ed,
            line(
                &[v(0.0, 0.0), v(10_030.0, 30.0)],
                LineType::Mainline,
                "50",
                "PVC",
            ),
        );
        add(
            &mut ed,
            line(
                &[v(9_970.0, -30.0), v(15_000.0, 0.0)],
                LineType::Lateral,
                "32",
                "PE",
            ),
        );
        add(&mut ed, outlet(v(15_000.0, 0.0), "Rotor-8", 180.0));
        let a = analyze(ed.doc(), 50.0);
        assert!(a.solution.zone(2).is_some());
        let b = analyze(ed.doc(), 10.0);
        assert!(b.solution.zone(2).is_none());
        assert!(
            b.warnings()
                .iter()
                .any(|w| w.kind == WarningKind::Unconnected || w.kind == WarningKind::NoSource)
        );
    }

    #[test]
    fn catalog_gaps_and_arc_range_warn() {
        let mut ed = Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 300.0, 50.0));
        add(&mut ed, valve(v(0.0, 0.0), 1, 5.0));
        add(
            &mut ed,
            line(
                &[v(0.0, 0.0), v(5_000.0, 0.0)],
                LineType::Lateral,
                "99",
                "PE",
            ),
        );
        add(
            &mut ed,
            line(
                &[v(5_000.0, 0.0), v(6_000.0, 0.0)],
                LineType::Lateral,
                "32",
                "Unobtainium",
            ),
        );
        add(&mut ed, outlet(v(5_000.0, 0.0), "Ghost", 360.0));
        // A rotor at 10 degrees: below its minimum arc of 40.
        add(&mut ed, outlet(v(6_000.0, 0.0), "Rotor-8", 10.0));
        let a = analyze(ed.doc(), 50.0);
        let msgs: Vec<String> = a.warnings().iter().map(|w| w.message.clone()).collect();
        assert!(msgs.iter().any(|m| m.contains("pipe PE 99")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("Unobtainium")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("Ghost")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("10 degrees")), "{msgs:?}");
        // The unknown pipe is lossless, so the rotor sees almost the full pressure of the unknown-material pipe.
        assert!(a.solution.zone(1).is_some());
    }

    #[test]
    fn drip_line_demand_counts_at_the_last_point() {
        let mut ed = Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 200.0, 50.0));
        add(&mut ed, valve(v(0.0, 0.0), 3, 2.0));
        let mut drip = line(&[v(0.0, 0.0), v(30_000.0, 0.0)], LineType::Drip, "16", "PE");
        if let IrrigationShape::Line {
            emitter_spacing,
            emitter_flow,
            ..
        } = &mut drip
        {
            *emitter_spacing = Some(500.0);
            *emitter_flow = Some(3.0);
        }
        let id = add(&mut ed, drip);
        let a = analyze(ed.doc(), 50.0);
        // 30 m / 0.5 m = 60 emitters of 3 l/h = 180 l/h = 3 l/min.
        let z = a.solution.zone(3).unwrap();
        assert!((z.flow_lpm - 3.0).abs() < 1e-12);
        assert_eq!(z.outlets[0].object, Some(id));
        assert_eq!(z.outlets[0].min_kpa, DRIP_MIN_KPA);
        assert!((a.extraction.lines[0].emitter_flow_lpm - 3.0).abs() < 1e-12);
        // A very short drip line still has one emitter, with the default spacing and flow.
        let mut ed = Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 200.0, 50.0));
        add(
            &mut ed,
            line(&[v(0.0, 0.0), v(100.0, 0.0)], LineType::Drip, "16", "PE"),
        );
        let a = analyze(ed.doc(), 50.0);
        assert!((a.extraction.lines[0].emitter_flow_lpm - 2.0 / 60.0).abs() < 1e-12);
    }

    #[test]
    fn controller_checks() {
        let mut ed = Editor::default();
        add(
            &mut ed,
            IrrigationShape::Controller {
                at: v(0.0, 0.0),
                name: "A".into(),
                stations: 2,
            },
        );
        let mk = |zone, controller: Option<&str>, station| IrrigationShape::Valve {
            at: v(zone as f64 * 5000.0, 0.0),
            zone,
            name: String::new(),
            kv: 3.0,
            controller: controller.map(String::from),
            station,
        };
        add(&mut ed, mk(1, Some("a"), Some(1)));
        add(&mut ed, mk(2, Some("A"), Some(1))); // used twice
        add(&mut ed, mk(3, Some("A"), Some(5))); // out of range
        add(&mut ed, mk(4, Some("B"), Some(1))); // no such controller
        add(&mut ed, mk(5, Some("A"), None)); // no station: fine
        let a = analyze(ed.doc(), 50.0);
        let c: Vec<&Warning> = a
            .warnings()
            .into_iter()
            .filter(|w| w.kind == WarningKind::Controller)
            .collect();
        assert_eq!(c.len(), 3, "{c:?}");
        assert!(c.iter().any(|w| w.message.contains("also used")));
        assert!(c.iter().any(|w| w.message.contains("outside 1 to 2")));
        assert!(c.iter().any(|w| w.message.contains("does not exist")));
        assert_eq!(a.extraction.controllers.len(), 1);
    }

    #[test]
    fn named_poc_and_valve_labels() {
        let mut ed = Editor::default();
        add(
            &mut ed,
            IrrigationShape::Poc {
                at: v(0.0, 0.0),
                name: String::new(),
                static_pressure: 300.0,
                available_flow: 90.0,
                supply_loss: None,
                meter_loss: None,
            },
        );
        add(
            &mut ed,
            IrrigationShape::Valve {
                at: v(9000.0, 0.0),
                zone: 7,
                name: "North bed".into(),
                kv: 4.0,
                controller: None,
                station: None,
            },
        );
        add(
            &mut ed,
            IrrigationShape::Valve {
                at: v(0.0, 9000.0),
                zone: 8,
                name: String::new(),
                kv: 4.0,
                controller: None,
                station: None,
            },
        );
        add(
            &mut ed,
            line(
                &[v(0.0, 0.0), v(9000.0, 0.0)],
                LineType::Mainline,
                "50",
                "PVC",
            ),
        );
        add(
            &mut ed,
            line(
                &[v(0.0, 0.0), v(0.0, 9000.0)],
                LineType::Mainline,
                "50",
                "PVC",
            ),
        );
        let ex = extract(ed.doc(), &Catalog::builtin(), 50.0);
        let labels: Vec<&str> = ex.network.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with("POC #")));
        assert!(labels.contains(&"North bed"));
        assert!(labels.contains(&"Valve zone 8"));
    }

    #[test]
    fn heads_and_branches_connect_in_the_middle_of_a_segment() {
        let mut ed = Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 400.0, 100.0));
        add(&mut ed, valve(v(0.0, 0.0), 1, 6.0));
        // One two-point lateral of 20 m with 10 heads along it (20 mm off the line).
        add(
            &mut ed,
            line(
                &[v(0.0, 0.0), v(20_000.0, 0.0)],
                LineType::Lateral,
                "32",
                "PE",
            ),
        );
        for k in 1..=10 {
            add(
                &mut ed,
                outlet(v(k as f64 * 2_000.0, 20.0), "Spray-2.5", 360.0),
            );
        }
        // A branch that ends in the middle of the lateral, and one that starts there.
        add(
            &mut ed,
            line(
                &[v(7_000.0, 3_000.0), v(7_000.0, 10.0)],
                LineType::Lateral,
                "25",
                "PE",
            ),
        );
        add(&mut ed, outlet(v(7_000.0, 3_000.0), "Spray-2.5", 360.0));
        let a = analyze(ed.doc(), DEFAULT_TOLERANCE);
        assert!(a.warnings().is_empty(), "{:?}", a.warnings());
        let z = a.solution.zone(1).unwrap();
        assert_eq!(z.outlets.len(), 11);
        assert!((z.flow_lpm - 44.0).abs() < 1e-9);
        // The first part of the lateral carries all the flow, the last part only the last head.
        let last = z
            .outlets
            .iter()
            .map(|o| o.pressure_kpa)
            .fold(f64::INFINITY, f64::min);
        assert!(last < z.outlets[0].pressure_kpa);
    }

    #[test]
    fn a_valve_in_the_middle_of_a_mainline_splits_it_and_zones_follow_the_lateral_side() {
        let mut ed = Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 400.0, 100.0));
        // One 40 m mainline passes a valve at 20 m. A lateral leaves the valve.
        add(
            &mut ed,
            line(
                &[v(0.0, 0.0), v(40_000.0, 0.0)],
                LineType::Mainline,
                "50",
                "PVC",
            ),
        );
        add(&mut ed, valve(v(20_000.0, 0.0), 1, 6.0));
        add(
            &mut ed,
            line(
                &[v(20_000.0, 0.0), v(20_000.0, 10_000.0)],
                LineType::Lateral,
                "32",
                "PE",
            ),
        );
        add(&mut ed, outlet(v(20_000.0, 10_000.0), "Spray-4.0", 360.0));
        // A second valve at the end of the mainline with its own lateral.
        add(&mut ed, valve(v(40_000.0, 0.0), 2, 6.0));
        add(
            &mut ed,
            line(
                &[v(40_000.0, 0.0), v(40_000.0, 5_000.0)],
                LineType::Lateral,
                "32",
                "PE",
            ),
        );
        add(&mut ed, outlet(v(40_000.0, 5_000.0), "Spray-4.0", 360.0));
        let a = analyze(ed.doc(), DEFAULT_TOLERANCE);
        assert!(a.warnings().is_empty(), "{:?}", a.warnings());
        assert_eq!(a.solution.zones.len(), 2);
        // Zone 2 does not pay the loss of valve 1: 8 l/min through 40 m of PVC 50 and its lateral.
        let expect = 400.0
            - pipe_loss_kpa(8.0, 45.2, 150.0, 40.0)
            - valve_loss_kpa(8.0, 6.0)
            - pipe_loss_kpa(8.0, 26.2, 140.0, 5.0);
        let z2 = a.solution.zone(2).unwrap();
        assert!((z2.outlets[0].pressure_kpa - expect).abs() < 1e-9);
    }

    #[test]
    fn drip_flow_is_spread_along_the_line_whichever_way_it_is_drawn() {
        let build = |flip: bool| {
            let mut ed = Editor::default();
            add(&mut ed, poc(v(0.0, 0.0), 300.0, 50.0));
            add(&mut ed, valve(v(0.0, 0.0), 3, 6.0));
            let mut pts = [v(0.0, 0.0), v(30_000.0, 0.0)];
            if flip {
                pts.reverse();
            }
            let mut drip = line(&pts, LineType::Drip, "16", "PE");
            if let IrrigationShape::Line {
                emitter_spacing,
                emitter_flow,
                ..
            } = &mut drip
            {
                *emitter_spacing = Some(500.0);
                *emitter_flow = Some(3.0);
            }
            add(&mut ed, drip);
            // The line is fed at (0, 0) in both cases: a short feeder from the valve.
            add(
                &mut ed,
                line(&[v(0.0, 0.0), v(0.0, 5.0)], LineType::Lateral, "25", "PE"),
            );
            analyze(ed.doc(), 50.0)
        };
        let p = |a: &Analysis| a.solution.zone(3).unwrap().outlets[0].pressure_kpa;
        let (a, b) = (build(false), build(true));
        assert!((p(&a) - p(&b)).abs() < 1e-9, "{} {}", p(&a), p(&b));
        // 60 emitters of 3 l/h = 3 l/min; the loss is the Christiansen factor of 60 emitters
        // times the loss of the whole flow in the whole pipe, with kv 6 in the valve.
        let f = crate::network::christiansen(60);
        let expect = 300.0 - valve_loss_kpa(3.0, 6.0) - f * pipe_loss_kpa(3.0, 12.4, 140.0, 30.0);
        assert!((p(&a) - expect).abs() < 1e-6, "{} {}", p(&a), expect);
        assert!((a.solution.zone(3).unwrap().flow_lpm - 3.0).abs() < 1e-9);
    }

    #[test]
    fn fittings_and_ground_change_the_pressure() {
        use cadektor_core::model::Object;
        struct Slope;
        impl GroundSource for Slope {
            fn exact(
                &self,
                _: Option<&Object>,
                p: DVec2,
                _: cadektor_core::ground::Surface,
            ) -> Option<f64> {
                // 1 m rise per 10 m along X, in mm.
                Some(p.x / 10.0)
            }
        }
        let ed = small_zone();
        let base = analyze(ed.doc(), 50.0);
        let pf = |a: &Analysis| a.solution.zone(1).unwrap().outlets[0].pressure_kpa;
        let fit = analyze_with(
            ed.doc(),
            &ExtractOptions {
                fittings_pct: 10.0,
                ..ExtractOptions::default()
            },
        );
        let loss = |a: &Analysis| 400.0 - pf(a);
        assert!((loss(&fit) - 1.1 * loss(&base) + 0.1 * valve_loss_kpa(32.0, 6.0)).abs() < 1e-6);
        // The first head is 50 m away along X: 5 m higher than the POC.
        let g = Slope;
        let hill = analyze_with(
            ed.doc(),
            &ExtractOptions {
                ground: Some(&g),
                ..ExtractOptions::default()
            },
        );
        assert!((pf(&base) - pf(&hill) - 5.0 * 9.80665).abs() < 1e-6);
    }

    #[test]
    fn many_heads_attach_quickly() {
        let mut ed = Editor::default();
        add(&mut ed, poc(v(0.0, 0.0), 400.0, 1e6));
        add(&mut ed, valve(v(0.0, 0.0), 1, 600.0));
        add(
            &mut ed,
            line(
                &[v(0.0, 0.0), v(2_000_000.0, 0.0)],
                LineType::Lateral,
                "63",
                "PE",
            ),
        );
        let cmds: Vec<Command> = (0..3000)
            .map(|k| Command::AddObject {
                shape: Shape::Irrigation(Box::new(outlet(
                    v(k as f64 * 600.0, 20.0),
                    "Spray-2.5",
                    360.0,
                ))),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .collect();
        ed.execute(&Command::Batch { commands: cmds }).unwrap();
        let t = std::time::Instant::now();
        let a = analyze(ed.doc(), 50.0);
        assert!(t.elapsed().as_secs_f64() < 5.0);
        assert_eq!(a.solution.zone(1).unwrap().outlets.len(), 3000);
        assert!(
            !a.warnings()
                .iter()
                .any(|w| w.kind == WarningKind::Unconnected)
        );
    }
}
