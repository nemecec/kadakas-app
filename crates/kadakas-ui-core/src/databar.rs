//! Data bar entry: type exact values instead of clicking. The math is pure.
//!
//! Before the first click the fields are X and Y. After it they depend on the
//! tool: Length and Angle (line and the path tools), Width and Height (rectangle), Radius
//! (circle). In Arc vertex mode of the path tools, a Radius field is added.
//! A field left empty follows the cursor. Enter turns the values into
//! a model point that is sent to the tool as a click, so the object is created
//! through the normal command path.

use glam::DVec2;
use kadakas_l10n::tr;

use crate::tools::{Tool, has_vertex_modes};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    X,
    Y,
    Length,
    Angle,
    Width,
    Height,
    Radius,
    /// Radius of new arc vertices (path tools in Arc mode).
    ArcRadius,
}

impl Field {
    pub fn label(self) -> String {
        match self {
            Field::X => tr!("uic-databar-x"),
            Field::Y => tr!("uic-databar-y"),
            Field::Length => tr!("uic-databar-l"),
            Field::Angle => tr!("uic-databar-a"),
            Field::Width => tr!("uic-databar-w"),
            Field::Height => tr!("uic-databar-h"),
            Field::Radius => tr!("uic-databar-r"),
            Field::ArcRadius => tr!("uic-databar-arc-r"),
        }
    }

    /// The unit that goes next to the field, in the units of the UI process: `mm`, `cm`, `m`,
    /// `ft` or `\u{b0}`. Empty for inches and feet-inches, where the text carries its marks
    /// (`5' 6"`).
    pub fn unit(self) -> &'static str {
        if self == Field::Angle {
            "\u{b0}"
        } else {
            kadakas_units::current().length_field_unit()
        }
    }

    /// True for the fields that hold an angle.
    pub fn is_angle(self) -> bool {
        self == Field::Angle
    }
}

/// Tools that have Data bar entry.
pub fn supported(tool: Tool) -> bool {
    matches!(tool, Tool::Line | Tool::Rect | Tool::Circle) || has_vertex_modes(tool)
}

/// The fields for a tool. `anchored` is true after the first click. `arc` is
/// true when a path tool is in Arc vertex mode, which adds the Radius field.
pub fn fields(tool: Tool, anchored: bool, arc: bool) -> &'static [Field] {
    use Field::*;
    if !supported(tool) {
        return &[];
    }
    let arc = arc && has_vertex_modes(tool);
    if !anchored {
        return if arc { &[X, Y, ArcRadius] } else { &[X, Y] };
    }
    if arc {
        return &[Length, Angle, ArcRadius];
    }
    match tool {
        Tool::Rect => &[Width, Height],
        Tool::Circle => &[Radius],
        _ => &[Length, Angle],
    }
}

/// Parses what a person typed into a field, in the units and number format of the UI process
/// (`kadakas-units`): a length gives millimeters (`1,5 m`, `5' 6 1/2"`), the angle gives degrees
/// (`45,5`, `45\u{b0}30'`, `N 45 E`). A number without a unit counts in the active unit. Empty
/// or bad text is `None`.
pub fn parse_field(field: Field, s: &str) -> Option<f64> {
    let units = kadakas_units::current();
    if field.is_angle() {
        units.parse_angle(s)
    } else {
        units.parse_length(s)
    }
}

/// The text for a field that shows the number `v` (mm or degrees) in the active units, for the
/// hint of an empty field. No digit grouping, so a person can edit it.
pub fn field_text(field: Field, v: f64) -> String {
    let units = kadakas_units::current();
    if field.is_angle() {
        units.angle_field(v, 1)
    } else {
        units.length_field(v, units.length_decimals())
    }
}

/// The model point that the typed values describe. Empty fields follow `cursor`.
/// `anchor` is the last clicked point. Returns `None` for tools without entry.
pub fn target_point(
    tool: Tool,
    anchor: Option<DVec2>,
    cursor: DVec2,
    values: &[Option<f64>],
    arc: bool,
) -> Option<DVec2> {
    if !supported(tool) {
        return None;
    }
    let fs = fields(tool, anchor.is_some(), arc);
    let get = |f: Field| {
        fs.iter()
            .position(|x| *x == f)
            .and_then(|i| values.get(i).copied().flatten())
    };
    let Some(a) = anchor else {
        return Some(DVec2::new(
            get(Field::X).unwrap_or(cursor.x),
            get(Field::Y).unwrap_or(cursor.y),
        ));
    };
    let d = cursor - a;
    Some(match tool {
        Tool::Rect => {
            a + DVec2::new(
                get(Field::Width).unwrap_or(d.x),
                get(Field::Height).unwrap_or(d.y),
            )
        }
        Tool::Circle => {
            let dir = if d.length() > 0.0 {
                d.normalize()
            } else {
                DVec2::X
            };
            a + dir * get(Field::Radius).unwrap_or_else(|| d.length())
        }
        _ => {
            let len = get(Field::Length).unwrap_or_else(|| d.length());
            let ang = get(Field::Angle).map_or_else(|| d.y.atan2(d.x), f64::to_radians);
            a + DVec2::new(ang.cos(), ang.sin()) * len
        }
    })
}

