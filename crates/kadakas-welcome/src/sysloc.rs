//! What the system says about the region and the number format of the person.
//!
//! The language of the user interface and the "regional formats" are two settings on every
//! system: an Estonian can use an English Windows and still have Estonian formats. So the units
//! and the number format come from the formats locale, not from the list of interface
//! languages, and the system's own answer (metric or imperial, the decimal and thousands
//! symbols) wins over the table of languages.
//!
//! - Windows: `GetUserDefaultLocaleName`, and `GetLocaleInfoEx` with `LOCALE_IMEASURE`,
//!   `LOCALE_SDECIMAL`, `LOCALE_STHOUSAND` (the user's own overrides count).
//! - macOS: `NSLocale.currentLocale` (the region and formats of System Settings): its
//!   identifier, `usesMetricSystem`, and the decimal and grouping separators.
//! - Linux and the rest: `LC_ALL`, then `LC_MEASUREMENT` or `LC_NUMERIC`, then `LANG`. The
//!   first that is set and is not `C` or `POSIX` names the locale.
//!
//! Every reader has a pure form that takes the answers as values, so the tests need no system.

use crate::choices::{NumberStyle, UnitSystem};

/// The answers of the system.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemLocale {
    /// The languages of the user interface, the most wanted first.
    pub languages: Vec<String>,
    /// The locale of the regional formats (`et-EE`), if the system names it.
    pub format_locale: Option<String>,
    /// The system says that it measures in meters (`true`) or in feet and inches (`false`).
    pub metric: Option<bool>,
    /// The decimal symbol that the system reports.
    pub decimal: Option<char>,
    /// The grouping symbol that the system reports: `Some(None)` is "no grouping".
    pub grouping: Option<Option<char>>,
}

impl SystemLocale {
    /// Only the interface languages are known (the system gave nothing else).
    pub fn from_languages(languages: Vec<String>) -> SystemLocale {
        SystemLocale {
            languages,
            ..SystemLocale::default()
        }
    }

    /// The locale that decides the region: the formats locale, else the first language.
    fn region_locale(&self) -> Option<&str> {
        self.format_locale
            .as_deref()
            .or_else(|| self.languages.first().map(String::as_str))
    }

    /// Metric or imperial. The answer of the system first, then the region of the formats
    /// locale, then the region of the first language. No region: metric. The United Kingdom is
    /// metric here (a decision of the project); feet and inches are for the United States,
    /// Liberia and Myanmar.
    pub fn units(&self) -> UnitSystem {
        if let Some(metric) = self.metric {
            return if metric {
                UnitSystem::Metric
            } else {
                UnitSystem::Imperial
            };
        }
        match self.region_locale().and_then(kadakas_units::region_of) {
            Some(r) if kadakas_units::imperial_region(&r) => UnitSystem::Imperial,
            _ => UnitSystem::Metric,
        }
    }

    /// The number style. The separators that the system reports first (when the pair is one of
    /// ours), then the table of `kadakas_units::number_convention` for the formats locale.
    pub fn number_style(&self) -> NumberStyle {
        if let (Some(d), Some(g)) = (self.decimal, self.grouping)
            && let Some(style) = NumberStyle::from_symbols(d, g)
        {
            return style;
        }
        let locale = self.region_locale().unwrap_or_default();
        NumberStyle::nearest(kadakas_units::number_convention(locale))
    }
}

/// A POSIX locale variable that names a locale: set, not empty, not `C` or `POSIX`.
fn named(value: Option<String>) -> Option<String> {
    value.filter(|v| {
        let base = v.split(['.', '@']).next().unwrap_or_default();
        !(base.is_empty() || base.eq_ignore_ascii_case("c") || base.eq_ignore_ascii_case("posix"))
    })
}

/// Linux and the other Unix systems, from the environment (`LC_ALL` before `LC_MEASUREMENT` or
/// `LC_NUMERIC` before `LANG`).
pub fn from_posix_env(
    env: impl Fn(&str) -> Option<String>,
    languages: Vec<String>,
) -> SystemLocale {
    let pick = |names: &[&str]| names.iter().find_map(|n| named(env(n)));
    let measurement = pick(&["LC_ALL", "LC_MEASUREMENT", "LANG"]);
    let numeric = pick(&["LC_ALL", "LC_NUMERIC", "LANG"]);
    let metric = measurement
        .as_deref()
        .and_then(kadakas_units::region_of)
        .map(|r| !kadakas_units::imperial_region(&r));
    SystemLocale {
        languages,
        format_locale: numeric.or(measurement),
        metric,
        decimal: None,
        grouping: None,
    }
}

