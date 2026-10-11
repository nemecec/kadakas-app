//! Database rows: criteria select objects, columns are expressions over object properties.
//!
//! The result is a list of output lines (header, detail, subtotal and total rows). `compute`
//! places them in the grid.

use kadakas_core::ground::{GroundSource, NoTerrain};
use kadakas_core::model::{Document, Object};
use kadakas_core::props::{self, Prop};
use kadakas_plants::plant_list::{PlantListRow, plant_list};

use crate::def::{ColumnDef, DatabaseDef, GroupMode, RowSource, Summary};
use crate::eval::{Env, eval};
use crate::formula::{self, CellRef, Expr};
use crate::value::{ErrorKind, Value};

/// What an output line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Header,
    Detail,
    /// A group row (summary mode), or the subtotal after a group (detail mode).
    Subtotal,
    Total,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub kind: LineKind,
    pub values: Vec<Value>,
}

/// The lines of one database block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DbOutput {
    pub lines: Vec<Line>,
    /// How many objects matched.
    pub matched: usize,
    pub problems: Vec<String>,
}

/// Environment of one object: names are properties.
pub struct ObjEnv<'a> {
    pub doc: &'a Document,
    pub obj: &'a Object,
    /// The ground, for draped hardscapes and landscape areas.
    pub ground: &'a dyn GroundSource,
}

/// Environment of one row of the plant list.
struct ListEnv<'a> {
    row: &'a PlantListRow,
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect::<String>()
        .to_ascii_lowercase()
}

impl Env for ListEnv<'_> {
    fn cell(&self, _: CellRef) -> Value {
        Value::Error(ErrorKind::Ref)
    }

    fn range(&self, _: CellRef, _: CellRef) -> Vec<Value> {
        vec![Value::Error(ErrorKind::Ref)]
    }

    fn name(&self, name: &str) -> Value {
        let r = self.row;
        let text = |s: &str| Value::Text(s.to_string());
        match norm(name).as_str() {
            "kind" => text("plant"),
            "style" | "name" => text(&r.name),
            "idcode" | "code" => text(&r.id_code),
            "botanical" | "botanicalname" => text(&r.botanical_name),
            "commonname" => text(&r.common_name),
            "category" => text(r.category.id()),
            "size" => text(&r.size),
            "container" => text(&r.container),
            "root" => text(&r.root),
            "plants" => Value::num(r.plants as f64),
            "inareas" => Value::num(r.in_areas as f64),
            "count" => Value::num(r.count as f64),
            "price" | "unitprice" => Value::num(r.unit_price),
            "cost" | "totalprice" => Value::num(r.total_price),
            "area" => Value::num(r.area_m2),
            "image" | "picture" => r.image.map_or(Value::Empty, Value::Image),
            _ => Value::Error(ErrorKind::Name),
        }
    }

    fn path(&self, _: &[String]) -> Value {
        Value::Error(ErrorKind::Name)
    }

    fn has_record(&self, _: &str) -> bool {
        false
    }

    fn blank_is_zero(&self) -> bool {
        false
    }
}

fn from_json(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map_or(Value::Error(ErrorKind::Num), Value::num),
        serde_json::Value::String(s) => Value::Text(s),
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Null => Value::Empty,
        other => Value::Text(other.to_string()),
    }
}

fn from_prop(p: Prop) -> Value {
    match p {
        Prop::Value(v) => from_json(v),
        Prop::NotApplicable => Value::Empty,
        Prop::Unknown => Value::Error(ErrorKind::Name),
    }
}

impl Env for ObjEnv<'_> {
    fn cell(&self, _: CellRef) -> Value {
        Value::Error(ErrorKind::Ref)
    }

    fn range(&self, _: CellRef, _: CellRef) -> Vec<Value> {
        vec![Value::Error(ErrorKind::Ref)]
    }

    fn name(&self, name: &str) -> Value {
        let found = from_prop(props::property_on(self.doc, self.obj, &[name], self.ground));
        // The picture of the plant style of a plant (PLT-16) answers to `Image` and `Picture` only
        // when the object has no property or record field of that name: its own data comes first.
        if found == Value::Error(ErrorKind::Name)
            && matches!(norm(name).as_str(), "image" | "picture")
        {
            return kadakas_core::plants::link::style_of(self.obj)
                .and_then(|id| kadakas_plants::plant_list::picture_of(self.doc, id))
                .map_or(Value::Empty, Value::Image);
        }
        found
    }

    fn path(&self, path: &[String]) -> Value {
        let p: Vec<&str> = path.iter().map(String::as_str).collect();
        from_prop(props::property_on(self.doc, self.obj, &p, self.ground))
    }

    fn has_record(&self, name: &str) -> bool {
        props::has_record(self.obj, name)
    }

    fn blank_is_zero(&self) -> bool {
        false
    }

    fn area_unit(&self) -> kadakas_units::AreaUnit {
        kadakas_core::annot::style::default_area_unit(self.doc)
    }
}

