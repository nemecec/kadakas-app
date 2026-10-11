//! The data of the annotation objects. Sizes named `*_mm` are paper millimeters: they get
//! bigger on a design layer when the scale is smaller (see `draw::Ctx`). Positions are in the
//! coordinates of the layer, or in paper mm inside a viewport.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::model::ObjectId;
use crate::resource::ResourceId;

use super::style::Terminator;

/// Which part of an object a dimension or leader sticks to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "at", rename_all = "snake_case")]
pub enum AnchorTarget {
    /// A vertex of the object. Lines have 2 (a, b), rectangles 4 corners (min, then counterclockwise),
    /// polylines and boundaries one per vertex, circles 4 quadrant points (east, north, west, south)
    /// and arcs 3 (start, end, middle).
    Vertex { index: usize },
    /// A point on an edge. Edge `i` runs from vertex `i` to the next vertex (a closed shape wraps
    /// around). `t` is the position along the edge, 0 to 1.
    Edge {
        index: usize,
        #[serde(default = "half")]
        t: f64,
    },
    /// The center of a circle, an arc or a rectangle, or the position of a plant or text.
    Center,
}

fn half() -> f64 {
    0.5
}

/// A link to an object. The annotation follows the object when it moves.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub object: ObjectId,
    pub target: AnchorTarget,
    /// The number of vertices that the object had when the anchor was last applied. If the count
    /// changes, the anchor binds again to the vertex that is still where it was, or lets go
    /// (see `sync`). `None` before the first sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

impl Anchor {
    pub fn new(object: ObjectId, target: AnchorTarget) -> Anchor {
        Anchor {
            object,
            target,
            count: None,
        }
    }
}

/// The direction in which a linear dimension measures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    #[default]
    Horizontal,
    Vertical,
    /// Along the line between the two points.
    Aligned,
}

/// The kind of a dimension and how its points are used.
///
/// - `Linear`: 2 points.
/// - `Chain`: 3 or more points. One dimension line, one value per segment between the points.
/// - `Baseline`: 3 or more points. Every value runs from the first point, on stacked lines.
/// - `Ordinate`: 2 or more points. The first is the datum (zero), every other point gets its
///   distance from the datum along the axis (`horizontal` measures along X, `vertical` along Y).
/// - `Angular`: 3 points: the vertex, a point on the first ray, a point on the second ray.
///   It measures counterclockwise from the first ray to the second.
/// - `Radial` and `Diameter`: 2 points: the center and a point on the circle. With an anchor on the
///   center of a circle or an arc, the radius follows the object.
/// - `ArcLength`: 3 points: the center, the start and the end of a counterclockwise arc. With an anchor
///   on the center of an arc object, the points follow the arc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DimMode {
    Linear { axis: Axis },
    Chain { axis: Axis },
    Baseline { axis: Axis },
    Ordinate { axis: Axis },
    Angular,
    Radial,
    Diameter,
    ArcLength,
}

