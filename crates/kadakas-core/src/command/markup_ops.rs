//! Commands for markup on the view (REV-11): markups and their marks.
//!
//! Every command is one undo step, so a stroke that the person draws (`AddMark`) is one step. The
//! data is review data: no command here touches the objects, the layers or the resources.

use super::*;
use crate::markup::{
    MAX_MARKS, MAX_MARKUPS, MAX_POINTS_PER_MARKUP, MAX_POINTS_TOTAL, MAX_WIDTH, MIN_WIDTH, MarkupId,
};
use crate::uid::Uid;

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    fn markup_index(&self, id: MarkupId) -> Result<usize, CommandError> {
        self.doc
            .markups
            .position(id)
            .ok_or(CommandError::NoMarkup(id))
    }

    pub(super) fn run_markup(
        &mut self,
        cmd: &Command,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        match cmd {
            Command::AddMarkup { markup } => {
                let mut m: Markup = (**markup).clone();
                if self.doc.markups.len() >= MAX_MARKUPS {
                    return Err(invalid(format!(
                        "a document has at most {MAX_MARKUPS} markups"
                    )));
                }
                if m.id.0 == Uid::NIL {
                    m.id = MarkupId(self.alloc()?);
                } else if self.doc.markups.get(m.id).is_some() {
                    return Err(invalid(format!("markup {} exists already", m.id)));
                }
                m.prepare();
                m.validate().map_err(invalid)?;
                if self.doc.markups.point_count() + m.point_count() > MAX_POINTS_TOTAL {
                    return Err(invalid(format!(
                        "the markups of a document have at most {MAX_POINTS_TOTAL} points in all"
                    )));
                }
                out.created.push(m.id.0);
                self.doc.markups.push(m);
            }
            Command::ModifyMarkup { id, patch } => {
                let at = self.markup_index(*id)?;
                let old = self.doc.markups.iter().nth(at).expect("indexed").clone();
                let new = patch.apply(&old);
                if new == old {
                    return Ok(());
                }
                // Only what the patch changes is checked: a mark of a file that is out of range
                // does not stop a rename.
                new.validate_meta().map_err(invalid)?;
                self.doc.markups.set(at, new);
            }
            Command::DeleteMarkup { id } => {
                let at = self.markup_index(*id)?;
                self.doc.markups.remove(at);
            }
            Command::AddMark { markup, mark } => {
                let at = self.markup_index(*markup)?;
                let mut m = self.doc.markups.iter().nth(at).expect("indexed").clone();
                if m.marks.len() >= MAX_MARKS {
                    return Err(invalid(format!("a markup has at most {MAX_MARKS} marks")));
                }
                if m.point_count() + mark.points.len() > MAX_POINTS_PER_MARKUP {
                    return Err(invalid(format!(
                        "a markup has at most {MAX_POINTS_PER_MARKUP} points in all"
                    )));
                }
                if self.doc.markups.point_count() + mark.points.len() > MAX_POINTS_TOTAL {
                    return Err(invalid(format!(
                        "the markups of a document have at most {MAX_POINTS_TOTAL} points in all"
                    )));
                }
                let mut k: Mark = (**mark).clone();
                if k.id == 0 {
                    k.id = m.next_mark_id();
                } else if m.mark(k.id).is_some() {
                    return Err(invalid(format!("mark {} exists already", k.id)));
                }
                k.validate().map_err(invalid)?;
                for p in &mut k.points {
                    *p = [
                        crate::markup::round_rel(p[0]),
                        crate::markup::round_rel(p[1]),
                    ];
                }
                out.report = Some(serde_json::json!({"mark": k.id}));
                m.marks.push(k);
                self.doc.markups.set(at, m);
            }
            Command::DeleteMark { markup, mark } => {
                let at = self.markup_index(*markup)?;
                let mut m = self.doc.markups.iter().nth(at).expect("indexed").clone();
                let i = m
                    .marks
                    .iter()
                    .position(|k| k.id == *mark)
                    .ok_or(CommandError::NoMark(*markup, *mark))?;
                m.marks.remove(i);
                self.doc.markups.set(at, m);
            }
            Command::SetMarkStyle {
                markup,
                mark,
                color,
                width,
            } => {
                let at = self.markup_index(*markup)?;
                let mut m = self.doc.markups.iter().nth(at).expect("indexed").clone();
                let k = m
                    .marks
                    .iter_mut()
                    .find(|k| k.id == *mark)
                    .ok_or(CommandError::NoMark(*markup, *mark))?;
                let before = k.clone();
                if let Some(c) = color {
                    k.color = *c;
                }
                if let Some(w) = width {
                    if !(w.is_finite() && (MIN_WIDTH..=MAX_WIDTH).contains(w)) {
                        return Err(invalid(format!(
                            "the width of a mark is {MIN_WIDTH} to {MAX_WIDTH} points"
                        )));
                    }
                    k.width = *w;
                }
                if *k == before {
                    return Ok(());
                }
                self.doc.markups.set(at, m);
            }
            Command::ClearMarkup { markup } => {
                let at = self.markup_index(*markup)?;
                let mut m = self.doc.markups.iter().nth(at).expect("indexed").clone();
                if m.marks.is_empty() {
                    return Ok(());
                }
                m.marks.clear();
                self.doc.markups.set(at, m);
            }
            _ => unreachable!("not a markup command"),
        }
        Ok(())
    }
}
