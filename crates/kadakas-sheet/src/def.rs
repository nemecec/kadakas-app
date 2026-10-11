//! The saved definition of a worksheet. It lives in a document resource. Values are not saved.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::formula::{self, CellRef};
use crate::value::Value;

pub const MAX_CELLS: usize = 20_000;
pub const MAX_DATABASES: usize = 20;
pub const MAX_COLUMNS: usize = 64;
pub const MAX_DECIMALS: u8 = 10;

/// How a cell shows its number.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellFormat {
    /// Number of decimals. Without it, the number shows as it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    /// A unit label after the number, for example `m2`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unit: String,
}

impl CellFormat {
    pub fn is_default(&self) -> bool {
        *self == CellFormat::default()
    }
}

/// One cell: what the user typed, and how it shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellDef {
    /// `=SUM(A1:A3)` is a formula. A number, `TRUE` or `FALSE` is read as such. Anything else is
    /// text. A leading `'` forces text.
    pub input: String,
    #[serde(flatten)]
    pub format: CellFormat,
}

/// What a cell input means.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Empty,
    Const(Value),
    Formula(String),
}

/// Reads the input of a cell.
pub fn interpret(input: &str) -> Content {
    let t = input.trim();
    if t.is_empty() {
        return Content::Empty;
    }
    if let Some(f) = t.strip_prefix('=') {
        return Content::Formula(f.to_string());
    }
    if let Some(text) = input.strip_prefix('\'') {
        return Content::Const(Value::Text(text.to_string()));
    }
    if let Ok(n) = t.parse::<f64>()
        && n.is_finite()
    {
        return Content::Const(Value::Number(n));
    }
    if t.eq_ignore_ascii_case("true") {
        return Content::Const(Value::Bool(true));
    }
    if t.eq_ignore_ascii_case("false") {
        return Content::Const(Value::Bool(false));
    }
    Content::Const(Value::Text(input.to_string()))
}

/// How a database column adds up its rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Summary {
    Sum,
    Average,
    Min,
    Max,
    /// The number of rows with a value.
    Count,
}

impl Summary {
    pub fn id(self) -> &'static str {
        match self {
            Summary::Sum => "sum",
            Summary::Average => "average",
            Summary::Min => "min",
            Summary::Max => "max",
            Summary::Count => "count",
        }
    }
}

/// A database column: a header and an expression over the properties of one object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnDef {
    pub header: String,
    /// For example `Area`, `Layer`, `Soil.Depth` or `ROUND(Area * 1.1, 1)`.
    pub expr: String,
    /// How totals and group rows add this column up. Without it, the column has no total.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unit: String,
}

/// Sort by one column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortKey {
    /// Index of the column, from 0.
    pub column: usize,
    #[serde(default)]
    pub descending: bool,
}

/// What a grouped database shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupMode {
    /// Every object, and a subtotal row after each group.
    #[default]
    Detail,
    /// One row per group.
    Summary,
}

/// Where the rows of a database block come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowSource {
    /// One row per object of the document.
    #[default]
    Objects,
    /// One row per plant style, with the single plants and the plants of landscape area mixes
    /// counted together, as `report.plant_list` does. Names: Style, IdCode, Botanical,
    /// CommonName, Category, Size, Container, Root, Plants, InAreas, Count, Price, Cost, Area, Image
    /// (the picture of the style). On objects, the name `Image` (or `Picture`) gives the picture of the
    /// plant style of a plant.
    PlantList,
}

fn yes() -> bool {
    true
}

fn a1() -> String {
    "A1".into()
}

/// A block of database rows: it lists the objects that match the criteria.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseDef {
    pub name: String,
    /// Objects (default), or the plant list.
    pub source: RowSource,
    /// The top left cell of the block.
    pub anchor: String,
    /// Which objects to list. For example `Kind = "plant" AND Layer = "Planting"`. Empty means all objects.
    pub criteria: String,
    pub columns: Vec<ColumnDef>,
    pub sort: Vec<SortKey>,
    /// Index of the column to group by. The block is sorted by it first.
    pub group_by: Option<usize>,
    pub group_mode: GroupMode,
    /// Show a header row.
    #[serde(default = "yes")]
    pub header: bool,
    /// Show a total row, if a column has a summary.
    #[serde(default = "yes")]
    pub total: bool,
}

