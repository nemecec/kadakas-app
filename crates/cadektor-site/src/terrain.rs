//! The site model as the ground under hardscapes and landscape areas.
//!
//! Core knows hardscapes and landscape areas but not terrain. It asks for the ground through
//! `GroundSource`, and this module answers with the site models on the layer of the object.
//! A hardscape can also act as a pad on the site model (see `pad_of`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use glam::{DVec2, DVec3};

use cadektor_core::ground::GroundSource;
use cadektor_core::hardscape;
use cadektor_core::model::{Document, LayerId, Object, Shape};
use cadektor_core::site::boundary_points;

use crate::model::{SiteCache, SiteResult, Surface};
use crate::modifier::{Modifier, PadSlope};

/// The ground of a document, from its site models. A site model serves the objects on its layer.
pub struct SiteTerrain<'a> {
    cache: &'a SiteCache,
    revision: u64,
    doc: &'a Document,
    hosts: RefCell<HashMap<LayerId, Vec<Arc<Host>>>>,
}

/// A site model with the edge of its surfaces, for points outside the model.
struct Host {
    result: Arc<SiteResult>,
    edges: [OnceLock<Vec<[DVec3; 2]>>; 2],
}

impl Host {
    fn new(result: Arc<SiteResult>) -> Self {
        Host {
            result,
            edges: [OnceLock::new(), OnceLock::new()],
        }
    }

    /// The height of the model at the nearest point of its edge, with the squared distance.
    fn edge_height(&self, p: DVec2, which: Surface) -> Option<(f64, f64)> {
        let edges = self.edges[which as usize]
            .get_or_init(|| crate::grading::boundary_edges(self.result.surface(which)));
        crate::grading::nearest_edge_height(edges, p)
    }
}

impl<'a> SiteTerrain<'a> {
    /// `revision` is the revision of `doc`.
    pub fn new(cache: &'a SiteCache, revision: u64, doc: &'a Document) -> Self {
        SiteTerrain {
            cache,
            revision,
            doc,
            hosts: RefCell::new(HashMap::new()),
        }
    }

    fn hosts(&self, layer: LayerId) -> Vec<Arc<Host>> {
        self.hosts
            .borrow_mut()
            .entry(layer)
            .or_insert_with(|| {
                self.doc
                    .objects
                    .values()
                    .filter(|o| {
                        o.layer == layer
                            && matches!(o.shape, Shape::SiteModel { .. })
                            && self.doc.is_visible(o)
                    })
                    .filter_map(|o| self.cache.get(self.revision, self.doc, o.id).ok())
                    .map(|r| Arc::new(Host::new(r)))
                    .collect()
            })
            .clone()
    }
}

impl SiteTerrain<'_> {
    /// The height of the base of a massing model above its layer, in mm: the lowest ground height
    /// under the footprint, so the building stands on the ground and never floats. A base that the
    /// model gives itself wins. Without a site model on the layer the base is 0.
    pub fn massing_base(&self, obj: &Object, m: &cadektor_core::massing::Massing) -> f64 {
        if let Some(b) = m.base {
            return b;
        }
        let ring = m.ring();
        let step = (ring.len() / 64).max(1);
        let mut samples: Vec<DVec2> = ring.iter().step_by(step).copied().collect();
        if !ring.is_empty() {
            samples.push(ring.iter().copied().sum::<DVec2>() / ring.len() as f64);
        }
        let on_ground = samples
            .iter()
            .filter(|p| self.covered(obj, **p))
            .filter_map(|p| self.elevation(obj, *p))
            .reduce(f64::min);
        on_ground
            .or_else(|| {
                samples
                    .iter()
                    .filter_map(|p| self.elevation(obj, *p))
                    .reduce(f64::min)
            })
            .unwrap_or(0.0)
    }
}

