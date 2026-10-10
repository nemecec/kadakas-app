//! Quantities of hardscapes: areas, perimeter, and the volume of every layer. The report lists
//! them per object and adds up the materials.

use std::collections::BTreeMap;

use glam::DVec2;
use serde::Serialize;

use super::TerrainSource;
use super::data::{self, HardscapeData, Mode, TOLERANCE};
use super::offset;
use super::style::HardscapeStyle;
use super::surface::{self, Terrain};
use crate::curve;
use crate::mesh::DEFAULT_SLAB_THICKNESS;
use crate::model::{Document, Object, ObjectId, Shape};
use crate::resource::ResourceId;

/// One layer of a hardscape with its quantities. `kind` is `surface`, `component`, `border`, or
/// `slab` for a hardscape without a style.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayerQty {
    pub kind: &'static str,
    pub name: String,
    pub material: String,
    pub thickness_mm: f64,
    pub area_m2: f64,
    pub volume_m3: f64,
}

/// The quantities of one hardscape.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Quantities {
    pub mode: &'static str,
    pub plan_area_m2: f64,
    /// Area of the top surface. It is larger than the plan area for a slope or a draped surface.
    pub surface_area_m2: f64,
    pub perimeter_m: f64,
    /// Total thickness of all layers.
    pub thickness_mm: f64,
    pub volume_m3: f64,
    pub draped: bool,
    /// The hardscape drapes, but only part of it lies on the site model. The rest follows the
    /// height of the nearest edge of the model.
    pub partially_draped: bool,
    pub layers: Vec<LayerQty>,
}

/// The quantities of a hardscape object. `terrain` is the ground, used when the hardscape
/// drapes. `None` for other objects.
pub fn quantities(
    doc: &Document,
    obj: &Object,
    terrain: Option<Terrain<'_>>,
) -> Option<Quantities> {
    let Shape::Hardscape {
        boundary,
        material,
        thickness,
        ..
    } = &obj.shape
    else {
        return None;
    };
    let d = data::data(obj);
    let style = data::style_of(doc, obj);
    let ring = curve::flatten(boundary, true, TOLERANCE);
    let plan = curve::area(boundary) / 1e6;
    // Ground that does not reach the hardscape does not count.
    let terrain = terrain.filter(|t| ring.iter().any(|p| t(*p).is_some()));
    let surf = surface::surface_of(obj, d.as_ref(), terrain);
    let surface_area = plan * surface::area_ratio(&ring, &surf);
    let ratio = if plan > 0.0 { surface_area / plan } else { 1.0 };
    let closed_perimeter = offset::length(&ring)
        + ring
            .last()
            .zip(ring.first())
            .map_or(0.0, |(a, b)| a.distance(*b));
    let m3 = |area: f64, t: f64| area * t / 1000.0;
    let mut layers: Vec<LayerQty> = Vec::new();
    match &style {
        Some((id, s)) => {
            let name = doc
                .resources
                .get(*id)
                .map(|r| r.name.clone())
                .unwrap_or_default();
            let border = border_area(s, d.as_ref(), plan, closed_perimeter / 1000.0);
            let net = (surface_area - border.map_or(0.0, |(_, a)| a * ratio)).max(0.0);
            layers.push(LayerQty {
                kind: "surface",
                name: name.clone(),
                material: s.label(&name),
                thickness_mm: s.thickness,
                area_m2: net,
                volume_m3: m3(net, s.thickness),
            });
            if let (Some(b), Some((_, a))) = (&s.border, border) {
                let a = a * ratio;
                layers.push(LayerQty {
                    kind: "border",
                    name: "Border".into(),
                    material: b.material.clone(),
                    thickness_mm: s.thickness,
                    area_m2: a,
                    volume_m3: m3(a, s.thickness),
                });
            }
            for c in &s.components {
                layers.push(LayerQty {
                    kind: "component",
                    name: c.name.clone(),
                    material: c.material.clone(),
                    thickness_mm: c.thickness,
                    area_m2: surface_area,
                    volume_m3: m3(surface_area, c.thickness),
                });
            }
        }
        None => {
            let t = thickness
                .filter(|t| t.is_finite() && *t > 0.0)
                .unwrap_or(DEFAULT_SLAB_THICKNESS);
            layers.push(LayerQty {
                kind: "slab",
                name: "Slab".into(),
                material: material.clone(),
                thickness_mm: t,
                area_m2: surface_area,
                volume_m3: m3(surface_area, t),
            });
        }
    }
    let total_thickness = layers
        .iter()
        .filter(|l| l.kind != "border")
        .map(|l| l.thickness_mm)
        .sum();
    let volume = layers.iter().map(|l| l.volume_m3).sum();
    Some(Quantities {
        mode: d.as_ref().map_or("slab", |d| d.mode.id()),
        plan_area_m2: plan,
        surface_area_m2: surface_area,
        perimeter_m: closed_perimeter / 1000.0,
        thickness_mm: total_thickness,
        volume_m3: volume,
        draped: surf.is_draped(),
        partially_draped: false,
        layers,
    })
}

