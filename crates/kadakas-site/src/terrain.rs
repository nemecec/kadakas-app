//! The site model as the ground under hardscapes, landscape areas, massing models and plants.
//!
//! Core knows these objects but not terrain. It asks for the ground through `GroundSource`, and
//! this module answers with the site models of the document.
//!
//! **Which site model.** An object that stands on the ground uses the site model that covers its
//! position, on any design layer. If the object (or its layer) names a site model (`site_model`
//! in its data, or the command `SetLayerSite`), only that model counts. Where several models
//! cover a position, the one on the layer of the object wins, then the first in the drawing.
//! Outside every model the height of the nearest edge counts (of the named model, else of the
//! models on the layer of the object, else of all models); a document without a site model has no
//! ground, and the callers say so.
//!
//! **Heights and layers.** A site model has its heights above the elevation of its own layer. The
//! answers of `GroundSource` are heights above the elevation of the layer of the object that asks
//! (the 3D scene adds that elevation back), so a model on a layer at 2 m and a plant on a layer at
//! 0 m differ by 2 m. `absolute` gives the height above the origin. A query without an object
//! answers in the frame of the model.
//!
//! **Speed.** The models have a grid over their boxes. A query looks at the models in its cell,
//! so 20 000 plants and a few models cost a lookup each. A model is built once per document
//! revision (`SiteCache`).
//! A hardscape can also act as a pad on the site model (see `pad_of`).

use std::cell::OnceCell;
use std::sync::{Arc, OnceLock};

use glam::{DVec2, DVec3};

use kadakas_core::ground::GroundSource;
use kadakas_core::hardscape;
use kadakas_core::model::{Document, LayerId, Object, ObjectId, Shape};
use kadakas_core::site::boundary_points;

use crate::model::{SiteCache, SiteResult, Surface};
use crate::modifier::{Modifier, PadSlope};

/// The ground of a document, from its site models. See the module text.
pub struct SiteTerrain<'a> {
    cache: &'a SiteCache,
    revision: u64,
    doc: &'a Document,
    index: OnceCell<Index>,
}

/// A site model with the edge of its surfaces, for points outside the model.
struct Host {
    id: ObjectId,
    layer: LayerId,
    /// The elevation of the layer of the model, in mm.
    elevation: f64,
    /// The box of the source data (the surfaces lie inside it).
    lo: DVec2,
    hi: DVec2,
    result: OnceCell<Option<Arc<SiteResult>>>,
    edges: [OnceLock<Vec<[DVec3; 2]>>; 2],
}

impl Host {
    fn contains(&self, p: DVec2) -> bool {
        p.x >= self.lo.x && p.x <= self.hi.x && p.y >= self.lo.y && p.y <= self.hi.y
    }
}

/// The site models of a document and a grid over their boxes.
struct Index {
    hosts: Vec<Host>,
    lo: DVec2,
    cell: f64,
    cols: usize,
    rows: usize,
    /// For each cell the hosts (indices, in drawing order) whose box touches it.
    cells: Vec<Vec<u32>>,
}

/// Cells in each direction of the grid, at most.
const GRID: usize = 32;

