//! Plant tags: the text of a tag and where its parts go, computed live from the document. The
//! SVG output of core draws tags with it.

use std::collections::HashMap;

use glam::DVec2;
use serde::Serialize;

use super::link;
use crate::model::{DEFAULT_TAG_HEIGHT, Document, LayerId, Object, ObjectId, Shape};
use crate::resource::ResourceId;

/// A plant tag for a group of plants placed together: the leader ends at the middle of the group
/// and the label sits above it. `spread` is the canopy diameter of the plants.
pub fn group_tag(style: ResourceId, positions: &[DVec2], spread: f64) -> Shape {
    let n = positions.len().max(1) as f64;
    let centroid = positions.iter().fold(DVec2::ZERO, |a, p| a + *p) / n;
    let top = positions
        .iter()
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, f64::max);
    let top = if top.is_finite() { top } else { 0.0 };
    let label = DVec2::new(centroid.x, top + spread / 2.0 + DEFAULT_TAG_HEIGHT);
    Shape::PlantTag {
        at: label,
        anchor: centroid,
        style,
        height: DEFAULT_TAG_HEIGHT,
    }
}

/// The data key of a plant tag that holds its format: `{levels: 1 to 3, bubble: bool}`.
pub const TAG_FORMAT_KEY: &str = "tag_format";

/// How much a plant tag shows. Level 1: the ID code and the count. Level 2 adds the plant
/// name. Level 3 adds the size and the container. With a bubble, the ID code sits in a circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TagFormat {
    pub levels: u8,
    pub bubble: bool,
}

impl Default for TagFormat {
    fn default() -> Self {
        TagFormat {
            levels: 1,
            bubble: false,
        }
    }
}

impl TagFormat {
    /// The format of a tag object. A missing or bad value gives the default.
    pub fn of(obj: &Object) -> TagFormat {
        let v = obj.data.get(TAG_FORMAT_KEY);
        TagFormat {
            levels: v
                .and_then(|v| v.get("levels"))
                .and_then(serde_json::Value::as_u64)
                .map_or(1, |n| n.clamp(1, 3) as u8),
            bubble: v
                .and_then(|v| v.get("bubble"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }
    }
}

/// What a plant tag shows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TagInfo {
    /// The ID code of the style, or the style name if it has no code.
    pub code: String,
    /// Plants of this style on the layer of the tag.
    pub count: usize,
    /// The plant name: the common name, else the botanical name (level 2).
    pub name: String,
    /// Size and container, for example `14-16 cm, C10` (level 3).
    pub details: String,
    pub format: TagFormat,
}

/// One line of text in a tag layout.
#[derive(Clone, Debug, PartialEq)]
pub struct TagText {
    /// Bottom left of the text, or its center if `centered`, from the label position. Y is up.
    pub at: DVec2,
    pub text: String,
    pub centered: bool,
}

/// The shapes of a tag in model mm, relative to the label position `at`.
#[derive(Clone, Debug, PartialEq)]
pub struct TagLayout {
    /// The ID bubble: center and radius.
    pub bubble: Option<(DVec2, f64)>,
    pub texts: Vec<TagText>,
}

impl TagInfo {
    /// The tag text on one line, for example `AC (12)`.
    pub fn text(&self) -> String {
        format!("{} ({})", self.code, self.count)
    }

    /// The text lines from the top, by level.
    pub fn lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        if !self.format.bubble {
            v.push(self.text());
        } else {
            v.push(format!("({})", self.count));
        }
        if self.format.levels >= 2 && !self.name.is_empty() {
            v.push(self.name.clone());
        }
        if self.format.levels >= 3 && !self.details.is_empty() {
            v.push(self.details.clone());
        }
        v
    }

    /// Where the text and the bubble go, for a text height of `h` mm.
    pub fn layout(&self, h: f64) -> TagLayout {
        let lines = self.lines();
        let n = lines.len() as f64;
        let step = 1.25 * h;
        if self.format.bubble {
            let chars = self.code.chars().count().max(2) as f64;
            let r = h * (0.3 * chars + 0.5);
            let x0 = 2.0 * r + 0.4 * h;
            let mut texts = vec![TagText {
                at: DVec2::new(r, 0.0),
                text: self.code.clone(),
                centered: true,
            }];
            for (k, t) in lines.into_iter().enumerate() {
                let centre = ((n - 1.0) / 2.0 - k as f64) * step;
                texts.push(TagText {
                    at: DVec2::new(x0, centre - 0.5 * h),
                    text: t,
                    centered: false,
                });
            }
            TagLayout {
                bubble: Some((DVec2::new(r, 0.0), r)),
                texts,
            }
        } else {
            let texts = lines
                .into_iter()
                .enumerate()
                .map(|(k, t)| TagText {
                    at: DVec2::new(0.0, (n - 1.0 - k as f64) * step),
                    text: t,
                    centered: false,
                })
                .collect();
            TagLayout {
                bubble: None,
                texts,
            }
        }
    }
}

/// Number of plants per layer and style. Count once, then ask `tag_info_with` for each tag.
pub type TagCounts = HashMap<(LayerId, ResourceId), usize>;

/// The plants of each style on each layer.
pub fn tag_counts(doc: &Document) -> TagCounts {
    let mut counts = TagCounts::new();
    for o in doc.objects.values() {
        if matches!(o.shape, Shape::Plant { .. })
            && let Some(style) = link::style_of(o)
        {
            *counts.entry((o.layer, style)).or_default() += 1;
        }
    }
    counts
}

/// The ID code and count for a tag object. `None` if the object is not a tag or its style is gone.
pub fn tag_info(doc: &Document, tag: ObjectId) -> Option<TagInfo> {
    tag_info_with(doc, tag, &tag_counts(doc))
}

/// Like `tag_info`, with the counts given. Use it to draw many tags.
pub fn tag_info_with(doc: &Document, tag: ObjectId, counts: &TagCounts) -> Option<TagInfo> {
    let obj = doc.object(tag)?;
    let Shape::PlantTag { style, .. } = &obj.shape else {
        return None;
    };
    let res = doc.resources.get(*style)?;
    let s = doc.resources.plant_style(*style)?;
    let code = if s.id_code.trim().is_empty() {
        res.name.clone()
    } else {
        s.id_code.clone()
    };
    let count = counts.get(&(obj.layer, *style)).copied().unwrap_or(0);
    let name = if s.common_name.trim().is_empty() {
        s.botanical_name.clone()
    } else {
        s.common_name.clone()
    };
    let details = [s.size.trim(), s.container.trim()]
        .iter()
        .filter(|t| !t.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    Some(TagInfo {
        code,
        count,
        name,
        details,
        format: TagFormat::of(obj),
    })
}
