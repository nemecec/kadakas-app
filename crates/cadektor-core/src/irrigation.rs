//! Irrigation objects: outlets, lines, points of connection, valves and controllers.
//!
//! These are the drawing objects only. They are one `Shape::Irrigation` variant. The hydraulic
//! model (catalog, pressure loss, zones, reports) is in the crate `cadektor-irrigation`.
//!
//! Units: lengths in mm, angles in degrees, pressure in kPa, flow in l/min.
//! Objects refer to catalog entries by name (a head model, a pipe size and material).
//! The catalog is a document resource (`ResourceBody::IrrigationCatalog`).

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// What a line carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineType {
    /// From the point of connection to the valves. Under pressure all the time.
    Mainline,
    /// From a valve to the outlets. Under pressure only while the zone runs.
    #[default]
    Lateral,
    /// A line with emitters along it. Its flow counts at the last point.
    Drip,
}

impl LineType {
    pub const ALL: [LineType; 3] = [LineType::Mainline, LineType::Lateral, LineType::Drip];

    pub fn id(self) -> &'static str {
        match self {
            LineType::Mainline => "mainline",
            LineType::Lateral => "lateral",
            LineType::Drip => "drip",
        }
    }

    pub fn from_name(s: &str) -> Option<LineType> {
        let s = s.trim().to_ascii_lowercase();
        LineType::ALL
            .into_iter()
            .find(|t| t.id() == s || s == format!("{}_line", t.id()))
    }
}

fn full_circle() -> f64 {
    360.0
}

fn one() -> u32 {
    1
}

/// An irrigation object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IrrigationShape {
    /// A spray head, rotor, drip emitter or bubbler.
    /// `arc` is the spray angle in degrees, `direction` the direction of the middle of the arc
    /// (degrees counterclockwise from +X). `radius` (mm) overrides the radius of the model.
    /// `zone` is set by the calculation (the valve upstream) and gives the color.
    Outlet {
        at: DVec2,
        model: String,
        #[serde(default = "full_circle")]
        arc: f64,
        #[serde(default)]
        direction: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<u32>,
    },
    /// A straight-segment polyline. `size` and `material` name a pipe of the catalog.
    /// A drip line has emitters every `emitter_spacing` mm (default 300), each using
    /// `emitter_flow` l/h (default 2).
    Line {
        points: Vec<DVec2>,
        #[serde(default)]
        line_type: LineType,
        size: String,
        material: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        emitter_spacing: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        emitter_flow: Option<f64>,
    },
    /// Point of connection: the water supply. `static_pressure` in kPa, `available_flow` in l/min.
    Poc {
        at: DVec2,
        #[serde(default)]
        name: String,
        static_pressure: f64,
        available_flow: f64,
    },
    /// Control valve of a zone. `kv` is the flow factor in m3/h at 1 bar of pressure loss.
    Valve {
        at: DVec2,
        zone: u32,
        #[serde(default)]
        name: String,
        kv: f64,
        /// Name of the controller that switches this valve, with the station on it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        controller: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        station: Option<u32>,
    },
    /// Irrigation controller (timer) with a number of stations.
    Controller {
        at: DVec2,
        name: String,
        #[serde(default = "one")]
        stations: u32,
    },
}

/// A color per zone: a fixed palette of distinct hues, repeated for large zone numbers.
/// Zone 0 (no valve) is gray.
pub fn zone_color(zone: u32) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 10] = [
        [31, 119, 180],
        [255, 127, 14],
        [44, 160, 44],
        [214, 39, 40],
        [148, 103, 189],
        [140, 86, 75],
        [227, 119, 194],
        [188, 189, 34],
        [23, 190, 207],
        [0, 0, 0],
    ];
    if zone == 0 {
        [128, 128, 128]
    } else {
        PALETTE[(zone as usize - 1) % PALETTE.len()]
    }
}

