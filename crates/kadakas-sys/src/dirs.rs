//! The per-user folders of Kadakas: config, state, runtime and cache.
//!
//! Each function returns the `kadakas` folder inside the base folder of the platform. The
//! `*_for` variants take the system name and an environment lookup, so tests can check every
//! platform. An empty variable counts as not set.

use std::ffi::OsString;
use std::path::PathBuf;

const APP: &str = "kadakas";

fn real_env(k: &str) -> Option<OsString> {
    std::env::var_os(k)
}

fn var(env: &impl Fn(&str) -> Option<OsString>, k: &str) -> Option<PathBuf> {
    env(k).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn mac_support(env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    Some(
        var(env, "HOME")?
            .join("Library")
            .join("Application Support"),
    )
}

/// The home folder of the user: `HOME`, else `USERPROFILE`.
pub fn home_dir() -> Option<PathBuf> {
    home_dir_for(real_env)
}

pub fn home_dir_for(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    var(&env, "HOME").or_else(|| var(&env, "USERPROFILE"))
}

/// Settings of the user: keymap, trusted plug-ins, preferences.
/// Windows `%APPDATA%\kadakas`, macOS `~/Library/Application Support/kadakas`,
/// elsewhere `$XDG_CONFIG_HOME/kadakas` or `~/.config/kadakas`.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_for(std::env::consts::OS, real_env)
}

pub fn config_dir_for(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let base = match os {
        "windows" => var(&env, "APPDATA")?,
        "macos" => mac_support(&env)?,
        _ => match var(&env, "XDG_CONFIG_HOME") {
            Some(x) => x,
            None => var(&env, "HOME")?.join(".config"),
        },
    };
    Some(base.join(APP))
}

/// Data that the app keeps between runs, such as recovery files.
/// Windows `%LOCALAPPDATA%\kadakas`, macOS `~/Library/Application Support/kadakas`,
/// elsewhere `$XDG_STATE_HOME/kadakas` or `~/.local/state/kadakas`.
pub fn state_dir() -> Option<PathBuf> {
    state_dir_for(std::env::consts::OS, real_env)
}

pub fn state_dir_for(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let base = match os {
        "windows" => var(&env, "LOCALAPPDATA")?,
        "macos" => mac_support(&env)?,
        _ => match var(&env, "XDG_STATE_HOME") {
            Some(x) => x,
            None => var(&env, "HOME")?.join(".local").join("state"),
        },
    };
    Some(base.join(APP))
}

/// Files of a running app: where it publishes how to reach it.
/// Like the state folder, but `$XDG_RUNTIME_DIR/kadakas` comes first on Unix.
pub fn runtime_dir() -> Option<PathBuf> {
    runtime_dir_for(std::env::consts::OS, real_env)
}

pub fn runtime_dir_for(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if os != "windows"
        && let Some(x) = var(&env, "XDG_RUNTIME_DIR")
    {
        return Some(x.join(APP));
    }
    let base = match os {
        "windows" => var(&env, "LOCALAPPDATA")?,
        "macos" => mac_support(&env)?,
        _ => var(&env, "HOME")?.join(".local").join("state"),
    };
    Some(base.join(APP))
}

/// Data that can be fetched again, such as map tiles.
/// macOS `~/Library/Caches/kadakas`, elsewhere `$XDG_CACHE_HOME/kadakas` or `~/.cache/kadakas`.
/// Without a home folder it is a folder in the temporary folder.
pub fn cache_dir() -> PathBuf {
    cache_dir_for(std::env::consts::OS, real_env)
}

