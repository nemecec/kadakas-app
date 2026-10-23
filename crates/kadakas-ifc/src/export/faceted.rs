//! Surface models of the 3D modeler (MOD-12) as faceted solids.
//!
//! A surface model is written as `IfcFacetedBrep` for each closed solid (the shells that
//! `SurfaceModel::solid_report` finds closed, with the fronts of the faces looking out). Every
//! other face goes into an open shell of an `IfcShellBasedSurfaceModel`, and a note says so. A
//! face with holes keeps them (`IfcFaceBound` for each hole). The faces are not cut into
//! triangles. When a model has both solids and open parts, everything is written as a surface
//! model, because a Brep representation holds solids only.

use kadakas_core::surface::{FaceId, ShellState, SurfaceModel};

use super::*;

impl<W: Write> Exporter<'_, W> {
    pub(super) fn surface_object(&mut self, obj: &Object) -> Outcome {
        let Shape::SurfaceModel(m) = &obj.shape else {
            return skip("not a surface model");
        };
        if let Some(why) = m.problem() {
            return skip(&why);
        }
        let report = m.solid_report();
        let mut solids: Vec<Vec<FaceId>> = Vec::new();
        let mut open: Vec<FaceId> = Vec::new();
        for shell in &report.shells {
            let faces: Vec<FaceId> = shell
                .faces
                .iter()
                .copied()
                .filter(|f| !m.face_hidden(*f))
                .collect();
            if faces.is_empty() {
                continue;
            }
            // A shell with a hidden face is not closed any more.
            if shell.state == ShellState::Solid && faces.len() == shell.faces.len() {
                solids.push(faces);
            } else {
                open.extend(faces);
            }
        }
        if solids.is_empty() && open.is_empty() {
            return skip("surface model without a visible face");
        }
        let name = if m.name.trim().is_empty() {
            self.object_name(obj)
        } else {
            m.name.trim().to_string()
        };
        let placement = self.place_in_layer(obj.layer, [0.0; 3])?;
        let lift = self.doc.layer(obj.layer).map_or(0.0, |l| l.elevation) - self.base_z(obj.layer);
        let mut points: std::collections::BTreeMap<kadakas_core::surface::VertexId, u64> =
            std::collections::BTreeMap::new();
        let flip = m.transform.matrix3.determinant() < 0.0;
        let as_surface = !open.is_empty();
        let mut items: Vec<u64> = Vec::new();
        let mut shells: Vec<u64> = Vec::new();
        for faces in &solids {
            let shell = self.faceted_shell(m, faces, lift, flip, &mut points, "IFCCLOSEDSHELL")?;
            if as_surface {
                shells.push(shell);
            } else {
                items.push(self.ent("IFCFACETEDBREP", vec![Value::Ref(shell)])?);
            }
        }
        if as_surface {
            if !open.is_empty() {
                shells.push(self.faceted_shell(
                    m,
                    &open,
                    lift,
                    flip,
                    &mut points,
                    "IFCOPENSHELL",
                )?);
            }
            items.push(self.ent("IFCSHELLBASEDSURFACEMODEL", vec![Value::refs(shells)])?);
            self.note(format!(
                "surface model {}: {} faces are not in a closed solid, so the model is an IfcShellBasedSurfaceModel (open shell) and not an IfcFacetedBrep{}",
                obj.id,
                open.len(),
                if solids.is_empty() {
                    String::new()
                } else {
                    format!(", with its {} closed solids as closed shells", solids.len())
                }
            ));
        }
        let default = obj.attrs.fill.map_or(MESH_GREY, |c| c.0);
        self.style_items(&items, self.color_of(obj, default))?;
        let rep = self.body_rep(if as_surface { "SurfaceModel" } else { "Brep" }, &items)?;
        self.element(
            ElementIn {
                obj,
                kind: "surface_model",
                name,
                placement,
                body: Body {
                    reps: vec![rep],
                    main: Some(rep),
                },
            },
            &datamap::evaluate(self.doc, &self.map, obj),
        )?;
        Ok(None)
    }

    /// One shell of faces: `IFCCLOSEDSHELL` or `IFCOPENSHELL`.
    fn faceted_shell(
        &mut self,
        m: &SurfaceModel,
        faces: &[FaceId],
        lift: f64,
        flip: bool,
        points: &mut std::collections::BTreeMap<kadakas_core::surface::VertexId, u64>,
        class: &str,
    ) -> R<u64> {
        let mut out: Vec<u64> = Vec::with_capacity(faces.len());
        for fid in faces {
            let Some(f) = m.face(*fid) else { continue };
            let mut bounds: Vec<u64> = Vec::new();
            for (k, l) in f.loops().enumerate() {
                let mut ids: Vec<u64> = Vec::with_capacity(l.len());
                for v in l {
                    let id = match points.get(v) {
                        Some(id) => *id,
                        None => {
                            let local = m.vertex(*v).unwrap_or_default();
                            let q = m.transform.transform_point3(local);
                            let id = self.pt3([q.x, q.y, q.z + lift])?;
                            points.insert(*v, id);
                            id
                        }
                    };
                    ids.push(id);
                }
                // A mirror turns every loop over: reverse them to keep the fronts outward.
                if flip {
                    ids.reverse();
                }
                let poly = self.ent("IFCPOLYLOOP", vec![Value::refs(ids)])?;
                let class = if k == 0 {
                    "IFCFACEOUTERBOUND"
                } else {
                    "IFCFACEBOUND"
                };
                bounds.push(self.ent(class, vec![Value::Ref(poly), Value::Bool(true)])?);
            }
            out.push(self.ent("IFCFACE", vec![Value::refs(bounds)])?);
        }
        self.ent(class, vec![Value::refs(out)])
    }
}