impl DimMode {
    /// The name that the API uses (`horizontal`, `chain_vertical`, ...).
    pub fn name(self) -> &'static str {
        match self {
            DimMode::Linear {
                axis: Axis::Horizontal,
            } => "horizontal",
            DimMode::Linear {
                axis: Axis::Vertical,
            } => "vertical",
            DimMode::Linear {
                axis: Axis::Aligned,
            } => "aligned",
            DimMode::Chain {
                axis: Axis::Horizontal,
            } => "chain_horizontal",
            DimMode::Chain {
                axis: Axis::Vertical,
            } => "chain_vertical",
            DimMode::Chain {
                axis: Axis::Aligned,
            } => "chain_aligned",
            DimMode::Baseline {
                axis: Axis::Horizontal,
            } => "baseline_horizontal",
            DimMode::Baseline {
                axis: Axis::Vertical,
            } => "baseline_vertical",
            DimMode::Baseline {
                axis: Axis::Aligned,
            } => "baseline_aligned",
            DimMode::Ordinate {
                axis: Axis::Vertical,
            } => "ordinate_vertical",
            DimMode::Ordinate { .. } => "ordinate_horizontal",
            DimMode::Angular => "angular",
            DimMode::Radial => "radial",
            DimMode::Diameter => "diameter",
            DimMode::ArcLength => "arc_length",
        }
    }

    pub const NAMES: [&'static str; 15] = [
        "horizontal",
        "vertical",
        "aligned",
        "chain_horizontal",
        "chain_vertical",
        "chain_aligned",
        "baseline_horizontal",
        "baseline_vertical",
        "baseline_aligned",
        "ordinate_horizontal",
        "ordinate_vertical",
        "angular",
        "radial",
        "diameter",
        "arc_length",
    ];

    pub fn from_name(name: &str) -> Option<DimMode> {
        let n = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        let axis_of = |s: &str| match s {
            "horizontal" => Some(Axis::Horizontal),
            "vertical" => Some(Axis::Vertical),
            "aligned" => Some(Axis::Aligned),
            _ => None,
        };
        Some(match n.as_str() {
            "angular" => DimMode::Angular,
            "radial" | "radius" => DimMode::Radial,
            "diameter" => DimMode::Diameter,
            "arc_length" | "arc" => DimMode::ArcLength,
            "linear" => DimMode::Linear {
                axis: Axis::Aligned,
            },
            s => {
                if let Some(a) = axis_of(s) {
                    DimMode::Linear { axis: a }
                } else if let Some(a) = s.strip_prefix("chain_").and_then(axis_of) {
                    DimMode::Chain { axis: a }
                } else if let Some(a) = s.strip_prefix("baseline_").and_then(axis_of) {
                    DimMode::Baseline { axis: a }
                } else {
                    let a = s.strip_prefix("ordinate_").and_then(axis_of)?;
                    DimMode::Ordinate { axis: a }
                }
            }
        })
    }

    /// The fewest points the mode needs, and the most (`usize::MAX` for no limit).
    pub fn point_range(self) -> (usize, usize) {
        match self {
            DimMode::Linear { .. } | DimMode::Radial | DimMode::Diameter => (2, 2),
            DimMode::Angular | DimMode::ArcLength => (3, 3),
            DimMode::Chain { .. } | DimMode::Baseline { .. } => (3, MAX_DIM_POINTS),
            DimMode::Ordinate { .. } => (2, MAX_DIM_POINTS),
        }
    }
}

/// Most points in one chain, baseline or ordinate dimension.
pub const MAX_DIM_POINTS: usize = 200;

/// A dimension. The points are the geometry that is drawn. An anchor ties a point to an object,
/// and the commands move the point with the object. When the object is deleted the anchor goes
/// and the point stays where it was.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub mode: DimMode,
    pub points: Vec<DVec2>,
    /// One entry per point (a missing entry means no anchor).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<Option<Anchor>>,
    /// Distance of the dimension line from the first point, in paper mm. The sign picks the side.
    #[serde(default)]
    pub offset: f64,
    /// A dimension style (standard). None: the built-in default style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceId>,
    /// Text that replaces the measured value (of a single dimension).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// True when an anchor let go because its vertex or edge is gone. The point keeps its place.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detached: bool,
}

/// Text with a text style. The size is paper mm, so it follows the scale of the layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyledText {
    pub at: DVec2,
    /// Lines are separated by a newline.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceId>,
}

/// A leader line of a callout. The tip is the first point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Leader {
    /// From the tip to the bend points. The last point connects to the callout text.
    pub points: Vec<DVec2>,
    /// Ties the tip to an object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
}

/// A note with leader lines. The text comes from a note of a notes database, or is typed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Callout {
    /// Where the leaders end and the shoulder starts.
    pub at: DVec2,
    #[serde(default)]
    pub text: String,
    /// The key of a note in `notes_db`. The text of the note replaces `text`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_db: Option<ResourceId>,
    /// A text style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceId>,
    pub leaders: Vec<Leader>,
    /// Length of the horizontal shoulder in paper mm. Zero for none.
    #[serde(default = "default_shoulder")]
    pub shoulder_mm: f64,
    #[serde(default)]
    pub terminator: Terminator,
    /// True when the anchor of a leader let go because its vertex or edge is gone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detached: bool,
}

fn default_shoulder() -> f64 {
    5.0
}

