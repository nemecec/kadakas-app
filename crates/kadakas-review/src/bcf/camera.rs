//! The camera of a viewpoint in the terms of BCF.
//!
//! Kadakas keeps a viewpoint as a plan view (center and zoom) or an orbit camera (target, yaw,
//! pitch, distance), in millimeters. BCF keeps an orthogonal or perspective camera as an eye
//! point, a direction and an up vector, in meters. These functions turn one into the other.
//!
//! - Perspective: the vertical field of view is 45 degrees, the field of view of the 3D view.
//! - Orthogonal: `ViewToWorldScale` is the visible height in meters. For an orbit camera this is
//!   `2 * distance * tan(22.5 degrees)`, as in the 3D view. For a plan view it is the height of a
//!   view of [`PLAN_VIEW_PX`] pixels at the zoom of the plan, so a plan view comes back as the
//!   same plan view.
//! - A plan view is an orthogonal camera that looks straight down with the up vector along +Y.
//!   BCF has no eye height for it: it is [`PLAN_EYE_HEIGHT_MM`] above the pin point.
//! - A perspective camera has no target in BCF. The target is [`DEFAULT_DISTANCE_MM`] from the
//!   eye, along the direction.

use kadakas_core::issue::ViewSpec;
use kadakas_core::model::MAX_COORD;

/// The vertical field of view of the perspective camera, in degrees.
pub const FOV_DEG: f64 = 45.0;
/// The size in pixels of the view that a plan zoom is measured against (width, height).
pub const PLAN_VIEW_PX: (f64, f64) = (1200.0, 800.0);
/// How far above the pin a plan camera is, in mm.
pub const PLAN_EYE_HEIGHT_MM: f64 = 100_000.0;
/// The distance to the target of a perspective camera that comes from BCF, in mm.
pub const DEFAULT_DISTANCE_MM: f64 = 10_000.0;

/// A BCF camera, in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BcfCamera {
    pub orthogonal: bool,
    pub eye: [f64; 3],
    pub direction: [f64; 3],
    pub up: [f64; 3],
    /// Orthogonal: the visible height in meters. Perspective: the field of view in degrees.
    pub size: f64,
    /// Width over height (BCF 3.0 only).
    pub aspect: f64,
}

fn add(a: [f64; 3], b: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

fn len(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = len(a);
    (l.is_finite() && l > 1e-9).then(|| [a[0] / l, a[1] / l, a[2] / l])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn half_tan() -> f64 {
    (FOV_DEG.to_radians() / 2.0).tan()
}

/// The BCF camera of a view. `ref_z_mm` is the height of the pin, for a plan view.
pub fn to_bcf(view: &ViewSpec, ref_z_mm: f64) -> BcfCamera {
    const M: f64 = 1.0 / 1000.0;
    match view {
        ViewSpec::Plan { center, zoom } => BcfCamera {
            orthogonal: true,
            eye: [
                center[0] * M,
                center[1] * M,
                (ref_z_mm + PLAN_EYE_HEIGHT_MM) * M,
            ],
            direction: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
            size: PLAN_VIEW_PX.1 / zoom * M,
            aspect: PLAN_VIEW_PX.0 / PLAN_VIEW_PX.1,
        },
        ViewSpec::Camera {
            target,
            yaw,
            pitch,
            distance,
            perspective,
        } => {
            let eye_dir = [
                pitch.cos() * yaw.cos(),
                pitch.cos() * yaw.sin(),
                pitch.sin(),
            ];
            let forward = [-eye_dir[0], -eye_dir[1], -eye_dir[2]];
            let right = [-yaw.sin(), yaw.cos(), 0.0];
            let up = cross(right, forward);
            let eye = add(*target, eye_dir, *distance);
            BcfCamera {
                orthogonal: !perspective,
                eye: [eye[0] * M, eye[1] * M, eye[2] * M],
                direction: forward,
                up,
                size: if *perspective {
                    FOV_DEG
                } else {
                    2.0 * distance * half_tan() * M
                },
                aspect: PLAN_VIEW_PX.0 / PLAN_VIEW_PX.1,
            }
        }
    }
}

/// The view of a BCF camera, or why it cannot be one.
pub fn from_bcf(c: &BcfCamera) -> Result<ViewSpec, String> {
    const MM: f64 = 1000.0;
    let finite = |v: &[f64; 3]| v.iter().all(|x| x.is_finite());
    if !finite(&c.eye) || !finite(&c.direction) || !finite(&c.up) || !c.size.is_finite() {
        return Err("the camera has a number that is not finite".into());
    }
    if c.eye.iter().any(|x| x.abs() * MM > MAX_COORD) {
        return Err("the camera is too far from the origin".into());
    }
    let dir = unit(c.direction).ok_or("the camera direction has no length")?;
    let eye_mm = [c.eye[0] * MM, c.eye[1] * MM, c.eye[2] * MM];
    if c.orthogonal {
        if c.size <= 0.0 {
            return Err("the visible size of an orthogonal camera must be above 0".into());
        }
        let up = unit(c.up);
        let straight_down = dir[0].abs() < 1e-6
            && dir[1].abs() < 1e-6
            && dir[2] < 0.0
            && up.is_some_and(|u| u[0].abs() < 1e-6 && u[1] > 0.999_999);
        if straight_down {
            return Ok(ViewSpec::Plan {
                center: [eye_mm[0], eye_mm[1]],
                zoom: PLAN_VIEW_PX.1 / (c.size * MM),
            });
        }
    } else if !(c.size > 0.0 && c.size < 180.0) {
        return Err("the field of view must be between 0 and 180 degrees".into());
    }
    let distance = if c.orthogonal {
        c.size * MM / 2.0 / half_tan()
    } else {
        DEFAULT_DISTANCE_MM
    };
    // The eye is the target plus `eye_dir * distance`.
    let eye_dir = [-dir[0], -dir[1], -dir[2]];
    let pitch = eye_dir[2].clamp(-1.0, 1.0).asin();
    let yaw = if eye_dir[0].abs() < 1e-9 && eye_dir[1].abs() < 1e-9 {
        // Straight up or down: the up vector says how the view is turned. For the 3D camera, up
        // is `(-cos(yaw), -sin(yaw))` looking down from above, and the opposite from below.
        let u = unit(c.up).unwrap_or([0.0, 1.0, 0.0]);
        if pitch > 0.0 {
            (-u[1]).atan2(-u[0])
        } else {
            u[1].atan2(u[0])
        }
    } else {
        eye_dir[1].atan2(eye_dir[0])
    };
    let target = add(eye_mm, dir, distance);
    if target.iter().any(|x| x.abs() > MAX_COORD) || !distance.is_finite() {
        return Err("the camera target is out of range".into());
    }
    Ok(ViewSpec::Camera {
        target,
        yaw,
        pitch,
        distance,
        perspective: !c.orthogonal,
    })
}