/// The typed arc radius, if the Radius field exists and holds a valid value (zero or more).
pub fn typed_arc_radius(
    tool: Tool,
    anchored: bool,
    arc: bool,
    values: &[Option<f64>],
) -> Option<f64> {
    let i = fields(tool, anchored, arc)
        .iter()
        .position(|f| *f == Field::ArcRadius)?;
    values.get(i).copied().flatten().filter(|r| *r >= 0.0)
}

/// The live value of each field, shown as a hint in empty fields.
/// `arc_radius` is the default radius of new arc vertices.
pub fn live_values(
    tool: Tool,
    anchor: Option<DVec2>,
    cursor: DVec2,
    arc: bool,
    arc_radius: f64,
) -> Vec<f64> {
    fields(tool, anchor.is_some(), arc)
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
                Field::ArcRadius => arc_radius,
            }
        })
        .collect()
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
    key: Option<(Tool, bool, bool)>,
}

impl DataEntry {
    /// Call every frame. The field set changes with the tool and the first click; the text resets then.
    pub fn sync(&mut self, tool: Tool, anchored: bool, arc: bool) {
        if self.key != Some((tool, anchored, arc)) {
            self.key = Some((tool, anchored, arc));
            self.text = vec![String::new(); fields(tool, anchored, arc).len()];
            self.focus = 0;
            self.want_focus = self.active;
            if !supported(tool) {
                self.active = false;
            }
        }
    }

    /// The typed values in mm and degrees; `None` for an empty field or one that does not parse.
    pub fn values(&self) -> Vec<Option<f64>> {
        let fs = self.current_fields();
        let units = kadakas_units::current();
        self.text
            .iter()
            .enumerate()
            // A number like `1,500` can mean 1500 or 1.5. The Data bar does not guess: it gives
            // no value and `problem` tells the person.
            .map(|(i, t)| {
                fs.get(i)
                    .filter(|_| units.ambiguous_number(t).is_none())
                    .and_then(|f| parse_field(*f, t))
            })
            .collect()
    }

    /// A message for a typed number that has two readings, or `None`. It names the decimal
    /// separator of the active format.
    pub fn problem(&self) -> Option<String> {
        let units = kadakas_units::current();
        self.text.iter().find_map(|t| {
            let decimal = units.ambiguous_number(t)?;
            Some(tr!(
                "uic-databar-ambiguous",
                text = t.trim(),
                decimal = decimal.to_string()
            ))
        })
    }

    pub fn any_typed(&self) -> bool {
        self.values().iter().any(Option::is_some)
    }

