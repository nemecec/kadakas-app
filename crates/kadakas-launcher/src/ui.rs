//! The launcher window. It only draws and passes clicks to the [`Model`].

use crate::model::Model;
use kadakas_update::tasks::Task;
use kadakas_update::{Channel, Slot, UpdateStatus};

pub struct LauncherApp {
    model: Model,
    /// The text in the profile field, which may be wrong while the user types.
    profile_text: String,
}

impl LauncherApp {
    pub fn new(model: Model) -> LauncherApp {
        let profile_text = model.settings.profile.clone();
        LauncherApp {
            model,
            profile_text,
        }
    }
}

fn channel_label(c: Channel) -> &'static str {
    match c {
        Channel::Stable => "Stable",
        Channel::Beta => "Beta",
    }
}

fn update_line(status: &Option<UpdateStatus>) -> String {
    match status {
        None => "Not checked yet.".into(),
        Some(UpdateStatus::UpToDate { .. }) => "Up to date.".into(),
        Some(UpdateStatus::Available(o)) => format!("Version {} is available.", o.version),
        Some(UpdateStatus::Older { offered, .. }) => {
            format!("The server offers {offered}, which is older. Ignored.")
        }
    }
}

impl eframe::App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.model.poll() {
            self.profile_text = self.model.settings.profile.clone();
        }
        if self.model.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.model.busy() {
                    ui.spinner();
                }
                ui.label(&self.model.status);
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Kadakas");
            ui.label("Keeps Kadakas up to date and starts it.");
            ui.add_space(8.0);

            ui.strong("Installed versions");
            egui::Grid::new("versions").striped(true).show(ui, |ui| {
                if self.model.installed.is_empty() {
                    ui.label("Nothing installed. Check for updates to get Kadakas.");
                    ui.end_row();
                }
                for i in &self.model.installed {
                    ui.label(i.version.to_string());
                    let mut tags = Vec::new();
                    if i.active {
                        tags.push("active");
                    }
                    if i.previous {
                        tags.push("previous (rollback target)");
                    }
                    if i.slot == Slot::Bundled {
                        tags.push("came with the installer");
                    }
                    ui.label(tags.join(", "));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);

            egui::Grid::new("choices").num_columns(2).show(ui, |ui| {
                ui.label("Update channel");
                let mut channel = self.model.settings.channel;
                egui::ComboBox::from_id_salt("channel")
                    .selected_text(channel_label(channel))
                    .show_ui(ui, |ui| {
                        for c in [Channel::Stable, Channel::Beta] {
                            ui.selectable_value(&mut channel, c, channel_label(c));
                        }
                    });
                self.model.set_channel(channel);
                ui.end_row();

                ui.label("UI flavor");
                let mut flavor = self.model.settings.flavor.clone();
                let shown = self
                    .model
                    .flavors
                    .iter()
                    .find(|f| f.id == flavor)
                    .map_or("none installed", |f| f.name.as_str());
                egui::ComboBox::from_id_salt("flavor")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for f in &self.model.flavors {
                            ui.selectable_value(&mut flavor, f.id.clone(), &f.name);
                        }
                    });
                self.model.set_flavor(&flavor);
                ui.end_row();

                ui.label("Profile");
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.profile_text)
                        .hint_text("none")
                        .desired_width(160.0),
                );
                if edit.changed() {
                    self.model.set_profile(&self.profile_text);
                }
                let wrong = !self.profile_text.trim().is_empty()
                    && !kadakas_update::settings::valid_name(self.profile_text.trim());
                if wrong {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "Letters, digits, - and _ only (40 at most).",
                    );
                }
                ui.end_row();
            });
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.strong("Updates:");
                ui.label(update_line(&self.model.update));
            });
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let wake = {
                    let ctx = ctx.clone();
                    move || ctx.request_repaint()
                };
                if ui
                    .add_enabled(self.model.can_launch(), egui::Button::new("Launch"))
                    .clicked()
                {
                    self.model.launch();
                }
                if ui
                    .add_enabled(!self.model.busy(), egui::Button::new("Check for updates"))
                    .clicked()
                {
                    self.model.start(Task::Check, wake.clone());
                }
                if ui
                    .add_enabled(self.model.can_update(), egui::Button::new("Update"))
                    .clicked()
                {
                    self.model.start(Task::Update, wake.clone());
                }
                if ui
                    .add_enabled(self.model.can_roll_back(), egui::Button::new("Roll back"))
                    .clicked()
                {
                    self.model.start(Task::Rollback, wake);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_update::Offer;
    use kadakas_update::Version;

    #[test]
    fn update_text() {
        let v = |s: &str| Version::parse(s).unwrap();
        assert_eq!(update_line(&None), "Not checked yet.");
        assert_eq!(
            update_line(&Some(UpdateStatus::UpToDate { current: None })),
            "Up to date."
        );
        let offer = Offer {
            version: v("1.1.0"),
            channel: Channel::Stable,
            size: 1,
        };
        assert!(update_line(&Some(UpdateStatus::Available(offer))).contains("1.1.0"));
        let older = UpdateStatus::Older {
            current: v("2.0.0"),
            offered: v("1.0.0"),
        };
        assert!(update_line(&Some(older)).contains("older"));
        assert_eq!(channel_label(Channel::Beta), "Beta");
    }
}