/// Length (m) and plan area (m2) of the border, if the style has one. A slab has the border along
/// the whole outline. A path has it along the chosen sides of the center line.
fn border_area(
    s: &HardscapeStyle,
    d: Option<&HardscapeData>,
    plan: f64,
    outline_m: f64,
) -> Option<(f64, f64)> {
    let b = s.border.as_ref()?;
    let length_m = match d {
        Some(d) if d.mode == Mode::Path => {
            offset::length(&d.center_points()) / 1000.0 * b.sides.count() as f64
        }
        _ => outline_m,
    };
    Some((length_m, (length_m * b.width / 1000.0).min(plan)))
}

/// One line of the hardscape report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub id: ObjectId,
    pub name: String,
    pub style: Option<ResourceId>,
    pub style_name: Option<String>,
    pub material: String,
    #[serde(flatten)]
    pub quantities: Quantities,
}

/// The total of one material.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MaterialTotal {
    pub kind: &'static str,
    pub material: String,
    pub area_m2: f64,
    pub volume_m3: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub rows: Vec<Row>,
    pub materials: Vec<MaterialTotal>,
    pub total_plan_area_m2: f64,
    pub total_surface_area_m2: f64,
    pub total_volume_m3: f64,
}

/// The quantities of a hardscape on the given ground, with the rules of the report: a hardscape that
/// lies fully outside the site model, or on a layer without one, is not draped.
pub fn quantities_on(
    doc: &Document,
    obj: &Object,
    ground: &dyn TerrainSource,
) -> Option<Quantities> {
    if !matches!(obj.shape, Shape::Hardscape { .. }) {
        return None;
    }
    let at = |p: DVec2| ground.elevation(obj, p);
    let ring = curve::flatten(
        obj.shape.vertices().map_or(&[][..], |v| v.as_slice()),
        true,
        TOLERANCE,
    );
    let step = (ring.len() / 64).max(1);
    let samples: Vec<DVec2> = ring.iter().step_by(step).copied().collect();
    let covered = samples.iter().filter(|p| ground.covered(obj, **p)).count();
    let mut q = quantities(doc, obj, (covered > 0).then_some(&at as Terrain<'_>))?;
    q.partially_draped = q.draped && covered < samples.len();
    Some(q)
}

/// The report of all hardscapes in the document, in document order.
pub fn report(doc: &Document, ground: &dyn TerrainSource) -> Report {
    let mut rows = Vec::new();
    let mut totals: BTreeMap<(&'static str, String), (f64, f64)> = BTreeMap::new();
    for obj in doc.objects.values() {
        let Shape::Hardscape { material, .. } = &obj.shape else {
            continue;
        };
        let Some(q) = quantities_on(doc, obj, ground) else {
            continue;
        };
        let style = data::style_of(doc, obj).map(|(id, _)| id);
        let style_name = style
            .and_then(|id| doc.resources.get(id))
            .map(|r| r.name.clone());
        for l in &q.layers {
            let e = totals.entry((l.kind, l.material.clone())).or_default();
            e.0 += l.area_m2;
            e.1 += l.volume_m3;
        }
        rows.push(Row {
            id: obj.id,
            name: style_name.clone().unwrap_or_else(|| material.clone()),
            style,
            style_name,
            material: material.clone(),
            quantities: q,
        });
    }
    let materials = totals
        .into_iter()
        .map(|((kind, material), (area_m2, volume_m3))| MaterialTotal {
            kind,
            material,
            area_m2,
            volume_m3,
        })
        .collect();
    Report {
        total_plan_area_m2: rows.iter().map(|r| r.quantities.plan_area_m2).sum(),
        total_surface_area_m2: rows.iter().map(|r| r.quantities.surface_area_m2).sum(),
        total_volume_m3: rows.iter().map(|r| r.quantities.volume_m3).sum(),
        rows,
        materials,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::hardscape::NoTerrain;
    use crate::hardscape::data::{HardscapePatch, Slope};
    use crate::hardscape::style::{Border, Component, Sides};
    use crate::model::Attributes;
    use crate::resource::ResourceBody;

    fn style_cmd() -> Command {
        Command::AddResource {
            name: "Setts".into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                material: "Granite setts".into(),
                thickness: 60.0,
                components: vec![
                    Component {
                        name: "Bedding".into(),
                        material: "Sand".into(),
                        thickness: 40.0,
                        color: None,
                    },
                    Component {
                        name: "Base".into(),
                        material: "Crushed stone".into(),
                        thickness: 150.0,
                        color: None,
                    },
                ],
                border: Some(Border {
                    width: 200.0,
                    material: "Granite kerb".into(),
                    color: None,
                    sides: Sides::Both,
                }),
                ..Default::default()
            })),
        }
    }

    fn add_slab(ed: &mut Editor, w: f64, h: f64) -> ObjectId {
        let b = curve::corners(&[
            DVec2::ZERO,
            DVec2::new(w, 0.0),
            DVec2::new(w, h),
            DVec2::new(0.0, h),
        ]);
        let shape = Shape::Hardscape {
            boundary: b,
            material: "Gravel".into(),
            thickness: None,
            elevation: None,
        };
        ObjectId(
            ed.execute(&Command::AddObject {
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
    fn plain_hardscape_has_one_slab_layer() {
        let mut ed = Editor::default();
        let id = add_slab(&mut ed, 4000.0, 2500.0);
        let q = quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
        assert_eq!(q.mode, "slab");
        assert!((q.plan_area_m2 - 10.0).abs() < 1e-9 && (q.surface_area_m2 - 10.0).abs() < 1e-9);
        assert!((q.perimeter_m - 13.0).abs() < 1e-9);
        assert_eq!(q.layers.len(), 1);
        assert_eq!(q.layers[0].kind, "slab");
        assert!((q.volume_m3 - 1.5).abs() < 1e-9, "150 mm default thickness");
        assert_eq!(q.thickness_mm, 150.0);
        assert!(!q.draped);
        let line = Object::new(
            ObjectId(99),
            crate::model::LayerId(1),
            crate::model::ClassId(0),
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        assert!(quantities(ed.doc(), &line, None).is_none());
    }

    #[test]
    fn styled_hardscape_has_layer_volumes_and_a_border() {
        let mut ed = Editor::default();
        let sid = ResourceId(ed.execute(&style_cmd()).unwrap().created[0]);
        let id = add_slab(&mut ed, 4000.0, 2500.0);
        ed.execute(&Command::SetHardscape {
            ids: vec![id],
            patch: HardscapePatch {
                style: Some(sid),
                ..Default::default()
            },
        })
        .unwrap();
        let q = quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
        let by = |k: &str| q.layers.iter().filter(|l| l.kind == k).collect::<Vec<_>>();
        // Border: 13 m long and 0.2 m wide.
        let border = by("border")[0];
        assert!(
            (border.area_m2 - 2.6).abs() < 1e-9 && (border.volume_m3 - 2.6 * 0.06).abs() < 1e-9
        );
        let surface = by("surface")[0];
        assert!(
            (surface.area_m2 - 7.4).abs() < 1e-9 && (surface.volume_m3 - 7.4 * 0.06).abs() < 1e-9
        );
        let comps = by("component");
        assert_eq!(comps.len(), 2);
        assert!(
            (comps[0].volume_m3 - 10.0 * 0.04).abs() < 1e-9
                && (comps[1].volume_m3 - 10.0 * 0.15).abs() < 1e-9
        );
        assert_eq!(q.thickness_mm, 250.0);
        assert!((q.volume_m3 - (7.4 * 0.06 + 2.6 * 0.06 + 0.4 + 1.5)).abs() < 1e-9);
    }

    #[test]
    fn path_border_follows_the_chosen_sides() {
        let mut ed = Editor::default();
        let sid = ResourceId(ed.execute(&style_cmd()).unwrap().created[0]);
        let id = add_slab(&mut ed, 1000.0, 1000.0);
        let center = curve::corners(&[DVec2::ZERO, DVec2::new(10_000.0, 0.0)]);
        let patch = HardscapePatch {
            style: Some(sid),
            center: Some(center),
            width: Some(2000.0),
            ..Default::default()
        };
        ed.execute(&Command::SetHardscape {
            ids: vec![id],
            patch,
        })
        .unwrap();
        let q = quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
        assert_eq!(q.mode, "path");
        assert!((q.plan_area_m2 - 20.0).abs() < 1e-9, "length times width");
        let b = q.layers.iter().find(|l| l.kind == "border").unwrap();
        assert!((b.area_m2 - 2.0 * 10.0 * 0.2).abs() < 1e-9, "both sides");
        // One side only.
        let mut style = match &ed.doc().resources.get(sid).unwrap().body {
            ResourceBody::HardscapeStyle(s) => (**s).clone(),
            _ => panic!(),
        };
        style.border.as_mut().unwrap().sides = Sides::Left;
        ed.execute(&Command::ModifyResource {
            id: sid,
            name: None,
            body: Some(ResourceBody::HardscapeStyle(Box::new(style))),
        })
        .unwrap();
        let q = quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
        assert!(
            (q.layers
                .iter()
                .find(|l| l.kind == "border")
                .unwrap()
                .area_m2
                - 2.0)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn slope_and_terrain_raise_the_surface_area() {
        let mut ed = Editor::default();
        let id = add_slab(&mut ed, 1000.0, 1000.0);
        ed.execute(&Command::SetHardscape {
            ids: vec![id],
            patch: HardscapePatch {
                slope: Some(Slope::Cross {
                    percent: 100.0,
                    angle_deg: 0.0,
                }),
                ..Default::default()
            },
        })
        .unwrap();
        let q = quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
        assert!((q.plan_area_m2 - 1.0).abs() < 1e-9);
        assert!((q.surface_area_m2 - 2.0f64.sqrt()).abs() < 1e-9);
        // The volume follows the surface area.
        assert!((q.volume_m3 - 2.0f64.sqrt() * 0.15).abs() < 1e-9);
        // Draped on a ramp of 100 percent.
        ed.execute(&Command::SetHardscape {
            ids: vec![id],
            patch: HardscapePatch {
                slope: Some(Slope::None),
                drape: Some(true),
                ..Default::default()
            },
        })
        .unwrap();
        let ramp = |p: DVec2| Some(p.y);
        let q = quantities(ed.doc(), ed.doc().object(id).unwrap(), Some(&ramp)).unwrap();
        assert!(q.draped && (q.surface_area_m2 - 2.0f64.sqrt()).abs() < 1e-9);
        // Without terrain the drape flag changes nothing.
        let q = quantities(ed.doc(), ed.doc().object(id).unwrap(), None).unwrap();
        assert!(!q.draped && (q.surface_area_m2 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn report_adds_up_materials() {
        let mut ed = Editor::default();
        let sid = ResourceId(ed.execute(&style_cmd()).unwrap().created[0]);
        let a = add_slab(&mut ed, 4000.0, 2500.0);
        let b = add_slab(&mut ed, 1000.0, 1000.0);
        add_slab(&mut ed, 2000.0, 1000.0);
        ed.execute(&Command::SetHardscape {
            ids: vec![a, b],
            patch: HardscapePatch {
                style: Some(sid),
                ..Default::default()
            },
        })
        .unwrap();
        let r = report(ed.doc(), &NoTerrain);
        assert_eq!(r.rows.len(), 3);
        assert_eq!(r.rows[0].style_name.as_deref(), Some("Setts"));
        assert_eq!(r.rows[2].name, "Gravel");
        assert!((r.total_plan_area_m2 - 13.0).abs() < 1e-9);
        let sand = r.materials.iter().find(|m| m.material == "Sand").unwrap();
        assert!((sand.area_m2 - 11.0).abs() < 1e-9 && (sand.volume_m3 - 0.44).abs() < 1e-9);
        assert!(
            r.materials
                .iter()
                .any(|m| m.kind == "slab" && m.material == "Gravel")
        );
        assert!(
            (r.total_volume_m3 - r.rows.iter().map(|r| r.quantities.volume_m3).sum::<f64>()).abs()
                < 1e-12
        );
        assert!(r.total_surface_area_m2 >= r.total_plan_area_m2);
        assert_eq!(report(&Document::default(), &NoTerrain).rows.len(), 0);
    }
}
