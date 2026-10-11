//! Snapping glue: the pure math is in `kadakas_core::snapping`. This module builds the geometry
//! that the pointer can snap to (the irrigation objects add their own points) and gives the
//! result with its cue label and guides. A UI program draws the marks.

use glam::DVec2;
use kadakas_core::irrigation::IrrigationShape;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::snapping::{self, Context, Geometry, Guide, Kind, Reach, Settings, Smart};
use kadakas_l10n::tr;

/// What an irrigation object offers to snap to. `Vertex` is an end or corner, `Center` a center.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Vertex,
    Center,
}

pub type SnapPoint = (DVec2, SnapKind);

/// Points that the pointer snaps to: the points of a line and the position of other objects.
pub fn snap_points(i: &IrrigationShape, out: &mut Vec<SnapPoint>) {
    match i {
        IrrigationShape::Line { points, .. } => {
            out.extend(points.iter().map(|p| (*p, SnapKind::Vertex)))
        }
        other => out.push((other.anchor(), SnapKind::Center)),
    }
}

/// A snap for the pointer, with what the canvas draws.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapResult {
    pub point: DVec2,
    pub kind: Kind,
    /// The text of the cue label.
    pub cue: String,
    pub guides: Vec<Guide>,
}

impl From<snapping::Snap> for SnapResult {
    fn from(s: snapping::Snap) -> SnapResult {
        SnapResult {
            point: s.point,
            kind: s.kind,
            cue: cue_text(&s),
            guides: s.guides,
        }
    }
}

/// The name of a snap kind in the language of the UI (the core crate has English names).
pub fn kind_name(kind: Kind) -> String {
    match kind {
        Kind::Grid => tr!("uic-snap-kind-grid"),
        Kind::Endpoint => tr!("uic-snap-kind-endpoint"),
        Kind::Midpoint => tr!("uic-snap-kind-midpoint"),
        Kind::Center => tr!("uic-snap-kind-center"),
        Kind::Quadrant => tr!("uic-snap-kind-quadrant"),
        Kind::Intersection => tr!("uic-snap-kind-intersection"),
        Kind::Edge => tr!("uic-snap-kind-on-edge"),
        Kind::Perpendicular => tr!("uic-snap-kind-perpendicular"),
        Kind::Tangent => tr!("uic-snap-kind-tangent"),
        Kind::Angle => tr!("uic-snap-kind-angle"),
        Kind::Distance => tr!("uic-snap-kind-distance"),
        Kind::SmartPoint => tr!("uic-snap-kind-smart-point"),
        Kind::SmartEdge => tr!("uic-snap-kind-smart-edge"),
    }
}

/// The text of the cue label at the pointer, in the language and the units of the UI.
fn cue_text(s: &snapping::Snap) -> String {
    let units = kadakas_units::current();
    let angle = s.angle_deg.map(|a| units.angle_compact(a));
    let length = s.length.map(|l| units.length_compact(l));
    match (s.kind, angle, length) {
        (Kind::Angle, Some(angle), Some(length)) => {
            tr!("uic-snap-cue-angle-length", angle = angle, length = length)
        }
        (Kind::Angle, Some(angle), None) => tr!("uic-snap-cue-angle", angle = angle),
        (Kind::Distance, _, Some(length)) => tr!("uic-snap-cue-distance", length = length),
        (kind, Some(angle), _) => {
            tr!(
                "uic-snap-cue-kind-angle",
                kind = kind_name(kind),
                angle = angle
            )
        }
        (kind, _, _) => kind_name(kind),
    }
}

/// The geometry near the pointer: objects of the document and irrigation objects.
pub fn geometry(
    doc: &Document,
    exclude: &[ObjectId],
    settings: &Settings,
    near: Option<(DVec2, f64)>,
) -> Geometry {
    let reach = Reach::from(settings);
    let mut g = Geometry::from_doc(doc, exclude, reach, near);
    for o in doc.objects.values() {
        let kadakas_core::model::Shape::Irrigation(i) = &o.shape else {
            continue;
        };
        if exclude.contains(&o.id) || !reach.allows(doc, o) {
            continue;
        }
        let mut pts: Vec<SnapPoint> = Vec::new();
        snap_points(i, &mut pts);
        for (p, k) in pts {
            let near_enough = near.is_none_or(|(c, r)| p.distance(c) <= r);
            if near_enough {
                g.add_point(
                    p,
                    match k {
                        SnapKind::Vertex => Kind::Endpoint,
                        SnapKind::Center => Kind::Center,
                    },
                );
            }
        }
    }
    g
}

