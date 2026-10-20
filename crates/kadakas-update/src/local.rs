//! Local versions for developers (PLAT-17).
//!
//! - A local folder can be an update site: [`FileHttp`] reads `file://` addresses, and
//!   [`RoutingHttp`] sends `file://` to the folder and everything else to the network client.
//!   Which addresses a manifest may name is decided by the manifest code (`Sites`), so a network
//!   manifest can never make the launcher read a local file.
//! - A build on this machine can go straight into the version store: [`find_build`] reads a
//!   build folder, [`git_info`] reads the commit, [`local_version`] makes the version number.
//!   The store does the copy (`Store::add_local`).

use crate::error::{Result, UpdateError};
use crate::http::Http;
use crate::version::Version;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The folder of a `file://` address. `file:///home/x/site` is `/home/x/site`,
/// `file:///C:/site` is `C:/site`. A `%20` and the other percent codes are decoded. `None` for
/// an address that is not a `file://` address or has a host (`file://server/share`).
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // Only an empty host or `localhost` is local.
    let path = if let Some(p) = rest.strip_prefix("localhost/") {
        format!("/{p}")
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        return None;
    };
    let mut out = Vec::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = path.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    // `file:////server/share` and `file:///\\server\share` are network paths on Windows.
    if text.starts_with("//") || text.starts_with('\\') || text.starts_with("/\\") {
        return None;
    }
    // `/C:/dir` is a Windows drive path.
    let drive = text.as_bytes();
    let text = if drive.len() >= 3
        && drive[0] == b'/'
        && drive[1].is_ascii_alphabetic()
        && drive[2] == b':'
    {
        text[1..].to_string()
    } else {
        text
    };
    Some(PathBuf::from(text))
}

/// The `file://` address of a folder or file.
pub fn path_to_file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !text.starts_with('/') {
        out.push('/');
    }
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Reads `file://` addresses.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileHttp;

impl Http for FileHttp {
    fn get(&self, url: &str, limit: u64, out: &mut dyn Write) -> Result<u64> {
        let path = file_url_to_path(url)
            .ok_or_else(|| UpdateError::Http(format!("address not allowed: {url}")))?;
        let file = std::fs::File::open(&path)
            .map_err(|e| UpdateError::Http(format!("{}: {e}", path.display())))?;
        // The length of the file is not trusted (a FIFO or a device has none): read at most one
        // byte more than the limit.
        let n = std::io::copy(&mut file.take(limit.saturating_add(1)), out)?;
        if n > limit {
            return Err(UpdateError::Http(format!("more than {limit} bytes")));
        }
        Ok(n)
    }
}

/// Sends `file://` addresses to [`FileHttp`] and all others to the network client.
pub struct RoutingHttp {
    net: Arc<dyn Http>,
}

impl RoutingHttp {
    pub fn new(net: Arc<dyn Http>) -> RoutingHttp {
        RoutingHttp { net }
    }
}

impl Http for RoutingHttp {
    fn get(&self, url: &str, limit: u64, out: &mut dyn Write) -> Result<u64> {
        if url.starts_with("file://") {
            FileHttp.get(url, limit, out)
        } else {
            self.net.get(url, limit, out)
        }
    }
}

/// The git state of a build: the short commit and whether the tree had changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitInfo {
    pub commit: String,
    pub dirty: bool,
}

/// Reads the output of `git rev-parse HEAD` and `git status --porcelain`.
pub fn parse_git(rev: &str, status: &str) -> Option<GitInfo> {
    let commit = rev.trim();
    if commit.len() < 7 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(GitInfo {
        commit: commit.to_ascii_lowercase(),
        dirty: !status.trim().is_empty(),
    })
}

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The commit of the git checkout that holds `dir`, if there is one and git is installed.
pub fn git_info(dir: &Path) -> Option<GitInfo> {
    let rev = git_output(dir, &["rev-parse", "HEAD"])?;
    let status = git_output(dir, &["status", "--porcelain", "--untracked-files=no"])?;
    parse_git(&rev, &status)
}

/// The version number of a local build: `<base>-local.g<short commit>`. A build of a changed
/// tree has the time too (`.d<seconds>`), and a build without git only the time (`.t<seconds>`),
/// so two different builds never share a folder. The `g` keeps a commit that is all digits
/// from being read as a number with leading zeros.
pub fn local_version(base: &Version, git: Option<&GitInfo>, now: u64) -> Result<Version> {
    let id = match git {
        Some(g) if !g.dirty => format!("g{}", &g.commit[..g.commit.len().min(10)]),
        Some(g) => format!("g{}.d{now}", &g.commit[..g.commit.len().min(10)]),
        None => format!("t{now}"),
    };
    Version::parse(&format!(
        "{}.{}.{}-local.{id}",
        base.major, base.minor, base.patch
    ))
}

