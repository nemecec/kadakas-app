//! The choices of the welcome dialog, and the defaults from the system.

use crate::sysloc::SystemLocale;
use kadakas_feedback::CrashPolicy;
use kadakas_units::{AreaUnit, Decimal, Grouping, LengthUnit, NumberFormat, Settings};
use kadakas_update::settings::{MAIN_FLAVOR, UpdateMode};

/// The id of the flavor of the 3D modeler.
pub const MODELER_FLAVOR: &str = "kadakas-modeler";

/// How the person wants to work. It decides the program that the launcher starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Simple push and pull shape modeling.
    #[default]
    Modeler,
    /// Plans, sheets and plants.
    Landscape,
}

impl Mode {
    pub const ALL: [Mode; 2] = [Mode::Modeler, Mode::Landscape];

    /// The flavor that the launcher starts.
    pub fn flavor(self) -> &'static str {
        match self {
            Mode::Modeler => MODELER_FLAVOR,
            Mode::Landscape => MAIN_FLAVOR,
        }
    }

    pub fn from_flavor(flavor: &str) -> Mode {
        if flavor == MODELER_FLAVOR {
            Mode::Modeler
        } else {
            Mode::Landscape
        }
    }
}

/// Metric or imperial lengths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnitSystem {
    /// Millimeters and meters.
    #[default]
    Metric,
    /// Feet and inches.
    Imperial,
}

impl UnitSystem {
    pub const ALL: [UnitSystem; 2] = [UnitSystem::Metric, UnitSystem::Imperial];

    pub fn length(self) -> LengthUnit {
        match self {
            UnitSystem::Metric => LengthUnit::Mm,
            UnitSystem::Imperial => LengthUnit::FtIn,
        }
    }

    pub fn area(self) -> AreaUnit {
        match self {
            UnitSystem::Metric => AreaUnit::M2,
            UnitSystem::Imperial => AreaUnit::Ft2,
        }
    }

    pub fn from_length(length: LengthUnit) -> UnitSystem {
        match length {
            LengthUnit::In | LengthUnit::Ft | LengthUnit::FtIn => UnitSystem::Imperial,
            LengthUnit::Mm | LengthUnit::Cm | LengthUnit::M => UnitSystem::Metric,
        }
    }
}

/// How numbers are written: the decimal separator and the digit grouping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NumberStyle {
    /// 1,234.56
    #[default]
    CommaDot,
    /// 1.234,56
    DotComma,
    /// 1 234,56 (Estonian, French)
    SpaceComma,
    /// 1’234.56 (Swiss)
    ApostropheDot,
    /// 1234.56
    PlainDot,
    /// 1234,56
    PlainComma,
}

impl NumberStyle {
    pub const ALL: [NumberStyle; 6] = [
        NumberStyle::CommaDot,
        NumberStyle::DotComma,
        NumberStyle::SpaceComma,
        NumberStyle::ApostropheDot,
        NumberStyle::PlainDot,
        NumberStyle::PlainComma,
    ];

    pub fn decimal(self) -> Decimal {
        match self {
            NumberStyle::CommaDot | NumberStyle::PlainDot | NumberStyle::ApostropheDot => {
                Decimal::Point
            }
            _ => Decimal::Comma,
        }
    }

    pub fn grouping(self) -> Grouping {
        match self {
            NumberStyle::CommaDot => Grouping::Comma,
            NumberStyle::DotComma => Grouping::Point,
            NumberStyle::SpaceComma => Grouping::Space,
            NumberStyle::ApostropheDot => Grouping::Apostrophe,
            NumberStyle::PlainDot | NumberStyle::PlainComma => Grouping::None,
        }
    }

    /// The style of a stored pair; `None` for a pair that the dialog does not offer.
    pub fn from_pair(decimal: Decimal, grouping: Grouping) -> Option<NumberStyle> {
        NumberStyle::ALL
            .into_iter()
            .find(|s| s.decimal() == decimal && s.grouping() == grouping)
    }

    /// The style that is nearest to a format: the pair itself if the dialog offers it, else the
    /// style with the same decimal separator.
    pub fn nearest(f: NumberFormat) -> NumberStyle {
        NumberStyle::from_pair(f.decimal, f.grouping).unwrap_or(match (f.decimal, f.grouping) {
            (Decimal::Point, Grouping::None) => NumberStyle::PlainDot,
            (Decimal::Point, _) => NumberStyle::CommaDot,
            (Decimal::Comma, Grouping::None) => NumberStyle::PlainComma,
            (Decimal::Comma, Grouping::Point) => NumberStyle::DotComma,
            (Decimal::Comma, _) => NumberStyle::SpaceComma,
        })
    }