impl Index {
    fn new(doc: &Document) -> Index {
        let hosts: Vec<Host> = doc
            .objects
            .values()
            .filter(|o| matches!(o.shape, Shape::SiteModel { .. }) && doc.is_visible(o))
            .map(|o| {
                let (lo, hi) = o.shape.bounds();
                Host {
                    id: o.id,
                    layer: o.layer,
                    elevation: doc.layer(o.layer).map_or(0.0, |l| l.elevation),
                    lo,
                    hi,
                    result: OnceCell::new(),
                    edges: [OnceLock::new(), OnceLock::new()],
                }
            })
            .filter(|h| h.lo.is_finite() && h.hi.is_finite())
            .collect();
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        for h in &hosts {
            lo = lo.min(h.lo);
            hi = hi.max(h.hi);
        }
        let (cols, rows, cell) = if hosts.len() <= 1 {
            (1, 1, f64::INFINITY)
        } else {
            let size = (hi - lo).max_element().max(1.0);
            let cell = size / GRID as f64;
            let cols = (((hi.x - lo.x) / cell).ceil() as usize).clamp(1, GRID);
            let rows = (((hi.y - lo.y) / cell).ceil() as usize).clamp(1, GRID);
            (cols, rows, cell)
        };
        let mut cells = vec![Vec::new(); cols * rows];
        for (i, h) in hosts.iter().enumerate() {
            let (c0, r0, c1, r1) = if cell.is_finite() {
                (
                    (((h.lo.x - lo.x) / cell).floor() as usize).min(cols - 1),
                    (((h.lo.y - lo.y) / cell).floor() as usize).min(rows - 1),
                    (((h.hi.x - lo.x) / cell).floor() as usize).min(cols - 1),
                    (((h.hi.y - lo.y) / cell).floor() as usize).min(rows - 1),
                )
            } else {
                (0, 0, 0, 0)
            };
            for r in r0..=r1 {
                for c in c0..=c1 {
                    cells[r * cols + c].push(i as u32);
                }
            }
        }
        Index {
            hosts,
            lo,
            cell,
            cols,
            rows,
            cells,
        }
    }

    /// The hosts whose box holds `p`, in drawing order.
    fn at(&self, p: DVec2) -> impl Iterator<Item = &Host> {
        let list: &[u32] = if !self.cell.is_finite() {
            &self.cells[0]
        } else {
            let c = ((p.x - self.lo.x) / self.cell).floor();
            let r = ((p.y - self.lo.y) / self.cell).floor();
            if c < 0.0 || r < 0.0 || c >= self.cols as f64 || r >= self.rows as f64 {
                &[]
            } else {
                &self.cells[r as usize * self.cols + c as usize]
            }
        };
        list.iter()
            .map(|i| &self.hosts[*i as usize])
            .filter(move |h| h.contains(p))
    }
}

impl<'a> SiteTerrain<'a> {
    /// `revision` is the revision of `doc`.
    pub fn new(cache: &'a SiteCache, revision: u64, doc: &'a Document) -> Self {
        SiteTerrain {
            cache,
            revision,
            doc,
            index: OnceCell::new(),
        }
    }

    fn index(&self) -> &Index {
        self.index.get_or_init(|| Index::new(self.doc))
    }

