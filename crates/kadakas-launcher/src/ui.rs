//! The launcher window. It only draws and passes clicks to the [`Model`].

use crate::model::{Model, slot_text};
use kadakas_l10n::tr;
use kadakas_update::handshake::SkipReason;
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

    /// A message for the status bar (for example why the start did not work).
    pub fn with_status(mut self, status: String) -> LauncherApp {
        self.model.status = status;
        self
    }
}

fn channel_label(c: Channel) -> String {
    match c {
        Channel::Stable => tr!("launcher-channel-stable"),
        Channel::Beta => tr!("launcher-channel-nightly"),
        Channel::Local => tr!("launcher-channel-local"),
    }
}

/// The label of a version as text (`stable`, `nightly`, `local`, `bundled`).
fn label_text(label: &str) -> String {
    match label {
        "stable" => tr!("launcher-label-stable"),
        "nightly" => tr!("launcher-label-nightly"),
        "local" => tr!("launcher-label-local"),
        "bundled" => tr!("launcher-label-bundled"),
        other => other.to_string(),
    }
}

fn update_line(status: &Option<UpdateStatus>) -> String {
    match status {
        None => tr!("launcher-update-not-checked"),
        Some(UpdateStatus::UpToDate { .. }) => tr!("launcher-update-up-to-date"),
        Some(UpdateStatus::Available(o)) => {
            tr!("launcher-update-available", version = o.version.to_string())
        }
        Some(UpdateStatus::Older { offered, .. }) => {
            tr!("launcher-update-older", offered = offered.to_string())
        }
    }
}

fn skip_text(reason: SkipReason) -> String {
    match reason {
        SkipReason::Crashed => tr!("launcher-tag-crashed"),
        SkipReason::RolledBack => tr!("launcher-tag-rolled-back"),
        SkipReason::NotStarted => tr!("launcher-tag-not-started"),
    }
}

