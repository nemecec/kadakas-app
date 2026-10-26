//! The test mode of the welcome dialog, for the smoke test of the installers (PLAT-28).
//!
//! `KADAKAS_SMOKE_STATE=<path>` turns it on. The dialog then writes a small JSON file with the
//! version, the kind of the dialog and the values that it shows. It waits for
//! `KADAKAS_SMOKE_HOLD_SECS` seconds (default 3, at most 60), so that a screenshot can be taken.
//! Then it answers like the second button ("Use defaults" on a first start) and ends. Without
//! the variable (or with an empty one) nothing changes. See `docs/welcome.md`.

use crate::choices::Choices;
use crate::gate::Kind;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The file that the dialog writes. Turns the test mode on.
pub const ENV_STATE: &str = "KADAKAS_SMOKE_STATE";
/// Seconds between the state file and the automatic answer.
pub const ENV_HOLD: &str = "KADAKAS_SMOKE_HOLD_SECS";

const DEFAULT_HOLD: Duration = Duration::from_secs(3);
const MAX_HOLD: Duration = Duration::from_secs(60);
/// Frames that are drawn before the state is written: the window really shows the dialog.
const FRAMES_BEFORE_WRITE: u32 = 3;

/// The hold time from the text of the variable. A wrong or missing value gives the default.
pub fn hold(value: Option<&str>) -> Duration {
    value
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map_or(DEFAULT_HOLD, |s| Duration::from_secs_f64(s).min(MAX_HOLD))
}

/// What the dialog does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    /// Write the state file now, then call [`Auto::written`].
    Write,
    /// Answer with the second button and close.
    Answer,
}

/// The automatic run of the dialog.
#[derive(Debug)]
pub struct Auto {
    path: PathBuf,
    hold: Duration,
    frames: u32,
    written_at: Option<Instant>,
}

impl Auto {
    /// The test mode from the environment (`get` reads a variable). `None` when it is off.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Option<Auto> {
        let path = get(ENV_STATE).filter(|p| !p.trim().is_empty())?;
        Some(Auto {
            path: PathBuf::from(path),
            hold: hold(get(ENV_HOLD).as_deref()),
            frames: 0,
            written_at: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Called once per frame.
    pub fn step(&mut self, now: Instant) -> Step {
        self.frames = self.frames.saturating_add(1);
        match self.written_at {
            None if self.frames > FRAMES_BEFORE_WRITE => Step::Write,
            None => Step::Wait,
            Some(t) if now.saturating_duration_since(t) >= self.hold => Step::Answer,
            Some(_) => Step::Wait,
        }
    }

    /// The state file is written: the hold time starts.
    pub fn written(&mut self, now: Instant) {
        self.written_at = Some(now);
    }
}

fn dbg_lower<T: std::fmt::Debug>(v: &T) -> String {
    format!("{v:?}").to_lowercase()
}

/// The content of the state file.
pub fn state_json(
    version: &str,
    kind: Kind,
    shown: &Choices,
    language: &str,
    modeler_installed: bool,
) -> Value {
    json!({
        "version": version,
        "kind": match kind {
            Kind::FirstRun => "first_run",
            Kind::Update => "update",
            Kind::Manual => "manual",
        },
        "modeler_installed": modeler_installed,
        "shown": {
            "mode": shown.mode.flavor(),
            "language": language,
            "units": dbg_lower(&shown.units),
            "number": shown.number.example(),
            "theme": shown.theme.key(),
            "navigation": shown.navigation.key(),
            "crash": dbg_lower(&shown.crash),
            "updates": dbg_lower(&shown.updates),
        },
    })
}

/// Writes the state file in one step (a temporary file, then a rename), so that a reader never
/// sees half of it.
pub fn write_state(path: &Path, state: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::choices::detect;
    use crate::sysloc::SystemLocale;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn without_the_variable_the_mode_is_off() {
        assert!(Auto::from_env(env(&[])).is_none());
        assert!(Auto::from_env(env(&[(ENV_STATE, "")])).is_none());
        assert!(Auto::from_env(env(&[(ENV_STATE, "  ")])).is_none());
        // The hold time alone does nothing.
        assert!(Auto::from_env(env(&[(ENV_HOLD, "1")])).is_none());
    }

    #[test]
    fn the_hold_time_has_a_default_and_a_limit() {
        assert_eq!(hold(None), Duration::from_secs(3));
        assert_eq!(hold(Some("x")), Duration::from_secs(3));
        assert_eq!(hold(Some("-1")), Duration::from_secs(3));
        assert_eq!(hold(Some("NaN")), Duration::from_secs(3));
        assert_eq!(hold(Some("0")), Duration::ZERO);
        assert_eq!(hold(Some("1.5")), Duration::from_millis(1500));
        assert_eq!(hold(Some("100000")), Duration::from_secs(60));
    }

    #[test]
    fn the_steps_are_wait_write_wait_answer() {
        let mut a = Auto::from_env(env(&[(ENV_STATE, "/x/s.json"), (ENV_HOLD, "2")])).unwrap();
        assert_eq!(a.path(), Path::new("/x/s.json"));
        let t0 = Instant::now();
        let mut steps = Vec::new();
        for _ in 0..FRAMES_BEFORE_WRITE {
            steps.push(a.step(t0));
        }
        assert!(steps.iter().all(|s| *s == Step::Wait), "{steps:?}");
        // It keeps asking for the write until the caller says that it worked.
        assert_eq!(a.step(t0), Step::Write);
        assert_eq!(a.step(t0), Step::Write);
        a.written(t0);
        assert_eq!(a.step(t0 + Duration::from_secs(1)), Step::Wait);
        assert_eq!(a.step(t0 + Duration::from_secs(2)), Step::Answer);
    }

    #[test]
    fn the_state_has_the_version_the_kind_and_the_shown_values() {
        let c = detect(&SystemLocale::from_languages(vec!["et-EE".into()]), true);
        let v = state_json("0.1.0", Kind::FirstRun, &c, "et", true);
        assert_eq!(v["version"], "0.1.0");
        assert_eq!(v["kind"], "first_run");
        assert_eq!(v["modeler_installed"], true);
        assert_eq!(v["shown"]["mode"], "kadakas-modeler");
        assert_eq!(v["shown"]["language"], "et");
        assert_eq!(v["shown"]["units"], "metric");
        assert_eq!(v["shown"]["number"], "1\u{a0}234,56");
        assert_eq!(v["shown"]["theme"], "system");
        assert_eq!(v["shown"]["navigation"], "mouse");
        assert!(v["shown"]["crash"].is_string());
        assert!(v["shown"]["updates"].is_string());
        for k in [Kind::Update, Kind::Manual] {
            let v = state_json("1", k, &c, "en-US", false);
            assert_ne!(v["kind"], "first_run");
        }
    }

    #[test]
    fn the_state_file_is_written_whole_and_leaves_no_temporary_file() {
        let dir = crate::gate::tests::tmp("smoke-state");
        let path = dir.join("deep").join("state.json");
        let c = detect(&SystemLocale::default(), false);
        let v = state_json("0.1.0", Kind::FirstRun, &c, "en-US", false);
        write_state(&path, &v).unwrap();
        let back: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back, v);
        assert!(!path.with_file_name("state.json.tmp").exists());
        // A second write replaces the first.
        let v2 = state_json("0.2.0", Kind::Update, &c, "en-US", false);
        write_state(&path, &v2).unwrap();
        let back: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back["version"], "0.2.0");
        // A folder in the way is an error, not a panic.
        assert!(write_state(&dir, &v).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