impl GroundSource for SiteTerrain<'_> {
    fn exact(&self, obj: Option<&Object>, p: DVec2, which: Surface) -> Option<f64> {
        self.hosts(obj?.layer)
            .iter()
            .find_map(|h| h.result.surface(which).elevation_at(p))
    }

    /// The height of the ground. Outside every site model on the layer it is the height of the
    /// nearest edge of the nearest model, so a hardscape that reaches over the edge stays level
    /// with the edge there.
    fn clamped(&self, obj: Option<&Object>, p: DVec2, which: Surface) -> Option<f64> {
        if let Some(z) = self.exact(obj, p, which) {
            return Some(z);
        }
        self.hosts(obj?.layer)
            .iter()
            .filter_map(|h| h.edge_height(p, which))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, z)| z)
    }
}

/// The pad that a hardscape makes on the site model, if it asks to ("Site modifier"). The pad is
/// the formation level: the top surface minus the whole build-up. A hardscape that drapes on
/// the site model makes no pad.
pub fn pad_of(doc: &Document, obj: &Object) -> Option<Modifier> {
    let Shape::Hardscape {
        boundary,
        thickness,
        ..
    } = &obj.shape
    else {
        return None;
    };
    let d = hardscape::data(obj).filter(|d| d.site_modifier && !d.drape)?;
    let boundary = boundary_points(boundary);
    if boundary.len() < 3 {
        return None;
    }
    let depth = match hardscape::data::style_of(doc, obj) {
        Some((_, s)) => s.total_thickness(),
        None => thickness
            .filter(|t| t.is_finite() && *t > 0.0)
            .unwrap_or(cadektor_core::mesh::DEFAULT_SLAB_THICKNESS),
    };
    let plane = hardscape::surface::surface_of(obj, Some(&d), None).plane;
    let mid = boundary.iter().copied().sum::<DVec2>() / boundary.len() as f64;
    let grade = DVec2::new(plane.a, plane.b);
    let slope = (grade.length() > 1e-12).then(|| PadSlope {
        direction: (-grade).to_angle(),
        percent: grade.length() * 100.0,
    });
    Some(Modifier::Pad {
        boundary,
        elevation: plane.z(mid) - depth,
        slope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{add, grid_site};
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::curve;
    use cadektor_core::hardscape::HardscapePatch;
    use cadektor_core::model::{Attributes, ObjectId};

    fn slab(ed: &mut Editor, lo: f64, hi: f64, patch: HardscapePatch) -> ObjectId {
        let b = curve::corners(&[
            DVec2::new(lo, lo),
            DVec2::new(hi, lo),
            DVec2::new(hi, hi),
            DVec2::new(lo, hi),
        ]);
        let shape = Shape::Hardscape {
            boundary: b,
            material: "m".into(),
            thickness: Some(200.0),
            elevation: Some(100.0),
        };
        let id = ObjectId(
            ed.execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        if !patch.is_empty() {
            ed.execute(&Command::SetHardscape {
                ids: vec![id],
                patch,
            })
            .unwrap();
        }
        id
    }

    #[test]
    fn terrain_answers_for_objects_on_the_layer() {
        let mut ed = Editor::default();
        add(&mut ed, grid_site(9, 1000.0, |x, _| x / 10.0));
        let id = slab(&mut ed, 1000.0, 2000.0, HardscapePatch::default());
        let cache = SiteCache::default();
        let t = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        let o = ed.doc().object(id).unwrap();
        assert_eq!(t.elevation(o, DVec2::new(3000.0, 3000.0)), Some(300.0));
        // Outside the model the height of the nearest edge counts (the ramp ends at x = 8000: z = 800).
        assert_eq!(t.elevation(o, DVec2::new(30_000.0, 0.0)), Some(800.0));
        assert!(
            t.covered(o, DVec2::new(3000.0, 3000.0)) && !t.covered(o, DVec2::new(30_000.0, 0.0))
        );
        // Another layer has no ground.
        let mut other = o.clone();
        other.layer = LayerId(77);
        assert_eq!(t.elevation(&other, DVec2::new(3000.0, 3000.0)), None);
    }

    #[test]
    fn a_hardscape_pad_changes_the_proposed_surface() {
        let mut ed = Editor::default();
        let site = add(&mut ed, grid_site(9, 1000.0, |x, _| x / 10.0));
        let id = slab(&mut ed, 2000.0, 4000.0, HardscapePatch::default());
        let r = SiteResult::compute(ed.doc(), site).unwrap();
        assert!(r.proposed.is_none(), "no pad without the option");
        ed.execute(&Command::SetHardscape {
            ids: vec![id],
            patch: HardscapePatch {
                site_modifier: Some(true),
                ..Default::default()
            },
        })
        .unwrap();
        let r = SiteResult::compute(ed.doc(), site).unwrap();
        // Top at 100 mm, 200 mm thick: formation level at -100 mm.
        assert!((r.elevation_at(DVec2::new(3000.0, 3000.0)).unwrap() + 100.0).abs() < 1e-6);
        // The ground outside is unchanged.
        assert!((r.elevation_at(DVec2::new(6000.0, 3000.0)).unwrap() - 600.0).abs() < 1e-6);
        assert!(r.modifiers.contains(&id));
        // A sloped hardscape makes a sloped pad.
        let patch = HardscapePatch {
            slope: Some(cadektor_core::hardscape::Slope::Cross {
                percent: 10.0,
                angle_deg: 0.0,
            }),
            ..Default::default()
        };
        ed.execute(&Command::SetHardscape {
            ids: vec![id],
            patch,
        })
        .unwrap();
        let r = SiteResult::compute(ed.doc(), site).unwrap();
        let (a, b) = (
            r.elevation_at(DVec2::new(2500.0, 3000.0)).unwrap(),
            r.elevation_at(DVec2::new(3500.0, 3000.0)).unwrap(),
        );
        assert!(
            (a - b - 100.0).abs() < 1e-6,
            "falls 10 percent over 1 m: {a} {b}"
        );
        // Draping switches the pad off.
        ed.execute(&Command::SetHardscape {
            ids: vec![id],
            patch: HardscapePatch {
                drape: Some(true),
                ..Default::default()
            },
        })
        .unwrap();
        assert!(
            SiteResult::compute(ed.doc(), site)
                .unwrap()
                .proposed
                .is_none()
        );
        assert!(pad_of(ed.doc(), ed.doc().object(id).unwrap()).is_none());
    }

    #[test]
    fn pad_of_uses_the_style_thickness_and_ignores_other_objects() {
        let mut ed = Editor::default();
        let style = hardscape::HardscapeStyle {
            thickness: 50.0,
            components: vec![hardscape::Component {
                thickness: 250.0,
                ..Default::default()
            }],
            ..Default::default()
        };
        let sid = cadektor_core::resource::ResourceId(
            ed.execute(&Command::AddResource {
                name: "S".into(),
                body: cadektor_core::resource::ResourceBody::HardscapeStyle(Box::new(style)),
            })
            .unwrap()
            .created[0],
        );
        let id = slab(
            &mut ed,
            0.0,
            1000.0,
            HardscapePatch {
                style: Some(sid),
                site_modifier: Some(true),
                ..Default::default()
            },
        );
        let Some(Modifier::Pad {
            elevation, slope, ..
        }) = pad_of(ed.doc(), ed.doc().object(id).unwrap())
        else {
            panic!()
        };
        assert!((elevation - (100.0 - 300.0)).abs() < 1e-9 && slope.is_none());
        let line = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        assert!(pad_of(ed.doc(), ed.doc().object(line).unwrap()).is_none());
        // A hardscape without the option makes no pad.
        let plain = slab(
            &mut ed,
            0.0,
            10.0,
            HardscapePatch {
                drape: Some(false),
                ..Default::default()
            },
        );
        assert!(pad_of(ed.doc(), ed.doc().object(plain).unwrap()).is_none());
        // Without a style, the shape thickness counts.
        let plain2 = slab(
            &mut ed,
            0.0,
            10.0,
            HardscapePatch {
                site_modifier: Some(true),
                ..Default::default()
            },
        );
        let Some(Modifier::Pad { elevation, .. }) =
            pad_of(ed.doc(), ed.doc().object(plain2).unwrap())
        else {
            panic!()
        };
        assert!((elevation - (100.0 - 200.0)).abs() < 1e-9);
    }
}
