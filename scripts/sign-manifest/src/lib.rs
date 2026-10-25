//! The logic of `sign-manifest`, the CI tool for update manifests (PLAT-06).
//!
//! ```text
//! sign-manifest keygen --out KEYFILE
//!     Makes a new key. The private seed goes to KEYFILE (owner-only). Prints the public key.
//! sign-manifest public (--key-file FILE | --key-env VAR)
//!     Prints the public key of a private key.
//! sign-manifest build --version V --channel stable|beta --out FILE
//!                     --platform NAME=ARCHIVE=URL [--platform ...]
//!                     [--valid-days N] [--now UNIX_SECONDS]
//!     Makes an unsigned manifest. Size and SHA-256 come from the archive files. It is valid
//!     for N days from now (default 30). CI signs again before that time is over.
//! sign-manifest sign --manifest FILE (--key-file FILE | --key-env VAR) --out FILE
//!     Signs a manifest and writes the signed envelope.
//! sign-manifest verify --signed FILE --pubkey HEX [--pubkey HEX] [--now UNIX_SECONDS]
//!     Checks a signed envelope: signature and expiry. CI runs this before it uploads.
//! sign-manifest newer --existing FILE --manifest FILE --pubkey HEX [--pubkey HEX]
//!                     [--allow-equal yes]
//!     CI guard against a backport: checks the signature of the manifest that is published now
//!     and fails unless the new manifest has a higher version (or an equal one, with
//!     --allow-equal, for a re-sign). It does not look at the expiry of the old file.
//! ```
//!
//! The private key is a 32-byte seed in hex. CI keeps it in a secret and passes it in an
//! environment variable, so it never touches the disk there. The tool prints no private key
//! except in `keygen`, which writes it to the key file only.

use kadakas_update::manifest::{Manifest, PlatformFile, SignedManifest};
use kadakas_update::sign;
use kadakas_update::{Channel, TrustedKeys, Version, sha256_hex};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Runs one command. `env` reads environment variables. Messages go to `out`.
pub fn run(args: &[String], env: Env, out: &mut dyn Write) -> Result<(), String> {
    let Some((cmd, rest)) = args.split_first() else {
        return Err(USAGE.into());
    };
    let flags = Flags::parse(rest)?;
    match cmd.as_str() {
        "keygen" => keygen(&flags, out),
        "public" => {
            let key = load_key(&flags, env)?;
            writeln!(out, "{}", sign::public_hex(&key)).map_err(io)
        }
        "build" => build(&flags),
        "sign" => sign_file(&flags, env),
        "verify" => verify(&flags, out),
        "newer" => newer(&flags, out),
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    }
}

const USAGE: &str =
    "usage: sign-manifest keygen|public|build|sign|verify [options] (see docs/release.md)";

fn io(e: std::io::Error) -> String {
    e.to_string()
}

/// `--name value` pairs. A name can repeat.
struct Flags(Vec<(String, String)>);

impl Flags {
    fn parse(args: &[String]) -> Result<Flags, String> {
        let mut v = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let name = a
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected argument {a:?}"))?;
            let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
            v.push((name.to_string(), value.clone()));
        }
        Ok(Flags(v))
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.0
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    fn one(&self, name: &str) -> Result<Option<&str>, String> {
        match self.all(name).as_slice() {
            [] => Ok(None),
            [v] => Ok(Some(v)),
            _ => Err(format!("--{name} given twice")),
        }
    }

    fn need(&self, name: &str) -> Result<&str, String> {
        self.one(name)?
            .ok_or_else(|| format!("--{name} is required"))
    }
}

fn load_key(flags: &Flags, env: Env) -> Result<sign::SigningKey, String> {
    let text = match (flags.one("key-file")?, flags.one("key-env")?) {
        (Some(f), None) => std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?,
        (None, Some(v)) => env(v).ok_or_else(|| format!("the variable {v} is not set"))?,
        _ => return Err("give exactly one of --key-file and --key-env".into()),
    };
    sign::key_from_hex(&text).map_err(|e| e.to_string())
}

