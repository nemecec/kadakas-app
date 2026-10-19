//! Preferences > Privacy (PLAT-20): what happens to crash reports, and the installation ID.
//!
//! The settings are kept by the feedback hub (`kadakas_feedback::Hub`), where the sender reads
//! them: the crash policy, the installation ID, and the switch that leaves the ID out of the
//! reports (crash reports and the reports of UI-14). The functions here only draw and call the
//! hub.

use kadakas_feedback::{CrashPolicy, Hub};
use kadakas_l10n::tr;

/// What the section remembers between frames, so it does not read the settings file each frame.
#[derive(Default)]
pub struct PrivacyState {
    id: Option<String>,
    policy: Option<CrashPolicy>,
    include: Option<bool>,
    error: Option<String>,
}

impl PrivacyState {
    /// Forget what was read (the dialog opens again).
    pub fn reset(&mut self) {
        *self = PrivacyState::default();
    }

    fn load(&mut self, hub: &mut Hub) {
        if self.id.is_none() {
            self.id = Some(hub.installation_id());
        }
        if self.policy.is_none() {
            self.policy = Some(hub.crash_policy());
        }
        if self.include.is_none() {
            self.include = Some(hub.include_installation());
        }
    }

    /// The person chose a policy.
    pub fn set_policy(&mut self, hub: &mut Hub, p: CrashPolicy) {
        match hub.set_crash_policy(p) {
            Ok(()) => self.policy = Some(p),
            Err(e) => self.error = Some(tr!("crash-ui-prefs-id-error", error = e.to_string())),
        }
    }

    /// The person pressed Reset: a new random ID.
    pub fn reset_id(&mut self, hub: &mut Hub) {
        match hub.reset_installation_id() {
            Ok(id) => self.id = Some(id),
            Err(e) => self.error = Some(tr!("crash-ui-prefs-id-error", error = e.to_string())),
        }
    }

    /// The person flipped the switch.
    pub fn set_include(&mut self, hub: &mut Hub, on: bool) {
        match hub.set_include_installation(on) {
            Ok(()) => self.include = Some(on),
            Err(e) => self.error = Some(tr!("crash-ui-prefs-id-error", error = e.to_string())),
        }
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
}

pub fn policy_name(p: CrashPolicy) -> String {
    match p {
        CrashPolicy::Ask => tr!("crash-ui-policy-ask"),
        CrashPolicy::Always => tr!("crash-ui-policy-always"),
        CrashPolicy::Never => tr!("crash-ui-policy-never"),
    }
}

/// Draws the section.
pub fn section(ui: &mut egui::Ui, hub: &mut Hub, state: &mut PrivacyState) {
    state.load(hub);
    ui.heading(tr!("crash-ui-prefs-privacy"));
    // Crash reports are captured and shown by the desktop program only.
    #[cfg(not(target_arch = "wasm32"))]
    {
        ui.label(tr!("crash-ui-prefs-crash-reports"));
        let mut policy = state.policy.unwrap_or_default();
        let before = policy;
        ui.horizontal(|ui| {
            for p in CrashPolicy::ALL {
                ui.selectable_value(&mut policy, p, policy_name(p));
            }
        });
        if policy != before {
            state.set_policy(hub, policy);
        }
        ui.small(tr!("crash-ui-prefs-crash-hint"));
        ui.add_space(4.0);
    }
    ui.horizontal(|ui| {
        ui.label(tr!("crash-ui-prefs-installation-id"));
        ui.monospace(state.id().unwrap_or_default());
        if ui.button(tr!("crash-ui-prefs-reset")).clicked() {
            state.reset_id(hub);
        }
    });
    let mut include = state.include.unwrap_or(true);
    if ui
        .checkbox(&mut include, tr!("crash-ui-prefs-include-id"))
        .changed()
    {
        state.set_include(hub, include);
    }
    ui.small(tr!("crash-ui-prefs-id-hint"));
    if let Some(e) = &state.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_made_on_first_use_and_reset_gives_a_new_one() {
        let mut hub = Hub::in_memory();
        let mut s = PrivacyState::default();
        s.load(&mut hub);
        let first = s.id().unwrap().to_string();
        assert_eq!(first.len(), 36);
        assert_eq!(first.as_bytes()[14], b'4', "a version 4 UUID");
        s.reset_id(&mut hub);
        let second = s.id().unwrap().to_string();
        assert_ne!(first, second);
        assert_eq!(hub.installation_id(), second);
    }

    #[test]
    fn the_switch_and_the_policy_reach_the_hub() {
        let mut hub = Hub::in_memory();
        let mut s = PrivacyState::default();
        s.load(&mut hub);
        assert_eq!(s.policy, Some(CrashPolicy::Ask));
        assert_eq!(s.include, Some(true));
        s.set_include(&mut hub, false);
        s.set_policy(&mut hub, CrashPolicy::Never);
        assert!(!hub.include_installation());
        assert_eq!(hub.crash_policy(), CrashPolicy::Never);
        s.reset();
        s.load(&mut hub);
        assert_eq!(s.policy, Some(CrashPolicy::Never));
        assert_eq!(s.include, Some(false));
    }

    #[test]
    fn the_section_draws_and_names_exist_in_both_languages() {
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                for p in CrashPolicy::ALL {
                    assert!(!policy_name(p).is_empty());
                }
                let mut hub = Hub::in_memory();
                let mut s = PrivacyState::default();
                let ctx = egui::Context::default();
                let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                    section(ui, &mut hub, &mut s);
                });
                assert!(s.id().is_some());
            });
        }
    }
}