    fn current_fields(&self) -> &'static [Field] {
        self.key
            .map_or(&[][..], |(tool, anchored, arc)| fields(tool, anchored, arc))
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
        self.focus = if back {
            (self.focus + n - 1) % n
        } else {
            (self.focus + 1) % n
        };
        self.want_focus = true;
    }

    pub fn clear_text(&mut self) {
        self.text.iter_mut().for_each(String::clear);
        self.focus = 0;
    }

    /// Leaves the entry mode. The UI program must also take keyboard focus from its fields.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.want_focus = false;
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
        assert_eq!(fields(Tool::Line, false, false), &[Field::X, Field::Y]);
        assert_eq!(
            fields(Tool::Line, true, false),
            &[Field::Length, Field::Angle]
        );
        assert_eq!(
            fields(Tool::Polyline, true, false),
            &[Field::Length, Field::Angle]
        );
        assert_eq!(
            fields(Tool::Rect, true, false),
            &[Field::Width, Field::Height]
        );
        assert_eq!(fields(Tool::Circle, true, false), &[Field::Radius]);
        assert!(fields(Tool::Text, true, false).is_empty());
        assert!(!supported(Tool::Select));
    }

    #[test]
    fn parses_values_in_the_active_units() {
        use kadakas_units::{LengthUnit, Settings, Units, with_units};
        // The default units: millimeters, and a comma works as a decimal point.
        assert_eq!(parse_field(Field::Length, " 12,5 "), Some(12.5));
        assert_eq!(parse_field(Field::Length, "-3"), Some(-3.0));
        assert_eq!(parse_field(Field::Length, ""), None);
        assert_eq!(parse_field(Field::Length, "abc"), None);
        assert_eq!(parse_field(Field::Length, "inf"), None);
        assert_eq!(parse_field(Field::Length, "1.5 m"), Some(1500.0));
        assert_eq!(parse_field(Field::Angle, "45,5"), Some(45.5));
        assert_eq!(parse_field(Field::Angle, "N 45 E"), Some(45.0));
        // A decimal comma, and a unit in the text.
        let et = Settings::default().resolve("et");
        with_units(et, || {
            assert_eq!(parse_field(Field::X, "1,5 m"), Some(1500.0));
            assert_eq!(parse_field(Field::X, "1 234,5"), Some(1234.5));
            assert_eq!(field_text(Field::X, 1234.56), "1234,6");
            assert_eq!(field_text(Field::Angle, 45.0), "45,0");
            assert_eq!(Field::X.unit(), "mm");
            assert_eq!(Field::Angle.unit(), "\u{b0}");
        });
        // Feet and inches.
        let imperial = Units {
            length: LengthUnit::FtIn,
            ..Units::METRIC
        };
        with_units(imperial, || {
            assert_eq!(parse_field(Field::Length, "5' 6 1/2\""), Some(25.4 * 66.5));
            assert_eq!(parse_field(Field::Width, "12"), Some(25.4 * 12.0));
            assert_eq!(field_text(Field::Width, 25.4 * 66.5), "5' 6 1/2\"");
            assert_eq!(Field::Width.unit(), "");
        });
    }

    #[test]
    fn an_ambiguous_number_gives_no_value_and_a_message() {
        use kadakas_units::{Settings, with_units};
        let mut e = DataEntry::default();
        e.sync(Tool::Line, true, false);
        e.text = vec!["1,500".into(), String::new()];
        // Decimal comma and space groups (Estonian): 1,500 is one and a half.
        with_units(Settings::default().resolve("et"), || {
            assert_eq!(e.values()[0], Some(1.5));
            assert_eq!(e.problem(), None);
        });
        // Decimal point and comma groups (en-US): the Data bar does not guess.
        with_units(Settings::default().resolve("en-US"), || {
            assert_eq!(e.values()[0], None);
            let m = e.problem().unwrap();
            assert!(m.contains("1,500") && m.contains('.'), "{m}");
            kadakas_l10n::with_language("et", || {
                assert!(e.problem().unwrap().contains("Kümnenderaldaja"));
            });
            e.text = vec!["1500".into(), "0,500".into()];
            assert_eq!(e.problem(), None);
            assert_eq!(e.values()[0], Some(1500.0));
        });
    }

    #[test]
    fn the_entry_parses_each_field_with_its_own_kind() {
        let mut e = DataEntry::default();
        assert!(e.values().is_empty());
        e.sync(Tool::Line, true, false);
        e.text = vec!["1,5 m".into(), "45".into()];
        assert_eq!(e.values(), vec![Some(1500.0), Some(45.0)]);
        assert!(e.any_typed());
        e.clear_text();
        assert!(!e.any_typed());
    }

    #[test]
    fn start_point_from_x_y() {
        let c = DVec2::new(7.0, 9.0);
        close(
            target_point(Tool::Line, None, c, &[Some(100.0), Some(200.0)], false).unwrap(),
            DVec2::new(100.0, 200.0),
        );
        close(
            target_point(Tool::Rect, None, c, &[Some(100.0), None], false).unwrap(),
            DVec2::new(100.0, 9.0),
        );
        close(
            target_point(Tool::Circle, None, c, &[None, None], false).unwrap(),
            c,
        );
    }

    #[test]
    fn line_length_and_angle() {
        let a = DVec2::new(10.0, 10.0);
        close(
            target_point(Tool::Line, Some(a), O, &[Some(100.0), Some(0.0)], false).unwrap(),
            DVec2::new(110.0, 10.0),
        );
        close(
            target_point(Tool::Line, Some(a), O, &[Some(50.0), Some(90.0)], false).unwrap(),
            DVec2::new(10.0, 60.0),
        );
        // Length only: the angle follows the cursor.
        close(
            target_point(
                Tool::Line,
                Some(a),
                DVec2::new(10.0, 500.0),
                &[Some(25.0), None],
                false,
            )
            .unwrap(),
            DVec2::new(10.0, 35.0),
        );
        // Angle only: the length follows the cursor.
        close(
            target_point(
                Tool::Polyline,
                Some(O),
                DVec2::new(30.0, 40.0),
                &[None, Some(0.0)],
                false,
            )
            .unwrap(),
            DVec2::new(50.0, 0.0),
        );
        // Nothing typed: the cursor.
        close(
            target_point(
                Tool::Line,
                Some(a),
                DVec2::new(30.0, 40.0),
                &[None, None],
                false,
            )
            .unwrap(),
            DVec2::new(30.0, 40.0),
        );
    }

    #[test]
    fn rect_width_height_are_signed() {
        let a = DVec2::new(100.0, 100.0);
        close(
            target_point(Tool::Rect, Some(a), O, &[Some(300.0), Some(-50.0)], false).unwrap(),
            DVec2::new(400.0, 50.0),
        );
        close(
            target_point(
                Tool::Rect,
                Some(a),
                DVec2::new(150.0, 180.0),
                &[Some(10.0), None],
                false,
            )
            .unwrap(),
            DVec2::new(110.0, 180.0),
        );
    }

    #[test]
    fn circle_radius_keeps_cursor_direction() {
        close(
            target_point(
                Tool::Circle,
                Some(O),
                DVec2::new(0.0, 3.0),
                &[Some(500.0)],
                false,
            )
            .unwrap(),
            DVec2::new(0.0, 500.0),
        );
        close(
            target_point(Tool::Circle, Some(O), O, &[Some(500.0)], false).unwrap(),
            DVec2::new(500.0, 0.0),
        );
    }

    #[test]
    fn unsupported_tool_has_no_target() {
        assert_eq!(target_point(Tool::Text, None, O, &[], false), None);
    }

    #[test]
    fn live_values_match_cursor() {
        let v = live_values(Tool::Line, Some(O), DVec2::new(0.0, 10.0), false, 0.0);
        assert!((v[0] - 10.0).abs() < 1e-9 && (v[1] - 90.0).abs() < 1e-9);
        assert_eq!(
            live_values(Tool::Rect, None, DVec2::new(1.0, 2.0), false, 0.0),
            vec![1.0, 2.0]
        );
    }

    #[test]
    fn entry_cycles_and_resets() {
        let mut e = DataEntry::default();
        e.sync(Tool::Line, false, false);
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
        e.sync(Tool::Line, true, false);
        assert!(e.active && e.want_focus && e.focus == 0 && !e.any_typed());
        // Switching to a tool without entry deactivates.
        e.sync(Tool::Select, false, false);
        assert!(!e.active);
    }

    #[test]
    fn arc_mode_adds_a_radius_field_for_path_tools() {
        assert_eq!(
            fields(Tool::Polyline, false, true),
            &[Field::X, Field::Y, Field::ArcRadius]
        );
        assert_eq!(
            fields(Tool::Hardscape, true, true),
            &[Field::Length, Field::Angle, Field::ArcRadius]
        );
        assert_eq!(
            fields(Tool::LandscapeArea, true, false),
            &[Field::Length, Field::Angle]
        );
        // Other tools ignore the arc flag.
        assert_eq!(
            fields(Tool::Rect, true, true),
            &[Field::Width, Field::Height]
        );
        assert_eq!(Field::ArcRadius.unit(), "mm");
        assert_eq!(Field::ArcRadius.label(), "Arc R");
        assert!(
            supported(Tool::Hardscape) && supported(Tool::LandscapeArea) && !supported(Tool::Plant)
        );
    }

    #[test]
    fn typed_arc_radius_and_target_with_arc_field() {
        let a = DVec2::new(10.0, 10.0);
        let vals = [Some(100.0), Some(0.0), Some(250.0)];
        close(
            target_point(Tool::Polyline, Some(a), O, &vals, true).unwrap(),
            DVec2::new(110.0, 10.0),
        );
        assert_eq!(
            typed_arc_radius(Tool::Polyline, true, true, &vals),
            Some(250.0)
        );
        assert_eq!(
            typed_arc_radius(Tool::Polyline, true, true, &[None, None, None]),
            None
        );
        // A negative radius is not accepted. Zero is (it makes corners).
        assert_eq!(
            typed_arc_radius(Tool::Polyline, true, true, &[None, None, Some(-1.0)]),
            None
        );
        assert_eq!(
            typed_arc_radius(Tool::Polyline, true, true, &[None, None, Some(0.0)]),
            Some(0.0)
        );
        // No Radius field without Arc mode, or for a tool without modes.
        assert_eq!(typed_arc_radius(Tool::Polyline, true, false, &vals), None);
        assert_eq!(typed_arc_radius(Tool::Line, true, true, &vals), None);
        // Before the first click the X and Y fields still work.
        close(
            target_point(
                Tool::Hardscape,
                None,
                O,
                &[Some(5.0), Some(6.0), None],
                true,
            )
            .unwrap(),
            DVec2::new(5.0, 6.0),
        );
        let live = live_values(Tool::Polyline, None, DVec2::new(1.0, 2.0), true, 750.0);
        assert_eq!(live, vec![1.0, 2.0, 750.0]);
    }

    #[test]
    fn entry_resets_when_the_arc_mode_changes() {
        let mut e = DataEntry::default();
        e.sync(Tool::Polyline, true, false);
        assert_eq!(e.text.len(), 2);
        e.activate();
        e.sync(Tool::Polyline, true, true);
        assert_eq!(e.text.len(), 3);
        assert!(e.active && e.want_focus);
    }
}
