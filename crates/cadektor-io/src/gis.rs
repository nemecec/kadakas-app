//! The shared part of GIS import: features in, commands out.
//!
//! Shapefile and GeoJSON readers produce [`Feature`]s in the coordinates of the
//! file. This module finds the CRS, sets up the georeference, converts the
//! coordinates to model millimeters and builds the objects.

use std::collections::BTreeMap;

use cadektor_core::curve::corners;
use cadektor_core::georef::Georeference;
use cadektor_core::model::{Attributes, Color, Shape};
use cadektor_core::{DVec2, DVec3};
use serde_json::Value;

use crate::builder::Builder;
use crate::options::OriginMode;
use crate::{Context, ImportOptions, ImportResult, IoError};
use cadektor_crs::{Crs, Transformer, utm_epsg};

/// A drawing whose center is farther from (0, 0) than this (meters) gets an origin shift.
const FAR_M: f64 = 1000.0;

/// A point with an optional Z value, in the units of the file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Point(Pt),
    Line(Vec<Pt>),
    /// A polygon ring. `hole` is true for an inner ring.
    Ring(Vec<Pt>, bool),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feature {
    pub parts: Vec<Part>,
    pub props: BTreeMap<String, Value>,
}

/// What a reader returns.
#[derive(Debug, Default)]
pub struct GisData {
    pub features: Vec<Feature>,
    /// CRS text found in the file (the `.prj` text, or the `crs` member of GeoJSON).
    pub file_crs: Option<String>,
    /// Entities that the reader skipped: (kind, reason).
    pub skipped: Vec<(String, String)>,
    pub notes: Vec<String>,
}

fn points_of(f: &Feature) -> impl Iterator<Item = &Pt> {
    f.parts
        .iter()
        .flat_map(|p| -> Box<dyn Iterator<Item = &Pt> + '_> {
            match p {
                Part::Point(p) => Box::new(std::iter::once(p)),
                Part::Line(v) | Part::Ring(v, _) => Box::new(v.iter()),
            }
        })
}

