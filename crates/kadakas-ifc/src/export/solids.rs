//! Extrudes and sweeps (3D-03): `IfcBuildingElementProxy` (or the class of the data map).
//!
//! An extrude with its profile is an `IfcExtrudedAreaSolid` with `IfcArbitraryProfileDefWithVoids`
//! when there are holes (curves are cut into lines). A sweep is a triangulated solid.

use super::*;
use kadakas_core::solid3d::{self, ring_points};

impl<W: Write> Exporter<'_, W> {
    pub(super) fn solid(&mut self, obj: &Object) -> Outcome {
        if let Some(why) = obj.shape.problem() {
            return skip(&why);
        }
        let mapped = datamap::evaluate(self.doc, &self.map, obj);
        let placement = self.place_in_layer(obj.layer, [0.0; 3])?;
        let default = solid3d::DEFAULT_COLOR;
        let (item, rep_type, kind, name, quantities) = match &obj.shape {
            Shape::Extrude(e) => {
                let outer = ring_points(&e.profile);
                let holes: Vec<Vec<DVec2>> = e.holes.iter().map(|h| ring_points(h)).collect();
                let profile = self.profile(&outer, &holes)?;
                let (z0, z1) = e.z_range();
                let item = self.extrude(profile, z0, z1 - z0)?;
                let area = e.profile_area();
                let q = vec![
                    ("length", "Height".to_string(), (z1 - z0) / 1e3),
                    ("area", "FootprintArea".to_string(), area / 1e6),
                    ("volume", "GrossVolume".to_string(), area * (z1 - z0) / 1e9),
                ];
                (item, "SweptSolid", "extrude", e.name.clone(), q)
            }
            Shape::Sweep(s) => {
                let Some(m) = s.mesh() else {
                    return skip("the sweep has no mesh");
                };
                let idx: Vec<u32> = m.triangles.iter().flatten().copied().collect();
                let item = self.tessellation(&m.vertices, &idx)?;
                let mut q = vec![("area", "SurfaceArea".to_string(), m.area() / 1e6)];
                if s.closed {
                    q.push(("volume", "GrossVolume".to_string(), m.volume() / 1e9));
                }
                (item, "Tessellation", "sweep", s.name.clone(), q)
            }
            _ => return skip("not an extrude or a sweep"),
        };
        self.style_items(&[item], self.color_of(obj, default))?;
        let rep = self.body_rep(rep_type, &[item])?;
        let name = if name.trim().is_empty() {
            self.object_name(obj)
        } else {
            name.trim().to_string()
        };
        self.element(
            ElementIn {
                obj,
                kind,
                name,
                placement,
                body: Body {
                    reps: vec![rep],
                    main: Some(rep),
                },
            },
            &mapped,
        )?;
        self.quantities(obj, "Kadakas_SolidQuantities", &quantities)?;
        self.data_psets(obj, &mapped)?;
        Ok(None)
    }
}
