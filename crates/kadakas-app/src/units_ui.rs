//! Number widgets in the units of the person (PLAT-05). The document keeps millimeters and
//! degrees; these helpers show and read them in the units and the number format that the
//! preferences give (`kadakas_units::current()`).

use egui::DragValue;
use kadakas_units::{AreaUnit, LengthUnit};

/// Extra methods of `DragValue` for values with a unit.
pub trait DragValueExt: Sized {
    /// The value is a length in mm.
    fn length(self) -> Self;
    /// The value is a length in meters.
    fn length_m(self) -> Self;
    /// The value is an angle in degrees.
    fn degrees(self) -> Self;
    /// The value is a number per square meter (a plant density).
    fn per_area(self) -> Self;
    /// The value is an area in m².
    fn area_m2(self) -> Self;
    /// The value is a volume in m³.
    fn volume_m3(self) -> Self;
}

impl DragValueExt for DragValue<'_> {
    fn length(self) -> Self {
        self.custom_formatter(|mm, _| kadakas_units::current().length_compact(mm))
            .custom_parser(|text| kadakas_units::current().parse_length(text))
    }

    fn length_m(self) -> Self {
        self.custom_formatter(|m, _| kadakas_units::current().length_compact(m * 1000.0))
            .custom_parser(|text| {
                kadakas_units::current()
                    .parse_length(text)
                    .map(|mm| mm / 1000.0)
            })
    }

    fn degrees(self) -> Self {
        self.custom_formatter(|deg, _| kadakas_units::current().angle_compact(deg))
            .custom_parser(|text| kadakas_units::current().parse_angle(text))
    }

    fn per_area(self) -> Self {
        self.custom_formatter(|per_m2, _| format_per_area(per_m2))
            .custom_parser(parse_per_area)
    }

    fn area_m2(self) -> Self {
        self.custom_formatter(|m2, _| kadakas_units::current().area_compact(m2 * 1.0e6))
            .custom_parser(|text| {
                kadakas_units::current()
                    .parse_area(text)
                    .map(|mm2| mm2 / 1.0e6)
            })
    }

    fn volume_m3(self) -> Self {
        self.custom_formatter(|m3, _| kadakas_units::current().volume_compact(m3 * 1.0e9))
            .custom_parser(|text| {
                kadakas_units::current()
                    .parse_volume(text)
                    .map(|mm3| mm3 / 1.0e9)
            })
    }
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
    use kadakas_units::{Settings, Units, with_units};

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
                let mut dens = 2.0;
                let (mut a, mut v) = (12.5, 3.0);
                let _ = ctx.run_ui(Default::default(), |ui| {
                    ui.add(DragValue::new(&mut a).area_m2());
                    ui.add(DragValue::new(&mut v).volume_m3());
                    ui.add(DragValue::new(&mut mm).length());
                    ui.add(DragValue::new(&mut m).length_m());
                    ui.add(DragValue::new(&mut deg).degrees());
                    ui.add(DragValue::new(&mut dens).per_area());
                });
            });
        }
    }
}