impl Default for DatabaseDef {
    fn default() -> Self {
        DatabaseDef {
            name: String::new(),
            source: RowSource::Objects,
            anchor: a1(),
            criteria: String::new(),
            columns: Vec::new(),
            sort: Vec::new(),
            group_by: None,
            group_mode: GroupMode::Detail,
            header: true,
            total: true,
        }
    }
}

/// A worksheet: free cells and database blocks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorksheetDef {
    /// Cells by address (`A1`). Database output covers cells at the same address.
    pub cells: BTreeMap<String, CellDef>,
    pub databases: Vec<DatabaseDef>,
}

impl WorksheetDef {
    /// Reads a definition from the JSON of a worksheet resource. Fields that do not belong are ignored.
    pub fn from_value(v: &serde_json::Value) -> Result<WorksheetDef, String> {
        serde_json::from_value(v.clone()).map_err(|e| format!("the worksheet is not valid: {e}"))
    }

    /// The JSON of a worksheet resource body.
    pub fn to_value(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        v["type"] = "worksheet".into();
        v
    }

    /// Sets, formats or clears one cell. An empty input without a format removes the cell.
    pub fn set_cell(
        &mut self,
        address: &str,
        input: &str,
        format: Option<CellFormat>,
    ) -> Result<(), String> {
        let at = CellRef::parse(address).ok_or_else(|| {
            format!(
                "'{address}' is not a cell address. Use A1 to {}{}",
                CellRef::col_name(formula::MAX_COL),
                formula::MAX_ROW
            )
        })?;
        let key = at.to_string();
        let old = self.cells.get(&key).cloned().unwrap_or_default();
        let new = CellDef {
            input: input.to_string(),
            format: format.unwrap_or(old.format),
        };
        if new.input.trim().is_empty() && new.format.is_default() {
            self.cells.remove(&key);
        } else {
            self.cells.insert(key, new);
        }
        Ok(())
    }

