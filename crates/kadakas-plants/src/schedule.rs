//! Plant schedule and the existing tree list. All are computed live from the document.

use serde::Serialize;

use kadakas_core::csv::write_rows as write_csv;
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::plants::link;
use kadakas_core::plants::style::{PlantCategory, PlantStyle, QuantityType};
use kadakas_core::plants::tree::rpz_radius;
use kadakas_core::resource::ResourceId;

/// One line of the plant schedule: a plant style, or one species of plants without a style.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScheduleRow {
    pub style: Option<ResourceId>,
    pub name: String,
    pub id_code: String,
    pub botanical_name: String,
    pub common_name: String,
    pub category: PlantCategory,
    pub size: String,
    pub container: String,
    pub root: String,
    pub spread: f64,
    pub height: f64,
    pub count: usize,
    /// Average price of one plant, after overrides.
    pub unit_price: f64,
    pub total_price: f64,
    /// How the row is counted. For `area`, the price is per m2.
    pub quantity: QuantityType,
    /// Covered area in m2 (canopy circles of the plants), for rows counted by area. Zero otherwise.
    pub area_m2: f64,
    /// The effective style of a styled row. Plants of one style with different values (overrides)
    /// make different rows.
    #[serde(skip)]
    key: Option<PlantStyle>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Schedule {
    pub rows: Vec<ScheduleRow>,
    pub total_count: usize,
    pub total_price: f64,
    /// Total area of the rows counted by area, in m2.
    pub total_area_m2: f64,
}

/// The schedule of all plants in the document. Styled plants are grouped by style and by their
/// effective values, so plants with different overrides (price, size, container, root) get
/// their own rows. The others are grouped by species. Rows are sorted by name.
pub fn schedule(doc: &Document) -> Schedule {
    let mut rows: Vec<ScheduleRow> = Vec::new();
    for obj in doc.objects.values() {
        let Some(e) = link::effective(doc, obj) else {
            continue;
        };
        // Size variation does not split a row: plants of one style differ in spread and height only by chance.
        let key = PlantStyle {
            spread: 0.0,
            height: 0.0,
            ..e.style.clone()
        };
        let found = rows.iter_mut().find(|r| match e.style_id {
            Some(id) => r.style == Some(id) && r.key.as_ref() == Some(&key),
            None => r.style.is_none() && r.name == e.style_name,
        });
        let row = match found {
            Some(r) => r,
            None => {
                // The row shows the effective values of its plants.
                let base = &e.style;
                rows.push(ScheduleRow {
                    style: e.style_id,
                    name: e.style_name.clone(),
                    id_code: base.id_code.clone(),
                    botanical_name: base.botanical_name.clone(),
                    common_name: base.common_name.clone(),
                    category: base.category,
                    size: base.size.clone(),
                    container: base.container.clone(),
                    root: base.root.clone(),
                    spread: e
                        .style_id
                        .and_then(|id| doc.resources.plant_style(id))
                        .map_or(base.spread, |s| s.spread),
                    height: e
                        .style_id
                        .and_then(|id| doc.resources.plant_style(id))
                        .map_or(base.height, |s| s.height),
                    count: 0,
                    unit_price: 0.0,
                    total_price: 0.0,
                    quantity: base.quantity,
                    area_m2: 0.0,
                    key: e.style_id.map(|_| key.clone()),
                });
                rows.last_mut().unwrap()
            }
        };
        row.count += 1;
        if e.style.quantity == QuantityType::Area {
            let area = std::f64::consts::PI * (e.style.spread / 2.0).powi(2) / 1e6;
            row.area_m2 += area;
            row.total_price += e.style.price * area;
        } else {
            row.total_price += e.style.price;
        }
    }
    for r in &mut rows {
        r.unit_price = r.total_price / r.count as f64;
    }
    rows.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.style.cmp(&b.style))
            .then(a.size.cmp(&b.size))
            .then(a.container.cmp(&b.container))
            .then(a.root.cmp(&b.root))
            .then(a.unit_price.total_cmp(&b.unit_price))
    });
    let total_count = rows.iter().map(|r| r.count).sum();
    let total_price = rows.iter().map(|r| r.total_price).sum::<f64>() + 0.0;
    let total_area_m2 = rows.iter().map(|r| r.area_m2).sum::<f64>() + 0.0;
    Schedule {
        rows,
        total_count,
        total_price,
        total_area_m2,
    }
}