pub fn cache_dir_for(os: &str, env: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    let base = if os == "macos" {
        var(&env, "HOME").map(|h| h.join("Library").join("Caches"))
    } else {
        var(&env, "XDG_CACHE_HOME").or_else(|| var(&env, "HOME").map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join(APP)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |k: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(*v))
        }
    }

    fn tail(p: Option<PathBuf>) -> Option<String> {
        p.map(|p| p.to_string_lossy().replace('\\', "/"))
    }

    #[test]
    fn config_folder_per_platform() {
        let t = |os, e| tail(config_dir_for(os, e));
        assert_eq!(
            t("macos", env(&[("HOME", "/Users/me")])).as_deref(),
            Some("/Users/me/Library/Application Support/kadakas")
        );
        assert_eq!(
            t("linux", env(&[("HOME", "/h")])).as_deref(),
            Some("/h/.config/kadakas")
        );
        assert_eq!(
            t("linux", env(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")])).as_deref(),
            Some("/x/kadakas")
        );
        assert_eq!(
            t("windows", env(&[("APPDATA", "C:/A")])).as_deref(),
            Some("C:/A/kadakas")
        );
        assert_eq!(t("linux", env(&[])), None);
        assert_eq!(
            t("linux", env(&[("XDG_CONFIG_HOME", ""), ("HOME", "")])),
            None
        );
    }

    #[test]
    fn state_folder_per_platform() {
        let t = |os, e| tail(state_dir_for(os, e));
        assert_eq!(
            t("macos", env(&[("HOME", "/Users/me")])).as_deref(),
            Some("/Users/me/Library/Application Support/kadakas")
        );
        assert_eq!(
            t("linux", env(&[("HOME", "/h")])).as_deref(),
            Some("/h/.local/state/kadakas")
        );
        assert_eq!(
            t("linux", env(&[("XDG_STATE_HOME", "/s")])).as_deref(),
            Some("/s/kadakas")
        );
        assert_eq!(
            t("windows", env(&[("LOCALAPPDATA", "C:/L")])).as_deref(),
            Some("C:/L/kadakas")
        );
        assert_eq!(t("windows", env(&[("HOME", "/h")])), None);
    }

    #[test]
    fn runtime_folder_per_platform() {
        let t = |os, e| tail(runtime_dir_for(os, e));
        assert_eq!(
            t(
                "linux",
                env(&[("XDG_RUNTIME_DIR", "/run/user/1"), ("HOME", "/h")])
            )
            .as_deref(),
            Some("/run/user/1/kadakas")
        );
        assert_eq!(
            t("linux", env(&[("HOME", "/h")])).as_deref(),
            Some("/h/.local/state/kadakas")
        );
        assert_eq!(
            t("macos", env(&[("HOME", "/Users/me")])).as_deref(),
            Some("/Users/me/Library/Application Support/kadakas")
        );
        assert!(
            t("macos", env(&[("XDG_RUNTIME_DIR", ""), ("HOME", "/h")]))
                .unwrap()
                .ends_with("Application Support/kadakas")
        );
        assert_eq!(
            t("windows", env(&[("LOCALAPPDATA", "C:/L")])).as_deref(),
            Some("C:/L/kadakas")
        );
    }

    #[test]
    fn cache_and_home_folders() {
        assert_eq!(
            tail(Some(cache_dir_for("macos", env(&[("HOME", "/Users/me")])))).as_deref(),
            Some("/Users/me/Library/Caches/kadakas")
        );
        assert_eq!(
            tail(Some(cache_dir_for("linux", env(&[("HOME", "/h")])))).as_deref(),
            Some("/h/.cache/kadakas")
        );
        assert_eq!(
            tail(Some(cache_dir_for(
                "linux",
                env(&[("XDG_CACHE_HOME", "/c")])
            )))
            .as_deref(),
            Some("/c/kadakas")
        );
        assert!(cache_dir_for("linux", env(&[])).starts_with(std::env::temp_dir()));
        assert_eq!(
            tail(home_dir_for(env(&[("USERPROFILE", "C:/U")]))).as_deref(),
            Some("C:/U")
        );
        assert_eq!(
            tail(home_dir_for(env(&[
                ("HOME", "/h"),
                ("USERPROFILE", "C:/U")
            ])))
            .as_deref(),
            Some("/h")
        );
        // The real lookups run without a panic.
        let _ = (
            config_dir(),
            state_dir(),
            runtime_dir(),
            cache_dir(),
            home_dir(),
        );
    }
}
