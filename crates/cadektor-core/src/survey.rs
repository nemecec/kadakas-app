//! Survey files: point files (P, N, E, Z, code) and tree surveys, read from delimited text.
//!
//! The readers are pure. They turn text into rows with map coordinates (easting, northing, in
//! the unit of the file) and report the lines they skip. [`place`] turns map coordinates into
//! model positions (mm) and, if the document has no georeference and the survey is far from the
//! origin, makes one, so the geometry stays near (0, 0).

use glam::{DVec2, DVec3};

use crate::georef::Georeference;
use crate::plants::tree::{TreeAction, TreeCondition};

/// The field delimiter of a survey file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Delimiter {
    /// Taken from the first line: comma, semicolon, tab, pipe, or else white space.
    #[default]
    Auto,
    Comma,
    Semicolon,
    Tab,
    Pipe,
    /// One or more spaces or tabs.
    Space,
}

impl Delimiter {
    pub fn from_name(s: &str) -> Option<Delimiter> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Delimiter::Auto,
            "," | "comma" => Delimiter::Comma,
            ";" | "semicolon" => Delimiter::Semicolon,
            "\t" | "\\t" | "tab" => Delimiter::Tab,
            "|" | "pipe" => Delimiter::Pipe,
            " " | "space" | "whitespace" => Delimiter::Space,
            _ => return None,
        })
    }

    fn char(self) -> Option<char> {
        match self {
            Delimiter::Auto => None,
            Delimiter::Comma => Some(','),
            Delimiter::Semicolon => Some(';'),
            Delimiter::Tab => Some('\t'),
            Delimiter::Pipe => Some('|'),
            Delimiter::Space => Some(' '),
        }
    }
}

/// The delimiter that the first lines use most (outside quotes).
pub fn detect_delimiter(text: &str) -> Delimiter {
    let mut counts = [0usize; 4];
    let lines = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .take(5);
    for line in lines {
        let mut quoted = false;
        for c in line.chars() {
            match c {
                '"' => quoted = !quoted,
                ',' if !quoted => counts[0] += 1,
                ';' if !quoted => counts[1] += 1,
                '\t' if !quoted => counts[2] += 1,
                '|' if !quoted => counts[3] += 1,
                _ => {}
            }
        }
    }
    let list = [
        Delimiter::Comma,
        Delimiter::Semicolon,
        Delimiter::Tab,
        Delimiter::Pipe,
    ];
    let best = (0..4)
        .max_by_key(|i| (counts[*i], std::cmp::Reverse(*i)))
        .unwrap();
    if counts[best] > 0 {
        list[best]
    } else {
        Delimiter::Space
    }
}

/// Splits text into rows of fields. Lines that are empty or start with `#` are skipped. Quotes
/// protect a delimiter. With the space delimiter, runs of spaces and tabs count as one.
/// Each row has the 1-based number of its line.
pub fn split_rows(text: &str, delim: Delimiter) -> Vec<(usize, Vec<String>)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let delim = if delim == Delimiter::Auto {
        detect_delimiter(text)
    } else {
        delim
    };
    let d = delim.char().unwrap_or(',');
    let space = delim == Delimiter::Space;
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if quoted {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        quoted = false;
                    }
                } else {
                    field.push(c);
                }
            } else if c == '"' && field.trim().is_empty() {
                field.clear();
                quoted = true;
            } else if space && (c == ' ' || c == '\t') {
                if !field.is_empty() {
                    fields.push(std::mem::take(&mut field));
                }
            } else if !space && c == d {
                fields.push(std::mem::take(&mut field));
            } else {
                field.push(c);
            }
        }
        if !space || !field.is_empty() {
            fields.push(field);
        }
        for f in &mut fields {
            *f = f.trim().to_string();
        }
        rows.push((i + 1, fields));
    }
    rows
}

/// A number with a point or a comma as the decimal sign. Not finite numbers are refused.
pub fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    let v: f64 = t.parse().ok().or_else(|| {
        (t.matches(',').count() == 1 && !t.contains('.'))
            .then(|| t.replace(',', "."))
            .and_then(|r| r.parse().ok())
    })?;
    v.is_finite().then_some(v)
}

// ---------------------------------------------------------------------------
// Point files

/// What a column of a point file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Id,
    Northing,
    Easting,
    Elevation,
    Code,
    Description,
    Skip,
}

