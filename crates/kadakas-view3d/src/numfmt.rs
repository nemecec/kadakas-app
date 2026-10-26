//! Number widgets that follow the number format of the person.
//!
//! A `DragValue` or a `Slider` of egui writes a float with a decimal point and reads only a
//! point. `.ui_number()` makes the widget write the number in the format of the person
//! (`kadakas_units::current()`: a comma or a point, no digit grouping while it is edited) and
//! read both `.` and `,` as the decimal separator, with the rules of
//! `kadakas_units::NumberFormat::parse` (`1,234` follows the format). The number of decimals
//! that the widget was given (`fixed_decimals`, `max_decimals`) stays.
//!
//! Lengths and angles have their own extension in the app (`units_ui`); this one is for plain
//! numbers: factors, opacity, scale, rotation.

use egui::{DragValue, Slider};
use std::ops::RangeInclusive;

fn text(value: f64, decimals: RangeInclusive<usize>) -> String {
    kadakas_units::num_range(value, *decimals.start(), *decimals.end())
}

fn read(text: &str) -> Option<f64> {
    kadakas_units::parse_num(text)
}

/// `.ui_number()` for the number widgets.
pub trait UiNumber: Sized {
    /// Writes and reads the number in the number format of the person.
    fn ui_number(self) -> Self;
}

impl UiNumber for DragValue<'_> {
    fn ui_number(self) -> Self {
        self.custom_formatter(text).custom_parser(read)
    }
}

impl UiNumber for Slider<'_> {
    fn ui_number(self) -> Self {
        self.custom_formatter(text).custom_parser(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_units::{Decimal, Grouping, Settings, Units};

    fn comma() -> Units {
        Settings {
            decimal: Some(Decimal::Comma),
            grouping: Some(Grouping::Space),
            ..Settings::default()
        }
        .resolve("en-US")
    }

    #[test]
    fn the_text_and_the_reading_follow_the_format() {
        kadakas_units::with_units(comma(), || {
            assert_eq!(text(1.5, 0..=3), "1,5");
            assert_eq!(text(1234.5, 2..=2), "1234,50", "no grouping while editing");
            assert_eq!(read("1,5"), Some(1.5));
            assert_eq!(read("1.5"), Some(1.5));
            assert_eq!(read("x"), None);
        });
        kadakas_units::with_units(Units::METRIC, || {
            assert_eq!(text(1.5, 0..=3), "1.5");
            assert_eq!(read("1,5"), Some(1.5));
        });
    }

    #[test]
    fn a_drag_value_and_a_slider_take_the_hooks_and_show_the_number() {
        // Both widgets accept the hooks; one frame draws them without a panic and a typed
        // comma reads back as a number.
        let ctx = egui::Context::default();
        let mut a = 1.5_f64;
        let mut b = 0.25_f64;
        kadakas_units::with_units(comma(), || {
            let _ = ctx.run_ui(Default::default(), |ui| {
                ui.add(DragValue::new(&mut a).max_decimals(3).ui_number());
                ui.add(Slider::new(&mut b, 0.0..=1.0).fixed_decimals(2).ui_number());
            });
        });
        assert_eq!((a, b), (1.5, 0.25));
    }
}