/// Adds up values. Errors in the values give that error. `None` if nothing can be added up.
pub fn summarize(s: Summary, values: &[&Value]) -> Value {
    if let Some(Value::Error(k)) = values.iter().find(|v| v.is_error()) {
        return Value::Error(*k);
    }
    if s == Summary::Count {
        return Value::Number(values.iter().filter(|v| !v.is_empty()).count() as f64);
    }
    let nums: Vec<f64> = values
        .iter()
        .filter_map(|v| match v {
            Value::Number(n) => Some(*n),
            _ => None,
        })
        .collect();
    if nums.is_empty() {
        return Value::Empty;
    }
    match s {
        Summary::Sum => Value::num(nums.iter().sum()),
        Summary::Average => Value::num(nums.iter().sum::<f64>() / nums.len() as f64),
        Summary::Min => Value::num(nums.iter().copied().fold(f64::INFINITY, f64::min)),
        _ => Value::num(nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
    }
}

/// The matching objects with their column values.
fn select(
    doc: &Document,
    db: &DatabaseDef,
    label: &str,
    ground: &dyn GroundSource,
    out: &mut DbOutput,
) -> Vec<Vec<Value>> {
    let criteria: Option<Expr> = if db.criteria.trim().is_empty() {
        None
    } else {
        match formula::parse(&db.criteria) {
            Ok(e) => Some(e),
            Err(e) => {
                out.problems.push(format!("{label}: criteria: {e}"));
                return Vec::new();
            }
        }
    };
    let exprs: Vec<Result<Expr, String>> = db
        .columns
        .iter()
        .map(|c| formula::parse(&c.expr).map_err(|e| e.to_string()))
        .collect();
    for (c, e) in db.columns.iter().zip(&exprs) {
        if let Err(e) = e {
            out.problems
                .push(format!("{label}: column '{}': {e}", c.header));
        }
    }
    let mut rows = Vec::new();
    let mut criteria_errors = 0;
    let list = if db.source == RowSource::PlantList {
        plant_list(doc).rows
    } else {
        Vec::new()
    };
    let envs: Vec<Box<dyn Env + '_>> = match db.source {
        RowSource::Objects => doc
            .objects
            .values()
            .map(|obj| Box::new(ObjEnv { doc, obj, ground }) as Box<dyn Env + '_>)
            .collect(),
        RowSource::PlantList => list
            .iter()
            .map(|row| Box::new(ListEnv { row }) as Box<dyn Env + '_>)
            .collect(),
    };
    for env in &envs {
        let env: &dyn Env = env.as_ref();
        if let Some(c) = &criteria {
            match eval(c, env).to_bool() {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => {
                    criteria_errors += 1;
                    continue;
                }
            }
        }
        rows.push(
            exprs
                .iter()
                .map(|e| {
                    e.as_ref()
                        .map_or(Value::Error(ErrorKind::Parse), |e| eval(e, env))
                })
                .collect(),
        );
    }
    if criteria_errors > 0 {
        out.problems.push(format!("{label}: the criteria give an error for {criteria_errors} object(s), which are left out (check the names)"));
    }
    rows
}

fn summary_line(columns: &[ColumnDef], rows: &[&Vec<Value>]) -> Vec<Value> {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            c.summary.map_or(Value::Empty, |s| {
                summarize(s, &rows.iter().map(|r| &r[i]).collect::<Vec<_>>())
            })
        })
        .collect()
}

/// The objects that match some criteria, and the values of some columns for them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueryResult {
    /// The matching objects in drawing order.
    pub ids: Vec<kadakas_core::model::ObjectId>,
    /// One row per matching object: the value of each column expression.
    pub rows: Vec<Vec<Value>>,
    /// Objects for which the criteria gave an error (they are left out).
    pub errors: usize,
}