/// Windows, from the answers of `GetUserDefaultLocaleName` and `GetLocaleInfoEx`. `measure` is
/// `LOCALE_IMEASURE` ("0" metric, "1" US).
pub fn from_windows(
    name: Option<String>,
    measure: Option<String>,
    decimal: Option<String>,
    thousand: Option<String>,
    languages: Vec<String>,
) -> SystemLocale {
    let one = |s: Option<String>| s.and_then(|s| s.chars().next());
    let decimal_symbol = one(decimal);
    // An empty thousands symbol means no grouping.
    let grouping = thousand.map(|t| t.chars().next());
    SystemLocale {
        languages,
        format_locale: name.filter(|n| !n.is_empty()),
        metric: measure.and_then(|m| match m.trim() {
            "0" => Some(true),
            "1" => Some(false),
            _ => None,
        }),
        decimal: decimal_symbol,
        grouping: decimal_symbol.and(grouping),
    }
}

/// macOS, from `NSLocale.currentLocale`.
pub fn from_apple(
    identifier: Option<String>,
    metric: Option<bool>,
    decimal: Option<String>,
    grouping: Option<String>,
    languages: Vec<String>,
) -> SystemLocale {
    let decimal_symbol = decimal.and_then(|s| s.chars().next());
    SystemLocale {
        languages,
        format_locale: identifier.filter(|n| !n.is_empty()),
        metric,
        decimal: decimal_symbol,
        grouping: decimal_symbol.map(|_| grouping.and_then(|s| s.chars().next())),
    }
}

/// Reads the system now.
pub fn read_system() -> SystemLocale {
    let languages: Vec<String> = sys_locale::get_locales().collect();
    #[cfg(windows)]
    {
        windows::read(languages)
    }
    #[cfg(target_os = "macos")]
    {
        apple::read(languages)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        from_posix_env(|k| std::env::var(k).ok(), languages)
    }
}

#[cfg(target_os = "macos")]
mod apple {
    use super::{SystemLocale, from_apple};
    use objc2_foundation::NSLocale;

    pub fn read(languages: Vec<String>) -> SystemLocale {
        let locale = NSLocale::currentLocale();
        from_apple(
            Some(locale.localeIdentifier().to_string()),
            Some(locale.usesMetricSystem()),
            Some(locale.decimalSeparator().to_string()),
            Some(locale.groupingSeparator().to_string()),
            languages,
        )
    }
}

#[cfg(windows)]
mod windows {
    use super::{SystemLocale, from_windows};

