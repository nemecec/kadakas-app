//! Change the CRS of a document.
//!
//! Every point of every object goes from the old map position to the new CRS with `proj4rs`, and the
//! georeference gets the new CRS, the new origin and the new rotation (the grid convergence between
//! the two CRSs). Because the model frame follows the new rotation, angles in the drawing do not
//! change. Distances change by the scale factor of the two projections, which the report shows.
//! The plan is a single batch of commands, so the change is one undo step. A dry run shows the report only.
//!
//! Heights (Z) stay as they are: there is no vertical datum conversion.

use kadakas_core::command::Command;
use kadakas_core::curve::{Vertex, VertexKind};
use kadakas_core::georef::Georeference;
use kadakas_core::hardscape::{HardscapePatch, Mode};
use kadakas_core::landscape::LandscapePatch;
use kadakas_core::model::{Document, Object, Shape};
use kadakas_core::{DMat2, DVec2};
use kadakas_crs::{Crs, Transformer};
use serde::Serialize;
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
    pub id: kadakas_core::uid::Uid,
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
        if q.is_finite() && q.abs().max_element() <= kadakas_core::model::MAX_COORD {
            Ok(q)
        } else {
            Err(GisError::Crs(
                "a point is out of range in the new CRS".into(),
            ))
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

    /// A point with a height (mm). The height goes through the conversion too: it stays the same
    /// between CRSs that share a datum, and changes by the datum shift where they do not.
    fn point3(&self, p: kadakas_core::DVec3) -> Result<kadakas_core::DVec3> {
        let m = self.old_g.to_map(p.truncate()) / self.old.to_meter();
        let (x, y, z) = Transformer::new(self.old, self.new)
            .apply(m.x, m.y, p.z / 1000.0)
            .map_err(GisError::Crs)?;
        let q = self.new_g.to_model(DVec2::new(x, y) * self.new.to_meter());
        let out = q.extend(z * 1000.0);
        if out.is_finite() && out.abs().max_element() <= kadakas_core::model::MAX_COORD {
            Ok(out)
        } else {
            Err(GisError::Crs(
                "a point is out of range in the new CRS".into(),
            ))
        }
    }

    fn vec3(&self, v: &mut [kadakas_core::DVec3]) -> Result<()> {
        for p in v {
            *p = self.point3(*p)?;
        }
        Ok(())
    }

    /// Moves a terrain texture mapping with the ground: its origin moves, it turns as the map
    /// turns there, and its tile follows the scale of the CRS change.
    fn mapping(&self, tile: &mut f64, offset: &mut [f64; 2], rotation: &mut f64) -> Result<()> {
        let step = 1000.0;
        let at = DVec2::from(*offset);
        let o = self.point(at)?;
        let dx = (self.point(at + DVec2::new(step, 0.0))? - o) / step;
        *rotation += dx.y.atan2(dx.x).to_degrees();
        *tile = (*tile * self.scale).clamp(
            kadakas_core::site::MIN_TEXTURE_TILE,
            kadakas_core::site::MAX_TEXTURE_TILE,
        );
        *offset = o.to_array();
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
            Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
                self.vertices(boundary)?
            }
            Shape::SiteModifier { modifier, boundary } => {
                self.vertices(boundary)?;
                // A texture bed with its own mapping is anchored to the ground like a site model.
                if let kadakas_core::site::ModifierKind::TextureBed {
                    mapping: Some(m), ..
                } = modifier
                {
                    self.mapping(&mut m.tile, &mut m.offset, &mut m.rotation)?;
                }
            }
            Shape::Massing(m) => self.vertices(&mut m.footprint)?,
            Shape::Extrude(e) => {
                self.vertices(&mut e.profile)?;
                for h in &mut e.holes {
                    self.vertices(h)?;
                }
                // The heights are lengths: they follow the scale of the CRS change.
                e.height *= self.scale;
                e.base *= self.scale;
            }
            // The axis point moves and the sizes follow the scale of the CRS change. The plan
            // directions (the axis, the plane of the profile) turn as the map turns at the axis
            // point: the map of two nearby points gives the turn.
            Shape::Sweep(sw) => {
                let o = self.point3(sw.origin)?;
                let step = 1000.0;
                let at = sw.origin.truncate();
                let dx = (self.point(at + DVec2::new(step, 0.0))? - o.truncate()) / step;
                let dy = (self.point(at + DVec2::new(0.0, step))? - o.truncate()) / step;
                sw.remap_plan(o.truncate(), DMat2::from_cols(dx, dy), self.scale, false);
                sw.origin.z = o.z;
            }
            Shape::Locus3d { p } => *p = self.point3(*p)?,
            Shape::Mesh3d(m) => self.vec3(&mut m.vertices)?,
            // The origin of the model moves; its plan directions turn and scale as the map does
            // there (as for a sweep), and heights follow the scale.
            Shape::SurfaceModel(m) => {
                let t = m.transform.translation;
                let o = self.point3(t)?;
                let step = 1000.0;
                let at = t.truncate();
                let dx = (self.point(at + DVec2::new(step, 0.0))? - o.truncate()) / step;
                let dy = (self.point(at + DVec2::new(0.0, step))? - o.truncate()) / step;
                m.remap_plan(o, dx, dy, self.scale);
            }
            // An instance of a component moves like a surface model: the definition stays as it is.
            Shape::Component { transform, .. } => {
                let a = kadakas_core::component::affine(transform);
                let t = a.translation;
                let o = self.point3(t)?;
                let step = 1000.0;
                let at = t.truncate();
                let dx = (self.point(at + DVec2::new(step, 0.0))? - o.truncate()) / step;
                let dy = (self.point(at + DVec2::new(0.0, step))? - o.truncate()) / step;
                *transform = kadakas_core::component::remap_plan(transform, o, dx, dy, self.scale);
            }
            // A guide moves with its point; its direction turns as the map does there.
            Shape::Guide(g) => {
                let o = self.point3(g.at)?;
                if let Some(d) = g.unit_dir() {
                    let q = self.point3(g.at + d * 1000.0)?;
                    g.dir = Some((q - o).try_normalize().unwrap_or(d));
                    if let Some(l) = &mut g.length {
                        *l *= self.scale;
                    }
                }
                g.at = o;
            }
            Shape::Polyline3d { points, .. } => self.vec3(points)?,
            Shape::SiteModel {
                points,
                lines,
                settings,
            } => {
                self.vec3(points)?;
                for l in lines {
                    self.vec3(&mut l.points)?;
                }
                // The texture is anchored to the ground: its origin moves, it turns as the map
                // turns there, and its tile follows the scale of the CRS change.
                if settings.surface_look == kadakas_core::site::SurfaceLook::Texture {
                    self.mapping(
                        &mut settings.texture_tile,
                        &mut settings.texture_offset,
                        &mut settings.texture_rotation,
                    )?;
                }
            }
            Shape::Symbol { at, .. } => *at = self.point(*at)?,
            Shape::Irrigation(i) => {
                use kadakas_core::irrigation::IrrigationShape as I;
                match &mut **i {
                    I::Outlet { at, .. }
                    | I::Poc { at, .. }
                    | I::Valve { at, .. }
                    | I::Controller { at, .. } => *at = self.point(*at)?,
                    I::Line { points, .. } => {
                        for p in points {
                            *p = self.point(*p)?;
                        }
                    }
                }
            }
            // Paper space, derived or placed by other code.
            Shape::Viewport(vp) => vp.center = self.point(vp.center)?,
            // Annotations on a design layer carry model points. The caller does not send those on a
            // sheet, where they are paper millimeters.
            Shape::Annotation(a) => {
                let mut failed = None;
                a.map_points(&mut |p| match self.point(p) {
                    Ok(q) => q,
                    Err(e) => {
                        failed.get_or_insert(e);
                        p
                    }
                });
                if let Some(e) = failed {
                    return Err(e);
                }
            }
            Shape::TitleBlock(_)
            | Shape::Group { .. }
            | Shape::Unknown { .. }
            | Shape::Underlay(_) => {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The data of an object that holds positions (a path center line, holes of a landscape area).
    fn data(&self, o: &mut Object) -> Result<()> {
        if let Some(mut d) = kadakas_core::hardscape::data(o)
            && !d.center.is_empty()
        {
            self.vertices(&mut d.center)?;
            kadakas_core::hardscape::data::set_data(o, &d);
        }
        if let Some(mut d) = kadakas_core::landscape::data(o)
            && !d.holes.is_empty()
        {
            for h in &mut d.holes {
                self.vertices(h)?;
            }
            kadakas_core::landscape::set_data(o, &d);
        }
        Ok(())
    }

    /// An underlay warped into the new model frame.
    fn underlay(
        &self,
        u: &kadakas_core::underlay::Underlay,
    ) -> Result<kadakas_core::underlay::Underlay> {
        let img = u
            .decode(kadakas_core::underlay::MAX_SIDE_PX)
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
        let placed =
            crate::geoimage::place(&geo, Some(self.new_g), kadakas_core::underlay::MAX_SIDE_PX)?;
        let mut out = placed.underlay;
        out.opacity = u.opacity;
        out.source = u.source.clone();
        out.name = u.name.clone();
        Ok(out)
    }
}

fn on_sheet(doc: &Document, o: &Object) -> bool {
    doc.layer(o.layer)
        .is_some_and(|l| l.kind == kadakas_core::model::LayerKind::Sheet)
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
                    && !matches!(o.shape, Shape::Group { .. } | Shape::TitleBlock(_))
                    && !(matches!(o.shape, Shape::Annotation(_)) && on_sheet(doc, o))
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
    for o in doc.objects.values() {
        // Paper space annotations are in paper millimeters and do not change.
        if matches!(o.shape, Shape::Annotation(_)) && on_sheet(doc, o) {
            continue;
        }
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
            report.max_shift_mm = report.max_shift_mm.max(a.distance(na)).max(b.distance(nb));
        }
        let new_view = match &shape {
            Shape::Underlay(u) => Some([u.min.x, u.min.y, u.max.x, u.max.y]),
            _ => None,
        };
        let modify = Command::ModifyObject {
            id: o.id,
            shape: Some(shape),
            layer: None,
            class: None,
            attrs: None,
        };
        let path = kadakas_core::hardscape::data(&obj).filter(|d| d.mode == Mode::Path);
        let holes = kadakas_core::landscape::data(&obj)
            .map(|d| d.holes)
            .filter(|h| !h.is_empty());
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
        // The view that a map was fetched for is part of its source. It moves with the picture.
        if let (Some(view), Some(src)) = (new_view, o.data.get("gis_source"))
            && src.get("bbox").is_some()
        {
            let mut src = src.clone();
            src["bbox"] = serde_json::json!(view);
            commands.push(Command::SetData {
                id: o.id,
                set: BTreeMap::from([("gis_source".to_string(), src)]),
                remove: Vec::new(),
            });
        }
        *report
            .changed
            .entry(o.shape.kind().to_string())
            .or_default() += 1;
        report.objects_changed += 1;
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
        command: (!nothing).then_some(Command::Batch { commands }),
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::Editor;
    use kadakas_core::curve::corners;
    use kadakas_core::model::{Attributes, ClassId, LayerId};

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

    fn add(ed: &mut Editor, shape: Shape) -> kadakas_core::model::ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        kadakas_core::model::ObjectId(out.created[0])
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
        let poly = add(
            &mut ed,
            Shape::Polyline {
                points: ring(50_000.0),
                closed: true,
            },
        );
        let line = add(
            &mut ed,
            Shape::Line {
                a: DVec2::new(1000.0, 2000.0),
                b: DVec2::new(30_000.0, -5000.0),
            },
        );
        let circle = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::new(5000.0, 5000.0),
                radius: 2000.0,
            },
        );
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
        let (Shape::Circle { radius: r0, .. }, Shape::Circle { radius: r1, .. }) = (
            &before.objects[&circle].shape,
            &after.objects[&circle].shape,
        ) else {
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
        let id = add(
            &mut ed,
            Shape::Polyline {
                points: ring(20_000.0),
                closed: true,
            },
        );
        let first = ed.doc().objects[&id].shape.clone();
        let there = plan(ed.doc(), "EPSG:3857", &ReprojectOptions::default()).unwrap();
        ed.execute(&there.command.unwrap()).unwrap();
        let back = plan(
            ed.doc(),
            "EPSG:3067",
            &ReprojectOptions {
                origin: Some(DVec2::new(385_000.0, 6_672_000.0)),
            },
        )
        .unwrap();
        ed.execute(&back.command.unwrap()).unwrap();
        let g = ed.doc().georef.as_ref().unwrap();
        assert_eq!(g.crs, "EPSG:3067");
        assert!((g.rotation - 0.4).abs() < 1e-5, "{}", g.rotation);
        let (Shape::Polyline { points: a, .. }, Shape::Polyline { points: b, .. }) =
            (&first, &ed.doc().objects[&id].shape)
        else {
            panic!()
        };
        for (x, y) in a.iter().zip(b) {
            assert!((x.p - y.p).length() < 5.0, "{} {}", x.p, y.p);
        }
    }

    #[test]
    fn same_crs_changes_nothing_and_a_new_origin_moves_the_frame() {
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        let id = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(1000.0, 2000.0),
            },
        );
        let p = plan(ed.doc(), "EPSG:3067", &ReprojectOptions::default()).unwrap();
        assert!(p.command.is_none());
        assert_eq!(p.report.scale_factor, 1.0);
        // Same CRS with a new origin 10 m east: the rectangle moves 10 m west in the frame.
        let p = plan(
            ed.doc(),
            "EPSG:3067",
            &ReprojectOptions {
                origin: Some(DVec2::new(385_010.0, 6_672_000.0)),
            },
        )
        .unwrap();
        ed.execute(&p.command.unwrap()).unwrap();
        let Shape::Rect { min, max } = ed.doc().objects[&id].shape else {
            panic!()
        };
        assert!((min - DVec2::new(-10_000.0, 0.0)).length() < 1e-6);
        assert!((max - DVec2::new(-9000.0, 2000.0)).length() < 1e-6);
        assert!(
            plan(
                ed.doc(),
                "EPSG:3067",
                &ReprojectOptions {
                    origin: Some(DVec2::NAN)
                }
            )
            .is_err()
        );
    }

    #[test]
    fn data_underlays_and_other_shapes_are_converted() {
        use kadakas_core::underlay::{PngData, Rgba, Underlay};
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        add(
            &mut ed,
            Shape::Hardscape {
                boundary: ring(4000.0),
                material: "stone".into(),
                thickness: None,
                elevation: None,
            },
        );
        add(
            &mut ed,
            Shape::Massing(Box::new(kadakas_core::massing::Massing::new(
                ring(8000.0),
                6000.0,
            ))),
        );
        add(
            &mut ed,
            Shape::Text {
                at: DVec2::new(10.0, 10.0),
                text: "x".into(),
                height: 100.0,
            },
        );
        add(
            &mut ed,
            Shape::Locus3d {
                p: kadakas_core::DVec3::new(5.0, 5.0, 42.0),
            },
        );
        add(
            &mut ed,
            Shape::Plant {
                at: DVec2::ONE,
                species: "Oak".into(),
                spread: 3000.0,
                height: 5000.0,
            },
        );
        let img = Rgba {
            width: 2,
            height: 2,
            data: [255, 0, 0, 255].repeat(4),
        };
        add(
            &mut ed,
            Shape::Underlay(Box::new(Underlay {
                name: "map".into(),
                min: DVec2::ZERO,
                max: DVec2::new(100_000.0, 100_000.0),
                width: 2,
                height: 2,
                png: PngData::new(img.to_png()),
                opacity: 0.5,
                source: "src".into(),
            })),
        );
        let p = plan(ed.doc(), "EPSG:32635", &ReprojectOptions::default()).unwrap();
        ed.execute(&p.command.clone().unwrap()).unwrap();
        for k in [
            "hardscape",
            "massing",
            "text",
            "locus3d",
            "plant",
            "underlay",
        ] {
            assert_eq!(p.report.changed[k], 1, "{k}");
        }
        let l = ed
            .doc()
            .objects
            .values()
            .find(|o| matches!(o.shape, Shape::Locus3d { .. }))
            .unwrap();
        let Shape::Locus3d { p: locus } = l.shape else {
            panic!()
        };
        assert_eq!(locus.z, 42.0);
        let u = ed
            .doc()
            .objects
            .values()
            .find_map(|o| match &o.shape {
                Shape::Underlay(u) => Some(u.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (u.opacity, u.source.as_str(), u.name.as_str()),
            (0.5, "src", "map")
        );
        assert!(u.problem().is_none());
        assert!(p.report.warnings.iter().any(|w| w.contains("heights")));
    }

    #[test]
    fn extrudes_and_sweeps_follow_the_map() {
        use kadakas_core::solid3d::{Extrude, Sweep};
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        let mut e = Extrude::new(ring(4000.0), 500.0);
        e.holes = vec![corners(&[
            DVec2::new(1000.0, 1000.0),
            DVec2::new(2000.0, 1000.0),
            DVec2::new(2000.0, 2000.0),
        ])];
        let x = add(&mut ed, Shape::Extrude(Box::new(e)));
        let mut sw = Sweep::new(corners(&[
            DVec2::new(100.0, 0.0),
            DVec2::new(600.0, 0.0),
            DVec2::new(600.0, 800.0),
        ]));
        sw.origin = kadakas_core::DVec3::new(2000.0, 3000.0, 100.0);
        sw.axis = kadakas_core::DVec3::new(1.0, 0.0, 0.0);
        let w = add(&mut ed, Shape::Sweep(Box::new(sw.clone())));
        let p = plan(ed.doc(), "EPSG:32635", &ReprojectOptions::default()).unwrap();
        ed.execute(&p.command.clone().unwrap()).unwrap();
        assert_eq!(p.report.changed["extrude"], 1);
        assert_eq!(p.report.changed["sweep"], 1);
        let Shape::Extrude(e2) = &ed.doc().object(x).unwrap().shape else {
            panic!()
        };
        assert!(e2.problem().is_none() && e2.holes.len() == 1);
        assert!((e2.height - 500.0 * p.report.scale_factor).abs() < 1e-6);
        let Shape::Sweep(s2) = &ed.doc().object(w).unwrap().shape else {
            panic!()
        };
        assert!(s2.problem().is_none());
        assert!((s2.origin.z - 100.0).abs() < 1.0e3, "{}", s2.origin.z);
        // The axis still points the same way on the map (the CRS turn is small here), and the
        // profile is as large as the scale says.
        assert!(s2.axis.truncate().normalize().dot(DVec2::X) > 0.99);
        assert!((s2.profile[1].p.x - 600.0 * p.report.scale_factor).abs() < 1e-6);
    }

    #[test]
    fn hardscape_and_landscape_data_follow() {
        use kadakas_core::hardscape::HardscapePatch;
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        ed.execute(&Command::AddHardscape {
            boundary: None,
            patch: Box::new(HardscapePatch {
                center: Some(corners(&[
                    DVec2::ZERO,
                    DVec2::new(10_000.0, 0.0),
                    DVec2::new(10_000.0, 8000.0),
                ])),
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
        let old = kadakas_core::hardscape::data(&before.objects[&id]).unwrap();
        let new = kadakas_core::hardscape::data(&ed.doc().objects[&id]).unwrap();
        for (a, b) in old.center.iter().zip(&new.center) {
            assert!((lonlat(&before, a.p) - lonlat(ed.doc(), b.p)).length() < 1e-8);
        }
    }

    #[test]
    fn refuses_what_it_cannot_do() {
        let doc = Document::default();
        assert!(
            plan(&doc, "EPSG:3067", &ReprojectOptions::default())
                .unwrap_err()
                .to_string()
                .contains("no CRS")
        );
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        assert!(
            plan(ed.doc(), "EPSG:4326", &ReprojectOptions::default())
                .unwrap_err()
                .to_string()
                .contains("projected")
        );
        assert!(plan(ed.doc(), "banana", &ReprojectOptions::default()).is_err());
        let id = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::ONE,
            },
        );
        let layer = ed.doc().objects[&id].layer;
        ed.execute(&Command::ModifyLayer {
            id: layer,
            name: None,
            visible: None,
            locked: Some(true),
            elevation: None,
            scale: None,
        })
        .unwrap();
        assert!(
            plan(ed.doc(), "EPSG:32635", &ReprojectOptions::default())
                .unwrap_err()
                .to_string()
                .contains("locked")
        );
        let mut bad = doc_in("EPSG:3067", DVec2::NAN, 0.0);
        assert!(plan(bad.doc(), "EPSG:32635", &ReprojectOptions::default()).is_err());
        let _ = (&mut bad, LayerId::from_num(0), ClassId::from_num(0));
    }

    #[test]
    fn objects_that_cannot_move_are_listed() {
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::ONE,
            },
        );
        // 800 km north is valid in TM35FIN, but in Web Mercator at 67 degrees north the distances
        // grow by more than two, and the point leaves the range of the drawing.
        let far = add(
            &mut ed,
            Shape::Line {
                a: DVec2::new(0.0, 8.0e8),
                b: DVec2::ONE,
            },
        );
        let p = plan(ed.doc(), "EPSG:3857", &ReprojectOptions::default()).unwrap();
        assert_eq!(p.report.objects_changed, 1);
        assert_eq!(p.report.skipped.len(), 1);
        assert_eq!(p.report.skipped[0].id, far.0);
        assert!(
            p.report
                .warnings
                .iter()
                .any(|w| w.contains("could not be converted"))
        );
        assert!(p.report.skipped[0].reason.contains("out of range"));
        // The rest of the plan still works.
        ed.execute(&p.command.unwrap()).unwrap();
    }

    #[test]
    fn every_kind_of_shape_is_moved_by_the_mapper() {
        use kadakas_core::DVec3;
        use kadakas_core::irrigation::{IrrigationShape, LineType};
        use kadakas_core::sheets::Viewport;
        let old = Crs::parse("EPSG:3067").unwrap();
        let new = Crs::parse("EPSG:32635").unwrap();
        let old_g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        let new_g = Georeference {
            crs: "EPSG:32635".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        let m = Mapper {
            old: &old,
            new: &new,
            old_g: &old_g,
            new_g: &new_g,
            scale: 1.5,
        };
        let moved = |mut shape: Shape| {
            let before = shape.clone();
            assert!(m.shape(&mut shape).unwrap(), "{}", before.kind());
            assert_ne!(shape, before, "{} stays where it was", before.kind());
            shape
        };
        let p = DVec2::new(1000.0, 2000.0);
        moved(Shape::Line { a: p, b: p * 2.0 });
        moved(Shape::Rect {
            min: p,
            max: p * 2.0,
        });
        let Shape::Circle { radius, .. } = moved(Shape::Circle {
            center: p,
            radius: 100.0,
        }) else {
            panic!()
        };
        assert_eq!(radius, 150.0, "the radius follows the scale factor");
        moved(Shape::Arc {
            center: p,
            radius: 100.0,
            start: 0.0,
            end: 1.0,
        });
        let mut arc = ring(1000.0);
        arc[1].kind = VertexKind::Arc { radius: 10.0 };
        let Shape::Polyline { points, .. } = moved(Shape::Polyline {
            points: arc,
            closed: true,
        }) else {
            panic!()
        };
        assert_eq!(points[1].kind, VertexKind::Arc { radius: 15.0 });
        moved(Shape::Text {
            at: p,
            text: "t".into(),
            height: 10.0,
        });
        moved(Shape::Plant {
            at: p,
            species: "Oak".into(),
            spread: 1.0,
            height: 1.0,
        });
        moved(Shape::PlantTag {
            at: p,
            anchor: p * 2.0,
            style: Default::default(),
            height: 10.0,
        });
        moved(Shape::LandscapeArea {
            boundary: ring(1000.0),
            name: "a".into(),
        });
        moved(Shape::SiteModifier {
            modifier: kadakas_core::model::ModifierKind::GradeLimit,
            boundary: ring(1000.0),
        });
        moved(Shape::Polyline3d {
            points: vec![DVec3::new(1.0, 2.0, 3.0); 3],
            closed: false,
        });
        let Shape::SiteModel { points, lines, .. } = moved(Shape::SiteModel {
            points: vec![DVec3::new(1.0, 2.0, 3.0)],
            lines: vec![kadakas_core::model::SiteLine {
                points: vec![DVec3::new(5.0, 6.0, 7.0)],
                closed: false,
            }],
            settings: Default::default(),
        }) else {
            panic!()
        };
        assert_eq!((points[0].z, lines[0].points[0].z), (3.0, 7.0));
        // A texture moves with the ground: its origin, its turn and its tile follow the map.
        let Shape::SiteModel { settings, .. } = moved(Shape::SiteModel {
            points: vec![DVec3::new(1.0, 2.0, 3.0)],
            lines: vec![],
            settings: kadakas_core::site::SiteSettings {
                surface_look: kadakas_core::site::SurfaceLook::Texture,
                texture_tile: 1000.0,
                texture_offset: [500.0, 700.0],
                ..Default::default()
            },
        }) else {
            panic!()
        };
        assert_eq!(settings.texture_tile, 1500.0);
        assert_ne!(settings.texture_offset, [500.0, 700.0]);
        assert!(settings.texture_rotation.is_finite() && settings.texture_rotation.abs() < 5.0);
        // A texture bed with its own mapping follows the map like the texture of a site model;
        // a bed with the mapping of the site model has nothing to move.
        let Shape::SiteModifier {
            modifier:
                kadakas_core::site::ModifierKind::TextureBed {
                    mapping: Some(bm), ..
                },
            ..
        } = moved(Shape::SiteModifier {
            modifier: kadakas_core::site::ModifierKind::TextureBed {
                texture: None,
                mapping: Some(kadakas_core::texture_bed::BedMapping {
                    tile: 1000.0,
                    offset: [500.0, 700.0],
                    rotation: 0.0,
                }),
            },
            boundary: ring(1000.0),
        })
        else {
            panic!()
        };
        assert_eq!(bm.tile, 1500.0);
        assert_ne!(bm.offset, [500.0, 700.0]);
        moved(Shape::SiteModifier {
            modifier: kadakas_core::site::ModifierKind::TextureBed {
                texture: None,
                mapping: None,
            },
            boundary: ring(1000.0),
        });
        moved(Shape::Symbol {
            symbol: Default::default(),
            at: p,
            rotation: 0.0,
            scale: 1.0,
            scale_y: None,
        });
        moved(Shape::Viewport(Box::new(Viewport::new(
            DVec2::ZERO,
            DVec2::splat(100.0),
            p,
            100.0,
            vec![],
        ))));
        for i in [
            IrrigationShape::Outlet {
                at: p,
                model: "m".into(),
                arc: 360.0,
                direction: 0.0,
                radius: None,
                zone: None,
            },
            IrrigationShape::Line {
                points: vec![p, p * 3.0],
                line_type: LineType::default(),
                size: "25".into(),
                material: "PE".into(),
                emitter_spacing: None,
                emitter_flow: None,
            },
        ] {
            moved(Shape::Irrigation(Box::new(i)));
        }
        // Shapes of paper space and derived shapes have no points in the model.
        let mut group = Shape::Group { name: "g".into() };
        assert!(!m.shape(&mut group).unwrap());
    }

    #[test]
    fn landscape_holes_and_a_broken_picture() {
        use kadakas_core::landscape::LandscapePatch;
        use kadakas_core::underlay::{PngData, Underlay};
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        ed.execute(&Command::AddLandscapeArea {
            boundary: ring(10_000.0),
            name: "Bed".into(),
            patch: Box::new(LandscapePatch {
                holes: Some(vec![corners(&[
                    DVec2::new(2000.0, 2000.0),
                    DVec2::new(4000.0, 2000.0),
                    DVec2::new(4000.0, 4000.0),
                ])]),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        // A picture that starts like a PNG but cannot be read stays where it is and is listed.
        add(
            &mut ed,
            Shape::Underlay(Box::new(Underlay {
                name: "broken".into(),
                min: DVec2::ZERO,
                max: DVec2::splat(1000.0),
                width: 2,
                height: 2,
                png: PngData::new(vec![0x89, b'P', b'N', b'G', 0, 0, 0, 0, 0]),
                opacity: 1.0,
                source: String::new(),
            })),
        );
        // A north arrow on the design layer moves like the rest.
        let arrow: kadakas_core::annot::Annot =
            serde_json::from_value(serde_json::json!({"type": "north_arrow", "at": [0, 0]}))
                .unwrap();
        add(&mut ed, Shape::Annotation(Box::new(arrow)));
        let before = ed.doc().clone();
        let p = plan(&before, "EPSG:32635", &ReprojectOptions::default()).unwrap();
        assert_eq!(p.report.changed["landscape_area"], 1);
        assert_eq!(p.report.skipped.len(), 1);
        assert_eq!(p.report.skipped[0].kind, "underlay");
        assert_eq!(
            p.report.changed["annotation"], 1,
            "the arrow moves with the drawing"
        );
        ed.execute(&p.command.unwrap()).unwrap();
        let id = *before.objects.keys().next().unwrap();
        let old = kadakas_core::landscape::data(&before.objects[&id]).unwrap();
        let new = kadakas_core::landscape::data(&ed.doc().objects[&id]).unwrap();
        assert_eq!(new.holes.len(), 1);
        for (a, b) in old.holes[0].iter().zip(&new.holes[0]) {
            assert!((lonlat(&before, a.p) - lonlat(ed.doc(), b.p)).length() < 1e-8);
        }
    }

    #[test]
    fn dimensions_heights_and_the_refresh_view_follow() {
        use kadakas_core::annot::Annot;
        use kadakas_core::underlay::{PngData, Rgba, Underlay};
        let mut ed = doc_in("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        let dim: Annot = serde_json::from_value(serde_json::json!({
            "type": "dimension", "mode": {"type": "linear", "axis": "aligned"}, "points": [[0, 0], [40000, 0]], "offset": 5.0
        }))
        .unwrap();
        let dim_id = add(&mut ed, Shape::Annotation(Box::new(dim)));
        let locus = add(
            &mut ed,
            Shape::Locus3d {
                p: kadakas_core::DVec3::new(10.0, 20.0, 5000.0),
            },
        );
        let img = Rgba {
            width: 2,
            height: 2,
            data: [1, 2, 3, 255].repeat(4),
        };
        let map = add(
            &mut ed,
            Shape::Underlay(Box::new(Underlay {
                name: "m".into(),
                min: DVec2::ZERO,
                max: DVec2::new(50_000.0, 50_000.0),
                width: 2,
                height: 2,
                png: PngData::new(img.to_png()),
                opacity: 1.0,
                source: String::new(),
            })),
        );
        ed.execute(&Command::SetData {
            id: map,
            set: BTreeMap::from([(
                "gis_source".to_string(),
                serde_json::json!({"kind": "wms", "source": {}, "bbox": [0.0, 0.0, 50000.0, 50000.0], "max_px": 100}),
            )]),
            remove: vec![],
        })
        .unwrap();
        let before = ed.doc().clone();
        let p = plan(&before, "EPSG:32635", &ReprojectOptions::default()).unwrap();
        let k = p.report.scale_factor;
        ed.execute(&p.command.unwrap()).unwrap();
        // The dimension spans the same two places, so it measures the new, longer or shorter, length.
        let Shape::Annotation(a) = &ed.doc().object(dim_id).unwrap().shape else {
            panic!()
        };
        let Annot::Dimension(d) = &**a else { panic!() };
        let len = d.points[0].distance(d.points[1]);
        assert!(
            (len - 40_000.0 * k).abs() < 1.0,
            "{len} against {}",
            40_000.0 * k
        );
        assert!(
            (lonlat(&before, DVec2::new(40_000.0, 0.0)) - lonlat(ed.doc(), d.points[1])).length()
                < 1e-8
        );
        assert_eq!(d.offset, 5.0, "paper offsets stay");
        // The height went through the conversion (the same datum, so it is unchanged).
        let Shape::Locus3d { p: l } = ed.doc().object(locus).unwrap().shape else {
            panic!()
        };
        assert!((l.z - 5000.0).abs() < 1e-6);
        // The view that the map was fetched for moved with the picture.
        let o = ed.doc().object(map).unwrap();
        let Shape::Underlay(u) = &o.shape else {
            panic!()
        };
        let bbox = o.data["gis_source"]["bbox"].as_array().unwrap();
        assert_eq!(bbox[0].as_f64().unwrap(), u.min.x);
        assert_eq!(bbox[3].as_f64().unwrap(), u.max.y);
        assert_eq!(o.data["gis_source"]["kind"], "wms");
    }
}
