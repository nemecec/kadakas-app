//! The hydraulic network and its solver.
//!
//! The network is a graph of nodes joined by pipes. It must be a tree from a point of connection
//! (POC) to the outlets: POC, mainline, valves, laterals, outlets. A loop is reported and one of
//! its pipes is ignored.
//!
//! # Model
//!
//! * Zones run one at a time. For each zone only the outlets downstream of its valve use water.
//!   The zone of an outlet is the zone of the nearest valve between it and the POC (zone 0 when
//!   there is none).
//! * Outlets use their rated flow (fixed-flow model: the flow does not change with pressure).
//! * The flow in a pipe is the sum of the demand downstream of it.
//! * The pressure falls along each path by the pipe losses (Hazen-Williams, see `hydraulics`) and
//!   by the loss of each valve that carries flow. Height differences are not modeled.
//! * An outlet is low when its pressure is below the minimum of its model.
//! * The critical outlet of a zone has the smallest margin (pressure minus minimum). The
//!   critical path runs from the POC to it.

use crate::hydraulics::{MAX_VELOCITY_MS, pipe_loss_kpa, valve_loss_kpa, velocity_ms};

pub type NodeId = usize;

/// The water supply.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Poc {
    /// Pressure with no flow, in kPa.
    pub static_kpa: f64,
    /// The most flow the supply gives, in l/min.
    pub available_lpm: f64,
    /// Loss in the supply line at the available flow, in kPa. The loss at another flow `Q`
    /// follows the Hazen-Williams exponent: `supply_loss * (Q / available)^1.852`. The pressure
    /// at the POC while water runs (the dynamic pressure) is the static pressure minus this loss.
    pub supply_loss_kpa: f64,
    /// Loss in the meter and backflow preventer at the available flow, in kPa. It grows with the
    /// square of the flow.
    pub meter_loss_kpa: f64,
}

impl Poc {
    /// Pressure at the POC while `flow_lpm` runs: static pressure minus supply and meter losses.
    pub fn dynamic_kpa(&self, flow_lpm: f64) -> f64 {
        if self.available_lpm <= 0.0 || flow_lpm <= 0.0 {
            return self.static_kpa;
        }
        let r = flow_lpm / self.available_lpm;
        self.static_kpa - self.supply_loss_kpa * r.powf(1.852) - self.meter_loss_kpa * r * r
    }
}

/// A control valve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValveSpec {
    pub zone: u32,
    /// Flow factor in m3/h at 1 bar.
    pub kv: f64,
}

/// Water use at a node: an outlet, or the emitters of a drip line.
#[derive(Clone, Debug, PartialEq)]
pub struct Demand {
    pub flow_lpm: f64,
    /// The lowest allowed pressure in kPa.
    pub min_kpa: f64,
}

/// A point of the network.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    /// The ID of the drawing object, if the node comes from one.
    pub object: Option<u64>,
    pub label: String,
    pub poc: Option<Poc>,
    pub valve: Option<ValveSpec>,
    pub demand: Option<Demand>,
    /// Ground height in m, when the document has a site model there.
    pub z_m: Option<f64>,
}

/// Emitters spread evenly along a drip line (the edge). The flow is taken from the line as it
/// goes, whichever end is connected. The pressure loss uses the Christiansen factor.
#[derive(Clone, Debug, PartialEq)]
pub struct DripInfo {
    /// Total flow of the emitters in l/min.
    pub flow_lpm: f64,
    /// Number of emitters.
    pub emitters: usize,
    /// The lowest pressure at the last emitter, in kPa.
    pub min_kpa: f64,
}

/// A pipe between two nodes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edge {
    pub a: NodeId,
    pub b: NodeId,
    pub length_m: f64,
    pub inner_diameter_mm: f64,
    /// Hazen-Williams coefficient.
    pub c: f64,
    pub object: Option<u64>,
    /// A mainline passes a valve: it is not on the lateral side of the valve.
    pub through: bool,
    pub drip: Option<DripInfo>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Network {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Allowance for fittings as a percentage of the pipe losses (for example 10).
    pub fittings_pct: f64,
}

impl Network {
    pub fn add_node(&mut self, node: Node) -> NodeId {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    pub fn add_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }
}

/// What a warning is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WarningKind {
    /// The pressure at an outlet is below its minimum.
    LowPressure,
    /// The velocity in a pipe is above the limit.
    HighVelocity,
    /// The flow of a zone is more than the supply gives.
    FlowExceedsSupply,
    /// An outlet has no water path from a POC.
    Unconnected,
    /// The pipes form a loop.
    Loop,
    /// The network has no POC.
    NoSource,
    /// An outlet has no valve upstream (it runs without a zone).
    NoValve,
    /// A catalog entry is missing (head model, pipe size, material).
    Catalog,
    /// An outlet spray angle is outside the range of its model.
    ArcRange,
    /// A valve refers to a controller or station that does not exist or is used twice.
    Controller,
}

