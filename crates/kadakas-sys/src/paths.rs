//! Paths as people and agents see them.

use std::path::{Path, PathBuf};

/// The canonical path on Windows has the prefix `\\?\` (`\\?\C:\x`, `\\?\UNC\server\share`). A
/// path shown to a person or an agent, or kept in a link, has the usual form (`C:\x`,
/// `\\server\share`), which a person can read and which the checks of a link know. Other
/// strings (also verbatim forms that have no usual form, like `\\?\Volume{1}\x`) stay as they are.
/// A pure string function: it gives the same result on every system.
pub fn strip_verbatim_str(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = path.strip_prefix("\\\\?\\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// [`strip_verbatim_str`] for a path. A path that is not text stays as it is.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    match path.to_str() {
        Some(s) => PathBuf::from(strip_verbatim_str(s)),
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verbatim_prefix_of_a_windows_path_is_taken_off() {
        for (given, want) in [
            ("\\\\?\\C:\\data\\a.dxf", "C:\\data\\a.dxf"),
            (
                "\\\\?\\UNC\\server\\share\\a.dxf",
                "\\\\server\\share\\a.dxf",
            ),
            ("\\\\?\\Volume{1}\\a.dxf", "\\\\?\\Volume{1}\\a.dxf"),
            ("C:\\data\\a.dxf", "C:\\data\\a.dxf"),
            ("\\\\server\\share\\a.dxf", "\\\\server\\share\\a.dxf"),
            ("/data/a.dxf", "/data/a.dxf"),
            ("", ""),
        ] {
            assert_eq!(strip_verbatim_str(given), want, "{given}");
        }
    }

    #[test]
    fn a_path_value_is_stripped_like_its_text() {
        assert_eq!(
            strip_verbatim(Path::new("\\\\?\\C:\\a")),
            PathBuf::from("C:\\a")
        );
    }
}