/// Finds objects by criteria. The criteria are the same language as the criteria of a database
/// block: `Kind = "plant" AND Area > 20`, `Class = "Trees" OR 'Soil'.'Depth' > 300`. An empty
/// criteria text matches all objects. `columns` are expressions over object properties. A bad
/// criteria or column text gives an error message.
pub fn query_objects(
    doc: &Document,
    criteria: &str,
    columns: &[String],
    ground: &dyn GroundSource,
) -> Result<QueryResult, String> {
    let crit: Option<Expr> = if criteria.trim().is_empty() {
        None
    } else {
        Some(formula::parse(criteria).map_err(|e| format!("criteria: {e}"))?)
    };
    let cols: Vec<Expr> = columns
        .iter()
        .map(|c| formula::parse(c).map_err(|e| format!("column '{c}': {e}")))
        .collect::<Result<_, _>>()?;
    let mut out = QueryResult::default();
    for obj in doc.objects.values() {
        let env = ObjEnv { doc, obj, ground };
        if let Some(c) = &crit {
            match eval(c, &env).to_bool() {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => {
                    out.errors += 1;
                    continue;
                }
            }
        }
        out.ids.push(obj.id);
        out.rows.push(cols.iter().map(|e| eval(e, &env)).collect());
    }
    Ok(out)
}

/// Runs one database block against the document, with no ground.
pub fn run(doc: &Document, db: &DatabaseDef, index: usize) -> DbOutput {
    run_on(doc, db, index, &NoTerrain)
}