fn keygen(flags: &Flags, out: &mut dyn Write) -> Result<(), String> {
    let path = flags.need("out")?;
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
    let key = sign::key_from_seed(seed);
    // The file must not exist yet: a key is never overwritten by mistake.
    let mut f = kadakas_sys::fsio::create_new_private(Path::new(path))
        .map_err(|e| format!("{path}: {e}"))?;
    let hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    writeln!(f, "{hex}").map_err(io)?;
    writeln!(out, "{}", sign::public_hex(&key)).map_err(io)
}

/// Seconds since 1970: `--now` or the clock.
fn now(flags: &Flags) -> Result<u64, String> {
    match flags.one("now")? {
        Some(t) => t
            .parse()
            .map_err(|_| format!("--now needs a number, got {t:?}")),
        None => Ok(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())),
    }
}

fn build(flags: &Flags) -> Result<(), String> {
    let version = Version::parse(flags.need("version")?).map_err(|e| e.to_string())?;
    let channel_text = flags.need("channel")?;
    let channel = Channel::parse_release(channel_text)
        .ok_or_else(|| format!("channel must be stable or beta, not {channel_text:?}"))?;
    let mut platforms = BTreeMap::new();
    for spec in flags.all("platform") {
        let parts: Vec<&str> = spec.splitn(3, '=').collect();
        let [name, archive, url] = parts[..] else {
            return Err(format!("--platform needs NAME=ARCHIVE=URL, got {spec:?}"));
        };
        let file = std::fs::File::open(archive).map_err(|e| format!("{archive}: {e}"))?;
        let size = file.metadata().map_err(io)?.len();
        let sha256 = sha256_hex(file).map_err(io)?;
        platforms.insert(
            name.to_string(),
            PlatformFile {
                url: url.to_string(),
                size,
                sha256,
            },
        );
    }
    let days: u64 = match flags.one("valid-days")? {
        Some(d) => d
            .parse()
            .ok()
            .filter(|d| (1..=365).contains(d))
            .ok_or_else(|| format!("--valid-days must be 1 to 365, got {d:?}"))?,
        None => 30,
    };
    let published = now(flags)?;
    let manifest = Manifest {
        version,
        channel,
        published,
        expires: published + days * 86_400,
        platforms,
    };
    manifest.validate().map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(flags.need("out")?, text + "\n").map_err(io)
}

fn sign_file(flags: &Flags, env: Env) -> Result<(), String> {
    let path = flags.need("manifest")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    manifest.validate().map_err(|e| e.to_string())?;
    let key = load_key(flags, env)?;
    let signed = sign::sign_manifest(&manifest, &key);
    std::fs::write(flags.need("out")?, signed.to_json() + "\n").map_err(io)
}

fn verify(flags: &Flags, out: &mut dyn Write) -> Result<(), String> {
    let path = flags.need("signed")?;
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let keys = TrustedKeys::new(&flags.all("pubkey")).map_err(|e| e.to_string())?;
    let manifest = SignedManifest::parse(&bytes)
        .and_then(|s| s.verify(&keys))
        .map_err(|e| e.to_string())?;
    manifest
        .check_fresh(now(flags)?)
        .map_err(|e| e.to_string())?;
    writeln!(
        out,
        "ok: version {} on the {} channel, {} platform(s), valid until {}",
        manifest.version,
        manifest.channel.as_str(),
        manifest.platforms.len(),
        manifest.expires
    )
    .map_err(io)
}