impl WarningKind {
    pub fn id(self) -> &'static str {
        match self {
            WarningKind::LowPressure => "low_pressure",
            WarningKind::HighVelocity => "high_velocity",
            WarningKind::FlowExceedsSupply => "flow_exceeds_supply",
            WarningKind::Unconnected => "unconnected",
            WarningKind::Loop => "loop",
            WarningKind::NoSource => "no_source",
            WarningKind::NoValve => "no_valve",
            WarningKind::Catalog => "catalog",
            WarningKind::ArcRange => "arc_range",
            WarningKind::Controller => "controller",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Warning {
    pub kind: WarningKind,
    /// The drawing object, when the warning is about one.
    pub object: Option<u64>,
    pub zone: Option<u32>,
    pub message: String,
}

/// The flow and loss in one pipe while a zone runs.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeResult {
    pub edge: usize,
    pub object: Option<u64>,
    pub length_m: f64,
    pub flow_lpm: f64,
    pub velocity_ms: f64,
    pub loss_kpa: f64,
}

/// The state of one outlet while its zone runs.
#[derive(Clone, Debug, PartialEq)]
pub struct OutletResult {
    pub node: NodeId,
    pub object: Option<u64>,
    pub label: String,
    pub flow_lpm: f64,
    pub pressure_kpa: f64,
    pub min_kpa: f64,
    /// Pressure minus minimum. Negative means low.
    pub margin_kpa: f64,
}

/// One step of the critical path.
#[derive(Clone, Debug, PartialEq)]
pub struct PathStep {
    pub node: NodeId,
    pub object: Option<u64>,
    pub label: String,
    /// Pressure at the node (after a valve there), in kPa.
    pub pressure_kpa: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ZoneResult {
    pub zone: u32,
    /// Pressure at the POC while the zone runs, in kPa.
    pub dynamic_kpa: f64,
    /// Total flow of the zone in l/min.
    pub flow_lpm: f64,
    /// Flow that the supply gives, in l/min (the smallest one of the POCs that feed the zone).
    pub available_lpm: f64,
    pub static_kpa: f64,
    pub outlets: Vec<OutletResult>,
    /// Pipes with flow, in order from the POC.
    pub edges: Vec<EdgeResult>,
    /// Index into `outlets` of the outlet with the smallest margin.
    pub critical: Option<usize>,
    pub critical_path: Vec<PathStep>,
    pub max_velocity_ms: f64,
    pub warnings: Vec<Warning>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solution {
    /// Zones in ascending order of the zone number.
    pub zones: Vec<ZoneResult>,
    /// Warnings of the whole network (loops, outlets without a connection, no POC).
    pub warnings: Vec<Warning>,
}

impl Solution {
    /// All warnings: those of the network, then those of each zone.
    pub fn all_warnings(&self) -> Vec<&Warning> {
        self.warnings
            .iter()
            .chain(self.zones.iter().flat_map(|z| z.warnings.iter()))
            .collect()
    }

    pub fn zone(&self, zone: u32) -> Option<&ZoneResult> {
        self.zones.iter().find(|z| z.zone == zone)
    }
}

/// Pressure per metre of height, in kPa.
const KPA_PER_M: f64 = crate::hydraulics::KPA_PER_M_HEAD;

/// Christiansen factor for a line with `n` equally spaced outlets and the Hazen-Williams exponent
/// 1.852: the loss of the line is this factor times the loss of the whole flow in the whole pipe.
/// `F = 1 / 2.852 + 1 / (2 n) + sqrt(0.852) / (6 n^2)`.
pub fn christiansen(n: usize) -> f64 {
    let n = n.max(1) as f64;
    1.0 / 2.852 + 1.0 / (2.0 * n) + 0.852f64.sqrt() / (6.0 * n * n)
}

/// Loss of a pipe in kPa for the flow at its inlet, with the fittings allowance.
fn edge_loss(net: &Network, e: &Edge, flow: f64) -> f64 {
    let fit = 1.0 + net.fittings_pct.max(0.0) / 100.0;
    let loss = |q: f64| pipe_loss_kpa(q, e.inner_diameter_mm, e.c, e.length_m);
    let base = match &e.drip {
        Some(d) => {
            let qd = d.flow_lpm.min(flow);
            christiansen(d.emitters) * loss(qd) + (loss(flow) - loss(qd))
        }
        None => loss(flow),
    };
    base * fit
}

/// The flow that the node and the drip line that ends at it use in a zone.
fn own_flow(net: &Network, parent: &[Option<(NodeId, usize)>], i: NodeId) -> f64 {
    let d = net.nodes[i]
        .demand
        .as_ref()
        .map_or(0.0, |d| d.flow_lpm.max(0.0));
    let drip = parent[i]
        .and_then(|(_, ei)| net.edges[ei].drip.as_ref())
        .map_or(0.0, |d| d.flow_lpm.max(0.0));
    d + drip
}

/// Solves the network for every zone, one zone at a time.
pub fn solve(net: &Network) -> Solution {
    let n = net.nodes.len();
    let mut sol = Solution::default();
    let sources: Vec<NodeId> = (0..n).filter(|&i| net.nodes[i].poc.is_some()).collect();
    if sources.is_empty() {
        sol.warnings.push(Warning {
            kind: WarningKind::NoSource,
            object: None,
            zone: None,
            message: "the network has no point of connection".into(),
        });
        return sol;
    }

    // Adjacency, then a breadth-first search from all POCs into a forest.
    let mut adj: Vec<Vec<(NodeId, usize)>> = vec![vec![]; n];
    for (i, e) in net.edges.iter().enumerate() {
        adj[e.a].push((e.b, i));
        adj[e.b].push((e.a, i));
    }
    let mut parent: Vec<Option<(NodeId, usize)>> = vec![None; n];
    let mut root: Vec<Option<NodeId>> = vec![None; n];
    let mut order: Vec<NodeId> = Vec::with_capacity(n);
    let mut edge_used = vec![false; net.edges.len()];
    for &s in &sources {
        if root[s].is_none() {
            root[s] = Some(s);
            order.push(s);
        }
    }
    let mut head = 0;
    while head < order.len() {
        let u = order[head];
        head += 1;
        for &(v, ei) in &adj[u] {
            if edge_used[ei] {
                continue;
            }
            edge_used[ei] = true;
            if root[v].is_none() {
                root[v] = root[u];
                parent[v] = Some((u, ei));
                order.push(v);
            } else {
                let e = &net.edges[ei];
                sol.warnings.push(Warning {
                    kind: WarningKind::Loop,
                    object: e.object,
                    zone: None,
                    message: "the pipes form a loop; this pipe is ignored in the calculation"
                        .into(),
                });
            }
        }
    }

    // The zone of each reachable node with water use: the nearest valve on the way to the POC,
    // entered from its lateral side (a mainline that passes a valve does not belong to it).
    let zone_of = |start: NodeId| -> u32 {
        let mut cur = start;
        while let Some((p, ei)) = parent[cur] {
            if let Some(v) = net.nodes[p].valve
                && !net.edges[ei].through
            {
                return v.zone;
            }
            cur = p;
        }
        0
    };
    let mut zones: Vec<u32> = Vec::new();
    let mut node_zone: Vec<u32> = vec![0; n];
    for i in 0..n {
        if own_flow(net, &parent, i) <= 0.0 {
            continue;
        }
        if root[i].is_none() {
            sol.warnings.push(Warning {
                kind: WarningKind::Unconnected,
                object: net.nodes[i].object,
                zone: None,
                message: format!(
                    "{} has no water path from a point of connection",
                    label(net, i)
                ),
            });
            continue;
        }
        node_zone[i] = zone_of(i);
        if node_zone[i] == 0 {
            sol.warnings.push(Warning {
                kind: WarningKind::NoValve,
                object: net.nodes[i].object,
                zone: Some(0),
                message: format!(
                    "{} has no valve upstream, so it belongs to no zone",
                    label(net, i)
                ),
            });
        }
        if !zones.contains(&node_zone[i]) {
            zones.push(node_zone[i]);
        }
    }
    // A drip line with no water path.
    for e in &net.edges {
        if e.drip.is_some() && root[e.a].is_none() && root[e.b].is_none() {
            sol.warnings.push(Warning {
                kind: WarningKind::Unconnected,
                object: e.object,
                zone: None,
                message: "a drip line has no water path from a point of connection".into(),
            });
        }
    }
    zones.sort_unstable();

    for z in zones {
        sol.zones
            .push(solve_zone(net, z, &parent, &root, &order, &node_zone));
    }
    sol
}

fn label(net: &Network, i: NodeId) -> String {
    let n = &net.nodes[i];
    if n.label.is_empty() {
        format!("node {i}")
    } else {
        n.label.clone()
    }
}

fn solve_zone(
    net: &Network,
    zone: u32,
    parent: &[Option<(NodeId, usize)>],
    root: &[Option<NodeId>],
    order: &[NodeId],
    node_zone: &[u32],
) -> ZoneResult {
    let n = net.nodes.len();
    let active = |i: NodeId| node_zone[i] == zone && root[i].is_some();
    // Flow below each node (the use of the zone downstream, itself included), and the flow that
    // leaves each valve on its lateral side.
    let mut below = vec![0.0f64; n];
    let mut valve_flow = vec![0.0f64; n];
    for &i in order.iter().rev() {
        if active(i) {
            below[i] += own_flow(net, parent, i);
        }
        if let Some((p, ei)) = parent[i] {
            let b = below[i];
            below[p] += b;
            if net.nodes[p].valve.is_some() && !net.edges[ei].through {
                valve_flow[p] += b;
            }
        }
    }
    // Pressures from the POCs down: pipe loss, valve loss on the lateral side, height.
    let mut pressure = vec![f64::NAN; n];
    for &i in order {
        pressure[i] = match parent[i] {
            None => net.nodes[i]
                .poc
                .map_or(f64::NAN, |p| p.dynamic_kpa(below[i])),
            Some((p, ei)) => {
                let e = &net.edges[ei];
                let vloss = match net.nodes[p].valve {
                    Some(v) if !e.through && below[i] > 0.0 => valve_loss_kpa(valve_flow[p], v.kv),
                    _ => 0.0,
                };
                let dz = match (net.nodes[i].z_m, net.nodes[p].z_m) {
                    (Some(a), Some(b)) => (a - b) * KPA_PER_M,
                    _ => 0.0,
                };
                pressure[p] - edge_loss(net, e, below[i]) - vloss - dz
            }
        };
    }

    let mut res = ZoneResult {
        zone,
        dynamic_kpa: f64::INFINITY,
        flow_lpm: 0.0,
        available_lpm: f64::INFINITY,
        static_kpa: f64::INFINITY,
        outlets: vec![],
        edges: vec![],
        critical: None,
        critical_path: vec![],
        max_velocity_ms: 0.0,
        warnings: vec![],
    };
    let add_outlet = |res: &mut ZoneResult,
                      i: NodeId,
                      object: Option<u64>,
                      label: String,
                      flow: f64,
                      min: f64| {
        res.flow_lpm += flow;
        let margin = pressure[i] - min;
        if margin < -1e-9 {
            res.warnings.push(Warning {
                kind: WarningKind::LowPressure,
                object,
                zone: Some(zone),
                message: format!(
                    "{label}: the pressure {:.1} kPa is below the minimum {:.1} kPa",
                    pressure[i], min
                ),
            });
        }
        res.outlets.push(OutletResult {
            node: i,
            object,
            label,
            flow_lpm: flow,
            pressure_kpa: pressure[i],
            min_kpa: min,
            margin_kpa: margin,
        });
    };
    for &i in order {
        let node = &net.nodes[i];
        if let Some(d) = &node.demand
            && active(i)
            && d.flow_lpm > 0.0
        {
            add_outlet(
                &mut res,
                i,
                node.object,
                label(net, i),
                d.flow_lpm,
                d.min_kpa,
            );
        }
        if let Some((_, ei)) = parent[i]
            && let Some(d) = &net.edges[ei].drip
            && active(i)
            && d.flow_lpm > 0.0
        {
            let e = &net.edges[ei];
            let name = e
                .object
                .map_or("Drip line".to_string(), |o| format!("Drip line #{o}"));
            add_outlet(&mut res, i, e.object, name, d.flow_lpm, d.min_kpa);
        }
        // The supplies of the trees that carry flow of this zone.
        if let Some(p) = node.poc
            && below[i] > 0.0
        {
            res.available_lpm = res.available_lpm.min(p.available_lpm);
            res.static_kpa = res.static_kpa.min(p.static_kpa);
            res.dynamic_kpa = res.dynamic_kpa.min(pressure[i]);
        }
        if let Some((_, ei)) = parent[i]
            && below[i] > 0.0
        {
            let e = &net.edges[ei];
            let v = velocity_ms(below[i], e.inner_diameter_mm);
            let loss = edge_loss(net, e, below[i]);
            res.max_velocity_ms = res.max_velocity_ms.max(v);
            if e.length_m > 0.0 && v > MAX_VELOCITY_MS {
                res.warnings.push(Warning {
                    kind: WarningKind::HighVelocity,
                    object: e.object,
                    zone: Some(zone),
                    message: format!(
                        "velocity {:.2} m/s in a pipe of {:.1} mm is above {:.1} m/s",
                        v, e.inner_diameter_mm, MAX_VELOCITY_MS
                    ),
                });
            }
            res.edges.push(EdgeResult {
                edge: ei,
                object: e.object,
                length_m: e.length_m,
                flow_lpm: below[i],
                velocity_ms: v,
                loss_kpa: loss,
            });
        }
    }
    if !res.available_lpm.is_finite() {
        res.available_lpm = 0.0;
        res.static_kpa = 0.0;
        res.dynamic_kpa = 0.0;
    }
    if res.flow_lpm > res.available_lpm + 1e-9 {
        res.warnings.push(Warning {
            kind: WarningKind::FlowExceedsSupply,
            object: None,
            zone: Some(zone),
            message: format!(
                "zone {zone} needs {:.1} l/min and the supply gives {:.1} l/min",
                res.flow_lpm, res.available_lpm
            ),
        });
    }
    // Critical outlet: the smallest margin (the first one on ties).
    let mut crit: Option<usize> = None;
    for (k, o) in res.outlets.iter().enumerate() {
        if crit.is_none_or(|c| o.margin_kpa < res.outlets[c].margin_kpa) {
            crit = Some(k);
        }
    }
    res.critical = crit;
    if let Some(k) = crit {
        let mut path = vec![];
        let mut cur = Some(res.outlets[k].node);
        while let Some(c) = cur {
            path.push(PathStep {
                node: c,
                object: net.nodes[c].object,
                label: label(net, c),
                pressure_kpa: pressure[c],
            });
            cur = parent[c].map(|p| p.0);
        }
        path.reverse();
        res.critical_path = path;
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydraulics::{pipe_loss_kpa, valve_loss_kpa};

    fn node(label: &str) -> Node {
        Node {
            label: label.into(),
            ..Node::default()
        }
    }

    fn edge(a: NodeId, b: NodeId, length_m: f64, d: f64) -> Edge {
        Edge {
            a,
            b,
            length_m,
            inner_diameter_mm: d,
            c: 150.0,
            object: Some(100 + a as u64 * 10 + b as u64),
            ..Edge::default()
        }
    }

    fn outlet(label: &str, flow: f64, min: f64, object: u64) -> Node {
        Node {
            object: Some(object),
            label: label.into(),
            demand: Some(Demand {
                flow_lpm: flow,
                min_kpa: min,
            }),
            ..Node::default()
        }
    }

    /// The example network:
    ///
    /// ```text
    /// POC(400 kPa, 100 l/min) -100 m, 50 mm- V(zone 1, kv 12) -100 m, 25.4 mm- J
    ///    J -50 m, 25.4 mm- A (15 l/min)     J -20 m, 25.4 mm- B (15 l/min)
    /// ```
    ///
    /// All pipes C = 150. Hand calculation (Hazen-Williams, see `hydraulics`):
    /// * 30 l/min in 50 mm over 100 m: 1.6296 kPa.
    /// * Valve at 30 l/min = 1.8 m3/h: 100 * (1.8 / 12)^2 = 2.25 kPa.
    /// * 30 l/min in 25.4 mm over 100 m: 44.1079 kPa. So J has 400 - 1.6296 - 2.25 - 44.1079 = 352.0125 kPa.
    /// * 15 l/min in 25.4 mm: 12.2182 kPa per 100 m, so A (50 m) loses 6.1091 and B (20 m) loses 2.4436.
    /// * A has 345.9034 kPa, B has 349.5689 kPa. A is the critical outlet.
    fn example() -> Network {
        let mut net = Network::default();
        let poc = net.add_node(Node {
            poc: Some(Poc {
                static_kpa: 400.0,
                available_lpm: 100.0,
                ..Poc::default()
            }),
            ..node("POC")
        });
        let v = net.add_node(Node {
            valve: Some(ValveSpec { zone: 1, kv: 12.0 }),
            ..node("V")
        });
        let j = net.add_node(node("J"));
        let a = net.add_node(outlet("A", 15.0, 140.0, 1));
        let b = net.add_node(outlet("B", 15.0, 140.0, 2));
        net.add_edge(edge(poc, v, 100.0, 50.0));
        net.add_edge(edge(v, j, 100.0, 25.4));
        net.add_edge(edge(j, a, 50.0, 25.4));
        net.add_edge(edge(j, b, 20.0, 25.4));
        net
    }

    #[test]
    fn hand_calculated_example() {
        let sol = solve(&example());
        assert!(sol.warnings.is_empty());
        let z = sol.zone(1).unwrap();
        assert_eq!(z.flow_lpm, 30.0);
        assert_eq!((z.available_lpm, z.static_kpa), (100.0, 400.0));
        assert_eq!(z.outlets.len(), 2);
        let a = &z.outlets[0];
        let b = &z.outlets[1];
        assert_eq!((a.label.as_str(), b.label.as_str()), ("A", "B"));
        assert!(
            (a.pressure_kpa - 345.9034).abs() < 1e-3,
            "{}",
            a.pressure_kpa
        );
        assert!(
            (b.pressure_kpa - 349.5689).abs() < 1e-3,
            "{}",
            b.pressure_kpa
        );
        assert!((a.margin_kpa - (345.9034 - 140.0)).abs() < 1e-3);
        assert_eq!(z.critical, Some(0));
        let labels: Vec<&str> = z.critical_path.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["POC", "V", "J", "A"]);
        assert_eq!(z.critical_path[0].pressure_kpa, 400.0);
        // The valve loss is charged on its lateral side, so the valve node shows 400 - 1.6296.
        assert!((z.critical_path[1].pressure_kpa - 398.3704).abs() < 1e-3);
        assert!((z.critical_path[2].pressure_kpa - (398.3704 - 2.25 - 44.1079)).abs() < 1e-3);
        assert!(z.warnings.is_empty());
        assert_eq!(z.edges.len(), 4);
        let sum_loss: f64 = z.edges[..].iter().map(|e| e.loss_kpa).sum();
        // Edges: POC-V 1.6296, V-J 44.1079, J-A 6.1091, J-B 2.4436.
        assert!((sum_loss - (1.6296 + 44.1079 + 6.1091 + 2.4436)).abs() < 1e-3);
        // Velocity: 30 l/min in 25.4 mm is 0.9868 m/s.
        assert!(
            (z.max_velocity_ms - 0.9868).abs() < 1e-3,
            "{}",
            z.max_velocity_ms
        );
        // The solver agrees with the formulas.
        let by_formula = 400.0
            - pipe_loss_kpa(30.0, 50.0, 150.0, 100.0)
            - valve_loss_kpa(30.0, 12.0)
            - pipe_loss_kpa(30.0, 25.4, 150.0, 100.0)
            - pipe_loss_kpa(15.0, 25.4, 150.0, 50.0);
        assert!((a.pressure_kpa - by_formula).abs() < 1e-9);
    }

    #[test]
    fn low_pressure_warns() {
        let mut net = example();
        // A needs 346 kPa and has 345.9034.
        net.nodes[3].demand.as_mut().unwrap().min_kpa = 346.0;
        let sol = solve(&net);
        let z = sol.zone(1).unwrap();
        assert_eq!(z.warnings.len(), 1);
        assert_eq!(z.warnings[0].kind, WarningKind::LowPressure);
        assert_eq!(z.warnings[0].object, Some(1));
        assert!(z.outlets[0].margin_kpa < 0.0);
        assert_eq!(sol.all_warnings().len(), 1);
    }

    #[test]
    fn velocity_and_supply_warnings() {
        let mut net = example();
        // 30 l/min in a 12 mm pipe: 4.4 m/s.
        net.edges[1].inner_diameter_mm = 12.0;
        net.nodes[0].poc.as_mut().unwrap().available_lpm = 20.0;
        let sol = solve(&net);
        let kinds: Vec<WarningKind> = sol
            .zone(1)
            .unwrap()
            .warnings
            .iter()
            .map(|w| w.kind)
            .collect();
        assert!(kinds.contains(&WarningKind::HighVelocity));
        assert!(kinds.contains(&WarningKind::FlowExceedsSupply));
        assert!(kinds.contains(&WarningKind::LowPressure));
        assert_eq!(WarningKind::HighVelocity.id(), "high_velocity");
    }

    #[test]
    fn no_poc() {
        let mut net = example();
        net.nodes[0].poc = None;
        let sol = solve(&net);
        assert!(sol.zones.is_empty());
        assert_eq!(sol.warnings[0].kind, WarningKind::NoSource);
        assert_eq!(WarningKind::NoSource.id(), "no_source");
    }

    #[test]
    fn unconnected_outlet_and_no_valve() {
        let mut net = example();
        let lonely = net.add_node(outlet("lonely", 5.0, 100.0, 9));
        let _ = lonely;
        // An outlet right on the mainline before the valve belongs to zone 0.
        let c = net.add_node(outlet("C", 5.0, 100.0, 8));
        net.add_edge(edge(0, c, 10.0, 50.0));
        let sol = solve(&net);
        let kinds: Vec<WarningKind> = sol.warnings.iter().map(|w| w.kind).collect();
        assert!(kinds.contains(&WarningKind::Unconnected));
        assert!(kinds.contains(&WarningKind::NoValve));
        assert_eq!(sol.zones.iter().map(|z| z.zone).collect::<Vec<_>>(), [0, 1]);
        // Zone 0 has only C. Its flow does not use the valve.
        let z0 = sol.zone(0).unwrap();
        assert_eq!(z0.flow_lpm, 5.0);
        assert_eq!(z0.outlets[0].label, "C");
        assert_eq!(WarningKind::Unconnected.id(), "unconnected");
        assert_eq!(WarningKind::NoValve.id(), "no_valve");
    }

    #[test]
    fn loop_is_reported() {
        let mut net = example();
        // A pipe from A back to V closes a loop.
        net.add_edge(edge(3, 1, 10.0, 25.4));
        let sol = solve(&net);
        assert!(sol.warnings.iter().any(|w| w.kind == WarningKind::Loop));
        assert!(sol.zone(1).is_some());
        assert_eq!(WarningKind::Loop.id(), "loop");
    }

    #[test]
    fn two_zones_run_separately() {
        let mut net = example();
        // A second valve and outlet D on the mainline side.
        let v2 = net.add_node(Node {
            valve: Some(ValveSpec { zone: 2, kv: 12.0 }),
            ..node("V2")
        });
        let d = net.add_node(outlet("D", 20.0, 140.0, 7));
        net.add_edge(edge(0, v2, 100.0, 50.0));
        net.add_edge(edge(v2, d, 10.0, 25.4));
        let sol = solve(&net);
        assert_eq!(sol.zones.len(), 2);
        // Zone 2 alone: 20 l/min in the 50 mm mainline. Zone 1 is unchanged.
        let z1 = sol.zone(1).unwrap();
        assert!((z1.outlets[0].pressure_kpa - 345.9034).abs() < 1e-3);
        let z2 = sol.zone(2).unwrap();
        assert_eq!(z2.flow_lpm, 20.0);
        let expect = 400.0
            - pipe_loss_kpa(20.0, 50.0, 150.0, 100.0)
            - valve_loss_kpa(20.0, 12.0)
            - pipe_loss_kpa(20.0, 25.4, 150.0, 10.0);
        assert!((z2.outlets[0].pressure_kpa - expect).abs() < 1e-9);
    }

    #[test]
    fn series_valve_and_zero_flow_are_harmless() {
        // A POC that is also a valve, with a demand node of zero flow ignored.
        let mut net = Network::default();
        let p = net.add_node(Node {
            poc: Some(Poc {
                static_kpa: 300.0,
                available_lpm: 50.0,
                ..Poc::default()
            }),
            valve: Some(ValveSpec { zone: 4, kv: 6.0 }),
            ..node("P")
        });
        let o = net.add_node(outlet("O", 12.0, 100.0, 1));
        let zero = net.add_node(outlet("Z", 0.0, 100.0, 2));
        net.add_edge(edge(p, o, 0.0, 25.0));
        net.add_edge(edge(p, zero, 5.0, 25.0));
        let sol = solve(&net);
        let z = sol.zone(4).unwrap();
        assert_eq!(z.outlets.len(), 1);
        // 12 l/min = 0.72 m3/h, kv 6: loss 100 * 0.12^2 = 1.44 kPa.
        assert!((z.outlets[0].pressure_kpa - (300.0 - 1.44)).abs() < 1e-9);
        // Zero-length pipes never raise a velocity warning.
        assert!(z.warnings.is_empty());
    }

    #[test]
    fn empty_label_falls_back_to_node_number() {
        let mut net = example();
        net.nodes[3].label.clear();
        net.nodes[3].demand.as_mut().unwrap().min_kpa = 1000.0;
        let sol = solve(&net);
        assert!(sol.zone(1).unwrap().warnings[0].message.contains("node 3"));
    }

    fn mainline(a: NodeId, b: NodeId, length_m: f64) -> Edge {
        Edge {
            through: true,
            ..edge(a, b, length_m, 50.0)
        }
    }

    /// A mainline POC - V1 - V2 with laterals. Valve 1 sits on the through mainline: zone 2
    /// does not pay its loss, and a tap past valve 1 does not belong to zone 1.
    #[test]
    fn a_valve_on_a_through_mainline_acts_only_on_its_lateral_side() {
        let mut net = Network::default();
        let poc = net.add_node(Node {
            poc: Some(Poc {
                static_kpa: 400.0,
                available_lpm: 100.0,
                ..Poc::default()
            }),
            ..node("POC")
        });
        let v1 = net.add_node(Node {
            valve: Some(ValveSpec { zone: 1, kv: 2.0 }),
            ..node("V1")
        });
        let v2 = net.add_node(Node {
            valve: Some(ValveSpec { zone: 2, kv: 12.0 }),
            ..node("V2")
        });
        let tap = net.add_node(outlet("tap", 6.0, 100.0, 5));
        let a = net.add_node(outlet("A", 10.0, 100.0, 6));
        let b = net.add_node(outlet("B", 20.0, 100.0, 7));
        net.add_edge(mainline(poc, v1, 50.0));
        net.add_edge(mainline(v1, v2, 50.0));
        // A tap on the mainline after valve 1 (a link to the mainline node): the edge is through.
        net.add_edge(Edge {
            through: true,
            ..edge(v2, tap, 0.0, 50.0)
        });
        net.add_edge(edge(v1, a, 10.0, 25.4));
        net.add_edge(edge(v2, b, 10.0, 25.4));
        let sol = solve(&net);
        let z1 = sol.zone(1).unwrap();
        let z2 = sol.zone(2).unwrap();
        assert_eq!(z1.outlets.len(), 1);
        assert_eq!(z1.outlets[0].label, "A");
        assert_eq!(z2.outlets[0].label, "B");
        // The tap is on a through pipe after valve 1 and before valve 2 (also through): zone 0.
        assert!(sol.zone(0).is_some_and(|z| z.outlets[0].label == "tap"));
        // Zone 2 does not pay the loss of valve 1 (kv 2 is a very tight valve).
        let expect = 400.0
            - pipe_loss_kpa(20.0, 50.0, 150.0, 100.0)
            - valve_loss_kpa(20.0, 12.0)
            - pipe_loss_kpa(20.0, 25.4, 150.0, 10.0);
        assert!((z2.outlets[0].pressure_kpa - expect).abs() < 1e-9);
        // Zone 1 pays the loss of valve 1 for its own flow: 10 l/min = 0.6 m3/h, kv 2: 9 kPa.
        let expect = 400.0
            - pipe_loss_kpa(10.0, 50.0, 150.0, 50.0)
            - valve_loss_kpa(10.0, 2.0)
            - pipe_loss_kpa(10.0, 25.4, 150.0, 10.0);
        assert!((z1.outlets[0].pressure_kpa - expect).abs() < 1e-9);
        assert!((valve_loss_kpa(10.0, 2.0) - 9.0).abs() < 1e-9);
    }

    #[test]
    fn height_fittings_meter_and_dynamic_pressure() {
        let mut net = example();
        // Outlet A is 3 m higher than J: 3 * 9.80665 = 29.42 kPa less.
        let base = solve(&net).zone(1).unwrap().outlets[0].pressure_kpa;
        net.nodes[2].z_m = Some(10.0);
        net.nodes[3].z_m = Some(13.0);
        let high = solve(&net).zone(1).unwrap().outlets[0].pressure_kpa;
        assert!((base - high - 3.0 * 9.80665).abs() < 1e-9);
        // 10 percent fittings on all pipes: the pipe losses grow by 10 percent.
        let mut net = example();
        net.fittings_pct = 10.0;
        let fit = solve(&net).zone(1).unwrap().outlets[0].pressure_kpa;
        let pipes = 1.6296 + 44.1079 + 6.1091;
        assert!((base - fit - 0.1 * pipes).abs() < 1e-3);
        // Supply loss 20 kPa and meter loss 8 kPa at 100 l/min; the zone runs 30 l/min:
        // 20 * 0.3^1.852 + 8 * 0.3^2 = 2.1396 + 0.72.
        let mut net = example();
        if let Some(p) = &mut net.nodes[0].poc {
            p.supply_loss_kpa = 20.0;
            p.meter_loss_kpa = 8.0;
        }
        let z = solve(&net).zone(1).unwrap().clone();
        let loss = 20.0 * 0.3f64.powf(1.852) + 8.0 * 0.09;
        assert!((z.dynamic_kpa - (400.0 - loss)).abs() < 1e-9);
        assert!((z.outlets[0].pressure_kpa - (base - loss)).abs() < 1e-6);
        assert_eq!(z.static_kpa, 400.0);
        // No flow: the dynamic pressure is the static pressure.
        let p = net.nodes[0].poc.unwrap();
        assert_eq!(p.dynamic_kpa(0.0), 400.0);
    }

    /// Christiansen factor for 10 emitters: 1/2.852 + 1/20 + 0.9230/600 = 0.4021.
    #[test]
    fn drip_lines_lose_pressure_with_the_christiansen_factor_in_both_directions() {
        assert!((christiansen(10) - 0.40214).abs() < 1e-4);
        assert!((christiansen(1) - (1.0 / 2.852 + 0.5 + 0.852f64.sqrt() / 6.0)).abs() < 1e-12);
        let build = |flip: bool| {
            let mut net = Network::default();
            let poc = net.add_node(Node {
                poc: Some(Poc {
                    static_kpa: 300.0,
                    available_lpm: 50.0,
                    ..Poc::default()
                }),
                ..node("POC")
            });
            let v = net.add_node(Node {
                valve: Some(ValveSpec { zone: 1, kv: 6.0 }),
                ..node("V")
            });
            let far = net.add_node(node("far"));
            net.add_edge(edge(poc, v, 5.0, 25.4));
            let (a, b) = if flip { (far, v) } else { (v, far) };
            net.add_edge(Edge {
                drip: Some(DripInfo {
                    flow_lpm: 3.0,
                    emitters: 10,
                    min_kpa: 70.0,
                }),
                ..edge(a, b, 30.0, 16.0)
            });
            solve(&net)
        };
        for flip in [false, true] {
            let sol = build(flip);
            let z = sol.zone(1).unwrap();
            assert_eq!(z.flow_lpm, 3.0);
            let o = &z.outlets[0];
            assert!(o.label.contains("Drip line"));
            let expect = 300.0
                - pipe_loss_kpa(3.0, 25.4, 150.0, 5.0)
                - valve_loss_kpa(3.0, 6.0)
                - christiansen(10) * pipe_loss_kpa(3.0, 16.0, 150.0, 30.0);
            assert!((o.pressure_kpa - expect).abs() < 1e-9, "{flip}");
        }
        // A drip line with no water path warns.
        let mut net = Network::default();
        net.add_node(Node {
            poc: Some(Poc {
                static_kpa: 300.0,
                available_lpm: 50.0,
                ..Poc::default()
            }),
            ..node("POC")
        });
        let a = net.add_node(node("a"));
        let b = net.add_node(node("b"));
        net.add_edge(Edge {
            drip: Some(DripInfo {
                flow_lpm: 1.0,
                emitters: 2,
                min_kpa: 70.0,
            }),
            ..edge(a, b, 10.0, 16.0)
        });
        let sol = solve(&net);
        assert!(
            sol.warnings
                .iter()
                .any(|w| w.kind == WarningKind::Unconnected)
        );
    }
}
