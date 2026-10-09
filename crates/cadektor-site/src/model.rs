//! Site models in a document: from the objects to surfaces, and a cache of the results.
//!
//! The data in a `Shape::SiteModel` is the source. Everything derived (TIN, contours, volumes)
//! is computed here and kept in a cache by document revision. Nothing derived is saved.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use glam::{DVec2, DVec3};

use cadektor_core::curve::{self, Vertex};
use cadektor_core::model::{Document, ModifierKind, ObjectId, Shape, SiteSettings};
use cadektor_core::site::boundary_points;

use crate::SiteError;
use crate::contour::{Contour, ContourSpec, contours};
use crate::drape::drape;
use crate::modifier::{Modifier, PadSlope, proposed_surface};
use crate::tin::{SlopeAspect, SourceData, SourceLine, Tin};
use crate::volume::{CutFill, cut_fill};

/// Which surface to ask about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Existing,
    Proposed,
}

/// The source data of a site model shape.
pub fn source_data(shape: &Shape) -> Option<SourceData> {
    match shape {
        Shape::SiteModel { points, lines, .. } => Some(SourceData {
            points: points.clone(),
            lines: lines.iter().map(|l| SourceLine { points: l.points.clone(), closed: l.closed }).collect(),
        }),
        _ => None,
    }
}

/// A modifier shape as plain geometry.
pub fn modifier_of(shape: &Shape) -> Option<Modifier> {
    match shape {
        Shape::SiteModifier { modifier: ModifierKind::Pad { elevation, slope }, boundary } => Some(Modifier::Pad {
            boundary: boundary_points(boundary),
            elevation: *elevation,
            slope: slope.map(|s| PadSlope { direction: s.direction, percent: s.percent }),
        }),
        Shape::SiteModifier { modifier: ModifierKind::GradeLimit, boundary } => Some(Modifier::GradeLimit { boundary: boundary_points(boundary) }),
        _ => None,
    }
}

/// The modifiers that act on a site model: visible modifiers on the same layer whose box touches
/// the box of the site model. Returns their IDs in document order.
pub fn modifiers_for(doc: &Document, site: ObjectId) -> Vec<(ObjectId, Modifier)> {
    let Some(obj) = doc.object(site) else { return Vec::new() };
    let (lo, hi) = obj.shape.bounds();
    doc.objects
        .values()
        .filter(|o| o.layer == obj.layer && doc.is_visible(o))
        .filter_map(|o| {
            let m = modifier_of(&o.shape)?;
            let (a, b) = o.shape.bounds();
            (a.x <= hi.x && b.x >= lo.x && a.y <= hi.y && b.y >= lo.y).then_some((o.id, m))
        })
        .collect()
}

/// The contour settings of a site model.
pub fn contour_spec(s: &SiteSettings) -> ContourSpec {
    ContourSpec { minor_interval: s.minor_interval, major_interval: s.major_interval, datum: s.datum, smooth: s.smooth_contours }
}

/// A site model with its derived surfaces.
pub struct SiteResult {
    pub id: ObjectId,
    pub settings: SiteSettings,
    pub existing: Tin,
    /// `None` when no modifier changes the surface. Then the proposed surface is the existing one.
    pub proposed: Option<Tin>,
    /// The modifiers that were applied.
    pub modifiers: Vec<ObjectId>,
    existing_contours: OnceLock<Result<Vec<Contour>, SiteError>>,
    proposed_contours: OnceLock<Result<Vec<Contour>, SiteError>>,
    volumes: OnceLock<CutFill>,
}

impl SiteResult {
    /// Builds the surfaces of site model `id`.
    pub fn compute(doc: &Document, id: ObjectId) -> Result<SiteResult, SiteError> {
        let obj = doc.object(id).ok_or(SiteError::NoObject(id))?;
        let src = source_data(&obj.shape).ok_or(SiteError::NotASiteModel(id))?;
        let Shape::SiteModel { settings, .. } = &obj.shape else { unreachable!("checked by source_data") };
        let existing = Tin::build(&src)?;
        let mods = modifiers_for(doc, id);
        let pads = mods.iter().any(|(_, m)| matches!(m, Modifier::Pad { .. }));
        let proposed = if pads {
            let list: Vec<Modifier> = mods.iter().map(|(_, m)| m.clone()).collect();
            Some(proposed_surface(&src, &existing, &list)?)
        } else {
            None
        };
        Ok(SiteResult {
            id,
            settings: settings.clone(),
            existing,
            proposed,
            modifiers: mods.into_iter().map(|(i, _)| i).collect(),
            existing_contours: OnceLock::new(),
            proposed_contours: OnceLock::new(),
            volumes: OnceLock::new(),
        })
    }

