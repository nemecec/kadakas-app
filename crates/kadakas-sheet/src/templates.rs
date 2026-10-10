//! Built-in report templates. Each one is a worksheet definition that the user can change.

use crate::def::{ColumnDef, DatabaseDef, GroupMode, RowSource, SortKey, Summary, WorksheetDef};

/// A template for the list of templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

pub const TEMPLATES: &[TemplateInfo] = &[
    TemplateInfo {
        id: "plant_list",
        title: "Plant list",
        description: "One row per plant style with botanical name, size, count and cost, and totals.",
    },
    TemplateInfo {
        id: "hardscape_quantities",
        title: "Hardscape quantities",
        description: "Count, area and volume per material, and totals.",
    },
    TemplateInfo {
        id: "area_schedule",
        title: "Area schedule",
        description: "Landscape areas and hardscapes with area and perimeter, in groups with subtotals.",
    },
    TemplateInfo {
        id: "tree_survey",
        title: "Existing tree survey",
        description: "Existing trees with trunk diameter, spread, height, condition, action and root protection radius.",
    },
    TemplateInfo {
        id: "irrigation_heads",
        title: "Irrigation heads",
        description: "Irrigation outlets per model and zone, with the count.",
    },
    TemplateInfo {
        id: "irrigation_pipes",
        title: "Irrigation pipes",
        description: "Irrigation lines per type, material and size: count and length in m.",
    },
];

fn col(header: &str, expr: &str) -> ColumnDef {
    ColumnDef {
        header: header.into(),
        expr: expr.into(),
        ..ColumnDef::default()
    }
}

fn sum(header: &str, expr: &str, decimals: Option<u8>, unit: &str) -> ColumnDef {
    ColumnDef {
        header: header.into(),
        expr: expr.into(),
        summary: Some(Summary::Sum),
        decimals,
        unit: unit.into(),
    }
}

fn dec(mut c: ColumnDef, decimals: u8, unit: &str) -> ColumnDef {
    c.decimals = Some(decimals);
    c.unit = unit.into();
    c
}

