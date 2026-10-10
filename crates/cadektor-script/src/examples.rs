//! Plug-in objects that ship with the app. Their scripts are trusted: they run even when agents
//! may not run their own scripts. The scripts live in `python/examples/plugins`.

use serde_json::json;

use crate::plugin::{ParamDef, ParamType, PluginDef, ShapeType};

const BENCH: &str = include_str!("../../../python/examples/plugins/bench.py");
const STEPPING_STONES: &str = include_str!("../../../python/examples/plugins/stepping_stones.py");

fn param(name: &str, label: &str, ty: ParamType, default: serde_json::Value) -> ParamDef {
    ParamDef {
        name: name.into(),
        label: label.into(),
        ty,
        default,
        min: None,
        max: None,
        options: vec![],
        fields: vec![],
    }
}

fn ranged(mut p: ParamDef, min: f64, max: f64) -> ParamDef {
    p.min = Some(min);
    p.max = Some(max);
    p
}

fn choice(name: &str, label: &str, options: &[&str]) -> ParamDef {
    let mut p = param(name, label, ParamType::Choice, json!(options[0]));
    p.options = options.iter().map(|s| s.to_string()).collect();
    p
}

/// The names and definitions of the shipped plug-in objects.
pub fn all() -> Vec<(&'static str, PluginDef)> {
    let mut supplier = param(
        "supplier",
        "Supplier",
        ParamType::Record,
        serde_json::Value::Null,
    );
    supplier.fields = vec![
        param("name", "Name", ParamType::Text, json!("")),
        param("article", "Article number", ParamType::Text, json!("")),
    ];
    let bench = PluginDef {
        shape_type: ShapeType::Point,
        params: vec![
            ranged(param("length", "Length", ParamType::Length, json!(1800.0)), 300.0, 6000.0),
            ranged(param("depth", "Depth", ParamType::Length, json!(450.0)), 200.0, 1200.0),
            ranged(param("slats", "Slats", ParamType::Number, json!(5.0)), 2.0, 12.0),
            param("back", "Back rest", ParamType::Bool, json!(true)),
            choice("material", "Material", &["Timber", "Steel", "Concrete"]),
            param("color", "Color", ParamType::Color, json!("#8b5a2b")),
            supplier,
        ],
        script: BENCH.into(),
        description: "A park bench. Place it at a point. The plan view shows the seat, the slats and the back rest.".into(),
        timeout_ms: None,
    };
    let stones = PluginDef {
        shape_type: ShapeType::Path,
        params: vec![
            ranged(
                param("size", "Stone size", ParamType::Length, json!(400.0)),
                50.0,
                2000.0,
            ),
            ranged(
                param("spacing", "Spacing", ParamType::Length, json!(700.0)),
                100.0,
                5000.0,
            ),
            ranged(
                param(
                    "start_offset",
                    "Start offset",
                    ParamType::Length,
                    json!(0.0),
                ),
                0.0,
                50000.0,
            ),
            ranged(
                param("zigzag", "Zigzag", ParamType::Length, json!(0.0)),
                0.0,
                1000.0,
            ),
            choice("stone", "Stone shape", &["Round", "Square"]),
            param("color", "Color", ParamType::Color, json!("#808080")),
        ],
        script: STEPPING_STONES.into(),
        description:
            "Stones along a path at an even spacing, round or square, with an optional zigzag."
                .into(),
        timeout_ms: None,
    };
    vec![("Bench", bench), ("Stepping stones", stones)]
}

/// The shipped definition with this name (compared without case).
pub fn find(name: &str) -> Option<PluginDef> {
    all()
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, d)| d)
}

/// True if the script is the script of a shipped example.
pub fn is_shipped_script(script: &str) -> bool {
    script == BENCH || script == STEPPING_STONES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_definitions_are_valid_and_trusted() {
        let all = all();
        assert_eq!(all.len(), 2);
        for (name, d) in &all {
            d.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(is_shipped_script(&d.script));
            // Defaults must pass their own checks.
            d.resolve_params(&serde_json::Map::new()).unwrap();
        }
        assert!(find("bench").is_some());
        assert!(find("nothing").is_none());
        assert!(!is_shipped_script("def reset(p, g): return []"));
    }
}
