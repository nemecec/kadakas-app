//! Snapping in the app: the pure math is in `kadakas_core::snapping`. This module builds the
//! geometry that the pointer can snap to (the irrigation objects add their own points) and draws
//! the marks, cue labels and guides.

use glam::DVec2;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::snapping::{self, Context, Geometry, Guide, Kind, Reach, Settings, Smart};

/// What an irrigation object offers to snap to. `Vertex` is an end or corner, `Center` a center.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Vertex,
    Center,
}

pub type SnapPoint = (DVec2, SnapKind);

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
            cue: s.cue,
            guides: s.guides,
        }
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
        crate::irrigation_ui::snap_points(i, &mut pts);
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

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_api::Session;
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
