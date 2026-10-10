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
use cadektor_core::irrigation::{IrrigationShape, LineType};
use cadektor_core::model::{Document, Object};

use crate::catalog::{Catalog, HeadKind};
use crate::network::{
    Demand, Edge, Network, Node, NodeId, Poc, ValveSpec, Warning, WarningKind, solve,
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
    pub id: u64,
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
    pub id: u64,
    pub line_type: LineType,
    pub size: String,
    pub material: String,
    pub length_m: f64,
    pub inner_diameter_mm: Option<f64>,
    pub price_per_m: f64,
    /// Total flow of the emitters in l/min (drip lines only).
    pub emitter_flow_lpm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValveInfo {
    pub id: u64,
    pub zone: u32,
    pub name: String,
    pub kv: f64,
    pub controller: Option<String>,
    pub station: Option<u32>,
    pub node: NodeId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PocInfo {
    pub id: u64,
    pub name: String,
    pub static_kpa: f64,
    pub available_lpm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControllerInfo {
    pub id: u64,
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

fn warn(out: &mut Vec<Warning>, kind: WarningKind, object: u64, message: String) {
    out.push(Warning {
        kind,
        object: Some(object),
        zone: None,
        message,
    });
}

/// Reads the irrigation objects of the document into a network.
pub fn extract(doc: &Document, catalog: &Catalog, tol: f64) -> Extraction {
    let mut ex = Extraction::default();
    let objs: Vec<(&Object, &IrrigationShape)> = doc
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            cadektor_core::model::Shape::Irrigation(i) => Some((o, i.as_ref())),
            _ => None,
        })
        .collect();

    // Points that become nodes, and who owns each point.
    enum Owner {
        LineVertex,
        Poc(usize),
        Valve(usize),
    }
    let mut points: Vec<DVec2> = vec![];
    let mut owners: Vec<Owner> = vec![];
    // Index in `points` of the first vertex of each line, in object order.
    let mut line_starts: Vec<usize> = vec![];
    for (o, i) in &objs {
        match i {
            IrrigationShape::Line { points: pts, .. } => {
                line_starts.push(points.len());
                for p in pts {
                    points.push(*p);
                    owners.push(Owner::LineVertex);
                }
            }
            IrrigationShape::Poc {
                at,
                name,
                static_pressure,
                available_flow,
            } => {
                ex.pocs.push(PocInfo {
                    id: o.id.0,
                    name: name.clone(),
                    static_kpa: *static_pressure,
                    available_lpm: *available_flow,
                });
                points.push(*at);
                owners.push(Owner::Poc(ex.pocs.len() - 1));
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
    for (k, owner) in owners.iter().enumerate() {
        let node = &mut ex.network.nodes[groups[k]];
        match owner {
            Owner::LineVertex => {}
            Owner::Poc(i) => {
                let p = &ex.pocs[*i];
                if node.poc.is_none() {
                    node.poc = Some(Poc {
                        static_kpa: p.static_kpa,
                        available_lpm: p.available_lpm,
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

    // Pipes and drip emitters.
    let mut line_no = 0usize;
    let mut drip_demands: Vec<(NodeId, u64, f64)> = vec![];
    for (o, i) in &objs {
        let IrrigationShape::Line {
            points: pts,
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
        let length_mm: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
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
        });
        for k in 0..pts.len().saturating_sub(1) {
            let (a, b) = (groups[base + k], groups[base + k + 1]);
            if a != b {
                ex.network.add_edge(Edge {
                    a,
                    b,
                    length_m: pts[k].distance(pts[k + 1]) / 1000.0,
                    inner_diameter_mm: pipe.map_or(LINK_DIAMETER, |p| p.inner_diameter),
                    c: c.unwrap_or(DEFAULT_C),
                    object: Some(id),
                });
            }
        }
        if emitter_lpm > 0.0 && pts.len() >= 2 {
            drip_demands.push((groups[base + pts.len() - 1], id, emitter_lpm));
        }
    }

    // Outlets attach to the nearest node within the tolerance.
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
        let node = ex.network.add_node(Node {
            object: Some(id),
            label: format!("Outlet #{id} ({model})"),
            demand: (flow > 0.0).then_some(Demand {
                flow_lpm: flow,
                min_kpa,
            }),
            ..Node::default()
        });
        // The nearest point within the tolerance.
        let near = (0..points.len())
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

    for (at_node, id, flow) in drip_demands {
        let node = ex.network.add_node(Node {
            object: Some(id),
            label: format!("Drip line #{id}"),
            demand: Some(Demand {
                flow_lpm: flow,
                min_kpa: DRIP_MIN_KPA,
            }),
            ..Node::default()
        });
        ex.network.add_edge(Edge {
            a: at_node,
            b: node,
            length_m: 0.0,
            inner_diameter_mm: LINK_DIAMETER,
            c: DEFAULT_C,
            object: None,
        });
    }

    check_controllers(&mut ex);
    ex
}

/// Valves that name a controller need an existing controller and a free station in its range.
fn check_controllers(ex: &mut Extraction) {
    let mut used: Vec<(String, u32, u64)> = vec![];
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
    pub fn outlet_zones(&self) -> HashMap<u64, u32> {
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
    let catalog = Catalog::of_document(doc);
    let extraction = extract(doc, &catalog, tol);
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

    pub fn add(ed: &mut Editor, s: IrrigationShape) -> u64 {
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
    use cadektor_core::command::Editor;

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
        let by_id = |id: u64| z.outlets.iter().find(|o| o.object == Some(id)).unwrap();
        let ids: Vec<u64> = a.extraction.outlets.iter().map(|o| o.id).collect();
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
                &[v(0.0, 0.0), v(10_030.0, 0.0)],
                LineType::Mainline,
                "50",
                "PVC",
            ),
        );
        add(
            &mut ed,
            line(
                &[v(9_970.0, 0.0), v(15_000.0, 0.0)],
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
}