    /// The style for the separators that the system reports (the decimal symbol and the grouping
    /// symbol, `None` for no grouping). `None` when the pair is not one of ours.
    pub fn from_symbols(decimal: char, grouping: Option<char>) -> Option<NumberStyle> {
        let decimal = match decimal {
            '.' => Decimal::Point,
            ',' => Decimal::Comma,
            _ => return None,
        };
        let grouping = match grouping {
            None => Grouping::None,
            Some(',') => Grouping::Comma,
            Some('.') => Grouping::Point,
            Some(' ' | '\u{a0}' | '\u{202f}' | '\u{2009}') => Grouping::Space,
            Some('\'' | '\u{2019}') => Grouping::Apostrophe,
            Some(_) => return None,
        };
        NumberStyle::from_pair(decimal, grouping)
    }

    /// The format of the style, for the example line.
    pub fn format(self) -> NumberFormat {
        NumberFormat {
            decimal: self.decimal(),
            grouping: self.grouping(),
        }
    }

    /// The number that the dialog shows as the example of the style: `1,234.56`.
    pub fn example(self) -> String {
        self.format().format(1234.56, 2)
    }
}

/// The color theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    /// The text in `prefs.json` and `modeler.json`.
    pub fn key(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    pub fn from_key(key: &str) -> Theme {
        Theme::ALL
            .into_iter()
            .find(|t| t.key() == key)
            .unwrap_or_default()
    }
}

/// The orbit, pan and zoom style of the 3D views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Navigation {
    #[default]
    Mouse,
    Trackpad,
}

impl Navigation {
    pub const ALL: [Navigation; 2] = [Navigation::Mouse, Navigation::Trackpad];

    /// The text in `prefs.json` (`kadakas_ui_core::nav3d::NavStyle` reads the same text).
    pub fn key(self) -> &'static str {
        match self {
            Navigation::Mouse => "mouse",
            Navigation::Trackpad => "trackpad",
        }
    }

    pub fn from_key(key: &str) -> Navigation {
        Navigation::ALL
            .into_iter()
            .find(|n| n.key() == key)
            .unwrap_or_default()
    }
}

/// Everything that the dialog asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
    pub mode: Mode,
    /// The tag of a language, or `None` to follow the language of the system.
    pub language: Option<String>,
    pub units: UnitSystem,
    pub number: NumberStyle,
    pub theme: Theme,
    pub navigation: Navigation,
    pub crash: CrashPolicy,
    pub updates: UpdateMode,
}

impl Choices {
    /// The language that the dialog shows now: the chosen one, or the one that the system asks for.
    pub fn language_tag(&self, system: &[String]) -> &'static str {
        match &self.language {
            Some(t) => kadakas_l10n::negotiate(&[t.as_str()]),
            None => kadakas_l10n::negotiate(system),
        }
    }

    /// The units that the choices give, for the examples (in the language of the dialog).
    pub fn unit_settings(&self) -> Settings {
        Settings {
            length: self.units.length(),
            area: self.units.area(),
            decimal: Some(self.number.decimal()),
            grouping: Some(self.number.grouping()),
            ..Settings::default()
        }
    }

    /// The example line for the units: ten feet in the chosen units and number style.
    pub fn length_example(&self, language: &str) -> String {
        self.unit_settings()
            .resolve(language)
            .length_text_d(3048.0, 0)
    }
}

