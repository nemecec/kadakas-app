//! Display rules of the document: the northing/easting grid, the display of project phases,
//! and data visualizations (color by criteria).
//!
//! These rules change how objects look. They never change the objects or their data. A
//! visualization is an override layer: it takes a property of each object (see `props`),
//! finds the first rule that fits, and gives the object that rule's color on screen, in the SVG,
//! and in print. Exports to other formats ignore it.

use glam::DVec2;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::georef::Georeference;
use crate::model::{Attributes, Color, Document, Object};
use crate::props::{self, Prop};

/// The data key that holds the phase of an object.
pub const PHASE_KEY: &str = "phase";

/// The phase of an object in a renovation project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Built before this project, and it stays.
    Existing,
    /// Built by this project. This is the default.
    #[default]
    New,
    /// Built before this project, and this project removes it.
    Demolished,
}

impl Phase {
    pub const ALL: [Phase; 3] = [Phase::Existing, Phase::New, Phase::Demolished];

    pub fn id(self) -> &'static str {
        match self {
            Phase::Existing => "existing",
            Phase::New => "new",
            Phase::Demolished => "demolished",
        }
    }

    pub fn from_name(s: &str) -> Option<Phase> {
        match s.trim().to_ascii_lowercase().as_str() {
            "existing" | "exist" | "retained" => Some(Phase::Existing),
            "new" | "proposed" => Some(Phase::New),
            "demolished" | "demolish" | "demo" | "removed" => Some(Phase::Demolished),
            _ => None,
        }
    }
}

/// The phase of an object: its `phase` data field, or `new` if it has none (or an unknown one).
pub fn phase_of(obj: &Object) -> Phase {
    obj.data
        .get(PHASE_KEY)
        .and_then(Value::as_str)
        .and_then(Phase::from_name)
        .unwrap_or_default()
}

/// How objects of one phase are shown.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseStyle {
    /// False hides the objects of the phase.
    pub visible: bool,
    /// Replaces the pen color, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<Color>,
    /// Replaces the fill color, if set. `[0,0,0,0]` means no fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    /// Multiplies the line weight, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight_factor: Option<f64>,
}

impl PhaseStyle {
    pub const SHOWN: PhaseStyle = PhaseStyle {
        visible: true,
        pen: None,
        fill: None,
        weight_factor: None,
    };

    fn is_valid(&self) -> bool {
        self.weight_factor
            .is_none_or(|w| w.is_finite() && (0.0..=100.0).contains(&w))
    }
}

/// The display rules of the three phases. When `enabled` is false, phases do nothing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseDisplay {
    pub enabled: bool,
    pub existing: PhaseStyle,
    pub new: PhaseStyle,
    pub demolished: PhaseStyle,
}

impl Default for PhaseDisplay {
    /// Existing objects in grey, new objects as drawn, demolished objects in red without fill.
    fn default() -> Self {
        PhaseDisplay {
            enabled: true,
            existing: PhaseStyle {
                visible: true,
                pen: Some(Color([110, 110, 110, 255])),
                fill: None,
                weight_factor: None,
            },
            new: PhaseStyle::SHOWN,
            demolished: PhaseStyle {
                visible: true,
                pen: Some(Color([200, 40, 40, 255])),
                fill: Some(Color::NONE),
                weight_factor: None,
            },
        }
    }
}

impl PhaseDisplay {
    pub fn style(&self, p: Phase) -> &PhaseStyle {
        match p {
            Phase::Existing => &self.existing,
            Phase::New => &self.new,
            Phase::Demolished => &self.demolished,
        }
    }
}

/// The grid of northings and eastings, drawn from the georeference.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NeGrid {
    /// Distance between grid lines in meters (map units).
    pub spacing: f64,
    #[serde(default = "yes")]
    pub labels: bool,
    /// Text height of the labels, in model mm. 0 means automatic (a fraction of the spacing).
    #[serde(default)]
    pub label_height: f64,
    #[serde(default = "grid_color")]
    pub color: Color,
}

fn yes() -> bool {
    true
}

fn grid_color() -> Color {
    Color([120, 140, 170, 255])
}

/// The most grid lines in one direction. A grid that needs more is too dense to read.
pub const MAX_GRID_LINES: usize = 200;

impl NeGrid {
    pub fn new(spacing: f64) -> NeGrid {
        NeGrid {
            spacing,
            labels: true,
            label_height: 0.0,
            color: grid_color(),
        }
    }

