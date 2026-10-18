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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FIRST_LAYER, NONE_CLASS};
    use crate::plants::style::PlantStyle;
    use crate::resource::{Resource, ResourceBody};
    use serde_json::json;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn info(bubble: bool, levels: u8) -> TagInfo {
        TagInfo {
            code: "AC".into(),
            count: 12,
            name: "Field maple".into(),
            details: "14-16 cm, C10".into(),
            format: TagFormat { levels, bubble },
        }
    }

    #[test]
    fn a_group_tag_sits_above_the_middle_of_the_group() {
        let Shape::PlantTag {
            at, anchor, height, ..
        } = group_tag(
            ResourceId::from_num(7),
            &[v(0.0, 0.0), v(4000.0, 0.0), v(2000.0, 1000.0)],
            3000.0,
        )
        else {
            panic!("not a tag");
        };
        assert_eq!(anchor, v(2000.0, 1000.0 / 3.0));
        assert_eq!(at, v(2000.0, 1000.0 + 1500.0 + DEFAULT_TAG_HEIGHT));
        assert_eq!(height, DEFAULT_TAG_HEIGHT);
        // No plants: the tag stays at the origin and does not become NaN.
        let Shape::PlantTag { at, anchor, .. } = group_tag(ResourceId::from_num(7), &[], 0.0)
        else {
            panic!("not a tag");
        };
        assert_eq!(anchor, DVec2::ZERO);
        assert_eq!(at, v(0.0, DEFAULT_TAG_HEIGHT));
    }

    #[test]
    fn the_format_of_a_tag_is_read_and_kept_in_range() {
        let tag = |data: Option<serde_json::Value>| {
            let mut o = Object::new(
                ObjectId::from_num(1),
                FIRST_LAYER,
                NONE_CLASS,
                group_tag(ResourceId::from_num(7), &[], 0.0),
            );
            if let Some(d) = data {
                o.data.insert(TAG_FORMAT_KEY.into(), d);
            }
            o
        };
        assert_eq!(TagFormat::of(&tag(None)), TagFormat::default());
        assert_eq!(TagFormat::default().levels, 1);
        assert!(!TagFormat::default().bubble);
        let f = TagFormat::of(&tag(Some(json!({"levels": 9, "bubble": true}))));
        assert_eq!(
            f,
            TagFormat {
                levels: 3,
                bubble: true
            }
        );
        assert_eq!(TagFormat::of(&tag(Some(json!({"levels": 0})))).levels, 1);
        // Bad types give the defaults.
        assert_eq!(
            TagFormat::of(&tag(Some(json!({"levels": "two", "bubble": 1})))),
            TagFormat::default()
        );
    }

    #[test]
    fn the_lines_of_a_tag_follow_its_levels_and_bubble() {
        assert_eq!(info(false, 1).text(), "AC (12)");
        assert_eq!(info(false, 1).lines(), ["AC (12)"]);
        assert_eq!(info(false, 2).lines(), ["AC (12)", "Field maple"]);
        assert_eq!(
            info(false, 3).lines(),
            ["AC (12)", "Field maple", "14-16 cm, C10"]
        );
        // The code is in the bubble, so the first line has only the count.
        assert_eq!(info(true, 2).lines(), ["(12)", "Field maple"]);
        // An empty name or detail leaves its line out.
        let mut bare = info(false, 3);
        bare.name.clear();
        bare.details.clear();
        assert_eq!(bare.lines(), ["AC (12)"]);
    }

    #[test]
    fn the_layout_stacks_lines_and_puts_the_code_in_a_bubble() {
        let plain = info(false, 3).layout(100.0);
        assert!(plain.bubble.is_none());
        let ys: Vec<f64> = plain.texts.iter().map(|t| t.at.y).collect();
        // From the top: each line 1.25 text heights under the one above, the last at 0.
        assert_eq!(ys, [250.0, 125.0, 0.0]);
        assert!(plain.texts.iter().all(|t| !t.centered && t.at.x == 0.0));

        let bubble = info(true, 2).layout(100.0);
        // A two letter code: radius = h * (0.3 * 2 + 0.5).
        let (centre, r) = bubble.bubble.unwrap();
        assert!((r - 110.0).abs() < 1e-9, "{r}");
        assert_eq!(centre, v(r, 0.0));
        assert_eq!(bubble.texts[0].text, "AC");
        assert!(bubble.texts[0].centered);
        // The lines sit right of the bubble, centred round the bubble.
        let x0 = 2.0 * r + 40.0;
        assert_eq!(bubble.texts.len(), 3);
        assert!(bubble.texts[1..].iter().all(|t| t.at.x == x0));
        assert!((bubble.texts[1].at.y - (62.5 - 50.0)).abs() < 1e-9);
        assert!((bubble.texts[2].at.y - (-62.5 - 50.0)).abs() < 1e-9);
    }

    #[test]
    fn the_info_of_a_tag_comes_from_its_style_and_the_plants_of_its_layer() {
        let mut doc = Document::default();
        let style_id = ResourceId::from_num(7);
        doc.resources.push(Resource {
            id: style_id,
            name: "Field maple".into(),
            body: ResourceBody::plant_style(PlantStyle {
                botanical_name: "Acer campestre".into(),
                size: " 14-16 cm ".into(),
                container: "C10".into(),
                ..PlantStyle::default()
            }),
        });
        let mut add = |n: u64, shape: Shape, linked: bool| {
            let id = ObjectId::from_num(n);
            let mut o = Object::new(id, FIRST_LAYER, NONE_CLASS, shape);
            if linked {
                link::set_link(
                    &mut o,
                    Some(&link::PlantLink {
                        style: style_id,
                        overrides: Default::default(),
                    }),
                );
            }
            doc.objects.insert(id, o);
            id
        };
        let tag = add(1, group_tag(style_id, &[v(0.0, 0.0)], 1000.0), false);
        let line = add(
            2,
            Shape::Line {
                a: v(0.0, 0.0),
                b: v(1.0, 0.0),
            },
            false,
        );
        for n in 3..6 {
            let at = v(n as f64 * 1000.0, 0.0);
            add(
                n,
                Shape::Plant {
                    at,
                    species: String::new(),
                    spread: 1000.0,
                    height: 1000.0,
                },
                true,
            );
        }
        assert!(tag_info(&doc, line).is_none());
        assert!(tag_info(&doc, ObjectId::from_num(99)).is_none());
        let counts = tag_counts(&doc);
        assert_eq!(counts.get(&(FIRST_LAYER, style_id)), Some(&3));
        let i = tag_info(&doc, tag).unwrap();
        assert_eq!(i.count, 3);
        // No code and no common name: the style name and the botanical name stand in.
        assert_eq!(i.code, "Field maple");
        assert_eq!(i.name, "Acer campestre");
        assert_eq!(i.details, "14-16 cm, C10");
        assert_eq!(i, tag_info_with(&doc, tag, &counts).unwrap());
        // A code and a common name win; a tag whose layer has no plants counts none.
        doc.resources.push(Resource {
            id: ResourceId::from_num(8),
            name: "Other".into(),
            body: ResourceBody::plant_style(PlantStyle {
                id_code: "AC".into(),
                common_name: "Maple".into(),
                botanical_name: "Acer".into(),
                ..PlantStyle::default()
            }),
        });
        let other = ObjectId::from_num(9);
        doc.objects.insert(
            other,
            Object::new(
                other,
                FIRST_LAYER,
                NONE_CLASS,
                group_tag(ResourceId::from_num(8), &[v(0.0, 0.0)], 1000.0),
            ),
        );
        let o = tag_info(&doc, other).unwrap();
        assert_eq!(
            (o.code.as_str(), o.name.as_str(), o.count),
            ("AC", "Maple", 0)
        );
        assert_eq!(o.details, "");
    }
}
