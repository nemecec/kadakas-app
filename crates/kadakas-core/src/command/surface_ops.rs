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
        let Some(Shape::SurfaceModel(model)) = self.doc.object(id).map(|o| &o.shape) else {
            return Err(CommandError::Invalid(format!(
                "object {id} is not a surface model"
            )));
        };
        let mut model = (**model).clone();
        let notes = model
            .apply_with_notes(edit)
            .map_err(|e| CommandError::Invalid(e.to_string()))?;
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