/// The tags of a row: active, rollback target, installer, kept, skipped.
fn tags(row: &kadakas_update::Installed, skipped: Option<SkipReason>) -> String {
    let mut tags = Vec::new();
    if row.active {
        tags.push(tr!("launcher-tag-active"));
    }
    if row.previous {
        tags.push(tr!("launcher-tag-previous"));
    }
    if row.slot == Slot::Bundled {
        tags.push(tr!("launcher-tag-bundled"));
    }
    if row.meta.kept {
        tags.push(tr!("launcher-tag-kept"));
    }
    if let Some(r) = skipped {
        tags.push(skip_text(r));
    }
    tags.join(", ")
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
            ui.heading(tr!("launcher-heading"));
            ui.label(tr!("launcher-intro"));
            ui.add_space(8.0);

            ui.strong(tr!("launcher-installed-versions"));
            egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                egui::Grid::new("versions").striped(true).show(ui, |ui| {
                    if self.model.installed.is_empty() {
                        ui.label(tr!("launcher-nothing-installed"));
                        ui.end_row();
                    }
                    let mut picked = None;
                    for i in &self.model.installed {
                        let selected = self.model.selected.as_ref() == Some(&i.slot);
                        if ui
                            .selectable_label(selected, i.version.to_string())
                            .clicked()
                        {
                            picked = Some(i.slot.clone());
                        }
                        ui.label(label_text(&i.meta.label));
                        ui.label(i.meta.commit.as_deref().unwrap_or_default());
                        ui.label(tags(i, self.model.skipped.get(&i.slot).copied()));
                        ui.end_row();
                    }
                    if let Some(slot) = picked {
                        self.model.select(slot);
                    }
                });
            });
            ui.horizontal(|ui| {
                if let Some(sel) = &self.model.selected {
                    ui.label(slot_text(sel));
                }
                if ui
                    .add_enabled(self.model.can_use(), egui::Button::new(tr!("launcher-use")))
                    .clicked()
                {
                    self.model.use_selected();
                }
                let keep_text = if self.model.selected_kept() {
                    tr!("launcher-unkeep")
                } else {
                    tr!("launcher-keep")
                };
                if ui
                    .add_enabled(self.model.can_keep(), egui::Button::new(keep_text))
                    .clicked()
                {
                    self.model.toggle_keep();
                }
                if ui
                    .add_enabled(
                        self.model.can_remove(),
                        egui::Button::new(tr!("launcher-remove")),
                    )
                    .clicked()
                {
                    self.model.remove_selected();
                }
                if ui
                    .add_enabled(
                        self.model.can_retry(),
                        egui::Button::new(tr!("launcher-try-again")),
                    )
                    .clicked()
                {
                    self.model.retry_selected();
                }
            });
            ui.add_space(8.0);

            egui::Grid::new("choices").num_columns(2).show(ui, |ui| {
                ui.label(tr!("launcher-update-channel"));
                let mut channel = self.model.settings.channel;
                egui::ComboBox::from_id_salt("channel")
                    .selected_text(channel_label(channel))
                    .show_ui(ui, |ui| {
                        for c in [Channel::Stable, Channel::Beta, Channel::Local] {
                            ui.selectable_value(&mut channel, c, channel_label(c));
                        }
                    });
                self.model.set_channel(channel);
                ui.end_row();

                ui.label(tr!("launcher-ui-flavor"));
                let mut flavor = self.model.settings.flavor.clone();
                let shown = self
                    .model
                    .flavors
                    .iter()
                    .find(|f| f.id == flavor)
                    .map_or_else(|| tr!("launcher-none-installed"), |f| f.name.clone());
                egui::ComboBox::from_id_salt("flavor")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for f in &self.model.flavors {
                            ui.selectable_value(&mut flavor, f.id.clone(), &f.name);
                        }
                    });
                self.model.set_flavor(&flavor);
                ui.end_row();

                ui.label(tr!("launcher-profile"));
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.profile_text)
                        .hint_text(tr!("launcher-none"))
                        .desired_width(160.0),
                );
                if edit.changed() {
                    self.model.set_profile(&self.profile_text);
                }
                let wrong = !self.profile_text.trim().is_empty()
                    && !kadakas_update::settings::valid_name(self.profile_text.trim());
                if wrong {
                    ui.colored_label(ui.visuals().error_fg_color, tr!("launcher-profile-hint"));
                }
                ui.end_row();
            });
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.strong(tr!("launcher-updates"));
                ui.label(update_line(&self.model.update));
            });
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let wake = {
                    let ctx = ctx.clone();
                    move || ctx.request_repaint()
                };
                if ui
                    .add_enabled(
                        self.model.can_launch(),
                        egui::Button::new(tr!("launcher-launch")),
                    )
                    .clicked()
                {
                    self.model.launch();
                }
                if ui
                    .add_enabled(
                        !self.model.busy(),
                        egui::Button::new(tr!("launcher-check")),
                    )
                    .clicked()
                {
                    self.model.start(Task::Check, wake.clone());
                }
                if ui
                    .add_enabled(
                        self.model.can_update(),
                        egui::Button::new(tr!("launcher-update")),
                    )
                    .clicked()
                {
                    self.model.start(Task::Update, wake.clone());
                }
                if ui
                    .add_enabled(
                        self.model.can_roll_back(),
                        egui::Button::new(tr!("launcher-roll-back")),
                    )
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
    use kadakas_update::Version;
    use kadakas_update::{Installed, Offer, VersionMeta};

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
        assert_eq!(channel_label(Channel::Beta), "Nightly");
        assert!(channel_label(Channel::Local).starts_with("Local"));
        assert_eq!(channel_label(Channel::Stable), "Stable");
    }

    #[test]
    fn rows_have_labels_and_tags_in_both_languages() {
        let row = |slot, active, kept| Installed {
            slot,
            version: Version::parse("1.0.0").unwrap(),
            dir: Default::default(),
            active,
            previous: false,
            meta: VersionMeta {
                kept,
                ..VersionMeta::default()
            },
        };
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                for l in ["stable", "nightly", "local", "bundled"] {
                    assert!(!label_text(l).is_empty());
                }
                assert_eq!(label_text("other"), "other");
                let t = tags(&row(Slot::Bundled, true, true), Some(SkipReason::Crashed));
                assert_eq!(t.split(", ").count(), 4, "{t}");
                for r in [
                    SkipReason::Crashed,
                    SkipReason::RolledBack,
                    SkipReason::NotStarted,
                ] {
                    assert!(!skip_text(r).is_empty());
                }
                let mut prev = row(Slot::Version(Version::parse("1.0.0").unwrap()), false, false);
                prev.previous = true;
                assert!(!tags(&prev, None).is_empty());
            });
        }
        assert_eq!(
            tags(&row(Slot::Bundled, false, false), None),
            "came with the installer"
        );
    }
}
