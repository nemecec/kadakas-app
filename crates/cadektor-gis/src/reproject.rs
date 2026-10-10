//! Change the CRS of a document.
//!
//! Every point of every object goes from the old map position to the new CRS with `proj4rs`, and the
//! georeference gets the new CRS, the new origin and the new rotation (the grid convergence between
//! the two CRSs). Because the model frame follows the new rotation, angles in the drawing do not
//! change. Distances change by the scale factor of the two projections, which the report shows.
//! The plan is a single batch of commands, so the change is one undo step. A dry run shows the report only.
//!
//! Heights (Z) stay as they are: there is no vertical datum conversion.

use cadektor_core::DVec2;
use cadektor_core::command::Command;
use cadektor_core::curve::{Vertex, VertexKind};
use cadektor_core::georef::Georeference;
use cadektor_core::hardscape::{HardscapePatch, Mode};
use cadektor_core::landscape::LandscapePatch;
use cadektor_core::model::{Document, Object, Shape};
use cadektor_io::crs::{Crs, Transformer};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::raster::{Affine, GeoRaster};
use crate::{GisError, Result};

/// Options of the conversion.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReprojectOptions {
    /// The map position (meters in the new CRS) of the new origin. Default: the old origin converted
    /// and rounded to a whole meter.
    pub origin: Option<DVec2>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Skipped {
    pub id: u64,
    pub kind: String,
    pub reason: String,
}

/// What the conversion does (or did).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReprojectReport {
    pub from_crs: String,
    pub to_crs: String,
    pub old_origin: [f64; 2],
    pub new_origin: [f64; 2],
    /// Counterclockwise angle of the model X axis against the new map X axis, in degrees.
    pub new_rotation_deg: f64,
    pub old_rotation_deg: f64,
    /// A length of 1 in the old CRS becomes this length in the new one, at the origin.
    pub scale_factor: f64,
    /// Objects that change, by kind.
    pub changed: BTreeMap<String, usize>,
    pub objects_changed: usize,
    /// The largest distance (mm) that a point of the drawing moves in the model frame. Points near
    /// the origin move by about `drawing size * (scale_factor - 1)`.
    pub max_shift_mm: f64,
    pub skipped: Vec<Skipped>,
    pub warnings: Vec<String>,
}

/// The conversion as commands plus the report.
#[derive(Clone, Debug)]
pub struct ReprojectPlan {
    /// `None` when nothing changes (same CRS and origin).
    pub command: Option<Command>,
    pub report: ReprojectReport,
}

struct Mapper<'a> {
    old: &'a Crs,
    new: &'a Crs,
    old_g: &'a Georeference,
    new_g: &'a Georeference,
    scale: f64,
}

