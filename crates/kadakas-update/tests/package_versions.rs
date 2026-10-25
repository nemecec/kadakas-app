//! The package forms of a version (scripts/packager-version.sh) must be accepted by the real
//! tools. cargo-packager 0.11.8 parses the version of the WiX package with the semver crate
//! (`convert_version`, src/package/wix/mod.rs) and turns a numeric pre-release into the fourth
//! number. This test runs the same steps, so a form that the packager would refuse fails here
//! and not in the release build on Windows. Needs bash, so it runs on Unix only.
#![cfg(unix)]

use kadakas_update::Version;
use std::process::Command;

fn script(format: &str, version: &str) -> Option<String> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let out = Command::new("bash")
        .arg(format!("{root}/scripts/packager-version.sh"))
        .args([format, version])
        .output()
        .expect("bash");
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).unwrap().trim().to_string())
}

/// The logic of `convert_version` in cargo-packager 0.11.8 (wix/mod.rs), without the limits on
/// major and minor, which our versions meet.
fn packager_convert_version(version_str: &str) -> Result<String, String> {
    let version = semver::Version::parse(version_str).map_err(|e| e.to_string())?;
    if version.patch > 65535 {
        return Err("patch".into());
    }
    if !version.build.is_empty() {
        return Err("build metadata is not used".into());
    }
    if !version.pre.is_empty() {
        return match version.pre.parse::<u64>() {
            Ok(pre) if pre <= 65535 => Ok(format!(
                "{}.{}.{}.{}",
                version.major, version.minor, version.patch, version.pre
            )),
            _ => Err("non-numeric pre-release".into()),
        };
    }
    Ok(version_str.to_string())
}

const VERSIONS: [&str; 9] = [
    "0.1.0-alpha.1",
    "0.1.0-beta.1",
    "0.1.0-beta.2",
    "0.1.0-dev",
    "0.1.0-rc.1",
    "0.1.0",
    "0.1.1-dev",
    "0.1.1",
    "12.34.56-beta.9999",
];

#[test]
fn the_wix_form_goes_through_the_packager_conversion() {
    for v in VERSIONS {
        let wix = script("wix", v).unwrap_or_else(|| panic!("no wix form of {v}"));
        let msi = packager_convert_version(&wix).unwrap_or_else(|e| panic!("{v} -> {wix}: {e}"));
        let parts: Vec<u64> = msi.split('.').map(|p| p.parse().unwrap()).collect();
        assert!(parts.len() == 3 || parts.len() == 4, "{msi}");
        assert!(parts.iter().skip(2).all(|&n| n <= 65535), "{msi}");
        // The real Version type reads the same text of the release as the script does.
        Version::parse(v).unwrap();
    }
    assert_eq!(script("wix", "0.1.0-beta.1").unwrap(), "0.1.0-10001");
    assert_eq!(
        packager_convert_version("0.1.0-10001").unwrap(),
        "0.1.0.10001"
    );
    // The form that failed in the first version of the script is not a valid semver text.
    assert!(packager_convert_version("0.1.0.10001").is_err());
}

#[test]
fn the_wix_numbers_keep_the_order_of_the_prereleases_of_one_release() {
    let list = [
        "0.1.0-alpha.1",
        "0.1.0-beta.1",
        "0.1.0-beta.2",
        "0.1.0-dev",
        "0.1.0-rc.1",
    ];
    let numbers: Vec<u64> = list
        .iter()
        .map(|v| {
            let msi = packager_convert_version(&script("wix", v).unwrap()).unwrap();
            msi.rsplit('.').next().unwrap().parse().unwrap()
        })
        .collect();
    assert!(numbers.windows(2).all(|w| w[0] < w[1]), "{numbers:?}");
    let versions: Vec<Version> = list.iter().map(|v| Version::parse(v).unwrap()).collect();
    assert!(versions.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn a_label_that_cannot_be_mapped_is_refused() {
    for v in [
        "0.1.0-local.g1",
        "0.1.0-beta",
        "0.1.0-dev.2",
        "0.1.0-beta.01",
    ] {
        assert!(script("wix", v).is_none(), "{v}");
        assert!(script("pep440", v).is_none(), "{v}");
    }
}
