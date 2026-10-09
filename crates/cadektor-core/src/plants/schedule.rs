//! Plant schedule, plant tags and the existing tree list. All are computed live from the document.

use serde::Serialize;

use super::catalog::write_csv;
use super::link;
use super::style::PlantCategory;
use super::tree::rpz_radius;
use crate::model::{DEFAULT_TAG_HEIGHT, Document, ObjectId, Shape};
use glam::DVec2;
use crate::resource::ResourceId;

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
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Schedule {
    pub rows: Vec<ScheduleRow>,
    pub total_count: usize,
    pub total_price: f64,
}

/// The schedule of all plants in the document. Styled plants are grouped by style, and the
/// others by species. Rows are sorted by name.
pub fn schedule(doc: &Document) -> Schedule {
    let mut rows: Vec<ScheduleRow> = Vec::new();
    for obj in doc.objects.values() {
        let Some(e) = link::effective(doc, obj) else { continue };
        let found = rows.iter_mut().find(|r| match e.style_id {
            Some(id) => r.style == Some(id),
            None => r.style.is_none() && r.name == e.style_name,
        });
        let row = match found {
            Some(r) => r,
            None => {
                // The row shows the style as it is, without the overrides of the first plant.
                let base = e.style_id.and_then(|id| doc.resources.plant_style(id)).unwrap_or(&e.style);
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
                    spread: base.spread,
                    height: base.height,
                    count: 0,
                    unit_price: 0.0,
                    total_price: 0.0,
                });
                rows.last_mut().unwrap()
            }
        };
        row.count += 1;
        row.total_price += e.style.price;
    }
    for r in &mut rows {
        r.unit_price = r.total_price / r.count as f64;
    }
    rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.style.cmp(&b.style)));
    let total_count = rows.iter().map(|r| r.count).sum();
    let total_price = rows.iter().map(|r| r.total_price).sum::<f64>() + 0.0;
    Schedule { rows, total_count, total_price }
}

/// The schedule as CSV text, with a total line.
pub fn schedule_csv(s: &Schedule) -> String {
    let mut out = vec![[
        "Name", "ID", "Botanical name", "Common name", "Category", "Size", "Container", "Root", "Spread (mm)", "Height (mm)", "Count",
        "Unit price", "Total price",
    ]
    .iter()
    .map(|h| h.to_string())
    .collect::<Vec<_>>()];
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
        ]);
    }
    let mut total = vec![String::new(); 13];
    total[0] = "Total".into();
    total[10] = s.total_count.to_string();
    total[12] = format!("{:.2}", s.total_price);
    out.push(total);
    write_csv(&out)
}

/// A plant tag for a group of plants placed together: the leader ends at the middle of the group
/// and the label sits above it. `spread` is the canopy diameter of the plants.
pub fn group_tag(style: ResourceId, positions: &[DVec2], spread: f64) -> Shape {
    let n = positions.len().max(1) as f64;
    let centroid = positions.iter().fold(DVec2::ZERO, |a, p| a + *p) / n;
    let top = positions.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    let top = if top.is_finite() { top } else { 0.0 };
    let label = DVec2::new(centroid.x, top + spread / 2.0 + DEFAULT_TAG_HEIGHT);
    Shape::PlantTag { at: label, anchor: centroid, style, height: DEFAULT_TAG_HEIGHT }
}

/// What a plant tag shows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TagInfo {
    /// The ID code of the style, or the style name if it has no code.
    pub code: String,
    /// Plants of this style on the layer of the tag.
    pub count: usize,
}

impl TagInfo {
    /// The tag text, for example `AC (12)`.
    pub fn text(&self) -> String {
        format!("{} ({})", self.code, self.count)
    }
}

