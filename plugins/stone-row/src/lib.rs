//! Stone row: an example plug-in for Kadakas that is an object and a tool.
//!
//! The object is a row of round stepping stones along a line (a linear object). The tool lets a
//! person click the start and the end of the row. While the pointer moves, the tool previews the
//! stones. The second click ends the run: a plug-in object, or plain shapes if Shift is held.
//!
//! A tool call runs in a new instance, so the tool keeps its progress in `state`
//! (`{"start": [x, y]}`) and the host sends it back with the next event. The module has no
//! imports and uses no files, network or clock.

use kadakas_plugin_abi::serde_json::{Map, Value, json};
use kadakas_plugin_abi::{
    Geometry, Manifest, ObjectSpec, OutShape, ParamSchema, ParamType, ResetRequest, ShapeData,
    ShapeType, ToolEvent, ToolReply, ToolRequest, ToolResult, ToolSpec, export_plugin,
};

fn manifest() -> Manifest {
    let mut m = Manifest::object(
        "org.kadakas.stone-row",
        "Stone row",
        "1.0.0",
        ObjectSpec {
            shape_type: ShapeType::Linear,
            params: vec![
                ParamSchema::new("size", ParamType::Length)
                    .with_label("Stone diameter")
                    .with_default(json!(400))
                    .with_range(50.0, 2000.0),
                ParamSchema::new("gap", ParamType::Length)
                    .with_label("Gap")
                    .with_default(json!(300))
                    .with_range(0.0, 2000.0),
            ],
        },
    )
    .with_tool(ToolSpec {
        params: vec![
            ParamSchema::new("size", ParamType::Length)
                .with_label("Stone diameter")
                .with_default(json!(400))
                .with_range(50.0, 2000.0),
            ParamSchema::new("gap", ParamType::Length)
                .with_label("Gap")
                .with_default(json!(300))
                .with_range(0.0, 2000.0),
        ],
    });
    m.description =
        "A row of stepping stones along a line. Use the tool, or place the object with two points."
            .into();
    m.author = "The Kadakas project".into();
    m
}

/// Round stones from `a` toward `b`. The first stone touches `a`.
fn stones(a: [f64; 2], b: [f64; 2], size: f64, gap: f64) -> Result<Vec<OutShape>, String> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy);
    if length < size {
        return Err(format!(
            "the line ({length:.0} mm) is shorter than one stone ({size:.0} mm)"
        ));
    }
    let step = size + gap;
    let count = ((length - size) / step).floor() as usize + 1;
    let (ux, uy) = (dx / length, dy / length);
    Ok((0..count.min(500))
        .map(|i| {
            let along = size / 2.0 + step * i as f64;
            OutShape::new(ShapeData::Circle {
                center: [a[0] + ux * along, a[1] + uy * along],
                radius: size / 2.0,
            })
        })
        .collect())
}

fn number(params: &Map<String, Value>, key: &str, default: f64) -> f64 {
    params.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn make_shapes(request: ResetRequest) -> Result<Vec<OutShape>, String> {
    let Geometry::Linear { a, b } = request.geometry else {
        return Err("a stone row needs a line".into());
    };
    stones(
        a,
        b,
        number(&request.params, "size", 400.0),
        number(&request.params, "gap", 300.0),
    )
}

fn point_of(v: &Value) -> Option<[f64; 2]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}

fn run_tool(request: ToolRequest) -> Result<ToolReply, String> {
    let start = request.state.get("start").and_then(point_of);
    let size = number(&request.params, "size", 400.0);
    let gap = number(&request.params, "gap", 300.0);
    match (request.event, start) {
        (ToolEvent::Start, _) => Ok(ToolReply {
            hint: "Click the start of the row".into(),
            ..ToolReply::default()
        }),
        (ToolEvent::Click { at }, None) => Ok(ToolReply {
            state: json!({"start": at}),
            hint: "Click the end of the row".into(),
            ..ToolReply::default()
        }),
        (ToolEvent::Move { at }, Some(a)) => Ok(ToolReply {
            state: request.state,
            // A line too short for one stone has no preview. That is not an error.
            preview: stones(a, at, size, gap).unwrap_or_default(),
            hint: "Click the end of the row".into(),
            ..ToolReply::default()
        }),
        (ToolEvent::Click { at: b }, Some(a)) => {
            // Check the row first, so a bad click keeps the tool running.
            let shapes = stones(a, b, size, gap)?;
            let result = if request.modifiers.shift {
                ToolResult::Shapes { shapes }
            } else {
                ToolResult::Object {
                    params: request.params.clone(),
                    geometry: Geometry::Linear { a, b },
                }
            };
            Ok(ToolReply {
                result: Some(result),
                ..ToolReply::default()
            })
        }
        (ToolEvent::Move { .. } | ToolEvent::Finish, None) => Ok(ToolReply {
            hint: "Click the start of the row".into(),
            ..ToolReply::default()
        }),
        (ToolEvent::Finish, Some(_)) => Err("click the end of the row".into()),
    }
}

export_plugin!(describe = manifest, reset = make_shapes, tool = run_tool);
