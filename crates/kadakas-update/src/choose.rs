//! Which version starts (PLAT-16). The choice is a pure function of the installed versions, the
//! settings and the list of versions that must be skipped. It is tested without any program.
//!
//! - **Channel.** A version has a label (`stable`, `nightly`, `local`, or `bundled`). The channel
//!   `stable` starts stable and bundled versions. `nightly` starts those and nightly ones.
//!   `local` starts all of them and prefers the newest local build (PLAT-17). A local build is
//!   never chosen by itself on another channel.
//! - **Mode.** *Auto-update* starts the newest eligible version. *Auto-download* and *Notify
//!   only* start the version that the person chose (the pointer of the store): a download does
//!   not change what starts, a switch does.
//! - **Skipped versions.** A version whose last start ended without the clean-exit mark, a
//!   version the person rolled back from, and a version that could not be started are skipped.
//!   The launcher takes the next one and tells the person which version it skipped and why.
//! - **Fallback.** If every version is skipped, the bundled app starts anyway (it is always
//!   there), else the first one that can be started. Something always starts.

use crate::handshake::{SkipReason, Skipped};
use crate::manifest::Channel;
use crate::settings::{Settings, UpdateMode};
use crate::store::{Active, BUNDLED_LABEL, Installed, Slot, Store};
use std::collections::BTreeSet;

/// Whether a version with this label may start by itself on this channel.
pub fn eligible(label: &str, channel: Channel) -> bool {
    match (label, channel) {
        ("local", c) => c == Channel::Local,
        ("nightly", c) => c != Channel::Stable,
        _ => true,
    }
}

/// The result of the choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub active: Active,
    /// The versions that were better by rank and were skipped, with the reason.
    pub skipped: Vec<Skipped>,
    /// Every version was skipped; this one starts anyway.
    pub forced: bool,
}

fn active_of(i: &Installed) -> Active {
    Active {
        slot: i.slot.clone(),
        version: i.version.clone(),
        dir: i.dir.clone(),
    }
}

/// Everything the choice reads.
pub struct Inputs<'a> {
    pub installed: &'a [Installed],
    /// The version that the pointer of the store names (the choice of the person).
    pub pointer: Option<&'a Slot>,
    pub settings: &'a Settings,
    pub crashed: &'a BTreeSet<Slot>,
    pub rolled_back: &'a BTreeSet<Slot>,
    /// Versions that could not be started in this run of the launcher.
    pub failed: &'a [Slot],
}

fn skip_reason(i: &Installed, inp: &Inputs<'_>) -> Option<SkipReason> {
    if inp.failed.contains(&i.slot) {
        Some(SkipReason::NotStarted)
    } else if inp.crashed.contains(&i.slot) {
        Some(SkipReason::Crashed)
    } else if inp.rolled_back.contains(&i.slot) {
        Some(SkipReason::RolledBack)
    } else {
        None
    }
}

