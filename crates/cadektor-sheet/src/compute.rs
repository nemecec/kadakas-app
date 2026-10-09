//! Worksheet calculation: database blocks first, then cells in dependency order.
//!
//! Cycles are found without recursion: every cell on a cycle gets `#CYCLE!`, and cells that read
//! them get that error too. Long chains of cells cannot overflow the stack either.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadektor_core::model::Document;

use crate::db::{self, LineKind};
use crate::def::{CellFormat, Content, WorksheetDef, interpret};
use crate::eval::{Env, eval};
use crate::formula::{self, CellRef, Expr};
use crate::value::{ErrorKind, Value, format_number};

type Pos = (u32, u32);

/// The result of calculating a worksheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Computed {
    values: BTreeMap<Pos, Value>,
    formats: BTreeMap<Pos, CellFormat>,
    /// Last used row and column (0 for an empty sheet).
    pub rows: u32,
    pub cols: u32,
    /// Things that went wrong, in plain words. Cells with errors show the error code too.
    pub problems: Vec<String>,
    /// Per database block: the first row, the number of rows and the number of matching objects.
    pub blocks: Vec<Block>,
}

/// Where a database block ended up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub name: String,
    pub first_row: u32,
    pub rows: u32,
    pub matched: usize,
}

impl Computed {
    /// The value of a cell (1-based row and column).
    pub fn value(&self, row: u32, col: u32) -> Value {
        self.values.get(&(row, col)).cloned().unwrap_or_default()
    }

    /// The value at an address such as `B3`.
    pub fn value_at(&self, address: &str) -> Option<Value> {
        let c = CellRef::parse(address)?;
        Some(self.value(c.row, c.col))
    }

    /// The format of a cell.
    pub fn format(&self, row: u32, col: u32) -> CellFormat {
        self.formats.get(&(row, col)).cloned().unwrap_or_default()
    }

    /// What the cell shows: the number with its decimals and unit, text, or the error code.
    pub fn display(&self, row: u32, col: u32) -> String {
        display_value(&self.value(row, col), &self.format(row, col), true)
    }

    /// The cells as text for exports: like `display`, but without the unit.
    pub fn display_plain(&self, row: u32, col: u32) -> String {
        display_value(&self.value(row, col), &self.format(row, col), false)
    }

    /// All cells as text, row by row, up to the last used row and column.
    pub fn grid(&self) -> Vec<Vec<String>> {
        (1..=self.rows).map(|r| (1..=self.cols).map(|c| self.display(r, c)).collect()).collect()
    }

    /// All non-empty values with their positions, row by row.
    pub fn cells(&self) -> impl Iterator<Item = (u32, u32, &Value)> {
        self.values.iter().map(|(&(r, c), v)| (r, c, v))
    }

    /// The number of cells that show an error.
    pub fn error_count(&self) -> usize {
        self.values.values().filter(|v| v.is_error()).count()
    }
}

fn display_value(v: &Value, f: &CellFormat, with_unit: bool) -> String {
    match v {
        Value::Number(n) => {
            let mut s = match f.decimals {
                Some(d) => {
                    let s = format!("{n:.prec$}", prec = usize::from(d));
                    // A tiny negative number rounds to "-0.00". Show it without the sign.
                    if s.starts_with('-') && !s.bytes().any(|b| (b'1'..=b'9').contains(&b)) { s[1..].to_string() } else { s }
                }
                None => format_number(*n),
            };
            if with_unit && !f.unit.is_empty() {
                s.push(' ');
                s.push_str(&f.unit);
            }
            s
        }
        other => other.to_text(),
    }
}

enum Cell {
    Const(Value),
    Formula(Expr),
}

struct CellEnv<'a> {
    values: &'a BTreeMap<Pos, Value>,
}

impl Env for CellEnv<'_> {
    fn cell(&self, at: CellRef) -> Value {
        self.values.get(&(at.row, at.col)).cloned().unwrap_or_default()
    }

    fn range(&self, from: CellRef, to: CellRef) -> Vec<Value> {
        self.values.range((from.row, from.col)..=(to.row, to.col)).filter(|((_, c), _)| (from.col..=to.col).contains(c)).map(|(_, v)| v.clone()).collect()
    }

    fn name(&self, _: &str) -> Value {
        Value::Error(ErrorKind::Name)
    }

    fn path(&self, _: &[String]) -> Value {
        Value::Error(ErrorKind::Name)
    }

    fn has_record(&self, _: &str) -> bool {
        false
    }
}

