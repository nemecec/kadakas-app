//! Site models in a document: from the objects to surfaces, and a cache of the results.
//!
//! The data in a `Shape::SiteModel` is the source. Everything derived (TIN, contours, volumes)
//! is computed here and kept in a cache by document revision. Nothing derived is saved.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use glam::{DVec2, DVec3};

use cadektor_core::curve::{self, Vertex};
use cadektor_core::model::{Document, ModifierKind, ObjectId, Shape, SiteSettings};
use cadektor_core::site::boundary_points;

use crate::SiteError;
use crate::contour::{Contour, ContourSpec, contours};
use crate::drape::drape;
use crate::modifier::{Modifier, PadSlope, proposed_surface};
use crate::overlay::crop;
use crate::tin::{SlopeAspect, SourceData, SourceLine, Tin};
use crate::volume::{CutFill, cut_fill};

pub use cadektor_core::ground::Surface;

/// The source data of a site model shape.
pub fn source_data(shape: &Shape) -> Option<SourceData> {
    match shape {
        Shape::SiteModel { points, lines, .. } => Some(SourceData {
            points: points.clone(),
            lines: lines
                .iter()
                .map(|l| SourceLine {
                    points: l.points.clone(),
                    closed: l.closed,
                })
                .collect(),
        }),
        _ => None,
    }
}

/// A modifier shape as plain geometry.
pub fn modifier_of(shape: &Shape) -> Option<Modifier> {
    match shape {
        Shape::SiteModifier {
            modifier: ModifierKind::Pad { elevation, slope },
            boundary,
        } => Some(Modifier::Pad {
            boundary: boundary_points(boundary),
            elevation: *elevation,
            slope: slope.map(|s| PadSlope {
                direction: s.direction,
                percent: s.percent,
            }),
        }),
        Shape::SiteModifier {
            modifier: ModifierKind::GradeLimit,
            boundary,
        } => Some(Modifier::GradeLimit {
            boundary: boundary_points(boundary),
        }),
        _ => None,
    }
}

/// The modifiers that act on a site model: visible modifiers (and hardscapes that act as pads) on the same layer whose box touches
/// the box of the site model. Returns their IDs in document order.
pub fn modifiers_for(doc: &Document, site: ObjectId) -> Vec<(ObjectId, Modifier)> {
    let Some(obj) = doc.object(site) else {
        return Vec::new();
    };
    let (lo, hi) = obj.shape.bounds();
    let mut list: Vec<(bool, ObjectId, Modifier)> = doc
        .objects
        .values()
        .filter(|o| o.layer == obj.layer && doc.is_visible(o))
        .flat_map(|o| {
            let (a, b) = o.shape.bounds();
            let touches = a.x <= hi.x && b.x >= lo.x && a.y <= hi.y && b.y >= lo.y;
            let mut mods: Vec<(bool, Modifier)> = Vec::new();
            if touches {
                let own = modifier_of(&o.shape).or_else(|| crate::terrain::pad_of(doc, o));
                mods.extend(own.map(|m| (false, m)));
                mods.extend(
                    crate::grading::grade::pads_of(o)
                        .into_iter()
                        .map(|m| (false, m)),
                );
                mods.extend(
                    crate::grading::wall::pads_of(doc, o)
                        .into_iter()
                        .map(|m| (true, m)),
                );
            }
            mods.into_iter().map(move |(w, m)| (w, o.id, m))
        })
        .collect();
    // The ground behind a retaining wall comes first, so that a pad of the user wins where they overlap.
    list.sort_by_key(|(wall, ..)| !*wall);
    list.into_iter().map(|(_, id, m)| (id, m)).collect()
}

/// The contour settings of a site model.
pub fn contour_spec(s: &SiteSettings) -> ContourSpec {
    ContourSpec {
        minor_interval: s.minor_interval,
        major_interval: s.major_interval,
        datum: s.datum,
        smooth: s.smooth_contours,
    }
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
    /// Hash of what the surfaces are made from (source data, modifiers, crop).
    pub geometry_hash: u64,
    /// Hash of the geometry and the settings.
    pub content_hash: u64,
    existing_contours: OnceLock<Result<Vec<Contour>, SiteError>>,
    proposed_contours: OnceLock<Result<Vec<Contour>, SiteError>>,
    volumes: OnceLock<CutFill>,
}