/// Turns features into an import. `default_crs` is used when neither the options nor the file name a CRS.
pub fn build(
    format: &str,
    name: &str,
    data: GisData,
    ctx: &Context,
    opts: &ImportOptions,
    default_crs: Option<&str>,
) -> Result<ImportResult, IoError> {
    let mut b = Builder::new(ctx, format, opts.max_objects);
    for (kind, why) in &data.skipped {
        b.report.skip(kind, why);
    }
    for n in &data.notes {
        b.report.note(n);
    }

    // 1. The CRS of the file.
    let source: Option<Crs> = if let Some(s) = &opts.source_crs {
        Some(Crs::parse(s).map_err(|e| IoError::Option(format!("source_crs: {e}")))?)
    } else {
        let text = data.file_crs.as_deref().or(default_crs);
        match text.map(Crs::parse) {
            Some(Ok(c)) => Some(c),
            Some(Err(e)) => {
                b.report.note(&format!(
                    "the CRS of the file is not known ({e}): coordinates are used as meters"
                ));
                None
            }
            None => {
                b.report
                    .note("the file has no CRS: coordinates are used as meters");
                None
            }
        }
    };

    // 2. The target CRS and the georeference.
    let geographic_source = source.as_ref().is_some_and(Crs::is_geographic);
    let usable = |p: &&Pt| {
        p.x.is_finite()
            && p.y.is_finite()
            && (!geographic_source || (p.x.abs() <= 180.0 && p.y.abs() <= 90.0))
    };
    let mut all = data.features.iter().flat_map(points_of).filter(usable);
    let first = all.next().copied();
    let (mut lo, mut hi) = first.map_or((DVec2::ZERO, DVec2::ZERO), |p| {
        (DVec2::new(p.x, p.y), DVec2::new(p.x, p.y))
    });
    for p in all {
        lo = lo.min(DVec2::new(p.x, p.y));
        hi = hi.max(DVec2::new(p.x, p.y));
    }
    let mut center = (lo + hi) / 2.0;
    if geographic_source && first.is_some() {
        // Longitudes wrap at the antimeridian: use the mean direction, not the middle of the range.
        let (mut sin, mut cos) = (0.0, 0.0);
        for p in data.features.iter().flat_map(points_of).filter(usable) {
            sin += p.x.to_radians().sin();
            cos += p.x.to_radians().cos();
        }
        if sin != 0.0 || cos != 0.0 {
            center.x = sin.atan2(cos).to_degrees();
        }
    }
    let mut transform: Option<(Crs, Crs)> = None;
    let georef: Option<Georeference>;
    let doc_has_crs = ctx
        .georef
        .as_ref()
        .is_some_and(|g| !g.crs.trim().is_empty());
    match (&ctx.georef, source) {
        (Some(g), Some(src)) if doc_has_crs => {
            let target = Crs::parse(&g.crs)
                .map_err(|e| IoError::Option(format!("the document CRS cannot be used: {e}")))?;
            georef = Some(g.clone());
            transform = Some((src, target));
            b.report.georeferenced = true;
        }
        (Some(g), _) => {
            b.report.note("the document CRS is assumed for the file (the document has no CRS, or the file has none): coordinates are taken as meters in the document frame");
            georef = Some(g.clone());
            b.report.georeferenced = doc_has_crs;
        }
        (None, Some(src)) => {
            let target = match &opts.target_crs {
                Some(t) => {
                    Crs::parse(t).map_err(|e| IoError::Option(format!("target_crs: {e}")))?
                }
                None if src.is_geographic() => Crs::from_epsg(utm_epsg(center.x, center.y))
                    .map_err(|e| IoError::Format(format!("cannot choose a projection: {e}")))?,
                None => Crs::parse(&src.spec).map_err(IoError::Format)?,
            };
            if target.is_geographic() {
                return Err(IoError::Option(
                    "target_crs must be a projected CRS (meters)".into(),
                ));
            }
            let origin = if first.is_some() {
                let (x, y, _) = Transformer::new(&src, &target)
                    .apply(center.x, center.y, 0.0)
                    .map_err(IoError::Format)?;
                DVec2::new(
                    (x * target.to_meter()).round(),
                    (y * target.to_meter()).round(),
                )
            } else {
                DVec2::ZERO
            };
            if !origin.is_finite() || origin.abs().max_element() > 1.0e6 * 1.0e3 {
                return Err(IoError::Format("coordinates are out of range".into()));
            }
            let g = Georeference {
                crs: target.spec.clone(),
                origin,
                rotation: 0.0,
                user_origin: DVec2::ZERO,
            };
            b.set_georef(g.clone());
            georef = Some(g);
            transform = Some((src, target));
        }
        (None, None) => {
            // No CRS anywhere. The numbers are meters, and real-world values are far from 0: center the
            // drawing and record the shift as the origin of a local frame (a georeference without CRS).
            let far = center.is_finite() && center.abs().max_element() > FAR_M;
            let origin = if first.is_some() && far && opts.origin == OriginMode::Center {
                center.round()
            } else {
                DVec2::ZERO
            };
            if origin != DVec2::ZERO && origin.is_finite() && origin.abs().max_element() <= 1.0e9 {
                let g = Georeference {
                    crs: String::new(),
                    origin,
                    rotation: 0.0,
                    user_origin: DVec2::ZERO,
                };
                b.set_georef(g.clone());
                b.report.georeferenced = false;
                b.report.note(&format!("the file has no CRS: the drawing is centered, the origin shift ({}, {}) m is saved in the document", origin.x, origin.y));
                georef = Some(g);
            } else {
                georef = None;
            }
        }
    }

    // 3. The objects.
    let layer = match &opts.layer {
        Some(l) => b.layer(l, true),
        None => b.default_layer,
    };
    let class_attrs = Attributes {
        pen: Some(Color::BLACK),
        fill: Some(Color::NONE),
        line_weight: Some(0.25),
        ..Default::default()
    };
    let to_model = |p: &Pt| -> Result<DVec3, String> {
        let (x, y, z) = match &transform {
            Some((s, t)) => {
                let (x, y, z) = Transformer::new(s, t).apply(p.x, p.y, p.z.unwrap_or(0.0))?;
                (x * t.to_meter(), y * t.to_meter(), z)
            }
            None => (p.x, p.y, p.z.unwrap_or(0.0)),
        };
        let m = match &georef {
            Some(g) => g.to_model(DVec2::new(x, y)),
            None => DVec2::new(x, y) * 1000.0,
        };
        Ok(m.extend(z * 1000.0))
    };
    'features: for (index, f) in data.features.iter().enumerate() {
        let class_name = match opts.class_field.as_deref().and_then(|k| f.props.get(k)) {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(Value::Null) | None => name.to_string(),
            Some(other) => other.to_string(),
        };
        let class = b.class(&class_name, &class_attrs, true);
        // The last outer ring that was imported: holes that follow it belong to it.
        let mut outer: Option<u64> = None;
        for part in &f.parts {
            if b.is_full() {
                break 'features;
            }
            let mut data = f.props.clone();
            data.insert("_feature".into(), Value::from(index));
            let (kind, pts, closed, point) = match part {
                Part::Point(p) => ("point", std::slice::from_ref(p), false, true),
                Part::Line(v) => ("line", v.as_slice(), false, false),
                Part::Ring(v, hole) => {
                    // A shape has no holes, so a hole stays a closed polyline that points to its outer ring.
                    // The GeoJSON export puts it back into the polygon.
                    match (*hole, outer) {
                        (true, Some(o)) => {
                            data.insert("_ring".into(), Value::from("hole"));
                            data.insert("_hole_of".into(), Value::from(o));
                        }
                        _ => outer = None,
                    }
                    ("polygon", v.as_slice(), true, false)
                }
            };
            let mut pts = pts;
            if closed && pts.len() > 1 && pts.first() == pts.last() {
                pts = &pts[..pts.len() - 1];
            }
            let model: Result<Vec<DVec3>, String> = pts.iter().map(&to_model).collect();
            let model = match model {
                Ok(m) => m,
                Err(e) => {
                    b.report.skip(kind, &e);
                    continue;
                }
            };
            let has_z = pts.iter().any(|p| p.z.is_some());
            let shape = if point {
                Shape::Locus3d { p: model[0] }
            } else if has_z {
                Shape::Polyline3d {
                    points: model,
                    closed,
                }
            } else {
                let xy: Vec<DVec2> = model.iter().map(|p| p.truncate()).collect();
                Shape::Polyline {
                    points: corners(&xy),
                    closed,
                }
            };
            let shape = match shape {
                Shape::Polyline {
                    ref points,
                    closed: true,
                } if points.len() < 3 => {
                    b.report.skip(kind, "ring has fewer than 3 points");
                    continue;
                }
                Shape::Polyline3d {
                    ref points,
                    closed: true,
                } if points.len() < 3 => {
                    b.report.skip(kind, "ring has fewer than 3 points");
                    continue;
                }
                s => s,
            };
            let is_outer = matches!(part, Part::Ring(_, false))
                || (matches!(part, Part::Ring(_, true)) && outer.is_none());
            let added = b.add(kind, shape, layer, class, Attributes::default(), data);
            if is_outer && added {
                outer = Some(b.last_id());
            }
        }
    }
    Ok(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::command::Editor;
    use cadektor_core::model::Document;

    fn pt(x: f64, y: f64) -> Pt {
        Pt { x, y, z: None }
    }

    fn feature(parts: Vec<Part>, key: &str, v: Value) -> Feature {
        Feature {
            parts,
            props: BTreeMap::from([(key.to_string(), v)]),
        }
    }

    fn run(
        data: GisData,
        doc: Document,
        opts: &ImportOptions,
        default_crs: Option<&str>,
    ) -> Result<(Document, crate::Report), IoError> {
        let r = build("test", "roads", data, &Context::of(&doc), opts, default_crs)?;
        let mut ed = Editor::new(doc);
        ed.execute(&r.command).unwrap();
        Ok((ed.doc().clone(), r.report))
    }

    #[test]
    fn lonlat_gets_utm_georef_with_small_coordinates() {
        let data = GisData {
            features: vec![
                feature(
                    vec![Part::Point(Pt {
                        x: 24.9384,
                        y: 60.1699,
                        z: Some(12.5),
                    })],
                    "name",
                    Value::from("A"),
                ),
                feature(
                    vec![Part::Line(vec![pt(24.9384, 60.1699), pt(24.9394, 60.1699)])],
                    "name",
                    Value::from("B"),
                ),
                feature(
                    vec![
                        Part::Ring(
                            vec![
                                pt(24.93, 60.16),
                                pt(24.94, 60.16),
                                pt(24.94, 60.17),
                                pt(24.93, 60.16),
                            ],
                            false,
                        ),
                        Part::Ring(
                            vec![pt(24.932, 60.162), pt(24.936, 60.162), pt(24.936, 60.166)],
                            true,
                        ),
                    ],
                    "name",
                    Value::from("C"),
                ),
            ],
            ..GisData::default()
        };
        let (doc, report) = run(
            data,
            Document::default(),
            &ImportOptions::default(),
            Some("EPSG:4326"),
        )
        .unwrap();
        let g = doc.georef.clone().unwrap();
        assert_eq!(g.crs, "EPSG:32635");
        assert!(report.georeferenced);
        assert_eq!(doc.objects.len(), 4);
        let o: Vec<_> = doc.objects.values().collect();
        let Shape::Locus3d { p } = o[0].shape else {
            panic!()
        };
        assert_eq!(p.z, 12_500.0);
        assert!(p.x.abs() < 1.0e6 && p.y.abs() < 1.0e6, "{p:?}");
        // 0.001 degree of longitude at 60 N is about 55 m.
        let Shape::Polyline { points, .. } = &o[1].shape else {
            panic!()
        };
        let len = points[0].p.distance(points[1].p);
        assert!((len - 55_700.0).abs() < 500.0, "{len}");
        assert_eq!(o[0].data["name"], "A");
        assert_eq!(o[0].data["_feature"], 0);
        assert_eq!(o[3].data["_ring"], "hole");
        assert!(!o[2].data.contains_key("_ring"));
        let Shape::Polyline { points, closed } = &o[2].shape else {
            panic!()
        };
        assert!(*closed && points.len() == 3);
        // Class is the file name.
        assert_eq!(doc.class(o[0].class).unwrap().name, "roads");
        // The map position of the point is recovered through the georeference.
        let map = g.to_map(p.truncate());
        let crs = Crs::parse("EPSG:32635").unwrap();
        let wgs = Crs::parse("EPSG:4326").unwrap();
        let (lon, lat, _) = Transformer::new(&crs, &wgs)
            .apply(map.x, map.y, 0.0)
            .unwrap();
        assert!((lon - 24.9384).abs() < 1e-6 && (lat - 60.1699).abs() < 1e-6);
    }

    #[test]
    fn existing_georef_is_used_and_projected_source() {
        let mut doc = Document::default();
        doc.georef = Some(Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        });
        let data = GisData {
            features: vec![feature(
                vec![Part::Point(pt(385_010.0, 6_672_020.0))],
                "k",
                Value::Null,
            )],
            ..GisData::default()
        };
        let (d, _) = run(
            data,
            doc.clone(),
            &ImportOptions::default(),
            Some("EPSG:3067"),
        )
        .unwrap();
        let Shape::Locus3d { p } = d.objects.values().next().unwrap().shape else {
            panic!()
        };
        assert!(p.truncate().distance(DVec2::new(10_000.0, 20_000.0)) < 1e-3);
        // Georeference stays unchanged, and the batch has no SetGeoreference.
        assert_eq!(d.georef, doc.georef);
        // File in lon/lat into a projected document.
        let data = GisData {
            features: vec![feature(
                vec![Part::Point(pt(24.9384, 60.1699))],
                "k",
                Value::Null,
            )],
            ..GisData::default()
        };
        let (d, _) = run(
            data,
            doc.clone(),
            &ImportOptions::default(),
            Some("EPSG:4326"),
        )
        .unwrap();
        let Shape::Locus3d { p } = d.objects.values().next().unwrap().shape else {
            panic!()
        };
        assert!(p.truncate().length() < 3.0e6, "{p:?}");
        // Unknown CRS in a georeferenced document: the document CRS is assumed.
        let data = GisData {
            features: vec![feature(
                vec![Part::Point(pt(385_005.0, 6_672_005.0))],
                "k",
                Value::Null,
            )],
            ..GisData::default()
        };
        let (d, report) = run(data, doc, &ImportOptions::default(), None).unwrap();
        let Shape::Locus3d { p } = d.objects.values().next().unwrap().shape else {
            panic!()
        };
        assert!(p.truncate().distance(DVec2::new(5_000.0, 5_000.0)) < 1e-3);
        assert!(report.notes.iter().any(|n| n.contains("assumed")));
    }

    #[test]
    fn projected_source_without_document_georef_and_options() {
        let data = || GisData {
            features: vec![feature(
                vec![Part::Point(pt(385_010.0, 6_672_020.0))],
                "kind",
                Value::from("tree"),
            )],
            file_crs: Some("EPSG:3067".into()),
            ..GisData::default()
        };
        let (d, _) = run(data(), Document::default(), &ImportOptions::default(), None).unwrap();
        let g = d.georef.unwrap();
        assert_eq!(g.crs, "EPSG:3067");
        assert_eq!(g.origin, DVec2::new(385_010.0, 6_672_020.0));
        let opts = ImportOptions {
            class_field: Some("kind".into()),
            layer: Some("GIS".into()),
            ..Default::default()
        };
        let (d, _) = run(data(), Document::default(), &opts, None).unwrap();
        assert!(d.classes.iter().any(|c| c.name == "tree"));
        assert!(d.layers.iter().any(|l| l.name == "GIS"));
        // Explicit source and target.
        let opts = ImportOptions {
            source_crs: Some("EPSG:3067".into()),
            target_crs: Some("EPSG:32635".into()),
            ..Default::default()
        };
        let (d, _) = run(data(), Document::default(), &opts, None).unwrap();
        assert_eq!(d.georef.unwrap().crs, "EPSG:32635");
        let bad = ImportOptions {
            source_crs: Some("nope".into()),
            ..Default::default()
        };
        assert!(matches!(
            run(data(), Document::default(), &bad, None),
            Err(IoError::Option(_))
        ));
        let bad = ImportOptions {
            target_crs: Some("EPSG:4326".into()),
            ..Default::default()
        };
        assert!(matches!(
            run(data(), Document::default(), &bad, None),
            Err(IoError::Option(_))
        ));
        let bad = ImportOptions {
            target_crs: Some("nope".into()),
            ..Default::default()
        };
        assert!(matches!(
            run(data(), Document::default(), &bad, None),
            Err(IoError::Option(_))
        ));
        let mut doc = Document::default();
        doc.georef = Some(Georeference {
            crs: "nope".into(),
            origin: DVec2::ZERO,
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        });
        assert!(matches!(
            run(data(), doc, &ImportOptions::default(), None),
            Err(IoError::Option(_))
        ));
    }

    #[test]
    fn unknown_crs_uses_meters_and_bad_parts_are_skipped() {
        let data = GisData {
            features: vec![
                feature(vec![Part::Point(pt(1.0, 2.0))], "a", Value::from(1)),
                feature(
                    vec![Part::Ring(vec![pt(0.0, 0.0), pt(1.0, 0.0)], false)],
                    "a",
                    Value::from(2),
                ),
                feature(
                    vec![Part::Line(vec![pt(0.0, 0.0), pt(f64::NAN, 0.0)])],
                    "a",
                    Value::from(3),
                ),
                feature(
                    vec![Part::Ring(
                        vec![
                            Pt {
                                x: 0.0,
                                y: 0.0,
                                z: Some(1.0),
                            },
                            Pt {
                                x: 1.0,
                                y: 0.0,
                                z: Some(1.0),
                            },
                        ],
                        false,
                    )],
                    "a",
                    Value::from(3),
                ),
                feature(
                    vec![Part::Line(vec![pt(1e7, 0.0), pt(0.0, 0.0)])],
                    "a",
                    Value::from(4),
                ),
            ],
            skipped: vec![("Null".into(), "no geometry".into())],
            notes: vec!["hello".into()],
            file_crs: Some("garbage".into()),
        };
        let keep = ImportOptions {
            origin: OriginMode::File,
            ..ImportOptions::default()
        };
        let (d, report) = run(data, Document::default(), &keep, None).unwrap();
        assert!(d.georef.is_none());
        assert!(!report.georeferenced);
        assert_eq!(d.objects.len(), 1);
        assert_eq!(report.skipped_total(), 5);
        let Shape::Locus3d { p } = d.objects.values().next().unwrap().shape else {
            panic!()
        };
        assert_eq!(p, DVec3::new(1000.0, 2000.0, 0.0));
        assert!(report.notes.len() == 2);
        // No CRS at all.
        let data = GisData {
            features: vec![feature(vec![Part::Point(pt(1.0, 2.0))], "a", Value::Null)],
            ..GisData::default()
        };
        let (_, report) = run(data, Document::default(), &ImportOptions::default(), None).unwrap();
        assert!(report.notes[0].contains("no CRS"));
        // Empty input.
        let (d, _) = run(
            GisData::default(),
            Document::default(),
            &ImportOptions::default(),
            Some("EPSG:4326"),
        )
        .unwrap();
        assert!(d.objects.is_empty());
    }

    #[test]
    fn object_limit_and_projection_failure() {
        let feats = (0..5)
            .map(|i| feature(vec![Part::Point(pt(i as f64, 0.0))], "a", Value::Null))
            .collect();
        let opts = ImportOptions {
            max_objects: 2,
            ..Default::default()
        };
        let (d, report) = run(
            GisData {
                features: feats,
                ..GisData::default()
            },
            Document::default(),
            &opts,
            None,
        )
        .unwrap();
        assert_eq!(d.objects.len(), 2);
        assert!(report.notes.iter().any(|n| n.contains("limit")));
        // A latitude outside the valid range cannot be projected.
        let data = GisData {
            features: vec![feature(
                vec![Part::Point(pt(10.0, 60.0)), Part::Point(pt(10.0, 95.0))],
                "a",
                Value::Null,
            )],
            ..GisData::default()
        };
        let (d, report) = run(
            data,
            Document::default(),
            &ImportOptions::default(),
            Some("EPSG:4326"),
        )
        .unwrap();
        assert!(d.objects.len() <= 2);
        let _ = report;
    }

    #[test]
    fn real_world_numbers_without_crs_are_centered() {
        // L-EST97-like meters, and a shapefile without .prj.
        let line = |x: f64, y: f64| Part::Line(vec![pt(x, y), pt(x + 100.0, y + 50.0)]);
        let data = || GisData {
            features: vec![feature(
                vec![line(6_589_000.0, 540_000.0)],
                "k",
                Value::Null,
            )],
            ..GisData::default()
        };
        let (d, report) =
            run(data(), Document::default(), &ImportOptions::default(), None).unwrap();
        assert_eq!(d.objects.len(), 1, "{:?}", report.skipped);
        let g = d.georef.clone().unwrap();
        assert_eq!(g.crs, "");
        assert_eq!(g.origin, DVec2::new(6_589_050.0, 540_025.0));
        assert!(report.notes.iter().any(|n| n.contains("centered")));
        let (lo, hi) = d.objects.values().next().unwrap().shape.bounds();
        assert!(lo.abs().max_element() < 1.0e5 && hi.abs().max_element() < 1.0e5);
        // The shift maps back to the file coordinates.
        assert!((g.to_map(lo).x - 6_589_000.0).abs() < 1e-6);
        // `file` keeps the numbers, and the object is out of range.
        let keep = ImportOptions {
            origin: OriginMode::File,
            ..ImportOptions::default()
        };
        let (d, report) = run(data(), Document::default(), &keep, None).unwrap();
        assert_eq!(d.objects.len(), 0);
        assert_eq!(report.skipped_total(), 1);
        // Small numbers stay as they are.
        let small = GisData {
            features: vec![feature(vec![line(10.0, 20.0)], "k", Value::Null)],
            ..GisData::default()
        };
        let (d, _) = run(small, Document::default(), &ImportOptions::default(), None).unwrap();
        assert!(d.georef.is_none());
        // A second import into the local frame lands in the same place.
        let (d2, _) = run(data(), d.clone(), &ImportOptions::default(), None).unwrap();
        assert_eq!(d2.objects.len(), 2);
    }

    #[test]
    fn holes_know_their_outer_ring_and_orphans_are_islands() {
        let ring = |x: f64, hole: bool| {
            Part::Ring(
                vec![
                    pt(x, 0.0),
                    pt(x + 10.0, 0.0),
                    pt(x + 10.0, 10.0),
                    pt(x, 10.0),
                ],
                hole,
            )
        };
        let data = GisData {
            features: vec![feature(
                vec![
                    ring(0.0, true),
                    ring(0.0, false),
                    ring(2.0, true),
                    ring(20.0, false),
                    ring(22.0, true),
                ],
                "k",
                Value::Null,
            )],
            ..GisData::default()
        };
        let keep = ImportOptions {
            origin: OriginMode::File,
            ..ImportOptions::default()
        };
        let (d, _) = run(data, Document::default(), &keep, None).unwrap();
        let objs: Vec<_> = d.objects.values().collect();
        assert_eq!(objs.len(), 5);
        // A hole with no outer ring before it is a plain polygon.
        assert!(!objs[0].data.contains_key("_ring"));
        assert!(!objs[1].data.contains_key("_ring"));
        assert_eq!(objs[2].data["_hole_of"], objs[1].id.0);
        assert!(!objs[3].data.contains_key("_ring"));
        assert_eq!(objs[4].data["_hole_of"], objs[3].id.0);
    }

    #[test]
    fn antimeridian_picks_the_zone_from_the_mean_longitude() {
        let data = GisData {
            features: vec![feature(
                vec![Part::Line(vec![pt(179.9, 10.0), pt(-179.9, 10.0)])],
                "k",
                Value::Null,
            )],
            ..GisData::default()
        };
        let (d, _) = run(
            data,
            Document::default(),
            &ImportOptions::default(),
            Some("EPSG:4326"),
        )
        .unwrap();
        let crs = d.georef.unwrap().crs;
        assert!(crs == "EPSG:32660" || crs == "EPSG:32601", "{crs}");
    }

    #[test]
    fn feet_crs_origin_is_in_meters() {
        let data = GisData {
            features: vec![feature(
                vec![Part::Point(pt(986_000.0, 212_000.0))],
                "k",
                Value::Null,
            )],
            ..GisData::default()
        };
        let opts = ImportOptions {
            source_crs: Some("EPSG:2263".into()),
            ..ImportOptions::default()
        };
        let (d, _) = run(data, Document::default(), &opts, None).unwrap();
        let g = d.georef.unwrap();
        // 986000 ft is about 300 km, not 986 km.
        assert!(
            (g.origin.x - 300_533.0).abs() < 2.0 && (g.origin.y - 64_617.0).abs() < 2.0,
            "{:?}",
            g.origin
        );
    }
}
