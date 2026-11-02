//! The camera of `render.png` and `mesh.render` (AGR-09). It is the camera that `view.get` gives,
//! with the one rule of `view.camera` for the position: the eye, or yaw, pitch and distance, and
//! with both they must agree (`kadakas_core::sheets::orbit_from`).

use serde_json::{Value, json};

use kadakas_core::sheets::{OrbitError, ViewCamera, orbit_from};

use crate::RpcError;
use crate::errors::bad;
use crate::mesh_api::typed;

/// The turn of a camera that looks straight down or up from an eye: the turn of the Top view.
const VERTICAL_YAW: f64 = -std::f64::consts::FRAC_PI_2;

/// The error for a camera request that is refused.
pub(crate) fn orbit_error(e: OrbitError) -> RpcError {
    match e {
        OrbitError::Conflict(m) => typed(
            bad(m),
            "camera_conflict",
            json!({"keys": ["eye", "yaw", "pitch", "distance"]}),
        ),
        OrbitError::Invalid(m) => bad(format!("camera: {m}")),
    }
}

fn number(c: &Value, key: &str) -> Result<Option<f64>, RpcError> {
    match c.get(key).filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => v
            .as_f64()
            .map(Some)
            .ok_or_else(|| bad(format!("camera.{key} must be a number"))),
    }
}

fn triple(c: &Value, key: &str) -> Result<Option<[f64; 3]>, RpcError> {
    let Some(v) = c.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let list: Option<Vec<f64>> = v
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_f64).collect());
    match list {
        Some(l) if l.len() == 3 => Ok(Some([l[0], l[1], l[2]])),
        _ => Err(bad(format!("camera.{key} must be [x, y, z]"))),
    }
}

/// The saved form of a camera given as JSON: `target` with `eye`, or with `yaw`, `pitch` and
/// `distance`. `fov` is 45 degrees and `projection` is perspective when they are left out. The
/// keys of the camera of `view.get` that the picture does not need are ignored.
pub(crate) fn render_camera(c: &Value) -> Result<ViewCamera, RpcError> {
    let target = triple(c, "target")?.ok_or_else(|| bad("camera.target is required"))?;
    let orbit = orbit_from(
        target,
        triple(c, "eye")?,
        number(c, "yaw")?,
        number(c, "pitch")?,
        number(c, "distance")?,
    )
    .map_err(orbit_error)?;
    let projection = match c.get("projection").filter(|v| !v.is_null()) {
        None => "perspective".to_string(),
        Some(v) => v
            .as_str()
            .ok_or_else(|| bad("camera.projection must be perspective or ortho"))?
            .to_string(),
    };
    Ok(ViewCamera {
        target,
        yaw: orbit.yaw.unwrap_or(VERTICAL_YAW),
        pitch: orbit.pitch,
        distance: orbit.distance,
        fov: number(c, "fov")?.unwrap_or(kadakas_core::sheets::DEFAULT_FOV_DEG),
        projection,
    })
}