impl Mapper<'_> {
    fn point(&self, p: DVec2) -> Result<DVec2> {
        let m = self.old_g.to_map(p) / self.old.to_meter();
        let (x, y, _) = Transformer::new(self.old, self.new)
            .apply(m.x, m.y, 0.0)
            .map_err(GisError::Crs)?;
        let q = self.new_g.to_model(DVec2::new(x, y) * self.new.to_meter());
        if q.is_finite() && q.abs().max_element() <= cadektor_core::model::MAX_COORD {
            Ok(q)
        } else {
            Err(GisError::Crs("a point is out of range in the new CRS".into()))
        }
    }

    fn vertices(&self, v: &mut [Vertex]) -> Result<()> {
        for x in v {
            x.p = self.point(x.p)?;
            if let VertexKind::Arc { radius } = &mut x.kind {
                *radius *= self.scale;
            }
        }
        Ok(())
    }

    fn vec3(&self, v: &mut [cadektor_core::DVec3]) -> Result<()> {
        for p in v {
            let q = self.point(p.truncate())?;
            p.x = q.x;
            p.y = q.y;
        }
        Ok(())
    }

    /// Moves the points of a shape. `Ok(false)` means the shape has no points in the model.
    fn shape(&self, s: &mut Shape) -> Result<bool> {
        match s {
            Shape::Line { a, b } => {
                *a = self.point(*a)?;
                *b = self.point(*b)?;
            }
            Shape::Rect { min, max } => {
                let (a, b) = (self.point(*min)?, self.point(*max)?);
                (*min, *max) = (a, b);
            }
            Shape::Circle { center, radius } => {
                *center = self.point(*center)?;
                *radius *= self.scale;
            }
            Shape::Arc { center, radius, .. } => {
                *center = self.point(*center)?;
                *radius *= self.scale;
            }
            Shape::Polyline { points, .. } => self.vertices(points)?,
            Shape::Text { at, .. } | Shape::Plant { at, .. } | Shape::ExistingTree { at, .. } => {
                *at = self.point(*at)?
            }
            Shape::PlantTag { at, anchor, .. } => {
                *at = self.point(*at)?;
                *anchor = self.point(*anchor)?;
            }
            Shape::LandscapeArea { boundary, .. }
            | Shape::Hardscape { boundary, .. }
            | Shape::SiteModifier { boundary, .. } => self.vertices(boundary)?,
            Shape::Massing(m) => self.vertices(&mut m.footprint)?,
            Shape::Locus3d { p } => {
                let q = self.point(p.truncate())?;
                p.x = q.x;
                p.y = q.y;
            }
            Shape::Polyline3d { points, .. } => self.vec3(points)?,
            Shape::SiteModel { points, lines, .. } => {
                self.vec3(points)?;
                for l in lines {
                    self.vec3(&mut l.points)?;
                }
            }
            Shape::Symbol { at, .. } => *at = self.point(*at)?,
            // Paper space, derived or placed by other code.
            Shape::Viewport(vp) => vp.center = self.point(vp.center)?,
            Shape::TitleBlock(_) | Shape::Group { .. } | Shape::Annotation(_) | Shape::Underlay(_) => {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The data of an object that holds positions (a path center line, holes of a landscape area).
    fn data(&self, o: &mut Object) -> Result<()> {
        if let Some(mut d) = cadektor_core::hardscape::data(o)
            && !d.center.is_empty()
        {
            self.vertices(&mut d.center)?;
            cadektor_core::hardscape::data::set_data(o, &d);
        }
        if let Some(mut d) = cadektor_core::landscape::data(o)
            && !d.holes.is_empty()
        {
            for h in &mut d.holes {
                self.vertices(h)?;
            }
            cadektor_core::landscape::set_data(o, &d);
        }
        Ok(())
    }

    /// An underlay warped into the new model frame.
    fn underlay(&self, u: &cadektor_core::underlay::Underlay) -> Result<cadektor_core::underlay::Underlay> {
        let img = u
            .decode(cadektor_core::underlay::MAX_SIDE_PX)
            .ok_or_else(|| GisError::Format("the picture of an underlay cannot be read".into()))?;
        // Pixel to old map position (in the units of the old CRS).
        let (w, h) = (f64::from(img.width), f64::from(img.height));
        let k = 1.0 / (1000.0 * self.old.to_meter());
        let px = DVec2::new((u.max.x - u.min.x) / w, (u.max.y - u.min.y) / h);
        let r = DVec2::from_angle(self.old_g.rotation);
        let origin = self.old_g.origin / self.old.to_meter();
        let corner = origin + r.rotate(DVec2::new(u.min.x, u.max.y)) * k;
        let col = r.rotate(DVec2::new(px.x, 0.0)) * k;
        let row = r.rotate(DVec2::new(0.0, -px.y)) * k;
        let affine = Affine {
            a: col.x,
            b: row.x,
            c: corner.x,
            d: col.y,
            e: row.y,
            f: corner.y,
        };
        let geo = crate::geoimage::GeoImage {
            name: u.name.clone(),
            raster: GeoRaster { image: img, affine },
            crs: Some(self.old.spec.clone()),
            notes: Vec::new(),
        };
        let placed = crate::geoimage::place(&geo, Some(self.new_g), cadektor_core::underlay::MAX_SIDE_PX)?;
        let mut out = placed.underlay;
        out.opacity = u.opacity;
        out.source = u.source.clone();
        out.name = u.name.clone();
        Ok(out)
    }
}

/// Plans the change of the CRS of a document to `target`.
pub fn plan(doc: &Document, target: &str, opts: &ReprojectOptions) -> Result<ReprojectPlan> {
    let old_g = doc
        .georef
        .as_ref()
        .filter(|g| !g.crs.trim().is_empty())
        .ok_or_else(|| {
            GisError::Param(
                "the document has no CRS. Set the georeference first (or import a file that has one)".into(),
            )
        })?;
    let old = Crs::parse(&old_g.crs).map_err(GisError::Crs)?;
    let new = Crs::parse(target).map_err(GisError::Crs)?;
    if new.is_geographic() {
        return Err(GisError::Crs(
            "the new CRS must be a projected CRS (meters), not longitude and latitude".into(),
        ));
    }
    if !old_g.is_finite() {
        return Err(GisError::Param("the georeference is not valid".into()));
    }
    if let Some(l) = doc.layers.iter().find(|l| {
        l.locked
            && doc.objects.values().any(|o| {
                o.layer == l.id
                    && !matches!(
                        o.shape,
                        Shape::Group { .. } | Shape::TitleBlock(_) | Shape::Annotation(_)
                    )
            })
    }) {
        return Err(GisError::Param(format!(
            "layer {} is locked. Unlock it before the conversion",
            l.name
        )));
    }
    let same = old.spec == new.spec || (old.epsg().is_some() && old.epsg() == new.epsg());
    // The origin, the rotation and the scale from the old origin and a step along the model X axis.
    let to_new = |p: DVec2| -> Result<DVec2> {
        let (x, y, _) = Transformer::new(&old, &new)
            .apply(p.x / old.to_meter(), p.y / old.to_meter(), 0.0)
            .map_err(GisError::Crs)?;
        Ok(DVec2::new(x, y) * new.to_meter())
    };
    let step = 100.0;
    let axis = DVec2::from_angle(old_g.rotation);
    let o_new = to_new(old_g.origin)?;
    let x_new = to_new(old_g.origin + axis * step)?;
    let scale = if same {
        1.0
    } else {
        (x_new - o_new).length() / step
    };
    let rotation = if same {
        old_g.rotation
    } else {
        (x_new - o_new).to_angle()
    };
    let new_origin = match opts.origin {
        Some(o) if o.is_finite() => o,
        Some(_) => return Err(GisError::Param("origin must be finite numbers".into())),
        None if same => old_g.origin,
        None => o_new.round(),
    };
    let mut new_g = Georeference {
        crs: new.spec.clone(),
        origin: new_origin,
        rotation,
        user_origin: old_g.user_origin,
    };
    let user_origin = Mapper {
        old: &old,
        new: &new,
        old_g,
        new_g: &new_g,
        scale,
    }
    .point(old_g.user_origin)?;
    new_g.user_origin = user_origin;
    let mapper = Mapper {
        old: &old,
        new: &new,
        old_g,
        new_g: &new_g,
        scale,
    };
    let mut commands = vec![Command::SetGeoreference {
        georef: Some(new_g.clone()),
    }];
    let mut report = ReprojectReport {
        from_crs: old.spec.clone(),
        to_crs: new.spec.clone(),
        old_origin: [old_g.origin.x, old_g.origin.y],
        new_origin: [new_origin.x, new_origin.y],
        new_rotation_deg: rotation.to_degrees(),
        old_rotation_deg: old_g.rotation.to_degrees(),
        scale_factor: scale,
        changed: BTreeMap::new(),
        objects_changed: 0,
        max_shift_mm: 0.0,
        skipped: Vec::new(),
        warnings: Vec::new(),
    };
    let mut annotations = 0;
    for o in doc.objects.values() {
        let mut shape = o.shape.clone();
        let mut obj = o.clone();
        let moved = match &mut shape {
            Shape::Underlay(u) => match mapper.underlay(u) {
                Ok(n) => {
                    **u = n;
                    true
                }
                Err(e) => {
                    report.skipped.push(Skipped {
                        id: o.id.0,
                        kind: "underlay".into(),
                        reason: e.to_string(),
                    });
                    continue;
                }
            },
            s => match mapper.shape(s) {
                Ok(m) => m,
                Err(e) => {
                    report.skipped.push(Skipped {
                        id: o.id.0,
                        kind: o.shape.kind().into(),
                        reason: e.to_string(),
                    });
                    continue;
                }
            },
        };
        if !moved {
            if matches!(o.shape, Shape::Annotation(_)) {
                annotations += 1;
            }
            continue;
        }
        if let Err(e) = mapper.data(&mut obj) {
            report.skipped.push(Skipped {
                id: o.id.0,
                kind: o.shape.kind().into(),
                reason: e.to_string(),
            });
            continue;
        }
        if !shape.is_finite() || !shape.is_in_range() {
            report.skipped.push(Skipped {
                id: o.id.0,
                kind: o.shape.kind().into(),
                reason: "the converted shape is out of range".into(),
            });
            continue;
        }
        let (a, b) = o.shape.bounds();
        let (na, nb) = shape.bounds();
        if a.is_finite() && na.is_finite() {
            report.max_shift_mm = report
                .max_shift_mm
                .max(a.distance(na))
                .max(b.distance(nb));
        }
        let modify = Command::ModifyObject {
            id: o.id,
            shape: Some(shape),
            layer: None,
            class: None,
            attrs: None,
        };
        let path = cadektor_core::hardscape::data(&obj).filter(|d| d.mode == Mode::Path);
        let holes = cadektor_core::landscape::data(&obj).map(|d| d.holes).filter(|h| !h.is_empty());
        if let Some(d) = path {
            // The outline of a path comes from its center line.
            commands.push(Command::SetHardscape {
                ids: vec![o.id],
                patch: HardscapePatch {
                    center: Some(d.center),
                    ..Default::default()
                },
            });
        } else if let Some(holes) = holes {
            // The holes must fit the boundary at every step, so they are cleared and set again.
            let set = |holes| Command::SetLandscapeArea {
                ids: vec![o.id],
                patch: LandscapePatch {
                    holes: Some(holes),
                    ..Default::default()
                },
            };
            commands.extend([set(Vec::new()), modify, set(holes)]);
        } else {
            commands.push(modify);
        }
        let set: BTreeMap<String, Value> = obj
            .data
            .iter()
            .filter(|(k, v)| o.data.get(*k) != Some(*v))
            .filter(|(k, _)| k.as_str() != "hardscape" && k.as_str() != "landscape")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !set.is_empty() {
            commands.push(Command::SetData {
                id: o.id,
                set,
                remove: Vec::new(),
            });
        }
        *report.changed.entry(o.shape.kind().to_string()).or_default() += 1;
        report.objects_changed += 1;
    }
    if annotations > 0 {
        report.warnings.push(format!(
            "{annotations} annotation object(s) (dimensions, tags, callouts) keep their model positions and are not moved. Check them"
        ));
    }
    if !report.skipped.is_empty() {
        report.warnings.push(format!(
            "{} object(s) could not be converted and stay where they are",
            report.skipped.len()
        ));
    }
    report
        .warnings
        .push("heights (Z) are not changed: there is no vertical datum conversion".into());
    if (scale - 1.0).abs() > 1e-4 && !same {
        report.warnings.push(format!(
            "lengths change by the scale factor {scale:.6} ({:+.3} %) between the two projections",
            (scale - 1.0) * 100.0
        ));
    }
    let nothing = same && new_origin == old_g.origin;
    Ok(ReprojectPlan {
        command: (!nothing).then(|| Command::Batch { commands }),
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::command::Editor;
    use cadektor_core::curve::corners;
    use cadektor_core::model::{Attributes, ClassId, LayerId};

    fn doc_in(crs: &str, origin: DVec2, rotation: f64) -> Editor {
        let mut doc = Document::default();
        doc.georef = Some(Georeference {
            crs: crs.into(),
            origin,
            rotation,
            user_origin: DVec2::ZERO,
        });
        Editor::new(doc)
    }

    fn add(ed: &mut Editor, shape: Shape) -> cadektor_core::model::ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        cadektor_core::model::ObjectId(out.created[0])
    }

    fn ring(w: f64) -> Vec<Vertex> {
        corners(&[
            DVec2::ZERO,
            DVec2::new(w, 0.0),
            DVec2::new(w, w),
            DVec2::new(0.0, w),
        ])
    }

    /// Where a model point lies on the earth, as longitude and latitude.
    fn lonlat(doc: &Document, p: DVec2) -> DVec2 {
        let g = doc.georef.as_ref().unwrap();
        let crs = Crs::parse(&g.crs).unwrap();
        let m = g.to_map(p) / crs.to_meter();
        let wgs = Crs::parse("EPSG:4326").unwrap();
        let (x, y, _) = Transformer::new(&crs, &wgs).apply(m.x, m.y, 0.0).unwrap();
        DVec2::new(x, y)
    }

    #[test]
    fn objects_stay_in_place_on_the_earth() {
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        let poly = add(&mut ed, Shape::Polyline { points: ring(50_000.0), closed: true });
        let line = add(&mut ed, Shape::Line { a: DVec2::new(1000.0, 2000.0), b: DVec2::new(30_000.0, -5000.0) });
        let circle = add(&mut ed, Shape::Circle { center: DVec2::new(5000.0, 5000.0), radius: 2000.0 });
        let before = ed.doc().clone();
        let p = plan(&before, "EPSG:32635", &ReprojectOptions::default()).unwrap();
        assert_eq!(p.report.objects_changed, 3);
        assert_eq!(p.report.changed["polyline"], 1);
        ed.execute(&p.command.unwrap()).unwrap();
        let after = ed.doc();
        assert_eq!(after.georef.as_ref().unwrap().crs, "EPSG:32635");
        // Every vertex is within a millimeter of the same place on the earth.
        let (Shape::Polyline { points: b, .. }, Shape::Polyline { points: a, .. }) =
            (&before.objects[&poly].shape, &after.objects[&poly].shape)
        else {
            panic!()
        };
        for (x, y) in b.iter().zip(a) {
            let (p0, p1) = (lonlat(&before, x.p), lonlat(after, y.p));
            assert!((p0 - p1).length() < 1e-8, "{p0} {p1}");
        }
        let (Shape::Line { b: lb, .. }, Shape::Line { b: la, .. }) =
            (&before.objects[&line].shape, &after.objects[&line].shape)
        else {
            panic!()
        };
        assert!((lonlat(&before, *lb) - lonlat(after, *la)).length() < 1e-8);
        // The scale factor is the one between the two projections, and the circle follows it.
        let k = p.report.scale_factor;
        assert!((k - 1.0).abs() < 0.002 && k != 1.0);
        let (Shape::Circle { radius: r0, .. }, Shape::Circle { radius: r1, .. }) =
            (&before.objects[&circle].shape, &after.objects[&circle].shape)
        else {
            panic!()
        };
        assert!((r1 / r0 - k).abs() < 1e-12);
        // Undo brings back the old CRS and shapes.
        ed.undo();
        assert_eq!(ed.doc().georef.as_ref().unwrap().crs, "EPSG:3067");
        assert_eq!(ed.doc().objects[&poly].shape, before.objects[&poly].shape);
    }

    #[test]
    fn round_trip_returns_the_drawing() {
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.4);
        let id = add(&mut ed, Shape::Polyline { points: ring(20_000.0), closed: true });
        let first = ed.doc().objects[&id].shape.clone();
        let there = plan(ed.doc(), "EPSG:3857", &ReprojectOptions::default()).unwrap();
        ed.execute(&there.command.unwrap()).unwrap();
        let back = plan(ed.doc(), "EPSG:3067", &ReprojectOptions { origin: Some(DVec2::new(385_000.0, 6_672_000.0)) }).unwrap();
        ed.execute(&back.command.unwrap()).unwrap();
        let g = ed.doc().georef.as_ref().unwrap();
        assert_eq!(g.crs, "EPSG:3067");
        assert!((g.rotation - 0.4).abs() < 1e-5, "{}", g.rotation);
        let (Shape::Polyline { points: a, .. }, Shape::Polyline { points: b, .. }) = (&first, &ed.doc().objects[&id].shape) else {
            panic!()
        };
        for (x, y) in a.iter().zip(b) {
            assert!((x.p - y.p).length() < 5.0, "{} {}", x.p, y.p);
        }
    }

    #[test]
    fn same_crs_changes_nothing_and_a_new_origin_moves_the_frame() {
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        let id = add(&mut ed, Shape::Rect { min: DVec2::ZERO, max: DVec2::new(1000.0, 2000.0) });
        let p = plan(ed.doc(), "EPSG:3067", &ReprojectOptions::default()).unwrap();
        assert!(p.command.is_none());
        assert_eq!(p.report.scale_factor, 1.0);
        // Same CRS with a new origin 10 m east: the rectangle moves 10 m west in the frame.
        let p = plan(ed.doc(), "EPSG:3067", &ReprojectOptions { origin: Some(DVec2::new(385_010.0, 6_672_000.0)) }).unwrap();
        ed.execute(&p.command.unwrap()).unwrap();
        let Shape::Rect { min, max } = ed.doc().objects[&id].shape else { panic!() };
        assert!((min - DVec2::new(-10_000.0, 0.0)).length() < 1e-6);
        assert!((max - DVec2::new(-9000.0, 2000.0)).length() < 1e-6);
        assert!(plan(ed.doc(), "EPSG:3067", &ReprojectOptions { origin: Some(DVec2::NAN) }).is_err());
    }

    #[test]
    fn data_underlays_and_other_shapes_are_converted() {
        use cadektor_core::underlay::{PngData, Rgba, Underlay};
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        add(&mut ed, Shape::Hardscape { boundary: ring(4000.0), material: "stone".into(), thickness: None, elevation: None });
        add(&mut ed, Shape::Massing(Box::new(cadektor_core::massing::Massing::new(ring(8000.0), 6000.0))));
        add(&mut ed, Shape::Text { at: DVec2::new(10.0, 10.0), text: "x".into(), height: 100.0 });
        add(&mut ed, Shape::Locus3d { p: cadektor_core::DVec3::new(5.0, 5.0, 42.0) });
        add(&mut ed, Shape::Plant { at: DVec2::ONE, species: "Oak".into(), spread: 3000.0, height: 5000.0 });
        let img = Rgba { width: 2, height: 2, data: vec![255, 0, 0, 255].repeat(4) };
        add(
            &mut ed,
            Shape::Underlay(Box::new(Underlay {
                name: "map".into(),
                min: DVec2::ZERO,
                max: DVec2::new(100_000.0, 100_000.0),
                width: 2,
                height: 2,
                png: PngData(std::sync::Arc::new(img.to_png())),
                opacity: 0.5,
                source: "src".into(),
            })),
        );
        let p = plan(ed.doc(), "EPSG:32635", &ReprojectOptions::default()).unwrap();
        ed.execute(&p.command.clone().unwrap()).unwrap();
        for k in ["hardscape", "massing", "text", "locus3d", "plant", "underlay"] {
            assert_eq!(p.report.changed[k], 1, "{k}");
        }
        let l = ed.doc().objects.values().find(|o| matches!(o.shape, Shape::Locus3d { .. })).unwrap();
        let Shape::Locus3d { p: locus } = l.shape else { panic!() };
        assert_eq!(locus.z, 42.0);
        let u = ed.doc().objects.values().find_map(|o| match &o.shape {
            Shape::Underlay(u) => Some(u.clone()),
            _ => None,
        }).unwrap();
        assert_eq!((u.opacity, u.source.as_str(), u.name.as_str()), (0.5, "src", "map"));
        assert!(u.problem().is_none());
        assert!(p.report.warnings.iter().any(|w| w.contains("heights")));
    }

    #[test]
    fn hardscape_and_landscape_data_follow() {
        use cadektor_core::hardscape::HardscapePatch;
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        ed.execute(&Command::AddHardscape {
            boundary: None,
            patch: Box::new(HardscapePatch {
                center: Some(corners(&[DVec2::ZERO, DVec2::new(10_000.0, 0.0), DVec2::new(10_000.0, 8000.0)])),
                width: Some(1200.0),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        let before = ed.doc().clone();
        let id = *before.objects.keys().next().unwrap();
        let p = plan(&before, "EPSG:32635", &ReprojectOptions::default()).unwrap();
        ed.execute(&p.command.unwrap()).unwrap();
        let old = cadektor_core::hardscape::data(&before.objects[&id]).unwrap();
        let new = cadektor_core::hardscape::data(&ed.doc().objects[&id]).unwrap();
        for (a, b) in old.center.iter().zip(&new.center) {
            assert!((lonlat(&before, a.p) - lonlat(ed.doc(), b.p)).length() < 1e-8);
        }
    }

    #[test]
    fn refuses_what_it_cannot_do() {
        let doc = Document::default();
        assert!(plan(&doc, "EPSG:3067", &ReprojectOptions::default()).unwrap_err().to_string().contains("no CRS"));
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        assert!(plan(ed.doc(), "EPSG:4326", &ReprojectOptions::default()).unwrap_err().to_string().contains("projected"));
        assert!(plan(ed.doc(), "banana", &ReprojectOptions::default()).is_err());
        let id = add(&mut ed, Shape::Line { a: DVec2::ZERO, b: DVec2::ONE });
        let layer = ed.doc().objects[&id].layer;
        ed.execute(&Command::ModifyLayer { id: layer, name: None, visible: None, locked: Some(true), elevation: None, scale: None }).unwrap();
        assert!(plan(ed.doc(), "EPSG:32635", &ReprojectOptions::default()).unwrap_err().to_string().contains("locked"));
        let mut bad = doc_in("EPSG:3067", DVec2::NAN, 0.0);
        assert!(plan(bad.doc(), "EPSG:32635", &ReprojectOptions::default()).is_err());
        let _ = (&mut bad, LayerId(0), ClassId(0));
    }

    #[test]
    fn objects_that_cannot_move_are_listed() {
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        add(&mut ed, Shape::Line { a: DVec2::ZERO, b: DVec2::ONE });
        // 800 km north is valid in TM35FIN, but in Web Mercator at 67 degrees north the distances
        // grow by more than two, and the point leaves the range of the drawing.
        let far = add(&mut ed, Shape::Line { a: DVec2::new(0.0, 8.0e8), b: DVec2::ONE });
        let p = plan(ed.doc(), "EPSG:3857", &ReprojectOptions::default()).unwrap();
        assert_eq!(p.report.objects_changed, 1);
        assert_eq!(p.report.skipped.len(), 1);
        assert_eq!(p.report.skipped[0].id, far.0);
        assert!(p.report.warnings.iter().any(|w| w.contains("could not be converted")));
        assert!(p.report.skipped[0].reason.contains("out of range"));
        // The rest of the plan still works.
        ed.execute(&p.command.unwrap()).unwrap();
    }
}