impl SiteResult {
    /// Builds the surfaces of site model `id`.
    pub fn compute(doc: &Document, id: ObjectId) -> Result<SiteResult, SiteError> {
        let obj = doc.object(id).ok_or(SiteError::NoObject(id))?;
        let src = source_data(&obj.shape).ok_or(SiteError::NotASiteModel(id))?;
        let Shape::SiteModel { settings, .. } = &obj.shape else {
            unreachable!("checked by source_data")
        };
        let mut existing = Tin::build(&src)?;
        if !settings.crop.is_empty() {
            existing = crop(&existing, &settings.crop)?;
        }
        let mods = modifiers_for(doc, id);
        let (geometry_hash, content_hash) = fingerprints(doc, id).unwrap_or_default();
        let pads = mods.iter().any(|(_, m)| matches!(m, Modifier::Pad { .. }));
        let proposed = if pads {
            let list: Vec<Modifier> = mods.iter().map(|(_, m)| m.clone()).collect();
            Some(proposed_surface(&existing, &list)?)
        } else {
            None
        };
        Ok(SiteResult {
            id,
            settings: settings.clone(),
            existing,
            proposed,
            modifiers: {
                // A wall makes many pads. It counts once.
                let mut ids: Vec<ObjectId> = mods.into_iter().map(|(i, _)| i).collect();
                ids.dedup();
                ids
            },
            geometry_hash,
            content_hash,
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
        cell.get_or_init(|| contours(tin, &contour_spec(&self.settings)))
            .as_deref()
            .map_err(|e| e.clone())
    }

    /// Cut and fill of the proposed surface against the existing one.
    pub fn cut_fill(&self) -> CutFill {
        *self.volumes.get_or_init(|| match &self.proposed {
            Some(p) => cut_fill(&self.existing, p),
            None => CutFill {
                area: self.existing.area_xy(),
                ..CutFill::default()
            },
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
pub fn drape_polyline(
    site: &SiteResult,
    vertices: &[Vertex],
    closed: bool,
) -> Result<Vec<DVec3>, SiteError> {
    let flat = curve::flatten(vertices, closed, 2.0);
    drape(site.surface(Surface::Proposed), &flat, closed)
}

/// Hashes of a site model: what its surfaces are made from, and that plus the settings.
/// Two documents with the same hashes give the same surfaces. `None` if `id` is not a site model.
pub fn fingerprints(doc: &Document, id: ObjectId) -> Option<(u64, u64)> {
    let obj = doc.object(id)?;
    let Shape::SiteModel {
        points,
        lines,
        settings,
    } = &obj.shape
    else {
        return None;
    };
    let mut h = DefaultHasher::new();
    let v3 = |h: &mut DefaultHasher, p: &DVec3| {
        [p.x, p.y, p.z]
            .iter()
            .for_each(|f| (f + 0.0).to_bits().hash(h))
    };
    points.iter().for_each(|p| v3(&mut h, p));
    for l in lines {
        l.closed.hash(&mut h);
        l.points.len().hash(&mut h);
        l.points.iter().for_each(|p| v3(&mut h, p));
    }
    format!("{:?}{:?}", modifiers_for(doc, id), settings.crop).hash(&mut h);
    let geometry = h.finish();
    let mut h = DefaultHasher::new();
    geometry.hash(&mut h);
    format!("{settings:?}").hash(&mut h);
    Some((geometry, h.finish()))
}

/// Results of site models. A result is kept while the content of its site model and modifiers
/// stays the same, so an edit of anything else in the document does not rebuild the terrain.
/// Failures are not kept.
#[derive(Default)]
pub struct SiteCache {
    inner: Mutex<CacheInner>,
}

struct Entry {
    result: Arc<SiteResult>,
    /// The last document revision that was checked against the result.
    revision: u64,
}

#[derive(Default)]
struct CacheInner {
    sites: HashMap<ObjectId, Entry>,
    /// How often a site model was computed.
    builds: u64,
}

impl SiteCache {
    /// The result for site model `id` in `doc`, which must be the document of `revision`.
    /// A site model with `auto_update` off keeps its old result when the geometry changes,
    /// until `update` is called. Use `is_stale` to ask.
    pub fn get(
        &self,
        revision: u64,
        doc: &Document,
        id: ObjectId,
    ) -> Result<Arc<SiteResult>, SiteError> {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = c.sites.get_mut(&id) {
            if e.revision == revision {
                return Ok(e.result.clone());
            }
            if let Some((geometry, content)) = fingerprints(doc, id) {
                let keep_old = !auto_update(doc, id) && e.result.geometry_hash != geometry;
                if e.result.content_hash == content || keep_old {
                    e.revision = revision;
                    return Ok(e.result.clone());
                }
            }
        }
        Self::build(&mut c, revision, doc, id)
    }

    fn build(
        c: &mut CacheInner,
        revision: u64,
        doc: &Document,
        id: ObjectId,
    ) -> Result<Arc<SiteResult>, SiteError> {
        let s = Arc::new(SiteResult::compute(doc, id)?);
        c.sites.insert(
            id,
            Entry {
                result: s.clone(),
                revision,
            },
        );
        c.builds += 1;
        Ok(s)
    }

    /// Rebuilds site model `id` now ("Update Site Model").
    pub fn update(
        &self,
        revision: u64,
        doc: &Document,
        id: ObjectId,
    ) -> Result<Arc<SiteResult>, SiteError> {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Self::build(&mut c, revision, doc, id)
    }

    /// True if the document has changed the geometry of site model `id` since its result was built
    /// and the site model does not update by itself.
    pub fn is_stale(&self, doc: &Document, id: ObjectId) -> bool {
        let c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match (c.sites.get(&id), fingerprints(doc, id)) {
            (Some(e), Some((geometry, _))) => {
                !auto_update(doc, id) && e.result.geometry_hash != geometry
            }
            _ => false,
        }
    }

    /// How many site models were computed so far.
    pub fn builds(&self) -> u64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).builds
    }
}

fn auto_update(doc: &Document, id: ObjectId) -> bool {
    matches!(doc.object(id).map(|o| &o.shape), Some(Shape::SiteModel { settings, .. }) if settings.auto_update)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::Attributes;

    pub fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
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
        Shape::SiteModel {
            points,
            lines: vec![],
            settings: SiteSettings::default(),
        }
    }

    fn sq(lo: f64, hi: f64) -> Vec<Vertex> {
        curve::corners(&[
            DVec2::new(lo, lo),
            DVec2::new(hi, lo),
            DVec2::new(hi, hi),
            DVec2::new(lo, hi),
        ])
    }

    #[test]
    fn result_without_modifiers_has_one_surface() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(5, 1000.0, |x, _| x / 10.0));
        let r = SiteResult::compute(ed.doc(), id).unwrap();
        assert!(r.proposed.is_none() && r.modifiers.is_empty());
        assert!((r.elevation_at(DVec2::new(2500.0, 100.0)).unwrap() - 250.0).abs() < 1e-9);
        assert_eq!(r.elevation_at(DVec2::new(-5.0, 0.0)), None);
        assert!(
            r.slope_at(Surface::Existing, DVec2::new(10.0, 10.0))
                .unwrap()
                .slope
                > 0.099
        );
        let c = r.cut_fill();
        assert_eq!((c.cut, c.fill), (0.0, 0.0));
        assert!((c.area - 16e6).abs() < 1e-3);
        // Contours of the proposed surface are those of the existing one.
        assert_eq!(
            r.contours(Surface::Proposed).unwrap().len(),
            r.contours(Surface::Existing).unwrap().len()
        );
        assert_eq!(elevation_at(&r, DVec2::new(0.0, 0.0)), Some(0.0));
    }

    #[test]
    fn modifiers_on_the_layer_change_the_proposed_surface() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(9, 1000.0, |_, _| 0.0));
        let pad = add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::Pad {
                    elevation: 500.0,
                    slope: None,
                },
                boundary: sq(2000.0, 4000.0),
            },
        );
        let r = SiteResult::compute(ed.doc(), id).unwrap();
        assert_eq!(r.modifiers, vec![pad]);
        assert!((r.elevation_at(DVec2::new(3000.0, 3000.0)).unwrap() - 500.0).abs() < 1e-9);
        assert!(
            (r.surface(Surface::Existing)
                .elevation_at(DVec2::new(3000.0, 3000.0))
                .unwrap())
            .abs()
                < 1e-9
        );
        let c = r.cut_fill();
        assert!(
            c.fill > 2000.0 * 2000.0 * 500.0 * 0.99 && c.cut.abs() < 1e-6,
            "{c:?}"
        );
        assert_eq!(r.cut_fill(), c);
        let _ = r.contours(Surface::Proposed).unwrap();
        assert!(r.contours(Surface::Proposed).is_ok());
        // A modifier on another layer, or far away, does nothing.
        ed.execute(&Command::AddLayer {
            name: "Other".into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        })
        .unwrap();
        let other = ed.doc().layers.last().unwrap().id;
        ed.execute(&Command::AddObject {
            shape: Shape::SiteModifier {
                modifier: ModifierKind::Pad {
                    elevation: 9.0,
                    slope: None,
                },
                boundary: sq(0.0, 1000.0),
            },
            layer: Some(other),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::GradeLimit,
                boundary: sq(1e6, 1e6 + 10.0),
            },
        );
        assert_eq!(modifiers_for(ed.doc(), id).len(), 1);
        add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::GradeLimit,
                boundary: sq(0.0, 9000.0),
            },
        );
        assert_eq!(modifiers_for(ed.doc(), id).len(), 2);
        assert!(modifiers_for(ed.doc(), ObjectId(9999)).is_empty());
    }

    #[test]
    fn errors_for_missing_and_wrong_objects() {
        let mut ed = Editor::default();
        let circle = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
        );
        assert!(matches!(
            SiteResult::compute(ed.doc(), circle),
            Err(SiteError::NotASiteModel(_))
        ));
        assert!(matches!(
            SiteResult::compute(ed.doc(), ObjectId(77)),
            Err(SiteError::NoObject(_))
        ));
        let few = add(
            &mut ed,
            Shape::SiteModel {
                points: vec![DVec3::ZERO],
                lines: vec![],
                settings: SiteSettings::default(),
            },
        );
        assert!(matches!(
            SiteResult::compute(ed.doc(), few),
            Err(SiteError::NoSurface)
        ));
        assert!(
            source_data(&Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0
            })
            .is_none()
        );
        assert!(
            modifier_of(&Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0
            })
            .is_none()
        );
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
        // An edit of something else does not rebuild the terrain.
        add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 5.0,
            },
        );
        let c = cache.get(ed.revision(), ed.doc(), id).unwrap();
        assert!(Arc::ptr_eq(&a, &c));
        assert_eq!(cache.builds(), 1);
        // An edit of a pad does.
        add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::Pad {
                    elevation: 9.0,
                    slope: None,
                },
                boundary: sq(500.0, 1500.0),
            },
        );
        let d = cache.get(ed.revision(), ed.doc(), id).unwrap();
        assert!(!Arc::ptr_eq(&a, &d));
        assert_eq!(cache.builds(), 2);
        assert!(cache.get(ed.revision(), ed.doc(), ObjectId(500)).is_err());
    }

    #[test]
    fn site_without_auto_update_goes_stale_until_updated() {
        let mut ed = Editor::default();
        let Shape::SiteModel {
            points,
            lines,
            mut settings,
        } = grid_site(5, 1000.0, |_, _| 0.0)
        else {
            unreachable!()
        };
        settings.auto_update = false;
        let id = add(
            &mut ed,
            Shape::SiteModel {
                points,
                lines,
                settings,
            },
        );
        let cache = SiteCache::default();
        let a = cache.get(ed.revision(), ed.doc(), id).unwrap();
        assert!(!cache.is_stale(ed.doc(), id));
        add(
            &mut ed,
            Shape::SiteModifier {
                modifier: ModifierKind::Pad {
                    elevation: 50.0,
                    slope: None,
                },
                boundary: sq(1000.0, 2000.0),
            },
        );
        assert!(cache.is_stale(ed.doc(), id));
        let b = cache.get(ed.revision(), ed.doc(), id).unwrap();
        assert!(Arc::ptr_eq(&a, &b) && b.proposed.is_none());
        let c = cache.update(ed.revision(), ed.doc(), id).unwrap();
        assert!(c.proposed.is_some() && !cache.is_stale(ed.doc(), id));
        assert!((c.elevation_at(DVec2::new(1500.0, 1500.0)).unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn crop_setting_cuts_the_site_model() {
        let mut ed = Editor::default();
        let Shape::SiteModel {
            points,
            lines,
            mut settings,
        } = grid_site(5, 1000.0, |_, _| 0.0)
        else {
            unreachable!()
        };
        settings.crop = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(4000.0, 0.0),
            DVec2::new(4000.0, 1000.0),
            DVec2::new(1000.0, 1000.0),
            DVec2::new(1000.0, 4000.0),
            DVec2::new(0.0, 4000.0),
        ];
        let id = add(
            &mut ed,
            Shape::SiteModel {
                points,
                lines,
                settings,
            },
        );
        let r = SiteResult::compute(ed.doc(), id).unwrap();
        assert!((r.existing.area_xy() - 7e6).abs() < 1e-3);
        assert!(r.elevation_at(DVec2::new(3000.0, 3000.0)).is_none());
    }

    #[test]
    fn drape_a_curved_boundary_on_a_ramp() {
        let mut ed = Editor::default();
        let id = add(&mut ed, grid_site(5, 1000.0, |x, _| x));
        let r = SiteResult::compute(ed.doc(), id).unwrap();
        let line = drape_polyline(&r, &sq(500.0, 3500.0), true).unwrap();
        assert!(line.len() >= 4);
        assert!(line.iter().all(|p| (p.z - p.x).abs() < 1e-9));
        assert!(matches!(
            drape_polyline(&r, &sq(500.0, 9000.0), true),
            Err(SiteError::Outside(_))
        ));
    }
}
