//! Massing models: `IfcBuildingElementProxy` by default, or `IfcBuilding` with the option
//! `massing_as_building`.
//!
//! A flat roof is an extruded solid. A gable or hip roof is a triangulated closed solid. The
//! base sits on the site model at the lowest ground height under the footprint (as in the 3D view).

use super::*;
use cadektor_core::massing::{Massing, RoofStyle};

impl<W: Write> Exporter<'_, W> {
    pub(super) fn massing(&mut self, obj: &Object) -> Outcome {
        let Shape::Massing(m) = &obj.shape else {
            return skip("not a massing model");
        };
        if let Some(why) = m.problem() {
            return skip(&why);
        }
        let ring = geom::clean_ring(&m.ring());
        if ring.is_empty() {
            return skip("the footprint has no area");
        }
        let base = self.ground.massing_base(obj, m);
        let mapped = datamap::evaluate(self.doc, &self.map, obj);
        let q = m.quantities();
        let eave = m.eave_height();

        let (item, rep_type) = if m.roof == RoofStyle::Flat || m.pitch_deg <= 0.0 {
            let profile = self.profile(&ring, &[])?;
            (self.extrude(profile, 0.0, eave)?, "SweptSolid")
        } else {
            let solid = m.solid(0.0, true);
            let idx: Vec<u32> = solid.tris.iter().flatten().copied().collect();
            (self.tessellation(&solid.positions, &idx)?, "Tessellation")
        };
        self.style_items(
            &[item],
            self.color_of(obj, cadektor_core::massing::DEFAULT_COLOR),
        )?;
        let rep = self.body_rep(rep_type, &[item])?;
        let name = if m.name.trim().is_empty() {
            self.object_name(obj)
        } else {
            m.name.trim().to_string()
        };
        let as_building = self.opts.massing_as_building;
        if as_building {
            // An IfcBuilding in the site. It has a placement of its own, relative to the site.
            let z = base + self.doc.layer(obj.layer).map_or(0.0, |l| l.elevation);
            let placement = self.placement(Some(self.site_placement), [0.0, 0.0, z])?;
            let shape = self.ent(
                "IFCPRODUCTDEFINITIONSHAPE",
                vec![Value::Null, Value::Null, Value::refs([rep])],
            )?;
            let id = self.ent(
                "IFCBUILDING",
                vec![
                    self.guid("obj", obj.id.0),
                    Value::Null,
                    Self::name_value(&name),
                    Value::Null,
                    Value::s("MASSING"),
                    Value::Ref(placement),
                    Value::Ref(shape),
                    Value::Null,
                    Value::e("ELEMENT"),
                    Value::Real(r4(z)),
                    Value::Null,
                    Value::Null,
                ],
            )?;
            self.elements.insert(obj.id.0, id);
            self.massing_buildings.push(id);
            if self.opts.classes_as_layers {
                self.class_layers.entry(obj.class).or_default().push(rep);
            }
        } else {
            let placement = self.place_in_layer(obj.layer, [0.0, 0.0, base])?;
            self.element(
                ElementIn {
                    obj,
                    kind: "massing",
                    name,
                    placement,
                    body: Body {
                        reps: vec![rep],
                        main: Some(rep),
                    },
                },
                &mapped,
            )?;
        }

        let common = vec![
            (
                "IsExternal".to_string(),
                Value::typed("IFCBOOLEAN", Value::Bool(true)),
            ),
            (
                "Reference".to_string(),
                Value::typed("IFCIDENTIFIER", Value::s("Massing")),
            ),
        ];
        if as_building {
            self.pset(
                "Pset_BuildingCommon",
                vec![(
                    "NumberOfStoreys".to_string(),
                    Value::typed("IFCINTEGER", Value::Int(i64::from(q.floors))),
                )],
            );
        } else {
            self.pset("Pset_BuildingElementProxyCommon", common);
        }
        self.pset("Cadektor_Massing", massing_props(m));
        let standard = vec![
            ("length", "Height".to_string(), q.ridge_height),
            ("length", "EavesHeight".to_string(), q.eave_height),
            (
                "area",
                "FootprintArea".to_string(),
                q.footprint_area / 1.0e6,
            ),
            (
                "area",
                "GrossFloorArea".to_string(),
                q.gross_floor_area / 1.0e6,
            ),
            ("volume", "GrossVolume".to_string(), q.volume / 1.0e9),
        ];
        if as_building {
            self.quantities(obj, "Qto_BuildingBaseQuantities", &standard)?;
        } else {
            self.quantities(obj, "Cadektor_MassingQuantities", &standard)?;
        }
        self.data_psets(obj, &mapped)?;
        Ok(None)
    }
}

fn massing_props(m: &Massing) -> Vec<(String, Value)> {
    vec![
        (
            "RoofStyle".to_string(),
            Value::typed("IFCLABEL", Value::s(m.roof.name())),
        ),
        (
            "RoofPitch".to_string(),
            Value::typed(
                "IFCPLANEANGLEMEASURE",
                Value::Real(r4(m.pitch_deg.to_radians())),
            ),
        ),
        (
            "Floors".to_string(),
            Value::typed("IFCINTEGER", Value::Int(i64::from(m.floor_count()))),
        ),
        (
            "FloorHeight".to_string(),
            Value::typed("IFCLENGTHMEASURE", Value::Real(r4(m.floor_height))),
        ),
    ]
}
