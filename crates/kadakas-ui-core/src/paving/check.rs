//! The pattern check (PAT-10): `unit_pattern.check` lays the units of each hardscape with its
//! zones and courses and tests them. This module asks for it, reads the findings into rows and
//! says where to look for each one.

use std::collections::BTreeMap;

use glam::DVec2;
use kadakas_core::layout::check::Kind;
use kadakas_core::model::{Object, ObjectId, Shape};
use kadakas_l10n::tr;
use serde_json::{Value, json};

use super::{Backend, xy};

/// The label of a kind of finding.
pub fn kind_label(id: &str) -> String {
    match id {
        "overlap" => tr!("uic-paving-kind-overlap"),
        "joint_too_narrow" => tr!("uic-paving-kind-joint-narrow"),
        "joint_too_wide" => tr!("uic-paving-kind-joint-wide"),
        "small_piece" => tr!("uic-paving-kind-small-piece"),
        "wrong_size" => tr!("uic-paving-kind-wrong-size"),
        "outside_area" => tr!("uic-paving-kind-outside"),
        "coverage" => tr!("uic-paving-kind-coverage"),
        other => other.to_string(),
    }
}

/// The ids of the kinds, in the order of the list.
pub fn kinds() -> Vec<&'static str> {
    Kind::ALL.iter().map(|k| k.id()).collect()
}

/// One fault, with its place.
#[derive(Clone, Debug, PartialEq)]
pub struct FindingRow {
    /// The hardscape it is in.
    pub object: Option<ObjectId>,
    pub kind: String,
    pub group: String,
    pub at: DVec2,
    pub size: f64,
    /// The text of the API (for agents; it is shown inside a sentence).
    pub message: String,
}

impl FindingRow {
    /// A box (x0, y0, x1, y1) to zoom to: the place with room round it. The room is the size of
    /// the finding when that is a length, else 600 mm, so that the units round it show.
    pub fn zoom_box(&self) -> [f64; 4] {
        let r = match self.kind.as_str() {
            "joint_too_narrow" | "joint_too_wide" => 400.0,
            _ => 600.0,
        };
        [self.at.x - r, self.at.y - r, self.at.x + r, self.at.y + r]
    }

    /// The size in the unit of its kind.
    pub fn size_text(&self) -> String {
        let u = kadakas_units::current();
        match self.kind.as_str() {
            "joint_too_narrow" | "joint_too_wide" => u.length_text_d(self.size, 1),
            "overlap" | "outside_area" | "wrong_size" => u.area_text(self.size),
            _ => kadakas_units::current().number_text(self.size, 2),
        }
    }
}

/// The result for one hardscape.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectCheck {
    pub id: Option<ObjectId>,
    pub name: String,
    pub units: usize,
    /// The exact count of each kind (the list may be cut).
    pub counts: BTreeMap<String, usize>,
    pub findings: Vec<FindingRow>,
    pub truncated: Vec<String>,
    pub error: Option<String>,
    pub notes: Vec<String>,
}

impl ObjectCheck {
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }
}

/// The reply of `unit_pattern.check`.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckResult {
    pub objects: Vec<ObjectCheck>,
    pub units: usize,
    /// No finding and no error.
    pub ok: bool,
    /// Every hardscape could be checked.
    pub complete: bool,
}