/// The defaults of a first start, from what the system reports (the most wanted language first).
pub fn detect(system: &SystemLocale, modeler_installed: bool) -> Choices {
    Choices {
        mode: if modeler_installed {
            Mode::Modeler
        } else {
            Mode::Landscape
        },
        language: None,
        units: system.units(),
        number: system.number_style(),
        theme: Theme::System,
        navigation: Navigation::Mouse,
        crash: CrashPolicy::Ask,
        updates: UpdateMode::AutoUpdate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(list: &[&str]) -> SystemLocale {
        SystemLocale::from_languages(list.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn the_defaults_depend_on_the_system_only() {
        let c = detect(&sys(&["et-EE"]), true);
        assert_eq!(
            (c.mode, c.units, c.number, c.language.clone()),
            (
                Mode::Modeler,
                UnitSystem::Metric,
                NumberStyle::SpaceComma,
                None
            )
        );
        assert_eq!(c.theme, Theme::System);
        assert_eq!(c.navigation, Navigation::Mouse);
        assert_eq!(c.crash, CrashPolicy::Ask);
        assert_eq!(c.updates, UpdateMode::AutoUpdate);
        let c = detect(&sys(&["en-US"]), false);
        assert_eq!((c.mode, c.units), (Mode::Landscape, UnitSystem::Imperial));
        let list = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(c.language_tag(&list(&["et_EE.UTF-8"])), "et");
        assert_eq!(c.language_tag(&list(&["fi"])), "en-US");
        let fixed = Choices {
            language: Some("et".into()),
            ..c
        };
        assert_eq!(fixed.language_tag(&list(&["en-US"])), "et");
        // The United Kingdom is metric.
        assert_eq!(detect(&sys(&["en-GB"]), true).units, UnitSystem::Metric);
    }

    #[test]
    fn modes_units_and_styles_map_to_their_stored_forms() {
        assert_eq!(Mode::Modeler.flavor(), "kadakas-modeler");
        assert_eq!(Mode::Landscape.flavor(), "kadakas-app");
        assert_eq!(Mode::from_flavor("kadakas-modeler"), Mode::Modeler);
        assert_eq!(Mode::from_flavor("kadakas-app"), Mode::Landscape);
        assert_eq!(Mode::from_flavor("sketch"), Mode::Landscape);
        for u in UnitSystem::ALL {
            assert_eq!(UnitSystem::from_length(u.length()), u);
        }
        for l in [LengthUnit::In, LengthUnit::Ft, LengthUnit::FtIn] {
            assert_eq!(UnitSystem::from_length(l), UnitSystem::Imperial);
        }
        for l in [LengthUnit::Mm, LengthUnit::Cm, LengthUnit::M] {
            assert_eq!(UnitSystem::from_length(l), UnitSystem::Metric);
        }
        assert_eq!(UnitSystem::Imperial.area(), AreaUnit::Ft2);
        assert_eq!(UnitSystem::Metric.area(), AreaUnit::M2);
        for s in NumberStyle::ALL {
            assert_eq!(NumberStyle::from_pair(s.decimal(), s.grouping()), Some(s));
            assert_eq!(NumberStyle::nearest(s.format()), s);
        }
        assert_eq!(
            NumberStyle::from_pair(Decimal::Point, Grouping::Point),
            None
        );
        // A pair that is not offered gets the style with the same decimal separator.
        let n = |d, g| {
            NumberStyle::nearest(NumberFormat {
                decimal: d,
                grouping: g,
            })
        };
        assert_eq!(n(Decimal::Point, Grouping::Space), NumberStyle::CommaDot);
        assert_eq!(n(Decimal::Comma, Grouping::Comma), NumberStyle::SpaceComma);
        assert_eq!(
            n(Decimal::Comma, Grouping::Apostrophe),
            NumberStyle::SpaceComma
        );
        for t in Theme::ALL {
            assert_eq!(Theme::from_key(t.key()), t);
        }
        assert_eq!(Theme::from_key("neon"), Theme::System);
        for n in Navigation::ALL {
            assert_eq!(Navigation::from_key(n.key()), n);
        }
        assert_eq!(Navigation::from_key("pen"), Navigation::Mouse);
    }

    #[test]
    fn the_examples_show_each_style_and_both_unit_systems() {
        let want = [
            "1,234.56",
            "1.234,56",
            "1\u{a0}234,56",
            "1\u{2019}234.56",
            "1234.56",
            "1234,56",
        ];
        for (s, w) in NumberStyle::ALL.into_iter().zip(want) {
            assert_eq!(s.example(), w);
        }
        let mut c = detect(&sys(&[]), true);
        c.number = NumberStyle::PlainDot;
        assert_eq!(c.length_example("en-US"), "3048 mm");
        c.units = UnitSystem::Imperial;
        assert_eq!(c.length_example("en-US"), "10' 0\"");
        c.units = UnitSystem::Metric;
        c.number = NumberStyle::CommaDot;
        assert_eq!(c.length_example("et"), "3,048 mm");
        // The negative number and large or small values follow the same rules.
        let f = NumberStyle::SpaceComma.format();
        assert_eq!(f.format(-1234567.891, 2), "-1\u{a0}234\u{a0}567,89");
        assert_eq!(f.format(0.5, 2), "0,50");
        assert_eq!(NumberStyle::CommaDot.format().format(1234.0, 0), "1,234");
        assert_eq!(NumberStyle::CommaDot.format().format(999.0, 0), "999");
        assert_eq!(NumberStyle::DotComma.format().format(12345.0, 0), "12.345");
        assert_eq!(
            NumberStyle::ApostropheDot.format().format(-12345.5, 1),
            "-12\u{2019}345.5"
        );
    }

    #[test]
    fn typed_numbers_read_back_in_every_style_and_the_ambiguous_one_follows_the_style() {
        for style in NumberStyle::ALL {
            let f = style.format();
            for v in [
                0.0, 0.5, -0.5, 7.25, 999.99, 1234.56, -1234.56, 12345.67, 1234567.89, -0.01,
            ] {
                let text = f.format(v, 2);
                assert_eq!(f.parse(&text), Some(v), "{style:?} {text}");
            }
            // Both symbols are accepted for a decimal that cannot be a group.
            assert_eq!(f.parse("1,5"), Some(1.5), "{style:?}");
            assert_eq!(f.parse("1.5"), Some(1.5), "{style:?}");
        }
        // `1,234`: a thousand with the comma grouping, one and a bit with the decimal comma.
        assert_eq!(NumberStyle::CommaDot.format().parse("1,234"), Some(1234.0));
        assert!(NumberStyle::CommaDot.format().is_ambiguous("1,234"));
        assert_eq!(NumberStyle::SpaceComma.format().parse("1,234"), Some(1.234));
        assert_eq!(NumberStyle::PlainComma.format().parse("1,234"), Some(1.234));
        assert!(!NumberStyle::SpaceComma.format().is_ambiguous("1,234"));
        assert_eq!(NumberStyle::DotComma.format().parse("1.234"), Some(1234.0));
        assert!(NumberStyle::DotComma.format().is_ambiguous("1.234"));
    }
}