/// The worksheet of a template, or `None` for an unknown ID.
pub fn template(id: &str) -> Option<WorksheetDef> {
    let db = match id {
        "plant_list" => DatabaseDef {
            name: "Plant list".into(),
            // The plant list of the document: single plants and the plants of landscape area mixes.
            source: RowSource::PlantList,
            criteria: String::new(),
            columns: vec![
                col("Style", "Style"),
                col("Code", "IdCode"),
                col("Botanical name", "Botanical"),
                col("Common name", "CommonName"),
                col("Size", "Size"),
                col("Container", "Container"),
                sum("Count", "Count", None, ""),
                dec(col("Unit price", "Price"), 2, ""),
                sum("Cost", "Cost", Some(2), ""),
            ],
            group_by: Some(0),
            group_mode: GroupMode::Summary,
            ..DatabaseDef::default()
        },
        "hardscape_quantities" => DatabaseDef {
            name: "Hardscape quantities".into(),
            criteria: "Kind = \"hardscape\"".into(),
            columns: vec![
                col("Material", "Material"),
                sum("Count", "Count", None, ""),
                sum("Area", "Area", Some(2), "m2"),
                dec(col("Thickness", "Thickness"), 0, "mm"),
                sum("Volume", "Volume", Some(2), "m3"),
            ],
            group_by: Some(0),
            group_mode: GroupMode::Summary,
            ..DatabaseDef::default()
        },
        "area_schedule" => DatabaseDef {
            name: "Area schedule".into(),
            criteria: "Kind = \"landscape_area\" OR Kind = \"hardscape\"".into(),
            columns: vec![
                col("Kind", "Kind"),
                col("Name", "Style"),
                col("Layer", "Layer"),
                sum("Area", "Area", Some(2), "m2"),
                dec(col("Perimeter", "Perimeter"), 2, "m"),
            ],
            sort: vec![SortKey {
                column: 1,
                descending: false,
            }],
            group_by: Some(0),
            group_mode: GroupMode::Detail,
            ..DatabaseDef::default()
        },
        "tree_survey" => DatabaseDef {
            name: "Existing tree survey".into(),
            criteria: "Kind = \"existing_tree\"".into(),
            columns: vec![
                col("Tree", "Id"),
                col("Species", "Species"),
                dec(col("Trunk diameter", "DBH"), 0, "mm"),
                dec(col("Spread", "Spread / 1000"), 1, "m"),
                dec(col("Height", "Height / 1000"), 1, "m"),
                col("Condition", "Condition"),
                col("Action", "Action"),
                dec(col("Root protection radius", "RPZ / 1000"), 1, "m"),
            ],
            sort: vec![SortKey {
                column: 0,
                descending: false,
            }],
            ..DatabaseDef::default()
        },
        "irrigation_heads" => DatabaseDef {
            name: "Irrigation heads".into(),
            criteria: "Kind = \"irrigation_outlet\"".into(),
            // One row per model and zone: the first column is the key to group by.
            columns: vec![
                col("Model and zone", "Model & \" / zone \" & Zone"),
                col("Model", "Model"),
                col("Zone", "Zone"),
                sum("Count", "Count", None, ""),
            ],
            group_by: Some(0),
            group_mode: GroupMode::Summary,
            ..DatabaseDef::default()
        },
        "irrigation_pipes" => DatabaseDef {
            name: "Irrigation pipes".into(),
            criteria: "Kind = \"irrigation_line\"".into(),
            columns: vec![
                col("Type", "LineType"),
                col("Material", "Material"),
                col("Size", "Size"),
                sum("Lines", "Count", None, ""),
                sum("Length", "Length", Some(2), "m"),
            ],
            group_by: Some(0),
            group_mode: GroupMode::Detail,
            ..DatabaseDef::default()
        },
        _ => return None,
    };
    let mut w = WorksheetDef::default();
    w.databases.push(db);
    Some(w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::compute;
    use crate::value::Value;
    use kadakas_core::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::curve::corners;
    use kadakas_core::model::{Attributes, Shape};
    use kadakas_core::plants::style::PlantStyle;
    use kadakas_core::resource::ResourceBody;

    fn add(ed: &mut Editor, shape: Shape) {
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    }

    fn scene() -> Editor {
        let mut ed = Editor::default();
        let sq = |s: f64| {
            corners(&[
                DVec2::ZERO,
                DVec2::new(s, 0.0),
                DVec2::new(s, s),
                DVec2::new(0.0, s),
            ])
        };
        add(
            &mut ed,
            Shape::Hardscape {
                boundary: sq(2000.0),
                material: "Granite".into(),
                thickness: Some(100.0),
                elevation: None,
            },
        );
        add(
            &mut ed,
            Shape::Hardscape {
                boundary: sq(1000.0),
                material: "Granite".into(),
                thickness: Some(100.0),
                elevation: None,
            },
        );
        add(
            &mut ed,
            Shape::LandscapeArea {
                boundary: sq(3000.0),
                name: "Lawn".into(),
            },
        );
        add(
            &mut ed,
            Shape::ExistingTree {
                at: DVec2::ZERO,
                species: "Oak".into(),
                dbh: 600.0,
                spread: 9000.0,
                height: 18000.0,
                condition: Default::default(),
                action: Default::default(),
                rpz_factor: None,
                rpz_cap: None,
            },
        );
        add(
            &mut ed,
            Shape::ExistingTree {
                at: DVec2::new(10.0, 0.0),
                species: "Ash".into(),
                dbh: 300.0,
                spread: 5000.0,
                height: 9000.0,
                condition: Default::default(),
                action: Default::default(),
                rpz_factor: None,
                rpz_cap: None,
            },
        );
        let sid = ed
            .execute(&Command::AddResource {
                name: "Maple".into(),
                body: ResourceBody::plant_style(PlantStyle {
                    botanical_name: "Acer".into(),
                    id_code: "AC".into(),
                    price: 50.0,
                    ..PlantStyle::default()
                }),
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::AddPlants {
            style: Some(kadakas_core::resource::ResourceId(sid)),
            overrides: Default::default(),
            positions: vec![
                DVec2::ZERO,
                DVec2::new(5000.0, 0.0),
                DVec2::new(9000.0, 0.0),
            ],
            layer: None,
            class: None,
            variation: None,
        })
        .unwrap();
        ed
    }

    #[test]
    fn all_templates_are_valid_and_listed() {
        assert_eq!(TEMPLATES.len(), 6);
        for t in TEMPLATES {
            let w = template(t.id).unwrap();
            w.validate().unwrap();
            assert_eq!(w.databases.len(), 1);
            assert!(!t.title.is_empty() && !t.description.is_empty());
            // Empty document: only header (and a total row where it applies).
            let c = compute(&w, &Default::default());
            assert!(c.problems.is_empty(), "{}: {:?}", t.id, c.problems);
        }
        assert!(template("nope").is_none());
    }

    #[test]
    fn plant_list() {
        let ed = scene();
        let c = compute(&template("plant_list").unwrap(), ed.doc());
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.grid()[0][0], "Style");
        assert_eq!(c.grid()[1][..3], ["Maple", "AC", "Acer"]);
        assert_eq!(c.value(2, 7), Value::Number(3.0));
        assert_eq!(c.display(2, 9), "150.00");
        assert_eq!(c.display(3, 1), "Total");
        assert_eq!(c.value(3, 7), Value::Number(3.0));
        assert_eq!(c.value(3, 9), Value::Number(150.0));
    }

    #[test]
    fn plant_list_includes_landscape_area_mixes_like_the_report() {
        use kadakas_core::landscape::LandscapePatch;
        use kadakas_core::landscape::style::{LandscapeAreaStyle, MixEntry};
        use kadakas_plants::plant_list::plant_list;
        let mut ed = scene();
        let maple = ed.doc().resources.find_plant_style("Maple").unwrap();
        let sedge = kadakas_core::resource::ResourceId(
            ed.execute(&Command::AddResource {
                name: "Sedge".into(),
                body: ResourceBody::plant_style(PlantStyle {
                    botanical_name: "Carex".into(),
                    id_code: "CX".into(),
                    price: 4.0,
                    spread: 500.0,
                    ..PlantStyle::default()
                }),
            })
            .unwrap()
            .created[0],
        );
        let mix = LandscapeAreaStyle {
            mix: vec![
                MixEntry {
                    plant: sedge,
                    percent: 70.0,
                    ..Default::default()
                },
                MixEntry {
                    plant: maple,
                    percent: 30.0,
                    min_count: 2,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let style = kadakas_core::resource::ResourceId(
            ed.execute(&Command::AddResource {
                name: "Meadow".into(),
                body: ResourceBody::LandscapeAreaStyle(Box::new(mix)),
            })
            .unwrap()
            .created[0],
        );
        let b = corners(&[
            DVec2::ZERO,
            DVec2::new(5000.0, 0.0),
            DVec2::new(5000.0, 4000.0),
            DVec2::new(0.0, 4000.0),
        ]);
        ed.execute(&Command::AddLandscapeArea {
            boundary: b,
            name: "Meadow".into(),
            patch: Box::new(LandscapePatch {
                style: Some(style),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        let report = plant_list(ed.doc());
        assert!(report.total_in_areas > 0);
        let c = compute(&template("plant_list").unwrap(), ed.doc());
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        let rows = report.rows.len() as u32;
        assert_eq!(c.rows, rows + 2, "header, a row per style, total");
        for (i, r) in report.rows.iter().enumerate() {
            let row = i as u32 + 2;
            assert_eq!(c.display(row, 1), r.name);
            assert_eq!(c.value(row, 7), Value::Number(r.count as f64), "{}", r.name);
            assert!(
                (match c.value(row, 9) {
                    Value::Number(n) => n,
                    other => panic!("{other:?}"),
                } - r.total_price)
                    .abs()
                    < 1e-9
            );
        }
        assert_eq!(
            c.value(rows + 2, 7),
            Value::Number(report.total_count as f64)
        );
        assert_eq!(c.value(rows + 2, 9), Value::Number(report.total_price));
        // The columns of the list can be used with criteria.
        let mut w = WorksheetDef::default();
        let mut db = template("plant_list").unwrap().databases.remove(0);
        db.criteria = "InAreas > 0 AND Category = \"tree\" OR Style = \"Sedge\"".into();
        w.databases.push(db);
        let c = compute(&w, ed.doc());
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert!(c.blocks[0].matched >= 1);
    }

    #[test]
    fn hardscape_quantities() {
        let ed = scene();
        let c = compute(&template("hardscape_quantities").unwrap(), ed.doc());
        assert_eq!(
            c.grid()[1],
            ["Granite", "2", "5.00 m2", "100 mm", "0.50 m3"]
        );
        assert_eq!(c.display(3, 3), "5.00 m2");
        assert_eq!(c.value(3, 5), Value::Number(0.5));
    }

    #[test]
    fn area_schedule_has_subtotals() {
        let ed = scene();
        let c = compute(&template("area_schedule").unwrap(), ed.doc());
        let first_col: Vec<String> = (1..=c.rows).map(|r| c.display(r, 1)).collect();
        assert_eq!(
            first_col,
            [
                "Kind",
                "hardscape",
                "hardscape",
                "hardscape total",
                "landscape_area",
                "landscape_area total",
                "Total"
            ]
        );
        assert_eq!(c.value(c.rows, 4), Value::Number(14.0));
        assert_eq!(c.display(5, 5), "12.00 m");
    }

    #[test]
    fn tree_survey() {
        let ed = scene();
        let c = compute(&template("tree_survey").unwrap(), ed.doc());
        assert_eq!(c.rows, 3);
        assert_eq!(
            c.grid()[1][1..],
            [
                "Oak", "600 mm", "9.0 m", "18.0 m", "fair", "retain", "7.2 m"
            ]
        );
        assert_eq!(c.grid()[2][1], "Ash");
    }
    #[test]
    fn irrigation_templates_list_heads_and_pipes() {
        use kadakas_core::irrigation::{IrrigationShape, LineType};
        let mut ed = Editor::default();
        let mut irr = |s: IrrigationShape| add(&mut ed, Shape::Irrigation(Box::new(s)));
        let outlet = |x: f64, model: &str, zone: Option<u32>| IrrigationShape::Outlet {
            at: DVec2::new(x, 0.0),
            model: model.into(),
            arc: 360.0,
            direction: 0.0,
            radius: None,
            zone,
        };
        irr(outlet(0.0, "Spray-4.0", Some(1)));
        irr(outlet(1.0, "Spray-4.0", Some(1)));
        irr(outlet(2.0, "Rotor-8", Some(2)));
        irr(outlet(3.0, "Spray-4.0", Some(3)));
        let line = |len: f64, t: LineType, size: &str| IrrigationShape::Line {
            points: vec![DVec2::ZERO, DVec2::new(len, 0.0)],
            line_type: t,
            size: size.into(),
            material: "PE".into(),
            emitter_spacing: None,
            emitter_flow: None,
        };
        irr(line(20_000.0, LineType::Mainline, "50"));
        irr(line(5_000.0, LineType::Lateral, "32"));
        irr(line(7_500.0, LineType::Lateral, "32"));
        let c = compute(&template("irrigation_heads").unwrap(), ed.doc());
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.grid()[1][..3], ["Rotor-8 / zone 2", "Rotor-8", "2"]);
        assert_eq!(c.value(2, 4), Value::Number(1.0));
        assert_eq!(c.grid()[2][..3], ["Spray-4.0 / zone 1", "Spray-4.0", "1"]);
        assert_eq!(c.value(3, 4), Value::Number(2.0));
        assert_eq!(c.grid()[3][..3], ["Spray-4.0 / zone 3", "Spray-4.0", "3"]);
        assert_eq!(c.value(c.rows, 4), Value::Number(4.0));
        let c = compute(&template("irrigation_pipes").unwrap(), ed.doc());
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.grid()[1][..3], ["lateral", "PE", "32"]);
        assert_eq!(c.value(c.rows, 5), Value::Number(32.5));
    }
}
