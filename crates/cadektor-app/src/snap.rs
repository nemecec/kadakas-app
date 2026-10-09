//! Snapping math: object vertices and centers first, then the grid.

use cadektor_core::model::{Document, ObjectId, Shape};
use glam::DVec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    Grid,
    Vertex,
    Center,
}

impl SnapKind {
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Grid => "Grid",
            SnapKind::Vertex => "Vertex",
            SnapKind::Center => "Center",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapResult {
    pub point: DVec2,
    pub kind: SnapKind,
}

#[derive(Clone, Copy, Debug)]
pub struct SnapConfig {
    pub grid: bool,
    pub objects: bool,
}

impl Default for SnapConfig {
    fn default() -> Self {
        SnapConfig { grid: true, objects: true }
    }
}

pub type SnapPoint = (DVec2, SnapKind);

pub fn shape_snap_points(shape: &Shape, out: &mut Vec<SnapPoint>) {
    use SnapKind::*;
    match shape {
        Shape::Line { a, b } => out.extend([(*a, Vertex), (*b, Vertex)]),
        Shape::Rect { min, max } => out.extend([
            (*min, Vertex),
            (DVec2::new(max.x, min.y), Vertex),
            (*max, Vertex),
            (DVec2::new(min.x, max.y), Vertex),
            ((*min + *max) / 2.0, Center),
        ]),
        Shape::Circle { center, radius } => {
            out.push((*center, Center));
            for d in [DVec2::X, DVec2::Y, -DVec2::X, -DVec2::Y] {
                out.push((*center + d * *radius, Vertex));
            }
        }
        Shape::Arc { center, radius, start, end } => out.extend([
            (*center, Center),
            (*center + DVec2::from_angle(*start) * *radius, Vertex),
            (*center + DVec2::from_angle(*end) * *radius, Vertex),
        ]),
        Shape::Polyline { points, .. }
        | Shape::LandscapeArea { boundary: points, .. }
        | Shape::SiteModifier { boundary: points, .. }
        | Shape::Hardscape { boundary: points, .. } => out.extend(points.iter().map(|v| (v.p, Vertex))),
        Shape::SiteModel { points, lines, .. } => out.extend(points.iter().chain(lines.iter().flat_map(|l| &l.points)).take(5000).map(|p| (p.truncate(), Vertex))),
        Shape::Locus3d { p } => out.push((p.truncate(), Vertex)),
        Shape::Polyline3d { points, .. } => out.extend(points.iter().map(|p| (p.truncate(), Vertex))),
        Shape::Text { at, .. } => out.push((*at, Vertex)),
        Shape::Plant { at, .. } => out.push((*at, Center)),
    }
}

/// Snap candidates of all visible objects, except those in `exclude`.
pub fn snap_points(doc: &Document, exclude: &[ObjectId]) -> Vec<SnapPoint> {
    let mut out = Vec::new();
    for o in doc.objects.values().filter(|o| doc.is_visible(o) && !exclude.contains(&o.id)) {
        shape_snap_points(&o.shape, &mut out);
    }
    out
}

pub fn snap_to_grid(p: DVec2, step: f64) -> DVec2 {
    (p / step).round() * step
}

/// Snaps `cursor`. An object point within `tol` wins over the grid.
/// The grid always applies when it is on.
pub fn snap(cursor: DVec2, tol: f64, grid_step: f64, cfg: SnapConfig, pts: &[SnapPoint]) -> Option<SnapResult> {
    if cfg.objects {
        let best = pts
            .iter()
            .map(|(p, k)| (p.distance(cursor), *p, *k))
            .filter(|(d, _, _)| *d <= tol)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, point, kind)) = best {
            return Some(SnapResult { point, kind });
        }
    }
    cfg.grid.then(|| SnapResult { point: snap_to_grid(cursor, grid_step), kind: SnapKind::Grid })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_api::Session;
    use cadektor_core::command::Command;
    use cadektor_core::model::Attributes;

    fn add(s: &mut Session, shape: Shape) -> ObjectId {
        ObjectId(
            s.editor
                .execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() })
                .unwrap()
                .created[0],
        )
    }

    #[test]
    fn grid_rounding() {
        assert_eq!(snap_to_grid(DVec2::new(124.0, -76.0), 50.0), DVec2::new(100.0, -100.0));
    }

    #[test]
    fn vertex_beats_grid() {
        let pts = vec![(DVec2::new(103.0, 98.0), SnapKind::Vertex)];
        let r = snap(DVec2::new(100.0, 100.0), 10.0, 50.0, SnapConfig::default(), &pts).unwrap();
        assert_eq!(r.kind, SnapKind::Vertex);
        assert_eq!(r.point, DVec2::new(103.0, 98.0));
    }

    #[test]
    fn far_vertex_falls_back_to_grid_or_none() {
        let pts = vec![(DVec2::new(500.0, 500.0), SnapKind::Vertex)];
        let r = snap(DVec2::new(101.0, 99.0), 10.0, 50.0, SnapConfig::default(), &pts).unwrap();
        assert_eq!((r.kind, r.point), (SnapKind::Grid, DVec2::new(100.0, 100.0)));
        let off = SnapConfig { grid: false, objects: true };
        assert!(snap(DVec2::new(101.0, 99.0), 10.0, 50.0, off, &pts).is_none());
    }

    #[test]
    fn nearest_wins() {
        let pts = vec![(DVec2::new(0.0, 8.0), SnapKind::Vertex), (DVec2::new(0.0, 3.0), SnapKind::Center)];
        let r = snap(DVec2::ZERO, 10.0, 50.0, SnapConfig::default(), &pts).unwrap();
        assert_eq!(r.kind, SnapKind::Center);
    }

    #[test]
    fn document_points_and_exclusion() {
        let mut s = Session::default();
        let a = add(&mut s, Shape::Rect { min: DVec2::ZERO, max: DVec2::new(100.0, 50.0) });
        add(&mut s, Shape::Plant { at: DVec2::new(5.0, 5.0), species: "x".into(), spread: 10.0, height: 10.0 });
        let all = snap_points(s.editor.doc(), &[]);
        assert_eq!(all.len(), 6);
        assert!(all.contains(&(DVec2::new(50.0, 25.0), SnapKind::Center)));
        assert_eq!(snap_points(s.editor.doc(), &[a]).len(), 1);
    }
}
