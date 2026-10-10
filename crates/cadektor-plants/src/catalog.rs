//! Plant catalog: plant styles from and to CSV text.
//!
//! The import maps columns by their header. A header is matched without case and
//! punctuation, and a list of common names works (`Botanical name`, `Latin`, `Species`
//! all give the botanical name). A header can carry a unit, for example `Spread (m)` or
//! `height_cm`. Columns without a unit use the unit that the caller gives (default mm).
//! Columns that match no field become custom data. A header `custom:water need` always
//! gives the custom field `water need`. The caller can also map fields to headers.
//!
//! The delimiter (comma, semicolon or tab) is found from the header line.
//! Quoted fields can hold delimiters, quotes (doubled) and line breaks.

use std::collections::BTreeMap;

use cadektor_core::csv::parse;
use cadektor_core::model::Color;
use cadektor_core::plants::style::{PlantCategory, PlantForm, PlantGraphic, PlantStyle};

/// Fields that a column can set, in export order.
pub const FIELDS: [&str; 19] = [
    "name",
    "botanical_name",
    "common_name",
    "id_code",
    "category",
    "spread",
    "height",
    "graphic",
    "form",
    "fill",
    "pen",
    "price",
    "size",
    "container",
    "root",
    "water_need",
    "hydrozone",
    "hours_per_year",
    "image",
];

/// The unit of lengths in a CSV file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unit {
    #[default]
    Mm,
    Cm,
    M,
}

impl Unit {
    pub fn from_name(s: &str) -> Option<Unit> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mm" => Some(Unit::Mm),
            "cm" => Some(Unit::Cm),
            "m" => Some(Unit::M),
            _ => None,
        }
    }

    fn to_mm(self) -> f64 {
        match self {
            Unit::Mm => 1.0,
            Unit::Cm => 10.0,
            Unit::M => 1000.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Headers

/// Lowercase letters and digits of a header, split at other characters.
fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

fn field_for_key(key: &str) -> Option<&'static str> {
    Some(match key {
        "name" | "stylename" | "style" | "plantname" | "plantstyle" => "name",
        "botanical" | "botanicalname" | "latin" | "latinname" | "species" | "scientificname"
        | "scientific" => "botanical_name",
        "common" | "commonname" => "common_name",
        "id" | "idcode" | "tag" | "tagid" | "plantid" | "code" | "plantcode" | "tagcode" => {
            "id_code"
        }
        "category" | "type" | "group" | "plantcategory" | "planttype" => "category",
        "spread" | "width" | "canopy" | "diameter" | "crown" => "spread",
        "height" | "matureheight" => "height",
        "graphic" | "symbol" | "outline" | "graphicstyle" => "graphic",
        "form" | "shape" | "habit" | "form3d" => "form",
        "fill" | "color" | "colour" | "fillcolor" | "fillcolour" => "fill",
        "pen" | "pencolor" | "pencolour" | "outlinecolor" => "pen",
        "price" | "cost" | "unitprice" => "price",
        "size" | "plantsize" => "size",
        "container" | "pot" | "potsize" | "containersize" => "container",
        "root" | "roottype" | "rootball" => "root",
        "waterneed" | "wateruse" | "wucols" | "waterusecategory" => "water_need",
        "hydrozone" | "hydrozones" | "irrigationzone" | "zone" => "hydrozone",
        "hoursperyear" | "maintenancehours" | "annualhours" => "hours_per_year",
        "image" | "picture" | "photo" | "imagename" => "image",
        _ => return None,
    })
}

/// What one column does.
#[derive(Clone, Debug, PartialEq)]
enum Column {
    Field(&'static str, Option<Unit>),
    Custom(String),
}

fn classify(header: &str) -> Column {
    if let Some(rest) = header.trim().strip_prefix("custom:") {
        return Column::Custom(rest.trim().to_string());
    }
    let toks = tokens(header);
    let whole = toks.join("");
    if let Some(f) = field_for_key(&whole) {
        return Column::Field(f, None);
    }
    if toks.len() > 1
        && let Some(unit) = Unit::from_name(toks.last().unwrap())
        && let Some(f) = field_for_key(&toks[..toks.len() - 1].join(""))
    {
        return Column::Field(f, Some(unit));
    }
    Column::Custom(header.trim().to_string())
}

// ---------------------------------------------------------------------------
// Values

fn hex(c: Color) -> String {
    let [r, g, b, a] = c.0;
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

fn parse_color(s: &str) -> Result<Option<Color>, String> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let h = s.strip_prefix('#').unwrap_or(s);
    if (h.len() == 6 || h.len() == 8) && h.chars().all(|c| c.is_ascii_hexdigit()) {
        let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
        return Ok(Some(Color([
            b(0),
            b(2),
            b(4),
            if h.len() == 8 { b(6) } else { 255 },
        ])));
    }
    Err(format!("'{s}' is not a color. Use #RRGGBB"))
}

/// Reads a number. Returns the value, and true if the text is ambiguous.
///
/// - Spaces and apostrophes are thousands marks.
/// - With both `.` and `,`, the last one is the decimal mark: `1.234,50` and `1,234.50` are both 1234.5.
/// - A mark that appears more than once is a thousands mark: `1,234,567`.
/// - A single `,` is a decimal comma (`1,5` is 1.5). If exactly three digits follow it, such as
///   `1,234`, the text could also be a thousands mark. It is read as a decimal comma and flagged.
fn parse_number(s: &str) -> Result<Option<(f64, bool)>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let t: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\'')
        .collect();
    let (dots, commas) = (t.matches('.').count(), t.matches(',').count());
    let mut ambiguous = false;
    let clean = match (dots, commas) {
        (0, 0) => t.clone(),
        (_, 0) if dots == 1 => t.clone(),
        (_, 0) => t.replace('.', ""),
        (0, 1) => {
            let (head, tail) = t.split_once(',').expect("one comma");
            ambiguous = tail.len() == 3
                && tail.chars().all(|c| c.is_ascii_digit())
                && !head.is_empty()
                && head.len() <= 3;
            t.replace(',', ".")
        }
        (0, _) => t.replace(',', ""),
        _ => {
            // Both marks: the one that comes last is the decimal mark.
            let decimal = if t.rfind('.') > t.rfind(',') {
                '.'
            } else {
                ','
            };
            let thousands = if decimal == '.' { ',' } else { '.' };
            // The decimal mark must appear once, and the thousands mark must not come after it.
            if t.matches(decimal).count() != 1 || t.rfind(thousands) > t.rfind(decimal) {
                return Err(format!("'{s}' is not a number"));
            }
            t.replace(thousands, "").replace(',', ".")
        }
    };
    clean
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| Some((v, ambiguous)))
        .ok_or_else(|| format!("'{s}' is not a number"))
}