/// The schedule as CSV text, with a total line.
pub fn schedule_csv(s: &Schedule) -> String {
    let mut out = vec![
        [
            "Name",
            "ID",
            "Botanical name",
            "Common name",
            "Category",
            "Size",
            "Container",
            "Root",
            "Spread (mm)",
            "Height (mm)",
            "Count",
            "Unit price",
            "Total price",
            "Quantity",
            "Area (m2)",
        ]
        .iter()
        .map(|h| h.to_string())
        .collect::<Vec<_>>(),
    ];
    for r in &s.rows {
        out.push(vec![
            r.name.clone(),
            r.id_code.clone(),
            r.botanical_name.clone(),
            r.common_name.clone(),
            r.category.id().into(),
            r.size.clone(),
            r.container.clone(),
            r.root.clone(),
            r.spread.to_string(),
            r.height.to_string(),
            r.count.to_string(),
            format!("{:.2}", r.unit_price),
            format!("{:.2}", r.total_price),
            r.quantity.id().into(),
            if r.quantity == QuantityType::Area {
                format!("{:.2}", r.area_m2)
            } else {
                String::new()
            },
        ]);
    }
    let mut total = vec![String::new(); 15];
    total[0] = "Total".into();
    total[10] = s.total_count.to_string();
    total[12] = format!("{:.2}", s.total_price);
    if s.total_area_m2 > 0.0 {
        total[14] = format!("{:.2}", s.total_area_m2);
    }
    out.push(total);
    write_csv(&out)
}

/// One existing tree in the tree list.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TreeRow {
    pub id: ObjectId,
    pub species: String,
    pub dbh: f64,
    pub spread: f64,
    pub height: f64,
    pub condition: &'static str,
    pub action: &'static str,
    pub rpz_radius: f64,
}

