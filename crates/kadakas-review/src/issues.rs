//! Review issues (REV-04): the helpers around the data model of `kadakas_core::issue`.
//!
//! The model and its commands are in the core, because the document holds them. This module has
//! what a host needs on top of it, without I/O and without a clock:
//!
//! - [`Filter`]: the selection of the list (status, assignee, label, object, text);
//! - [`time`]: ISO 8601 text of the times;
//! - [`capture_visibility`] and [`visibility_commands`]: the visible layers and classes of a
//!   viewpoint, taken from a document and put back with one batch of commands;
//! - [`pin_position`]: where an issue shows on the canvas;
//! - [`match_ifc`]: how an object ID and an IFC GlobalId go into each other.

use std::collections::BTreeMap;

use kadakas_core::change::Origin;
use kadakas_core::command::Command;
use kadakas_core::issue::{Comment, Issue, IssueId, IssueStatus, Issues, Viewpoint};
use kadakas_core::model::{ClassId, Document, LayerId, ObjectId};
use kadakas_core::uid::Uid;
use kadakas_core::visibility::VisMode;

pub mod time;

/// Which issues a list shows. An empty filter shows all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// Only these states. Empty: all.
    pub status: Vec<IssueStatus>,
    /// Only issues of this assignee (without case).
    pub assignee: Option<String>,
    /// Only issues with this label (without case).
    pub label: Option<String>,
    /// Only issues that are about this object (the anchor or a viewpoint selection).
    pub object: Option<ObjectId>,
    /// Only issues whose title or description holds this text (without case).
    pub text: Option<String>,
    /// Only issues of this origin.
    pub origin: Option<Origin>,
    /// Only issues that are not done (not resolved or closed).
    pub open_only: bool,
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl Filter {
    pub fn matches(&self, i: &Issue) -> bool {
        if !self.status.is_empty() && !self.status.contains(&i.status) {
            return false;
        }
        if self.open_only && i.status.is_done() {
            return false;
        }
        if let Some(a) = &self.assignee
            && !i.assignee.as_deref().is_some_and(|x| same(x, a))
        {
            return false;
        }
        if let Some(l) = &self.label
            && !i.labels.iter().any(|x| same(x, l))
        {
            return false;
        }
        if let Some(o) = self.object
            && !(i.objects.contains(&o) || i.viewpoints.iter().any(|v| v.selection.contains(&o)))
        {
            return false;
        }
        if let Some(t) = &self.text {
            let t = t.to_lowercase();
            if !(i.title.to_lowercase().contains(&t) || i.description.to_lowercase().contains(&t)) {
                return false;
            }
        }
        if let Some(o) = self.origin
            && i.origin != o
        {
            return false;
        }
        true
    }
}

/// The issues that match, in the order of the store (oldest first).
pub fn select<'a>(issues: &'a Issues, filter: &Filter) -> Vec<&'a Issue> {
    issues.iter().filter(|i| filter.matches(i)).collect()
}

/// The assignees and labels in use, sorted, for the filter lists of a UI.
pub fn facets(issues: &Issues) -> (Vec<String>, Vec<String>) {
    let mut assignees: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for i in issues.iter() {
        if let Some(a) = &i.assignee
            && !assignees.iter().any(|x| same(x, a))
        {
            assignees.push(a.clone());
        }
        for l in &i.labels {
            if !labels.iter().any(|x| same(x, l)) {
                labels.push(l.clone());
            }
        }
    }
    assignees.sort_by_key(|a| a.to_lowercase());
    labels.sort_by_key(|a| a.to_lowercase());
    (assignees, labels)
}

/// The visibility of all layers and classes of a document, for a viewpoint.
pub fn capture_visibility(doc: &Document) -> (BTreeMap<LayerId, bool>, BTreeMap<ClassId, bool>) {
    (
        doc.layers.iter().map(|l| (l.id, l.visible)).collect(),
        doc.classes.iter().map(|c| (c.id, c.visible)).collect(),
    )
}