    fn is_valid(&self) -> bool {
        self.spacing.is_finite()
            && self.spacing > 0.0
            && self.label_height.is_finite()
            && self.label_height >= 0.0
    }
}

/// A line of the N/E grid in model space.
#[derive(Clone, Debug, PartialEq)]
pub struct GridLine {
    pub a: DVec2,
    pub b: DVec2,
    /// `E 385000` or `N 6672000`, in map units.
    pub label: String,
    /// True for an easting line (it runs north), false for a northing line (it runs east).
    pub easting: bool,
}

/// The grid lines that cover the box `lo..hi` (model mm). Empty if the spacing is too small
/// for the box (more than [`MAX_GRID_LINES`] lines in a direction).
pub fn grid_lines(g: &Georeference, grid: &NeGrid, lo: DVec2, hi: DVec2) -> Vec<GridLine> {
    if !(grid.is_valid() && lo.is_finite() && hi.is_finite()) {
        return Vec::new();
    }
    let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)].map(|c| g.to_map(c));
    let (mut mlo, mut mhi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for c in corners {
        mlo = mlo.min(c);
        mhi = mhi.max(c);
    }
    let s = grid.spacing;
    let (e0, e1) = ((mlo.x / s).ceil(), (mhi.x / s).floor());
    let (n0, n1) = ((mlo.y / s).ceil(), (mhi.y / s).floor());
    if (e1 - e0 + 1.0) > MAX_GRID_LINES as f64 || (n1 - n0 + 1.0) > MAX_GRID_LINES as f64 {
        return Vec::new();
    }
    let text = |v: f64| {
        if (v - v.round()).abs() < 1e-9 {
            format!("{}", v.round() as i64)
        } else {
            format!("{v}")
        }
    };
    let mut out = Vec::new();
    let mut e = e0;
    while e <= e1 {
        let x = e * s;
        out.push(GridLine {
            a: g.to_model(DVec2::new(x, mlo.y)),
            b: g.to_model(DVec2::new(x, mhi.y)),
            label: format!("E {}", text(x)),
            easting: true,
        });
        e += 1.0;
    }
    let mut n = n0;
    while n <= n1 {
        let y = n * s;
        out.push(GridLine {
            a: g.to_model(DVec2::new(mlo.x, y)),
            b: g.to_model(DVec2::new(mhi.x, y)),
            label: format!("N {}", text(y)),
            easting: false,
        });
        n += 1.0;
    }
    out
}

/// What a visualization rule tests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Criterion {
    /// The value equals this text (any case) or number.
    Equals { value: Value },
    /// A number in `min <= v < max`. A missing bound is open.
    Range {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
    },
    /// Anything that no rule before it fits (also objects without the property, if
    /// `include_missing` of the visualization is set).
    Otherwise,
}

/// One rule: if the criterion fits, the object gets this color.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VizRule {
    pub criterion: Criterion,
    pub color: Color,
    /// The name in the legend. Empty: made from the criterion.
    #[serde(default)]
    pub label: String,
}

/// What the color changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VizTarget {
    #[default]
    Fill,
    Pen,
    Both,
}

/// A data visualization: color by a property.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Visualization {
    pub name: String,
    /// The property to test, as `props::property` reads it: `Condition`, `Plant.category`,
    /// `Phase`, `Area`, a record field such as `Soil.Depth`.
    pub field: String,
    pub rules: Vec<VizRule>,
    #[serde(default)]
    pub target: VizTarget,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Only objects of these kinds (`plant`, `hardscape`, ...) are colored. Empty: all.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// Objects without the property go to the `Otherwise` rule. Default false: they stay as they are.
    #[serde(default)]
    pub include_missing: bool,
}

/// Most rules in a visualization.
pub const MAX_RULES: usize = 64;

impl Visualization {
    fn is_valid(&self) -> Result<(), String> {
        if self.field.trim().is_empty() {
            return Err("a visualization needs a field".into());
        }
        if self.rules.is_empty() || self.rules.len() > MAX_RULES {
            return Err(format!("give 1 to {MAX_RULES} rules"));
        }
        for r in &self.rules {
            match &r.criterion {
                Criterion::Range { min, max } => {
                    if min.is_some_and(|v| !v.is_finite()) || max.is_some_and(|v| !v.is_finite()) {
                        return Err("range limits must be finite numbers".into());
                    }
                    if let (Some(a), Some(b)) = (min, max)
                        && a >= b
                    {
                        return Err("a range needs min below max".into());
                    }
                }
                Criterion::Equals { value } => {
                    if !(value.is_string() || value.is_number() || value.is_boolean()) {
                        return Err("equals needs a text, number or boolean".into());
                    }
                }
                Criterion::Otherwise => {}
            }
        }
        Ok(())
    }