    /// The surface, where the proposed one is the existing one if nothing changes it.
    pub fn surface(&self, which: Surface) -> &Tin {
        match which {
            Surface::Existing => &self.existing,
            Surface::Proposed => self.proposed.as_ref().unwrap_or(&self.existing),
        }
    }

    /// Elevation of the proposed surface at a point (the ground after the work).
    pub fn elevation_at(&self, xy: DVec2) -> Option<f64> {
        self.surface(Surface::Proposed).elevation_at(xy)
    }

    pub fn slope_at(&self, which: Surface, xy: DVec2) -> Option<SlopeAspect> {
        self.surface(which).slope_at(xy)
    }

    /// Contours of a surface at the intervals of the settings.
    pub fn contours(&self, which: Surface) -> Result<&[Contour], SiteError> {
        let cell = match which {
            Surface::Existing => &self.existing_contours,
            Surface::Proposed if self.proposed.is_none() => &self.existing_contours,
            Surface::Proposed => &self.proposed_contours,
        };
        let tin = self.surface(which);
        cell.get_or_init(|| contours(tin, &contour_spec(&self.settings))).as_deref().map_err(|e| e.clone())
    }

    /// Cut and fill of the proposed surface against the existing one.
    pub fn cut_fill(&self) -> CutFill {
        *self.volumes.get_or_init(|| match &self.proposed {
            Some(p) => cut_fill(&self.existing, p),
            None => CutFill { area: self.existing.area_xy(), ..CutFill::default() },
        })
    }
}

/// Elevation of a site model's proposed surface at `xy`. Use it to put objects on the terrain.
pub fn elevation_at(site: &SiteResult, xy: DVec2) -> Option<f64> {
    site.elevation_at(xy)
}

/// Puts a polyline on the proposed surface of a site model, with extra points where it crosses
/// triangle edges. Curved vertices are flattened first. A closed line does not repeat its first
/// point. Fails with `SiteError::Outside` if a point is off the model.
pub fn drape_polyline(site: &SiteResult, vertices: &[Vertex], closed: bool) -> Result<Vec<DVec3>, SiteError> {
    let flat = curve::flatten(vertices, closed, 2.0);
    drape(site.surface(Surface::Proposed), &flat, closed)
}

/// Results by document revision. Revisions are unique across editors, so one cache can serve
/// many documents. Failures are not kept.
#[derive(Default)]
pub struct SiteCache {
    inner: Mutex<CacheInner>,
}

#[derive(Default)]
struct CacheInner {
    revision: Option<u64>,
    sites: HashMap<ObjectId, Arc<SiteResult>>,
    /// How often a site model was computed.
    builds: u64,
}

impl SiteCache {
    /// The result for site model `id` in `doc`, which must be the document of `revision`.
    pub fn get(&self, revision: u64, doc: &Document, id: ObjectId) -> Result<Arc<SiteResult>, SiteError> {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if c.revision != Some(revision) {
            c.revision = Some(revision);
            c.sites.clear();
        }
        if let Some(s) = c.sites.get(&id) {
            return Ok(s.clone());
        }
        let s = Arc::new(SiteResult::compute(doc, id)?);
        c.sites.insert(id, s.clone());
        c.builds += 1;
        Ok(s)
    }