impl CheckResult {
    pub fn parse(reply: &Value) -> Result<CheckResult, String> {
        let rows = reply
            .get("objects")
            .and_then(Value::as_array)
            .ok_or("the reply has no objects")?;
        let objects = rows
            .iter()
            .map(|r| {
                let id: Option<ObjectId> = r
                    .get("id")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                let counts = r
                    .get("counts")
                    .and_then(Value::as_object)
                    .map(|m| {
                        m.iter()
                            .map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0) as usize))
                            .collect()
                    })
                    .unwrap_or_default();
                let findings = r
                    .get("findings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|f| {
                        Some(FindingRow {
                            object: id,
                            kind: f.get("kind")?.as_str()?.to_string(),
                            group: f
                                .get("group")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                            at: xy(f.get("at")?)?,
                            size: f.get("size").and_then(Value::as_f64).unwrap_or(0.0),
                            message: f
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect();
                let strings = |k: &str| -> Vec<String> {
                    r.get(k)
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                };
                ObjectCheck {
                    id,
                    name: r
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    units: r.get("units").and_then(Value::as_u64).unwrap_or(0) as usize,
                    counts,
                    findings,
                    truncated: strings("truncated"),
                    error: r.get("error").and_then(Value::as_str).map(str::to_string),
                    notes: strings("notes"),
                }
            })
            .collect();
        let totals = reply.get("totals").cloned().unwrap_or(Value::Null);
        Ok(CheckResult {
            objects,
            units: totals.get("units").and_then(Value::as_u64).unwrap_or(0) as usize,
            ok: totals.get("ok").and_then(Value::as_bool).unwrap_or(false),
            complete: totals
                .get("complete")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// Every finding, in the order of the objects.
    pub fn findings(&self) -> impl Iterator<Item = &FindingRow> {
        self.objects.iter().flat_map(|o| o.findings.iter())
    }

    pub fn finding_count(&self) -> usize {
        self.objects.iter().map(ObjectCheck::total).sum()
    }

    pub fn errors(&self) -> usize {
        self.objects.iter().filter(|o| o.error.is_some()).count()
    }
}

/// The hardscapes that the check looks at: the selected ones, or when none is selected, all.
pub fn targets(be: &dyn Backend) -> Vec<ObjectId> {
    let doc = be.document();
    let is_target = |o: &Object| {
        matches!(o.shape, Shape::Hardscape { .. })
            && kadakas_core::layout::object::pattern_of(doc, o).is_some()
    };
    let chosen: Vec<ObjectId> = be
        .selection()
        .iter()
        .copied()
        .filter(|id| doc.object(*id).is_some_and(is_target))
        .collect();
    if !chosen.is_empty() {
        return chosen;
    }
    doc.objects
        .values()
        .filter(|o| is_target(o))
        .map(|o| o.id)
        .collect()
}

/// The panel of the check: the last result and the finding that the person looks at.
#[derive(Clone, Debug, Default)]
pub struct CheckPanel {
    pub result: Option<Result<CheckResult, String>>,
    /// The revision of the document that the result is for.
    pub revision: u64,
    /// The finding that was clicked, as (object index, finding index).
    pub selected: Option<(usize, usize)>,
    /// Only the findings of this kind (an id from [`kinds`]).
    pub only: Option<String>,
}

impl CheckPanel {
    /// Runs the check on the selected hardscapes, or on all of them.
    pub fn run(&mut self, be: &mut dyn Backend) {
        let ids = targets(be);
        self.revision = be.revision();
        self.selected = None;
        if ids.is_empty() {
            self.result = Some(Err(tr!("uic-paving-check-nothing")));
            return;
        }
        self.result = Some(
            be.call("unit_pattern.check", json!({"ids": ids}))
                .and_then(|r| CheckResult::parse(&r)),
        );
    }

    /// True when the drawing changed since the check ran: the findings may be old.
    pub fn stale(&self, be: &dyn Backend) -> bool {
        self.result.is_some() && self.revision != be.revision()
    }

    pub fn result(&self) -> Option<&CheckResult> {
        self.result.as_ref().and_then(|r| r.as_ref().ok())
    }

    /// The finding that is selected.
    pub fn current(&self) -> Option<&FindingRow> {
        let (o, f) = self.selected?;
        self.result()?.objects.get(o)?.findings.get(f)
    }

    /// True if a finding of this kind is shown.
    pub fn shows(&self, kind: &str) -> bool {
        self.only.as_deref().is_none_or(|k| k == kind)
    }
}

#[cfg(test)]
mod tests;