    /// The label of rule `i` for a legend.
    pub fn rule_label(&self, i: usize) -> String {
        let r = &self.rules[i];
        if !r.label.is_empty() {
            return r.label.clone();
        }
        match &r.criterion {
            Criterion::Equals { value } => match value {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            },
            Criterion::Range { min, max } => match (min, max) {
                (Some(a), Some(b)) => format!("{a} to {b}"),
                (Some(a), None) => format!("{a} and above"),
                (None, Some(b)) => format!("below {b}"),
                (None, None) => "any number".into(),
            },
            Criterion::Otherwise => "other".into(),
        }
    }
}

/// The rules of the document. All of them are off by default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewRules {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ne_grid: Option<NeGrid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phases: Option<PhaseDisplay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visualization: Option<Visualization>,
}

impl ViewRules {
    pub fn is_empty(&self) -> bool {
        *self == ViewRules::default()
    }

    /// Checks the numbers and the rules.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(g) = &self.ne_grid
            && !g.is_valid()
        {
            return Err("the grid spacing must be a positive number (m)".into());
        }
        if let Some(p) = &self.phases
            && !(p.existing.is_valid() && p.new.is_valid() && p.demolished.is_valid())
        {
            return Err("a line weight factor must be between 0 and 100".into());
        }
        if let Some(v) = &self.visualization {
            v.is_valid()?;
        }
        Ok(())
    }
}

fn rules_of(doc: &Document) -> &ViewRules {
    &doc.drawing.view_rules
}

/// True if the phase rules hide the object.
pub fn hidden_by_phase(doc: &Document, obj: &Object) -> bool {
    match &rules_of(doc).phases {
        Some(p) if p.enabled => !p.style(phase_of(obj)).visible,
        _ => false,
    }
}

