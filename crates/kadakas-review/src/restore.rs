//! Restoring a version: the document that a version describes, and the history to keep.
//!
//! The caller (the API session) applies the result to the editor as one undo step
//! (`Editor::restore_content`). Before that, the current state is saved as an automatic version,
//! so a restore never loses work. If the document already has the content of the newest version,
//! it is saved already and no new version is made.

use kadakas_core::model::Document;

use crate::history::{History, Limits, NewVersion, ReviewError};
use crate::state::State;

/// What a restore gives to the caller.
#[derive(Clone, Debug)]
pub struct Restore {
    /// The document with the content of the version. Its history is not set.
    pub doc: Document,
    /// The history to keep: the old one, with the automatic version added.
    pub history: History,
    /// The automatic version that holds the state from before the restore: `Some((id, true))` for
    /// a new one, `Some((id, false))` for the newest version, which had the same content.
    pub saved: Option<(u64, bool)>,
    /// IDs of versions that the caps pushed out when the automatic version was added.
    pub dropped: Vec<u64>,
    /// Things of the version that do not fit the file format of this program and were left out or
    /// repaired (short texts).
    pub notes: Vec<String>,
}

/// Makes the restore of version `id`. `auto` describes the automatic version.
pub fn prepare(
    history: &History,
    current: &Document,
    id: u64,
    auto: NewVersion,
    limits: Limits,
) -> Result<Restore, ReviewError> {
    let state = history.state_of(id)?;
    let mut next = history.clone();
    let (saved, dropped) = if next.document_is_tip(current)? {
        let tip = next.versions().last().map(|m| m.id);
        (tip.map(|t| (t, false)), Vec::new())
    } else {
        let added = next.add(current, auto, limits)?;
        (Some((added.id, true)), added.dropped)
    };
    let built = state.build(current);
    let mut notes: Vec<String> = built
        .quarantined
        .iter()
        .map(|q| {
            format!(
                "{:?} {} was left out: {}",
                q.target.kind, q.target.id, q.reason
            )
        })
        .collect();
    notes.extend(built.repairs);
    let mut doc = built.doc;
    keep_stacking_extras(&mut doc, current);
    Ok(Restore {
        doc,
        history: next,
        saved,
        dropped,
        notes,
    })
}

type Extra = serde_json::Map<String, serde_json::Value>;

/// Removes the stacking keys that the build writes into every object and layer (the field `order`
/// in `extra`). A document that is not shared has none.
pub fn without_stacking_keys(doc: &mut Document) {
    for o in doc.objects.values_mut() {
        o.extra.remove("order");
    }
    for l in &mut doc.layers {
        l.extra.remove("order");
    }
}

/// Gives the document the stacking keys that the current one has (and none for new things), so
/// the objects that did not change compare equal and the undo step stays small.
fn keep_stacking_extras(doc: &mut Document, current: &Document) {
    fn copy(to: &mut Extra, from: Option<&Extra>) {
        match from.and_then(|m| m.get("order")) {
            Some(k) => {
                to.insert("order".into(), k.clone());
            }
            None => {
                to.remove("order");
            }
        }
    }
    for (id, o) in doc.objects.iter_mut() {
        copy(&mut o.extra, current.objects.get(id).map(|c| &c.extra));
    }
    for l in &mut doc.layers {
        copy(
            &mut l.extra,
            current
                .layers
                .iter()
                .find(|c| c.id == l.id)
                .map(|c| &c.extra),
        );
    }
}

/// True if two documents have the same content: the same things with the same properties and the
/// same stacking order in each layer. Their histories, the active layer and class, and the order
/// of classes and resources do not count.
pub fn same_content(a: &Document, b: &Document) -> bool {
    State::of(a, None) == State::of(b, None)
}

#[cfg(test)]
mod tests;
