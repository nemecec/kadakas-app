//! Annotation resources: text styles, dimension styles (standards), tag styles and notes databases.
//! They live in the resource store as one variant, `ResourceBody::Annot`.

use serde::{Deserialize, Serialize};

use crate::model::Color;
use crate::resource::{ResourceBody, ResourceId, Resources};

/// Largest size of a text in paper mm.
pub const MAX_TEXT_MM: f64 = 200.0;

fn fin(name: &str, x: f64, lo: f64, hi: f64) -> Result<(), String> {
    if x.is_finite() && (lo..=hi).contains(&x) { Ok(()) } else { Err(format!("{name} must be a number from {lo} to {hi}")) }
}

/// Where the lines of a text sit against its position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Justify {
    #[default]
    Left,
    Center,
    Right,
}

/// The end of a dimension line or a leader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminator {
    /// A filled arrow head.
    #[default]
    Arrow,
    OpenArrow,
    /// A short slash across the line (architectural tick).
    Tick,
    Dot,
    None,
}

/// A text style: font, size on paper, and look.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    pub font: String,
    /// Height of the text on paper in mm. On a design layer it is multiplied by the scale.
    pub size_mm: f64,
    pub bold: bool,
    pub italic: bool,
    pub color: Option<Color>,
    pub justify: Justify,
    /// Distance between lines, as a multiple of the size.
    pub line_spacing: f64,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle { font: "sans-serif".into(), size_mm: 3.0, bold: false, italic: false, color: None, justify: Justify::Left, line_spacing: 1.3 }
    }
}

impl TextStyle {
    pub fn validate(&self) -> Result<(), String> {
        fin("size_mm", self.size_mm, 0.1, MAX_TEXT_MM)?;
        fin("line_spacing", self.line_spacing, 0.5, 5.0)?;
        if self.font.trim().is_empty() {
            return Err("a text style needs a font".into());
        }
        Ok(())
    }
}

/// The unit in which a dimension shows lengths. The model unit is mm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Mm,
    Cm,
    M,
    /// Inches.
    In,
    Ft,
    /// Feet and inches, for example 3'-4.5".
    FtIn,
}

impl LengthUnit {
    pub fn suffix(self) -> &'static str {
        match self {
            LengthUnit::Mm => " mm",
            LengthUnit::Cm => " cm",
            LengthUnit::M => " m",
            LengthUnit::In => "\"",
            LengthUnit::Ft => "'",
            LengthUnit::FtIn => "",
        }
    }
}

/// A tolerance that follows the value, in the unit of the dimension.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    pub plus: f64,
    pub minus: f64,
}

/// A dimension style (a dimension standard): text, units, ends and gaps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimensionStyle {
    pub font: String,
    /// Text height on paper in mm.
    pub text_size_mm: f64,
    pub unit: LengthUnit,
    /// Decimals of lengths.
    pub precision: u8,
    /// Add the unit after the number (feet and inches always show it).
    pub show_unit: bool,
    /// Decimals of angles, in degrees.
    pub angle_precision: u8,
    pub terminator: Terminator,
    pub terminator_size_mm: f64,
    /// Gap between the measured point and the start of an extension line.
    pub ext_gap_mm: f64,
    /// How far an extension line goes past the dimension line.
    pub ext_overshoot_mm: f64,
    /// Distance between the lines of a baseline dimension.
    pub baseline_spacing_mm: f64,
    /// Gap between the dimension line and the text.
    pub text_gap_mm: f64,
    pub prefix: String,
    pub suffix: String,
    pub tolerance: Option<Tolerance>,
    pub color: Option<Color>,
}

impl Default for DimensionStyle {
    fn default() -> Self {
        DimensionStyle {
            font: "sans-serif".into(),
            text_size_mm: 2.5,
            unit: LengthUnit::Mm,
            precision: 0,
            show_unit: false,
            angle_precision: 1,
            terminator: Terminator::Arrow,
            terminator_size_mm: 2.5,
            ext_gap_mm: 1.0,
            ext_overshoot_mm: 2.0,
            baseline_spacing_mm: 7.0,
            text_gap_mm: 0.8,
            prefix: String::new(),
            suffix: String::new(),
            tolerance: None,
            color: None,
        }
    }
}

impl DimensionStyle {
    pub fn validate(&self) -> Result<(), String> {
        fin("text_size_mm", self.text_size_mm, 0.1, MAX_TEXT_MM)?;
        fin("terminator_size_mm", self.terminator_size_mm, 0.0, 50.0)?;
        fin("ext_gap_mm", self.ext_gap_mm, 0.0, 50.0)?;
        fin("ext_overshoot_mm", self.ext_overshoot_mm, 0.0, 50.0)?;
        fin("baseline_spacing_mm", self.baseline_spacing_mm, 1.0, 100.0)?;
        fin("text_gap_mm", self.text_gap_mm, 0.0, 50.0)?;
        if self.precision > 8 || self.angle_precision > 8 {
            return Err("precision is at most 8 decimals".into());
        }
        if let Some(t) = self.tolerance {
            fin("tolerance plus", t.plus, 0.0, 1.0e6)?;
            fin("tolerance minus", t.minus, 0.0, 1.0e6)?;
        }
        if self.font.trim().is_empty() {
            return Err("a dimension style needs a font".into());
        }
        Ok(())
    }
}

/// The frame around the text of a data tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagFrame {
    #[default]
    None,
    Box,
    Underline,
    Circle,
}

