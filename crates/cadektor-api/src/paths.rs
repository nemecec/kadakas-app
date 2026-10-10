//! Which files an agent may read and write. Native only.
//!
//! A session that serves agents (`cadektor serve`, `cadektor mcp`, the app's agent server) is restricted to
//! a few folders: the folder of the open document, plus the folders the owner allows with `--allow-dir`
//! (default: the current folder). Every path is canonicalized first, so `..` and symlinks cannot leave
//! the allowed folders. A session without a policy (a library user, the `cadektor import` command) can use any path.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathPolicy {
    /// Canonical allowed folders. `None`: every path is allowed.
    roots: Option<Vec<PathBuf>>,
    /// May agents start an external converter (DWG)? Off by default. Needs `--allow-converters`.
    /// A session without a path policy (a library user) and the dialogs of the user are not limited.
    pub allow_converters: bool,
}

impl PathPolicy {
    pub fn unrestricted() -> Self {
        PathPolicy {
            roots: None,
            allow_converters: false,
        }
    }

    /// Allows (or forbids) agents to start external converters.
    pub fn with_converters(mut self, allow: bool) -> Self {
        self.allow_converters = allow;
        self
    }

    /// True if a request from an agent may start an external converter.
    pub fn converters_allowed(&self) -> bool {
        self.roots.is_none() || self.allow_converters
    }

    /// Allows these folders (and the folder of the document). Folders that do not exist are dropped.
    pub fn restricted(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        PathPolicy {
            roots: Some(
                roots
                    .into_iter()
                    .filter_map(|r| std::fs::canonicalize(r).ok())
                    .collect(),
            ),
            allow_converters: false,
        }
    }

    pub fn is_restricted(&self) -> bool {
        self.roots.is_some()
    }

    /// The allowed folders, without the folder of the document.
    pub fn roots(&self) -> &[PathBuf] {
        self.roots.as_deref().unwrap_or(&[])
    }

    /// The canonical form of `path` if the policy allows it. `doc_dir` is the folder of the open document.
    /// For a write, the file may be new, but its folder must exist.
    pub fn resolve(
        &self,
        path: &Path,
        doc_dir: Option<&Path>,
        write: bool,
    ) -> Result<PathBuf, String> {
        let canonical = if write {
            canonical_for_write(path)?
        } else {
            std::fs::canonicalize(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?
        };
        let Some(roots) = &self.roots else {
            return Ok(canonical);
        };
        let doc_dir = doc_dir.and_then(|d| std::fs::canonicalize(d).ok());
        if roots
            .iter()
            .chain(doc_dir.iter())
            .any(|r| canonical.starts_with(r))
        {
            return Ok(canonical);
        }
        let list: Vec<String> = roots
            .iter()
            .chain(doc_dir.iter())
            .map(|r| r.display().to_string())
            .collect();
        Err(format!(
            "{} is outside the folders that agents may use ({}). The owner can allow a folder with --allow-dir",
            path.display(),
            if list.is_empty() {
                "none".to_string()
            } else {
                list.join(", ")
            }
        ))
    }
}

/// Canonical path of a file that may not exist yet: the folder is canonicalized, then the name is added.
fn canonical_for_write(path: &Path) -> Result<PathBuf, String> {
    // A link, also a broken one, must be followed to its target: never write "through" a name that escapes.
    if std::fs::symlink_metadata(path).is_ok() {
        return std::fs::canonicalize(path)
            .map_err(|e| format!("cannot use {}: {e}", path.display()));
    }
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} is not a file name", path.display()))?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let dir = std::fs::canonicalize(&dir)
        .map_err(|e| format!("the folder of {} does not exist: {e}", path.display()))?;
    Ok(dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadektor-paths-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn unrestricted_allows_everything_that_exists() {
        let d = dir("free");
        std::fs::write(d.join("a.txt"), "x").unwrap();
        let p = PathPolicy::unrestricted();
        assert!(!p.is_restricted());
        assert!(p.roots().is_empty());
        assert!(p.resolve(&d.join("a.txt"), None, false).is_ok());
        assert!(p.resolve(&d.join("missing.txt"), None, false).is_err());
        assert!(p.resolve(&d.join("new.txt"), None, true).is_ok());
        assert!(
            p.resolve(&d.join("nodir").join("new.txt"), None, true)
                .is_err()
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn restricted_keeps_paths_inside_the_roots() {
        let root = dir("root");
        let other = dir("other");
        std::fs::write(root.join("a.dxf"), "x").unwrap();
        std::fs::write(other.join("secret.dxf"), "x").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let p = PathPolicy::restricted([root.clone(), PathBuf::from("/nonexistent-folder")]);
        assert!(p.is_restricted());
        assert_eq!(p.roots().len(), 1);
        assert!(p.resolve(&root.join("a.dxf"), None, false).is_ok());
        assert!(
            p.resolve(&root.join("sub").join("new.dxf"), None, true)
                .is_ok()
        );
        // Outside, also with `..`.
        let e = p
            .resolve(&other.join("secret.dxf"), None, false)
            .unwrap_err();
        assert!(e.contains("--allow-dir"), "{e}");
        assert!(
            p.resolve(
                &root
                    .join("..")
                    .join(other.file_name().unwrap())
                    .join("secret.dxf"),
                None,
                false
            )
            .is_err()
        );
        assert!(p.resolve(&other.join("new.dxf"), None, true).is_err());
        // The folder of the document is allowed too.
        assert!(
            p.resolve(&other.join("secret.dxf"), Some(&other), false)
                .is_ok()
        );
        // A symlink inside the root that points out is outside.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&other, root.join("link")).unwrap();
            assert!(
                p.resolve(&root.join("link").join("secret.dxf"), None, false)
                    .is_err()
            );
            std::os::unix::fs::symlink(other.join("secret.dxf"), root.join("out.dxf")).unwrap();
            assert!(p.resolve(&root.join("out.dxf"), None, true).is_err());
            std::os::unix::fs::symlink(other.join("not-there.dxf"), root.join("dangling.dxf"))
                .unwrap();
            assert!(p.resolve(&root.join("dangling.dxf"), None, true).is_err());
        }
        // No allowed folder at all.
        let none = PathPolicy::restricted([PathBuf::from("/nonexistent-folder")]);
        assert!(
            none.resolve(&root.join("a.dxf"), None, false)
                .unwrap_err()
                .contains("none")
        );
        assert!(p.resolve(Path::new(".."), None, true).is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(other);
    }
}