impl IrrigationShape {
    /// The kind name, as used by `objects.list` and worksheet criteria.
    pub fn kind(&self) -> &'static str {
        match self {
            IrrigationShape::Outlet { .. } => "irrigation_outlet",
            IrrigationShape::Line { .. } => "irrigation_line",
            IrrigationShape::Poc { .. } => "irrigation_poc",
            IrrigationShape::Valve { .. } => "irrigation_valve",
            IrrigationShape::Controller { .. } => "irrigation_controller",
        }
    }

    /// The position of a point object, or the first point of a line.
    pub fn anchor(&self) -> DVec2 {
        match self {
            IrrigationShape::Outlet { at, .. }
            | IrrigationShape::Poc { at, .. }
            | IrrigationShape::Valve { at, .. }
            | IrrigationShape::Controller { at, .. } => *at,
            IrrigationShape::Line { points, .. } => points.first().copied().unwrap_or_default(),
        }
    }

    /// Length of a line in mm. Zero for other objects.
    pub fn length(&self) -> f64 {
        match self {
            IrrigationShape::Line { points, .. } => {
                points.windows(2).map(|w| w[0].distance(w[1])).sum()
            }
            _ => 0.0,
        }
    }

    /// Calls `f` with every number of the object.
    pub fn for_each_number(&self, f: &mut dyn FnMut(f64)) {
        let mut v = |p: &DVec2| {
            f(p.x);
            f(p.y);
        };
        match self {
            IrrigationShape::Outlet {
                at,
                arc,
                direction,
                radius,
                ..
            } => {
                v(at);
                v(&DVec2::new(*arc, *direction));
                if let Some(r) = radius {
                    f(*r);
                }
            }
            IrrigationShape::Line {
                points,
                emitter_spacing,
                emitter_flow,
                ..
            } => {
                points.iter().for_each(&mut v);
                for x in emitter_spacing.iter().chain(emitter_flow.iter()) {
                    f(*x);
                }
            }
            IrrigationShape::Poc {
                at,
                static_pressure,
                available_flow,
                ..
            } => {
                v(at);
                v(&DVec2::new(*static_pressure, *available_flow));
            }
            IrrigationShape::Valve { at, kv, .. } => {
                v(at);
                f(*kv);
            }
            IrrigationShape::Controller { at, .. } => v(at),
        }
    }

    /// True if every number is finite.
    pub fn is_finite(&self) -> bool {
        let mut ok = true;
        self.for_each_number(&mut |x| ok &= x.is_finite());
        ok
    }

    pub fn translate(&mut self, d: DVec2) {
        match self {
            IrrigationShape::Outlet { at, .. }
            | IrrigationShape::Poc { at, .. }
            | IrrigationShape::Valve { at, .. }
            | IrrigationShape::Controller { at, .. } => *at += d,
            IrrigationShape::Line { points, .. } => points.iter_mut().for_each(|p| *p += d),
        }
    }

    /// Bounding box. An outlet counts its position only: its radius depends on the catalog.
    pub fn bounds(&self) -> (DVec2, DVec2) {
        match self {
            IrrigationShape::Line { points, .. } => points.iter().fold(
                (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            ),
            other => (other.anchor(), other.anchor()),
        }
    }

    /// Turns the object about `center` by `angle` radians (counterclockwise).
    /// An outlet also turns its direction.
    pub fn rotate(&mut self, center: DVec2, angle: f64) {
        let rot = glam::DMat2::from_angle(angle);
        let turn = |p: &mut DVec2| *p = center + rot * (*p - center);
        match self {
            IrrigationShape::Outlet { at, direction, .. } => {
                turn(at);
                *direction += angle.to_degrees();
            }
            IrrigationShape::Poc { at, .. }
            | IrrigationShape::Valve { at, .. }
            | IrrigationShape::Controller { at, .. } => turn(at),
            IrrigationShape::Line { points, .. } => points.iter_mut().for_each(turn),
        }
    }

    /// The first problem that makes the object invalid, if any.
    pub fn problem(&self) -> Option<String> {
        if !self.is_finite() {
            return Some("irrigation object has a number that is not finite".into());
        }
        match self {
            IrrigationShape::Outlet {
                model, arc, radius, ..
            } => {
                if model.trim().is_empty() {
                    Some("an outlet needs a model name".into())
                } else if !(*arc > 0.0 && *arc <= 360.0) {
                    Some("the arc of an outlet must be above 0 and at most 360 degrees".into())
                } else if radius.is_some_and(|r| r <= 0.0) {
                    Some("the radius of an outlet must be positive".into())
                } else {
                    None
                }
            }
            IrrigationShape::Line {
                points,
                size,
                material,
                emitter_spacing,
                emitter_flow,
                ..
            } => {
                if points.len() < 2 {
                    Some("an irrigation line needs 2 or more points".into())
                } else if size.trim().is_empty() || material.trim().is_empty() {
                    Some("an irrigation line needs a size and a material".into())
                } else if emitter_spacing.is_some_and(|s| s <= 0.0)
                    || emitter_flow.is_some_and(|s| s <= 0.0)
                {
                    Some("emitter spacing and flow must be positive".into())
                } else {
                    None
                }
            }
            IrrigationShape::Poc {
                static_pressure,
                available_flow,
                ..
            } => {
                if *static_pressure <= 0.0 || *available_flow <= 0.0 {
                    Some(
                        "the static pressure and the available flow of a POC must be positive"
                            .into(),
                    )
                } else {
                    None
                }
            }
            IrrigationShape::Valve { kv, .. } => {
                if *kv <= 0.0 {
                    Some("the flow factor kv of a valve must be positive".into())
                } else {
                    None
                }
            }
            IrrigationShape::Controller { stations, .. } => {
                if *stations == 0 {
                    Some("a controller needs 1 or more stations".into())
                } else {
                    None
                }
            }
        }
    }
}

