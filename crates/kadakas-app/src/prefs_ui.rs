//! File > Preferences: the dialog with the appearance settings and the autosave settings.

use crate::autosave_ui::AutosaveUi;
use crate::icon_size::IconSize;

/// The state of the Preferences dialog.
#[derive(Default)]
pub struct PrefsDialog {
    open: bool,
}

impl PrefsDialog {
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Shows the dialog when it is open.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        icon_size: &mut IconSize,
        autosave: &mut AutosaveUi,
    ) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Preferences")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Appearance");
                ui.horizontal(|ui| {
                    ui.label("Icon size");
                    for z in IconSize::ALL {
                        ui.selectable_value(icon_size, z, z.label());
                    }
                });
                ui.add_space(6.0);
                autosave.settings_section(ui);
            });
        self.open = open;
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_shows_only_when_open_and_changes_the_icon_size() {
        let ctx = egui::Context::default();
        let mut d = PrefsDialog::default();
        let mut au = AutosaveUi::inert();
        let mut size = IconSize::Medium;
        d.show(&ctx, &mut size, &mut au);
        assert!(!d.is_open());
        d.open();
        let _ = ctx.run_ui(Default::default(), |ui| {
            d.show(ui.ctx(), &mut size, &mut au)
        });
        assert!(d.is_open());
        assert_eq!(size, IconSize::Medium);
    }
}
