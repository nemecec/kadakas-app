//! Help > About Cadektor: version, license, trademark note and the licenses of all third-party
//! parts. The license text is built into the program (`THIRD_PARTY_NOTICES.txt`), so the dialog
//! works in the web build too.

/// The licenses of the libraries, fonts and icons that we ship (made by `scripts/notices.sh`).
pub const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.txt");

/// The license of Cadektor itself.
pub const LICENSE: &str = "Apache-2.0 OR MIT";

/// The note about the Vectorworks trademark.
pub const TRADEMARK: &str = "Vectorworks is a trademark of Vectorworks, Inc. Cadektor is an independent project and is not affiliated with, endorsed by, or sponsored by Vectorworks, Inc.";

/// Other trademarks that the documents name.
pub const OTHER_TRADEMARKS: &str =
    "AutoCAD and DWG are trademarks of Autodesk, Inc. SketchUp is a trademark of Trimble Inc.";

/// The version line, for example `Version 0.1.0`.
pub fn version_line() -> String {
    format!("Version {}", env!("CARGO_PKG_VERSION"))
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
        egui::Window::new("About Cadektor")
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
        ui.heading("Cadektor");
        ui.label(version_line());
        ui.label("CAD for landscape architects.");
        ui.add_space(6.0);
        ui.label(format!("License: {LICENSE}"));
        ui.label("Cadektor uses open source libraries, icons and fonts. See Licenses.");
        ui.add_space(6.0);
        ui.label(TRADEMARK);
        ui.label(OTHER_TRADEMARKS);
        ui.add_space(6.0);
        if ui.button("Licenses...").clicked() {
            self.show_licenses();
        }
    }

    fn licenses_view(&mut self, ui: &mut egui::Ui) {
        if ui.button("Back").clicked() {
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
    fn notices_are_built_in_and_name_the_non_crate_items() {
        assert!(NOTICES.contains("THIRD-PARTY SOFTWARE NOTICES"));
        assert!(NOTICES.contains("Tabler Icons"));
        assert!(NOTICES.contains("EPSG Geodetic Parameter Dataset"));
        assert!(NOTICES.contains("Vectorworks, Inc."));
    }

    #[test]
    fn trademark_note_is_exact() {
        assert!(TRADEMARK.starts_with("Vectorworks is a trademark of Vectorworks, Inc."));
        assert!(TRADEMARK.contains("not affiliated with, endorsed by, or sponsored by"));
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