/// The commands that make the layers and classes of the document show as the viewpoint says.
/// Layers and classes that are gone, or that are listed the same as they are, make no command;
/// the answer is empty when nothing differs.
pub fn visibility_commands(doc: &Document, vp: &Viewpoint) -> Vec<Command> {
    let mut show_layers = Vec::new();
    let mut hide_layers = Vec::new();
    for (id, visible) in &vp.layers {
        match doc.layer(*id) {
            Some(l) if l.visible != *visible => {
                if *visible {
                    show_layers.push(*id);
                } else {
                    hide_layers.push(*id);
                }
            }
            _ => {}
        }
    }
    let mut show_classes = Vec::new();
    let mut hide_classes = Vec::new();
    for (id, visible) in &vp.classes {
        match doc.class(*id) {
            Some(c) if c.visible != *visible => {
                if *visible {
                    show_classes.push(*id);
                } else {
                    hide_classes.push(*id);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (layers, classes, mode) in [
        (show_layers, show_classes, VisMode::Visible),
        (hide_layers, hide_classes, VisMode::Invisible),
    ] {
        if !layers.is_empty() || !classes.is_empty() {
            out.push(Command::SetVisibility {
                layers,
                classes,
                mode,
            });
        }
    }
    out
}

/// Where the pin of an issue is: its point, else the place of its first object that is in the
/// document (`position` gives it), else the center of its first plan viewpoint. The z is 0 for a
/// plan viewpoint.
pub fn pin_position(
    issue: &Issue,
    position: &dyn Fn(ObjectId) -> Option<[f64; 3]>,
) -> Option<[f64; 3]> {
    if let Some(p) = issue.point {
        return Some(p);
    }
    if let Some(p) = issue.objects.iter().find_map(|o| position(*o)) {
        return Some(p);
    }
    issue.viewpoints.iter().find_map(|v| match &v.view {
        Some(kadakas_core::issue::ViewSpec::Plan { center, .. }) => {
            Some([center[0], center[1], 0.0])
        }
        _ => None,
    })
}

/// The IFC GlobalId of an object, the same one that the IFC export writes (the 128 bits of the ID
/// in the IFC alphabet, see `kadakas_core::guid`).
pub fn ifc_guid(id: ObjectId) -> String {
    kadakas_core::guid::derive_uid("", "obj", id.0)
}

/// The object ID that an IFC GlobalId stands for. This is the inverse of [`ifc_guid`], so a
/// GlobalId from an IFC export of this model gives the object it was made from.
pub fn match_ifc(global_id: &str) -> Option<ObjectId> {
    kadakas_core::guid::decode(global_id).map(|v| ObjectId(Uid::from_u128(v)))
}

/// True if two author names are the same person: without case and spaces at the ends. An author
/// that is empty (not known) counts as the same, because there is nobody to ask for.
fn same_author(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    a.is_empty() || b.is_empty() || a.to_lowercase() == b.to_lowercase()
}

/// Why deleting an issue needs a yes from the person, or `None` when the one who deletes may do
/// it alone. Work of somebody else is never deleted silently. Somebody else is another origin (a
/// person, an agent, a script, another user) or another author with the same origin (the agent of
/// Bob is not the agent of Alice, and two people are two people). `me` and `my_author` say who
/// deletes. The rule is also for the removal of viewpoints.
pub fn consent_reason(
    issue: &Issue,
    comments: &[&Comment],
    me: Origin,
    my_author: &str,
) -> Option<String> {
    if issue.origin != me {
        return Some(format!(
            "the issue was made by origin {}, not by {}",
            issue.origin.name(),
            me.name()
        ));
    }
    if !same_author(&issue.author, my_author) {
        return Some(format!(
            "the issue was made by {}, not by {}",
            issue.author.trim(),
            my_author.trim()
        ));
    }
    let others = comments
        .iter()
        .filter(|c| c.origin != me || !same_author(&c.author, my_author))
        .count();
    (others > 0).then(|| format!("the issue has {others} comment(s) of somebody else"))
}

/// An issue in a state where it is done: sets the status to resolved or closed.
pub fn is_done(issues: &Issues, id: IssueId) -> bool {
    issues.get(id).is_some_and(|i| i.status.is_done())
}

#[cfg(test)]
mod tests;