/// The cells that a formula reads, among the cells that exist.
fn dependencies(expr: &Expr, cells: &BTreeMap<Pos, Cell>) -> Vec<Pos> {
    let (mut refs, mut ranges) = (Vec::new(), Vec::new());
    expr.references(&mut refs, &mut ranges);
    let mut out: Vec<Pos> = refs.iter().map(|c| (c.row, c.col)).filter(|p| cells.contains_key(p)).collect();
    for (a, b) in ranges {
        out.extend(cells.range((a.row, a.col)..=(b.row, b.col)).map(|(p, _)| *p).filter(|(_, c)| (a.col..=b.col).contains(c)));
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Calculates a worksheet against a document, with no ground.
pub fn compute(def: &WorksheetDef, doc: &Document) -> Computed {
    compute_with(def, doc, &cadektor_core::hardscape::NoTerrain)
}

/// Calculates a worksheet against a document and the ground of its site models, so draped
/// hardscapes and landscape areas are measured as the reports measure them.
pub fn compute_with(def: &WorksheetDef, doc: &Document, ground: &dyn cadektor_core::hardscape::TerrainSource) -> Computed {
    let mut result = Computed::default();
    let mut cells: BTreeMap<Pos, Cell> = BTreeMap::new();

    for (key, cell) in &def.cells {
        let Some(at) = CellRef::parse(key) else {
            result.problems.push(format!("'{key}' is not a cell address"));
            continue;
        };
        let pos = (at.row, at.col);
        if !cell.format.is_default() {
            result.formats.insert(pos, cell.format.clone());
        }
        match interpret(&cell.input) {
            Content::Empty => {}
            Content::Const(v) => {
                cells.insert(pos, Cell::Const(v));
            }
            Content::Formula(f) => match formula::parse(&f) {
                Ok(e) => {
                    cells.insert(pos, Cell::Formula(e));
                }
                Err(e) => {
                    result.problems.push(format!("cell {at}: {e}"));
                    cells.insert(pos, Cell::Const(Value::Error(ErrorKind::Parse)));
                }
            },
        }
    }

    // Database blocks write constants over the cells at their addresses.
    for (i, d) in def.databases.iter().enumerate() {
        let Some(anchor) = CellRef::parse(&d.anchor) else {
            result.problems.push(format!("database {}: '{}' is not a cell address", i + 1, d.anchor));
            continue;
        };
        let out = db::run_on(doc, d, i, ground);
        result.problems.extend(out.problems.iter().cloned());
        for (li, line) in out.lines.iter().enumerate() {
            for (ci, v) in line.values.iter().enumerate() {
                let (row, col) = (anchor.row + li as u32, anchor.col + ci as u32);
                if row > formula::MAX_ROW || col > formula::MAX_COL {
                    continue;
                }
                cells.insert((row, col), Cell::Const(v.clone()));
                let c = &d.columns[ci];
                if line.kind != LineKind::Header && (c.decimals.is_some() || !c.unit.is_empty()) {
                    result.formats.insert((row, col), CellFormat { decimals: c.decimals, unit: c.unit.clone() });
                } else {
                    result.formats.remove(&(row, col));
                }
            }
        }
        result.blocks.push(Block { name: d.name.clone(), first_row: anchor.row, rows: out.lines.len() as u32, matched: out.matched });
    }

    // Order the formulas so that every cell comes after the cells it reads.
    let mut values: BTreeMap<Pos, Value> = BTreeMap::new();
    for (p, c) in &cells {
        if let Cell::Const(v) = c {
            values.insert(*p, v.clone());
        }
    }
    let mut state: HashMap<Pos, u8> = HashMap::new(); // 1: being visited, 2: done
    let mut cyclic: HashSet<Pos> = HashSet::new();
    let formulas: Vec<Pos> = cells.iter().filter(|(_, c)| matches!(c, Cell::Formula(_))).map(|(p, _)| *p).collect();
    for root in formulas {
        if state.contains_key(&root) {
            continue;
        }
        // Each frame: the cell, its dependencies and the next dependency to visit.
        let mut stack: Vec<(Pos, Vec<Pos>, usize)> = Vec::new();
        let deps_of = |p: Pos| match cells.get(&p) {
            Some(Cell::Formula(e)) => dependencies(e, &cells),
            _ => Vec::new(),
        };
        state.insert(root, 1);
        stack.push((root, deps_of(root), 0));
        while let Some(top) = stack.last_mut() {
            if top.2 < top.1.len() {
                let dep = top.1[top.2];
                top.2 += 1;
                match state.get(&dep) {
                    None => {
                        state.insert(dep, 1);
                        let d = deps_of(dep);
                        stack.push((dep, d, 0));
                    }
                    Some(1) => {
                        // A cycle: the cells from `dep` up to the top of the stack.
                        if let Some(at) = stack.iter().position(|f| f.0 == dep) {
                            cyclic.extend(stack[at..].iter().map(|f| f.0));
                        }
                    }
                    Some(_) => {}
                }
            } else {
                let (p, _, _) = stack.pop().expect("a frame");
                state.insert(p, 2);
                if let Some(Cell::Formula(e)) = cells.get(&p) {
                    let v = if cyclic.contains(&p) { Value::Error(ErrorKind::Cycle) } else { eval(e, &CellEnv { values: &values }) };
                    values.insert(p, v);
                }
            }
        }
    }
    if !cyclic.is_empty() {
        let mut at: Vec<CellRef> = cyclic.iter().map(|&(row, col)| CellRef { row, col }).collect();
        at.sort();
        result.problems.push(format!("circular reference in {}", at.iter().take(5).map(ToString::to_string).collect::<Vec<_>>().join(", ")));
    }

    values.retain(|_, v| !v.is_empty());
    result.rows = values.keys().chain(result.formats.keys()).map(|p| p.0).max().unwrap_or(0);
    result.cols = values.keys().chain(result.formats.keys()).map(|p| p.1).max().unwrap_or(0);
    result.values = values;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::{ColumnDef, DatabaseDef, GroupMode, Summary};
    use cadektor_core::DVec2;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::curve::corners;
    use cadektor_core::model::{Attributes, Shape};
    use proptest::prelude::*;

    fn sheet(cells: &[(&str, &str)]) -> WorksheetDef {
        let mut w = WorksheetDef::default();
        for (a, i) in cells {
            w.set_cell(a, i, None).unwrap();
        }
        w
    }

    fn run(cells: &[(&str, &str)]) -> Computed {
        compute(&sheet(cells), &Document::default())
    }

    fn v(c: &Computed, a: &str) -> Value {
        c.value_at(a).unwrap()
    }

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    #[test]
    fn constants_and_formulas() {
        let c = run(&[("A1", "1"), ("A2", "2"), ("A3", "=A1+A2"), ("B1", "text"), ("B2", "=B1 & \"!\""), ("B3", "true"), ("C1", "=SUM(A1:A3)*2"), ("C2", "=AVERAGE(A1:A3)"), ("C3", "=IF(A3>2, \"big\", \"small\")")]);
        assert_eq!(v(&c, "A3"), n(3.0));
        assert_eq!(v(&c, "B2"), Value::Text("text!".into()));
        assert_eq!(v(&c, "B3"), Value::Bool(true));
        assert_eq!(v(&c, "C1"), n(12.0));
        assert_eq!(v(&c, "C2"), n(2.0));
        assert_eq!(v(&c, "C3"), Value::Text("big".into()));
        assert_eq!((c.rows, c.cols), (3, 3));
        assert!(c.problems.is_empty() && c.error_count() == 0);
        assert_eq!(c.grid()[2], ["3", "TRUE", "big"]);
        assert_eq!(c.value_at("zz"), None);
        assert_eq!(v(&c, "Z9"), Value::Empty);
    }

    #[test]
    fn order_does_not_matter() {
        let c = run(&[("A1", "=B1+1"), ("B1", "=C1+1"), ("C1", "=D1+1"), ("D1", "10")]);
        assert_eq!(v(&c, "A1"), n(13.0));
        let c = run(&[("A1", "=SUM(B1:B3)"), ("B1", "=B2"), ("B2", "=B3"), ("B3", "4")]);
        assert_eq!(v(&c, "A1"), n(12.0));
    }

    #[test]
    fn cycles_are_errors_not_hangs() {
        let c = run(&[("A1", "=B1"), ("B1", "=A1"), ("C1", "=A1+1"), ("D1", "5")]);
        assert_eq!(v(&c, "A1"), Value::Error(ErrorKind::Cycle));
        assert_eq!(v(&c, "B1"), Value::Error(ErrorKind::Cycle));
        assert_eq!(v(&c, "C1"), Value::Error(ErrorKind::Cycle), "readers of a cycle show the error");
        assert_eq!(v(&c, "D1"), n(5.0));
        assert!(c.problems[0].contains("circular") && c.problems[0].contains("A1"));
        let c = run(&[("A1", "=A1+1")]);
        assert_eq!(v(&c, "A1"), Value::Error(ErrorKind::Cycle));
        // A cycle through a range.
        let c = run(&[("A1", "=SUM(A1:A3)"), ("A2", "1")]);
        assert_eq!(v(&c, "A1"), Value::Error(ErrorKind::Cycle));
        // A three-cell cycle with a branch that is not on it.
        let c = run(&[("A1", "=B1+C1"), ("B1", "=C1"), ("C1", "=A1"), ("D1", "=E1"), ("E1", "2")]);
        assert_eq!(v(&c, "B1"), Value::Error(ErrorKind::Cycle));
        assert_eq!(v(&c, "D1"), n(2.0));
        // IFERROR can hide the error from a reader.
        let c = run(&[("A1", "=B1"), ("B1", "=A1"), ("C1", "=IFERROR(A1, -1)")]);
        assert_eq!(v(&c, "C1"), n(-1.0));
    }

    #[test]
    fn long_chains_do_not_overflow_the_stack() {
        let mut w = WorksheetDef::default();
        w.set_cell("A1", "1", None).unwrap();
        for r in 2..=5000 {
            w.set_cell(&format!("A{r}"), &format!("=A{}+1", r - 1), None).unwrap();
        }
        let c = compute(&w, &Document::default());
        assert_eq!(v(&c, "A5000"), n(5000.0));
        // The same chain as a cycle.
        w.set_cell("A1", "=A5000", None).unwrap();
        let c = compute(&w, &Document::default());
        assert_eq!(v(&c, "A2500"), Value::Error(ErrorKind::Cycle));
        assert_eq!(c.error_count(), 5000);
    }

    #[test]
    fn parse_errors_show_in_the_cell() {
        let c = run(&[("A1", "=1+"), ("A2", "=A1+1")]);
        assert_eq!(v(&c, "A1"), Value::Error(ErrorKind::Parse));
        assert_eq!(v(&c, "A2"), Value::Error(ErrorKind::Parse));
        assert!(c.problems[0].starts_with("cell A1"));
        let c = run(&[("A1", "=Area"), ("A2", "=Nope(1)"), ("A3", "=1/0"), ("A4", "=A1:A2")]);
        assert_eq!(v(&c, "A1"), Value::Error(ErrorKind::Name));
        assert_eq!(v(&c, "A2"), Value::Error(ErrorKind::Name));
        assert_eq!(v(&c, "A3"), Value::Error(ErrorKind::Div0));
        assert_eq!(v(&c, "A4"), Value::Error(ErrorKind::Value));
        let c = run(&[("A1", "=Soil.Depth"), ("A2", "=HASRECORD(\"x\")")]);
        assert_eq!(v(&c, "A1"), Value::Error(ErrorKind::Name));
        assert_eq!(v(&c, "A2"), Value::Bool(false));
    }

    #[test]
    fn a_huge_formula_is_an_error_cell_not_a_crash() {
        let long = format!("={}1", "1+".repeat(1900));
        let c = run(&[("A1", &long), ("A2", "=A1+1"), ("B1", "=ROUND(1.005, 2)"), ("B2", "=50%"), ("B3", "=$B$2*2"), ("B4", "=SUM(C1:C2)=0")]);
        assert_eq!(v(&c, "A1"), Value::Error(ErrorKind::Parse));
        assert_eq!(v(&c, "A2"), Value::Error(ErrorKind::Parse));
        assert!(c.problems[0].contains("levels"), "{:?}", c.problems);
        assert_eq!(v(&c, "B1"), n(1.01));
        assert_eq!(v(&c, "B2"), n(0.5));
        assert_eq!(v(&c, "B3"), n(1.0));
        assert_eq!(v(&c, "B4"), Value::Bool(true));
    }

    #[test]
    fn formats_and_display() {
        let mut w = sheet(&[("A1", "3.14159"), ("A2", "-0.001"), ("A3", "12"), ("A4", "x"), ("A5", "=1/0")]);
        w.set_cell("A1", "3.14159", Some(CellFormat { decimals: Some(2), unit: "m".into() })).unwrap();
        w.set_cell("A2", "-0.001", Some(CellFormat { decimals: Some(1), unit: String::new() })).unwrap();
        w.set_cell("A3", "12", Some(CellFormat { decimals: None, unit: "kg".into() })).unwrap();
        w.set_cell("A4", "x", Some(CellFormat { decimals: Some(2), unit: "m".into() })).unwrap();
        w.set_cell("B7", "", Some(CellFormat { decimals: Some(2), unit: String::new() })).unwrap();
        let c = compute(&w, &Document::default());
        assert_eq!(c.display(1, 1), "3.14 m");
        assert_eq!(c.display_plain(1, 1), "3.14");
        assert_eq!(c.display(2, 1), "0.0");
        assert_eq!(c.display(3, 1), "12 kg");
        assert_eq!(c.display(4, 1), "x");
        assert_eq!(c.display(5, 1), "#DIV/0!");
        assert_eq!(c.display(9, 9), "");
        assert_eq!((c.rows, c.cols), (7, 2), "a formatted empty cell counts for the size");
        assert_eq!(c.format(1, 1).decimals, Some(2));
        assert_eq!(c.cells().count(), 5);
    }

    fn scene() -> cadektor_core::command::Editor {
        let mut ed = Editor::default();
        for (m, side) in [("Granite", 2000.0), ("Brick", 1000.0), ("Granite", 3000.0)] {
            let b = corners(&[DVec2::ZERO, DVec2::new(side, 0.0), DVec2::new(side, side), DVec2::new(0.0, side)]);
            ed.execute(&Command::AddObject { shape: Shape::Hardscape { boundary: b, material: m.into(), thickness: None, elevation: None }, layer: None, class: None, attrs: Attributes::default() }).unwrap();
        }
        ed
    }

    fn quantities() -> DatabaseDef {
        DatabaseDef {
            name: "Paving".into(),
            anchor: "B2".into(),
            criteria: "Kind = \"hardscape\"".into(),
            columns: vec![
                ColumnDef { header: "Material".into(), expr: "Material".into(), ..Default::default() },
                ColumnDef { header: "Area".into(), expr: "Area".into(), summary: Some(Summary::Sum), decimals: Some(1), unit: "m2".into() },
            ],
            group_by: Some(0),
            group_mode: GroupMode::Summary,
            ..Default::default()
        }
    }

    #[test]
    fn database_blocks_sit_at_their_anchor_and_cells_can_read_them() {
        let ed = scene();
        let mut w = sheet(&[("A1", "Paving quantities"), ("F2", "=C5*2"), ("F3", "=SUM(C3:C4)"), ("F4", "=COUNTA(B3:B5)")]);
        w.databases.push(quantities());
        let c = compute(&w, ed.doc());
        // Header row 2, groups in rows 3 and 4, total in row 5.
        assert_eq!(c.grid()[1][1..3], ["Material", "Area"]);
        assert_eq!(c.display(3, 2), "Brick");
        assert_eq!(c.display(3, 3), "1.0 m2");
        assert_eq!(c.display(4, 3), "13.0 m2");
        assert_eq!(c.display(5, 2), "Total");
        assert_eq!(v(&c, "C5"), n(14.0));
        assert_eq!(v(&c, "F2"), n(28.0));
        assert_eq!(v(&c, "F3"), n(14.0));
        assert_eq!(v(&c, "F4"), n(3.0));
        assert_eq!(c.blocks, vec![Block { name: "Paving".into(), first_row: 2, rows: 4, matched: 3 }]);
        assert_eq!(c.display(1, 1), "Paving quantities");
        // The header has no number format.
        assert_eq!(c.format(2, 3), CellFormat::default());
    }

    #[test]
    fn database_output_covers_cells_and_later_blocks_win() {
        let ed = scene();
        let mut w = sheet(&[("B3", "will be covered"), ("B2", "=\"x\"")]);
        w.databases.push(quantities());
        let mut second = quantities();
        second.anchor = "B4".into();
        second.header = false;
        second.criteria = "Material = \"Brick\"".into();
        second.group_by = None;
        second.total = false;
        w.databases.push(second);
        let c = compute(&w, ed.doc());
        assert_eq!(c.display(2, 2), "Material");
        assert_eq!(c.display(3, 2), "Brick");
        assert_eq!(c.display(4, 2), "Brick", "the second block covers the first");
        assert_eq!(c.blocks.len(), 2);
    }

    #[test]
    fn database_problems_are_listed() {
        let ed = scene();
        let mut bad = quantities();
        bad.anchor = "??".into();
        let mut off = quantities();
        off.anchor = "A99999".into();
        off.columns.push(ColumnDef { header: "x".into(), expr: "Id".into(), ..Default::default() });
        let mut col_off = quantities();
        col_off.anchor = "ZZ1".into();
        let mut w = WorksheetDef { databases: vec![bad, off, col_off], ..Default::default() };
        w.cells.insert("not an address".into(), Default::default());
        let c = compute(&w, ed.doc());
        assert!(c.problems.iter().any(|p| p.contains("database 1") && p.contains("not a cell address")));
        assert!(c.problems.iter().any(|p| p.contains("'not an address' is not a cell address")));
        assert!(c.rows > 0);
    }

    #[test]
    fn results_follow_the_document() {
        let mut ed = scene();
        let mut w = sheet(&[("A1", "=COUNTA(B3:B10)")]);
        w.databases.push(quantities());
        assert_eq!(v(&compute(&w, ed.doc()), "A1"), n(3.0));
        let b = corners(&[DVec2::ZERO, DVec2::new(500.0, 0.0), DVec2::new(500.0, 500.0), DVec2::new(0.0, 500.0)]);
        ed.execute(&Command::AddObject { shape: Shape::Hardscape { boundary: b, material: "Slate".into(), thickness: None, elevation: None }, layer: None, class: None, attrs: Attributes::default() }).unwrap();
        assert_eq!(v(&compute(&w, ed.doc()), "A1"), n(4.0));
    }

    proptest! {
        /// Random cell inputs never panic or hang, and every cell gets a value or an error.
        #[test]
        fn random_sheets_terminate(inputs in prop::collection::vec("=?[A-D1-4+*(): ,SUMIF\"]{0,14}", 1..16)) {
            let mut w = WorksheetDef::default();
            for (i, input) in inputs.iter().enumerate() {
                let addr = format!("{}{}", ["A", "B", "C", "D"][i % 4], i / 4 + 1);
                w.set_cell(&addr, input, None).unwrap();
            }
            let c = compute(&w, &Document::default());
            let _ = c.grid();
        }

        /// Formulas that only read cells in the grid and point forward never make a cycle.
        #[test]
        fn forward_references_never_cycle(vals in prop::collection::vec(0u32..100, 2..30)) {
            let mut w = WorksheetDef::default();
            let n = vals.len();
            for (i, x) in vals.iter().enumerate() {
                let input = if i + 1 < n { format!("={}+A{}", x, i + 2) } else { x.to_string() };
                w.set_cell(&format!("A{}", i + 1), &input, None).unwrap();
            }
            let c = compute(&w, &Document::default());
            prop_assert_eq!(c.error_count(), 0);
            prop_assert_eq!(c.value(1, 1), Value::Number(f64::from(vals.iter().sum::<u32>())));
        }

        /// Every ring of formulas is a cycle, and all cells on it show #CYCLE!.
        #[test]
        fn rings_are_cycles(len in 1usize..20) {
            let mut w = WorksheetDef::default();
            for i in 0..len {
                w.set_cell(&format!("A{}", i + 1), &format!("=A{}+1", (i + 1) % len + 1), None).unwrap();
            }
            let c = compute(&w, &Document::default());
            prop_assert_eq!(c.error_count(), len);
        }
    }
}
