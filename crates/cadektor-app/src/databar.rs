//! Data bar entry: type exact values instead of clicking. The math is pure.
//!
//! Before the first click the fields are X and Y. After it they depend on the
//! tool: Length and Angle (line, polyline), Width and Height (rectangle), Radius
//! (circle). A field left empty follows the cursor. Enter turns the values into
//! a model point that is sent to the tool as a click, so the object is created
//! through the normal command path.

use glam::DVec2;

use crate::tools::Tool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    X,
    Y,
    Length,
    Angle,
    Width,
    Height,
    Radius,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::X => "X",
            Field::Y => "Y",
            Field::Length => "L",
            Field::Angle => "A",
            Field::Width => "W",
            Field::Height => "H",
            Field::Radius => "R",
        }
    }

    pub fn unit(self) -> &'static str {
        if self == Field::Angle { "\u{b0}" } else { "mm" }
    }
}

/// Tools that have Data bar entry.
pub fn supported(tool: Tool) -> bool {
    matches!(tool, Tool::Line | Tool::Rect | Tool::Circle | Tool::Polyline)
}

/// The fields for a tool. `anchored` is true after the first click.
pub fn fields(tool: Tool, anchored: bool) -> &'static [Field] {
    use Field::*;
    if !supported(tool) {
        return &[];
    }
    if !anchored {
        return &[X, Y];
    }
    match tool {
        Tool::Rect => &[Width, Height],
        Tool::Circle => &[Radius],
        _ => &[Length, Angle],
    }
}

/// Parses user text. A comma counts as a decimal point. Empty or bad text is `None`.
pub fn parse_value(s: &str) -> Option<f64> {
    let v: f64 = s.trim().replace(',', ".").parse().ok()?;
    v.is_finite().then_some(v)
}

/// The model point that the typed values describe. Empty fields follow `cursor`.
/// `anchor` is the last clicked point. Returns `None` for tools without entry.
pub fn target_point(tool: Tool, anchor: Option<DVec2>, cursor: DVec2, values: &[Option<f64>]) -> Option<DVec2> {
    if !supported(tool) {
        return None;
    }
    let fs = fields(tool, anchor.is_some());
    let get = |f: Field| fs.iter().position(|x| *x == f).and_then(|i| values.get(i).copied().flatten());
    let Some(a) = anchor else {
        return Some(DVec2::new(get(Field::X).unwrap_or(cursor.x), get(Field::Y).unwrap_or(cursor.y)));
    };
    let d = cursor - a;
    Some(match tool {
        Tool::Rect => a + DVec2::new(get(Field::Width).unwrap_or(d.x), get(Field::Height).unwrap_or(d.y)),
        Tool::Circle => {
            let dir = if d.length() > 0.0 { d.normalize() } else { DVec2::X };
            a + dir * get(Field::Radius).unwrap_or_else(|| d.length())
        }
        _ => {
            let len = get(Field::Length).unwrap_or_else(|| d.length());
            let ang = get(Field::Angle).map_or_else(|| d.y.atan2(d.x), f64::to_radians);
            a + DVec2::new(ang.cos(), ang.sin()) * len
        }
    })
}

/// The live value of each field, shown as a hint in empty fields.
pub fn live_values(tool: Tool, anchor: Option<DVec2>, cursor: DVec2) -> Vec<f64> {
    fields(tool, anchor.is_some())
        .iter()
        .map(|f| {
            let d = anchor.map_or(DVec2::ZERO, |a| cursor - a);
            match f {
                Field::X => cursor.x,
                Field::Y => cursor.y,
                Field::Length | Field::Radius => d.length(),
                Field::Angle => d.y.atan2(d.x).to_degrees(),
                Field::Width => d.x,
                Field::Height => d.y,
            }
        })
        .collect()
}

pub fn field_id(i: usize) -> egui::Id {
    egui::Id::new(("databar_field", i))
}

/// Edit state of the Data bar fields.
#[derive(Default)]
pub struct DataEntry {
    /// A field has (or is about to get) keyboard focus.
    pub active: bool,
    pub focus: usize,
    pub text: Vec<String>,
    /// Ask the UI to move focus to field `focus`.
    pub want_focus: bool,
    key: Option<(Tool, bool)>,
}

impl DataEntry {
    /// Call every frame. The field set changes with the tool and the first click; the text resets then.
    pub fn sync(&mut self, tool: Tool, anchored: bool) {
        if self.key != Some((tool, anchored)) {
            self.key = Some((tool, anchored));
            self.text = vec![String::new(); fields(tool, anchored).len()];
            self.focus = 0;
            self.want_focus = self.active;
            if !supported(tool) {
                self.active = false;
            }
        }
    }

    pub fn values(&self) -> Vec<Option<f64>> {
        self.text.iter().map(|t| parse_value(t)).collect()
    }

    pub fn any_typed(&self) -> bool {
        self.text.iter().any(|t| parse_value(t).is_some())
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.focus = 0;
        self.want_focus = true;
    }

    /// Tab (or Shift+Tab) moves to the next (previous) field and wraps around.
    pub fn cycle(&mut self, back: bool) {
        let n = self.text.len();
        if n == 0 {
            return;
        }
        self.focus = if back { (self.focus + n - 1) % n } else { (self.focus + 1) % n };
        self.want_focus = true;
    }

