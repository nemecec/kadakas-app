//! Number widgets in the units of the person (PLAT-05). The document keeps millimeters and
//! degrees. These helpers show and read them in the units and the number format that the
//! preferences give (`kadakas_units::current()`).
//!
//! egui reads the text of a `DragValue` again when the field loses focus. A text with rounded
//! numbers (`39 3/8"`) would then change the value (1000.125 mm for 1000 mm). So each widget
//! remembers the text that it showed last and the exact value of that text. The parser gives the
//! exact value back when the person did not change the text.

use std::cell::RefCell;
use std::rc::Rc;

use egui::DragValue;
use kadakas_units::{AreaUnit, LengthUnit};

/// The text and the value that a widget showed last.
type Last = Rc<RefCell<Option<(String, f64)>>>;

/// Turns text into a value.
type Parser = Box<dyn Fn(&str) -> Option<f64>>;

/// A formatter and a parser that agree: the parser returns the exact value for the text that the
/// formatter made last. For other text it uses `parse`.
pub struct Memo {
    last: Last,
    format: Box<dyn Fn(f64) -> String>,
    parse: Parser,
}

impl Memo {
    pub fn new(
        format: impl Fn(f64) -> String + 'static,
        parse: impl Fn(&str) -> Option<f64> + 'static,
    ) -> Memo {
        Memo {
            last: Rc::new(RefCell::new(None)),
            format: Box::new(format),
            parse: Box::new(parse),
        }
    }

    /// The text for a value. The memo keeps the pair.
    pub fn format(&self, value: f64) -> String {
        let text = (self.format)(value);
        *self.last.borrow_mut() = Some((text.clone(), value));
        text
    }

    /// The value of a text. Text that is the text of the last value gives that value.
    pub fn parse(&self, text: &str) -> Option<f64> {
        if let Some((last, value)) = &*self.last.borrow()
            && last == text
        {
            return Some(*value);
        }
        (self.parse)(text)
    }
}

/// Extra methods of `DragValue` for values with a unit.
pub trait DragValueExt: Sized {
    /// The value is a length in mm.
    fn length(self) -> Self;
    /// The value is a length in meters.
    fn length_m(self) -> Self;
    /// The value is a length on paper in mm: a line weight, a text height, a symbol size. It
    /// shows in mm, whatever the length unit of the person is.
    fn paper_mm(self) -> Self;
    /// The value is an angle in degrees that is not a direction (a step, a sweep, a pitch, 360
    /// degrees). A bearing becomes degrees-minutes-seconds.
    fn degrees(self) -> Self;
    /// The value is a direction in degrees: a rotation, a direction of fall. It follows the
    /// angle format, including the bearing.
    fn direction(self) -> Self;
    /// The value is a number per square meter (a plant density).
    fn per_area(self) -> Self;
    /// The value is an area in m².
    fn area_m2(self) -> Self;
    /// The value is a volume in m³.
    fn volume_m3(self) -> Self;
}

fn with_memo(drag: DragValue<'_>, memo: Memo) -> DragValue<'_> {
    let memo = Rc::new(memo);
    let parser = memo.clone();
    drag.custom_formatter(move |v, _| memo.format(v))
        .custom_parser(move |t| parser.parse(t))
}

impl DragValueExt for DragValue<'_> {
    fn length(self) -> Self {
        with_memo(
            self,
            Memo::new(
                |mm| kadakas_units::current().length_compact(mm),
                |t| kadakas_units::current().parse_length(t),
            ),
        )
    }

    fn length_m(self) -> Self {
        with_memo(
            self,
            Memo::new(
                |m| kadakas_units::current().length_compact(m * 1000.0),
                |t| {
                    kadakas_units::current()
                        .parse_length(t)
                        .map(|mm| mm / 1000.0)
                },
            ),
        )
    }

    fn paper_mm(self) -> Self {
        with_memo(self, Memo::new(format_paper_mm, parse_paper_mm))
    }

    fn degrees(self) -> Self {
        with_memo(
            self,
            Memo::new(
                |d| kadakas_units::current().plain_angle().angle_compact(d),
                |t| kadakas_units::current().parse_angle(t),
            ),
        )
    }

    fn direction(self) -> Self {
        with_memo(
            self,
            Memo::new(
                |d| kadakas_units::current().angle_compact(d),
                |t| kadakas_units::current().parse_angle(t),
            ),
        )
    }

    fn per_area(self) -> Self {
        with_memo(self, Memo::new(format_per_area, parse_per_area))
    }

    fn area_m2(self) -> Self {
        with_memo(
            self,
            Memo::new(
                |m2| kadakas_units::current().area_compact(m2 * 1.0e6),
                |t| {
                    kadakas_units::current()
                        .parse_area(t)
                        .map(|mm2| mm2 / 1.0e6)
                },
            ),
        )
    }

    fn volume_m3(self) -> Self {
        with_memo(
            self,
            Memo::new(
                |m3| kadakas_units::current().volume_compact(m3 * 1.0e9),
                |t| {
                    kadakas_units::current()
                        .parse_volume(t)
                        .map(|mm3| mm3 / 1.0e9)
                },
            ),
        )
    }
}