/// A tag style: the layout of a data tag. Every line is text with fields in braces. A field is a
/// worksheet expression over the properties of the tagged object, for example `{IdCode}`,
/// `{ROUND(Area, 1)} m2` or `{Soil.Depth}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagStyle {
    pub lines: Vec<String>,
    pub font: String,
    pub size_mm: f64,
    pub frame: TagFrame,
    /// Draw a leader line from the label to the object.
    pub leader: bool,
    pub terminator: Terminator,
    /// Space between the text and the frame, in paper mm.
    pub pad_mm: f64,
    pub color: Option<Color>,
}

impl Default for TagStyle {
    fn default() -> Self {
        TagStyle {
            lines: vec!["{Kind}".into()],
            font: "sans-serif".into(),
            size_mm: 2.5,
            frame: TagFrame::None,
            leader: true,
            terminator: Terminator::Dot,
            pad_mm: 1.0,
            color: None,
        }
    }
}

/// Most lines in a tag style.
pub const MAX_TAG_LINES: usize = 20;

impl TagStyle {
    pub fn validate(&self) -> Result<(), String> {
        fin("size_mm", self.size_mm, 0.1, MAX_TEXT_MM)?;
        fin("pad_mm", self.pad_mm, 0.0, 50.0)?;
        if self.lines.is_empty() || self.lines.len() > MAX_TAG_LINES {
            return Err(format!("a tag style has 1 to {MAX_TAG_LINES} lines"));
        }
        let mut open = 0i32;
        for line in &self.lines {
            for c in line.chars() {
                match c {
                    '{' => open += 1,
                    '}' => open -= 1,
                    _ => {}
                }
                if !(0..=1).contains(&open) {
                    return Err(format!("the braces of '{line}' do not match"));
                }
            }
            if open != 0 {
                return Err(format!("the braces of '{line}' do not match"));
            }
        }
        Ok(())
    }
}

/// One note of a notes database.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub key: String,
    pub text: String,
}

/// A list of standard notes. Callouts refer to a note by its key, so a change of the text reaches all callouts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotesDb {
    pub notes: Vec<Note>,
}

impl NotesDb {
    pub fn get(&self, key: &str) -> Option<&Note> {
        self.notes.iter().find(|n| n.key.eq_ignore_ascii_case(key.trim()))
    }

    pub fn validate(&self) -> Result<(), String> {
        for (i, n) in self.notes.iter().enumerate() {
            if n.key.trim().is_empty() {
                return Err("a note needs a key".into());
            }
            if self.notes[..i].iter().any(|o| o.key.eq_ignore_ascii_case(n.key.trim())) {
                return Err(format!("two notes have the key '{}'", n.key.trim()));
            }
        }
        Ok(())
    }
}

/// The content of an annotation resource. The `type` key in JSON names the variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnnotResource {
    TextStyle(TextStyle),
    DimensionStyle(DimensionStyle),
    TagStyle(TagStyle),
    NotesDatabase(NotesDb),
}

/// The JSON type names of the annotation resources.
pub const TYPE_NAMES: [&str; 4] = ["text_style", "dimension_style", "tag_style", "notes_database"];

impl AnnotResource {
    pub fn type_name(&self) -> &'static str {
        match self {
            AnnotResource::TextStyle(_) => TYPE_NAMES[0],
            AnnotResource::DimensionStyle(_) => TYPE_NAMES[1],
            AnnotResource::TagStyle(_) => TYPE_NAMES[2],
            AnnotResource::NotesDatabase(_) => TYPE_NAMES[3],
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            AnnotResource::TextStyle(s) => s.validate(),
            AnnotResource::DimensionStyle(s) => s.validate(),
            AnnotResource::TagStyle(s) => s.validate(),
            AnnotResource::NotesDatabase(s) => s.validate(),
        }
    }

    /// Reads the JSON of a resource body of an annotation type. `None` for other types.
    pub fn from_json(v: &serde_json::Value) -> Option<Result<AnnotResource, String>> {
        let t = v.get("type")?.as_str()?;
        TYPE_NAMES.contains(&t).then(|| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
    }
}

impl ResourceBody {
    /// A resource body for an annotation resource.
    pub fn annot(r: AnnotResource) -> Self {
        ResourceBody::Annot(Box::new(r))
    }
}

impl Resources {
    fn annot(&self, id: ResourceId) -> Option<&AnnotResource> {
        match &self.get(id)?.body {
            ResourceBody::Annot(a) => Some(a.as_ref()),
            _ => None,
        }
    }

    pub fn text_style(&self, id: ResourceId) -> Option<&TextStyle> {
        match self.annot(id)? {
            AnnotResource::TextStyle(s) => Some(s),
            _ => None,
        }
    }

    pub fn dimension_style(&self, id: ResourceId) -> Option<&DimensionStyle> {
        match self.annot(id)? {
            AnnotResource::DimensionStyle(s) => Some(s),
            _ => None,
        }
    }

    pub fn tag_style(&self, id: ResourceId) -> Option<&TagStyle> {
        match self.annot(id)? {
            AnnotResource::TagStyle(s) => Some(s),
            _ => None,
        }
    }

    pub fn notes_db(&self, id: ResourceId) -> Option<&NotesDb> {
        match self.annot(id)? {
            AnnotResource::NotesDatabase(s) => Some(s),
            _ => None,
        }
    }

    /// The ID of the annotation resource of this type name (`text_style`, ...) and name (case ignored).
    pub fn find_annot(&self, type_name: &str, name: &str) -> Option<ResourceId> {
        self.iter().find(|r| r.body.type_name() == type_name && r.name.eq_ignore_ascii_case(name.trim())).map(|r| r.id)
    }
}