    /// How many site models were computed so far.
    pub fn builds(&self) -> u64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).builds
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::Attributes;

    pub fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
        let out = ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).unwrap();
        ObjectId(out.created[0])
    }

    pub fn grid_site(n: usize, step: f64, f: impl Fn(f64, f64) -> f64) -> Shape {
        let mut points = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f64 * step, j as f64 * step);
                points.push(DVec3::new(x, y, f(x, y)));
            }
        }
        Shape::SiteModel { points, lines: vec![], settings: SiteSettings::default() }
    }

    fn sq(lo: f64, hi: f64) -> Vec<Vertex> {
        curve::corners(&[DVec2::new(lo, lo), DVec2::new(hi, lo), DVec2::new(hi, hi), DVec2::new(lo, hi)])
    }

    #[test]
    fn result_without_modifiers_has_one_surface() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(5, 1000.0, |x, _| x / 10.0));
        let r = SiteResult::compute(ed.doc(), id).unwrap();
        assert!(r.proposed.is_none() && r.modifiers.is_empty());
        assert!((r.elevation_at(DVec2::new(2500.0, 100.0)).unwrap() - 250.0).abs() < 1e-9);
        assert_eq!(r.elevation_at(DVec2::new(-5.0, 0.0)), None);
        assert!(r.slope_at(Surface::Existing, DVec2::new(10.0, 10.0)).unwrap().slope > 0.099);
        let c = r.cut_fill();
        assert_eq!((c.cut, c.fill), (0.0, 0.0));
        assert!((c.area - 16e6).abs() < 1e-3);
        // Contours of the proposed surface are those of the existing one.
        assert_eq!(r.contours(Surface::Proposed).unwrap().len(), r.contours(Surface::Existing).unwrap().len());
        assert_eq!(elevation_at(&r, DVec2::new(0.0, 0.0)), Some(0.0));
    }

    #[test]
    fn modifiers_on_the_layer_change_the_proposed_surface() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(9, 1000.0, |_, _| 0.0));
        let pad = add(&mut ed, Shape::SiteModifier { modifier: ModifierKind::Pad { elevation: 500.0, slope: None }, boundary: sq(2000.0, 4000.0) });
        let r = SiteResult::compute(ed.doc(), id).unwrap();
        assert_eq!(r.modifiers, vec![pad]);
        assert!((r.elevation_at(DVec2::new(3000.0, 3000.0)).unwrap() - 500.0).abs() < 1e-9);
        assert!((r.surface(Surface::Existing).elevation_at(DVec2::new(3000.0, 3000.0)).unwrap()).abs() < 1e-9);
        let c = r.cut_fill();
        assert!(c.fill > 2000.0 * 2000.0 * 500.0 * 0.99 && c.cut.abs() < 1e-6, "{c:?}");
        assert_eq!(r.cut_fill(), c);
        let _ = r.contours(Surface::Proposed).unwrap();
        assert!(r.contours(Surface::Proposed).is_ok());
        // A modifier on another layer, or far away, does nothing.
        ed.execute(&Command::AddLayer { name: "Other".into(), elevation: 0.0, scale: None, kind: None }).unwrap();
        let other = ed.doc().layers.last().unwrap().id;
        ed.execute(&Command::AddObject {
            shape: Shape::SiteModifier { modifier: ModifierKind::Pad { elevation: 9.0, slope: None }, boundary: sq(0.0, 1000.0) },
            layer: Some(other),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        add(&mut ed, Shape::SiteModifier { modifier: ModifierKind::GradeLimit, boundary: sq(1e6, 1e6 + 10.0) });
        assert_eq!(modifiers_for(ed.doc(), id).len(), 1);
        add(&mut ed, Shape::SiteModifier { modifier: ModifierKind::GradeLimit, boundary: sq(0.0, 9000.0) });
        assert_eq!(modifiers_for(ed.doc(), id).len(), 2);
        assert!(modifiers_for(ed.doc(), ObjectId(9999)).is_empty());
    }

    #[test]
    fn errors_for_missing_and_wrong_objects() {
        let mut ed = Editor::default();
        let circle = add(&mut ed, Shape::Circle { center: DVec2::ZERO, radius: 5.0 });
        assert!(matches!(SiteResult::compute(ed.doc(), circle), Err(SiteError::NotASiteModel(_))));
        assert!(matches!(SiteResult::compute(ed.doc(), ObjectId(77)), Err(SiteError::NoObject(_))));
        let few = add(&mut ed, Shape::SiteModel { points: vec![DVec3::ZERO], lines: vec![], settings: SiteSettings::default() });
        assert!(matches!(SiteResult::compute(ed.doc(), few), Err(SiteError::NoSurface)));
        assert!(source_data(&Shape::Circle { center: DVec2::ZERO, radius: 1.0 }).is_none());
        assert!(modifier_of(&Shape::Circle { center: DVec2::ZERO, radius: 1.0 }).is_none());
    }

    #[test]
    fn cache_reuses_results_per_revision() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(4, 1000.0, |_, _| 3.0));
        let cache = SiteCache::default();
        let a = cache.get(ed.revision(), ed.doc(), id).unwrap();
        let b = cache.get(ed.revision(), ed.doc(), id).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.builds(), 1);
        add(&mut ed, Shape::Circle { center: DVec2::ZERO, radius: 5.0 });
        let c = cache.get(ed.revision(), ed.doc(), id).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(cache.builds(), 2);
        assert!(cache.get(ed.revision(), ed.doc(), ObjectId(500)).is_err());
    }

    #[test]
    fn drape_a_curved_boundary_on_a_ramp() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(5, 1000.0, |x, _| x));
        let r = SiteResult::compute(ed.doc(), id).unwrap();
        let line = drape_polyline(&r, &sq(500.0, 3500.0), true).unwrap();
        assert!(line.len() >= 4);
        assert!(line.iter().all(|p| (p.z - p.x).abs() < 1e-9));
        assert!(matches!(drape_polyline(&r, &sq(500.0, 9000.0), true), Err(SiteError::Outside(_))));
    }
}