/// The ID code and count for a tag object. `None` if the object is not a tag or its style is gone.
pub fn tag_info(doc: &Document, tag: ObjectId) -> Option<TagInfo> {
    let obj = doc.object(tag)?;
    let Shape::PlantTag { style, .. } = &obj.shape else { return None };
    let res = doc.resources.get(*style)?;
    let s = doc.resources.plant_style(*style)?;
    let code = if s.id_code.trim().is_empty() { res.name.clone() } else { s.id_code.clone() };
    let count = doc.objects.values().filter(|o| o.layer == obj.layer && link::link(o).is_some_and(|l| l.style == *style)).count();
    Some(TagInfo { code, count })
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
            Shape::ExistingTree { species, dbh, spread, height, condition, action, rpz_factor, .. } => Some(TreeRow {
                id: o.id,
                species: species.clone(),
                dbh: *dbh,
                spread: *spread,
                height: *height,
                condition: condition.id(),
                action: action.id(),
                rpz_radius: rpz_radius(*dbh, *rpz_factor),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::model::Attributes;
    use crate::plants::style::PlantStyle;
    use crate::resource::ResourceBody;
    use glam::DVec2;

    fn setup() -> (Editor, ResourceId) {
        let mut ed = Editor::default();
        let style = PlantStyle { id_code: "AC".into(), botanical_name: "Acer".into(), price: 100.0, size: "16-18".into(), ..PlantStyle::default() };
        let out = ed.execute(&Command::AddResource { name: "Maple".into(), body: ResourceBody::plant_style(style) }).unwrap();
        (ed, ResourceId(out.created[0]))
    }

    fn put(ed: &mut Editor, style: Option<ResourceId>, overrides: serde_json::Map<String, serde_json::Value>, n: usize) {
        let positions = (0..n).map(|i| DVec2::new(i as f64 * 5000.0, 0.0)).collect();
        ed.execute(&Command::AddPlants { style, overrides, positions, layer: None, class: None }).unwrap();
    }

    #[test]
    fn counts_and_prices_per_style_with_overrides() {
        let (mut ed, id) = setup();
        put(&mut ed, Some(id), Default::default(), 3);
        let mut o = serde_json::Map::new();
        o.insert("price".into(), 50.0.into());
        put(&mut ed, Some(id), o, 2);
        let s = schedule(ed.doc());
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0].count, 5);
        assert_eq!(s.rows[0].total_price, 400.0);
        assert_eq!(s.rows[0].unit_price, 80.0);
        assert_eq!((s.total_count, s.total_price), (5, 400.0));
        assert_eq!(s.rows[0].id_code, "AC");
        assert_eq!(s.rows[0].size, "16-18");
    }

    #[test]
    fn unstyled_plants_group_by_species() {
        let (mut ed, id) = setup();
        put(&mut ed, Some(id), Default::default(), 1);
        for sp in ["Oak", "Oak", "Beech"] {
            ed.execute(&Command::AddObject {
                shape: Shape::Plant { at: DVec2::ZERO, species: sp.into(), spread: 3000.0, height: 5000.0 },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        }
        let s = schedule(ed.doc());
        let names: Vec<_> = s.rows.iter().map(|r| (r.name.as_str(), r.count, r.style.is_some())).collect();
        assert_eq!(names, vec![("Beech", 1, false), ("Maple", 1, true), ("Oak", 2, false)]);
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
        let rows = crate::plants::catalog::parse_csv(&csv).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][0], "Name");
        assert_eq!((rows[1][0].as_str(), rows[1][10].as_str(), rows[1][11].as_str(), rows[1][12].as_str()), ("Maple", "2", "100.00", "200.00"));
        assert_eq!((rows[2][0].as_str(), rows[2][10].as_str(), rows[2][12].as_str()), ("Total", "2", "200.00"));
    }

    #[test]
    fn tag_shows_code_and_count_on_its_layer() {
        let (mut ed, id) = setup();
        put(&mut ed, Some(id), Default::default(), 4);
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::PlantTag { at: DVec2::new(0.0, 3000.0), anchor: DVec2::ZERO, style: id, height: 400.0 },
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
            body: Some(ResourceBody::plant_style(PlantStyle { id_code: String::new(), ..PlantStyle::default() })),
        })
        .unwrap();
        assert_eq!(tag_info(ed.doc(), tag).unwrap().code, "Maple");
        // Not a tag, or missing.
        assert!(tag_info(ed.doc(), ObjectId(9999)).is_none());
        let plant = ed.doc().objects.values().find(|o| matches!(o.shape, Shape::Plant { .. })).unwrap().id;
        assert!(tag_info(ed.doc(), plant).is_none());
    }

    #[test]
    fn group_tag_sits_above_the_group() {
        let pts = [DVec2::new(0.0, 0.0), DVec2::new(4000.0, 2000.0)];
        let Shape::PlantTag { at, anchor, style, height } = group_tag(ResourceId(5), &pts, 1000.0) else { panic!() };
        assert_eq!((anchor, style, height), (DVec2::new(2000.0, 1000.0), ResourceId(5), DEFAULT_TAG_HEIGHT));
        assert_eq!(at, DVec2::new(2000.0, 2000.0 + 500.0 + DEFAULT_TAG_HEIGHT));
        // No positions: the tag still has finite numbers.
        let Shape::PlantTag { at, anchor, .. } = group_tag(ResourceId(5), &[], 1000.0) else { panic!() };
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
                condition: crate::plants::tree::TreeCondition::Good,
                action: crate::plants::tree::TreeAction::Remove,
                rpz_factor: None,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        let rows = existing_trees(ed.doc());
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].condition, rows[0].action, rows[0].rpz_radius), ("good", "remove", 6000.0));
    }
}