impl Column {
    /// A column from a letter or a name: `P`, `N`, `E`, `Z`, `C`, `D`, `-` and the full words.
    pub fn from_name(s: &str) -> Option<Column> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "p" | "id" | "point" | "pt" | "pointid" | "point_id" | "name" | "no" | "number" => {
                Column::Id
            }
            "n" | "northing" | "north" | "y" => Column::Northing,
            "e" | "easting" | "east" | "x" => Column::Easting,
            "z" | "elevation" | "elev" | "h" | "height" | "el" => Column::Elevation,
            "c" | "code" | "feature" | "fc" => Column::Code,
            "d" | "desc" | "description" | "note" | "notes" | "remark" => Column::Description,
            "skip" | "-" | "ignore" | "_" => Column::Skip,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Column::Id => "id",
            Column::Northing => "northing",
            Column::Easting => "easting",
            Column::Elevation => "elevation",
            Column::Code => "code",
            Column::Description => "description",
            Column::Skip => "skip",
        }
    }
}

/// The usual order: point number, northing, easting, elevation, code.
pub const DEFAULT_ORDER: [Column; 5] = [
    Column::Id,
    Column::Northing,
    Column::Easting,
    Column::Elevation,
    Column::Code,
];

/// How to read a point file.
#[derive(Clone, Debug)]
pub struct PointOptions {
    pub delimiter: Delimiter,
    /// The columns in file order. Extra fields at the end of a line are ignored.
    pub columns: Vec<Column>,
    /// Lines to skip at the start. `None`: skip the first line if its coordinates are not numbers.
    pub skip_lines: Option<usize>,
    /// Meters per unit of the northing and easting. Default 1 (meters).
    pub xy_unit_m: f64,
    /// Meters per unit of the elevation. Default 1 (meters).
    pub z_unit_m: f64,
    /// The most points to read.
    pub max_points: usize,
}

impl Default for PointOptions {
    fn default() -> Self {
        PointOptions {
            delimiter: Delimiter::Auto,
            columns: DEFAULT_ORDER.to_vec(),
            skip_lines: None,
            xy_unit_m: 1.0,
            z_unit_m: 1.0,
            max_points: 1_000_000,
        }
    }
}

/// A survey point with map coordinates in meters.
#[derive(Clone, Debug, PartialEq)]
pub struct SurveyPoint {
    pub id: String,
    pub easting: f64,
    pub northing: f64,
    pub z: f64,
    pub code: String,
    pub description: String,
}

/// The result of reading a survey file.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed<T> {
    pub rows: Vec<T>,
    /// Lines that were not used: (line number, reason).
    pub skipped: Vec<(usize, String)>,
}

impl<T> Default for Parsed<T> {
    fn default() -> Self {
        Parsed {
            rows: Vec::new(),
            skipped: Vec::new(),
        }
    }
}

/// Reads a point file. A line needs a northing, an easting and an elevation as numbers.
pub fn parse_points(text: &str, opts: &PointOptions) -> Result<Parsed<SurveyPoint>, String> {
    let col = |c: Column| opts.columns.iter().position(|x| *x == c);
    let (Some(ni), Some(ei)) = (col(Column::Northing), col(Column::Easting)) else {
        return Err("the column order needs a northing (N) and an easting (E)".into());
    };
    let zi = col(Column::Elevation);
    for u in [opts.xy_unit_m, opts.z_unit_m] {
        if !(u.is_finite() && u > 0.0) {
            return Err("units must be positive numbers".into());
        }
    }
    let mut rows = split_rows(text, opts.delimiter).into_iter();
    let mut out = Parsed::default();
    let mut first = true;
    let mut to_skip = opts.skip_lines;
    for (line, f) in &mut rows {
        if let Some(n) = to_skip.as_mut().filter(|n| **n > 0) {
            *n -= 1;
            continue;
        }
        let get = |i: usize| f.get(i).map(String::as_str).unwrap_or("");
        let (n, e) = (parse_number(get(ni)), parse_number(get(ei)));
        let z = match zi {
            Some(i) => parse_number(get(i)),
            None => Some(0.0),
        };
        if first {
            first = false;
            if opts.skip_lines.is_none() && (n.is_none() || e.is_none()) {
                continue; // a header
            }
        }
        let (Some(n), Some(e), Some(z)) = (n, e, z) else {
            out.skipped.push((
                line,
                "northing, easting or elevation is not a number".into(),
            ));
            continue;
        };
        if out.rows.len() >= opts.max_points {
            out.skipped.push((line, "too many points".into()));
            continue;
        }
        let text_of = |c: Column| col(c).map(|i| get(i).to_string()).unwrap_or_default();
        out.rows.push(SurveyPoint {
            id: text_of(Column::Id),
            easting: e * opts.xy_unit_m,
            northing: n * opts.xy_unit_m,
            z: z * opts.z_unit_m,
            code: text_of(Column::Code),
            description: text_of(Column::Description),
        });
    }
    Ok(out)
}

