//! Commands for hardscapes and landscape areas. They are part of `Command`, and run inside `Tx`.

use super::*;
use crate::hardscape::{self, HardscapePatch};
use crate::landscape::{self, LandscapePatch};

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    fn check_hardscape_style(&self, patch: &HardscapePatch) -> Result<(), CommandError> {
        match patch.style {
            Some(id) if self.doc.resources.hardscape_style(id).is_none() => {
                Err(if self.doc.resources.get(id).is_some() { invalid("the resource is not a hardscape style") } else { CommandError::NoResource(id) })
            }
            _ => Ok(()),
        }
    }

    /// A path hardscape gets its outline from its center line. Its outline must not be edited directly.
    pub(super) fn guard_path(&self, id: ObjectId, new_shape: Option<&Shape>) -> Result<(), CommandError> {
        let Some(o) = self.doc.object(id) else { return Ok(()) };
        let is_path = hardscape::data(o).is_some_and(|d| d.mode == hardscape::Mode::Path);
        let changes = match (new_shape, &o.shape) {
            (Some(Shape::Hardscape { boundary: b, .. }), Shape::Hardscape { boundary: old, .. }) => b != old,
            (Some(_), _) => true,
            (None, _) => true,
        };
        if is_path && changes {
            return Err(invalid("a path hardscape gets its outline from its center line. Change the center line or the width with set_hardscape"));
        }
        Ok(())
    }

    /// The holes of a landscape area must stay inside its boundary when the boundary is edited.
    pub(super) fn check_holes(&self, id: ObjectId, boundary: &[Vertex]) -> Result<(), CommandError> {
        let Some(d) = self.doc.object(id).and_then(landscape::data) else { return Ok(()) };
        if landscape::holes_fit(boundary, &d.holes) {
            Ok(())
        } else {
            Err(invalid("the area has holes that would lie outside the new boundary. Move or clear the holes first"))
        }
    }

    pub(super) fn run_area(&mut self, cmd: &Command, out: &mut Outcome) -> Result<(), CommandError> {
        match cmd {
            Command::AddHardscape { boundary, patch, layer, class, attrs } => {
                let layer = layer.unwrap_or(self.doc.active_layer);
                let class = class.unwrap_or(self.doc.active_class);
                self.check_layer(layer)?;
                self.check_class(class)?;
                validate_attrs(attrs)?;
                self.check_hardscape_style(patch)?;
                if boundary.is_none() && patch.center.is_none() {
                    return Err(invalid("give a boundary (slab) or a center line (path)"));
                }
                if boundary.is_some() && patch.center.is_some() {
                    return Err(invalid("give a boundary or a center line, not both"));
                }
                // A path gets its real outline from the patch.
                let start = boundary.clone().unwrap_or_else(|| crate::curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]));
                let shape = Shape::Hardscape { boundary: start, material: "Paving".into(), thickness: None, elevation: None };
                validate_shape(&shape)?;
                self.check_sheet_rules(layer, &shape, None)?;
                let id = ObjectId(self.alloc()?);
                let mut o = Object::new(id, layer, class, shape);
                o.attrs = attrs.clone();
                hardscape::data::apply_patch(self.doc, &mut o, patch).map_err(invalid)?;
                validate_shape(&o.shape)?;
                self.added.push(id);
                self.added_set.insert(id);
                self.doc.objects.insert(id, o);
                out.created.push(id.0);
            }
            Command::AddLandscapeArea { boundary, name, patch, layer, class, attrs } => {
                let layer = layer.unwrap_or(self.doc.active_layer);
                let class = class.unwrap_or(self.doc.active_class);
                self.check_layer(layer)?;
                self.check_class(class)?;
                validate_attrs(attrs)?;
                if let Some(id) = patch.style
                    && self.doc.resources.landscape_area_style(id).is_none()
                {
                    return Err(if self.doc.resources.get(id).is_some() { invalid("the resource is not a landscape area style") } else { CommandError::NoResource(id) });
                }
                let shape = Shape::LandscapeArea { boundary: boundary.clone(), name: name.clone() };
                validate_shape(&shape)?;
                self.check_sheet_rules(layer, &shape, None)?;
                let id = ObjectId(self.alloc()?);
                let mut o = Object::new(id, layer, class, shape);
                o.attrs = attrs.clone();
                if **patch != LandscapePatch::default() {
                    landscape::apply_patch(&mut o, patch).map_err(invalid)?;
                }
                validate_shape(&o.shape)?;
                self.added.push(id);
                self.added_set.insert(id);
                self.doc.objects.insert(id, o);
                out.created.push(id.0);
            }
            Command::SetHardscape { ids, patch } => {
                if patch.is_empty() {
                    return Err(invalid("give at least one setting to change"));
                }
                self.check_hardscape_style(patch)?;
                let ids = unique(ids);
                let mut plan = Vec::new();
                for &id in &ids {
                    self.editable(id)?;
                    let mut o = self.doc.object(id).expect("checked").clone();
                    hardscape::data::apply_patch(self.doc, &mut o, patch).map_err(invalid)?;
                    validate_shape(&o.shape)?;
                    plan.push(o);
                }
                for o in plan {
                    let id = o.id;
                    self.touch(id);
                    *self.doc.object_mut(id).expect("checked") = o;
                }
            }
            Command::SetLandscapeArea { ids, patch } => {
                if *patch == LandscapePatch::default() {
                    return Err(invalid("give at least one setting to change"));
                }
                if let Some(id) = patch.style
                    && self.doc.resources.landscape_area_style(id).is_none()
                {
                    return Err(if self.doc.resources.get(id).is_some() { invalid("the resource is not a landscape area style") } else { CommandError::NoResource(id) });
                }
                let ids = unique(ids);
                let mut plan = Vec::new();
                for &id in &ids {
                    self.editable(id)?;
                    let mut o = self.doc.object(id).expect("checked").clone();
                    landscape::apply_patch(&mut o, patch).map_err(invalid)?;
                    plan.push(o);
                }
                for o in plan {
                    let id = o.id;
                    self.touch(id);
                    *self.doc.object_mut(id).expect("checked") = o;
                }
            }
            _ => unreachable!("run_area is called for hardscape and landscape commands only"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