    pub fn clear_text(&mut self) {
        self.text.iter_mut().for_each(String::clear);
        self.focus = 0;
    }

    /// Takes keyboard focus away from the fields.
    pub fn release(&mut self, ctx: &egui::Context) {
        self.active = false;
        self.want_focus = false;
        ctx.memory_mut(|m| (0..4).for_each(|i| m.surrender_focus(field_id(i))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: DVec2 = DVec2::ZERO;

    fn close(a: DVec2, b: DVec2) {
        assert!(a.distance(b) < 1e-9, "{a} != {b}");
    }

    #[test]
    fn field_sets() {
        assert_eq!(fields(Tool::Line, false), &[Field::X, Field::Y]);
        assert_eq!(fields(Tool::Line, true), &[Field::Length, Field::Angle]);
        assert_eq!(fields(Tool::Polyline, true), &[Field::Length, Field::Angle]);
        assert_eq!(fields(Tool::Rect, true), &[Field::Width, Field::Height]);
        assert_eq!(fields(Tool::Circle, true), &[Field::Radius]);
        assert!(fields(Tool::Text, true).is_empty());
        assert!(!supported(Tool::Select));
    }

    #[test]
    fn parses_values() {
        assert_eq!(parse_value(" 12,5 "), Some(12.5));
        assert_eq!(parse_value("-3"), Some(-3.0));
        assert_eq!(parse_value(""), None);
        assert_eq!(parse_value("abc"), None);
        assert_eq!(parse_value("inf"), None);
    }

    #[test]
    fn start_point_from_x_y() {
        let c = DVec2::new(7.0, 9.0);
        close(target_point(Tool::Line, None, c, &[Some(100.0), Some(200.0)]).unwrap(), DVec2::new(100.0, 200.0));
        close(target_point(Tool::Rect, None, c, &[Some(100.0), None]).unwrap(), DVec2::new(100.0, 9.0));
        close(target_point(Tool::Circle, None, c, &[None, None]).unwrap(), c);
    }

    #[test]
    fn line_length_and_angle() {
        let a = DVec2::new(10.0, 10.0);
        close(target_point(Tool::Line, Some(a), O, &[Some(100.0), Some(0.0)]).unwrap(), DVec2::new(110.0, 10.0));
        close(target_point(Tool::Line, Some(a), O, &[Some(50.0), Some(90.0)]).unwrap(), DVec2::new(10.0, 60.0));
        // Length only: the angle follows the cursor.
        close(target_point(Tool::Line, Some(a), DVec2::new(10.0, 500.0), &[Some(25.0), None]).unwrap(), DVec2::new(10.0, 35.0));
        // Angle only: the length follows the cursor.
        close(target_point(Tool::Polyline, Some(O), DVec2::new(30.0, 40.0), &[None, Some(0.0)]).unwrap(), DVec2::new(50.0, 0.0));
        // Nothing typed: the cursor.
        close(target_point(Tool::Line, Some(a), DVec2::new(30.0, 40.0), &[None, None]).unwrap(), DVec2::new(30.0, 40.0));
    }

    #[test]
    fn rect_width_height_are_signed() {
        let a = DVec2::new(100.0, 100.0);
        close(target_point(Tool::Rect, Some(a), O, &[Some(300.0), Some(-50.0)]).unwrap(), DVec2::new(400.0, 50.0));
        close(target_point(Tool::Rect, Some(a), DVec2::new(150.0, 180.0), &[Some(10.0), None]).unwrap(), DVec2::new(110.0, 180.0));
    }

    #[test]
    fn circle_radius_keeps_cursor_direction() {
        close(target_point(Tool::Circle, Some(O), DVec2::new(0.0, 3.0), &[Some(500.0)]).unwrap(), DVec2::new(0.0, 500.0));
        close(target_point(Tool::Circle, Some(O), O, &[Some(500.0)]).unwrap(), DVec2::new(500.0, 0.0));
    }

    #[test]
    fn unsupported_tool_has_no_target() {
        assert_eq!(target_point(Tool::Text, None, O, &[]), None);
    }

    #[test]
    fn live_values_match_cursor() {
        let v = live_values(Tool::Line, Some(O), DVec2::new(0.0, 10.0));
        assert!((v[0] - 10.0).abs() < 1e-9 && (v[1] - 90.0).abs() < 1e-9);
        assert_eq!(live_values(Tool::Rect, None, DVec2::new(1.0, 2.0)), vec![1.0, 2.0]);
    }

    #[test]
    fn entry_cycles_and_resets() {
        let mut e = DataEntry::default();
        e.sync(Tool::Line, false);
        e.activate();
        assert_eq!(e.text.len(), 2);
        e.cycle(false);
        assert_eq!(e.focus, 1);
        e.cycle(false);
        assert_eq!(e.focus, 0);
        e.cycle(true);
        assert_eq!(e.focus, 1);
        e.text[0] = "5".into();
        assert_eq!(e.values(), vec![Some(5.0), None]);
        assert!(e.any_typed());
        // The first click changes the field set; text resets and the entry stays active.
        e.sync(Tool::Line, true);
        assert!(e.active && e.want_focus && e.focus == 0 && !e.any_typed());
        // Switching to a tool without entry deactivates.
        e.sync(Tool::Select, false);
        assert!(!e.active);
    }
}
