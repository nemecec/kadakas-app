//! File > Preferences: the dialog with the language, the units and formats, the appearance
//! settings and the autosave settings.

use crate::autosave_ui::AutosaveUi;
use crate::icon_size::IconSize;
use crate::locale::LanguageChoice;
use kadakas_l10n::tr;
use kadakas_units::{
    AngleFormat, AreaUnit, Date, DateFormat, Decimal, Grouping, LengthUnit, Settings,
};

/// The name of a navigation style.
pub fn nav_style_label(n: kadakas_ui_core::nav3d::NavStyle) -> String {
    match n {
        kadakas_ui_core::nav3d::NavStyle::Mouse => tr!("prefs-ui-nav-mouse"),
        kadakas_ui_core::nav3d::NavStyle::Trackpad => tr!("prefs-ui-nav-trackpad"),
    }
}

/// The state of the Preferences dialog.
#[derive(Default)]
pub struct PrefsDialog {
    open: bool,
    /// What Preferences > Privacy read from the feedback settings (PLAT-20).
    privacy: crate::privacy_ui::PrivacyState,
}

/// What the dialog changes.
pub struct PrefsEdit<'a> {
    pub icon_size: &'a mut IconSize,
    pub language: &'a mut LanguageChoice,
    pub units: &'a mut Settings,
    /// The language that is active now, for the entry "system default".
    pub active_language: &'static str,
    /// The workspace profiles (PLAT-04).
    pub profiles: &'a mut crate::profiles::Profiles,
    /// The limits of the versions of a file.
    pub max_versions: &'a mut u32,
    pub max_version_mib: &'a mut u32,
    /// Animate the view when an agent moves it.
    pub animate_agent_view: &'a mut bool,
    /// Mouse or trackpad: what the wheel and the scroll do in the 3D view (PLAT-26).
    pub navigation: &'a mut kadakas_ui_core::nav3d::NavStyle,
    /// Preferences > Privacy (PLAT-20): crash reports and the installation ID. The settings are
    /// in the feedback hub.
    pub privacy: &'a mut kadakas_feedback::Hub,
    /// Preferences > Updates (PLAT-16).
    #[cfg(not(target_arch = "wasm32"))]
    pub updates: &'a mut crate::app_updates::Updates,
}

impl PrefsDialog {
    pub fn open(&mut self) {
        self.open = true;
        self.privacy.reset();
    }

    /// Shows the dialog when it is open.
    pub fn show(&mut self, ctx: &egui::Context, edit: PrefsEdit<'_>, autosave: &mut AutosaveUi) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr!("prefs-ui-preferences"))
            .id(egui::Id::new("win-preferences"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading(tr!("prefs-ui-language"));
                language_row(ui, edit.language, edit.active_language);
                ui.add_space(6.0);
                ui.heading(tr!("prefs-ui-units-and-formats"));
                units_section(ui, edit.units, edit.active_language);
                ui.add_space(6.0);
                crate::profiles::section(ui, edit.profiles);
                ui.add_space(6.0);
                ui.heading(tr!("prefs-ui-appearance"));
                ui.horizontal(|ui| {
                    ui.label(tr!("prefs-ui-icon-size"));
                    for z in IconSize::ALL {
                        ui.selectable_value(edit.icon_size, z, z.label());
                    }
                });
                ui.checkbox(edit.animate_agent_view, tr!("prefs-ui-animate-agent-view"));
                ui.horizontal(|ui| {
                    ui.label(tr!("prefs-ui-navigation"));
                    for n in kadakas_ui_core::nav3d::NavStyle::ALL {
                        ui.selectable_value(edit.navigation, n, nav_style_label(n));
                    }
                });
                ui.small(tr!("prefs-ui-navigation-hint"));
                ui.add_space(6.0);
                ui.heading(tr!("prefs-ui-versions"));
                ui.horizontal(|ui| {
                    ui.label(tr!("prefs-ui-max-versions"));
                    ui.add(
                        egui::DragValue::new(edit.max_versions)
                            .range(1..=kadakas_api::VERSIONS_MAX as u32),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(tr!("prefs-ui-max-version-size"));
                    ui.add(
                        egui::DragValue::new(edit.max_version_mib)
                            .range(1..=(kadakas_api::VERSION_BYTES_MAX / (1024 * 1024)) as u32),
                    );
                });
                ui.small(tr!("prefs-ui-versions-hint"));
                ui.add_space(6.0);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    crate::app_updates::prefs_section(ui, edit.updates);
                    ui.add_space(6.0);
                }
                autosave.settings_section(ui);
                ui.add_space(6.0);
                crate::privacy_ui::section(ui, edit.privacy, &mut self.privacy);
            });
        self.open = open;
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open
    }
}