/// A length on paper: `0.25 mm`, with the decimal separator of the person and no grouping.
pub fn format_paper_mm(mm: f64) -> String {
    if !mm.is_finite() {
        return "\u{2013}".to_string();
    }
    format!(
        "{} mm",
        kadakas_units::current().number.plain().format_trim(mm, 3)
    )
}

/// Reads a length on paper. A number without a unit counts in mm.
pub fn parse_paper_mm(text: &str) -> Option<f64> {
    kadakas_units::parse_length(
        text,
        LengthUnit::Mm,
        &kadakas_units::current().number.plain(),
    )
}

/// The area unit that a density uses: per m² for metric lengths, per ft² for imperial ones.
fn density_unit(length: LengthUnit) -> AreaUnit {
    if length.is_imperial() {
        AreaUnit::Ft2
    } else {
        AreaUnit::M2
    }
}

/// A density (number per m²) in the units of the person: `2.5 /m²` or `0.23 /ft²`.
pub fn format_per_area(per_m2: f64) -> String {
    let u = kadakas_units::current();
    let unit = density_unit(u.length);
    let shown = per_m2 * unit.mm2_per_unit() / 1.0e6;
    format!(
        "{} /{}",
        u.number.plain().format_trim(shown, 4),
        unit.short_name()
    )
}