fn path_of(field: &str) -> Vec<&str> {
    field
        .split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// The index of the rule that fits the object, or `None` if the object stays as it is.
pub fn classify(doc: &Document, obj: &Object, viz: &Visualization) -> Option<usize> {
    if !viz.kinds.is_empty()
        && !viz
            .kinds
            .iter()
            .any(|k| k.eq_ignore_ascii_case(obj.shape.kind()))
    {
        return None;
    }
    let path = path_of(&viz.field);
    let value = match props::property(doc, obj, &path) {
        Prop::Value(v) => Some(v),
        // The phase has a default, so every object has it.
        _ if viz.field.eq_ignore_ascii_case("phase") => {
            Some(Value::String(phase_of(obj).id().into()))
        }
        _ => None,
    };
    for (i, r) in viz.rules.iter().enumerate() {
        let fits = match (&r.criterion, &value) {
            (Criterion::Otherwise, Some(_)) => true,
            (Criterion::Otherwise, None) => viz.include_missing,
            (_, None) => false,
            (Criterion::Equals { value: want }, Some(have)) => same(want, have),
            (Criterion::Range { min, max }, Some(have)) => have
                .as_f64()
                .or_else(|| have.as_str().and_then(|s| s.trim().parse().ok()))
                .is_some_and(|v| min.is_none_or(|a| v >= a) && max.is_none_or(|b| v < b)),
        };
        if fits {
            return Some(i);
        }
    }
    None
}

fn same(want: &Value, have: &Value) -> bool {
    match (want, have) {
        (Value::String(a), Value::String(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Number(a), Value::String(b)) | (Value::String(b), Value::Number(a)) => {
            b.trim().parse::<f64>().ok() == a.as_f64()
        }
        (a, b) => a == b,
    }
}

/// The colors and line weight that the rules give an object.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Overrides {
    fill: Option<Color>,
    pen: Option<Color>,
    weight_factor: Option<f64>,
}

fn overrides(doc: &Document, obj: &Object) -> Option<Overrides> {
    let rules = rules_of(doc);
    if rules.phases.is_none() && rules.visualization.is_none() {
        return None;
    }
    let mut o = Overrides::default();
    if let Some(p) = &rules.phases
        && p.enabled
    {
        let s = p.style(phase_of(obj));
        (o.fill, o.pen, o.weight_factor) = (s.fill, s.pen, s.weight_factor);
    }
    if let Some(v) = rules.visualization.as_ref().filter(|v| v.enabled)
        && let Some(i) = classify(doc, obj, v)
    {
        let c = v.rules[i].color;
        if matches!(v.target, VizTarget::Fill | VizTarget::Both) {
            o.fill = Some(c);
        }
        if matches!(v.target, VizTarget::Pen | VizTarget::Both) {
            o.pen = Some(c);
        }
    }
    Some(o)
}

/// The attributes to draw an object with: its own, with the phase rules and the active
/// visualization on top. Use this in drawing code (screen, SVG, print). Do not use it in
/// exports: the rules are a view, not data.
pub fn display_attrs(doc: &Document, obj: &Object, mut a: Attributes) -> Attributes {
    let Some(o) = overrides(doc, obj) else {
        return a;
    };
    if let Some(c) = o.pen {
        a.pen = Some(c);
    }
    if let Some(c) = o.fill {
        a.fill = Some(c);
    }
    if let Some(f) = o.weight_factor {
        a.line_weight = Some(a.line_weight.unwrap_or(0.25) * f);
    }
    a
}

/// The look of a plant with the rules on top, for drawing. The phase and the visualization
/// change its colors.
pub fn plant_look(doc: &Document, obj: &Object) -> crate::plants::link::Look {
    let mut look = crate::plants::link::look(doc, obj);
    if let Some(o) = overrides(doc, obj) {
        if o.fill.is_some() {
            look.fill = o.fill;
        }
        if o.pen.is_some() {
            look.pen = o.pen;
        }
    }
    look
}

/// True if the rules give the object a color of its own, so that a drawing must not use
/// the colors of its style instead.
pub fn recolors(doc: &Document, obj: &Object) -> bool {
    overrides(doc, obj).is_some_and(|o| o.fill.is_some() || o.pen.is_some())
}

/// Colors for categories: a palette that stays readable on white and tells its colors apart.
pub const PALETTE: [[u8; 3]; 10] = [
    [31, 119, 180],
    [255, 127, 14],
    [44, 160, 44],
    [214, 39, 40],
    [148, 103, 189],
    [140, 86, 75],
    [227, 119, 194],
    [127, 127, 127],
    [188, 189, 34],
    [23, 190, 207],
];

/// The most categories that [`auto_rules`] makes.
pub const MAX_AUTO_VALUES: usize = 40;

/// Rules from the values in the document: one `Equals` rule per distinct value of the property
/// (text compared without case), or `bins` equal ranges when `bins` is set and the values are
/// numbers. Objects without the property are left out. `kinds` limits the objects.
pub fn auto_rules(
    doc: &Document,
    field: &str,
    kinds: &[String],
    bins: Option<usize>,
) -> Result<Vec<VizRule>, String> {
    let path = path_of(field);
    if path.is_empty() {
        return Err("give a field".into());
    }
    let mut values: Vec<Value> = Vec::new();
    for o in doc.objects.values() {
        if !kinds.is_empty() && !kinds.iter().any(|k| k.eq_ignore_ascii_case(o.shape.kind())) {
            continue;
        }
        match props::property(doc, o, &path) {
            Prop::Value(v) => values.push(v),
            _ if field.eq_ignore_ascii_case("phase") => {
                values.push(Value::String(phase_of(o).id().into()))
            }
            _ => {}
        }
    }
    if values.is_empty() {
        return Err(format!("no object has the property '{field}'"));
    }
    let color = |i: usize| {
        let [r, g, b] = PALETTE[i % PALETTE.len()];
        Color([r, g, b, 255])
    };
    if let Some(n) = bins {
        if !(2..=MAX_RULES).contains(&n) {
            return Err(format!("bins must be from 2 to {MAX_RULES}"));
        }
        let nums: Vec<f64> = values.iter().filter_map(Value::as_f64).collect();
        if nums.is_empty() {
            return Err(format!("the values of '{field}' are not numbers"));
        }
        let (lo, hi) = nums
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
        let step = (hi - lo) / n as f64;
        if !(step.is_finite() && step > 0.0) {
            return Err("all values are equal: there is nothing to split".into());
        }
        // From light to dark blue.
        return Ok((0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                let c = |a: f64, b: f64| (a + (b - a) * t).round() as u8;
                VizRule {
                    criterion: Criterion::Range {
                        min: (i > 0).then_some(lo + step * i as f64),
                        max: (i + 1 < n).then(|| lo + step * (i + 1) as f64),
                    },
                    color: Color([c(222.0, 8.0), c(235.0, 48.0), c(247.0, 107.0), 255]),
                    label: String::new(),
                }
            })
            .collect());
    }
    let mut seen: Vec<(String, Value)> = Vec::new();
    for v in values {
        let key = match &v {
            Value::String(s) => s.trim().to_lowercase(),
            other => other.to_string(),
        };
        if !seen.iter().any(|(k, _)| *k == key) {
            seen.push((key, v));
        }
    }
    if seen.len() > MAX_AUTO_VALUES {
        return Err(format!(
            "'{field}' has {} different values. Use bins to split a number into ranges",
            seen.len()
        ));
    }
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(seen
        .into_iter()
        .enumerate()
        .map(|(i, (_, v))| VizRule {
            criterion: Criterion::Equals { value: v },
            color: color(i),
            label: String::new(),
        })
        .collect())
}