/// The name of a language in the language itself.
fn native_name(tag: &str) -> &'static str {
    kadakas_l10n::languages()
        .iter()
        .find(|l| l.tag == tag)
        .map_or("", |l| l.native_name)
}

fn language_row(ui: &mut egui::Ui, choice: &mut LanguageChoice, active: &str) {
    ui.horizontal(|ui| {
        ui.label(tr!("prefs-ui-language-of-the-program"));
        let shown = match choice.tag() {
            Some(tag) => native_name(tag).to_string(),
            None => tr!("prefs-ui-system-default", language = native_name(active)),
        };
        egui::ComboBox::from_id_salt("pref_language")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    choice,
                    LanguageChoice::SYSTEM,
                    tr!("prefs-ui-system-default", language = native_name(active)),
                );
                for lang in kadakas_l10n::languages() {
                    ui.selectable_value(choice, LanguageChoice::fixed(lang.tag), lang.native_name);
                }
            });
    });
}

/// A combo box over a list of values with a text for each.
fn pick<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    label: String,
    value: &mut T,
    options: &[(T, String)],
) {
    pick_with(ui, id, label, value, options, |_| false);
}

/// Like `pick`, and the options for which `disabled` is true are grey and cannot be chosen.
fn pick_with<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    label: String,
    value: &mut T,
    options: &[(T, String)],
    disabled: impl Fn(&T) -> bool,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        let shown = options
            .iter()
            .find(|(v, _)| v == value)
            .map(|(_, t)| t.clone())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt(id)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (v, t) in options {
                    let button = egui::Button::selectable(*value == *v, t.as_str());
                    if ui.add_enabled(!disabled(v), button).clicked() {
                        *value = *v;
                    }
                }
            });
    });
}

/// The angle format as text.
pub fn angle_format_name(f: AngleFormat) -> String {
    match f {
        AngleFormat::Decimal => tr!("prefs-ui-decimal-degrees"),
        AngleFormat::Dms => tr!("prefs-ui-degrees-minutes-seconds"),
        AngleFormat::Bearing => tr!("prefs-ui-bearing"),
    }
}

/// The symbol of the decimal separator and of the digit grouping that the settings give in a
/// language. The choices that use the same symbol twice are grey in the lists.
fn separator_symbols(s: &Settings, language: &str) -> (char, Option<char>) {
    let (convention, _) = kadakas_units::Units::conventions(language);
    (
        s.decimal.unwrap_or(convention.decimal).symbol(),
        s.grouping.unwrap_or(convention.grouping).symbol(),
    )
}