/// All existing trees, in document order.
pub fn existing_trees(doc: &Document) -> Vec<TreeRow> {
    doc.objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::ExistingTree {
                species,
                dbh,
                spread,
                height,
                condition,
                action,
                rpz_factor,
                rpz_cap,
                ..
            } => Some(TreeRow {
                id: o.id,
                species: species.clone(),
                dbh: *dbh,
                spread: *spread,
                height: *height,
                condition: condition.id(),
                action: action.id(),
                rpz_radius: rpz_radius(*dbh, *rpz_factor, *rpz_cap),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::Attributes;
    use kadakas_core::model::DEFAULT_TAG_HEIGHT;
    use kadakas_core::plants::style::PlantStyle;
    use kadakas_core::plants::tag::*;
    use kadakas_core::resource::ResourceBody;

    fn setup() -> (Editor, ResourceId) {
        let mut ed = Editor::default();
        let style = PlantStyle {
            id_code: "AC".into(),
            botanical_name: "Acer".into(),
            price: 100.0,
            size: "16-18".into(),
            ..PlantStyle::default()
        };
        let out = ed
            .execute(&Command::AddResource {
                name: "Maple".into(),
                body: ResourceBody::plant_style(style),
            })
            .unwrap();
        (ed, ResourceId(out.created[0]))
    }

    fn put(
        ed: &mut Editor,
        style: Option<ResourceId>,
        overrides: serde_json::Map<String, serde_json::Value>,
        n: usize,
    ) {
        let positions = (0..n).map(|i| DVec2::new(i as f64 * 5000.0, 0.0)).collect();
        ed.execute(&Command::AddPlants {
            style,
            overrides,
            positions,
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
    }

    #[test]
    fn counts_and_prices_per_style_with_overrides() {
        let (mut ed, id) = setup();
        put(&mut ed, Some(id), Default::default(), 3);
        let mut o = serde_json::Map::new();
        o.insert("price".into(), 50.0.into());
        put(&mut ed, Some(id), o, 2);
        let s = schedule(ed.doc());
        // The two prices make two rows.
        assert_eq!(s.rows.len(), 2);
        let by_price: Vec<_> = s
            .rows
            .iter()
            .map(|r| (r.unit_price, r.count, r.total_price))
            .collect();
        assert_eq!(by_price, vec![(50.0, 2, 100.0), (100.0, 3, 300.0)]);
        assert_eq!((s.total_count, s.total_price), (5, 400.0));
        assert_eq!(s.rows[1].id_code, "AC");
        assert_eq!(s.rows[1].size, "16-18");
    }

    #[test]
    fn groundcovers_are_counted_by_area_and_priced_per_square_meter() {
        let mut ed = Editor::default();
        let style = PlantStyle {
            quantity: QuantityType::Area,
            spread: 1000.0,
            price: 20.0,
            id_code: "GC".into(),
            ..PlantStyle::default()
        };
        let id = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Thyme".into(),
                body: ResourceBody::plant_style(style),
            })
            .unwrap()
            .created[0],
        );
        put(&mut ed, Some(id), Default::default(), 4);
        let s = schedule(ed.doc());
        let one = std::f64::consts::PI * 0.25;
        assert_eq!(s.rows.len(), 1);
        assert!((s.rows[0].area_m2 - 4.0 * one).abs() < 1e-9);
        assert!((s.rows[0].total_price - 4.0 * one * 20.0).abs() < 1e-9);
        assert!((s.total_area_m2 - 4.0 * one).abs() < 1e-9);
        assert_eq!(
            (s.rows[0].count, s.total_count),
            (4, 4),
            "the plants are still counted"
        );
        let csv = schedule_csv(&s);
        let rows = kadakas_core::csv::parse(&csv).unwrap();
        assert_eq!(
            (
                rows[1][13].as_str(),
                rows[1][14].as_str(),
                rows[2][14].as_str()
            ),
            ("area", "3.14", "3.14")
        );
        // Count rows have no area.
        let (mut ed2, id2) = setup();
        put(&mut ed2, Some(id2), Default::default(), 2);
        let s2 = schedule(ed2.doc());
        assert_eq!((s2.rows[0].area_m2, s2.total_area_m2), (0.0, 0.0));
        assert_eq!(
            kadakas_core::csv::parse(&schedule_csv(&s2)).unwrap()[1][13],
            "count"
        );
    }

    #[test]
    fn size_variation_keeps_plants_in_one_row() {
        let (mut ed, id) = setup();
        let v = kadakas_core::plants::variation::Variation {
            seed: 5,
            size: 0.4,
            rotation: true,
        };
        ed.execute(&Command::AddPlants {
            style: Some(id),
            overrides: Default::default(),
            positions: (0..30)
                .map(|i| DVec2::new(i as f64 * 9000.0, 0.0))
                .collect(),
            layer: None,
            class: None,
            variation: Some(v),
        })
        .unwrap();
        let s = schedule(ed.doc());
        assert_eq!(
            (s.rows.len(), s.rows[0].count, s.total_price),
            (1, 30, 3000.0)
        );
        assert_eq!(s.rows[0].spread, 3000.0, "the row shows the style size");
    }

    #[test]
    fn plants_with_different_sizes_containers_and_roots_get_their_own_rows() {
        let (mut ed, id) = setup();
        for (field, value) in [
            ("size", "20-25"),
            ("container", "C30"),
            ("root", "bare root"),
        ] {
            let mut o = serde_json::Map::new();
            o.insert(field.into(), value.into());
            put(&mut ed, Some(id), o, 1);
        }
        put(&mut ed, Some(id), Default::default(), 2);
        let s = schedule(ed.doc());
        assert_eq!(s.rows.len(), 4);
        assert_eq!(s.total_count, 5);
        let plain = s
            .rows
            .iter()
            .find(|r| r.size == "16-18" && r.container.is_empty() && r.root.is_empty())
            .unwrap();
        assert_eq!(plain.count, 2);
        assert!(s.rows.iter().any(|r| r.size == "20-25" && r.count == 1));
        assert!(s.rows.iter().any(|r| r.container == "C30" && r.count == 1));
        assert!(s.rows.iter().any(|r| r.root == "bare root" && r.count == 1));
        let csv = schedule_csv(&s);
        assert!(csv.contains("C30") && csv.contains("bare root"));
    }

    #[test]
    fn unstyled_plants_group_by_species() {
        let (mut ed, id) = setup();
        put(&mut ed, Some(id), Default::default(), 1);
        for sp in ["Oak", "Oak", "Beech"] {
            ed.execute(&Command::AddObject {
                shape: Shape::Plant {
                    at: DVec2::ZERO,
                    species: sp.into(),
                    spread: 3000.0,
                    height: 5000.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        }
        let s = schedule(ed.doc());
        let names: Vec<_> = s
            .rows
            .iter()
            .map(|r| (r.name.as_str(), r.count, r.style.is_some()))
            .collect();
        assert_eq!(
            names,
            vec![("Beech", 1, false), ("Maple", 1, true), ("Oak", 2, false)]
        );
        assert_eq!(s.total_count, 4);
        assert_eq!(s.total_price, 100.0);
    }

    #[test]
    fn empty_schedule() {
        let s = schedule(&Document::default());
        assert!(s.rows.is_empty() && s.total_count == 0 && s.total_price == 0.0);
        let csv = schedule_csv(&s);
        assert_eq!(csv.lines().count(), 2);
    }

    #[test]
    fn csv_has_header_rows_and_total() {
        let (mut ed, id) = setup();
        put(&mut ed, Some(id), Default::default(), 2);
        let csv = schedule_csv(&schedule(ed.doc()));
        let rows = kadakas_core::csv::parse(&csv).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][0], "Name");
        assert_eq!(
            (
                rows[1][0].as_str(),
                rows[1][10].as_str(),
                rows[1][11].as_str(),
                rows[1][12].as_str()
            ),
            ("Maple", "2", "100.00", "200.00")
        );
        assert_eq!(
            (
                rows[2][0].as_str(),
                rows[2][10].as_str(),
                rows[2][12].as_str()
            ),
            ("Total", "2", "200.00")
        );
    }

    #[test]
    fn tag_shows_code_and_count_on_its_layer() {
        let (mut ed, id) = setup();
        put(&mut ed, Some(id), Default::default(), 4);
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::PlantTag {
                    at: DVec2::new(0.0, 3000.0),
                    anchor: DVec2::ZERO,
                    style: id,
                    height: 400.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let tag = ObjectId(out.created[0]);
        let info = tag_info(ed.doc(), tag).unwrap();
        assert_eq!((info.code.as_str(), info.count), ("AC", 4));
        assert_eq!(info.text(), "AC (4)");
        // A style without a code shows its name.
        ed.execute(&Command::ModifyResource {
            id,
            name: None,
            body: Some(ResourceBody::plant_style(PlantStyle {
                id_code: String::new(),
                ..PlantStyle::default()
            })),
        })
        .unwrap();
        assert_eq!(tag_info(ed.doc(), tag).unwrap().code, "Maple");
        // Not a tag, or missing.
        assert!(tag_info(ed.doc(), ObjectId::from_num(9999)).is_none());
        let plant = ed
            .doc()
            .objects
            .values()
            .find(|o| matches!(o.shape, Shape::Plant { .. }))
            .unwrap()
            .id;
        assert!(tag_info(ed.doc(), plant).is_none());
    }

    #[test]
    fn counts_agree_with_tag_info_on_many_tags() {
        let (mut ed, id) = setup();
        put(&mut ed, Some(id), Default::default(), 3);
        let mut tags = Vec::new();
        for _ in 0..5 {
            let out = ed
                .execute(&Command::AddObject {
                    shape: group_tag(id, &[DVec2::ZERO], 1000.0),
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap();
            tags.push(ObjectId(out.created[0]));
        }
        let counts = tag_counts(ed.doc());
        assert_eq!(counts.values().sum::<usize>(), 3);
        for t in tags {
            assert_eq!(tag_info_with(ed.doc(), t, &counts), tag_info(ed.doc(), t));
            assert_eq!(tag_info(ed.doc(), t).unwrap().count, 3);
        }
        assert!(tag_info_with(ed.doc(), ObjectId::from_num(1), &counts).is_none());
    }

    #[test]
    fn group_tag_sits_above_the_group() {
        let pts = [DVec2::new(0.0, 0.0), DVec2::new(4000.0, 2000.0)];
        let Shape::PlantTag {
            at,
            anchor,
            style,
            height,
        } = group_tag(ResourceId::from_num(5), &pts, 1000.0)
        else {
            panic!()
        };
        assert_eq!(
            (anchor, style, height),
            (
                DVec2::new(2000.0, 1000.0),
                ResourceId::from_num(5),
                DEFAULT_TAG_HEIGHT
            )
        );
        assert_eq!(at, DVec2::new(2000.0, 2000.0 + 500.0 + DEFAULT_TAG_HEIGHT));
        // No positions: the tag still has finite numbers.
        let Shape::PlantTag { at, anchor, .. } = group_tag(ResourceId::from_num(5), &[], 1000.0)
        else {
            panic!()
        };
        assert!(at.is_finite() && anchor.is_finite());
    }

    #[test]
    fn existing_tree_list() {
        let mut ed = Editor::default();
        ed.execute(&Command::AddObject {
            shape: Shape::ExistingTree {
                at: DVec2::ZERO,
                species: "Quercus".into(),
                dbh: 500.0,
                spread: 9000.0,
                height: 15000.0,
                condition: kadakas_core::plants::tree::TreeCondition::Good,
                action: kadakas_core::plants::tree::TreeAction::Remove,
                rpz_factor: None,
                rpz_cap: None,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        let rows = existing_trees(ed.doc());
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].condition, rows[0].action, rows[0].rpz_radius),
            ("good", "remove", 6000.0)
        );
    }

    fn info(levels: u8, bubble: bool) -> TagInfo {
        TagInfo {
            code: "AC".into(),
            count: 12,
            name: "Field maple".into(),
            details: "P9, C2".into(),
            format: TagFormat { levels, bubble },
        }
    }

    #[test]
    fn tag_lines_follow_the_levels_and_the_bubble() {
        assert_eq!(info(1, false).lines(), vec!["AC (12)"]);
        assert_eq!(info(2, false).lines(), vec!["AC (12)", "Field maple"]);
        assert_eq!(
            info(3, false).lines(),
            vec!["AC (12)", "Field maple", "P9, C2"]
        );
        assert_eq!(info(3, true).lines()[0], "(12)");
        // Empty fields leave their lines out.
        let mut bare = info(3, false);
        bare.name.clear();
        bare.details.clear();
        assert_eq!(bare.lines(), vec!["AC (12)"]);
        assert_eq!(info(1, false).text(), "AC (12)");
    }

    #[test]
    fn tag_layout_stacks_lines_and_places_the_bubble() {
        let h = 400.0;
        let plain = info(3, false).layout(h);
        assert!(plain.bubble.is_none());
        assert_eq!(plain.texts.len(), 3);
        // The first line is the highest, and every line starts at the label position.
        assert!(plain.texts[0].at.y > plain.texts[1].at.y);
        assert!(plain.texts.iter().all(|t| t.at.x == 0.0 && !t.centered));
        assert_eq!(plain.texts[2].at.y, 0.0);
        let bubble = info(2, true).layout(h);
        let (c, r) = bubble.bubble.unwrap();
        assert_eq!(c.x, r);
        assert!(bubble.texts[0].centered && bubble.texts[0].text == "AC");
        // The text lines start to the right of the bubble.
        assert!(bubble.texts[1..].iter().all(|t| t.at.x > 2.0 * r));
        // A longer code gives a bigger bubble.
        let mut long = info(1, true);
        long.code = "ACPLX".into();
        assert!(long.layout(h).bubble.unwrap().1 > r);
    }

    #[test]
    fn tag_format_comes_from_the_data_and_defaults_to_one_level() {
        let mut o = kadakas_core::model::Object::new(
            ObjectId::from_num(1),
            kadakas_core::model::FIRST_LAYER,
            kadakas_core::model::NONE_CLASS,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
        );
        assert_eq!(TagFormat::of(&o), TagFormat::default());
        o.data.insert(
            TAG_FORMAT_KEY.into(),
            serde_json::json!({"levels": 3, "bubble": true}),
        );
        assert_eq!(
            TagFormat::of(&o),
            TagFormat {
                levels: 3,
                bubble: true
            }
        );
        o.data
            .insert(TAG_FORMAT_KEY.into(), serde_json::json!({"levels": 99}));
        assert_eq!(TagFormat::of(&o).levels, 3);
        o.data.insert(
            TAG_FORMAT_KEY.into(),
            serde_json::json!({"levels": 0, "bubble": "x"}),
        );
        assert_eq!(TagFormat::of(&o), TagFormat::default());
    }
}
