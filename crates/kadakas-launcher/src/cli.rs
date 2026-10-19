//! The command line of the launcher.
//!
//! ```text
//! kadakas-launcher [--site URL] [FILE...]    start Kadakas (the default; shortcuts and .kad files)
//! kadakas-launcher manage                    open the window with the versions
//! kadakas-launcher add PATH [--version X.Y.Z]   put a local build in the store (PLAT-17)
//! ```

use kadakas_update::Version;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// Start the app with these documents.
    Start {
        files: Vec<OsString>,
        /// A local update site for this run (`--site file:///...`).
        site: Option<String>,
    },
    /// Open the window.
    Manage,
    /// Put a build in the store.
    Add {
        path: PathBuf,
        /// The base version number of the build, when it is not the version of the launcher.
        version: Option<Version>,
    },
    Help,
}

pub const USAGE: &str = "Usage:
  kadakas-launcher [--site file:///FOLDER] [FILE...]   start Kadakas
  kadakas-launcher manage                              show the versions
  kadakas-launcher add PATH [--version X.Y.Z]          add a local build (a folder with kadakas-app)";

/// Reads the arguments (without the program name).
pub fn parse(args: &[OsString]) -> Result<Command, String> {
    let text = |a: &OsString| a.to_string_lossy().into_owned();
    match args.first().map(text).as_deref() {
        Some("manage" | "window") if args.len() == 1 => return Ok(Command::Manage),
        Some("manage" | "window") => return Err("manage takes no arguments".into()),
        Some("add") => return parse_add(&args[1..]),
        Some("-h" | "--help" | "help") => return Ok(Command::Help),
        _ => {}
    }
    let mut files = Vec::new();
    let mut site = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = text(a);
        if t == "--" {
            files.extend(it.by_ref().cloned());
        } else if t == "--site" {
            site = Some(text(it.next().ok_or("--site needs an address")?));
        } else if let Some(v) = t.strip_prefix("--site=") {
            site = Some(v.to_string());
        } else if t.starts_with("-psn_") {
            // The process serial number that macOS adds when Finder opens a program.
        } else if t.starts_with('-') && t.len() > 1 {
            return Err(format!("unknown option {t}"));
        } else {
            files.push(a.clone());
        }
    }
    if let Some(s) = &site
        && !kadakas_update::settings::valid_site(s)
    {
        return Err("--site needs a file:// address".into());
    }
    Ok(Command::Start { files, site })
}

fn parse_add(args: &[OsString]) -> Result<Command, String> {
    let mut path = None;
    let mut version = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.to_string_lossy().into_owned();
        if t == "--version" {
            let v = it.next().ok_or("--version needs a number")?;
            version = Some(Version::parse(&v.to_string_lossy()).map_err(|e| e.to_string())?);
        } else if let Some(v) = t.strip_prefix("--version=") {
            version = Some(Version::parse(v).map_err(|e| e.to_string())?);
        } else if t.starts_with("--") {
            return Err(format!("unknown option {t}"));
        } else if path.replace(PathBuf::from(a)).is_some() {
            return Err("add takes one path".into());
        }
    }
    Ok(Command::Add {
        path: path.ok_or("add needs the path of a build")?,
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn files_are_forwarded() {
        assert_eq!(
            p(&[]).unwrap(),
            Command::Start {
                files: vec![],
                site: None
            }
        );
        assert_eq!(
            p(&["/a/one.kad", "two.kad"]).unwrap(),
            Command::Start {
                files: vec!["/a/one.kad".into(), "two.kad".into()],
                site: None
            }
        );
        // After `--` everything is a file, also a name that starts with a dash.
        assert_eq!(
            p(&["--", "-odd.kad", "--site"]).unwrap(),
            Command::Start {
                files: vec!["-odd.kad".into(), "--site".into()],
                site: None
            }
        );
        // The serial number of macOS is not a file.
        assert_eq!(
            p(&["-psn_0_12345", "a.kad"]).unwrap(),
            Command::Start {
                files: vec!["a.kad".into()],
                site: None
            }
        );
        assert!(p(&["--nope"]).is_err());
    }

    #[test]
    fn a_site_must_be_a_local_folder() {
        for args in [
            &["--site", "file:///s/site"][..],
            &["a.kad", "--site=file:///s/site"][..],
        ] {
            match p(args).unwrap() {
                Command::Start { site, .. } => assert_eq!(site.as_deref(), Some("file:///s/site")),
                other => panic!("{other:?}"),
            }
        }
        assert!(p(&["--site", "https://evil.example/m"]).is_err());
        assert!(p(&["--site"]).is_err());
    }

    #[test]
    fn manage_and_help() {
        assert_eq!(p(&["manage"]).unwrap(), Command::Manage);
        assert_eq!(p(&["window"]).unwrap(), Command::Manage);
        assert!(p(&["manage", "x"]).is_err());
        for h in ["-h", "--help", "help"] {
            assert_eq!(p(&[h]).unwrap(), Command::Help);
        }
        assert!(USAGE.contains("add PATH"));
    }

    #[test]
    fn add_needs_one_path() {
        assert_eq!(
            p(&["add", "target/release"]).unwrap(),
            Command::Add {
                path: "target/release".into(),
                version: None
            }
        );
        assert_eq!(
            p(&["add", "b", "--version", "0.3.0"]).unwrap(),
            Command::Add {
                path: "b".into(),
                version: Some(Version::parse("0.3.0").unwrap())
            }
        );
        assert!(matches!(
            p(&["add", "--version=1.0.0", "b"]).unwrap(),
            Command::Add {
                version: Some(_),
                ..
            }
        ));
        for bad in [
            &["add"][..],
            &["add", "a", "b"][..],
            &["add", "a", "--version"][..],
            &["add", "a", "--version", "x"][..],
            &["add", "a", "--version=x"][..],
            &["add", "a", "--other"][..],
        ] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }
}
