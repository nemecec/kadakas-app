//! The command that edits a surface model (MOD-06). The model does the work
//! (`crate::surface::SurfaceModel::apply`); the command checks the object, records it for undo and
//! puts the new model in place, so collaboration sends it like any other shape change.

use super::*;

impl Tx<'_> {
    pub(super) fn run_surface(
        &mut self,
        id: ObjectId,
        edit: &crate::surface::Edit,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        self.editable(id)?;
        if let crate::surface::Edit::SetFaceMaterial {
            material: Some(text),
            ..
        } = edit
        {
            let known = crate::uid::Uid::parse(text)
                .is_some_and(|u| crate::present::material(self.doc, ResourceId(u)).is_some());
            if !known {
                return Err(CommandError::Invalid(format!(
                    "there is no material with the id {text}"
                )));
            }
        }
        let Some(Shape::SurfaceModel(model)) = self.doc.object(id).map(|o| &o.shape) else {
            return Err(CommandError::Invalid(format!(
                "object {id} is not a surface model"
            )));
        };
        let mut model = (**model).clone();
        let notes = model
            .apply_with_notes(edit)
            .map_err(CommandError::Surface)?;
        if let Some(problem) = model.problem() {
            return Err(CommandError::Invalid(problem));
        }
        // The notes of the edit (a material dropped in a merge) go to the result.
        out.notes
            .extend(notes.into_iter().map(|n| format!("object {id}: {n}")));
        self.touch(id);
        self.doc.object_mut(id).expect("checked").shape = Shape::SurfaceModel(Box::new(model));
        Ok(())
    }
}

impl Tx<'_> {
    /// Mesh to surface model (MOD-12): replaces the shape of a `mesh3d` object by the surface
    /// model that `surface_from_triangles` makes from it.
    pub(super) fn run_mesh_to_surface(
        &mut self,
        id: ObjectId,
        tolerance: Option<f64>,
        soften_below_angle: Option<f64>,
        merge: bool,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        self.editable(id)?;
        let obj = self.doc.object(id).ok_or(CommandError::NoObject(id))?;
        let Shape::Mesh3d(mesh) = &obj.shape else {
            return Err(CommandError::Invalid(format!(
                "object {id} is not a mesh ({}): only a mesh3d object becomes a surface model",
                obj.shape.kind()
            )));
        };
        let elevation = self.doc.layer(obj.layer).map_or(0.0, |l| l.elevation);
        let converted = crate::surface::surface_from_triangles(
            &mesh.vertices,
            &mesh.triangles,
            &crate::surface::ConvertOptions {
                tolerance,
                soften_below_angle,
                merge,
            },
        )
        .map_err(CommandError::MeshToSurface)?;
        let crate::surface::Converted {
            mut model,
            origin,
            report,
            mut notes,
        } = converted;
        // The mesh has absolute heights. The model measures them from the layer.
        model.transform = glam::DAffine3::from_translation(glam::DVec3::new(
            origin.x,
            origin.y,
            origin.z - elevation,
        ));
        model.name = mesh.name.clone();
        let color = mesh.color;
        if let Some(p) = model.problem() {
            return Err(CommandError::Invalid(p));
        }
        self.touch(id);
        let o = self.doc.object_mut(id).expect("checked");
        o.shape = Shape::SurfaceModel(Box::new(model));
        if let Some(c) = color
            && o.attrs.fill.is_none()
        {
            o.attrs.fill = Some(c);
            notes.push("the color of the mesh is now the fill of the object".into());
        }
        out.notes
            .extend(notes.into_iter().map(|n| format!("object {id}: {n}")));
        out.report = serde_json::to_value(&report).ok();
        Ok(())
    }
}