/// The versions in the order in which they are tried.
fn ranked<'a>(inp: &Inputs<'a>) -> Vec<&'a Installed> {
    let channel = inp.settings.channel;
    let mut rest: Vec<&Installed> = inp
        .installed
        .iter()
        .filter(|i| eligible(&i.meta.label, channel))
        .collect();
    // Newest first. On the channel local, local builds come first, the last added first.
    rest.sort_by(|a, b| {
        let local = |i: &Installed| channel == Channel::Local && i.meta.label == "local";
        local(b)
            .cmp(&local(a))
            .then_with(|| {
                if local(a) {
                    b.meta.added.cmp(&a.meta.added)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| b.version.cmp(&a.version))
            .then_with(|| b.meta.added.cmp(&a.meta.added))
    });
    if inp.settings.mode == UpdateMode::AutoUpdate {
        return rest;
    }
    // The person's choice comes first, whatever its channel.
    let mut out: Vec<&Installed> = Vec::with_capacity(rest.len() + 1);
    if let Some(p) = inp.pointer.and_then(|s| inp.installed.iter().find(|i| &i.slot == s)) {
        out.push(p);
    }
    for i in rest {
        if !out.iter().any(|o| o.slot == i.slot) {
            out.push(i);
        }
    }
    out
}

/// Chooses the version to start. `None` only when nothing can be started at all.
pub fn choose_from(inp: &Inputs<'_>) -> Option<Choice> {
    let order = ranked(inp);
    let mut skipped = Vec::new();
    for i in &order {
        match skip_reason(i, inp) {
            None => {
                return Some(Choice {
                    active: active_of(i),
                    skipped,
                    forced: false,
                });
            }
            Some(reason) => skipped.push(Skipped {
                version: i.version.to_string(),
                reason,
            }),
        }
    }
    // Everything is skipped. The bundled app is the fallback that is always there.
    let startable = |i: &&Installed| !inp.failed.contains(&i.slot);
    let last_resort = inp
        .installed
        .iter()
        .find(|i| i.meta.label == BUNDLED_LABEL && startable(i))
        .or_else(|| order.iter().copied().find(startable))
        .or_else(|| inp.installed.iter().find(startable))?;
    // The version that starts anyway is not "skipped".
    skipped.retain(|s| s.version != last_resort.version.to_string());
    Some(Choice {
        active: active_of(last_resort),
        skipped,
        forced: true,
    })
}

/// Chooses from the store on disk.
pub fn choose(store: &Store, settings: &Settings, failed: &[Slot]) -> Option<Choice> {
    let installed = store.list();
    let pointer = store.active().map(|a| a.slot);
    choose_from(&Inputs {
        installed: &installed,
        pointer: pointer.as_ref(),
        settings,
        crashed: &store.crashed_slots(),
        rolled_back: &store.rolled_back_slots(),
        failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::VersionMeta;
    use crate::store::tests::{bundled, tmp, v};
    use std::path::PathBuf;

    fn inst(ver: &str, label: &str, added: u64) -> Installed {
        let slot = if label == BUNDLED_LABEL {
            Slot::Bundled
        } else {
            Slot::Version(v(ver))
        };
        Installed {
            slot,
            version: v(ver),
            dir: PathBuf::from(format!("/v/{ver}")),
            active: false,
            previous: false,
            meta: VersionMeta {
                label: label.into(),
                commit: None,
                added,
                kept: false,
            },
        }
    }

    fn settings(mode: UpdateMode, channel: Channel) -> Settings {
        Settings {
            mode,
            channel,
            ..Settings::default()
        }
    }

    struct Case {
        installed: Vec<Installed>,
        pointer: Option<Slot>,
        crashed: BTreeSet<Slot>,
        rolled: BTreeSet<Slot>,
        failed: Vec<Slot>,
    }

    impl Case {
        fn new(installed: Vec<Installed>) -> Case {
            Case {
                installed,
                pointer: None,
                crashed: BTreeSet::new(),
                rolled: BTreeSet::new(),
                failed: Vec::new(),
            }
        }
        fn run(&self, s: &Settings) -> Option<Choice> {
            choose_from(&Inputs {
                installed: &self.installed,
                pointer: self.pointer.as_ref(),
                settings: s,
                crashed: &self.crashed,
                rolled_back: &self.rolled,
                failed: &self.failed,
            })
        }
    }

    fn picked(c: Option<Choice>) -> String {
        c.map_or("none".into(), |c| c.active.version.to_string())
    }

    fn basic() -> Case {
        Case::new(vec![
            inst("1.0.0", BUNDLED_LABEL, 0),
            inst("1.1.0", "stable", 10),
            inst("1.2.0", "stable", 20),
        ])
    }

    #[test]
    fn eligibility_by_channel() {
        for label in ["stable", BUNDLED_LABEL, "anything"] {
            for c in [Channel::Stable, Channel::Beta, Channel::Local] {
                assert!(eligible(label, c), "{label} {c:?}");
            }
        }
        assert!(!eligible("nightly", Channel::Stable));
        assert!(eligible("nightly", Channel::Beta));
        assert!(eligible("nightly", Channel::Local));
        assert!(!eligible("local", Channel::Stable));
        assert!(!eligible("local", Channel::Beta));
        assert!(eligible("local", Channel::Local));
    }

    #[test]
    fn auto_update_starts_the_newest() {
        let c = basic();
        assert_eq!(picked(c.run(&settings(UpdateMode::AutoUpdate, Channel::Stable))), "1.2.0");
        // The pointer does not matter in this mode.
        let mut c = basic();
        c.pointer = Some(Slot::Bundled);
        let got = c.run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).unwrap();
        assert_eq!(got.active.version, v("1.2.0"));
        assert!(got.skipped.is_empty() && !got.forced);
    }

    #[test]
    fn the_manual_modes_start_what_the_person_chose() {
        for mode in [UpdateMode::AutoDownload, UpdateMode::NotifyOnly] {
            let mut c = basic();
            c.pointer = Some(Slot::Version(v("1.1.0")));
            // 1.2.0 is downloaded and waits.
            assert_eq!(picked(c.run(&settings(mode, Channel::Stable))), "1.1.0");
            // No pointer: the newest.
            c.pointer = None;
            assert_eq!(picked(c.run(&settings(mode, Channel::Stable))), "1.2.0");
            // A pointer to a version that is gone is ignored.
            c.pointer = Some(Slot::Version(v("9.9.9")));
            assert_eq!(picked(c.run(&settings(mode, Channel::Stable))), "1.2.0");
        }
    }

    #[test]
    fn a_crashed_version_is_skipped_and_the_reason_is_told() {
        let mut c = basic();
        c.crashed.insert(Slot::Version(v("1.2.0")));
        let got = c.run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).unwrap();
        assert_eq!(got.active.version, v("1.1.0"));
        assert_eq!(
            got.skipped,
            [Skipped {
                version: "1.2.0".into(),
                reason: SkipReason::Crashed
            }]
        );
        // Two in a row.
        c.crashed.insert(Slot::Version(v("1.1.0")));
        let got = c.run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).unwrap();
        assert_eq!(got.active.slot, Slot::Bundled);
        assert_eq!(got.skipped.len(), 2);
        // The person's choice crashed, in a manual mode.
        let mut m = basic();
        m.pointer = Some(Slot::Version(v("1.1.0")));
        m.crashed.insert(Slot::Version(v("1.1.0")));
        let got = m.run(&settings(UpdateMode::NotifyOnly, Channel::Stable)).unwrap();
        assert_eq!(got.active.version, v("1.2.0"));
        assert_eq!(got.skipped[0].version, "1.1.0");
    }

    #[test]
    fn a_rollback_and_a_failed_start_skip_too() {
        let mut c = basic();
        c.rolled.insert(Slot::Version(v("1.2.0")));
        c.failed.push(Slot::Version(v("1.1.0")));
        let got = c.run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).unwrap();
        assert_eq!(got.active.slot, Slot::Bundled);
        let reasons: Vec<SkipReason> = got.skipped.iter().map(|s| s.reason).collect();
        assert_eq!(reasons, [SkipReason::RolledBack, SkipReason::NotStarted]);
    }

    #[test]
    fn the_bundled_app_is_the_last_resort() {
        let mut c = basic();
        for i in &c.installed {
            c.crashed.insert(i.slot.clone());
        }
        let got = c.run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).unwrap();
        assert!(got.forced);
        assert_eq!(got.active.slot, Slot::Bundled);
        assert_eq!(got.skipped.len(), 2, "the bundled app is not 'skipped'");
        // The bundled app cannot be started either: the best of the others.
        c.failed.push(Slot::Bundled);
        let got = c.run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).unwrap();
        assert!(got.forced);
        assert_eq!(got.active.version, v("1.2.0"));
        // Nothing can be started.
        for i in &c.installed {
            c.failed.push(i.slot.clone());
        }
        assert!(c.run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).is_none());
        // Without a bundled app, the others are the fallback.
        let mut d = Case::new(vec![inst("1.1.0", "stable", 1), inst("1.2.0", "stable", 2)]);
        d.crashed.insert(Slot::Version(v("1.1.0")));
        d.crashed.insert(Slot::Version(v("1.2.0")));
        let got = d.run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).unwrap();
        assert!(got.forced);
        assert_eq!(got.active.version, v("1.2.0"));
        assert!(Case::new(vec![]).run(&settings(UpdateMode::AutoUpdate, Channel::Stable)).is_none());
    }

    #[test]
    fn the_channel_filters_nightly_and_local_versions() {
        let c = Case::new(vec![
            inst("1.0.0", BUNDLED_LABEL, 0),
            inst("1.1.0", "stable", 10),
            inst("1.2.0-beta.1", "nightly", 20),
            inst("1.1.0-local.g1", "local", 30),
        ]);
        let auto = |ch| picked(c.run(&settings(UpdateMode::AutoUpdate, ch)));
        assert_eq!(auto(Channel::Stable), "1.1.0");
        assert_eq!(auto(Channel::Beta), "1.2.0-beta.1");
        // On the channel local the local build wins over a newer version.
        assert_eq!(auto(Channel::Local), "1.1.0-local.g1");
        // A local build never starts by itself on another channel, not even in a manual mode,
        // unless it is the person's choice.
        let mut m = Case::new(c.installed.clone());
        assert_eq!(
            picked(m.run(&settings(UpdateMode::NotifyOnly, Channel::Stable))),
            "1.1.0"
        );
        m.pointer = Some(Slot::Version(v("1.1.0-local.g1")));
        assert_eq!(
            picked(m.run(&settings(UpdateMode::NotifyOnly, Channel::Stable))),
            "1.1.0-local.g1"
        );
    }

    #[test]
    fn the_newest_local_build_wins_on_the_local_channel() {
        let mut c = Case::new(vec![
            inst("1.0.0", BUNDLED_LABEL, 0),
            inst("1.0.0-local.ga", "local", 50),
            inst("1.0.0-local.gb", "local", 70),
            inst("1.0.0-local.gc", "local", 60),
        ]);
        let s = settings(UpdateMode::AutoUpdate, Channel::Local);
        assert_eq!(picked(c.run(&s)), "1.0.0-local.gb");
        // If it crashed, the next local build, not an older release.
        c.crashed.insert(Slot::Version(v("1.0.0-local.gb")));
        assert_eq!(picked(c.run(&s)), "1.0.0-local.gc");
        // With no local build at all, the channel local starts the newest release.
        let r = Case::new(vec![inst("1.0.0", BUNDLED_LABEL, 0), inst("1.1.0", "stable", 5)]);
        assert_eq!(picked(r.run(&s)), "1.1.0");
    }

    #[test]
    fn the_choice_reads_the_store_on_disk() {
        let root = tmp("choose-disk");
        let s = Store::new(root.join("r"), Some(bundled(&root, "1.0.0")));
        s.install(&v("1.1.0"), &crate::store::tests::archive_file(&root, "1.1.0"), false)
            .unwrap();
        let st = settings(UpdateMode::AutoUpdate, Channel::Stable);
        assert_eq!(picked(choose(&s, &st, &[])), "1.1.0");
        s.note_run_end(&Slot::Version(v("1.1.0")), false).unwrap();
        assert_eq!(picked(choose(&s, &st, &[])), "1.0.0");
        let empty = Store::new(root.join("none"), None);
        assert!(choose(&empty, &st, &[]).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