/// A label that shows data of an object. The tag style gives the layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataTag {
    pub style: ResourceId,
    /// The tagged object.
    pub object: ObjectId,
    /// Where the label sits, relative to the reference point of the object, in paper mm.
    #[serde(default)]
    pub offset: DVec2,
    /// The place of the label (kept up to date by the commands).
    #[serde(default)]
    pub at: DVec2,
    /// The reference point of the object (kept up to date by the commands).
    #[serde(default)]
    pub anchor: DVec2,
}

/// A legend: the objects that match the criteria, grouped, with a swatch, a label and a count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Legend {
    /// Top left corner.
    pub at: DVec2,
    #[serde(default)]
    pub title: String,
    /// Which objects to list: a worksheet expression such as `Kind = "plant"`. Empty: all objects.
    #[serde(default)]
    pub criteria: String,
    /// What makes a group: a worksheet expression, for example `Style` (the default), `Class` or `Material`.
    #[serde(default = "default_group")]
    pub group_by: String,
    #[serde(default = "yes")]
    pub show_count: bool,
    /// Width and height of a swatch in paper mm.
    #[serde(default = "default_swatch")]
    pub swatch_mm: f64,
    /// A text style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceId>,
}

fn default_group() -> String {
    "Style".into()
}

fn yes() -> bool {
    true
}

fn default_swatch() -> f64 {
    6.0
}

/// How a north arrow looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NorthStyle {
    /// A half-filled arrow head with an N.
    #[default]
    Arrow,
    /// A circle with a cross and an N.
    Compass,
    /// A plain arrow with an N.
    Simple,
}

/// A north arrow. Rotation 0 points up on the page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NorthArrow {
    pub at: DVec2,
    /// Height in paper mm.
    #[serde(default = "default_north_size")]
    pub size_mm: f64,
    /// Counterclockwise, in degrees.
    #[serde(default)]
    pub rotation_deg: f64,
    /// Add the rotation of the georeference of the document, so the arrow shows true north.
    #[serde(default)]
    pub use_georef: bool,
    #[serde(default)]
    pub style: NorthStyle,
}

fn default_north_size() -> f64 {
    15.0
}

/// The unit of a scale bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarUnit {
    Mm,
    #[default]
    M,
    Ft,
}

impl BarUnit {
    /// Real millimeters in one unit.
    pub fn mm(self) -> f64 {
        match self {
            BarUnit::Mm => 1.0,
            BarUnit::M => 1000.0,
            BarUnit::Ft => 304.8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BarUnit::Mm => "mm",
            BarUnit::M => "m",
            BarUnit::Ft => "ft",
        }
    }
}

/// A scale bar. It follows the scale of its layer or viewport, so the bar always stands for the
/// same real length. `segment * segments` in `unit` is the full length.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScaleBar {
    /// Left end of the bar, at the lower edge.
    pub at: DVec2,
    #[serde(default)]
    pub unit: BarUnit,
    /// Length of one segment in `unit`.
    #[serde(default = "default_segment")]
    pub segment: f64,
    #[serde(default = "default_segments")]
    pub segments: u32,
    /// Height of the bar in paper mm.
    #[serde(default = "default_bar_height")]
    pub height_mm: f64,
    /// Use this scale denominator instead of the one of the layer or viewport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// A text style for the labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceId>,
}

fn default_segment() -> f64 {
    5.0
}

fn default_segments() -> u32 {
    4
}

fn default_bar_height() -> f64 {
    2.0
}

impl ScaleBar {
    /// The full length of the bar in real mm.
    pub fn real_length_mm(&self) -> f64 {
        self.segment * self.segments as f64 * self.unit.mm()
    }
}

/// A cloud around changes, with the data of the revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RevisionCloud {
    /// The outline (3 or more points).
    pub points: Vec<DVec2>,
    /// Size of one bump in paper mm.
    #[serde(default = "default_bump")]
    pub bump_mm: f64,
    /// The revision mark, for example "A" or "3".
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// Draw the triangle with the revision mark.
    #[serde(default = "yes")]
    pub mark: bool,
}

fn default_bump() -> f64 {
    6.0
}
