//! Landscape walls: `IfcWall`, with the solid that the 3D view shows (the wall and its coping),
//! the layers of the wall style as materials, and standard quantities.
//!
//! A wall is a polyline with wall settings. Its solid follows the site model of its layer, the same
//! way as in the 3D scene (`kadakas_site::grading::hook::wall_of`). A wall that retains ground
//! (`site_modifier`) is an `IfcWall` of type `RETAININGWALL` (IFC4x3), or a user-defined wall with that
//! object type (IFC4 and IFC2x3 have no such type). Other walls are `SOLIDWALL` with the object type
//! `LANDSCAPEWALL`.

use super::*;
use kadakas_site::grading::{hook, wall};

impl<W: Write> Exporter<'_, W> {
    pub(super) fn wall(&mut self, obj: &Object) -> Outcome {
        let doc = self.doc;
        let Some(w) = hook::wall_of(doc, self.cache, 0, obj) else {
            return skip("the wall has no path");
        };
        let parts = wall::solids(&w);
        if parts.is_empty() {
            return skip("the wall has no length");
        }
        let style = w.data.style.and_then(|id| wall::style_of(doc, id));
        let mapped = datamap::evaluate(doc, &self.map, obj);
        let placement = self.place_in_layer(obj.layer, [0.0; 3])?;
        let mut items = Vec::new();
        for (mesh, color) in &parts {
            let item = self.tessellation(&mesh.pos, &mesh.idx)?;
            self.style_items(&[item], *color)?;
            items.push(item);
        }
        let rep = self.body_rep("Tessellation", &items)?;
        let name = w
            .style_name
            .clone()
            .unwrap_or_else(|| "Landscape wall".to_string());
        let retaining = w.data.site_modifier;
        let (id, _) = self.element(
            ElementIn {
                obj,
                kind: if retaining { "retaining_wall" } else { "wall" },
                name,
                placement,
                body: Body {
                    reps: vec![rep],
                    main: Some(rep),
                },
            },
            &mapped,
        )?;

        // The build-up of the wall as material layers: the components of the style, or one
        // layer of the material of the style.
        let layers: Vec<(String, f64)> = match &style {
            Some(s) if !s.components.is_empty() => s
                .components
                .iter()
                .map(|c| {
                    let m = if c.material.trim().is_empty() {
                        &c.name
                    } else {
                        &c.material
                    };
                    (m.clone(), c.thickness)
                })
                .collect(),
            Some(s) => vec![(s.material.clone(), w.thickness)],
            None => vec![(String::new(), w.thickness)],
        };
        let label = w.style_name.clone().unwrap_or_default();
        self.associate_layers(id, &label, &layers, false)?;

        let mut common = vec![
            (
                "IsExternal".to_string(),
                Value::typed("IFCBOOLEAN", Value::Bool(true)),
            ),
            (
                "LoadBearing".to_string(),
                Value::typed("IFCBOOLEAN", Value::Bool(retaining)),
            ),
        ];
        if !label.trim().is_empty() {
            common.push((
                "Reference".to_string(),
                Value::typed("IFCIDENTIFIER", Value::s(label.trim())),
            ));
        }
        self.pset("Pset_WallCommon", common);
        let length = |v: f64| Value::typed("IFCLENGTHMEASURE", Value::Real(r4(v)));
        let mut own = vec![
            ("Thickness".to_string(), length(w.thickness)),
            ("Embed".to_string(), length(w.data.embed)),
            (
                "TopMode".to_string(),
                Value::typed(
                    "IFCLABEL",
                    Value::s(match w.data.top {
                        wall::Top::Fixed { .. } => "fixed",
                        wall::Top::Terrain { .. } => "terrain",
                        wall::Top::Slope { .. } => "slope",
                    }),
                ),
            ),
            (
                "RetainsGround".to_string(),
                Value::typed("IFCBOOLEAN", Value::Bool(retaining)),
            ),
        ];
        if let Some(c) = &w.coping {
            own.push(("CopingHeight".to_string(), length(c.height)));
            own.push(("CopingOverhang".to_string(), length(c.overhang)));
            if !c.material.trim().is_empty() {
                own.push((
                    "CopingMaterial".to_string(),
                    Value::typed("IFCLABEL", Value::s(c.material.trim())),
                ));
            }
        }
        self.pset("Kadakas_Wall", own);

        let q = &w.quantities;
        let mm2 = 1.0e-6;
        let mm3 = 1.0e-9;
        let standard = vec![
            ("length", "Length".to_string(), q.length),
            ("length", "Width".to_string(), w.thickness),
            ("length", "Height".to_string(), q.max_height),
            (
                "area",
                "GrossFootprintArea".to_string(),
                q.length * w.thickness * mm2,
            ),
            (
                "area",
                "NetFootprintArea".to_string(),
                q.length * w.thickness * mm2,
            ),
            ("area", "GrossSideArea".to_string(), q.face_area * mm2),
            ("area", "NetSideArea".to_string(), q.face_area * mm2),
            // The body has the coping, so the volume has it too. The wall alone is in the Kadakas set.
            (
                "volume",
                "GrossVolume".to_string(),
                (q.volume + q.coping_volume) * mm3,
            ),
            (
                "volume",
                "NetVolume".to_string(),
                (q.volume + q.coping_volume) * mm3,
            ),
        ];
        self.quantities(obj, "Qto_WallBaseQuantities", &standard)?;
        let extra = vec![
            ("length", "AverageHeight".to_string(), q.avg_height),
            ("length", "MinHeight".to_string(), q.min_height),
            ("volume", "WallVolume".to_string(), q.volume * mm3),
            ("volume", "CopingVolume".to_string(), q.coping_volume * mm3),
        ];
        self.quantities(obj, "Kadakas_WallQuantities", &extra)?;
        self.data_psets(obj, &mapped)?;
        if w.partly_off_model() {
            self.note(format!(
                "wall {}: part of it is outside the site model, so its ground height there is the height of the nearest edge",
                obj.id
            ));
        }
        Ok(None)
    }
}