// ---------------------------------------------------------------------------
// Import

/// Options of an import.
#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    /// The unit of spread and height columns that have no unit in the header.
    pub unit: Unit,
    /// Field name to header text. A mapped field ignores the automatic match.
    pub mapping: BTreeMap<String, String>,
}

/// One style read from a CSV row.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedStyle {
    pub name: String,
    pub style: PlantStyle,
    /// The fields (and `custom:key` entries) that the row gave a value for.
    pub given: Vec<String>,
}

impl ImportedStyle {
    /// `base` with the fields of this row applied. Fields that the row left empty, or that the file
    /// has no column for, stay as they are in `base`. The name is not part of the style.
    pub fn apply_to(&self, base: &PlantStyle) -> PlantStyle {
        let mut out = serde_json::to_value(base).expect("a style is JSON");
        let new = serde_json::to_value(&self.style).expect("a style is JSON");
        for g in &self.given {
            match g.strip_prefix("custom:") {
                Some(k) => {
                    out["custom"][k] = new["custom"][k].clone();
                }
                None => {
                    if let Some(v) = new.get(g.as_str()) {
                        out[g.as_str()] = v.clone();
                    }
                }
            }
        }
        serde_json::from_value(out).expect("fields of a style stay a style")
    }
}

/// The styles read from a CSV file, and the rows that were not used.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportResult {
    pub styles: Vec<ImportedStyle>,
    /// One message per skipped row or ignored value, with the row number (the header is row 1).
    pub warnings: Vec<String>,
}

