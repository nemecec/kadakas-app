//! Version numbers: `major.minor.patch`, with an optional pre-release part (`1.2.0-beta.3`).
//!
//! The characters are limited to letters, digits, `.` and `-`, so a version is always safe as a
//! folder name.

use crate::error::{Result, UpdateError};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

const MAX_LEN: usize = 40;

impl Version {
    pub fn parse(text: &str) -> Result<Version> {
        let bad = |why: &str| UpdateError::BadVersion(format!("{text:?}: {why}"));
        if text.is_empty() || text.len() > MAX_LEN {
            return Err(bad("empty or too long"));
        }
        let (core, pre) = match text.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (text, None),
        };
        let nums: Vec<&str> = core.split('.').collect();
        if nums.len() != 3 {
            return Err(bad("need major.minor.patch"));
        }
        let mut n = [0u64; 3];
        for (i, part) in nums.iter().enumerate() {
            if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad("numbers must be 1 to 9 digits"));
            }
            if part.len() > 1 && part.starts_with('0') {
                return Err(bad("no leading zeros"));
            }
            n[i] = part.parse().map_err(|_| bad("not a number"))?;
        }
        if let Some(p) = pre
            && (!p
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
                || p.split('.').any(str::is_empty))
        {
            // Lower case only: two versions that differ only in case would share a folder
            // on a case-insensitive file system.
            return Err(bad(
                "bad pre-release part (lower-case letters, digits, . and -)",
            ));
        }
        if let Some(p) = pre
            && p.split('.').any(|id| id.split('-').any(str::is_empty))
        {
            // The same rule as the version pattern of the scripts: no empty part around a hyphen.
            return Err(bad("bad pre-release part (an empty part around a hyphen)"));
        }
        if let Some(p) = pre
            && p.split('.')
                .any(|id| id.len() > 9 && id.bytes().all(|b| b.is_ascii_digit()))
        {
            // Same cap as for the numbers of the version: the order never depends on an overflow.
            return Err(bad(
                "a number of the pre-release part has more than 9 digits",
            ));
        }
        if let Some(p) = pre
            && p.split('.').any(|id| {
                id.len() > 1 && id.starts_with('0') && id.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return Err(bad("no leading zeros in numbers of the pre-release part"));
        }
        Ok(Version {
            major: n[0],
            minor: n[1],
            patch: n[2],
            pre: pre.map(str::to_string),
        })
    }

    /// A development version (`0.1.1-dev`, or `0.1.1-dev.3`) is what `main` carries between two
    /// releases. It is never tagged and never in a manifest.
    pub fn is_dev(&self) -> bool {
        self.pre
            .as_deref()
            .is_some_and(|p| p.split('.').next() == Some("dev"))
    }
}