/// Model positions (mm) for map positions (meters), and the georeference to set, if a new one
/// is needed.
///
/// A document with a georeference keeps it. Without one, a survey whose center lies more than
/// 1 km from the origin gets a new georeference at the center (whole meters, `crs` as given),
/// when `shift` is true. A survey near the origin is placed as it is.
pub fn place(
    map: &[DVec2],
    georef: Option<&Georeference>,
    crs: &str,
    shift: bool,
) -> (Vec<DVec2>, Option<Georeference>) {
    if let Some(g) = georef {
        return (map.iter().map(|p| g.to_model(*p)).collect(), None);
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for p in map {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if map.is_empty() || !shift || ((lo + hi) / 2.0).length() <= 1000.0 {
        return (map.iter().map(|p| *p * 1000.0).collect(), None);
    }
    let origin = ((lo + hi) / 2.0).round();
    let g = Georeference {
        crs: crs.to_string(),
        origin,
        rotation: 0.0,
        user_origin: DVec2::ZERO,
    };
    (
        map.iter().map(|p| (*p - origin) * 1000.0).collect(),
        Some(g),
    )
}

/// The positions of survey points in model space, with the elevation in mm.
pub fn place_points(
    points: &[SurveyPoint],
    georef: Option<&Georeference>,
    crs: &str,
    shift: bool,
) -> (Vec<DVec3>, Option<Georeference>) {
    let map: Vec<DVec2> = points
        .iter()
        .map(|p| DVec2::new(p.easting, p.northing))
        .collect();
    let (xy, g) = place(&map, georef, crs, shift);
    (
        xy.iter()
            .zip(points)
            .map(|(p, s)| p.extend(s.z * 1000.0))
            .collect(),
        g,
    )
}

// ---------------------------------------------------------------------------
// Tree surveys

/// What a column of a tree survey holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TreeCol {
    Id,
    Easting,
    Northing,
    Species,
    Dbh,
    Height,
    Spread,
    Condition,
    Action,
    Note,
}

/// Meters per unit named in a header such as `DBH (cm)`, `height_m` or `spread [mm]`.
fn header_unit(h: &str) -> Option<f64> {
    let l = h.to_ascii_lowercase();
    let word: String = l
        .rsplit(|c: char| !c.is_ascii_alphabetic())
        .find(|w| !w.is_empty())
        .unwrap_or("")
        .to_string();
    match word.as_str() {
        "mm" => Some(0.001),
        "cm" => Some(0.01),
        "m" => Some(1.0),
        _ if l.contains("(in)") || l.ends_with("_in") => Some(0.0254),
        _ => None,
    }
}

fn tree_col(h: &str) -> Option<TreeCol> {
    let l = h.trim().to_ascii_lowercase();
    // Cut off the unit: "dbh (cm)", "height_m".
    let base: String = l
        .split(['(', '[', '_'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let b = base.as_str();
    Some(match b {
        "id" | "tag" | "no" | "number" | "tree" | "tree id" | "tree no" | "p" | "point"
        | "name" => TreeCol::Id,
        "e" | "easting" | "east" | "x" => TreeCol::Easting,
        "n" | "northing" | "north" | "y" => TreeCol::Northing,
        "species" | "latin" | "botanical" | "latin name" | "taxon" | "common name" => {
            TreeCol::Species
        }
        "dbh" | "trunk" | "trunk diameter" | "diameter" | "stem" | "stem diameter" | "girth" => {
            TreeCol::Dbh
        }
        "height" | "h" | "tree height" => TreeCol::Height,
        "spread" | "crown" | "canopy" | "crown spread" | "canopy spread" | "crown diameter" => {
            TreeCol::Spread
        }
        "condition" | "health" | "vitality" => TreeCol::Condition,
        "action" | "status" | "retention" | "decision" => TreeCol::Action,
        "note" | "notes" | "remarks" | "remark" | "comment" | "comments" | "description" => {
            TreeCol::Note
        }
        _ => return None,
    })
}

/// One tree of a survey, with map coordinates in meters and sizes in mm.
#[derive(Clone, Debug, PartialEq)]
pub struct SurveyTree {
    pub id: String,
    pub easting: f64,
    pub northing: f64,
    pub species: String,
    pub dbh: f64,
    pub height: f64,
    pub spread: f64,
    pub condition: TreeCondition,
    pub action: TreeAction,
    pub note: String,
}

/// How to read a tree survey.
#[derive(Clone, Debug)]
pub struct TreeOptions {
    pub delimiter: Delimiter,
    /// Meters per unit of the coordinates. Default 1.
    pub xy_unit_m: f64,
    /// Meters per unit of the trunk diameter when the header names no unit. Default 0.01 (cm).
    pub dbh_unit_m: f64,
    /// Meters per unit of the height and spread when the header names no unit. Default 1 (m).
    pub size_unit_m: f64,
    pub max_trees: usize,
}

impl Default for TreeOptions {
    fn default() -> Self {
        TreeOptions {
            delimiter: Delimiter::Auto,
            xy_unit_m: 1.0,
            dbh_unit_m: 0.01,
            size_unit_m: 1.0,
            max_trees: 1_000_000,
        }
    }
}

/// A condition from a word. Unknown words mean `None`.
pub fn condition_of(s: &str) -> Option<TreeCondition> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "good" | "excellent" | "healthy" | "a" | "1" => TreeCondition::Good,
        "fair" | "average" | "moderate" | "b" | "2" => TreeCondition::Fair,
        "poor" | "bad" | "declining" | "c" | "3" => TreeCondition::Poor,
        "dead" | "dying" | "d" | "u" | "4" => TreeCondition::Dead,
        _ => return None,
    })
}