/// A build that `add` can put in the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalBuild {
    /// The folder that holds the files.
    pub dir: PathBuf,
    /// The files to copy, relative to `dir`: the app, the `kadakas` tool and the other UI
    /// flavors (`flavors.json` and its programs).
    pub files: Vec<PathBuf>,
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Finds the files of a build. `path` is the folder of the build (for example `target/release`)
/// or the app program in it. The app must be there; `kadakas` and the other flavors are copied
/// when they are.
pub fn find_build(path: &Path) -> Result<LocalBuild> {
    let dir = if path.is_file() {
        path.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| UpdateError::BadBuild(path.display().to_string()))?
    } else {
        path.to_path_buf()
    };
    let app = PathBuf::from(exe(crate::settings::MAIN_FLAVOR));
    if !dir.join(&app).is_file() {
        return Err(UpdateError::BadBuild(format!(
            "{} has no {}",
            dir.display(),
            app.display()
        )));
    }
    let mut files = vec![app];
    let cli = PathBuf::from(exe("kadakas"));
    if dir.join(&cli).is_file() {
        files.push(cli);
    }
    for f in crate::flavors::discover(&dir) {
        if !files.contains(&f.exe) {
            files.push(f.exe);
        }
    }
    if dir.join("flavors.json").is_file() {
        files.push(PathBuf::from("flavors.json"));
    }
    Ok(LocalBuild { dir, files })
}

/// The address of the update site (the folder with `manifest-<channel>.json`).
///
/// The order: the command line option `--site`, then for the channel `local` the site of the
/// settings, then the variable `KADAKAS_UPDATE_URL`, then (not for the channel `local`) the
/// default address. `None`: there is no site to ask. The channel `local` has no default: a local
/// version comes from a local folder or from `add`, never from the network.
pub fn resolve_base(
    cli_site: Option<&str>,
    env_url: Option<&str>,
    settings: &crate::settings::Settings,
    default: &str,
) -> Option<String> {
    let not_empty = |s: &&str| !s.trim().is_empty();
    let local = settings.channel == crate::manifest::Channel::Local;
    cli_site
        .filter(not_empty)
        .or(if local {
            Some(settings.site.as_str()).filter(not_empty)
        } else {
            None
        })
        .or(env_url.filter(not_empty))
        .or(if local { None } else { Some(default) })
        .map(|s| s.trim().to_string())
}

/// What `add` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub slot: crate::store::Slot,
    pub version: Version,
    /// The git commit, with `+dirty` if the tree had changes.
    pub commit: Option<String>,
}