    fn result<'h>(&self, h: &'h Host) -> Option<&'h Arc<SiteResult>> {
        h.result
            .get_or_init(|| self.cache.get(self.revision, self.doc, h.id).ok())
            .as_ref()
    }

    fn elevation_of(&self, layer: LayerId) -> f64 {
        self.doc.layer(layer).map_or(0.0, |l| l.elevation)
    }

    /// The change from the frame of the model to the frame of the asking object.
    fn shift(&self, obj: Option<&Object>, h: &Host) -> f64 {
        obj.map_or(0.0, |o| h.elevation - self.elevation_of(o.layer))
    }

    /// True if the document has a visible site model.
    pub fn has_site_models(&self) -> bool {
        !self.index().hosts.is_empty()
    }

    /// The site model that serves the object at `p`, if the ground itself lies under `p`: the
    /// named one, else the first that covers the point (the layer of the object first).
    fn pick(&self, obj: Option<&Object>, p: DVec2, which: Surface) -> Option<(&Host, f64)> {
        let idx = self.index();
        let named = obj.and_then(|o| kadakas_core::site::named_site(self.doc, o));
        if let Some(id) = named {
            let h = idx.hosts.iter().find(|h| h.id == id)?;
            let z = self.result(h)?.surface(which).elevation_at(p)?;
            return Some((h, z));
        }
        let own = obj.map(|o| o.layer);
        let mut found: Option<(&Host, f64)> = None;
        for h in idx.at(p) {
            let Some(z) = self
                .result(h)
                .and_then(|r| r.surface(which).elevation_at(p))
            else {
                continue;
            };
            if Some(h.layer) == own {
                return Some((h, z));
            }
            found.get_or_insert((h, z));
        }
        found
    }

    /// The site model that holds the ground under `p` for this object, and its result.
    pub fn host_at(&self, obj: Option<&Object>, p: DVec2) -> Option<(ObjectId, Arc<SiteResult>)> {
        let (h, _) = self.pick(obj, p, Surface::Existing)?;
        Some((h.id, self.result(h)?.clone()))
    }

    /// The height of the ground at `p` above the origin (the elevation of the layer of the site
    /// model included), on the proposed surface. Outside every model it is the height of the
    /// nearest edge.
    pub fn absolute(&self, obj: &Object, p: DVec2) -> Option<f64> {
        self.elevation(obj, p)
            .map(|z| z + self.elevation_of(obj.layer))
    }

    /// The height above the origin where a pin or a note about `obj` at `p` belongs: the stored
    /// ground height of a plant, else the ground under an object that stands on the ground (plants,
    /// hardscapes, landscape areas, massing models, walls, grades, spots, sections), else the
    /// elevation of the layer of the object.
    pub fn pin_height(&self, obj: &Object, p: DVec2) -> f64 {
        if let Some(z) = kadakas_core::plants::link::ground_z(obj) {
            return z;
        }
        let on_ground = matches!(
            obj.shape,
            Shape::Plant { .. }
                | Shape::ExistingTree { .. }
                | Shape::Hardscape { .. }
                | Shape::LandscapeArea { .. }
                | Shape::Massing(_)
        ) || kadakas_core::site::carries_extra(obj);
        on_ground
            .then(|| self.absolute(obj, p))
            .flatten()
            .unwrap_or_else(|| self.elevation_of(obj.layer))
    }

    fn edge_height(&self, h: &Host, p: DVec2, which: Surface) -> Option<(f64, f64)> {
        let result = self.result(h)?;
        let edges = h.edges[which as usize]
            .get_or_init(|| crate::grading::boundary_edges(result.surface(which)));
        crate::grading::nearest_edge_height(edges, p)
    }
}

impl SiteTerrain<'_> {
    /// The height of the base of a massing model above its layer, in mm: the lowest ground height
    /// under the footprint, so the building stands on the ground and never floats. A base that the
    /// model gives itself wins. Without a site model that covers the footprint the base is 0.
    pub fn massing_base(&self, obj: &Object, m: &kadakas_core::massing::Massing) -> f64 {
        if let Some(b) = m.base {
            return b;
        }
        let samples = self.footprint_samples(m);
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

    /// True if a site model covers some of the footprint of the massing model (or the model gives
    /// its own base). A massing model that is not on the ground stands at the elevation of its
    /// layer; the callers say so.
    pub fn massing_on_ground(&self, obj: &Object, m: &kadakas_core::massing::Massing) -> bool {
        m.base.is_some()
            || self
                .footprint_samples(m)
                .iter()
                .any(|p| self.covered(obj, *p))
    }

    fn footprint_samples(&self, m: &kadakas_core::massing::Massing) -> Vec<DVec2> {
        let ring = m.ring();
        let step = (ring.len() / 64).max(1);
        let mut samples: Vec<DVec2> = ring.iter().step_by(step).copied().collect();
        if !ring.is_empty() {
            samples.push(ring.iter().copied().sum::<DVec2>() / ring.len() as f64);
        }
        samples
    }
}