fn cmp_pre(a: &str, b: &str) -> Ordering {
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let xn = x.parse::<u64>().ok();
                let yn = y.parse::<u64>().ok();
                let o = match (xn, yn) {
                    (Some(p), Some(q)) => p.cmp(&q),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => x.cmp(y),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                // A release is newer than its pre-releases.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => cmp_pre(a, b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(p) = &self.pre {
            write!(f, "-{p}")?;
        }
        Ok(())
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let t = String::deserialize(d)?;
        Version::parse(&t).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_and_prints() {
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        assert_eq!(v("0.10.0-beta.2").to_string(), "0.10.0-beta.2");
    }

    #[test]
    fn rejects_bad_text() {
        for t in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "1.2.x",
            "1..3",
            "../x",
            "1.2.3-",
            "1.2.3-a..b",
            "1.2.3-a/b",
            "1.2.3-é",
            "1.2.3-Beta",
            "01.2.3",
            "1.02.3",
            "1.2.3-a.01",
            "1.2.3-00",
            "0.0.0--x",
            "1.2.3-a-",
            "1.2.3-a.-b",
            "1.2.3-a--b",
            "1.2.3-1234567890",
            "1.2.3-a.99999999999999999999999",
            "1234567890.0.0",
        ] {
            assert!(Version::parse(t).is_err(), "{t}");
        }
        assert!(Version::parse(&format!("1.2.3-{}", "a".repeat(40))).is_err());
        // Inner hyphens are fine, as in the version pattern of the scripts.
        assert!(Version::parse("1.2.3-a-b.c-1").is_ok());
    }

    #[test]
    fn order() {
        assert!(v("1.2.3") < v("1.2.4"));
        assert!(v("1.10.0") > v("1.9.9"));
        assert!(v("2.0.0") > v("1.99.99"));
        assert!(v("1.0.0-beta.1") < v("1.0.0"));
        assert!(v("1.0.0") > v("1.0.0-beta.1"));
        assert!(v("1.0.0-beta.2") < v("1.0.0-beta.10"));
        assert!(v("1.0.0-alpha") < v("1.0.0-beta"));
        assert!(v("1.0.0-1") < v("1.0.0-a"));
        assert!(v("1.0.0-a") > v("1.0.0-1"));
        assert!(v("1.0.0-a") < v("1.0.0-a.1"));
        assert!(v("1.0.0-a.1") > v("1.0.0-a"));
        assert_eq!(v("1.0.0").cmp(&v("1.0.0")), Ordering::Equal);
        assert_eq!(v("1.0.0").partial_cmp(&v("1.0.1")), Some(Ordering::Less));
    }

    #[test]
    fn rehearsal_and_development_versions() {
        // The first rehearsal release, the next beta, the development version after it, and the release.
        assert!(v("0.1.0-beta.1") < v("0.1.0-beta.2"));
        assert!(v("0.1.0-beta.2") < v("0.1.0-dev"));
        assert!(v("0.1.0-dev") < v("0.1.0"));
        assert!(v("0.1.0") < v("0.1.1-dev"));
        assert!(v("0.1.1-dev") < v("0.1.1"));
        // After a release candidate, "dev" sorts below "rc" (letters compare as text).
        assert!(v("0.1.0-dev") < v("0.1.0-rc.1"));
        assert_eq!(v("0.1.0-beta.1").to_string(), "0.1.0-beta.1");
        assert!(v("0.1.1-dev").is_dev() && v("0.1.1-dev.2").is_dev());
        // Only the first identifier decides, like the scripts.
        assert!(!v("0.1.0-beta.1-dev").is_dev() && !v("0.1.0-dev-x").is_dev());
        assert!(!v("0.1.0-beta.1").is_dev() && !v("0.1.0").is_dev() && !v("0.1.0-devel").is_dev());
    }

    #[test]
    fn json_form_is_text() {
        let s = serde_json::to_string(&v("1.2.3-rc.1")).unwrap();
        assert_eq!(s, "\"1.2.3-rc.1\"");
        assert_eq!(
            serde_json::from_str::<Version>(&s).unwrap(),
            v("1.2.3-rc.1")
        );
        assert!(serde_json::from_str::<Version>("\"../..\"").is_err());
    }

    proptest! {
        #[test]
        fn text_round_trip(a in 0u64..1000, b in 0u64..1000, c in 0u64..1000, pre in proptest::option::of("[a-z][a-z0-9]{0,7}(\\.[a-z][a-z0-9]{0,3}){0,2}")) {
            let text = match &pre { Some(p) => format!("{a}.{b}.{c}-{p}"), None => format!("{a}.{b}.{c}") };
            let ver = Version::parse(&text).unwrap();
            prop_assert_eq!(ver.to_string(), text);
        }

        #[test]
        fn eq_and_ord_agree(a in "[0-2]\\.[0-2]\\.[0-2](-[a-c0-3]{1,2}(\\.[a-c0-3]{1,2})?)?", b in "[0-2]\\.[0-2]\\.[0-2](-[a-c0-3]{1,2}(\\.[a-c0-3]{1,2})?)?") {
            if let (Ok(x), Ok(y)) = (Version::parse(&a), Version::parse(&b)) {
                prop_assert_eq!(x == y, x.cmp(&y) == Ordering::Equal);
                prop_assert_eq!(x.cmp(&y), y.cmp(&x).reverse());
            }
        }

        #[test]
        fn never_a_path_trick(text in ".{0,50}") {
            if let Ok(ver) = Version::parse(&text) {
                let s = ver.to_string();
                prop_assert!(!s.contains('/') && !s.contains('\\') && !s.contains(".."), "{}", s);
            }
        }
    }
}
