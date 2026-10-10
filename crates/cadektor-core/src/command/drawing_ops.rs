//! Commands for sheets, project data and saved views. They run inside `Tx` like all others.

use super::*;
use crate::sheets::{MAX_PAPER, Sheet};

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    /// Called when a layer is added: a sheet layer gets page settings.
    pub(super) fn on_layer_added(&mut self, id: LayerId) {
        let Some(l) = self.doc.layer(id) else { return };
        if l.kind == LayerKind::Sheet {
            let n = self.doc.sheet_layers().count();
            let sheet = Sheet::new(n.to_string(), l.name.clone());
            self.doc.drawing.sheets.insert(id, sheet);
        }
    }

    /// Makes the page size of a title block equal to the size of its sheet.
    pub(super) fn sync_title_block(&mut self, id: ObjectId) {
        let Some(o) = self.doc.object(id) else { return };
        let (Shape::TitleBlock(tb), Some(sheet)) = (&o.shape, self.doc.sheet(o.layer)) else {
            return;
        };
        let page = sheet.size_mm();
        if tb.page != page {
            self.touch(id);
            if let Some(Shape::TitleBlock(tb)) = self.doc.object_mut(id).map(|o| &mut o.shape) {
                tb.page = page;
            }
        }
    }

    /// Viewports and title blocks belong on sheet layers. Sheet layers hold only 2D drawing
    /// shapes. A viewport shows design layers and uses classes that exist.
    pub(super) fn check_sheet_rules(
        &self,
        layer: LayerId,
        shape: &Shape,
        except: Option<ObjectId>,
    ) -> Result<(), CommandError> {
        let sheet = self
            .doc
            .layer(layer)
            .is_some_and(|l| l.kind == LayerKind::Sheet);
        let sheet_only = matches!(shape, Shape::Viewport(_) | Shape::TitleBlock(_));
        if sheet_only && !sheet {
            return Err(invalid(format!("a {} goes on a sheet layer", shape.kind())));
        }
        if sheet && !crate::sheets::allowed_on_sheet(shape) {
            return Err(invalid(format!(
                "a sheet layer cannot hold a {}. Put it on a design layer and show it in a viewport",
                shape.kind()
            )));
        }
        if let Shape::Viewport(v) = shape {
            if !v.is_plan() {
                let others = self
                    .doc
                    .objects
                    .values()
                    .filter(|o| {
                        o.layer == layer
                            && Some(o.id) != except
                            && matches!(&o.shape, Shape::Viewport(x) if !x.is_plan())
                    })
                    .count();
                if others >= crate::sheets::MAX_3D_VIEWPORTS {
                    return Err(invalid(format!(
                        "a sheet layer holds at most {} 3D viewports",
                        crate::sheets::MAX_3D_VIEWPORTS
                    )));
                }
            }
            for l in &v.layers {
                match self.doc.layer(*l) {
                    None => return Err(CommandError::NoLayer(*l)),
                    Some(x) if x.kind != LayerKind::Design => {
                        return Err(invalid(format!(
                            "layer {l} is a sheet layer. A viewport shows design layers"
                        )));
                    }
                    Some(_) => {}
                }
            }
            if let Some(l) = v
                .layer_overrides
                .keys()
                .find(|l| self.doc.layer(**l).is_none())
            {
                return Err(CommandError::NoLayer(*l));
            }
            let classes = v
                .class_overrides
                .keys()
                .copied()
                .chain(v.annotations.iter().map(|a| a.class));
            for c in classes {
                self.check_class(c)?;
            }
        }
        Ok(())
    }

    pub(super) fn run_drawing(
        &mut self,
        cmd: &Command,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        match cmd {
            Command::AddSheet {
                name,
                paper,
                orientation,
                number,
                title,
            } => {
                let id = LayerId(self.alloc()?);
                let n = self.doc.sheet_layers().count() + 1;
                let mut sheet =
                    Sheet::new(n.to_string(), title.clone().unwrap_or_else(|| name.clone()));
                sheet.paper = paper.unwrap_or(sheet.paper);
                sheet.orientation = orientation.unwrap_or(sheet.orientation);
                if let Some(num) = number {
                    sheet.number = num.clone();
                }
                if !sheet.is_valid() {
                    return Err(invalid(format!(
                        "a paper side must be 10 to {MAX_PAPER} mm"
                    )));
                }
                self.doc.layers.push(Layer {
                    extra: Default::default(),
                    id,
                    name: name.clone(),
                    kind: LayerKind::Sheet,
                    visible: true,
                    locked: false,
                    elevation: 0.0,
                    scale: 1.0,
                });
                self.doc.drawing.sheets.insert(id, sheet);
                out.created.push(id.0);
            }
            Command::SetSheet {
                layer,
                paper,
                orientation,
                number,
                title,
            } => {
                if !self
                    .doc
                    .layer(*layer)
                    .is_some_and(|l| l.kind == LayerKind::Sheet)
                {
                    return Err(invalid(format!("layer {layer} is not a sheet layer")));
                }
                let mut sheet = self.doc.sheet(*layer).expect("checked");
                if let Some(p) = paper {
                    sheet.paper = *p;
                }
                if let Some(o) = orientation {
                    sheet.orientation = *o;
                }
                if let Some(n) = number {
                    sheet.number = n.clone();
                }
                if let Some(t) = title {
                    sheet.title = t.clone();
                }
                if !sheet.is_valid() {
                    return Err(invalid(format!(
                        "a paper side must be 10 to {MAX_PAPER} mm"
                    )));
                }
                let ids: Vec<ObjectId> = self
                    .doc
                    .objects
                    .values()
                    .filter(|o| o.layer == *layer && matches!(o.shape, Shape::TitleBlock(_)))
                    .map(|o| o.id)
                    .collect();
                self.doc.drawing.sheets.insert(*layer, sheet);
                for id in ids {
                    self.sync_title_block(id);
                }
            }
            Command::SetProject {
                name,
                client,
                address,
                drawn_by,
                date,
                revision,
            } => {
                let p = &mut self.doc.drawing.project;
                for (field, value) in [
                    (&mut p.name, name),
                    (&mut p.client, client),
                    (&mut p.address, address),
                    (&mut p.drawn_by, drawn_by),
                    (&mut p.date, date),
                    (&mut p.revision, revision),
                ] {
                    if let Some(v) = value {
                        *field = v.clone();
                    }
                }
            }
            Command::SetProjectGuid { guid } => {
                if guid.len() != 22
                    || !guid
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
                    || !matches!(guid.as_bytes()[0], b'0'..=b'3')
                {
                    return Err(invalid(
                        "a project GUID has 22 characters of 0-9, A-Z, a-z, _ and $, and starts with 0 to 3",
                    ));
                }
                self.doc.drawing.project.guid = guid.clone();
            }
            Command::SaveView { view } => {
                let name = view.name.trim().to_string();
                if name.is_empty() {
                    return Err(invalid("a saved view needs a name"));
                }
                if !view.is_finite() {
                    return Err(invalid("numbers must be finite"));
                }
                if let Some(l) = view.layers.keys().find(|l| self.doc.layer(**l).is_none()) {
                    return Err(CommandError::NoLayer(*l));
                }
                if let Some(c) = view.classes.keys().find(|c| self.doc.class(**c).is_none()) {
                    return Err(CommandError::NoClass(*c));
                }
                if let Some(l) = view.active_layer {
                    self.doc.layer(l).ok_or(CommandError::NoLayer(l))?;
                }
                if let Some(c) = view.active_class {
                    self.check_class(c)?;
                }
                let mut view = (**view).clone();
                view.name = name.clone();
                let views = &mut self.doc.drawing.saved_views;
                match views
                    .iter()
                    .position(|v| v.name.eq_ignore_ascii_case(&name))
                {
                    Some(i) => views[i] = view,
                    None => views.push(view),
                }
            }
            Command::DeleteView { name } => {
                let views = &mut self.doc.drawing.saved_views;
                let i = views
                    .iter()
                    .position(|v| v.name.eq_ignore_ascii_case(name.trim()))
                    .ok_or_else(|| invalid(format!("no saved view named '{name}'")))?;
                views.remove(i);
            }
            Command::RestoreView { name } => {
                let view = self
                    .doc
                    .drawing
                    .saved_view(name)
                    .cloned()
                    .ok_or_else(|| invalid(format!("no saved view named '{name}'")))?;
                for l in &mut self.doc.layers {
                    if let Some(v) = view.layers.get(&l.id) {
                        l.visible = *v;
                    }
                }
                for c in &mut self.doc.classes {
                    if let Some(v) = view.classes.get(&c.id) {
                        c.visible = *v;
                    }
                }
                if let Some(l) = view.active_layer.filter(|l| self.doc.layer(*l).is_some()) {
                    self.doc.active_layer = l;
                }
                if let Some(c) = view.active_class.filter(|c| self.doc.class(*c).is_some()) {
                    self.doc.active_class = c;
                }
            }
            _ => unreachable!("run_drawing gets drawing commands only"),
        }
        Ok(())
    }
}