    const LOCALE_NAME_MAX_LENGTH: usize = 85;
    const LOCALE_IMEASURE: u32 = 0x0D;
    const LOCALE_SDECIMAL: u32 = 0x0E;
    const LOCALE_STHOUSAND: u32 = 0x0F;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
        fn GetLocaleInfoEx(name: *const u16, lctype: u32, data: *mut u16, len: i32) -> i32;
    }

    fn text(buffer: &[u16], written: i32) -> Option<String> {
        // `written` counts the terminating zero.
        let n = usize::try_from(written).ok()?.checked_sub(1)?;
        String::from_utf16(buffer.get(..n)?).ok()
    }

    pub fn read(languages: Vec<String>) -> SystemLocale {
        let mut name = [0u16; LOCALE_NAME_MAX_LENGTH];
        // SAFETY: the buffer is as long as the length that we pass.
        let written = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
        let locale = if written > 0 {
            text(&name, written)
        } else {
            None
        };
        let info = |lctype: u32| -> Option<String> {
            let mut data = [0u16; 16];
            // SAFETY: `name` ends with a zero (it came from the system), and the buffer is as
            // long as the length that we pass.
            let written = unsafe {
                GetLocaleInfoEx(name.as_ptr(), lctype, data.as_mut_ptr(), data.len() as i32)
            };
            if written > 0 {
                text(&data, written)
            } else {
                None
            }
        };
        if locale.is_none() {
            return SystemLocale::from_languages(languages);
        }
        from_windows(
            locale,
            info(LOCALE_IMEASURE),
            info(LOCALE_SDECIMAL),
            info(LOCALE_STHOUSAND),
            languages,
        )
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_text_of_a_buffer_ends_at_the_written_length() {
            let buf: Vec<u16> = "et-EE\0xx".encode_utf16().collect();
            assert_eq!(super::text(&buf, 6).as_deref(), Some("et-EE"));
            assert_eq!(super::text(&buf, 0), None);
            assert_eq!(super::text(&buf, 99), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn langs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn an_estonian_with_an_english_interface_gets_metric_and_estonian_numbers() {
        // GNOME: language English, formats Estonia.
        let s = from_posix_env(
            env(&[
                ("LANG", "en_US.UTF-8"),
                ("LC_MEASUREMENT", "et_EE.UTF-8"),
                ("LC_NUMERIC", "et_EE.UTF-8"),
            ]),
            langs(&["en-US"]),
        );
        assert_eq!(s.units(), UnitSystem::Metric);
        assert_eq!(s.number_style(), NumberStyle::SpaceComma);
        // The other way round: Estonian interface, American formats.
        let s = from_posix_env(
            env(&[("LANG", "et_EE.UTF-8"), ("LC_ALL", "")]),
            langs(&["et-EE"]),
        );
        assert_eq!(s.units(), UnitSystem::Metric);
        let s = from_posix_env(
            env(&[("LANG", "et_EE.UTF-8"), ("LC_MEASUREMENT", "en_US.UTF-8")]),
            langs(&["et-EE"]),
        );
        assert_eq!(s.units(), UnitSystem::Imperial);
        assert_eq!(
            s.number_style(),
            NumberStyle::SpaceComma,
            "LC_NUMERIC is not set: LANG"
        );
    }

    #[test]
    fn posix_variables_follow_the_precedence_and_c_means_unset() {
        let e =
            |pairs: &'static [(&'static str, &'static str)]| from_posix_env(env(pairs), langs(&[]));
        // LC_ALL beats the others.
        let s = e(&[
            ("LC_ALL", "de_DE.UTF-8"),
            ("LC_NUMERIC", "en_US"),
            ("LANG", "en_US"),
        ]);
        assert_eq!(s.number_style(), NumberStyle::DotComma);
        // C and POSIX name no locale.
        let s = e(&[
            ("LC_ALL", "C"),
            ("LC_NUMERIC", "POSIX"),
            ("LANG", "fr_FR.UTF-8"),
        ]);
        assert_eq!(s.number_style(), NumberStyle::SpaceComma);
        let s = e(&[("LANG", "C.UTF-8")]);
        assert_eq!(s.format_locale, None);
        assert_eq!(s.units(), UnitSystem::Metric);
        assert_eq!(s.number_style(), NumberStyle::CommaDot);
        // Swiss formats.
        let s = e(&[("LANG", "de_CH.UTF-8")]);
        assert_eq!(s.number_style(), NumberStyle::ApostropheDot);
        // LC_MEASUREMENT alone decides the units.
        let s = e(&[("LANG", "de_DE.UTF-8"), ("LC_MEASUREMENT", "en_US.UTF-8")]);
        assert_eq!(s.units(), UnitSystem::Imperial);
        assert_eq!(s.number_style(), NumberStyle::DotComma);
        // No variables at all.
        let s = e(&[]);
        assert_eq!(
            (s.units(), s.number_style()),
            (UnitSystem::Metric, NumberStyle::CommaDot)
        );
    }

    #[test]
    fn windows_answers_win_over_the_language_table() {
        let s = from_windows(
            Some("et-EE".into()),
            Some("0".into()),
            Some(",".into()),
            Some("\u{a0}".into()),
            langs(&["en-US"]),
        );
        assert_eq!(s.units(), UnitSystem::Metric);
        assert_eq!(s.number_style(), NumberStyle::SpaceComma);
        // A user who changed the symbols in Control Panel: the symbols win over the locale name.
        let s = from_windows(
            Some("en-US".into()),
            Some("1".into()),
            Some(",".into()),
            Some(".".into()),
            langs(&["en-US"]),
        );
        assert_eq!(s.units(), UnitSystem::Imperial);
        assert_eq!(s.number_style(), NumberStyle::DotComma);
        // An empty thousands symbol is no grouping.
        let s = from_windows(
            Some("en-US".into()),
            None,
            Some(".".into()),
            Some(String::new()),
            langs(&[]),
        );
        assert_eq!(s.number_style(), NumberStyle::PlainDot);
        // Symbols that we do not offer fall back to the locale name.
        let s = from_windows(
            Some("de-CH".into()),
            Some("2".into()),
            Some("\u{b7}".into()),
            Some("x".into()),
            langs(&[]),
        );
        assert_eq!(s.number_style(), NumberStyle::ApostropheDot);
        assert_eq!(s.units(), UnitSystem::Metric);
        // The Swiss apostrophe reported by the system.
        let s = from_windows(
            Some("de-CH".into()),
            None,
            Some(".".into()),
            Some("\u{2019}".into()),
            langs(&[]),
        );
        assert_eq!(s.number_style(), NumberStyle::ApostropheDot);
        // No answers at all.
        let s = from_windows(None, None, None, None, langs(&["et"]));
        assert_eq!(s.number_style(), NumberStyle::SpaceComma);
    }

    #[test]
    fn apple_answers_are_used_as_they_come() {
        let s = from_apple(
            Some("et_EE".into()),
            Some(true),
            Some(",".into()),
            Some("\u{a0}".into()),
            langs(&["en-US"]),
        );
        assert_eq!(
            (s.units(), s.number_style()),
            (UnitSystem::Metric, NumberStyle::SpaceComma)
        );
        let s = from_apple(
            Some("en_US".into()),
            Some(false),
            Some(".".into()),
            Some(",".into()),
            langs(&[]),
        );
        assert_eq!(
            (s.units(), s.number_style()),
            (UnitSystem::Imperial, NumberStyle::CommaDot)
        );
        let s = from_apple(
            Some("de_CH".into()),
            Some(true),
            Some(".".into()),
            Some("\u{2019}".into()),
            langs(&[]),
        );
        assert_eq!(s.number_style(), NumberStyle::ApostropheDot);
        let s = from_apple(None, None, None, None, langs(&["de-DE"]));
        assert_eq!(s.number_style(), NumberStyle::DotComma);
    }

    #[test]
    fn without_formats_the_first_language_decides() {
        let s = SystemLocale::from_languages(langs(&["en-US", "et"]));
        assert_eq!(s.units(), UnitSystem::Imperial);
        let s = SystemLocale::from_languages(langs(&["en"]));
        assert_eq!(s.units(), UnitSystem::Metric, "no region");
        // The United Kingdom is metric.
        let s = SystemLocale::from_languages(langs(&["en-GB"]));
        assert_eq!(s.units(), UnitSystem::Metric);
        // Liberia and Myanmar are imperial.
        for tag in ["en-LR", "my-MM"] {
            assert_eq!(
                SystemLocale::from_languages(langs(&[tag])).units(),
                UnitSystem::Imperial
            );
        }
        assert_eq!(SystemLocale::default().units(), UnitSystem::Metric);
    }

    #[test]
    fn the_symbols_map_to_styles() {
        for (d, g, want) in [
            ('.', Some(','), Some(NumberStyle::CommaDot)),
            (',', Some('.'), Some(NumberStyle::DotComma)),
            (',', Some(' '), Some(NumberStyle::SpaceComma)),
            (',', Some('\u{202f}'), Some(NumberStyle::SpaceComma)),
            ('.', Some('\''), Some(NumberStyle::ApostropheDot)),
            ('.', None, Some(NumberStyle::PlainDot)),
            (',', None, Some(NumberStyle::PlainComma)),
            ('.', Some('.'), None),
            ('x', None, None),
            (',', Some('x'), None),
        ] {
            assert_eq!(NumberStyle::from_symbols(d, g), want, "{d} {g:?}");
        }
    }

    #[test]
    fn the_real_system_can_be_read() {
        let s = read_system();
        let _ = (s.units(), s.number_style());
    }
}