    /// Checks the whole definition: addresses, formulas, expressions and indexes.
    pub fn validate(&self) -> Result<(), String> {
        if self.cells.len() > MAX_CELLS {
            return Err(format!("a worksheet has at most {MAX_CELLS} cells"));
        }
        for (key, cell) in &self.cells {
            let at = CellRef::parse(key).ok_or_else(|| format!("'{key}' is not a cell address"))?;
            if at.to_string() != *key {
                return Err(format!("the cell address '{key}' must be written as {at}"));
            }
            if cell.format.decimals.is_some_and(|d| d > MAX_DECIMALS) {
                return Err(format!(
                    "cell {key}: decimals must be at most {MAX_DECIMALS}"
                ));
            }
            if let Content::Formula(f) = interpret(&cell.input) {
                formula::parse(&f).map_err(|e| format!("cell {key}: {e}"))?;
            }
            if cell.input.chars().count() > formula::MAX_LEN {
                return Err(format!("cell {key}: the text is too long"));
            }
        }
        if self.databases.len() > MAX_DATABASES {
            return Err(format!(
                "a worksheet has at most {MAX_DATABASES} database blocks"
            ));
        }
        for (i, db) in self.databases.iter().enumerate() {
            let at = |m: String| format!("database {}: {m}", i + 1);
            if CellRef::parse(&db.anchor).is_none() {
                return Err(at(format!("'{}' is not a cell address", db.anchor)));
            }
            if !db.criteria.trim().is_empty() {
                formula::parse(&db.criteria).map_err(|e| at(format!("criteria: {e}")))?;
            }
            if db.columns.is_empty() || db.columns.len() > MAX_COLUMNS {
                return Err(at(format!("give 1 to {MAX_COLUMNS} columns")));
            }
            for (j, c) in db.columns.iter().enumerate() {
                formula::parse(&c.expr)
                    .map_err(|e| at(format!("column {} ({}): {e}", j + 1, c.header)))?;
                if c.decimals.is_some_and(|d| d > MAX_DECIMALS) {
                    return Err(at(format!(
                        "column {}: decimals must be at most {MAX_DECIMALS}",
                        j + 1
                    )));
                }
            }
            if let Some(k) = db.sort.iter().find(|k| k.column >= db.columns.len()) {
                return Err(at(format!("sort column {} does not exist", k.column)));
            }
            if db.group_by.is_some_and(|g| g >= db.columns.len()) {
                return Err(at("group_by column does not exist".into()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inputs_are_read() {
        assert_eq!(interpret(""), Content::Empty);
        assert_eq!(interpret("  "), Content::Empty);
        assert_eq!(interpret("=1+2"), Content::Formula("1+2".into()));
        assert_eq!(interpret(" =A1"), Content::Formula("A1".into()));
        assert_eq!(interpret("12.5"), Content::Const(Value::Number(12.5)));
        assert_eq!(interpret(" -3 "), Content::Const(Value::Number(-3.0)));
        assert_eq!(interpret("'123"), Content::Const(Value::Text("123".into())));
        assert_eq!(interpret("true"), Content::Const(Value::Bool(true)));
        assert_eq!(interpret("FALSE"), Content::Const(Value::Bool(false)));
        assert_eq!(
            interpret("Oak tree"),
            Content::Const(Value::Text("Oak tree".into()))
        );
        assert_eq!(interpret("inf"), Content::Const(Value::Text("inf".into())));
        assert_eq!(interpret("NaN"), Content::Const(Value::Text("NaN".into())));
    }

    #[test]
    fn set_cell_normalizes_and_clears() {
        let mut w = WorksheetDef::default();
        w.set_cell("b2", "5", None).unwrap();
        assert!(w.cells.contains_key("B2"));
        w.set_cell(
            "B2",
            "=A1",
            Some(CellFormat {
                decimals: Some(2),
                unit: "m".into(),
            }),
        )
        .unwrap();
        assert_eq!(w.cells["B2"].format.decimals, Some(2));
        // Changing the input keeps the format.
        w.set_cell("B2", "7", None).unwrap();
        assert_eq!(w.cells["B2"].format.unit, "m");
        w.set_cell("B2", "", Some(CellFormat::default())).unwrap();
        assert!(w.cells.is_empty());
        w.set_cell(
            "C1",
            "",
            Some(CellFormat {
                decimals: Some(1),
                unit: String::new(),
            }),
        )
        .unwrap();
        assert_eq!(w.cells.len(), 1, "an empty cell with a format stays");
        let e = w.set_cell("1A", "x", None).unwrap_err();
        assert!(e.contains("not a cell address"), "{e}");
    }

    #[test]
    fn json_round_trip_and_shape() {
        let mut w = WorksheetDef::default();
        w.set_cell(
            "A1",
            "=SUM(B1:B3)",
            Some(CellFormat {
                decimals: Some(2),
                unit: String::new(),
            }),
        )
        .unwrap();
        w.databases.push(DatabaseDef {
            name: "Plants".into(),
            criteria: "Kind = \"plant\"".into(),
            columns: vec![
                ColumnDef {
                    header: "Style".into(),
                    expr: "Style".into(),
                    ..ColumnDef::default()
                },
                ColumnDef {
                    header: "N".into(),
                    expr: "Count".into(),
                    summary: Some(Summary::Sum),
                    ..ColumnDef::default()
                },
            ],
            sort: vec![SortKey {
                column: 0,
                descending: true,
            }],
            group_by: Some(0),
            group_mode: GroupMode::Summary,
            ..DatabaseDef::default()
        });
        let v = w.to_value();
        assert_eq!(v["type"], "worksheet");
        assert_eq!(
            v["cells"]["A1"],
            json!({"input": "=SUM(B1:B3)", "decimals": 2})
        );
        assert_eq!(v["databases"][0]["columns"][1]["summary"], "sum");
        assert_eq!(WorksheetDef::from_value(&v).unwrap(), w);
        // Defaults for missing fields.
        let min = WorksheetDef::from_value(
            &json!({"type": "worksheet", "databases": [{"columns": [{"expr": "Area"}]}]}),
        )
        .unwrap();
        let db = &min.databases[0];
        assert!(db.header && db.total && db.anchor == "A1" && db.group_mode == GroupMode::Detail);
        assert!(
            WorksheetDef::from_value(&json!({"cells": 5}))
                .unwrap_err()
                .contains("not valid")
        );
        assert_eq!(Summary::Average.id(), "average");
        for s in [Summary::Sum, Summary::Min, Summary::Max, Summary::Count] {
            assert!(!s.id().is_empty());
        }
    }

    fn db() -> DatabaseDef {
        DatabaseDef {
            columns: vec![ColumnDef {
                header: "A".into(),
                expr: "Area".into(),
                ..ColumnDef::default()
            }],
            ..DatabaseDef::default()
        }
    }

    #[test]
    fn validate_checks_everything() {
        let mut w = WorksheetDef::default();
        w.set_cell("A1", "=1+", None).unwrap();
        assert!(w.validate().unwrap_err().contains("cell A1"));
        w.set_cell("A1", "=1+2", None).unwrap();
        assert!(w.validate().is_ok());
        w.cells.insert("a1".into(), CellDef::default());
        assert!(w.validate().unwrap_err().contains("must be written as"));
        w.cells.clear();
        w.cells.insert("zz".into(), CellDef::default());
        assert!(w.validate().unwrap_err().contains("not a cell address"));
        w.cells.clear();
        w.cells.insert(
            "A1".into(),
            CellDef {
                input: "x".into(),
                format: CellFormat {
                    decimals: Some(11),
                    unit: String::new(),
                },
            },
        );
        assert!(w.validate().unwrap_err().contains("decimals"));
        w.cells.insert(
            "A1".into(),
            CellDef {
                input: "x".repeat(5000),
                format: CellFormat::default(),
            },
        );
        assert!(w.validate().unwrap_err().contains("too long"));
        w.cells.clear();
        for c in 1..20 {
            for r in 1..1100 {
                w.cells
                    .insert(format!("{}{r}", CellRef::col_name(c)), CellDef::default());
            }
        }
        assert!(w.validate().unwrap_err().contains("at most"));

        let mut w = WorksheetDef {
            databases: vec![db()],
            ..Default::default()
        };
        assert!(w.validate().is_ok());
        w.databases[0].anchor = "?".into();
        assert!(w.validate().unwrap_err().contains("database 1"));
        w.databases[0] = db();
        w.databases[0].criteria = "Kind =".into();
        assert!(w.validate().unwrap_err().contains("criteria"));
        w.databases[0] = db();
        w.databases[0].columns.clear();
        assert!(w.validate().unwrap_err().contains("columns"));
        w.databases[0] = db();
        w.databases[0].columns[0].expr = "1 +".into();
        assert!(w.validate().unwrap_err().contains("column 1"));
        w.databases[0] = db();
        w.databases[0].columns[0].decimals = Some(12);
        assert!(w.validate().unwrap_err().contains("decimals"));
        w.databases[0] = db();
        w.databases[0].sort = vec![SortKey {
            column: 3,
            descending: false,
        }];
        assert!(w.validate().unwrap_err().contains("sort column"));
        w.databases[0] = db();
        w.databases[0].group_by = Some(1);
        assert!(w.validate().unwrap_err().contains("group_by"));
        w.databases = vec![db(); MAX_DATABASES + 1];
        assert!(w.validate().unwrap_err().contains("database blocks"));
    }
}
