//! Grading and site analysis objects: landscape walls, grade objects, spot elevations, site
//! sections and the slope and elevation analysis of site models.
//!
//! These objects keep their plain shape (a polyline, a line or a circle) and their settings in
//! `Object::data`, like landscape areas do. The shape moves, snaps and exports as before. This
//! module reads the settings, asks the site model for the ground and draws the extras through the
//! extension hooks of core (see `hook`).

pub mod analysis;
pub mod draw;
pub mod grade;
pub mod hook;
pub mod section;
pub mod spot;
pub mod wall;

use std::cell::OnceCell;
use std::sync::Arc;

use glam::DVec2;
use serde::de::DeserializeOwned;

use kadakas_core::ground::{GroundSource, Surface};
use kadakas_core::model::{Document, Object, ObjectId, Shape};

use crate::model::{SiteCache, SiteResult};

/// Which surface of the site model an object follows.
pub type Follow = Surface;

/// The ground of an object: a source of heights.
pub type Ground<'a> = &'a dyn GroundSource;

/// The ground of one site model, with the heights of its edge for places outside it.
pub struct SiteGround<'a> {
    site: &'a SiteResult,
    edges: [OnceCell<Vec<[glam::DVec3; 2]>>; 2],
}

impl<'a> SiteGround<'a> {
    pub fn new(site: &'a SiteResult) -> Self {
        SiteGround {
            site,
            edges: [OnceCell::new(), OnceCell::new()],
        }
    }
}

impl GroundSource for SiteGround<'_> {
    fn exact(&self, _obj: Option<&Object>, p: DVec2, which: Follow) -> Option<f64> {
        self.site.surface(which).elevation_at(p)
    }

    fn clamped(&self, obj: Option<&Object>, p: DVec2, which: Follow) -> Option<f64> {
        if let Some(z) = self.exact(obj, p, which) {
            return Some(z);
        }
        let tin = self.site.surface(which);
        let edges = self.edges[which as usize].get_or_init(|| boundary_edges(tin));
        nearest_edge_height(edges, p).map(|(_, z)| z)
    }
}

/// The edges of the boundary of a surface, with their heights.
pub(crate) fn boundary_edges(tin: &crate::tin::Tin) -> Vec<[glam::DVec3; 2]> {
    tin.boundary_edges()
        .into_iter()
        .map(|(a, b)| [tin.vertices[a as usize], tin.vertices[b as usize]])
        .collect()
}