/// Reads plant styles from CSV text.
pub fn import(text: &str, opts: &ImportOptions) -> Result<ImportResult, String> {
    let rows = parse(text)?;
    let Some((head, body)) = rows.split_first() else {
        return Err("the file is empty".into());
    };
    let mut cols: Vec<Column> = head.iter().map(|h| classify(h)).collect();
    for (field, header) in &opts.mapping {
        let field = FIELDS
            .iter()
            .find(|f| **f == field.as_str())
            .ok_or_else(|| format!("unknown field '{field}' in the mapping"))?;
        let i = head
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(header.trim()))
            .ok_or_else(|| {
                format!("the mapping names a column '{header}' that the file does not have")
            })?;
        // A column that was mapped by hand replaces the automatic match of the same field.
        for c in &mut cols {
            if matches!(c, Column::Field(f, _) if f == field) {
                *c = Column::Custom(String::new());
            }
        }
        cols[i] = Column::Field(field, None);
    }
    if !cols.iter().any(|c| {
        matches!(
            c,
            Column::Field("name" | "botanical_name" | "common_name", _)
        )
    }) {
        return Err("no column gives a name. Add a 'name' or 'botanical name' column".into());
    }
    let mut out = ImportResult::default();
    for (n, row) in body.iter().enumerate() {
        let line = n + 2;
        match read_row(row, &cols, head, opts.unit) {
            Ok((imported, warns)) => {
                out.warnings
                    .extend(warns.into_iter().map(|w| format!("row {line}: {w}")));
                if out
                    .styles
                    .iter()
                    .any(|s| s.name.eq_ignore_ascii_case(&imported.name))
                {
                    out.warnings.push(format!(
                        "row {line}: skipped, the name '{}' appears twice",
                        imported.name
                    ));
                } else {
                    out.styles.push(imported);
                }
            }
            Err(e) => out.warnings.push(format!("row {line}: skipped, {e}")),
        }
    }
    Ok(out)
}

fn read_row(
    row: &[String],
    cols: &[Column],
    head: &[String],
    unit: Unit,
) -> Result<(ImportedStyle, Vec<String>), String> {
    let mut s = PlantStyle::default();
    let mut name = String::new();
    let mut warns = Vec::new();
    let mut given: Vec<String> = Vec::new();
    for (i, col) in cols.iter().enumerate() {
        let cell = row.get(i).map(|c| c.trim()).unwrap_or("");
        match col {
            Column::Custom(_) if cell.is_empty() => {}
            Column::Custom(k) if k.is_empty() => {}
            Column::Custom(k) => {
                s.custom.insert(k.clone(), cell.to_string());
                given.push(format!("custom:{k}"));
            }
            Column::Field(_, _) if cell.is_empty() => {}
            Column::Field(f, u) => {
                let warned = warns.len();
                let num = |warns: &mut Vec<String>| -> Result<Option<f64>, String> {
                    match parse_number(cell).map_err(|e| format!("{} {e}", head[i])) {
                        Ok(Some((v, ambiguous))) => {
                            if ambiguous {
                                warns.push(format!(
                                    "{} '{cell}' could be {} or {v}. Read as {v}",
                                    head[i],
                                    v * 1000.0
                                ));
                            }
                            Ok(Some(v))
                        }
                        Ok(None) => Ok(None),
                        Err(e) => {
                            warns.push(e.clone());
                            Err(e)
                        }
                    }
                };
                match *f {
                    "name" => name = cell.to_string(),
                    "botanical_name" => s.botanical_name = cell.to_string(),
                    "common_name" => s.common_name = cell.to_string(),
                    "id_code" => s.id_code = cell.to_string(),
                    "size" => s.size = cell.to_string(),
                    "container" => s.container = cell.to_string(),
                    "root" => s.root = cell.to_string(),
                    "category" => match PlantCategory::from_name(cell) {
                        Some(c) => s.category = c,
                        None => warns.push(format!("'{cell}' is not a plant category. Used shrub")),
                    },
                    "graphic" => match PlantGraphic::from_name(cell) {
                        Some(g) => s.graphic = g,
                        None => warns.push(format!("'{cell}' is not a plan graphic. Used circle")),
                    },
                    "form" => match PlantForm::from_name(cell) {
                        Some(g) => s.form = g,
                        None => warns.push(format!("'{cell}' is not a 3D form. Used tree")),
                    },
                    "fill" | "pen" => match parse_color(cell) {
                        Ok(c) if *f == "fill" => s.fill = c,
                        Ok(c) => s.pen = c,
                        Err(e) => warns.push(e),
                    },
                    "water_need" => match cadektor_core::plants::care::WaterNeed::from_name(cell) {
                        Some(w) => s.irrigation.water_need = w,
                        None => warns.push(format!(
                            "'{cell}' is not a water need. Use very low, low, moderate or high"
                        )),
                    },
                    "hydrozone" => s.irrigation.hydrozone = cell.to_string(),
                    // The picture is a resource of the document. The file only names it.
                    "image" => {}
                    "hours_per_year" => {
                        if let Ok(Some(v)) = num(&mut warns) {
                            s.maintenance.hours_per_year = v;
                        }
                    }
                    "price" => {
                        if let Ok(Some(v)) = num(&mut warns) {
                            s.price = v;
                        }
                    }
                    "spread" | "height" => {
                        if let Ok(Some(v)) = num(&mut warns) {
                            let mm = v * u.unwrap_or(unit).to_mm();
                            if *f == "spread" {
                                s.spread = mm
                            } else {
                                s.height = mm
                            }
                        }
                    }
                    _ => unreachable!("FIELDS lists every field"),
                }
                // A value that gave a warning does not count as given.
                if warns.len() == warned {
                    given.push(f.to_string());
                }
            }
        }
    }
    if name.is_empty() {
        name = [&s.botanical_name, &s.common_name]
            .into_iter()
            .find(|n| !n.is_empty())
            .cloned()
            .unwrap_or_default();
    }
    if name.is_empty() {
        return Err("no name".into());
    }
    s.validate().map_err(|e| format!("'{name}': {e}"))?;
    Ok((
        ImportedStyle {
            name,
            style: s,
            given,
        },
        warns,
    ))
}