impl GroundSource for SiteTerrain<'_> {
    fn exact(&self, obj: Option<&Object>, p: DVec2, which: Surface) -> Option<f64> {
        self.pick(obj, p, which)
            .map(|(h, z)| z + self.shift(obj, h))
    }

    /// The height of the ground. Outside every site model it is the height of the nearest edge
    /// of the nearest model (a named model, else the models on the layer of the object, else all
    /// models), so a hardscape that reaches over the edge stays level with the edge there.
    fn clamped(&self, obj: Option<&Object>, p: DVec2, which: Surface) -> Option<f64> {
        if let Some(z) = self.exact(obj, p, which) {
            return Some(z);
        }
        let idx = self.index();
        let named = obj.and_then(|o| kadakas_core::site::named_site(self.doc, o));
        let hosts: Vec<&Host> = match named {
            Some(id) => idx.hosts.iter().filter(|h| h.id == id).collect(),
            None => {
                let own: Vec<&Host> = idx
                    .hosts
                    .iter()
                    .filter(|h| Some(h.layer) == obj.map(|o| o.layer))
                    .collect();
                if own.is_empty() {
                    idx.hosts.iter().collect()
                } else {
                    own
                }
            }
        };
        hosts
            .into_iter()
            .filter_map(|h| {
                self.edge_height(h, p, which)
                    .map(|(d, z)| (d, z + self.shift(obj, h)))
            })
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
            .unwrap_or(kadakas_core::mesh::DEFAULT_SLAB_THICKNESS),
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
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::curve;
    use kadakas_core::hardscape::HardscapePatch;
    use kadakas_core::model::{Attributes, ObjectId};

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
        // An object on another layer stands on the same ground (the layer has the same elevation).
        let mut other = o.clone();
        other.layer = LayerId::from_num(77);
        assert_eq!(t.elevation(&other, DVec2::new(3000.0, 3000.0)), Some(300.0));
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
            slope: Some(kadakas_core::hardscape::Slope::Cross {
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
        let sid = kadakas_core::resource::ResourceId(
            ed.execute(&Command::AddResource {
                name: "S".into(),
                body: kadakas_core::resource::ResourceBody::HardscapeStyle(Box::new(style)),
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

    fn layer(ed: &mut Editor, name: &str, elevation: f64) -> LayerId {
        LayerId(
            ed.execute(&Command::AddLayer {
                name: name.into(),
                elevation,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0],
        )
    }

    fn add_on(ed: &mut Editor, shape: Shape, layer: LayerId) -> ObjectId {
        ObjectId(
            ed.execute(&Command::AddObject {
                shape,
                layer: Some(layer),
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        )
    }

    fn probe(layer: LayerId) -> Object {
        Object::new(
            ObjectId::from_num(0),
            layer,
            kadakas_core::model::NONE_CLASS,
            Shape::Locus3d { p: DVec3::ZERO },
        )
    }

    /// A site model of 9 by 9 points, 1 m apart from `x0`, flat at `z`.
    fn flat(x0: f64, z: f64) -> Shape {
        let mut s = grid_site(9, 1000.0, |_, _| z);
        if let Shape::SiteModel { points, .. } = &mut s {
            points.iter_mut().for_each(|p| p.x += x0);
        }
        s
    }

    #[test]
    fn the_site_model_that_covers_the_place_serves_objects_of_any_layer() {
        let mut ed = Editor::default();
        let site = layer(&mut ed, "Site", 2000.0);
        let plants = layer(&mut ed, "Planting", 5000.0);
        let near = add_on(&mut ed, flat(0.0, 300.0), site);
        let far = add_on(&mut ed, flat(20_000.0, 900.0), site);
        let cache = SiteCache::default();
        let t = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        let o = probe(plants);
        let q = DVec2::new(4000.0, 4000.0);
        // Above the layer of the object: the model is 300 above its layer at 2 m; the object layer
        // is at 5 m, so the ground is 2300 - 5000 = -2700 from it, and 2300 above the origin.
        assert_eq!(t.elevation(&o, q), Some(-2700.0));
        assert_eq!(t.host_at(Some(&o), q).unwrap().0, near);
        assert_eq!(t.elevation(&o, DVec2::new(24_000.0, 4000.0)), Some(-2100.0));
        assert_eq!(
            t.host_at(Some(&o), DVec2::new(24_000.0, 4000.0)).unwrap().0,
            far
        );
        let mut on_site = probe(site);
        on_site.data.clear();
        assert_eq!(t.elevation(&on_site, q), Some(300.0));
        assert!(t.covered(&o, q) && !t.covered(&o, DVec2::new(15_000.0, 4000.0)));
        // Between the models the nearest edge counts: the near model ends at x = 8000.
        assert_eq!(t.elevation(&o, DVec2::new(10_000.0, 4000.0)), Some(-2700.0));
        // Without an object the answer is in the frame of the model.
        assert_eq!(t.exact(None, q, Surface::Existing), Some(300.0));
        assert!(t.has_site_models());
    }

    #[test]
    fn the_layer_of_the_object_wins_and_a_name_wins_over_the_place() {
        let mut ed = Editor::default();
        let a = layer(&mut ed, "A", 0.0);
        let b = layer(&mut ed, "B", 0.0);
        let first = add_on(&mut ed, flat(0.0, 100.0), a);
        let second = add_on(&mut ed, flat(0.0, 700.0), b);
        let cache = SiteCache::default();
        let q = DVec2::new(3000.0, 3000.0);
        let t = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        // Both cover the place: the model on the layer of the object, else the first.
        assert_eq!(t.elevation(&probe(a), q), Some(100.0));
        assert_eq!(t.elevation(&probe(b), q), Some(700.0));
        let c = layer(&mut ed, "C", 0.0);
        let t = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        assert_eq!(t.elevation(&probe(c), q), Some(100.0));
        // The layer names the second model; the object names the first.
        ed.execute(&Command::SetLayerSite {
            id: c,
            site: Some(second),
        })
        .unwrap();
        let t = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        assert_eq!(t.elevation(&probe(c), q), Some(700.0));
        let mut o = probe(c);
        o.data.insert("site_model".into(), serde_json::json!(first));
        let t = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        // The probe is not in the document: its layer names the second model, its data the first.
        assert_eq!(t.elevation(&o, q), Some(100.0));
        // A named model that does not cover the place gives the height of its edge.
        o.data
            .insert("site_model".into(), serde_json::json!(second));
        assert_eq!(t.elevation(&o, DVec2::new(30_000.0, 0.0)), Some(700.0));
        // The command checks its input.
        assert!(
            ed.execute(&Command::SetLayerSite {
                id: c,
                site: Some(ObjectId::from_num(5)),
            })
            .is_err()
        );
        ed.execute(&Command::SetLayerSite { id: c, site: None })
            .unwrap();
        assert!(ed.doc().layer(c).unwrap().extra.get("site_model").is_none());
        assert!(
            ed.execute(&Command::SetLayerSite {
                id: LayerId::from_num(99),
                site: None,
            })
            .is_err()
        );
    }

    #[test]
    fn many_models_use_the_grid_and_hidden_models_are_not_ground() {
        let mut ed = Editor::default();
        let l = layer(&mut ed, "L", 0.0);
        let ids: Vec<ObjectId> = (0..6)
            .map(|i| {
                add_on(
                    &mut ed,
                    flat(i as f64 * 20_000.0, 100.0 * (i + 1) as f64),
                    l,
                )
            })
            .collect();
        let cache = SiteCache::default();
        let t = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        for (i, id) in ids.iter().enumerate() {
            let p = DVec2::new(i as f64 * 20_000.0 + 3000.0, 3000.0);
            assert_eq!(t.host_at(None, p).unwrap().0, *id);
        }
        // Outside the grid, and in a gap between two models.
        assert!(t.host_at(None, DVec2::new(-5000.0, 0.0)).is_none());
        assert!(t.host_at(None, DVec2::new(500_000.0, 0.0)).is_none());
        assert!(t.host_at(None, DVec2::new(12_000.0, 0.0)).is_none());
        // A document with no model has no ground.
        let empty = Editor::default();
        let t = SiteTerrain::new(&cache, empty.revision(), empty.doc());
        assert!(!t.has_site_models());
        assert_eq!(t.elevation(&probe(LayerId::from_num(1)), DVec2::ZERO), None);
    }
}
