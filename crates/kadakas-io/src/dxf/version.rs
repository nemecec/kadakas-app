//! The release of a DXF file (`$ACADVER`) and what each release can hold (IOP-01).
//!
//! The release names come from the public DXF reference of Autodesk and from the ezdxf
//! documentation. The code is `AC` and a number. A bigger number is a newer release.

/// A release that the header names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The text of `$ACADVER`, for example `AC1015`.
    pub code: String,
    /// The name of the release, for example `2000`.
    pub name: &'static str,
    /// The number of the code (1015), or 0 for the very old codes such as `AC1.50`.
    number: u32,
}

/// (code, name of the release)
const KNOWN: &[(&str, &str)] = &[
    ("AC1.40", "1.4"),
    ("AC1.50", "2.0"),
    ("AC2.10", "2.10"),
    ("AC2.21", "2.21"),
    ("AC2.22", "2.22"),
    ("AC1002", "2.5"),
    ("AC1003", "2.6"),
    ("AC1004", "9"),
    ("AC1006", "10"),
    ("AC1009", "12"),
    ("AC1012", "13"),
    ("AC1014", "14"),
    ("AC1015", "2000"),
    ("AC1018", "2004"),
    ("AC1021", "2007"),
    ("AC1024", "2010"),
    ("AC1027", "2013"),
    ("AC1032", "2018"),
];

impl Release {
    /// The release of a `$ACADVER` value. `None` if it is not a code of the form `AC` and digits.
    pub fn parse(value: &str) -> Option<Release> {
        let code = value.trim().to_ascii_uppercase();
        if let Some((c, name)) = KNOWN.iter().find(|(c, _)| *c == code) {
            let number = c
                .strip_prefix("AC")
                .and_then(|d| d.parse().ok())
                .unwrap_or(0);
            return Some(Release { code, name, number });
        }
        let number: u32 = code.strip_prefix("AC")?.parse().ok()?;
        if !(1000..10_000).contains(&number) {
            return None;
        }
        Some(Release {
            code,
            name: if number > 1032 {
                "newer than 2018"
            } else {
                "unknown"
            },
            number,
        })
    }

    /// The code and the name, for the report: `AC1015 (2000)`.
    pub fn label(&self) -> String {
        format!("{} (release {})", self.code, self.name)
    }

    /// True from release 2000 on: only these files can hold `$INSUNITS`.
    pub fn has_units(&self) -> bool {
        self.number >= 1015
    }

    /// True from release 2007 on: the text of the file is UTF-8. Older text is in the code page of
    /// `$DWGCODEPAGE`.
    pub fn is_unicode(&self) -> bool {
        self.number >= 1021
    }

    /// True for release 12 and older. These files have no LWPOLYLINE, no layouts and no units.
    pub fn is_r12_or_older(&self) -> bool {
        self.number < 1012
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_codes() {
        for (code, name) in KNOWN {
            let r = Release::parse(code).unwrap();
            assert_eq!(&r.code, code);
            assert_eq!(r.name, *name);
        }
        let r = Release::parse(" ac1015 ").unwrap();
        assert_eq!(r.code, "AC1015");
        assert_eq!(r.label(), "AC1015 (release 2000)");
    }

    #[test]
    fn what_a_release_holds() {
        let r = |c| Release::parse(c).unwrap();
        assert!(!r("AC1009").has_units());
        assert!(!r("AC1014").has_units());
        assert!(r("AC1015").has_units());
        assert!(!r("AC1018").is_unicode());
        assert!(r("AC1021").is_unicode());
        assert!(r("AC1032").is_unicode());
        assert!(r("AC1006").is_r12_or_older());
        assert!(r("AC1009").is_r12_or_older());
        assert!(!r("AC1012").is_r12_or_older());
        assert!(r("AC1.50").is_r12_or_older());
    }

    #[test]
    fn odd_codes() {
        assert_eq!(Release::parse("AC1040").unwrap().name, "newer than 2018");
        assert_eq!(Release::parse("AC1017").unwrap().name, "unknown");
        assert!(Release::parse("hello").is_none());
        assert!(Release::parse("AC").is_none());
        assert!(Release::parse("AC12").is_none());
        assert!(Release::parse("").is_none());
    }
}