// ---------------------------------------------------------------------------
// Export

fn number(v: f64) -> String {
    // The shortest text that reads back as the same number.
    format!("{v}")
}

/// CSV text for plant styles. Lengths are in mm. The header names the unit, so the file imports again unchanged.
pub fn export<'a>(styles: impl IntoIterator<Item = (&'a str, &'a PlantStyle)>) -> String {
    let styles: Vec<_> = styles.into_iter().collect();
    let custom: Vec<&String> = {
        let mut keys: Vec<&String> = styles.iter().flat_map(|(_, s)| s.custom.keys()).collect();
        keys.sort();
        keys.dedup();
        keys
    };
    let mut head: Vec<String> = FIELDS
        .iter()
        .map(|f| {
            if matches!(*f, "spread" | "height") {
                format!("{f}_mm")
            } else {
                f.to_string()
            }
        })
        .collect();
    head.extend(custom.iter().map(|k| format!("custom:{k}")));
    let mut rows = vec![head];
    for (name, s) in styles {
        let mut r = vec![
            name.to_string(),
            s.botanical_name.clone(),
            s.common_name.clone(),
            s.id_code.clone(),
            s.category.id().into(),
            number(s.spread),
            number(s.height),
            s.graphic.id().into(),
            s.form.id().into(),
            s.fill.map(hex).unwrap_or_default(),
            s.pen.map(hex).unwrap_or_default(),
            number(s.price),
            s.size.clone(),
            s.container.clone(),
            s.root.clone(),
            s.irrigation.water_need.id().into(),
            s.irrigation.hydrozone.clone(),
            number(s.maintenance.hours_per_year),
            String::new(),
        ];
        r.extend(
            custom
                .iter()
                .map(|k| s.custom.get(*k).cloned().unwrap_or_default()),
        );
        rows.push(r);
    }
    cadektor_core::csv::write_rows(&rows)
}

