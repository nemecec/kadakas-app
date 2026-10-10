//! The plant list: single plants and the plants of landscape areas in one report.

use serde::Serialize;

use kadakas_core::csv::write_rows as write_csv;
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::plants::style::{PlantCategory, QuantityType};
use kadakas_core::resource::ResourceId;

use crate::schedule::schedule;

/// The share of one landscape area in a row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AreaShare {
    pub area: ObjectId,
    pub name: String,
    pub count: usize,
    pub area_m2: f64,
}

/// One line of the plant list: a plant style (all its plants and areas together), or a species
/// of plants without a style.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlantListRow {
    pub style: Option<ResourceId>,
    pub name: String,
    pub id_code: String,
    pub botanical_name: String,
    pub common_name: String,
    pub category: PlantCategory,
    pub size: String,
    pub container: String,
    pub root: String,
    /// Single plants in the drawing.
    pub plants: usize,
    /// Plants counted from landscape areas.
    pub in_areas: usize,
    pub count: usize,
    pub quantity: QuantityType,
    /// Covered area in m2, for rows counted by area. Zero otherwise.
    pub area_m2: f64,
    pub unit_price: f64,
    pub total_price: f64,
    pub areas: Vec<AreaShare>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PlantList {
    pub rows: Vec<PlantListRow>,
    pub total_count: usize,
    pub total_plants: usize,
    pub total_in_areas: usize,
    pub total_price: f64,
    pub total_area_m2: f64,
}

/// The plant list of the document. Single plants come from the plant schedule. Plants of
/// landscape areas are added to the row of their style. Plants of one style with different
/// overrides share one row here. The rows are sorted by name.
pub fn plant_list(doc: &Document) -> PlantList {
    let mut rows: Vec<PlantListRow> = Vec::new();
    for r in schedule(doc).rows {
        let existing = r
            .style
            .and_then(|id| rows.iter().position(|x| x.style == Some(id)));
        match existing {
            Some(i) => {
                let x = &mut rows[i];
                x.plants += r.count;
                x.count += r.count;
                x.area_m2 += r.area_m2;
                x.total_price += r.total_price;
            }
            None => rows.push(PlantListRow {
                style: r.style,
                name: r.name,
                id_code: r.id_code,
                botanical_name: r.botanical_name,
                common_name: r.common_name,
                category: r.category,
                size: r.size,
                container: r.container,
                root: r.root,
                plants: r.count,
                in_areas: 0,
                count: r.count,
                quantity: r.quantity,
                area_m2: r.area_m2,
                unit_price: 0.0,
                total_price: r.total_price,
                areas: Vec::new(),
            }),
        }
    }
    for obj in doc.objects.values() {
        let Shape::LandscapeArea { name, .. } = &obj.shape else {
            continue;
        };
        let Some(q) = kadakas_core::landscape::quantities(doc, obj) else {
            continue;
        };
        let area_name = if name.trim().is_empty() {
            format!("Landscape area {}", obj.id)
        } else {
            name.clone()
        };
        for p in q.rows {
            let i = match rows.iter().position(|x| x.style == Some(p.plant)) {
                Some(i) => i,
                None => {
                    let Some(s) = doc.resources.plant_style(p.plant) else {
                        continue;
                    };
                    rows.push(PlantListRow {
                        style: Some(p.plant),
                        name: p.name.clone(),
                        id_code: s.id_code.clone(),
                        botanical_name: s.botanical_name.clone(),
                        common_name: s.common_name.clone(),
                        category: s.category,
                        size: s.size.clone(),
                        container: s.container.clone(),
                        root: s.root.clone(),
                        plants: 0,
                        in_areas: 0,
                        count: 0,
                        quantity: s.quantity,
                        area_m2: 0.0,
                        unit_price: 0.0,
                        total_price: 0.0,
                        areas: Vec::new(),
                    });
                    rows.len() - 1
                }
            };
            let x = &mut rows[i];
            x.in_areas += p.count;
            x.count += p.count;
            x.total_price += p.total_price;
            if p.quantity == QuantityType::Area {
                x.area_m2 += p.covered_area_m2;
            }
            x.areas.push(AreaShare {
                area: obj.id,
                name: area_name.clone(),
                count: p.count,
                area_m2: p.covered_area_m2,
            });
        }
    }
    for r in &mut rows {
        r.unit_price = if r.count == 0 {
            0.0
        } else {
            r.total_price / r.count as f64
        };
    }
    rows.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.style.cmp(&b.style))
    });
    PlantList {
        total_count: rows.iter().map(|r| r.count).sum(),
        total_plants: rows.iter().map(|r| r.plants).sum(),
        total_in_areas: rows.iter().map(|r| r.in_areas).sum(),
        total_price: rows.iter().map(|r| r.total_price).sum::<f64>() + 0.0,
        total_area_m2: rows.iter().map(|r| r.area_m2).sum::<f64>() + 0.0,
        rows,
    }
}

/// The plant list as CSV text, with a total line.
pub fn plant_list_csv(list: &PlantList) -> String {
    let head = [
        "Name",
        "ID",
        "Botanical name",
        "Common name",
        "Category",
        "Size",
        "Container",
        "Root",
        "Plants",
        "In areas",
        "Count",
        "Unit price",
        "Total price",
        "Quantity",
        "Area (m2)",
    ];
    let mut out = vec![head.iter().map(|h| h.to_string()).collect::<Vec<_>>()];
    for r in &list.rows {
        out.push(vec![
            r.name.clone(),
            r.id_code.clone(),
            r.botanical_name.clone(),
            r.common_name.clone(),
            r.category.id().into(),
            r.size.clone(),
            r.container.clone(),
            r.root.clone(),
            r.plants.to_string(),
            r.in_areas.to_string(),
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
    let mut total = vec![String::new(); head.len()];
    total[0] = "Total".into();
    total[8] = list.total_plants.to_string();
    total[9] = list.total_in_areas.to_string();
    total[10] = list.total_count.to_string();
    total[12] = format!("{:.2}", list.total_price);
    if list.total_area_m2 > 0.0 {
        total[14] = format!("{:.2}", list.total_area_m2);
    }
    out.push(total);
    write_csv(&out)
}