/// Puts a build from this machine into the store with the label `local` (PLAT-17). `base` is
/// the version number of the program (the version of the workspace); the version in the store is
/// always a pre-release of it (`<base>-local.<id>`), so it can never be taken for a release.
/// `git` reads the commit of the build folder.
pub fn add(
    store: &crate::store::Store,
    path: &Path,
    base: &Version,
    now: u64,
    git: &dyn Fn(&Path) -> Option<GitInfo>,
) -> Result<Added> {
    let build = find_build(path)?;
    let info = git(&build.dir);
    let version = local_version(base, info.as_ref(), now)?;
    let commit = info.map(|g| {
        if g.dirty {
            format!("{}+dirty", g.commit)
        } else {
            g.commit
        }
    });
    let slot = store.add_local(&version, &build, commit.as_deref())?;
    Ok(Added {
        slot,
        version,
        commit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::store::tests::{tmp, v};

    #[test]
    fn file_addresses_become_paths_and_back() {
        assert_eq!(
            file_url_to_path("file:///home/x/my%20site"),
            Some(PathBuf::from("/home/x/my site"))
        );
        assert_eq!(
            file_url_to_path("file://localhost/tmp/s"),
            Some(PathBuf::from("/tmp/s"))
        );
        assert_eq!(
            file_url_to_path("file:///C:/site"),
            Some(PathBuf::from("C:/site"))
        );
        // A host, another scheme, a broken code, bytes that are not text.
        for bad in [
            "file://server/share",
            "https://x/y",
            "file:///a%zz",
            "file:///a%4",
            "file:///a%ff",
            "file:////server/share",
            "file:///%5C%5Cserver%5Cshare",
            "file://///server/share",
            "/plain/path",
        ] {
            assert_eq!(file_url_to_path(bad), None, "{bad}");
        }
        assert_eq!(path_to_file_url(Path::new("/a b/c")), "file:///a%20b/c");
        assert_eq!(path_to_file_url(Path::new("C:\\x\\y")), "file:///C:/x/y");
        let p = Path::new("/tmp/ü site/é");
        assert_eq!(file_url_to_path(&path_to_file_url(p)).as_deref(), Some(p));
    }

    #[test]
    fn file_http_reads_files_with_a_limit() {
        let d = tmp("filehttp");
        std::fs::write(d.join("a.txt"), b"hello").unwrap();
        let url = path_to_file_url(&d.join("a.txt"));
        let mut out = Vec::new();
        assert_eq!(FileHttp.get(&url, 10, &mut out).unwrap(), 5);
        assert_eq!(out, b"hello");
        assert!(FileHttp.get(&url, 4, &mut Vec::new()).is_err());
        assert!(
            FileHttp
                .get(&path_to_file_url(&d.join("none")), 10, &mut Vec::new())
                .is_err()
        );
        assert!(FileHttp.get("https://x/y", 10, &mut Vec::new()).is_err());
        // A device has no length: the limit still holds, and not more than limit + 1 bytes are read.
        #[cfg(unix)]
        {
            let mut out = Vec::new();
            assert!(FileHttp.get("file:///dev/zero", 100, &mut out).is_err());
            assert_eq!(out.len(), 101);
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn routing_sends_each_address_to_its_client() {
        let d = tmp("routing");
        std::fs::write(d.join("a.txt"), b"disk").unwrap();
        let net = Arc::new(crate::http::MockHttp::new());
        net.put("https://x.example/a", b"net".to_vec());
        let r = RoutingHttp::new(net.clone());
        let mut out = Vec::new();
        r.get(&path_to_file_url(&d.join("a.txt")), 10, &mut out)
            .unwrap();
        assert_eq!(out, b"disk");
        let mut out = Vec::new();
        r.get("https://x.example/a", 10, &mut out).unwrap();
        assert_eq!(out, b"net");
        assert_eq!(net.requests(), ["https://x.example/a"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn git_output_is_read() {
        let rev = "0123456789abcdef0123456789abcdef01234567\n";
        assert_eq!(
            parse_git(rev, ""),
            Some(GitInfo {
                commit: rev.trim().into(),
                dirty: false
            })
        );
        assert!(parse_git(rev, " M src/a.rs\n").unwrap().dirty);
        assert_eq!(parse_git("not a commit", ""), None);
        assert_eq!(parse_git("abc", ""), None);
    }

    #[test]
    fn local_versions_are_unique_and_valid() {
        let base = v("0.1.0");
        let clean = GitInfo {
            commit: "0123456789abcdef".into(),
            dirty: false,
        };
        let dirty = GitInfo {
            dirty: true,
            ..clean.clone()
        };
        assert_eq!(
            local_version(&base, Some(&clean), 5).unwrap().to_string(),
            "0.1.0-local.g0123456789"
        );
        assert_eq!(
            local_version(&base, Some(&dirty), 5).unwrap().to_string(),
            "0.1.0-local.g0123456789.d5"
        );
        assert_eq!(
            local_version(&base, None, 5).unwrap().to_string(),
            "0.1.0-local.t5"
        );
        // A local build is a pre-release of its base, so it sorts below the release.
        assert!(local_version(&base, None, 5).unwrap() < base);
    }

    #[test]
    fn a_build_needs_the_app() {
        let d = tmp("findbuild");
        assert!(matches!(find_build(&d), Err(UpdateError::BadBuild(_))));
        assert!(matches!(
            find_build(&d.join("nothing")),
            Err(UpdateError::BadBuild(_))
        ));
        std::fs::write(d.join(exe("kadakas-app")), b"x").unwrap();
        let b = find_build(&d).unwrap();
        assert_eq!(b.files, [PathBuf::from(exe("kadakas-app"))]);
        std::fs::write(d.join(exe("kadakas")), b"x").unwrap();
        std::fs::write(d.join(exe("kadakas-sketch")), b"x").unwrap();
        std::fs::write(
            d.join("flavors.json"),
            br#"{"flavors":[{"id":"sketch","name":"Sketch","exe":"kadakas-sketch"}]}"#,
        )
        .unwrap();
        // The app itself as the path works too.
        let b2 = find_build(&d.join(exe("kadakas-app"))).unwrap();
        assert_eq!(b2.dir, d);
        assert_eq!(
            b2.files,
            [
                PathBuf::from(exe("kadakas-app")),
                PathBuf::from(exe("kadakas")),
                PathBuf::from(exe("kadakas-sketch")),
                PathBuf::from("flavors.json"),
            ]
        );
        // The modeler needs no flavors.json: it is copied when it is there.
        std::fs::write(d.join(exe("kadakas-modeler")), b"x").unwrap();
        let b3 = find_build(&d).unwrap();
        assert!(b3.files.contains(&PathBuf::from(exe("kadakas-modeler"))));
        std::fs::remove_file(d.join("flavors.json")).unwrap();
        let b4 = find_build(&d).unwrap();
        assert_eq!(
            b4.files,
            [
                PathBuf::from(exe("kadakas-app")),
                PathBuf::from(exe("kadakas")),
                PathBuf::from(exe("kadakas-modeler")),
            ]
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_site_is_chosen_by_option_settings_variable_and_default() {
        use crate::manifest::Channel;
        use crate::settings::Settings;
        let stable = Settings::default();
        let local = Settings {
            channel: Channel::Local,
            site: "file:///s".into(),
            ..Settings::default()
        };
        let no_site = Settings {
            channel: Channel::Local,
            ..Settings::default()
        };
        let d = "https://d.example/m";
        assert_eq!(resolve_base(None, None, &stable, d).as_deref(), Some(d));
        assert_eq!(
            resolve_base(None, Some("https://e.example"), &stable, d).as_deref(),
            Some("https://e.example")
        );
        // The settings' site is for the channel local only.
        assert_eq!(
            resolve_base(None, None, &local, d).as_deref(),
            Some("file:///s")
        );
        let other = Settings {
            site: "file:///s".into(),
            ..Settings::default()
        };
        assert_eq!(resolve_base(None, None, &other, d).as_deref(), Some(d));
        // The option wins over everything.
        assert_eq!(
            resolve_base(Some("file:///o"), Some("https://e"), &local, d).as_deref(),
            Some("file:///o")
        );
        // The channel local has no default address.
        assert_eq!(resolve_base(None, None, &no_site, d), None);
        assert_eq!(
            resolve_base(None, Some("file:///v"), &no_site, d).as_deref(),
            Some("file:///v")
        );
        // Empty values do not count.
        assert_eq!(
            resolve_base(Some(" "), Some(""), &stable, d).as_deref(),
            Some(d)
        );
    }

    #[test]
    fn add_puts_a_build_in_the_store() {
        let root = tmp("add");
        let build = root.join("release");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join(exe("kadakas-app")), b"app").unwrap();
        let store = Store::new(root.join("r"), None);
        let clean = |_: &Path| {
            Some(GitInfo {
                commit: "0123456789abcdef".into(),
                dirty: false,
            })
        };
        let a = add(&store, &build, &v("0.1.0"), 7, &clean).unwrap();
        assert_eq!(a.version.to_string(), "0.1.0-local.g0123456789");
        assert_eq!(a.commit.as_deref(), Some("0123456789abcdef"));
        let item = store.list().into_iter().find(|i| i.slot == a.slot).unwrap();
        assert_eq!(item.meta.label, "local");
        assert_eq!(item.meta.commit, a.commit);
        // A changed tree and no git at all.
        let dirty = |_: &Path| {
            Some(GitInfo {
                commit: "0123456789abcdef".into(),
                dirty: true,
            })
        };
        let b = add(&store, &build, &v("0.1.0"), 7, &dirty).unwrap();
        assert_eq!(b.version.to_string(), "0.1.0-local.g0123456789.d7");
        assert_eq!(b.commit.as_deref(), Some("0123456789abcdef+dirty"));
        let c = add(&store, &build, &v("0.1.0"), 7, &|_| None).unwrap();
        assert_eq!(
            (c.version.to_string().as_str(), c.commit),
            ("0.1.0-local.t7", None)
        );
        // Not a build.
        assert!(add(&store, &root, &v("0.1.0"), 7, &clean).is_err());
        // A release folder with the same name is never overwritten by an unsigned build.
        let rel = v("0.1.0-local.g0123456789.d9");
        let archive = crate::store::tests::archive_file(&root, "0.1.0-local.g0123456789.d9");
        store.stage(&rel, &archive, "nightly").unwrap();
        assert!(add(&store, &build, &v("0.1.0"), 9, &dirty).is_err());
        assert_eq!(
            store.meta(&crate::store::Slot::Version(rel)).label,
            "nightly"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn git_info_of_a_real_checkout() {
        let d = tmp("gitinfo");
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&d)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.org"])
                .args(args)
                .output()
        };
        // No git installed: nothing to check here.
        let Ok(init) = git(&["init", "-q"]) else {
            return;
        };
        if !init.status.success() {
            return;
        }
        assert_eq!(git_info(&d), None, "no commit yet");
        std::fs::write(d.join("f"), b"1").unwrap();
        git(&["add", "f"]).unwrap();
        git(&["commit", "-q", "-m", "x"]).unwrap();
        let info = git_info(&d).expect("a commit");
        assert!(!info.dirty);
        std::fs::write(d.join("f"), b"2").unwrap();
        assert!(git_info(&d).unwrap().dirty);
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(git_info(&d), None);
    }
}
