//! The logic of `sign-manifest`, the CI tool for update manifests (PLAT-06).
//!
//! ```text
//! sign-manifest keygen --out KEYFILE
//!     Makes a new key. The private seed goes to KEYFILE (owner-only). Prints the public key.
//! sign-manifest public (--key-file FILE | --key-env VAR)
//!     Prints the public key of a private key.
//! sign-manifest build --version V --channel stable|beta --out FILE
//!                     --platform NAME=ARCHIVE=URL [--platform ...]
//!     Makes an unsigned manifest. Size and SHA-256 come from the archive files.
//! sign-manifest sign --manifest FILE (--key-file FILE | --key-env VAR) --out FILE
//!     Signs a manifest and writes the signed envelope.
//! sign-manifest verify --signed FILE --pubkey HEX [--pubkey HEX]
//!     Checks a signed envelope. CI runs this before it uploads.
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

fn build(flags: &Flags) -> Result<(), String> {
    let version = Version::parse(flags.need("version")?).map_err(|e| e.to_string())?;
    let channel_text = flags.need("channel")?;
    let channel = Channel::parse(channel_text)
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
    let manifest = Manifest {
        version,
        channel,
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
    writeln!(
        out,
        "ok: version {} on the {} channel, {} platform(s)",
        manifest.version,
        manifest.channel.as_str(),
        manifest.platforms.len()
    )
    .map_err(io)
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
}