/// Reads a density that a person typed and gives the number per m². The text after `/` is
/// ignored: the number counts in the density unit of the person.
pub fn parse_per_area(text: &str) -> Option<f64> {
    let u = kadakas_units::current();
    let unit = density_unit(u.length);
    let number = text.split('/').next().unwrap_or("");
    u.number
        .parse(number)
        .map(|shown| shown * 1.0e6 / unit.mm2_per_unit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_units::{AngleFormat, Settings, Units, with_units};

    /// Every unit and format: a value that a widget shows, read again from its own text, is the
    /// same value, to the last bit.
    #[test]
    fn focus_and_leave_never_changes_a_value() {
        let values = [
            0.0,
            1.0,
            12.5,
            25.4,
            304.8,
            1000.0,
            1234.5678901,
            -37.123,
            0.0001,
            99999.99,
            1.0e7,
        ];
        let mut all_units = Vec::new();
        for length in LengthUnit::ALL {
            for fraction in [None, Some(0), Some(2), Some(16), Some(64)] {
                for angle in AngleFormat::ALL {
                    for lang in ["en-US", "et"] {
                        all_units.push(
                            Settings {
                                length,
                                angle,
                                inch_fraction: fraction,
                                area: AreaUnit::Ac,
                                ..Settings::default()
                            }
                            .resolve(lang),
                        );
                    }
                }
            }
        }
        for units in all_units {
            with_units(units, || {
                let memos: Vec<(&str, Memo)> = vec![
                    (
                        "length",
                        Memo::new(
                            |v| kadakas_units::current().length_compact(v),
                            |t| kadakas_units::current().parse_length(t),
                        ),
                    ),
                    ("paper", Memo::new(format_paper_mm, parse_paper_mm)),
                    (
                        "degrees",
                        Memo::new(
                            |d| kadakas_units::current().plain_angle().angle_compact(d),
                            |t| kadakas_units::current().parse_angle(t),
                        ),
                    ),
                    (
                        "direction",
                        Memo::new(
                            |d| kadakas_units::current().angle_compact(d),
                            |t| kadakas_units::current().parse_angle(t),
                        ),
                    ),
                    ("per_area", Memo::new(format_per_area, parse_per_area)),
                    (
                        "area",
                        Memo::new(
                            |m2| kadakas_units::current().area_compact(m2 * 1.0e6),
                            |t| kadakas_units::current().parse_area(t).map(|v| v / 1.0e6),
                        ),
                    ),
                    (
                        "volume",
                        Memo::new(
                            |m3| kadakas_units::current().volume_compact(m3 * 1.0e9),
                            |t| kadakas_units::current().parse_volume(t).map(|v| v / 1.0e9),
                        ),
                    ),
                ];
                for (name, memo) in &memos {
                    for v in values {
                        let text = memo.format(v);
                        let back = memo.parse(&text);
                        assert_eq!(
                            back.map(f64::to_bits),
                            Some(v.to_bits()),
                            "{name}: {v} -> {text:?} in {units:?}"
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn changed_text_is_parsed_normally() {
        let memo = Memo::new(
            |v| format!("{v}!"),
            |t| t.trim_end_matches('!').parse().ok(),
        );
        assert_eq!(memo.parse("7"), Some(7.0)); // nothing formatted yet
        assert_eq!(memo.format(2.5), "2.5!");
        assert_eq!(memo.parse("2.5!"), Some(2.5));
        assert_eq!(memo.parse("3!"), Some(3.0));
    }

    #[test]
    fn paper_values_stay_in_millimeters() {
        let imperial = Units {
            length: LengthUnit::FtIn,
            ..Units::METRIC
        };
        with_units(imperial, || {
            assert_eq!(format_paper_mm(0.25), "0.25 mm");
            assert_eq!(parse_paper_mm("0.25"), Some(0.25));
            assert_eq!(parse_paper_mm("0.25 mm"), Some(0.25));
        });
        with_units(Settings::default().resolve("et"), || {
            assert_eq!(format_paper_mm(0.25), "0,25 mm");
            assert_eq!(parse_paper_mm("0,25"), Some(0.25));
        });
        assert_eq!(format_paper_mm(f64::NAN), "\u{2013}");
    }

    #[test]
    fn a_bearing_shows_only_for_directions() {
        let mut u = Units::METRIC;
        u.angle = AngleFormat::Bearing;
        with_units(u, || {
            let plain = Memo::new(
                |d| kadakas_units::current().plain_angle().angle_compact(d),
                |_| None,
            );
            assert_eq!(plain.format(360.0), "360\u{b0}00'00.0\"");
            assert_eq!(plain.format(-45.0), "-45\u{b0}00'00.0\"");
            let dir = kadakas_units::current().angle_compact(45.0);
            assert!(dir.starts_with("N 45"), "{dir}");
        });
    }

    #[test]
    fn densities_follow_the_length_system() {
        assert_eq!(density_unit(LengthUnit::M), AreaUnit::M2);
        assert_eq!(density_unit(LengthUnit::FtIn), AreaUnit::Ft2);
        with_units(Settings::default().resolve("et"), || {
            assert_eq!(format_per_area(2.5), "2,5 /m\u{b2}");
            assert_eq!(parse_per_area("2,5 /m2"), Some(2.5));
            assert_eq!(parse_per_area("3"), Some(3.0));
            assert_eq!(parse_per_area("abc"), None);
        });
        let imperial = Units {
            length: LengthUnit::Ft,
            ..Units::METRIC
        };
        with_units(imperial, || {
            let text = format_per_area(10.0);
            assert!(text.ends_with(" /ft\u{b2}"), "{text}");
            let back = parse_per_area(&text).unwrap();
            assert!((back - 10.0).abs() < 1e-3, "{back}");
        });
    }

    #[test]
    fn widgets_build_with_every_kind_of_value() {
        let ctx = egui::Context::default();
        let units = [Units::METRIC, Settings::default().resolve("et")];
        for u in units {
            with_units(u, || {
                let mut mm = 1234.5;
                let mut m = 1.2;
                let mut deg = 45.0;
                let mut dir = 45.0;
                let mut dens = 2.0;
                let mut paper = 0.25;
                let (mut a, mut v) = (12.5, 3.0);
                let _ = ctx.run_ui(Default::default(), |ui| {
                    ui.add(DragValue::new(&mut a).area_m2());
                    ui.add(DragValue::new(&mut v).volume_m3());
                    ui.add(DragValue::new(&mut mm).length());
                    ui.add(DragValue::new(&mut m).length_m());
                    ui.add(DragValue::new(&mut deg).degrees());
                    ui.add(DragValue::new(&mut dir).direction());
                    ui.add(DragValue::new(&mut dens).per_area());
                    ui.add(DragValue::new(&mut paper).paper_mm());
                });
            });
        }
    }
}
