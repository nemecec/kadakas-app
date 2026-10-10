//! Raised planter: an example plug-in for Kadakas.
//!
//! Place it with two corners (a rectangle object). The plan view shows the wall, the soil and,
//! if you ask, a label with the height. The module has no imports and uses no files or network.

use kadakas_plugin_abi::{
    Geometry, Manifest, ObjectSpec, OutShape, ParamSchema, ParamType, ResetRequest, ShapeData,
    ShapeType, color_rgba, export_plugin,
};
use kadakas_plugin_abi::serde_json::json;

fn manifest() -> Manifest {
    let mut corners = ParamSchema::new("corners", ParamType::Choice).with_label("Corners");
    corners.options = vec!["Square".into(), "Cut".into()];
    let mut m = Manifest::object(
        "org.kadakas.raised-planter",
        "Raised planter",
        "1.0.0",
        ObjectSpec {
            shape_type: ShapeType::Rectangle,
            params: vec![
                ParamSchema::new("wall", ParamType::Length)
                    .with_label("Wall thickness")
                    .with_default(json!(150))
                    .with_range(50.0, 500.0),
                ParamSchema::new("height", ParamType::Length)
                    .with_label("Height")
                    .with_default(json!(450))
                    .with_range(100.0, 1500.0),
                corners,
                ParamSchema::new("wall_color", ParamType::Color)
                    .with_label("Wall color")
                    .with_default(json!("#6b4f3a")),
                ParamSchema::new("soil_color", ParamType::Color)
                    .with_label("Soil color")
                    .with_default(json!("#8a6a45")),
                ParamSchema::new("label", ParamType::Bool)
                    .with_label("Height label")
                    .with_default(json!(true)),
            ],
        },
    );
    m.description = "A raised planter. Place it with two corners. The plan shows the wall and the soil.".into();
    m
}

/// The outline of a rectangle. With `cut`, each corner is cut off by `c` mm.
fn outline(min: [f64; 2], max: [f64; 2], cut: f64) -> Vec<[f64; 2]> {
    if cut <= 0.0 {
        return vec![min, [max[0], min[1]], max, [min[0], max[1]]];
    }
    vec![
        [min[0] + cut, min[1]],
        [max[0] - cut, min[1]],
        [max[0], min[1] + cut],
        [max[0], max[1] - cut],
        [max[0] - cut, max[1]],
        [min[0] + cut, max[1]],
        [min[0], max[1] - cut],
        [min[0], min[1] + cut],
    ]
}

fn make_shapes(request: ResetRequest) -> Result<Vec<OutShape>, String> {
    let Geometry::Rectangle { min, max } = request.geometry else {
        return Err("a raised planter needs a rectangle".into());
    };
    let number = |k: &str| request.params.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let text = |k: &str| request.params.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let wall = number("wall");
    let (w, d) = (max[0] - min[0], max[1] - min[1]);
    if w <= 2.0 * wall + 1.0 || d <= 2.0 * wall + 1.0 {
        return Err(format!(
            "the planter ({w:.0} by {d:.0} mm) is too small for walls of {wall:.0} mm"
        ));
    }
    let cut = if text("corners") == "Cut" {
        (wall * 2.0).min(w.min(d) / 4.0)
    } else {
        0.0
    };
    let wall_color = color_rgba(text("wall_color"));
    let soil_color = color_rgba(text("soil_color"));
    let inner_min = [min[0] + wall, min[1] + wall];
    let inner_max = [max[0] - wall, max[1] - wall];
    let mut shapes = vec![
        OutShape::new(ShapeData::Polyline {
            points: outline(min, max, cut),
            closed: true,
        })
        .colored(wall_color, wall_color),
        OutShape::new(ShapeData::Polyline {
            points: outline(inner_min, inner_max, (cut - wall).max(0.0)),
            closed: true,
        })
        .colored(wall_color, soil_color),
    ];
    if request.params.get("label").and_then(|v| v.as_bool()) == Some(true) {
        shapes.push(OutShape::new(ShapeData::Text {
            at: [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0],
            text: format!("H {:.0}", number("height")),
            height: (d.min(w) / 5.0).clamp(40.0, 150.0),
        }));
    }
    Ok(shapes)
}

export_plugin!(describe = manifest, reset = make_shapes);