/// Points of the coverage of an outlet: a circle, or a fan (the position, then the arc).
/// `steps` is the number of segments of a full circle. The arc is centered on `direction` (degrees).
pub fn sector_points(at: DVec2, radius: f64, arc: f64, direction: f64, steps: usize) -> Vec<DVec2> {
    let arc = arc.clamp(0.0, 360.0);
    let n = ((steps as f64 * arc / 360.0).ceil() as usize).max(2);
    let start = (direction - arc / 2.0).to_radians();
    let sweep = arc.to_radians();
    let mut pts = Vec::with_capacity(n + 2);
    if arc < 360.0 {
        pts.push(at);
    }
    let last = if arc < 360.0 { n } else { n - 1 };
    for i in 0..=last {
        pts.push(at + DVec2::from_angle(start + sweep * i as f64 / n as f64) * radius);
    }
    pts
}

/// The radius of an outlet in mm: its own value, if it has one.
pub fn outlet_radius(shape: &IrrigationShape) -> Option<f64> {
    match shape {
        IrrigationShape::Outlet { radius, .. } => *radius,
        _ => None,
    }
}

fn css(c: [u8; 3]) -> String {
    format!("rgb({},{},{})", c[0], c[1], c[2])
}

/// SVG elements of an irrigation object. `scale` is the drawing scale (1 mm on paper is `scale` mm
/// in the model). An outlet draws a translucent fan in the color of its zone.
pub fn svg(id: u64, shape: &IrrigationShape, scale: f64) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let k = shape.kind();
    let sw = 0.25 * scale;
    let marker = 1.2 * scale;
    match shape {
        IrrigationShape::Outlet {
            at,
            arc,
            direction,
            radius,
            zone,
            ..
        } => {
            let color = css(zone_color(zone.unwrap_or(0)));
            if let Some(r) = radius {
                let pts: Vec<String> = sector_points(*at, *r, *arc, *direction, 48)
                    .iter()
                    .map(|p| format!("{},{}", p.x, -p.y))
                    .collect();
                let _ = writeln!(
                    s,
                    r#"<polygon points="{}" fill="{color}" fill-opacity="0.25" stroke="{color}" stroke-width="{sw}" data-kind="{k}" data-id="{id}"/>"#,
                    pts.join(" ")
                );
            }
            let _ = writeln!(
                s,
                r#"<circle cx="{}" cy="{}" r="{marker}" fill="{color}" stroke="black" stroke-width="{sw}" data-kind="{k}" data-id="{id}"/>"#,
                at.x, -at.y
            );
        }
        IrrigationShape::Line {
            points, line_type, ..
        } => {
            let (color, w, dash) = match line_type {
                LineType::Mainline => ("rgb(20,50,140)", 0.6 * scale, String::new()),
                LineType::Lateral => ("rgb(40,110,200)", 0.35 * scale, String::new()),
                LineType::Drip => (
                    "rgb(30,140,70)",
                    0.25 * scale,
                    format!(r#" stroke-dasharray="{} {}""#, 2.0 * scale, scale),
                ),
            };
            let pts: Vec<String> = points.iter().map(|p| format!("{},{}", p.x, -p.y)).collect();
            let _ = writeln!(
                s,
                r#"<polyline points="{}" fill="none" stroke="{color}" stroke-width="{w}"{dash} data-kind="{k}" data-id="{id}"/>"#,
                pts.join(" ")
            );
        }
        IrrigationShape::Poc { at, .. } => {
            let r = 2.0 * scale;
            let _ = writeln!(
                s,
                r#"<g data-kind="{k}" data-id="{id}"><circle cx="{x}" cy="{y}" r="{r}" fill="rgb(20,50,140)" fill-opacity="0.3" stroke="rgb(20,50,140)" stroke-width="{sw}"/><path d="M {x1} {y} L {x2} {y} M {x} {y1} L {x} {y2}" stroke="rgb(20,50,140)" stroke-width="{sw}"/></g>"#,
                x = at.x,
                y = -at.y,
                x1 = at.x - r,
                x2 = at.x + r,
                y1 = -at.y - r,
                y2 = -at.y + r
            );
        }
        IrrigationShape::Valve { at, zone, .. } => {
            let c = css(zone_color(*zone));
            let r = 1.6 * scale;
            let _ = writeln!(
                s,
                r#"<polygon points="{},{} {},{} {},{} {},{}" fill="{c}" fill-opacity="0.5" stroke="{c}" stroke-width="{sw}" data-kind="{k}" data-id="{id}"/>"#,
                at.x - r,
                -at.y - r,
                at.x + r,
                -at.y + r,
                at.x + r,
                -at.y - r,
                at.x - r,
                -at.y + r
            );
        }
        IrrigationShape::Controller { at, .. } => {
            let r = 2.0 * scale;
            let _ = writeln!(
                s,
                r#"<rect x="{}" y="{}" width="{}" height="{}" fill="rgb(255,255,255)" stroke="rgb(20,50,140)" stroke-width="{sw}" data-kind="{k}" data-id="{id}"/>"#,
                at.x - r,
                -at.y - r,
                2.0 * r,
                2.0 * r
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outlet() -> IrrigationShape {
        IrrigationShape::Outlet {
            at: DVec2::new(1.0, 2.0),
            model: "Spray-4.0".into(),
            arc: 90.0,
            direction: 45.0,
            radius: None,
            zone: None,
        }
    }

    fn line() -> IrrigationShape {
        IrrigationShape::Line {
            points: vec![
                DVec2::ZERO,
                DVec2::new(3000.0, 0.0),
                DVec2::new(3000.0, 4000.0),
            ],
            line_type: LineType::Lateral,
            size: "32".into(),
            material: "PE".into(),
            emitter_spacing: None,
            emitter_flow: None,
        }
    }

    #[test]
    fn kinds_lengths_and_anchor() {
        assert_eq!(outlet().kind(), "irrigation_outlet");
        assert_eq!(line().kind(), "irrigation_line");
        assert_eq!(line().length(), 7000.0);
        assert_eq!(outlet().length(), 0.0);
        assert_eq!(outlet().anchor(), DVec2::new(1.0, 2.0));
        assert_eq!(line().anchor(), DVec2::ZERO);
        let poc = IrrigationShape::Poc {
            at: DVec2::X,
            name: String::new(),
            static_pressure: 400.0,
            available_flow: 100.0,
        };
        let valve = IrrigationShape::Valve {
            at: DVec2::Y,
            zone: 1,
            name: String::new(),
            kv: 5.0,
            controller: None,
            station: None,
        };
        let ctl = IrrigationShape::Controller {
            at: DVec2::ONE,
            name: "A".into(),
            stations: 4,
        };
        assert_eq!(
            [poc.kind(), valve.kind(), ctl.kind()],
            [
                "irrigation_poc",
                "irrigation_valve",
                "irrigation_controller"
            ]
        );
        assert!(poc.problem().is_none() && valve.problem().is_none() && ctl.problem().is_none());
    }

    #[test]
    fn line_types_parse() {
        assert_eq!(LineType::from_name("Mainline"), Some(LineType::Mainline));
        assert_eq!(LineType::from_name("drip_line"), Some(LineType::Drip));
        assert_eq!(LineType::from_name("x"), None);
        for t in LineType::ALL {
            assert_eq!(LineType::from_name(t.id()), Some(t));
        }
    }

    #[test]
    fn translate_bounds_rotate() {
        let mut l = line();
        l.translate(DVec2::new(10.0, 20.0));
        assert_eq!(
            l.bounds(),
            (DVec2::new(10.0, 20.0), DVec2::new(3010.0, 4020.0))
        );
        let mut o = outlet();
        o.translate(DVec2::ONE);
        assert_eq!(o.bounds().0, DVec2::new(2.0, 3.0));
        let mut o = IrrigationShape::Outlet {
            at: DVec2::new(1.0, 0.0),
            model: "m".into(),
            arc: 90.0,
            direction: 0.0,
            radius: None,
            zone: None,
        };
        o.rotate(DVec2::ZERO, std::f64::consts::FRAC_PI_2);
        let IrrigationShape::Outlet { at, direction, .. } = o else {
            unreachable!()
        };
        assert!((at - DVec2::new(0.0, 1.0)).length() < 1e-12);
        assert!((direction - 90.0).abs() < 1e-9);
        let mut l = line();
        l.rotate(DVec2::ZERO, std::f64::consts::PI);
        assert!((l.anchor()).length() < 1e-9);
        let mut p = IrrigationShape::Controller {
            at: DVec2::X,
            name: "c".into(),
            stations: 1,
        };
        p.rotate(DVec2::ZERO, std::f64::consts::PI);
        assert!((p.anchor() + DVec2::X).length() < 1e-12);
    }

    #[test]
    fn problems() {
        assert!(outlet().problem().is_none());
        assert!(line().problem().is_none());
        let mut o = outlet();
        if let IrrigationShape::Outlet { arc, .. } = &mut o {
            *arc = 0.0;
        }
        assert!(o.problem().unwrap().contains("arc"));
        if let IrrigationShape::Outlet { arc, model, .. } = &mut o {
            *arc = 90.0;
            *model = " ".into();
        }
        assert!(o.problem().unwrap().contains("model"));
        if let IrrigationShape::Outlet { model, radius, .. } = &mut o {
            *model = "m".into();
            *radius = Some(-1.0);
        }
        assert!(o.problem().unwrap().contains("radius"));
        if let IrrigationShape::Outlet { radius, arc, .. } = &mut o {
            *radius = Some(1.0);
            *arc = f64::NAN;
        }
        assert!(o.problem().unwrap().contains("finite"));
        let mut l = line();
        if let IrrigationShape::Line { points, .. } = &mut l {
            points.truncate(1);
        }
        assert!(l.problem().unwrap().contains("2 or more"));
        let mut l = line();
        if let IrrigationShape::Line { size, .. } = &mut l {
            size.clear();
        }
        assert!(l.problem().unwrap().contains("size"));
        let mut l = line();
        if let IrrigationShape::Line { emitter_flow, .. } = &mut l {
            *emitter_flow = Some(0.0);
        }
        assert!(l.problem().unwrap().contains("emitter"));
        let poc = IrrigationShape::Poc {
            at: DVec2::ZERO,
            name: String::new(),
            static_pressure: 0.0,
            available_flow: 10.0,
        };
        assert!(poc.problem().unwrap().contains("POC"));
        let valve = IrrigationShape::Valve {
            at: DVec2::ZERO,
            zone: 1,
            name: String::new(),
            kv: 0.0,
            controller: None,
            station: None,
        };
        assert!(valve.problem().unwrap().contains("kv"));
        let ctl = IrrigationShape::Controller {
            at: DVec2::ZERO,
            name: String::new(),
            stations: 0,
        };
        assert!(ctl.problem().unwrap().contains("station"));
    }

    #[test]
    fn json_round_trip_with_defaults() {
        let v: IrrigationShape =
            serde_json::from_str(r#"{"type":"outlet","at":[0,0],"model":"Spray-4.0"}"#).unwrap();
        let IrrigationShape::Outlet { arc, direction, .. } = &v else {
            panic!()
        };
        assert_eq!((*arc, *direction), (360.0, 0.0));
        for s in [outlet(), line()] {
            let back: IrrigationShape =
                serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn sector_points_and_svg() {
        let full = sector_points(DVec2::ZERO, 10.0, 360.0, 0.0, 36);
        assert_eq!(full.len(), 36);
        assert!(full.iter().all(|p| (p.length() - 10.0).abs() < 1e-9));
        let fan = sector_points(DVec2::ZERO, 10.0, 90.0, 90.0, 36);
        assert_eq!(fan.len(), 11);
        assert_eq!(fan[0], DVec2::ZERO);
        // The fan is centered on 90 degrees: it spans 45 to 135.
        assert!((fan[1].to_angle().to_degrees() - 45.0).abs() < 1e-9);
        assert!((fan[10].to_angle().to_degrees() - 135.0).abs() < 1e-9);
        // A tiny arc still has two edge points.
        assert_eq!(sector_points(DVec2::ZERO, 1.0, 1.0, 0.0, 36).len(), 4);

        let mut o = outlet();
        assert!(!svg(1, &o, 100.0).contains("polygon"));
        if let IrrigationShape::Outlet { radius, zone, .. } = &mut o {
            *radius = Some(4000.0);
            *zone = Some(2);
        }
        let text = svg(1, &o, 100.0);
        assert!(text.contains("polygon") && text.contains("irrigation_outlet"));
        assert_eq!(outlet_radius(&o), Some(4000.0));
        assert_eq!(outlet_radius(&line()), None);
        for t in LineType::ALL {
            let mut l = line();
            if let IrrigationShape::Line { line_type, .. } = &mut l {
                *line_type = t;
            }
            assert!(svg(2, &l, 100.0).contains("polyline"));
        }
        let at = DVec2::ZERO;
        let others = [
            IrrigationShape::Poc {
                at,
                name: String::new(),
                static_pressure: 1.0,
                available_flow: 1.0,
            },
            IrrigationShape::Valve {
                at,
                zone: 1,
                name: String::new(),
                kv: 1.0,
                controller: None,
                station: None,
            },
            IrrigationShape::Controller {
                at,
                name: String::new(),
                stations: 1,
            },
        ];
        for o in &others {
            assert!(svg(3, o, 100.0).contains(o.kind()));
        }
    }

    #[test]
    fn zone_colors_differ_and_wrap() {
        assert_eq!(zone_color(0), [128, 128, 128]);
        assert_ne!(zone_color(1), zone_color(2));
        assert_eq!(zone_color(1), zone_color(11));
    }
}