fn units_section(ui: &mut egui::Ui, s: &mut Settings, language: &str) {
    let (decimal_symbol, grouping_symbol) = separator_symbols(s, language);
    let auto = || tr!("prefs-ui-automatic");
    pick(
        ui,
        "pref_length",
        tr!("prefs-ui-length"),
        &mut s.length,
        &LengthUnit::ALL.map(|u| (u, u.short_name().to_string())),
    );
    pick(
        ui,
        "pref_area",
        tr!("prefs-ui-area"),
        &mut s.area,
        &AreaUnit::ALL.map(|u| (u, u.short_name().to_string())),
    );
    pick(
        ui,
        "pref_angle",
        tr!("prefs-ui-angles"),
        &mut s.angle,
        &AngleFormat::ALL.map(|f| (f, angle_format_name(f))),
    );
    pick(
        ui,
        "pref_fraction",
        tr!("prefs-ui-inch-fractions"),
        &mut s.inch_fraction,
        &[
            (None, auto()),
            (Some(0), tr!("prefs-ui-decimal-inches")),
            (Some(2), "1/2".into()),
            (Some(4), "1/4".into()),
            (Some(8), "1/8".into()),
            (Some(16), "1/16".into()),
            (Some(32), "1/32".into()),
            (Some(64), "1/64".into()),
        ],
    );
    pick_with(
        ui,
        "pref_decimal",
        tr!("prefs-ui-decimal-separator"),
        &mut s.decimal,
        &[
            (None, auto()),
            (Some(Decimal::Point), tr!("prefs-ui-point-example")),
            (Some(Decimal::Comma), tr!("prefs-ui-comma-example")),
        ],
        |d| d.is_some_and(|d| Some(d.symbol()) == grouping_symbol),
    );
    pick_with(
        ui,
        "pref_grouping",
        tr!("prefs-ui-digit-grouping"),
        &mut s.grouping,
        &[
            (None, auto()),
            (Some(Grouping::None), tr!("prefs-ui-no-grouping")),
            (Some(Grouping::Comma), "1,234,567".into()),
            (Some(Grouping::Point), "1.234.567".into()),
            (Some(Grouping::Space), "1\u{a0}234\u{a0}567".into()),
        ],
        |g| g.is_some_and(|g| g.symbol() == Some(decimal_symbol)),
    );
    let date = |f: DateFormat| (Some(f), f.pattern().to_string());
    pick(
        ui,
        "pref_date",
        tr!("prefs-ui-date-format"),
        &mut s.date,
        &[
            (None, auto()),
            date(DateFormat::Iso),
            date(DateFormat::Dmy),
            date(DateFormat::Mdy),
        ],
    );
    // An example, so the person sees what the choices do.
    let u = kadakas_units::current();
    let d = Date::new(2026, 10, 11).unwrap_or(Date::from_days(0));
    ui.label(
        egui::RichText::new(tr!(
            "prefs-ui-example",
            length = u.length_text(1234.5),
            area = u.area_text(12_345_678_000.0),
            angle = u.angle_text(37.5),
            number = u.number_text(1_234_567.891, 2),
            date = u.date_text(d)
        ))
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit<'a>(
        size: &'a mut IconSize,
        lang: &'a mut LanguageChoice,
        units: &'a mut Settings,
        profiles: &'a mut crate::profiles::Profiles,
    ) -> PrefsEdit<'a> {
        PrefsEdit {
            icon_size: size,
            language: lang,
            units,
            active_language: "en-US",
            profiles,
            max_versions: Box::leak(Box::new(crate::prefs::DEFAULT_MAX_VERSIONS)),
            max_version_mib: Box::leak(Box::new(crate::prefs::DEFAULT_MAX_VERSION_MIB)),
            animate_agent_view: Box::leak(Box::new(true)),
            navigation: Box::leak(Box::default()),
            privacy: Box::leak(Box::new(kadakas_feedback::Hub::in_memory())),
            updates: Box::leak(Box::new(crate::app_updates::Updates::new(None, None))),
        }
    }

    #[test]
    fn the_dialog_shows_only_when_open_and_changes_nothing_by_itself() {
        let ctx = egui::Context::default();
        let mut d = PrefsDialog::default();
        let mut au = AutosaveUi::inert();
        let mut size = IconSize::Medium;
        let mut lang = LanguageChoice::SYSTEM;
        let mut units = Settings::default();
        let mut profiles = crate::profiles::Profiles::default();
        d.show(
            &ctx,
            edit(&mut size, &mut lang, &mut units, &mut profiles),
            &mut au,
        );
        assert!(!d.is_open());
        d.open();
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                let _ = ctx.run_ui(Default::default(), |ui| {
                    d.show(
                        ui.ctx(),
                        edit(&mut size, &mut lang, &mut units, &mut profiles),
                        &mut au,
                    )
                });
            });
        }
        assert!(d.is_open());
        assert_eq!(size, IconSize::Medium);
        assert_eq!(lang, LanguageChoice::SYSTEM);
        assert_eq!(units, Settings::default());
    }

    #[test]
    fn the_navigation_styles_have_names_in_both_languages() {
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                let a = nav_style_label(kadakas_ui_core::nav3d::NavStyle::Mouse);
                let b = nav_style_label(kadakas_ui_core::nav3d::NavStyle::Trackpad);
                assert!(!a.is_empty() && a != b, "{a} / {b}");
            });
        }
    }

    #[test]
    fn the_symbol_of_the_decimal_separator_and_of_the_groups_are_found() {
        let s = Settings::default();
        assert_eq!(separator_symbols(&s, "et"), (',', Some('\u{a0}')));
        assert_eq!(separator_symbols(&s, "en-US"), ('.', Some(',')));
        let s = Settings {
            decimal: Some(Decimal::Comma),
            grouping: Some(Grouping::None),
            ..Settings::default()
        };
        assert_eq!(separator_symbols(&s, "en-US"), (',', None));
    }

    #[test]
    fn the_names_of_languages_and_angle_formats_exist_in_every_language() {
        assert_eq!(native_name("et"), "Eesti");
        assert_eq!(native_name("xx"), "");
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                for f in AngleFormat::ALL {
                    assert!(!angle_format_name(f).is_empty());
                }
            });
        }
    }
}