/// Where the pointer snaps to.
#[allow(clippy::too_many_arguments)]
pub fn snap_cursor(
    doc: &Document,
    cursor: DVec2,
    tol: f64,
    grid_step: f64,
    settings: &Settings,
    smart: &Smart,
    anchor: Option<DVec2>,
    prior_direction: Option<f64>,
    exclude: &[ObjectId],
) -> Option<SnapResult> {
    let geom = geometry(doc, exclude, settings, Some((cursor, tol * 4.0)));
    let ctx = Context {
        tol,
        grid_step,
        anchor,
        prior_direction,
    };
    snapping::snap(cursor, &ctx, settings, &geom, smart).map(SnapResult::from)
}

/// The T key: takes the point or edge under the pointer as a smart point or smart edge. Returns
/// the text for the status line.
pub fn acquire_smart(
    doc: &Document,
    cursor: DVec2,
    tol: f64,
    settings: &Settings,
    smart: &mut Smart,
) -> String {
    let geom = geometry(doc, &[], settings, Some((cursor, tol * 4.0)));
    match smart.acquire_at(cursor, &geom, tol, settings.set.smart_edges) {
        Some(snapping::Acquired::Point(_)) => tr!("uic-snap-smart-point-set"),
        Some(snapping::Acquired::Edge(..)) => tr!("uic-snap-smart-edge-set"),
        None => tr!("uic-snap-no-object-point-under-the-pointer-move"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::Session;
    use kadakas_core::command::Command;
    use kadakas_core::model::{Attributes, Shape};

    fn add(s: &mut Session, shape: Shape) -> ObjectId {
        ObjectId(
            s.editor
                .execute(&Command::AddObject {
                    shape,
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap()
                .created[0],
        )
    }

    #[test]
    fn document_points_and_exclusion() {
        let mut s = Session::default();
        let a = add(
            &mut s,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(100.0, 50.0),
            },
        );
        add(
            &mut s,
            Shape::Plant {
                at: DVec2::new(5.0, 5.0),
                species: "x".into(),
                spread: 10.0,
                height: 10.0,
            },
        );
        let st = Settings::default();
        let g = geometry(s.editor.doc(), &[], &st, None);
        // Rectangle: 4 corners, 4 midpoints, the center. Plant: its position.
        assert_eq!(g.points.len(), 10);
        assert!(
            g.points
                .iter()
                .any(|p| p.p == DVec2::new(50.0, 25.0) && p.kind == Kind::Center)
        );
        assert_eq!(geometry(s.editor.doc(), &[a], &st, None).points.len(), 1);
        // Only near the pointer.
        let near = geometry(
            s.editor.doc(),
            &[],
            &st,
            Some((DVec2::new(1000.0, 1000.0), 10.0)),
        );
        assert!(near.points.is_empty());
    }

    /// A snap with the data that the cue names.
    fn snap_with(kind: Kind, angle: Option<f64>, length: Option<f64>) -> snapping::Snap {
        snapping::Snap {
            point: DVec2::ZERO,
            kind,
            cue: "English".into(),
            angle_deg: angle,
            length,
            guides: Vec::new(),
        }
    }

    #[test]
    fn the_cue_label_speaks_the_language_and_the_units_of_the_ui() {
        let cue = |s: snapping::Snap| SnapResult::from(s).cue;
        let all = [
            (Kind::Grid, "Grid", "Ruudustik"),
            (Kind::Endpoint, "Endpoint", "Otspunkt"),
            (Kind::Midpoint, "Midpoint", "Poolituspunkt"),
            (Kind::Center, "Center", "Keskpunkt"),
            (Kind::Quadrant, "Quadrant", "Veerandipunkt"),
            (Kind::Intersection, "Intersection", "Lõikepunkt"),
            (Kind::Edge, "On edge", "Serval"),
            (Kind::Perpendicular, "Perpendicular", "Ristipunkt"),
            (Kind::Tangent, "Tangent", "Puutuja"),
            (Kind::Angle, "Angle", "Nurk"),
            (Kind::Distance, "Distance", "Kaugus"),
            (Kind::SmartPoint, "Smart point", "Nutikas punkt"),
            (Kind::SmartEdge, "Smart edge", "Nutikas serv"),
        ];
        for (kind, en, et) in all {
            assert_eq!(kind_name(kind), en);
            assert_eq!(cue(snap_with(kind, None, None)), en);
            kadakas_l10n::with_language("et", || assert_eq!(kind_name(kind), et));
        }
        assert_eq!(
            cue(snap_with(Kind::Angle, Some(45.0), Some(300.0))),
            "Angle 45\u{b0}, length 300 mm"
        );
        assert_eq!(
            cue(snap_with(Kind::Angle, Some(45.5), None)),
            "Angle 45.5\u{b0}"
        );
        assert_eq!(
            cue(snap_with(Kind::Distance, None, Some(250.0))),
            "Distance 250 mm"
        );
        assert_eq!(
            cue(snap_with(Kind::SmartPoint, Some(90.0), None)),
            "Smart point + Angle 90\u{b0}"
        );
        // Estonian words, a decimal comma and the units of the person.
        let et = kadakas_units::Settings::default().resolve("et");
        kadakas_l10n::with_language("et", || {
            kadakas_units::with_units(et, || {
                assert_eq!(
                    cue(snap_with(Kind::Angle, Some(45.5), Some(300.0))),
                    "Nurk 45,5\u{b0}, pikkus 300 mm"
                );
                assert_eq!(
                    cue(snap_with(Kind::SmartEdge, Some(30.0), None)),
                    "Nutikas serv + nurk 30\u{b0}"
                );
            })
        });
    }

    #[test]
    fn snapping_uses_the_settings_and_the_smart_points() {
        let mut s = Session::default();
        add(
            &mut s,
            Shape::Line {
                a: DVec2::new(0.0, 0.0),
                b: DVec2::new(100.0, 0.0),
            },
        );
        let st = Settings::default();
        let r = snap_cursor(
            s.editor.doc(),
            DVec2::new(2.0, 3.0),
            10.0,
            50.0,
            &st,
            &Smart::default(),
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(
            (r.kind, r.point, r.cue.as_str()),
            (Kind::Endpoint, DVec2::ZERO, "Endpoint")
        );
        let mut smart = Smart::default();
        smart.acquire_point(DVec2::new(300.0, 400.0));
        let r = snap_cursor(
            s.editor.doc(),
            DVec2::new(303.0, 402.0),
            10.0,
            50.0,
            &Settings {
                set: snapping::SnapSet {
                    grid: false,
                    ..Default::default()
                },
                ..st
            },
            &smart,
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(r.kind, Kind::SmartPoint);
        assert_eq!(r.point, DVec2::new(300.0, 400.0));
    }

    #[test]
    fn the_t_key_sets_a_smart_point_near_a_corner_and_nothing_elsewhere() {
        let mut s = Session::default();
        add(
            &mut s,
            Shape::Line {
                a: DVec2::new(0.0, 0.0),
                b: DVec2::new(100.0, 0.0),
            },
        );
        let st = Settings::default();
        let mut smart = Smart::default();
        let doc = s.editor.doc();
        let msg = acquire_smart(doc, DVec2::new(100.0, 2.0), 10.0, &st, &mut smart);
        assert_eq!(msg, "Smart point set");
        assert_eq!(smart.points, vec![DVec2::new(100.0, 0.0)]);
        let msg = acquire_smart(doc, DVec2::new(5000.0, 5000.0), 10.0, &st, &mut smart);
        assert!(msg.starts_with("No object point"));
    }

    #[test]
    fn snap_points_of_lines_and_other_objects() {
        use kadakas_core::irrigation::LineType;
        let line = IrrigationShape::Line {
            points: vec![DVec2::ZERO, DVec2::new(1000.0, 0.0)],
            line_type: LineType::Lateral,
            size: "32".into(),
            material: "PE".into(),
            emitter_spacing: None,
            emitter_flow: None,
        };
        let poc = IrrigationShape::Poc {
            at: DVec2::ZERO,
            name: String::new(),
            static_pressure: 1.0,
            available_flow: 1.0,
            supply_loss: None,
            meter_loss: None,
        };
        let mut out = vec![];
        snap_points(&line, &mut out);
        snap_points(&poc, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].1, SnapKind::Center);
    }

    #[test]
    fn irrigation_objects_are_snap_points() {
        use kadakas_core::irrigation::{IrrigationShape, LineType};
        let mut s = Session::default();
        add(
            &mut s,
            Shape::Irrigation(Box::new(IrrigationShape::Line {
                points: vec![DVec2::new(0.0, 0.0), DVec2::new(1000.0, 0.0)],
                line_type: LineType::Lateral,
                size: "32".into(),
                material: "PE".into(),
                emitter_spacing: None,
                emitter_flow: None,
            })),
        );
        let g = geometry(s.editor.doc(), &[], &Settings::default(), None);
        assert_eq!(g.points.len(), 2);
        assert!(g.points.iter().all(|p| p.kind == Kind::Endpoint));
        let near = geometry(
            s.editor.doc(),
            &[],
            &Settings::default(),
            Some((DVec2::new(1000.0, 3.0), 10.0)),
        );
        assert_eq!(near.points.len(), 1);
    }
}