fn newer(flags: &Flags, out: &mut dyn Write) -> Result<(), String> {
    let existing_path = flags.need("existing")?;
    let bytes = std::fs::read(existing_path).map_err(|e| format!("{existing_path}: {e}"))?;
    let keys = TrustedKeys::new(&flags.all("pubkey")).map_err(|e| e.to_string())?;
    let old = SignedManifest::parse(&bytes)
        .and_then(|s| s.verify(&keys))
        .map_err(|e| format!("the published manifest: {e}"))?;
    let path = flags.need("manifest")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let new: Manifest = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let equal_ok = flags.one("allow-equal")?.is_some();
    let good = new.version > old.version || (equal_ok && new.version == old.version);
    if !good {
        return Err(format!(
            "version {} is not newer than the published {} (a backport?)",
            new.version, old.version
        ));
    }
    writeln!(out, "ok: {} follows {}", new.version, old.version).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("kadakas-signtool-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn go(args: &[&str], env: &[(&str, &str)]) -> Result<String, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let lookup = move |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let mut out = Vec::new();
        run(&args, &lookup, &mut out)?;
        Ok(String::from_utf8(out).unwrap())
    }

    fn s(p: &Path) -> &str {
        p.to_str().unwrap()
    }

    #[test]
    fn full_ci_flow_with_a_key_file() {
        let d = tmp("flow");
        let key = d.join("key.txt");
        let public = go(&["keygen", "--out", s(&key)], &[])
            .unwrap()
            .trim()
            .to_string();
        assert_eq!(public.len(), 64);
        // The key file is private and is never overwritten.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(go(&["keygen", "--out", s(&key)], &[]).is_err());
        assert_eq!(
            go(&["public", "--key-file", s(&key)], &[]).unwrap().trim(),
            public
        );

        let archive = d.join("a.tar.gz");
        std::fs::write(&archive, b"archive bytes").unwrap();
        let manifest = d.join("m.json");
        go(
            &[
                "build",
                "--version",
                "1.2.0",
                "--channel",
                "beta",
                "--platform",
                &format!(
                    "linux-x86_64={}=https://example.org/a.tar.gz?x=1",
                    s(&archive)
                ),
                "--out",
                s(&manifest),
            ],
            &[],
        )
        .unwrap();
        let text = std::fs::read_to_string(&manifest).unwrap();
        assert!(text.contains("\"size\": 13"), "{text}");
        assert!(text.contains("x=1"), "{text}");

        let signed = d.join("signed.json");
        go(
            &[
                "sign",
                "--manifest",
                s(&manifest),
                "--key-file",
                s(&key),
                "--out",
                s(&signed),
            ],
            &[],
        )
        .unwrap();
        let ok = go(
            &["verify", "--signed", s(&signed), "--pubkey", &public],
            &[],
        )
        .unwrap();
        assert!(ok.contains("version 1.2.0 on the beta channel"), "{ok}");

        // A different key does not verify. A damaged file does not verify.
        let other = go(&["keygen", "--out", s(&d.join("k2.txt"))], &[]).unwrap();
        assert!(
            go(
                &["verify", "--signed", s(&signed), "--pubkey", other.trim()],
                &[]
            )
            .is_err()
        );
        // Two keys (rotation): the old one is still accepted next to a new one.
        assert!(
            go(
                &[
                    "verify",
                    "--signed",
                    s(&signed),
                    "--pubkey",
                    other.trim(),
                    "--pubkey",
                    &public
                ],
                &[]
            )
            .is_ok()
        );
        std::fs::write(&signed, b"{}").unwrap();
        assert!(
            go(
                &["verify", "--signed", s(&signed), "--pubkey", &public],
                &[]
            )
            .is_err()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn key_from_the_environment() {
        let d = tmp("env");
        let seed = "0b".repeat(32);
        let public = go(
            &["public", "--key-env", "SIGNING_KEY"],
            &[("SIGNING_KEY", &seed)],
        )
        .unwrap();
        let manifest = d.join("m.json");
        let archive = d.join("a.tar.gz");
        std::fs::write(&archive, b"x").unwrap();
        go(
            &[
                "build",
                "--version",
                "1.0.0",
                "--channel",
                "stable",
                "--platform",
                &format!("macos-aarch64={}=https://example.org/a", s(&archive)),
                "--out",
                s(&manifest),
            ],
            &[],
        )
        .unwrap();
        let signed = d.join("s.json");
        go(
            &[
                "sign",
                "--manifest",
                s(&manifest),
                "--key-env",
                "K",
                "--out",
                s(&signed),
            ],
            &[("K", &seed)],
        )
        .unwrap();
        assert!(
            go(
                &["verify", "--signed", s(&signed), "--pubkey", public.trim()],
                &[]
            )
            .is_ok()
        );
        // The variable is missing.
        assert!(
            go(
                &[
                    "sign",
                    "--manifest",
                    s(&manifest),
                    "--key-env",
                    "K",
                    "--out",
                    s(&signed)
                ],
                &[]
            )
            .is_err()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn bad_input_gives_clear_errors() {
        let d = tmp("bad");
        assert!(go(&[], &[]).unwrap_err().contains("usage"));
        assert!(go(&["nope"], &[]).unwrap_err().contains("unknown command"));
        assert!(
            go(&["keygen", "x"], &[])
                .unwrap_err()
                .contains("unexpected")
        );
        assert!(
            go(&["keygen", "--out"], &[])
                .unwrap_err()
                .contains("needs a value")
        );
        assert!(go(&["keygen"], &[]).unwrap_err().contains("required"));
        assert!(
            go(&["keygen", "--out", "a", "--out", "b"], &[])
                .unwrap_err()
                .contains("twice")
        );
        assert!(go(&["public"], &[]).is_err());
        assert!(go(&["public", "--key-file", "a", "--key-env", "B"], &[]).is_err());
        assert!(go(&["public", "--key-file", s(&d.join("missing"))], &[]).is_err());
        assert!(go(&["public", "--key-env", "B"], &[("B", "zz")]).is_err());
        let a = d.join("a");
        std::fs::write(&a, b"1").unwrap();
        let base = |extra: &[&str]| {
            let out_path = d.join("m.json");
            let mut v = vec!["build", "--out", s(&out_path)];
            v.extend_from_slice(extra);
            go(&v, &[])
        };
        assert!(base(&["--version", "x", "--channel", "stable"]).is_err());
        // A development version is never released.
        let dev = base(&[
            "--version",
            "0.1.1-dev",
            "--channel",
            "beta",
            "--platform",
            &format!("linux-x86_64={}=https://example.org/a", s(&a)),
        ]);
        assert!(dev.unwrap_err().contains("development"));
        assert!(base(&["--version", "1.0.0", "--channel", "nightly"]).is_err());
        assert!(base(&["--version", "1.0.0", "--channel", "stable"]).is_err()); // no platform
        assert!(
            base(&[
                "--version",
                "1.0.0",
                "--channel",
                "stable",
                "--platform",
                "x"
            ])
            .is_err()
        );
        let missing = format!("p={}=https://e.org/a", s(&d.join("none")));
        assert!(
            base(&[
                "--version",
                "1.0.0",
                "--channel",
                "stable",
                "--platform",
                &missing
            ])
            .is_err()
        );
        let http = format!("p={}=http://e.org/a", s(&a));
        assert!(
            base(&[
                "--version",
                "1.0.0",
                "--channel",
                "stable",
                "--platform",
                &http
            ])
            .is_err()
        );
        // sign: no manifest, bad json, invalid content.
        let key = "0c".repeat(32);
        let sign = |m: &Path| {
            go(
                &[
                    "sign",
                    "--manifest",
                    s(m),
                    "--key-env",
                    "K",
                    "--out",
                    s(&d.join("o")),
                ],
                &[("K", &key)],
            )
        };
        assert!(sign(&d.join("none")).is_err());
        std::fs::write(d.join("bad.json"), b"nope").unwrap();
        assert!(sign(&d.join("bad.json")).is_err());
        std::fs::write(
            d.join("empty.json"),
            br#"{"version":"1.0.0","channel":"stable","platforms":{}}"#,
        )
        .unwrap();
        assert!(sign(&d.join("empty.json")).is_err());
        // verify: missing file and bad key text.
        assert!(
            go(
                &["verify", "--signed", s(&d.join("none")), "--pubkey", "00"],
                &[]
            )
            .is_err()
        );
        assert!(go(&["verify", "--signed", s(&a), "--pubkey", "00"], &[]).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Builds and signs a manifest file. Returns the path of the signed file.
    fn make(
        d: &Path,
        name: &str,
        version: &str,
        now: &str,
        days: &str,
        key: &str,
    ) -> std::path::PathBuf {
        let archive = d.join("a.tar.gz");
        std::fs::write(&archive, b"x").unwrap();
        let unsigned = d.join(format!("{name}-unsigned.json"));
        go(
            &[
                "build",
                "--version",
                version,
                "--channel",
                "beta",
                "--now",
                now,
                "--valid-days",
                days,
                "--platform",
                &format!("linux-x86_64={}=https://example.org/a", s(&archive)),
                "--out",
                s(&unsigned),
            ],
            &[],
        )
        .unwrap();
        let signed = d.join(format!("{name}.json"));
        go(
            &[
                "sign",
                "--manifest",
                s(&unsigned),
                "--key-env",
                "K",
                "--out",
                s(&signed),
            ],
            &[("K", key)],
        )
        .unwrap();
        signed
    }

    #[test]
    fn validity_time_and_expiry() {
        let d = tmp("expiry");
        let key = "0d".repeat(32);
        let public = go(&["public", "--key-env", "K"], &[("K", &key)]).unwrap();
        let public = public.trim();
        let f = make(&d, "m", "1.0.0", "1000000", "2", &key);
        let text = std::fs::read_to_string(&f).unwrap();
        // published and expires are inside the signed payload.
        assert!(text.contains("\\\"published\\\":1000000"), "{text}");
        assert!(
            text.contains(&format!("\\\"expires\\\":{}", 1_000_000 + 2 * 86_400)),
            "{text}"
        );
        let verify = |now: &str| {
            go(
                &[
                    "verify",
                    "--signed",
                    s(&f),
                    "--pubkey",
                    public,
                    "--now",
                    now,
                ],
                &[],
            )
        };
        assert!(verify("1000000").unwrap().contains("valid until 1172800"));
        assert!(verify(&(1_000_000 + 2 * 86_400).to_string()).is_ok());
        assert!(
            verify(&(1_000_000 + 2 * 86_400 + 1).to_string())
                .unwrap_err()
                .contains("expired")
        );
        assert!(verify("x").is_err());
        let archive = d.join("a.tar.gz");
        for days in ["0", "366", "x"] {
            let out = d.join("o.json");
            let plat = format!("linux-x86_64={}=https://example.org/a", s(&archive));
            let args = [
                "build",
                "--version",
                "1.0.0",
                "--channel",
                "beta",
                "--valid-days",
                days,
                "--platform",
                plat.as_str(),
                "--out",
                s(&out),
            ];
            assert!(go(&args, &[]).is_err(), "{days}");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_newer_guard_stops_a_backport() {
        let d = tmp("newer");
        let key = "0e".repeat(32);
        let public = go(&["public", "--key-env", "K"], &[("K", &key)]).unwrap();
        let public = public.trim();
        let published = make(&d, "pub", "2.0.0", "1000", "30", &key);
        let newer = make(&d, "new", "2.1.0", "2000", "30", &key);
        let same = make(&d, "same", "2.0.0", "2000", "30", &key);
        let older = make(&d, "old", "1.9.0", "2000", "30", &key);
        let unsigned = |n: &str| d.join(format!("{n}-unsigned.json"));
        let check = |new: &str, extra: &[&str]| {
            let new_path = unsigned(new);
            let mut a = vec![
                "newer",
                "--existing",
                s(&published),
                "--manifest",
                s(&new_path),
                "--pubkey",
                public,
            ];
            a.extend_from_slice(extra);
            go(&a, &[])
        };
        assert!(check("new", &[]).unwrap().contains("2.1.0 follows 2.0.0"));
        assert!(check("same", &[]).unwrap_err().contains("backport"));
        assert!(check("same", &["--allow-equal", "yes"]).is_ok());
        assert!(
            check("old", &["--allow-equal", "yes"])
                .unwrap_err()
                .contains("backport")
        );
        let _ = (newer, same, older);
        // The published file must have a good signature, and exist.
        let other = go(&["keygen", "--out", s(&d.join("k.txt"))], &[]).unwrap();
        let wrong_key = go(
            &[
                "newer",
                "--existing",
                s(&published),
                "--manifest",
                s(&unsigned("new")),
                "--pubkey",
                other.trim(),
            ],
            &[],
        );
        assert!(wrong_key.unwrap_err().contains("published manifest"));
        assert!(
            go(
                &[
                    "newer",
                    "--existing",
                    s(&d.join("none")),
                    "--manifest",
                    s(&unsigned("new")),
                    "--pubkey",
                    public
                ],
                &[]
            )
            .is_err()
        );
        assert!(
            go(
                &[
                    "newer",
                    "--existing",
                    s(&published),
                    "--manifest",
                    s(&d.join("none")),
                    "--pubkey",
                    public
                ],
                &[]
            )
            .is_err()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
