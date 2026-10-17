//! The counts of whole and cut units of one hardscape in Object Info. They are the numbers of
//! `report.unit_pattern` for that object (PAT-04): the call is the same, so the panel, the
//! report and the DXF export agree.

use kadakas_core::hardscape;
use kadakas_core::layout::object::pattern_of;
use kadakas_core::model::{Document, ObjectId, Shape};
use serde_json::{Value, json};

use super::Backend;

/// The most units that Object Info counts without being asked. More than this takes noticeable
/// time on each edit, so the person presses a button.
pub const AUTO_UNITS: f64 = 20_000.0;

/// One unit type.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeCount {
    pub name: String,
    pub whole: usize,
    pub cut: usize,
    pub order_units: usize,
}

/// One part of the hardscape: the field, a zone or a course.
#[derive(Clone, Debug, PartialEq)]
pub struct PartCount {
    pub name: String,
    pub role: String,
    pub whole: usize,
    pub cut: usize,
}

/// The counts of an object.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitCounts {
    pub whole: usize,
    pub cut: usize,
    pub slivers: usize,
    pub unfillable: usize,
    pub order_units: usize,
    pub order_units_min: usize,
    pub area_laid_m2: f64,
    pub cut_area_m2: f64,
    pub widest_joint_mm: f64,
    pub joint_warning: Option<String>,
    pub types: Vec<TypeCount>,
    /// Zones and courses, when the hardscape has any.
    pub parts: Vec<PartCount>,
    pub notes: Vec<String>,
}

fn n(v: &Value, k: &str) -> usize {
    v.get(k).and_then(Value::as_u64).unwrap_or(0) as usize
}

fn f(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

impl UnitCounts {
    /// Reads the reply of `report.unit_pattern` for one object. `Err`: the text of the row's error.
    pub fn parse(reply: &Value) -> Result<UnitCounts, String> {
        let row = reply
            .get("rows")
            .and_then(Value::as_array)
            .and_then(|r| r.first())
            .ok_or("the report has no row")?;
        if let Some(e) = row.get("error").and_then(Value::as_str) {
            return Err(e.to_string());
        }
        let list = |k: &str| -> Vec<Value> {
            row.get(k)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        Ok(UnitCounts {
            whole: n(row, "whole_units"),
            cut: n(row, "cut_units"),
            slivers: n(row, "slivers"),
            unfillable: n(row, "unfillable_pieces"),
            order_units: n(row, "order_units"),
            order_units_min: n(row, "order_units_min"),
            area_laid_m2: f(row, "area_laid_m2"),
            cut_area_m2: f(row, "cut_area_m2"),
            widest_joint_mm: f(row, "widest_joint_mm"),
            joint_warning: row
                .get("joint_warning")
                .and_then(Value::as_str)
                .map(str::to_string),
            types: list("by_unit")
                .iter()
                .map(|u| TypeCount {
                    name: u["name"].as_str().unwrap_or("").to_string(),
                    whole: n(u, "whole_units"),
                    cut: n(u, "cut_units"),
                    order_units: n(u, "order_units"),
                })
                .collect(),
            parts: list("groups")
                .iter()
                .map(|g| PartCount {
                    name: g["name"].as_str().unwrap_or("").to_string(),
                    role: g["role"].as_str().unwrap_or("").to_string(),
                    whole: n(g, "whole_units"),
                    cut: n(g, "cut_units"),
                })
                .collect(),
            notes: list("notes")
                .iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect(),
        })
    }
}

/// Counts the units of a hardscape with the report of the API.
pub fn count(be: &mut dyn Backend, id: ObjectId) -> Result<UnitCounts, String> {
    let reply = be.call("report.unit_pattern", json!({"ids": [id]}))?;
    UnitCounts::parse(&reply)
}

/// True if Object Info counts this hardscape by itself: the estimate of its units is small. A
/// hardscape without a pattern is not counted at all.
pub fn auto(doc: &Document, id: ObjectId) -> bool {
    let Some(o) = doc.object(id) else {
        return false;
    };
    let Some((pattern, _)) = pattern_of(doc, o) else {
        return false;
    };
    if !matches!(o.shape, Shape::Hardscape { .. }) {
        return false;
    }
    let area = hardscape::report::quantities(doc, o, None).map_or(0.0, |q| q.plan_area_m2 * 1e6);
    let unit = match pattern.module {
        Some(_) => 0.0,
        None => (pattern.unit_length + pattern.gap) * (pattern.unit_width + pattern.gap),
    };
    // A module, or a pattern whose size is not known here, is counted when asked.
    unit > 0.0 && area / unit <= AUTO_UNITS
}

#[cfg(test)]
mod tests;