/// The squared distance to the nearest point of the edges, and the height of the edge there.
pub(crate) fn nearest_edge_height(edges: &[[glam::DVec3; 2]], p: DVec2) -> Option<(f64, f64)> {
    edges
        .iter()
        .map(|[a, b]| {
            let ab = (b - a).truncate();
            let t = if ab.length_squared() == 0.0 {
                0.0
            } else {
                ((p - a.truncate()).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
            };
            (
                p.distance_squared(a.truncate() + ab * t),
                a.z + (b.z - a.z) * t,
            )
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
}

/// The settings under `key` of an object, if present and valid.
pub fn read<T: DeserializeOwned>(obj: &Object, key: &str) -> Option<T> {
    serde_json::from_value(obj.data.get(key)?.clone()).ok()
}

/// The site model that serves a layer: the one named, or the first visible site model on the
/// layer whose surface can be built.
pub fn host(
    doc: &Document,
    cache: &SiteCache,
    revision: u64,
    layer: kadakas_core::model::LayerId,
    named: Option<ObjectId>,
) -> Option<Arc<SiteResult>> {
    if let Some(id) = named {
        return cache.get(revision, doc, id).ok();
    }
    let visible = |o: &&Object| matches!(o.shape, Shape::SiteModel { .. }) && doc.is_visible(o);
    // The layer of the object first, then any layer: the ground does not stop at a layer.
    doc.objects
        .values()
        .filter(visible)
        .filter(|o| o.layer == layer)
        .chain(doc.objects.values().filter(visible))
        .find_map(|o| cache.get(revision, doc, o.id).ok())
}

/// Like [`host`], for an object at a place: the named site model, else the one that covers `at`
/// (on the layer of the object first, then on any layer), else the one that [`host`] finds.
pub fn host_near(
    doc: &Document,
    cache: &SiteCache,
    revision: u64,
    layer: kadakas_core::model::LayerId,
    named: Option<ObjectId>,
    at: DVec2,
) -> Option<Arc<SiteResult>> {
    if named.is_none() {
        let terrain = crate::terrain::SiteTerrain::new(cache, revision, doc);
        let probe = Object::new(
            ObjectId::from_num(0),
            layer,
            kadakas_core::model::NONE_CLASS,
            Shape::Locus3d {
                p: glam::DVec3::ZERO,
            },
        );
        if let Some((_, result)) = terrain.host_at(Some(&probe), at) {
            return Some(result);
        }
    }
    host(doc, cache, revision, layer, named)
}

/// Checks that a number is finite.
pub fn finite(name: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(format!("{name} must be a finite number"))
    }
}

/// Meters text for an elevation or length in mm, with `decimals` places.
pub fn meters(mm: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, mm / 1000.0)
}

/// Escapes text for SVG.
pub(crate) fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::curve;
    use kadakas_core::model::Attributes;
    use std::collections::BTreeMap;

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

    /// Adds a shape with data under one key.
    pub fn add_with(ed: &mut Editor, shape: Shape, key: &str, data: serde_json::Value) -> ObjectId {
        let id = add(ed, shape);
        ed.execute(&Command::SetData {
            id,
            set: BTreeMap::from([(key.to_string(), data)]),
            remove: vec![],
        })
        .unwrap();
        id
    }

    pub fn path(pts: &[(f64, f64)]) -> Shape {
        Shape::Polyline {
            points: curve::corners(&pts.iter().map(|p| DVec2::new(p.0, p.1)).collect::<Vec<_>>()),
            closed: false,
        }
    }

    /// A 10 by 10 m site of 1 m steps with the elevation `f(x, y)`.
    pub fn site(ed: &mut Editor, f: impl Fn(f64, f64) -> f64) -> ObjectId {
        add(ed, crate::model::tests::grid_site(11, 1000.0, f))
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use kadakas_core::command::Editor;

    #[test]
    fn follow_and_small_helpers() {
        assert_eq!(
            (Follow::Existing.name(), Follow::Proposed.name()),
            ("existing", "proposed")
        );
        assert_eq!(
            (Follow::Existing, Follow::Proposed),
            (Surface::Existing, Surface::Proposed)
        );
        assert!(finite("x", 1.0).is_ok() && finite("x", f64::NAN).unwrap_err().contains('x'));
        assert_eq!(meters(1234.0, 1), "1.2");
        assert_eq!(esc("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }

    #[test]
    fn the_host_is_named_or_found_on_the_layer() {
        let mut ed = Editor::default();
        let circle = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0,
            },
        );
        let cache = SiteCache::default();
        let obj = ed.doc().object(circle).unwrap();
        assert!(host(ed.doc(), &cache, ed.revision(), obj.layer, None).is_none());
        let s = site(&mut ed, |x, _| x);
        let obj = ed.doc().object(circle).unwrap();
        let found = host(ed.doc(), &cache, ed.revision(), obj.layer, None).unwrap();
        assert_eq!(found.id, s);
        assert_eq!(
            host(ed.doc(), &cache, ed.revision(), obj.layer, Some(s))
                .unwrap()
                .id,
            s
        );
        assert!(host(ed.doc(), &cache, ed.revision(), obj.layer, Some(circle)).is_none());
        let g = SiteGround::new(&found);
        assert_eq!(
            g.exact(None, DVec2::new(5000.0, 5000.0), Follow::Proposed),
            Some(5000.0)
        );
        assert_eq!(g.exact(None, DVec2::new(-5.0, 0.0), Follow::Existing), None);
        // Outside the model the ground has the height of the nearest edge. The edge list is made once.
        assert_eq!(
            g.clamped(None, DVec2::new(-500.0, 3000.0), Follow::Existing),
            Some(0.0)
        );
        assert_eq!(
            g.clamped(None, DVec2::new(20_000.0, 3000.0), Follow::Existing),
            Some(10_000.0)
        );
        assert_eq!(
            g.clamped(None, DVec2::new(20_000.0, 3000.0), Follow::Proposed),
            Some(10_000.0)
        );
        assert_eq!(
            g.clamped(None, DVec2::new(5000.0, 5000.0), Follow::Proposed),
            Some(5000.0)
        );
        // A closure is a ground without an edge.
        let flat = |_: DVec2, _: Follow| Some(7.0);
        assert_eq!(flat.clamped(None, DVec2::ZERO, Follow::Existing), Some(7.0));
        assert!(read::<wall::WallData>(ed.doc().object(circle).unwrap(), "nothing").is_none());
    }
}