/// A row of the visualization legend.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendRow {
    pub label: String,
    pub color: Color,
    pub count: usize,
}

/// Counts the objects of each rule. The last value is the number of objects that no rule fits.
pub fn legend(doc: &Document, viz: &Visualization) -> (Vec<LegendRow>, usize) {
    let mut rows: Vec<LegendRow> = (0..viz.rules.len())
        .map(|i| LegendRow {
            label: viz.rule_label(i),
            color: viz.rules[i].color,
            count: 0,
        })
        .collect();
    let mut none = 0;
    for o in doc.objects.values() {
        if !viz.kinds.is_empty()
            && !viz
                .kinds
                .iter()
                .any(|k| k.eq_ignore_ascii_case(o.shape.kind()))
        {
            continue;
        }
        match classify(doc, o, viz) {
            Some(i) => rows[i].count += 1,
            None => none += 1,
        }
    }
    (rows, none)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Editor;
    use crate::model::Shape;
    use crate::plants::tree::TreeCondition;
    use serde_json::json;

    fn tree(at: f64, c: TreeCondition) -> Shape {
        Shape::ExistingTree {
            at: DVec2::new(at, 0.0),
            species: String::new(),
            dbh: 300.0,
            spread: 5000.0,
            height: 8000.0,
            condition: c,
            action: Default::default(),
            rpz_factor: None,
            rpz_cap: None,
        }
    }

    fn add(ed: &mut Editor, shape: Shape) -> crate::model::ObjectId {
        let out = ed
            .execute(&crate::command::Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        crate::model::ObjectId(out.created[0])
    }

    fn viz() -> Visualization {
        Visualization {
            name: "Condition".into(),
            field: "Condition".into(),
            rules: vec![
                VizRule {
                    criterion: Criterion::Equals {
                        value: json!("good"),
                    },
                    color: Color([0, 160, 0, 255]),
                    label: String::new(),
                },
                VizRule {
                    criterion: Criterion::Otherwise,
                    color: Color([200, 0, 0, 255]),
                    label: "Not good".into(),
                },
            ],
            target: VizTarget::Fill,
            enabled: true,
            kinds: vec!["existing_tree".into()],
            include_missing: false,
        }
    }

    #[test]
    fn phases_read_from_data_and_default_to_new() {
        let mut ed = Editor::new(Document::default());
        let id = add(&mut ed, tree(0.0, TreeCondition::Good));
        assert_eq!(phase_of(ed.doc().object(id).unwrap()), Phase::New);
        let mut set = std::collections::BTreeMap::new();
        set.insert(PHASE_KEY.to_string(), json!("Demolished"));
        ed.execute(&crate::command::Command::SetData {
            id,
            set,
            remove: vec![],
        })
        .unwrap();
        assert_eq!(phase_of(ed.doc().object(id).unwrap()), Phase::Demolished);
        assert_eq!(Phase::from_name("exist"), Some(Phase::Existing));
        assert_eq!(Phase::from_name("?"), None);
        for p in Phase::ALL {
            assert_eq!(Phase::from_name(p.id()), Some(p));
        }
    }

    #[test]
    fn phase_rules_recolor_and_hide() {
        let mut ed = Editor::new(Document::default());
        let id = add(&mut ed, tree(0.0, TreeCondition::Good));
        let mut set = std::collections::BTreeMap::new();
        set.insert(PHASE_KEY.to_string(), json!("demolished"));
        ed.execute(&crate::command::Command::SetData {
            id,
            set,
            remove: vec![],
        })
        .unwrap();
        let mut doc = ed.doc().clone();
        let o = doc.object(id).unwrap().clone();
        // No rules: unchanged.
        assert_eq!(
            display_attrs(&doc, &o, Attributes::default()),
            Attributes::default()
        );
        doc.drawing.view_rules.phases = Some(PhaseDisplay::default());
        let a = display_attrs(&doc, &o, Attributes::default());
        assert_eq!(a.pen, Some(Color([200, 40, 40, 255])));
        assert_eq!(a.fill, Some(Color::NONE));
        assert!(!hidden_by_phase(&doc, &o));
        doc.drawing
            .view_rules
            .phases
            .as_mut()
            .unwrap()
            .demolished
            .visible = false;
        assert!(hidden_by_phase(&doc, &o));
        assert!(doc.is_visible(&o), "calculations still see the object");
        assert!(!doc.is_displayed(&o));
        // Switched off: nothing happens.
        doc.drawing.view_rules.phases.as_mut().unwrap().enabled = false;
        assert!(!hidden_by_phase(&doc, &o));
        assert_eq!(
            display_attrs(&doc, &o, Attributes::default()),
            Attributes::default()
        );
    }

    #[test]
    fn a_visualization_colors_by_criteria_without_changing_data() {
        let mut ed = Editor::new(Document::default());
        let good = add(&mut ed, tree(0.0, TreeCondition::Good));
        let poor = add(&mut ed, tree(1000.0, TreeCondition::Poor));
        let line = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        let mut doc = ed.doc().clone();
        let before = doc.objects.clone();
        doc.drawing.view_rules.visualization = Some(viz());
        let v = doc.drawing.view_rules.visualization.clone().unwrap();
        let fill = |doc: &Document, id| {
            display_attrs(doc, doc.object(id).unwrap(), Attributes::default()).fill
        };
        assert_eq!(fill(&doc, good), Some(Color([0, 160, 0, 255])));
        assert_eq!(fill(&doc, poor), Some(Color([200, 0, 0, 255])));
        assert_eq!(fill(&doc, line), None, "the line is not an existing tree");
        let (rows, none) = legend(&doc, &v);
        assert_eq!((rows[0].count, rows[1].count, none), (1, 1, 0));
        assert_eq!(rows[0].label, "good");
        assert_eq!(rows[1].label, "Not good");
        assert!(doc.objects.iter().eq(before.iter()), "no object changed");
        // Pen and both.
        let mut v2 = v.clone();
        v2.target = VizTarget::Both;
        doc.drawing.view_rules.visualization = Some(v2);
        let a = display_attrs(&doc, doc.object(good).unwrap(), Attributes::default());
        assert_eq!(
            (a.fill, a.pen),
            (Some(Color([0, 160, 0, 255])), Some(Color([0, 160, 0, 255])))
        );
        // Off.
        doc.drawing
            .view_rules
            .visualization
            .as_mut()
            .unwrap()
            .enabled = false;
        assert_eq!(fill(&doc, good), None);
    }

    #[test]
    fn ranges_numbers_and_missing_values() {
        let mut ed = Editor::new(Document::default());
        let small = add(&mut ed, tree(0.0, TreeCondition::Good));
        let mut doc = ed.doc().clone();
        let v = Visualization {
            name: "Spread".into(),
            field: "Spread".into(),
            rules: vec![
                VizRule {
                    criterion: Criterion::Range {
                        min: None,
                        max: Some(4000.0),
                    },
                    color: Color([1, 1, 1, 255]),
                    label: String::new(),
                },
                VizRule {
                    criterion: Criterion::Range {
                        min: Some(4000.0),
                        max: Some(9000.0),
                    },
                    color: Color([2, 2, 2, 255]),
                    label: String::new(),
                },
                VizRule {
                    criterion: Criterion::Range {
                        min: Some(9000.0),
                        max: None,
                    },
                    color: Color([3, 3, 3, 255]),
                    label: String::new(),
                },
            ],
            target: VizTarget::Pen,
            enabled: true,
            kinds: vec![],
            include_missing: false,
        };
        assert_eq!(classify(&doc, doc.object(small).unwrap(), &v), Some(1));
        assert_eq!(v.rule_label(0), "below 4000");
        assert_eq!(v.rule_label(1), "4000 to 9000");
        assert_eq!(v.rule_label(2), "9000 and above");
        // A line has no spread: stays, unless include_missing is set with an Otherwise rule.
        let line = Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::X,
        };
        let mut ed2 = Editor::new(Document::default());
        let l = add(&mut ed2, line);
        doc = ed2.doc().clone();
        assert_eq!(classify(&doc, doc.object(l).unwrap(), &v), None);
        let mut v3 = v.clone();
        v3.rules.push(VizRule {
            criterion: Criterion::Otherwise,
            color: Color([9, 9, 9, 255]),
            label: String::new(),
        });
        assert_eq!(classify(&doc, doc.object(l).unwrap(), &v3), None);
        v3.include_missing = true;
        assert_eq!(classify(&doc, doc.object(l).unwrap(), &v3), Some(3));
        assert_eq!(v3.rule_label(3), "other");
        // Text and number compare.
        assert!(same(&json!("5"), &json!(5)));
        assert!(same(&json!(5), &json!("5.0")));
        assert!(same(&json!(true), &json!(true)));
        assert!(!same(&json!("a"), &json!("b")));
    }

    #[test]
    fn auto_rules_make_a_rule_per_value_or_per_range() {
        let mut ed = Editor::new(Document::default());
        for (i, c) in [
            TreeCondition::Good,
            TreeCondition::Poor,
            TreeCondition::Good,
        ]
        .into_iter()
        .enumerate()
        {
            add(&mut ed, tree(i as f64 * 1000.0, c));
        }
        let doc = ed.doc().clone();
        let r = auto_rules(&doc, "Condition", &[], None).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(
            r[0].criterion,
            Criterion::Equals {
                value: json!("good")
            }
        );
        assert_ne!(r[0].color, r[1].color);
        let viz = Visualization {
            name: "c".into(),
            field: "Condition".into(),
            rules: r,
            target: VizTarget::Fill,
            enabled: true,
            kinds: vec![],
            include_missing: false,
        };
        assert!(viz.is_valid().is_ok());
        let (rows, none) = legend(&doc, &viz);
        assert_eq!((rows[0].count, rows[1].count, none), (2, 1, 0));
        // Ranges over a number: X goes 0 to 2000.
        let r = auto_rules(&doc, "X", &["existing_tree".into()], Some(4)).unwrap();
        assert_eq!(r.len(), 4);
        assert!(matches!(
            r[0].criterion,
            Criterion::Range {
                min: None,
                max: Some(_)
            }
        ));
        assert!(matches!(
            r[3].criterion,
            Criterion::Range {
                min: Some(_),
                max: None
            }
        ));
        let viz = Visualization {
            rules: r,
            field: "X".into(),
            ..viz
        };
        assert!(viz.is_valid().is_ok());
        let (rows, _) = legend(&doc, &viz);
        assert_eq!(rows.iter().map(|r| r.count).sum::<usize>(), 3);
        // Errors.
        assert!(auto_rules(&doc, "", &[], None).is_err());
        assert!(auto_rules(&doc, "Nope", &[], None).is_err());
        assert!(auto_rules(&doc, "Condition", &[], Some(3)).is_err());
        assert!(auto_rules(&doc, "X", &[], Some(1)).is_err());
        assert!(
            auto_rules(&doc, "Height", &[], Some(3)).is_err(),
            "all heights are equal"
        );
        assert!(auto_rules(&doc, "Kind", &["line".into()], None).is_err());
        // Phase has a default.
        assert_eq!(auto_rules(&doc, "Phase", &[], None).unwrap().len(), 1);
        // Too many values.
        let mut ed = Editor::new(Document::default());
        for i in 0..(MAX_AUTO_VALUES + 1) {
            add(&mut ed, tree(i as f64, TreeCondition::Good));
        }
        assert!(auto_rules(ed.doc(), "X", &[], None).is_err());
        assert!(auto_rules(ed.doc(), "X", &[], Some(5)).is_ok());
    }

    #[test]
    fn the_phase_can_be_a_visualization_field() {
        let mut ed = Editor::new(Document::default());
        let id = add(&mut ed, tree(0.0, TreeCondition::Good));
        let doc = ed.doc().clone();
        let v = Visualization {
            name: "Phase".into(),
            field: "Phase".into(),
            rules: vec![VizRule {
                criterion: Criterion::Equals {
                    value: json!("new"),
                },
                color: Color([0, 0, 255, 255]),
                label: String::new(),
            }],
            target: VizTarget::Fill,
            enabled: true,
            kinds: vec![],
            include_missing: false,
        };
        assert_eq!(classify(&doc, doc.object(id).unwrap(), &v), Some(0));
    }

    #[test]
    fn rules_are_validated() {
        assert!(ViewRules::default().validate().is_ok());
        assert!(ViewRules::default().is_empty());
        let mut r = ViewRules {
            ne_grid: Some(NeGrid::new(0.0)),
            ..ViewRules::default()
        };
        assert!(r.validate().is_err());
        r.ne_grid = Some(NeGrid::new(10.0));
        assert!(r.validate().is_ok());
        assert!(!r.is_empty());
        let mut p = PhaseDisplay::default();
        p.existing.weight_factor = Some(-1.0);
        r.phases = Some(p);
        assert!(r.validate().is_err());
        r.phases = None;
        let mut v = viz();
        v.rules.clear();
        r.visualization = Some(v);
        assert!(r.validate().is_err());
        let mut v = viz();
        v.field = " ".into();
        r.visualization = Some(v);
        assert!(r.validate().is_err());
        let mut v = viz();
        v.rules[0].criterion = Criterion::Range {
            min: Some(5.0),
            max: Some(1.0),
        };
        r.visualization = Some(v);
        assert!(r.validate().is_err());
        let mut v = viz();
        v.rules[0].criterion = Criterion::Range {
            min: Some(f64::NAN),
            max: None,
        };
        r.visualization = Some(v);
        assert!(r.validate().is_err());
        let mut v = viz();
        v.rules[0].criterion = Criterion::Equals { value: json!([1]) };
        r.visualization = Some(v);
        assert!(r.validate().is_err());
        r.visualization = Some(viz());
        assert!(r.validate().is_ok());
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<ViewRules>(&text).unwrap(), r);
    }

    #[test]
    fn grid_lines_follow_the_georeference() {
        let g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        let lines = grid_lines(
            &g,
            &NeGrid::new(10.0),
            DVec2::new(-5000.0, -5000.0),
            DVec2::new(25_000.0, 15_000.0),
        );
        let e: Vec<&GridLine> = lines.iter().filter(|l| l.easting).collect();
        let n: Vec<&GridLine> = lines.iter().filter(|l| !l.easting).collect();
        assert_eq!(e.len(), 3); // 385000, 385010, 385020
        assert_eq!(n.len(), 2); // 6672000, 6672010
        assert_eq!(e[0].label, "E 385000");
        assert_eq!(n[1].label, "N 6672010");
        assert!((e[0].a.x - 0.0).abs() < 1e-6 && e[0].a.x == e[0].b.x);
        assert!((n[1].a.y - 10_000.0).abs() < 1e-6);
        // A rotated frame gives slanted lines that still cover the box.
        let rot = Georeference {
            rotation: 0.5,
            ..g.clone()
        };
        let l = grid_lines(
            &rot,
            &NeGrid::new(10.0),
            DVec2::ZERO,
            DVec2::new(20_000.0, 20_000.0),
        );
        assert!(!l.is_empty());
        assert!(
            l.iter()
                .all(|x| (x.a.x - x.b.x).abs() > 1e-3 || (x.a.y - x.b.y).abs() > 1e-3)
        );
        // A spacing that is too small gives no lines. A bad grid gives none.
        assert!(grid_lines(&g, &NeGrid::new(0.01), DVec2::ZERO, DVec2::splat(1e6)).is_empty());
        assert!(grid_lines(&g, &NeGrid::new(-1.0), DVec2::ZERO, DVec2::splat(1e6)).is_empty());
        assert!(grid_lines(&g, &NeGrid::new(1.0), DVec2::NAN, DVec2::splat(1.0)).is_empty());
        // A fractional spacing writes decimals.
        let f = grid_lines(
            &g,
            &NeGrid::new(0.5),
            DVec2::ZERO,
            DVec2::new(1000.0, 1000.0),
        );
        assert!(f.iter().any(|l| l.label == "E 385000.5"));
    }
}