/// Runs one database block against the document and the ground of its site models.
pub fn run_on(
    doc: &Document,
    db: &DatabaseDef,
    index: usize,
    ground: &dyn GroundSource,
) -> DbOutput {
    let label = if db.name.trim().is_empty() {
        format!("database {}", index + 1)
    } else {
        format!("database '{}'", db.name.trim())
    };
    let mut out = DbOutput::default();
    let mut rows = select(doc, db, &label, ground, &mut out);
    out.matched = rows.len();

    // Sort: the group column first, then the sort keys. Stable, so objects keep document order on ties.
    let keys: Vec<(usize, bool)> = db
        .group_by
        .map(|g| (g, false))
        .into_iter()
        .chain(db.sort.iter().map(|k| (k.column, k.descending)))
        .collect();
    if !keys.is_empty() {
        rows.sort_by(|a, b| {
            for &(c, desc) in &keys {
                let o = a[c].compare(&b[c]);
                if o != std::cmp::Ordering::Equal {
                    return if desc { o.reverse() } else { o };
                }
            }
            std::cmp::Ordering::Equal
        });
    }

    if db.header {
        out.lines.push(Line {
            kind: LineKind::Header,
            values: db
                .columns
                .iter()
                .map(|c| Value::Text(c.header.clone()))
                .collect(),
        });
    }
    let any_summary = db.columns.iter().any(|c| c.summary.is_some());
    match db.group_by {
        None => out.lines.extend(rows.iter().map(|r| Line {
            kind: LineKind::Detail,
            values: r.clone(),
        })),
        Some(g) => {
            let mut start = 0;
            while start < rows.len() {
                let mut end = start + 1;
                while end < rows.len()
                    && rows[end][g].compare(&rows[start][g]) == std::cmp::Ordering::Equal
                {
                    end += 1;
                }
                let group: Vec<&Vec<Value>> = rows[start..end].iter().collect();
                match db.group_mode {
                    GroupMode::Detail => {
                        out.lines.extend(group.iter().map(|r| Line {
                            kind: LineKind::Detail,
                            values: (*r).clone(),
                        }));
                        if any_summary {
                            let mut v = summary_line(&db.columns, &group);
                            if db.columns[g].summary.is_none() {
                                v[g] = Value::Text(format!("{} total", rows[start][g].to_text()));
                            }
                            out.lines.push(Line {
                                kind: LineKind::Subtotal,
                                values: v,
                            });
                        }
                    }
                    GroupMode::Summary => {
                        let mut v = summary_line(&db.columns, &group);
                        for (i, c) in db.columns.iter().enumerate() {
                            if c.summary.is_none() {
                                let first = &group[0][i];
                                v[i] = if group
                                    .iter()
                                    .all(|r| r[i].compare(first) == std::cmp::Ordering::Equal)
                                {
                                    first.clone()
                                } else {
                                    Value::Empty
                                };
                            }
                        }
                        out.lines.push(Line {
                            kind: LineKind::Subtotal,
                            values: v,
                        });
                    }
                }
                start = end;
            }
        }
    }
    if db.total && any_summary {
        let all: Vec<&Vec<Value>> = rows.iter().collect();
        let mut v = summary_line(&db.columns, &all);
        if db.columns[0].summary.is_none() {
            v[0] = Value::Text("Total".into());
        }
        out.lines.push(Line {
            kind: LineKind::Total,
            values: v,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::{RowSource, SortKey};
    use kadakas_core::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::curve::corners;
    use kadakas_core::model::{Attributes, ObjectId, Shape};
    use kadakas_core::plants::style::PlantStyle;
    use kadakas_core::record::{FieldDef, FieldType, RecordFormat};
    use kadakas_core::resource::{ResourceBody, ResourceId};
    use serde_json::json;

    fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
        ObjectId(
            ed.execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        )
    }

    fn square(side: f64) -> Vec<kadakas_core::curve::Vertex> {
        corners(&[
            DVec2::ZERO,
            DVec2::new(side, 0.0),
            DVec2::new(side, side),
            DVec2::new(0.0, side),
        ])
    }

    fn hardscape(ed: &mut Editor, material: &str, side: f64) -> ObjectId {
        add(
            ed,
            Shape::Hardscape {
                boundary: square(side),
                material: material.into(),
                thickness: None,
                elevation: None,
            },
        )
    }

    fn col(header: &str, expr: &str, summary: Option<Summary>) -> ColumnDef {
        ColumnDef {
            header: header.into(),
            expr: expr.into(),
            summary,
            ..ColumnDef::default()
        }
    }

    fn database(criteria: &str, columns: Vec<ColumnDef>) -> DatabaseDef {
        DatabaseDef {
            criteria: criteria.into(),
            columns,
            ..DatabaseDef::default()
        }
    }

    fn texts(out: &DbOutput, c: usize) -> Vec<String> {
        out.lines.iter().map(|l| l.values[c].to_text()).collect()
    }

    fn scene() -> Editor {
        let mut ed = Editor::default();
        hardscape(&mut ed, "Granite", 2000.0); // 4 m2
        hardscape(&mut ed, "Brick", 1000.0); // 1 m2
        hardscape(&mut ed, "Granite", 3000.0); // 9 m2
        add(
            &mut ed,
            Shape::LandscapeArea {
                boundary: square(5000.0),
                name: "Lawn".into(),
            },
        ); // 25 m2
        add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(1000.0, 0.0),
            },
        );
        ed
    }

    #[test]
    fn criteria_select_by_kind() {
        let ed = scene();
        for (crit, n) in [
            ("", 5),
            ("Kind = \"hardscape\"", 3),
            ("kind = 'HARDSCAPE'", 3),
            ("Kind <> \"hardscape\"", 2),
            ("Kind = \"hardscape\" AND Material = \"Granite\"", 2),
            ("Kind = \"landscape_area\" OR Kind = \"line\"", 2),
            ("NOT Kind = \"hardscape\"", 2),
            ("Area > 3", 3),
            ("Area >= 4 AND Area < 25", 2),
            ("Area", 0),
            ("Area > 0 AND Length > 0", 4),
            ("Layer = \"Layer 1\" AND Class = \"None\"", 5),
            ("Thickness = 150", 3),
            ("Style = \"Granite\"", 2),
            ("Style = \"lawn\"", 1),
            ("Count = 1", 5),
            ("IF(Area > 3, TRUE, FALSE)", 3),
            ("HASRECORD(\"Soil\")", 0),
        ] {
            let out = run(ed.doc(), &database(crit, vec![col("Id", "Id", None)]), 0);
            let n_found = out.lines.len() - 1;
            if crit == "Area" {
                // Area is a number, so non-zero means true; the line has no area (empty is false).
                assert_eq!(n_found, 4, "{crit}");
                continue;
            }
            assert_eq!(n_found, n, "{crit}");
            assert_eq!(out.matched, n);
        }
    }

    #[test]
    fn criteria_errors_and_parse_problems() {
        let ed = scene();
        let out = run(
            ed.doc(),
            &database("Bogus = 1", vec![col("Id", "Id", None)]),
            0,
        );
        assert_eq!(out.matched, 0);
        assert!(
            out.problems[0].contains("5 object(s)") && out.problems[0].contains("database 1"),
            "{:?}",
            out.problems
        );
        let out = run(
            ed.doc(),
            &database("Kind =", vec![col("Id", "Id", None)]),
            2,
        );
        assert!(out.problems[0].contains("criteria") && out.problems[0].contains("database 3"));
        assert_eq!(out.lines.len(), 1, "only the header is shown");
        let mut named = database("", vec![col("Bad", "1 +", None), col("Id", "Id", None)]);
        named.name = "Mine".into();
        let out = run(ed.doc(), &named, 0);
        assert!(
            out.problems[0].contains("database 'Mine'") && out.problems[0].contains("column 'Bad'")
        );
        assert_eq!(out.lines[1].values[0], Value::Error(ErrorKind::Parse));
        // A cell reference in a column is not allowed.
        let out = run(
            ed.doc(),
            &database("", vec![col("X", "A1", None), col("Y", "SUM(A1:B2)", None)]),
            0,
        );
        assert_eq!(
            out.lines[1].values,
            vec![Value::Error(ErrorKind::Ref), Value::Error(ErrorKind::Ref)]
        );
        // Unknown names in a column show #NAME?.
        let out = run(ed.doc(), &database("", vec![col("X", "Bogus", None)]), 0);
        assert_eq!(out.lines[1].values[0], Value::Error(ErrorKind::Name));
    }

    #[test]
    fn columns_header_and_not_applicable() {
        let ed = scene();
        let out = run(
            ed.doc(),
            &database(
                "Kind = \"hardscape\"",
                vec![
                    col("Material", "Material", None),
                    col("Area m2", "Area", None),
                    col("Double", "Area * 2", None),
                    col("Perimeter", "Perimeter", None),
                ],
            ),
            0,
        );
        assert_eq!(texts(&out, 0), ["Material", "Granite", "Brick", "Granite"]);
        assert_eq!(texts(&out, 1), ["Area m2", "4", "1", "9"]);
        assert_eq!(texts(&out, 2), ["Double", "8", "2", "18"]);
        assert_eq!(texts(&out, 3), ["Perimeter", "8", "4", "12"]);
        assert_eq!(out.lines[0].kind, LineKind::Header);
        // An object without the property gives an empty cell, and arithmetic treats it as zero.
        let out = run(
            ed.doc(),
            &database(
                "Kind = \"line\"",
                vec![
                    col("A", "Area", None),
                    col("B", "Area + 1", None),
                    col("L", "Length", None),
                ],
            ),
            0,
        );
        assert_eq!(
            out.lines[1].values,
            vec![Value::Empty, Value::Number(1.0), Value::Number(1.0)]
        );
        let mut d = database("", vec![col("Id", "Id", None)]);
        d.header = false;
        assert_eq!(run(ed.doc(), &d, 0).lines.len(), 5);
    }

    #[test]
    fn sort_by_columns_ascending_and_descending() {
        let ed = scene();
        let mut d = database(
            "Kind = \"hardscape\"",
            vec![col("Material", "Material", None), col("Area", "Area", None)],
        );
        d.sort = vec![SortKey {
            column: 1,
            descending: false,
        }];
        assert_eq!(texts(&run(ed.doc(), &d, 0), 1), ["Area", "1", "4", "9"]);
        d.sort = vec![SortKey {
            column: 1,
            descending: true,
        }];
        assert_eq!(texts(&run(ed.doc(), &d, 0), 1), ["Area", "9", "4", "1"]);
        d.sort = vec![
            SortKey {
                column: 0,
                descending: false,
            },
            SortKey {
                column: 1,
                descending: true,
            },
        ];
        let out = run(ed.doc(), &d, 0);
        assert_eq!(texts(&out, 0), ["Material", "Brick", "Granite", "Granite"]);
        assert_eq!(texts(&out, 1), ["Area", "1", "9", "4"]);
    }

    #[test]
    fn totals_and_summaries() {
        let ed = scene();
        let cols = vec![
            col("Material", "Material", None),
            col("N", "Count", Some(Summary::Sum)),
            col("Area", "Area", Some(Summary::Sum)),
            col("Avg", "Area", Some(Summary::Average)),
            col("Min", "Area", Some(Summary::Min)),
            col("Max", "Area", Some(Summary::Max)),
            col("Has", "Area", Some(Summary::Count)),
        ];
        let out = run(ed.doc(), &database("Kind = \"hardscape\"", cols.clone()), 0);
        let total = out.lines.last().unwrap();
        assert_eq!(total.kind, LineKind::Total);
        assert_eq!(total.values[0], Value::Text("Total".into()));
        assert_eq!(
            &total.values[1..],
            &[
                Value::Number(3.0),
                Value::Number(14.0),
                Value::Number(14.0 / 3.0),
                Value::Number(1.0),
                Value::Number(9.0),
                Value::Number(3.0)
            ]
        );
        let mut d = database("Kind = \"hardscape\"", cols.clone());
        d.total = false;
        assert!(
            run(ed.doc(), &d, 0)
                .lines
                .iter()
                .all(|l| l.kind != LineKind::Total)
        );
        // No summary column: no total row.
        let out = run(ed.doc(), &database("", vec![col("Id", "Id", None)]), 0);
        assert!(out.lines.iter().all(|l| l.kind != LineKind::Total));
        // Nothing matched: counts are 0, others empty.
        let out = run(ed.doc(), &database("Kind = \"plant\"", cols), 0);
        let total = out.lines.last().unwrap();
        assert_eq!(total.values[1], Value::Empty);
        assert_eq!(total.values[6], Value::Number(0.0));
        // The first column may carry the summary: no label then.
        let out = run(
            ed.doc(),
            &database(
                "Kind = \"hardscape\"",
                vec![col("N", "Count", Some(Summary::Sum))],
            ),
            0,
        );
        assert_eq!(out.lines.last().unwrap().values[0], Value::Number(3.0));
    }

    #[test]
    fn summarize_handles_errors_and_text() {
        let e = Value::Error(ErrorKind::Div0);
        let n = Value::Number(2.0);
        let t = Value::Text("x".into());
        assert_eq!(summarize(Summary::Sum, &[&n, &e]), e);
        assert_eq!(summarize(Summary::Count, &[&n, &e]), e);
        assert_eq!(summarize(Summary::Sum, &[&t, &n, &Value::Empty]), n);
        assert_eq!(
            summarize(Summary::Count, &[&t, &n, &Value::Empty]),
            Value::Number(2.0)
        );
        assert_eq!(summarize(Summary::Max, &[&t]), Value::Empty);
        assert_eq!(
            summarize(Summary::Min, &[&n, &Value::Number(-1.0)]),
            Value::Number(-1.0)
        );
    }

    #[test]
    fn grouping_in_detail_and_summary_mode() {
        let ed = scene();
        let cols = vec![
            col("Material", "Material", None),
            col("Area", "Area", Some(Summary::Sum)),
            col("Layer", "Layer", None),
        ];
        let mut d = database("Kind = \"hardscape\"", cols.clone());
        d.group_by = Some(0);
        let out = run(ed.doc(), &d, 0);
        assert_eq!(
            texts(&out, 0),
            [
                "Material",
                "Brick",
                "Brick total",
                "Granite",
                "Granite",
                "Granite total",
                "Total"
            ]
        );
        assert_eq!(texts(&out, 1), ["Area", "1", "1", "4", "9", "13", "14"]);
        assert_eq!(out.lines[2].kind, LineKind::Subtotal);
        d.group_mode = GroupMode::Summary;
        let out = run(ed.doc(), &d, 0);
        assert_eq!(texts(&out, 0), ["Material", "Brick", "Granite", "Total"]);
        assert_eq!(texts(&out, 1), ["Area", "1", "13", "14"]);
        assert_eq!(
            texts(&out, 2),
            ["Layer", "Layer 1", "Layer 1", ""],
            "equal values show, the total has none"
        );
        // Group by a summarized column keeps its number in detail mode.
        let mut d = database("Kind = \"hardscape\"", cols);
        d.group_by = Some(1);
        let out = run(ed.doc(), &d, 0);
        assert_eq!(texts(&out, 1), ["Area", "1", "1", "4", "4", "9", "9", "14"]);
        // No summary column at all: groups are sorted, with no subtotal rows.
        let mut d = database(
            "Kind = \"hardscape\"",
            vec![col("Material", "Material", None)],
        );
        d.group_by = Some(0);
        assert_eq!(
            texts(&run(ed.doc(), &d, 0), 0),
            ["Material", "Brick", "Granite", "Granite"]
        );
        // Summary mode: a column that differs inside the group is empty.
        let mut d = database(
            "Kind = \"hardscape\"",
            vec![
                col("Kind", "Kind", None),
                col("Material", "Material", None),
                col("N", "Count", Some(Summary::Sum)),
            ],
        );
        d.group_by = Some(0);
        d.group_mode = GroupMode::Summary;
        let out = run(ed.doc(), &d, 0);
        assert_eq!(
            out.lines[1].values,
            vec![
                Value::Text("hardscape".into()),
                Value::Empty,
                Value::Number(3.0)
            ]
        );
    }

    #[test]
    fn plants_and_plant_fields() {
        let mut ed = Editor::default();
        let style = |name: &str, botanical: &str, price: f64| Command::AddResource {
            name: name.into(),
            body: ResourceBody::plant_style(PlantStyle {
                botanical_name: botanical.into(),
                price,
                spread: 3000.0,
                ..PlantStyle::default()
            }),
        };
        let oak = ResourceId(
            ed.execute(&style("Oak", "Quercus robur", 100.0))
                .unwrap()
                .created[0],
        );
        let birch = ResourceId(
            ed.execute(&style("Birch", "Betula pendula", 40.0))
                .unwrap()
                .created[0],
        );
        for (s, n) in [(oak, 2), (birch, 3)] {
            ed.execute(&Command::AddPlants {
                style: Some(s),
                overrides: Default::default(),
                positions: (0..n)
                    .map(|i| DVec2::new(f64::from(i) * 5000.0, 0.0))
                    .collect(),
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
        }
        let cols = vec![
            col("Style", "Style", None),
            col("Botanical", "Botanical", None),
            col("Count", "Count", Some(Summary::Sum)),
            col("Cost", "Plant.price", Some(Summary::Sum)),
        ];
        let mut d = database("Kind = \"plant\"", cols);
        d.group_by = Some(0);
        d.group_mode = GroupMode::Summary;
        let out = run(ed.doc(), &d, 0);
        assert_eq!(texts(&out, 0), ["Style", "Birch", "Oak", "Total"]);
        assert_eq!(
            texts(&out, 1),
            ["Botanical", "Betula pendula", "Quercus robur", ""]
        );
        assert_eq!(texts(&out, 2), ["Count", "3", "2", "5"]);
        assert_eq!(texts(&out, 3), ["Cost", "120", "200", "320"]);
    }

    #[test]
    fn record_fields_in_criteria_and_columns() {
        let mut ed = scene();
        let fmt = RecordFormat {
            fields: vec![
                FieldDef::new("Depth", FieldType::Dimension),
                FieldDef::new("Note", FieldType::Text),
            ],
        };
        let rid = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Soil".into(),
                body: ResourceBody::RecordFormat(Box::new(fmt)),
            })
            .unwrap()
            .created[0],
        );
        let ids: Vec<ObjectId> = ed.doc().objects.keys().copied().collect();
        ed.execute(&Command::AttachRecord {
            ids: vec![ids[0]],
            record: rid,
            values: json!({"Depth": 300, "Note": "a"})
                .as_object()
                .unwrap()
                .clone(),
        })
        .unwrap();
        ed.execute(&Command::AttachRecord {
            ids: vec![ids[1]],
            record: rid,
            values: json!({"Depth": 100}).as_object().unwrap().clone(),
        })
        .unwrap();
        let cols = vec![
            col("Id", "Id", None),
            col("Depth", "Soil.Depth", Some(Summary::Sum)),
            col("Same", "FIELD(\"Soil\", \"Depth\")", None),
            col("Quoted", "'Soil'.'Note'", None),
        ];
        let out = run(ed.doc(), &database("HASRECORD(\"Soil\")", cols.clone()), 0);
        assert_eq!(out.matched, 2);
        assert_eq!(texts(&out, 1), ["Depth", "300", "100", "400"]);
        assert_eq!(texts(&out, 2)[1], "300");
        assert_eq!(texts(&out, 3), ["Quoted", "a", "", ""]);
        let out = run(ed.doc(), &database("Soil.Depth > 200", cols.clone()), 0);
        assert_eq!(out.matched, 1);
        let out = run(ed.doc(), &database("Soil.Depth <= 200", cols.clone()), 0);
        assert_eq!(out.matched, 1, "objects without the record do not match");
        // Names that do not exist are errors, not silent misses.
        let out = run(ed.doc(), &database("Soil.Nope = 1", cols), 0);
        assert_eq!(out.matched, 0);
        assert!(!out.problems.is_empty());
    }

    #[test]
    fn plant_list_rows_have_names_and_errors() {
        let mut ed = Editor::default();
        ed.execute(&Command::AddPlants {
            style: None,
            overrides: Default::default(),
            positions: vec![DVec2::ZERO, DVec2::new(3000.0, 0.0)],
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        let cols = vec![
            col("Style", "Style", None),
            col("Code", "Code", None),
            col("Count", "Count", Some(Summary::Sum)),
            col("Kind", "Kind", None),
            col("Plants", "Plants", None),
            col("In areas", "InAreas", None),
            col("Price", "Price", None),
            col("Cost", "Cost", None),
            col("Area", "Area", None),
            col("Cat", "Category", None),
            col("Bad", "Bogus", None),
            col("Path", "Plant.price", None),
            col("Cell", "A1", None),
        ];
        let mut d = database("Kind = \"plant\" AND NOT Style = \"\"", cols);
        d.source = RowSource::PlantList;
        let out = run(ed.doc(), &d, 0);
        assert_eq!(out.matched, 1);
        let row = &out.lines[1].values;
        assert_eq!(row[2], Value::Number(2.0));
        assert_eq!(row[3], Value::Text("plant".into()));
        assert_eq!(
            (row[4].clone(), row[5].clone()),
            (Value::Number(2.0), Value::Number(0.0))
        );
        assert_eq!(row[10], Value::Error(ErrorKind::Name));
        assert_eq!(row[11], Value::Error(ErrorKind::Name));
        assert_eq!(row[12], Value::Error(ErrorKind::Ref));
        let mut d2 = database(
            "SUM(A1:B2) = 0 OR HASRECORD(\"x\")",
            vec![col("S", "Style", None)],
        );
        d2.source = RowSource::PlantList;
        assert_eq!(run(ed.doc(), &d2, 0).matched, 0, "errors leave a row out");
        let mut d3 = database(
            "HASRECORD(\"x\") OR Style = \"nothing\"",
            vec![col("S", "Style", None)],
        );
        d3.source = RowSource::PlantList;
        assert_eq!(run(ed.doc(), &d3, 0).matched, 0);
    }

    #[test]
    fn the_image_column_shows_the_picture_of_the_plant_style() {
        use kadakas_core::picture::header_only_sample;
        let mut ed = Editor::default();
        let style = |ed: &mut Editor, name: &str| {
            kadakas_core::resource::ResourceId(
                ed.execute(&Command::AddResource {
                    name: name.into(),
                    body: kadakas_core::resource::ResourceBody::plant_style(Default::default()),
                })
                .unwrap()
                .created[0],
            )
        };
        let (a, b) = (style(&mut ed, "Maple"), style(&mut ed, "Oak"));
        let out = ed
            .execute(&Command::SetPlantPicture {
                style: a,
                picture: Some(Box::new(header_only_sample(1))),
            })
            .unwrap();
        let img = kadakas_core::resource::ResourceId(out.created[0]);
        for s in [a, b] {
            ed.execute(&Command::AddPlants {
                style: Some(s),
                overrides: Default::default(),
                positions: vec![DVec2::ZERO],
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
        }
        // The plant list: one row per style.
        let mut d = database(
            "",
            vec![col("Style", "Style", None), col("Pic", "Image", None)],
        );
        d.source = RowSource::PlantList;
        let out = run(ed.doc(), &d, 0);
        let pics: Vec<Value> = out.lines[1..].iter().map(|l| l.values[1].clone()).collect();
        assert_eq!(pics, vec![Value::Image(img), Value::Empty]);
        // Objects: a plant gets the picture of its style. Other objects have none.
        let d = database("Kind = \"plant\"", vec![col("Pic", "Picture", None)]);
        let out = run(ed.doc(), &d, 0);
        let mut got: Vec<Value> = out.lines[1..].iter().map(|l| l.values[0].clone()).collect();
        got.sort_by(|x, y| x.compare(y));
        assert_eq!(got, vec![Value::Empty, Value::Image(img)]);
        // A text column for a picture is empty text, so CSV skips it.
        assert_eq!(Value::Image(img).to_text(), "");
    }

    #[test]
    fn many_objects_are_fast() {
        let mut ed = Editor::default();
        let positions: Vec<DVec2> = (0..5000)
            .map(|i| DVec2::new(f64::from(i) * 100.0, 0.0))
            .collect();
        ed.execute(&Command::AddPlants {
            style: None,
            overrides: Default::default(),
            positions,
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        let out = run(
            ed.doc(),
            &database(
                "Kind = \"plant\"",
                vec![
                    col("N", "Count", Some(Summary::Sum)),
                    col("A", "Area", Some(Summary::Sum)),
                ],
            ),
            0,
        );
        assert_eq!(out.matched, 5000);
    }

    #[test]
    fn query_finds_objects_by_criteria() {
        let mut ed = Editor::default();
        let a = add(
            &mut ed,
            Shape::Polyline {
                points: square(1000.0),
                closed: true,
            },
        );
        let b = add(
            &mut ed,
            Shape::Polyline {
                points: square(5000.0),
                closed: true,
            },
        );
        add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 10.0,
            },
        );
        let q = |crit: &str, cols: &[&str]| {
            let cols: Vec<String> = cols.iter().map(|c| c.to_string()).collect();
            query_objects(ed.doc(), crit, &cols, &NoTerrain)
        };
        assert_eq!(q("", &[]).unwrap().ids.len(), 3);
        assert_eq!(q("   ", &[]).unwrap().ids.len(), 3);
        let r = q("Kind = \"polyline\" AND Area > 10", &["Kind", "Area"]).unwrap();
        assert_eq!(r.ids, vec![b]);
        assert_eq!(r.rows.len(), 1);
        assert_eq!(r.rows[0][0], Value::Text("polyline".into()));
        let r = q("Kind = \"polyline\"", &[]).unwrap();
        assert_eq!(r.ids, vec![a, b]);
        assert!(r.rows.iter().all(Vec::is_empty));
        // An error in the criteria for an object leaves it out and counts it.
        let r = q("NoSuchProperty > 1", &[]).unwrap();
        assert_eq!((r.ids.len(), r.errors), (0, 3));
        // A name that is not a property in the columns gives an error value, not a failure.
        let r = q("Kind = \"circle\"", &["NoSuchProperty"]).unwrap();
        assert!(r.rows[0][0].is_error());
        // Bad text.
        assert!(q("Kind =", &[]).unwrap_err().starts_with("criteria:"));
        assert!(q("", &["((("]).unwrap_err().starts_with("column '((('"));
    }
}