/// A sample catalog, for the documentation and for tests.
pub const SAMPLE_CSV: &str = include_str!("sample_catalog.csv");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quotes_line_breaks_and_delimiters() {
        let rows = parse("a,b\r\n\"x, y\",\"he said \"\"hi\"\"\"\n\n\"multi\nline\",z").unwrap();
        assert_eq!(
            rows,
            vec![
                vec!["a", "b"],
                vec!["x, y", "he said \"hi\""],
                vec!["multi\nline", "z"]
            ]
        );
        assert_eq!(parse("a;b\n1,5;2").unwrap()[1], vec!["1,5", "2"]);
        assert_eq!(parse("a\tb\n1\t2").unwrap()[1], vec!["1", "2"]);
        assert_eq!(parse("\u{feff}a,b\n1,2\r3,4").unwrap().len(), 3);
        assert_eq!(parse("a,b\n\"\",x").unwrap()[1], vec!["", "x"]);
        assert!(parse("a,\"b").is_err());
        assert!(parse("").unwrap().is_empty());
        assert_eq!(parse("a,b\n\"\"\n").unwrap().len(), 2);
    }

    #[test]
    fn writes_quotes_where_needed() {
        let rows = vec![vec![
            "a b".to_string(),
            "c,d".into(),
            "e\"f".into(),
            " lead".into(),
            "".into(),
        ]];
        assert_eq!(
            cadektor_core::csv::write_rows(&rows),
            "a b,\"c,d\",\"e\"\"f\",\" lead\",\n"
        );
        assert_eq!(parse(&cadektor_core::csv::write_rows(&rows)).unwrap(), rows);
    }

    #[test]
    fn headers_match_by_name_synonym_and_unit() {
        assert_eq!(
            classify("Botanical Name"),
            Column::Field("botanical_name", None)
        );
        assert_eq!(classify("LATIN"), Column::Field("botanical_name", None));
        assert_eq!(
            classify("Spread (m)"),
            Column::Field("spread", Some(Unit::M))
        );
        assert_eq!(
            classify("height_cm"),
            Column::Field("height", Some(Unit::Cm))
        );
        assert_eq!(
            classify("spread_mm"),
            Column::Field("spread", Some(Unit::Mm))
        );
        assert_eq!(classify("Tag ID"), Column::Field("id_code", None));
        assert_eq!(
            classify("custom:Water Need"),
            Column::Custom("Water Need".into())
        );
        assert_eq!(classify("Water need"), Column::Field("water_need", None));
        assert_eq!(classify("Hydrozone"), Column::Field("hydrozone", None));
        assert_eq!(
            classify("Hours per year"),
            Column::Field("hours_per_year", None)
        );
        assert_eq!(classify("Image"), Column::Field("image", None));
        assert_eq!(
            classify("weight (kg)"),
            Column::Custom("weight (kg)".into())
        );
    }

    #[test]
    fn numbers_and_colors() {
        let n = |s: &str| parse_number(s).unwrap().unwrap();
        assert_eq!(n("1,5"), (1.5, false));
        assert_eq!(n("1,200.5"), (1200.5, false));
        assert_eq!(n("1.234,50"), (1234.5, false));
        assert_eq!(n("1.234.567,5"), (1_234_567.5, false));
        assert_eq!(n("1,234,567.25"), (1_234_567.25, false));
        assert_eq!(n("1,234,567"), (1_234_567.0, false));
        assert_eq!(n("1.234.567"), (1_234_567.0, false));
        assert_eq!(n("1 234,5"), (1234.5, false));
        assert_eq!(n("1'234.5"), (1234.5, false));
        assert_eq!(n("0.5"), (0.5, false));
        // One comma and three digits could be a thousands mark.
        assert_eq!(n("1,234"), (1.234, true));
        assert_eq!(n("12,345"), (12.345, true));
        assert_eq!(n("1,2345"), (1.2345, false));
        assert_eq!(n("0,123"), (0.123, true));
        assert!(parse_number("1,234.5,6").is_err());
        assert!(parse_number("1.2,3.4").is_err());
        assert!(parse_number("1,5.5,5").is_err());
        assert_eq!(n(" 7 "), (7.0, false));
        assert_eq!(parse_number(""), Ok(None));
        assert!(parse_number("abc").is_err());
        assert!(parse_number("inf").is_err());
        assert_eq!(
            parse_color("#336699"),
            Ok(Some(Color([0x33, 0x66, 0x99, 255])))
        );
        assert_eq!(
            parse_color("33669980"),
            Ok(Some(Color([0x33, 0x66, 0x99, 0x80])))
        );
        assert_eq!(parse_color("none"), Ok(None));
        assert!(parse_color("red").is_err());
        assert_eq!(hex(Color([1, 2, 3, 255])), "#010203");
        assert_eq!(hex(Color([1, 2, 3, 4])), "#01020304");
    }

    #[test]
    fn imports_with_units_and_custom_columns() {
        let csv = "Style name,Latin,Spread (m),Height cm,Category,Price,Water need\nOak,Quercus robur,8,1500,Trees,120.5,low\n";
        let r = import(csv, &ImportOptions::default()).unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let s = &r.styles[0];
        assert_eq!(s.name, "Oak");
        assert_eq!(s.style.botanical_name, "Quercus robur");
        assert_eq!(
            (s.style.spread, s.style.height, s.style.price),
            (8000.0, 15000.0, 120.5)
        );
        assert_eq!(s.style.category, PlantCategory::Tree);
        assert_eq!(
            s.style.irrigation.water_need,
            cadektor_core::plants::care::WaterNeed::Low
        );
    }

    #[test]
    fn unit_option_applies_to_columns_without_a_unit() {
        let csv = "name,spread,height\nA,2,3\n";
        let r = import(
            csv,
            &ImportOptions {
                unit: Unit::M,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (r.styles[0].style.spread, r.styles[0].style.height),
            (2000.0, 3000.0)
        );
        let r = import(
            csv,
            &ImportOptions {
                unit: Unit::Cm,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.styles[0].style.spread, 20.0);
        assert_eq!(Unit::from_name(" CM "), Some(Unit::Cm));
        assert_eq!(Unit::from_name("ft"), None);
    }

    #[test]
    fn explicit_mapping_overrides_the_automatic_match() {
        let csv = "Plant,Bloom,Wide\nRose,Rosa,1200\n";
        let mut mapping = BTreeMap::new();
        mapping.insert("name".to_string(), "Plant".to_string());
        mapping.insert("botanical_name".to_string(), "Bloom".to_string());
        mapping.insert("spread".to_string(), "wide".to_string());
        let r = import(
            csv,
            &ImportOptions {
                mapping,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.styles[0].name, "Rose");
        assert_eq!(r.styles[0].style.botanical_name, "Rosa");
        assert_eq!(r.styles[0].style.spread, 1200.0);
        // Remapping a field that matched automatically replaces the automatic column.
        let csv = "Name,Width,Span\nA,100,900\n";
        let mut mapping = BTreeMap::new();
        mapping.insert("spread".to_string(), "Span".to_string());
        let r = import(
            csv,
            &ImportOptions {
                mapping,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.styles[0].style.spread, 900.0);
    }

    #[test]
    fn mapping_errors() {
        let csv = "Name\nA\n";
        let mut m = BTreeMap::new();
        m.insert("spread".to_string(), "Nope".to_string());
        assert!(
            import(
                csv,
                &ImportOptions {
                    mapping: m,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .contains("Nope")
        );
        let mut m = BTreeMap::new();
        m.insert("colour".to_string(), "Name".to_string());
        assert!(
            import(
                csv,
                &ImportOptions {
                    mapping: m,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .contains("unknown field")
        );
        assert!(import("", &ImportOptions::default()).is_err());
        assert!(
            import("price\n3\n", &ImportOptions::default())
                .unwrap_err()
                .contains("name")
        );
    }

    #[test]
    fn european_numbers_import_and_ambiguous_ones_warn() {
        let csv = "name;spread;height;price\nA;2,5;3,000;1.234,50\nB;1,234;1;2\n";
        let r = import(
            csv,
            &ImportOptions {
                unit: Unit::M,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.styles[0].style.price, 1234.5);
        assert_eq!(r.styles[0].style.spread, 2500.0);
        assert_eq!(r.styles[1].style.spread, 1234.0);
        let w = r.warnings.join("\n");
        assert_eq!(r.warnings.len(), 2, "{w}");
        assert!(
            w.contains("row 2") && w.contains("'3,000' could be 3000 or 3"),
            "{w}"
        );
        assert!(w.contains("row 3") && w.contains("'1,234'"), "{w}");
    }

    #[test]
    fn bad_rows_are_skipped_with_a_warning() {
        let csv = "name,spread,category,fill,graphic,form,price\n\
                   Good,1000,tree,#00FF00,star,conical,5\n\
                   ,1000,,,,,\n\
                   Bad spread,-5,,,,,\n\
                   Odd,abc,wizard,pink,blob,cube,xyz\n\
                   good,1000,,,,,\n";
        let r = import(csv, &ImportOptions::default()).unwrap();
        let names: Vec<_> = r.styles.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Good", "Odd"]);
        assert_eq!(r.styles[0].style.graphic, PlantGraphic::Star);
        assert_eq!(r.styles[0].style.form, PlantForm::Conical);
        assert_eq!(r.styles[0].style.fill, Some(Color([0, 255, 0, 255])));
        let w = r.warnings.join("\n");
        assert!(w.contains("row 3: skipped, no name"), "{w}");
        assert!(w.contains("row 4: skipped"), "{w}");
        assert!(w.contains("not a plant category"), "{w}");
        assert!(w.contains("not a color"), "{w}");
        assert!(w.contains("not a plan graphic"), "{w}");
        assert!(w.contains("not a 3D form"), "{w}");
        assert!(w.contains("not a number"), "{w}");
        assert!(w.contains("appears twice"), "{w}");
    }

    #[test]
    fn name_falls_back_to_the_botanical_name_and_short_rows_work() {
        let csv = "botanical name,common name,id\nAcer campestre\n,Maple,M\n";
        let r = import(csv, &ImportOptions::default()).unwrap();
        assert_eq!(
            r.styles.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["Acer campestre", "Maple"]
        );
        assert_eq!(r.styles[1].style.id_code, "M");
    }

    #[test]
    fn export_then_import_gives_the_same_styles() {
        let mut a = PlantStyle {
            botanical_name: "Acer, \"campestre\"".into(),
            common_name: "Field maple".into(),
            id_code: "AC".into(),
            category: PlantCategory::Tree,
            spread: 5000.5,
            height: 9000.25,
            graphic: PlantGraphic::Cloud,
            form: PlantForm::Spreading,
            fill: Some(Color([10, 20, 30, 255])),
            pen: Some(Color([1, 2, 3, 128])),
            price: 55.75,
            size: "14-16".into(),
            container: "C10".into(),
            root: "balled".into(),
            ..PlantStyle::default()
        };
        a.custom.insert("water need".into(), "low".into());
        let b = PlantStyle {
            botanical_name: "Poa".into(),
            category: PlantCategory::Grass,
            ..PlantStyle::default()
        };
        let text = export([("Field maple", &a), ("Meadow grass", &b)]);
        let back = import(&text, &ImportOptions::default()).unwrap();
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        assert_eq!(back.styles.len(), 2);
        assert_eq!(
            (back.styles[0].name.as_str(), &back.styles[0].style),
            ("Field maple", &a)
        );
        assert_eq!(
            (back.styles[1].name.as_str(), &back.styles[1].style),
            ("Meadow grass", &b)
        );
    }

    #[test]
    fn apply_to_changes_only_the_given_columns() {
        let base = PlantStyle {
            botanical_name: "Acer".into(),
            price: 5.0,
            spread: 1000.0,
            ..PlantStyle::default()
        };
        let mut base = base;
        base.custom.insert("a".into(), "keep".into());
        base.custom.insert("b".into(), "old".into());
        let r = import(
            "name,spread (m),b,price\nX,3,new,\n",
            &ImportOptions::default(),
        )
        .unwrap();
        let out = r.styles[0].apply_to(&base);
        assert_eq!(
            (out.botanical_name.as_str(), out.spread, out.price),
            ("Acer", 3000.0, 5.0)
        );
        assert_eq!(
            (out.custom["a"].as_str(), out.custom["b"].as_str()),
            ("keep", "new")
        );
    }

    #[test]
    fn export_of_nothing_is_only_the_header() {
        let text = export(std::iter::empty());
        assert_eq!(parse(&text).unwrap().len(), 1);
        assert!(text.starts_with("name,botanical_name"));
    }

    #[test]
    fn the_sample_catalog_imports_cleanly() {
        let r = import(SAMPLE_CSV, &ImportOptions::default()).unwrap();
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert!(r.styles.len() >= 8);
        for cat in PlantCategory::ALL {
            assert!(r.styles.iter().any(|s| s.style.category == cat), "{cat:?}");
        }
    }
}
