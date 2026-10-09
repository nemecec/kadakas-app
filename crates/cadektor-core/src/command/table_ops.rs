//! Commands for worksheet tables placed in the drawing (see `crate::worksheet_table`).

use super::*;
use crate::worksheet_table::{self as table, TableData};

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

fn check_size(size: DVec2) -> Result<(), CommandError> {
    if !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 || size.max_element() > table::MAX_SIZE {
        return Err(invalid("the size of a table must be positive and finite"));
    }
    Ok(())
}

impl Tx<'_> {
    pub(super) fn run_table(&mut self, cmd: &Command, out: &mut Outcome) -> Result<(), CommandError> {
        match cmd {
            Command::AddWorksheetTable { worksheet, at, size, text_height, layer, class, attrs } => {
                let layer = layer.unwrap_or(self.doc.active_layer);
                let class = class.unwrap_or(self.doc.active_class);
                self.check_layer(layer)?;
                self.check_class(class)?;
                validate_attrs(attrs)?;
                check_size(*size)?;
                if !at.is_finite() || at.abs().max_element() > MAX_COORD {
                    return Err(invalid(NOT_FINITE));
                }
                let d = TableData { worksheet: *worksheet, text_height: text_height.unwrap_or(table::DEFAULT_TEXT_HEIGHT) };
                table::validate(self.doc, &d).map_err(invalid)?;
                let shape = Shape::Rect { min: DVec2::new(at.x, at.y - size.y), max: DVec2::new(at.x + size.x, at.y) };
                validate_shape(&shape)?;
                self.check_sheet_rules(layer, &shape, None)?;
                let id = ObjectId(self.alloc()?);
                let mut o = Object::new(id, layer, class, shape);
                o.attrs = attrs.clone();
                table::set_data(&mut o, &d);
                self.added.push(id);
                self.added_set.insert(id);
                self.doc.objects.insert(id, o);
                out.created.push(id.0);
            }
            Command::SetWorksheetTable { ids, worksheet, text_height, size } => {
                let ids = unique(ids);
                if ids.is_empty() {
                    return Err(invalid("give at least one table"));
                }
                if let Some(s) = size {
                    check_size(*s)?;
                }
                for &id in &ids {
                    self.editable(id)?;
                    let o = self.doc.object(id).expect("checked");
                    let Some(old) = table::data(o) else { return Err(invalid(format!("object {id} is not a worksheet table"))) };
                    let new = TableData { worksheet: worksheet.unwrap_or(old.worksheet), text_height: text_height.unwrap_or(old.text_height) };
                    table::validate(self.doc, &new).map_err(invalid)?;
                }
                for &id in &ids {
                    self.touch(id);
                    let o = self.doc.object_mut(id).expect("checked");
                    let old = table::data(o).expect("checked");
                    table::set_data(o, &TableData { worksheet: worksheet.unwrap_or(old.worksheet), text_height: text_height.unwrap_or(old.text_height) });
                    if let (Some(s), Shape::Rect { min, max }) = (size, &mut o.shape) {
                        // The top left corner stays where it is.
                        let top_left = DVec2::new(min.x.min(max.x), min.y.max(max.y));
                        *min = DVec2::new(top_left.x, top_left.y - s.y);
                        *max = DVec2::new(top_left.x + s.x, top_left.y);
                    }
                }
            }
            _ => unreachable!("run_table gets table commands only"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Editor, ResourceId) {
        let mut ed = Editor::default();
        let ws = ResourceId(ed.execute(&Command::AddResource { name: "S".into(), body: ResourceBody::Worksheet(serde_json::json!({})) }).unwrap().created[0]);
        (ed, ws)
    }

    fn add(ws: ResourceId, layer: Option<LayerId>) -> Command {
        Command::AddWorksheetTable { worksheet: ws, at: DVec2::new(10.0, 100.0), size: DVec2::new(40.0, 20.0), text_height: None, layer, class: None, attrs: Attributes::default() }
    }

    #[test]
    fn add_set_undo_and_delete_rules() {
        let (mut ed, ws) = setup();
        let before = ed.doc().clone();
        let id = ObjectId(ed.execute(&add(ws, None)).unwrap().created[0]);
        let o = ed.doc().object(id).unwrap();
        assert_eq!(o.shape, Shape::Rect { min: DVec2::new(10.0, 80.0), max: DVec2::new(50.0, 100.0) });
        assert_eq!(table::data(o), Some(TableData { worksheet: ws, text_height: 3.5 }));
        assert_eq!(table::users_of(ed.doc(), ws), vec![id]);
        // The frame is resized around its top left corner.
        ed.execute(&Command::SetWorksheetTable { ids: vec![id], worksheet: None, text_height: Some(2.0), size: Some(DVec2::new(60.0, 30.0)) }).unwrap();
        let o = ed.doc().object(id).unwrap();
        assert_eq!(o.shape, Shape::Rect { min: DVec2::new(10.0, 70.0), max: DVec2::new(70.0, 100.0) });
        assert_eq!(table::data(o).unwrap().text_height, 2.0);
        // A worksheet in use cannot be deleted.
        let e = ed.execute(&Command::DeleteResource { id: ws, reassign: None }).unwrap_err();
        assert!(matches!(e, CommandError::ResourceInUse(_, 1)), "{e}");
        // SetData cannot write the key.
        let e = ed.execute(&Command::SetData { id, set: [(table::KEY.to_string(), serde_json::json!({}))].into(), remove: vec![] }).unwrap_err();
        assert!(e.to_string().contains("reserved"), "{e}");
        assert!(ed.undo() && ed.undo());
        assert_eq!(ed.doc().objects.len(), before.objects.len());
        assert!(ed.redo());
    }

    #[test]
    fn tables_follow_the_sheet_rules() {
        let (mut ed, ws) = setup();
        ed.execute(&Command::AddSheet { name: "A1".into(), paper: None, orientation: None, number: None, title: None }).unwrap();
        let sheet = ed.doc().sheet_layers().next().unwrap().id;
        // On a sheet layer and on a design layer.
        ed.execute(&add(ws, Some(sheet))).unwrap();
        ed.execute(&add(ws, None)).unwrap();
        // Locked layers refuse.
        ed.execute(&Command::ModifyLayer { id: sheet, name: None, visible: None, locked: Some(true), elevation: None, scale: None }).unwrap();
        assert!(matches!(ed.execute(&add(ws, Some(sheet))).unwrap_err(), CommandError::LayerLocked(_)));
        assert!(matches!(ed.execute(&add(ws, Some(LayerId(999)))).unwrap_err(), CommandError::NoLayer(_)));
    }

    #[test]
    fn errors_change_nothing() {
        let (mut ed, ws) = setup();
        let id = ObjectId(ed.execute(&add(ws, None)).unwrap().created[0]);
        let plain = ObjectId(ed.execute(&Command::AddObject { shape: Shape::Rect { min: DVec2::ZERO, max: DVec2::ONE }, layer: None, class: None, attrs: Attributes::default() }).unwrap().created[0]);
        let snap = ed.doc().clone();
        let err = |ed: &mut Editor, c: Command| ed.execute(&c).unwrap_err().to_string();
        let with = |f: &dyn Fn(&mut Command)| {
            let mut c = add(ws, None);
            f(&mut c);
            c
        };
        assert!(err(&mut ed, with(&|c| if let Command::AddWorksheetTable { size, .. } = c { *size = DVec2::new(0.0, 5.0) })).contains("size"));
        assert!(err(&mut ed, with(&|c| if let Command::AddWorksheetTable { size, .. } = c { *size = DVec2::new(f64::NAN, 5.0) })).contains("size"));
        assert!(err(&mut ed, with(&|c| if let Command::AddWorksheetTable { at, .. } = c { *at = DVec2::new(f64::INFINITY, 0.0) })).contains("finite"));
        assert!(err(&mut ed, with(&|c| if let Command::AddWorksheetTable { text_height, .. } = c { *text_height = Some(0.0) })).contains("text height"));
        assert!(err(&mut ed, with(&|c| if let Command::AddWorksheetTable { worksheet, .. } = c { *worksheet = ResourceId(999) })).contains("does not exist"));
        let set = |ids: Vec<ObjectId>, size: Option<DVec2>, ws: Option<ResourceId>| Command::SetWorksheetTable { ids, worksheet: ws, text_height: None, size };
        assert!(err(&mut ed, set(vec![], None, None)).contains("at least one"));
        assert!(err(&mut ed, set(vec![plain], None, None)).contains("not a worksheet table"));
        assert!(err(&mut ed, set(vec![id], Some(DVec2::new(-1.0, 1.0)), None)).contains("size"));
        assert!(err(&mut ed, set(vec![id], None, Some(ResourceId(77)))).contains("does not exist"));
        assert!(err(&mut ed, set(vec![ObjectId(555)], None, None)).contains("does not exist"));
        assert_eq!(*ed.doc(), snap);
    }
}