/// An action from a word. Unknown words mean `None`.
pub fn action_of(s: &str) -> Option<TreeAction> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "retain" | "keep" | "protect" | "save" | "existing" => TreeAction::Retain,
        "remove" | "fell" | "cut" | "demolish" | "removal" => TreeAction::Remove,
        "relocate" | "move" | "transplant" | "moved" => TreeAction::Relocate,
        _ => return None,
    })
}

/// Reads a tree survey. The first line is a header that names the columns (for example
/// `ID,E,N,Species,DBH (cm),Height (m),Spread (m),Condition,Action`). Easting and northing,
/// or `x` and `y`, are needed. Other columns are optional. The header may name the unit of a size.
pub fn parse_trees(text: &str, opts: &TreeOptions) -> Result<Parsed<SurveyTree>, String> {
    for u in [opts.xy_unit_m, opts.dbh_unit_m, opts.size_unit_m] {
        if !(u.is_finite() && u > 0.0) {
            return Err("units must be positive numbers".into());
        }
    }
    let mut rows = split_rows(text, opts.delimiter).into_iter();
    let Some((_, header)) = rows.next() else {
        return Err("the file is empty".into());
    };
    let cols: Vec<Option<(TreeCol, Option<f64>)>> = header
        .iter()
        .map(|h| tree_col(h).map(|c| (c, header_unit(h))))
        .collect();
    let find = |c: TreeCol| cols.iter().position(|x| x.is_some_and(|(k, _)| k == c));
    let (Some(ei), Some(ni)) = (find(TreeCol::Easting), find(TreeCol::Northing)) else {
        return Err("the header needs an easting (E or X) and a northing (N or Y) column".into());
    };
    let unit_of = |c: TreeCol, default: f64| {
        cols.iter()
            .flatten()
            .find(|(k, _)| *k == c)
            .and_then(|(_, u)| *u)
            .unwrap_or(default)
    };
    let (dbh_u, h_u, s_u) = (
        unit_of(TreeCol::Dbh, opts.dbh_unit_m),
        unit_of(TreeCol::Height, opts.size_unit_m),
        unit_of(TreeCol::Spread, opts.size_unit_m),
    );
    let mut out = Parsed::default();
    for (line, f) in rows {
        let get = |c: TreeCol| find(c).and_then(|i| f.get(i)).map(String::as_str);
        let (Some(e), Some(n)) = (
            f.get(ei).and_then(|s| parse_number(s)),
            f.get(ni).and_then(|s| parse_number(s)),
        ) else {
            out.skipped
                .push((line, "easting or northing is not a number".into()));
            continue;
        };
        if out.rows.len() >= opts.max_trees {
            out.skipped.push((line, "too many trees".into()));
            continue;
        }
        let size = |c: TreeCol, unit: f64| -> Result<f64, ()> {
            match get(c).map(str::trim).filter(|s| !s.is_empty()) {
                None => Ok(0.0),
                Some(s) => parse_number(s)
                    .filter(|v| *v >= 0.0)
                    .map(|v| v * unit * 1000.0)
                    .ok_or(()),
            }
        };
        let (Ok(dbh), Ok(height), Ok(spread)) = (
            size(TreeCol::Dbh, dbh_u),
            size(TreeCol::Height, h_u),
            size(TreeCol::Spread, s_u),
        ) else {
            out.skipped
                .push((line, "a size is not a positive number".into()));
            continue;
        };
        let condition = match get(TreeCol::Condition)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => TreeCondition::default(),
            Some(s) => match condition_of(s) {
                Some(c) => c,
                None => {
                    out.skipped.push((line, format!("unknown condition '{s}'")));
                    continue;
                }
            },
        };
        let action = match get(TreeCol::Action)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            None => TreeAction::default(),
            Some(s) => match action_of(s) {
                Some(a) => a,
                None => {
                    out.skipped.push((line, format!("unknown action '{s}'")));
                    continue;
                }
            },
        };
        out.rows.push(SurveyTree {
            id: get(TreeCol::Id).unwrap_or("").to_string(),
            easting: e * opts.xy_unit_m,
            northing: n * opts.xy_unit_m,
            species: get(TreeCol::Species).unwrap_or("").to_string(),
            dbh,
            height,
            spread,
            condition,
            action,
            note: get(TreeCol::Note).unwrap_or("").to_string(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn delimiters_are_detected() {
        assert_eq!(detect_delimiter("1,2,3"), Delimiter::Comma);
        assert_eq!(detect_delimiter("1;2;3"), Delimiter::Semicolon);
        assert_eq!(detect_delimiter("1\t2\t3"), Delimiter::Tab);
        assert_eq!(detect_delimiter("1|2"), Delimiter::Pipe);
        assert_eq!(detect_delimiter("1 2 3"), Delimiter::Space);
        assert_eq!(detect_delimiter(""), Delimiter::Space);
        assert_eq!(Delimiter::from_name("tab"), Some(Delimiter::Tab));
        assert_eq!(Delimiter::from_name(";"), Some(Delimiter::Semicolon));
        assert_eq!(Delimiter::from_name("x"), None);
        assert_eq!(Delimiter::from_name("auto"), Some(Delimiter::Auto));
    }

    #[test]
    fn rows_handle_quotes_blank_lines_comments_and_space_runs() {
        let r = split_rows("# c\n\n\"a,b\",c\n1,\"x\"\"y\"\n", Delimiter::Comma);
        assert_eq!(r[0], (3, vec!["a,b".to_string(), "c".to_string()]));
        assert_eq!(r[1].1[1], "x\"y");
        let r = split_rows("1   2\t3  \n", Delimiter::Space);
        assert_eq!(r[0].1, vec!["1", "2", "3"]);
        let r = split_rows("\u{feff}a;b", Delimiter::Auto);
        assert_eq!(r[0].1, vec!["a", "b"]);
    }

    #[test]
    fn numbers_accept_a_decimal_comma_and_refuse_junk() {
        assert_eq!(parse_number("1.5"), Some(1.5));
        assert_eq!(parse_number(" 2,5 "), Some(2.5));
        assert_eq!(parse_number("1,000.5"), None);
        assert_eq!(parse_number("NaN"), None);
        assert_eq!(parse_number("inf"), None);
        assert_eq!(parse_number("x"), None);
    }

    #[test]
    fn points_read_in_the_default_order_with_a_header() {
        let t = "P,N,E,Z,Code\n1,6672000.5,385000.25,12.5,TREE\n2,6672001,385001,12.75,KERB\n";
        let p = parse_points(t, &PointOptions::default()).unwrap();
        assert_eq!(p.rows.len(), 2);
        assert_eq!(p.rows[0].id, "1");
        assert_eq!(p.rows[0].northing, 6672000.5);
        assert_eq!(p.rows[0].easting, 385000.25);
        assert_eq!(p.rows[0].z, 12.5);
        assert_eq!(p.rows[1].code, "KERB");
        assert!(p.skipped.is_empty());
    }

    #[test]
    fn points_read_in_another_order_with_spaces_and_units() {
        let opts = PointOptions {
            columns: vec![
                Column::Easting,
                Column::Northing,
                Column::Elevation,
                Column::Skip,
                Column::Description,
            ],
            xy_unit_m: 0.3048,
            z_unit_m: 0.3048,
            ..PointOptions::default()
        };
        let p = parse_points("10 20 30 x spot level\n", &opts).unwrap();
        assert_eq!(p.rows[0].easting, 3.048);
        assert!((p.rows[0].z - 9.144).abs() < 1e-9);
        assert_eq!(p.rows[0].description, "spot");
        assert_eq!(p.rows[0].code, "");
    }

    #[test]
    fn bad_lines_are_reported_and_a_number_header_is_not_skipped() {
        let t = "1,5,6,7,A\n2,x,6,7,B\n3,5,6\n4,5,6,NaN\n";
        let p = parse_points(t, &PointOptions::default()).unwrap();
        assert_eq!(p.rows.len(), 1);
        assert_eq!(p.skipped.iter().map(|s| s.0).collect::<Vec<_>>(), [2, 3, 4]);
        let p = parse_points(
            "h\n1,5,6,7\n",
            &PointOptions {
                skip_lines: Some(1),
                ..PointOptions::default()
            },
        )
        .unwrap();
        assert_eq!(p.rows.len(), 1);
    }

    #[test]
    fn point_options_are_checked() {
        let bad = PointOptions {
            columns: vec![Column::Id, Column::Elevation],
            ..PointOptions::default()
        };
        assert!(parse_points("1,2", &bad).is_err());
        let bad = PointOptions {
            xy_unit_m: 0.0,
            ..PointOptions::default()
        };
        assert!(parse_points("1,2,3,4", &bad).is_err());
        let few = PointOptions {
            max_points: 1,
            ..PointOptions::default()
        };
        let p = parse_points("1,2,3,4\n2,2,3,4\n", &few).unwrap();
        assert_eq!((p.rows.len(), p.skipped.len()), (1, 1));
        let no_z = PointOptions {
            columns: vec![Column::Northing, Column::Easting],
            ..PointOptions::default()
        };
        assert_eq!(parse_points("1,2", &no_z).unwrap().rows[0].z, 0.0);
    }

    #[test]
    fn column_names() {
        assert_eq!(Column::from_name("P"), Some(Column::Id));
        assert_eq!(Column::from_name("Northing"), Some(Column::Northing));
        assert_eq!(Column::from_name("-"), Some(Column::Skip));
        assert_eq!(Column::from_name("q"), None);
        for c in DEFAULT_ORDER {
            assert_eq!(Column::from_name(c.id()), Some(c));
        }
        assert_eq!(Column::Description.id(), "description");
        assert_eq!(Column::Skip.id(), "skip");
    }

    #[test]
    fn a_survey_far_from_the_origin_makes_a_georeference() {
        let map = [
            DVec2::new(385_000.0, 6_672_000.0),
            DVec2::new(385_010.0, 6_672_020.0),
        ];
        let (xy, g) = place(&map, None, "EPSG:3067", true);
        let g = g.expect("georeference");
        assert_eq!(g.origin, DVec2::new(385_005.0, 6_672_010.0));
        assert_eq!(g.crs, "EPSG:3067");
        assert_eq!(xy[0], DVec2::new(-5000.0, -10_000.0));
        assert!(g.to_model(map[1]).distance(xy[1]) < 1e-6);
        // No shift when asked not to, or when the survey is near the origin.
        assert_eq!(place(&map, None, "", false).0[0], map[0] * 1000.0);
        assert!(place(&[DVec2::new(5.0, 5.0)], None, "", true).1.is_none());
        assert!(place(&[], None, "", true).0.is_empty());
        // A georeference of the document wins.
        let (xy2, none) = place(&map, Some(&g), "x", true);
        assert!(none.is_none());
        assert_eq!(xy2, xy);
    }

    #[test]
    fn place_points_gives_millimeters() {
        let pts = [SurveyPoint {
            id: "1".into(),
            easting: 2.0,
            northing: 3.0,
            z: 1.5,
            code: String::new(),
            description: String::new(),
        }];
        let (p, g) = place_points(&pts, None, "", true);
        assert!(g.is_none());
        assert_eq!(p[0], DVec3::new(2000.0, 3000.0, 1500.0));
    }

    const TREES: &str = "ID;E;N;Species;DBH (cm);Height (m);Spread (m);Condition;Action;Notes\n\
        T1;100;200;Quercus robur;50;15;9;good;retain;big\n\
        T2;110;210;Betula;30,5;8;4;poor;fell;\n\
        T3;x;1;;;;;;;\n\
        T4;1;2;Alnus;20;6;3;sick;;\n";

    #[test]
    fn trees_read_by_header_name_with_units() {
        let t = parse_trees(TREES, &TreeOptions::default()).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].species, "Quercus robur");
        assert_eq!(t.rows[0].dbh, 500.0);
        assert_eq!(t.rows[0].height, 15_000.0);
        assert_eq!(t.rows[0].spread, 9000.0);
        assert_eq!(t.rows[0].condition, TreeCondition::Good);
        assert_eq!(t.rows[0].note, "big");
        assert_eq!(t.rows[1].dbh, 305.0);
        assert_eq!(t.rows[1].action, TreeAction::Remove);
        assert_eq!(t.skipped.len(), 2);
        assert!(t.skipped[1].1.contains("sick"));
    }

    #[test]
    fn tree_defaults_and_errors() {
        let t = parse_trees("x,y,dbh\n1,2,40\n", &TreeOptions::default()).unwrap();
        assert_eq!(t.rows[0].dbh, 400.0);
        assert_eq!(t.rows[0].condition, TreeCondition::Fair);
        assert_eq!(t.rows[0].action, TreeAction::Retain);
        let t = parse_trees(
            "x,y,dbh [mm],height_m\n1,2,400,7\n",
            &TreeOptions::default(),
        )
        .unwrap();
        assert_eq!((t.rows[0].dbh, t.rows[0].height), (400.0, 7000.0));
        assert!(parse_trees("", &TreeOptions::default()).is_err());
        assert!(parse_trees("a,b\n1,2", &TreeOptions::default()).is_err());
        let bad = TreeOptions {
            xy_unit_m: -1.0,
            ..TreeOptions::default()
        };
        assert!(parse_trees("x,y\n1,2", &bad).is_err());
        let t = parse_trees("x,y,dbh\n1,2,-4\n", &TreeOptions::default()).unwrap();
        assert_eq!(t.skipped.len(), 1);
        let few = TreeOptions {
            max_trees: 1,
            ..TreeOptions::default()
        };
        let t = parse_trees("x,y\n1,2\n3,4\n", &few).unwrap();
        assert_eq!((t.rows.len(), t.skipped.len()), (1, 1));
    }

    #[test]
    fn words_for_conditions_and_actions() {
        assert_eq!(condition_of("Dead"), Some(TreeCondition::Dead));
        assert_eq!(condition_of("?"), None);
        assert_eq!(action_of("Keep"), Some(TreeAction::Retain));
        assert_eq!(action_of("transplant"), Some(TreeAction::Relocate));
        assert_eq!(action_of("?"), None);
    }

    proptest! {
        #[test]
        fn readers_never_panic(s in "\\PC{0,200}") {
            let _ = parse_points(&s, &PointOptions::default());
            let _ = parse_trees(&s, &TreeOptions::default());
        }

        #[test]
        fn points_survive_a_write_and_read(
            pts in prop::collection::vec((-1.0e6f64..1.0e6, -1.0e6f64..1.0e6, -1000.0f64..1000.0), 1..30)
        ) {
            let mut text = String::new();
            for (i, (n, e, z)) in pts.iter().enumerate() {
                text += &format!("{i},{n},{e},{z},C{i}\n");
            }
            let p = parse_points(&text, &PointOptions::default()).unwrap();
            prop_assert_eq!(p.rows.len(), pts.len());
            for (r, (n, e, z)) in p.rows.iter().zip(&pts) {
                prop_assert_eq!(r.northing, *n);
                prop_assert_eq!(r.easting, *e);
                prop_assert_eq!(r.z, *z);
            }
        }
    }
}
