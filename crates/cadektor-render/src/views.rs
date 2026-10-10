//! Standard views, projections and render modes.

/// The ready-made views of the View menu. Names follow the Vectorworks standard views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StandardView {
    /// Top/Plan: the 2D drawing in the app. A render shows it from above, like `Top`.
    #[default]
    Plan,
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
    LeftIsometric,
    RightIsometric,
    LeftRearIsometric,
    RightRearIsometric,
}

impl StandardView {
    pub const ALL: [StandardView; 11] = [
        StandardView::Plan,
        StandardView::Top,
        StandardView::Bottom,
        StandardView::Front,
        StandardView::Back,
        StandardView::Left,
        StandardView::Right,
        StandardView::LeftIsometric,
        StandardView::RightIsometric,
        StandardView::LeftRearIsometric,
        StandardView::RightRearIsometric,
    ];

    /// Stable name for the API.
    pub fn id(self) -> &'static str {
        match self {
            StandardView::Plan => "plan",
            StandardView::Top => "top",
            StandardView::Bottom => "bottom",
            StandardView::Front => "front",
            StandardView::Back => "back",
            StandardView::Left => "left",
            StandardView::Right => "right",
            StandardView::LeftIsometric => "left_isometric",
            StandardView::RightIsometric => "right_isometric",
            StandardView::LeftRearIsometric => "left_rear_isometric",
            StandardView::RightRearIsometric => "right_rear_isometric",
        }
    }

    /// Name for menus.
    pub fn label(self) -> &'static str {
        match self {
            StandardView::Plan => "Top/Plan",
            StandardView::Top => "Top",
            StandardView::Bottom => "Bottom",
            StandardView::Front => "Front",
            StandardView::Back => "Back",
            StandardView::Left => "Left",
            StandardView::Right => "Right",
            StandardView::LeftIsometric => "Left Isometric",
            StandardView::RightIsometric => "Right Isometric",
            StandardView::LeftRearIsometric => "Left Rear Isometric",
            StandardView::RightRearIsometric => "Right Rear Isometric",
        }
    }

    /// Finds a view by id or label. Case, spaces, hyphens and slashes do not matter,
    /// so `Right Isometric`, `right-isometric` and `right_isometric` all work.
    /// `top/plan` is `plan`; `iso` names are accepted for `isometric`.
    pub fn from_name(name: &str) -> Option<StandardView> {
        let norm: String = name
            .trim()
            .to_ascii_lowercase()
            .chars()
            .map(|c| {
                if c == ' ' || c == '-' || c == '/' {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let norm = norm.replace("isometric", "iso");
        StandardView::ALL.into_iter().find(|v| {
            let id = v.id().replace("isometric", "iso");
            id == norm
                || v.label()
                    .to_ascii_lowercase()
                    .replace(['/', ' '], "_")
                    .replace("isometric", "iso")
                    == norm
        })
    }

    /// True for Top/Plan, which the app shows as the 2D drawing.
    pub fn is_plan(self) -> bool {
        self == StandardView::Plan
    }

    /// Eye direction as (yaw, pitch) in radians.
    pub fn yaw_pitch(self) -> (f32, f32) {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        // True isometric elevation: the angle whose tangent is 1/sqrt(2).
        let iso = (1.0f32 / 2.0f32.sqrt()).atan();
        match self {
            StandardView::Plan | StandardView::Top => (-FRAC_PI_2, FRAC_PI_2),
            StandardView::Bottom => (-FRAC_PI_2, -FRAC_PI_2),
            StandardView::Front => (-FRAC_PI_2, 0.0),
            StandardView::Back => (FRAC_PI_2, 0.0),
            StandardView::Left => (PI, 0.0),
            StandardView::Right => (0.0, 0.0),
            StandardView::LeftIsometric => (-3.0 * FRAC_PI_4, iso),
            StandardView::RightIsometric => (-FRAC_PI_4, iso),
            StandardView::LeftRearIsometric => (3.0 * FRAC_PI_4, iso),
            StandardView::RightRearIsometric => (FRAC_PI_4, iso),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Projection {
    #[default]
    Orthographic,
    Perspective,
}

impl Projection {
    pub fn id(self) -> &'static str {
        match self {
            Projection::Orthographic => "ortho",
            Projection::Perspective => "perspective",
        }
    }

    pub fn from_name(name: &str) -> Option<Projection> {
        match name.trim().to_ascii_lowercase().as_str() {
            "ortho" | "orthographic" | "parallel" => Some(Projection::Orthographic),
            "perspective" | "persp" => Some(Projection::Perspective),
            _ => None,
        }
    }

    pub fn toggled(self) -> Projection {
        match self {
            Projection::Orthographic => Projection::Perspective,
            Projection::Perspective => Projection::Orthographic,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderMode {
    #[default]
    Shaded,
    Wireframe,
}

impl RenderMode {
    pub fn id(self) -> &'static str {
        match self {
            RenderMode::Shaded => "shaded",
            RenderMode::Wireframe => "wireframe",
        }
    }

    pub fn from_name(name: &str) -> Option<RenderMode> {
        match name.trim().to_ascii_lowercase().as_str() {
            "shaded" => Some(RenderMode::Shaded),
            "wireframe" => Some(RenderMode::Wireframe),
            _ => None,
        }
    }

    pub fn toggled(self) -> RenderMode {
        match self {
            RenderMode::Shaded => RenderMode::Wireframe,
            RenderMode::Wireframe => RenderMode::Shaded,
        }
    }
}

/// The view that the user, or an agent through `view.set`, has chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewSettings {
    pub view: StandardView,
    pub projection: Projection,
    pub mode: RenderMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_names() {
        assert_eq!(
            StandardView::from_name("Top/Plan"),
            Some(StandardView::Plan)
        );
        assert_eq!(StandardView::from_name("plan"), Some(StandardView::Plan));
        assert_eq!(StandardView::from_name("top"), Some(StandardView::Top));
        assert!(StandardView::Plan.is_plan() && !StandardView::Top.is_plan());
        assert_eq!(
            StandardView::from_name(" Right Isometric "),
            Some(StandardView::RightIsometric)
        );
        assert_eq!(
            StandardView::from_name("left-rear-isometric"),
            Some(StandardView::LeftRearIsometric)
        );
        assert_eq!(
            StandardView::from_name("right_iso"),
            Some(StandardView::RightIsometric)
        );
        assert_eq!(StandardView::from_name("sideways"), None);
        for v in StandardView::ALL {
            assert_eq!(StandardView::from_name(v.id()), Some(v));
            assert_eq!(StandardView::from_name(v.label()), Some(v));
        }
        assert_eq!(
            Projection::from_name("Orthographic"),
            Some(Projection::Orthographic)
        );
        assert_eq!(
            Projection::from_name("persp"),
            Some(Projection::Perspective)
        );
        assert_eq!(Projection::from_name("fisheye"), None);
        assert_eq!(
            Projection::Orthographic.toggled().toggled(),
            Projection::Orthographic
        );
        assert_eq!(Projection::Perspective.id(), "perspective");
        assert_eq!(Projection::Orthographic.id(), "ortho");
        assert_eq!(
            RenderMode::from_name("Wireframe"),
            Some(RenderMode::Wireframe)
        );
        assert_eq!(RenderMode::from_name("x"), None);
        assert_eq!(RenderMode::Shaded.toggled(), RenderMode::Wireframe);
        assert_eq!(RenderMode::Shaded.toggled().id(), "wireframe");
    }
}
