//! Help > About: the name, the version, the license, the trademark notes and the licenses of all
//! third-party parts. The text of the licenses is built into the program (`THIRD_PARTY_NOTICES.txt`,
//! the same file as in `kadakas-app`).

use kadakas_l10n::tr;

/// The licenses of the libraries, fonts and icons that we ship (made by `scripts/notices.sh`).
pub const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.txt");

/// The license of Kadakas itself.
pub const LICENSE: &str = "Apache-2.0 OR MIT";

/// The version line, for example `Version 0.1.0`.
pub fn version_line() -> String {
    tr!("about-version", version = env!("CARGO_PKG_VERSION"))
}

/// The state of the About dialog.
#[derive(Default)]
pub struct About {
    open: bool,
    licenses: bool,
    /// The lines of `NOTICES`, made when the licenses view first opens.
    lines: Vec<&'static str>,
}

impl About {
    pub fn open(&mut self) {
        self.open = true;
        self.licenses = false;
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show_licenses(&mut self) {
        self.licenses = true;
        if self.lines.is_empty() {
            self.lines = NOTICES.lines().collect();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new(tr!("modeler-about-title"))
            .id(egui::Id::new("win-about-modeler"))
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                if self.licenses {
                    self.licenses_view(ui);
                } else {
                    self.main_view(ui);
                }
            });
        self.open = open;
    }

    fn main_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("modeler-window-title"));
        ui.label(version_line());
        ui.label(tr!("modeler-about-description"));
        ui.add_space(6.0);
        ui.label(tr!("about-license", license = LICENSE));
        ui.label(tr!("about-kadakas-uses-open-source-libraries-icons-and"));
        ui.add_space(6.0);
        ui.label(tr!("modeler-about-trademark"));
        ui.label(tr!("about-trademark"));
        ui.add_space(6.0);
        if ui.button(tr!("about-licenses")).clicked() {
            self.show_licenses();
        }
    }

    fn licenses_view(&mut self, ui: &mut egui::Ui) {
        if ui.button(tr!("about-back")).clicked() {
            self.licenses = false;
        }
        ui.separator();
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both()
            .max_height(420.0)
            .auto_shrink([false, false])
            .show_rows(ui, row_height, self.lines.len(), |ui, range| {
                for i in range {
                    ui.add(
                        egui::Label::new(egui::RichText::new(self.lines[i]).monospace()).extend(),
                    );
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_are_built_in() {
        assert!(NOTICES.contains("THIRD-PARTY SOFTWARE NOTICES"));
    }

    #[test]
    fn the_trademark_note_names_the_owner_in_both_languages() {
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                let t = tr!("modeler-about-trademark");
                assert!(
                    t.contains("SketchUp") && t.contains("Trimble Inc."),
                    "{tag}: {t}"
                );
            });
        }
        assert!(version_line().starts_with("Version "));
    }

    #[test]
    fn the_dialog_opens_and_draws_both_views() {
        let ctx = egui::Context::default();
        let mut about = About::default();
        about.show(&ctx);
        assert!(!about.is_open());
        about.open();
        let _ = ctx.run_ui(Default::default(), |ui| about.show(ui.ctx()));
        about.show_licenses();
        assert!(about.lines.len() > 100);
        let _ = ctx.run_ui(Default::default(), |ui| about.show(ui.ctx()));
        about.open();
        assert!(!about.licenses);
    }
}
