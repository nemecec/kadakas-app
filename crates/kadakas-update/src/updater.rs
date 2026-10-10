//! The update flow: fetch the signed manifest, verify it, download, check size and hash, install.
//!
//! Order of checks (nothing later runs when an earlier check fails):
//!
//! 1. The signature of the manifest, with the trusted keys.
//! 2. The channel in the manifest is the channel that was asked for.
//! 3. The version is newer than the active one (no downgrade).
//! 4. The download has exactly the announced size.
//! 5. The download has the announced SHA-256.
//! 6. The archive paths are safe (see `archive`) and the install is atomic (see `store`).

use crate::error::{Result, UpdateError};
use crate::hex;
use crate::http::Http;
use crate::keys::TrustedKeys;
use crate::manifest::{Channel, MAX_ENVELOPE_BYTES, Manifest, PlatformFile, SignedManifest};
use crate::store::{Slot, Store};
use crate::version::Version;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// An update that can be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub version: Version,
    pub channel: Channel,
    pub size: u64,
}

/// The result of a check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Nothing newer is offered.
    UpToDate { current: Option<Version> },
    /// A newer version can be installed.
    Available(Offer),
    /// The manifest offers an older version. It is refused.
    Older { current: Version, offered: Version },
}

/// The SHA-256 of a stream, as lower-case hex.
pub fn sha256_hex(mut reader: impl Read) -> std::io::Result<String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize().as_slice()))
}

/// The system name used as the key in the manifest, for example `macos-aarch64`.
pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

pub struct Updater {
    store: Store,
    http: Arc<dyn Http>,
    keys: TrustedKeys,
    manifest_base: String,
    platform: String,
}

impl Updater {
    /// `manifest_base` is the folder address of the manifests: the manifest of a channel is at
    /// `<manifest_base>/manifest-<channel>.json`.
    pub fn new(
        store: Store,
        http: Arc<dyn Http>,
        keys: TrustedKeys,
        manifest_base: impl Into<String>,
        platform: impl Into<String>,
    ) -> Updater {
        Updater {
            store,
            http,
            keys,
            manifest_base: manifest_base.into().trim_end_matches('/').to_string(),
            platform: platform.into(),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn manifest_url(&self, channel: Channel) -> String {
        format!("{}/manifest-{}.json", self.manifest_base, channel.as_str())
    }

    /// Downloads the manifest of a channel and checks it (steps 1 and 2).
    pub fn fetch_manifest(&self, channel: Channel) -> Result<Manifest> {
        let mut body = Vec::new();
        self.http
            .get(&self.manifest_url(channel), MAX_ENVELOPE_BYTES, &mut body)?;
        let manifest = SignedManifest::parse(&body)?.verify(&self.keys)?;
        if manifest.channel != channel {
            return Err(UpdateError::ChannelMismatch {
                expected: channel.as_str().into(),
                got: manifest.channel.as_str().into(),
            });
        }
        Ok(manifest)
    }

    fn judge(&self, manifest: &Manifest) -> Result<UpdateStatus> {
        let file = manifest.file_for(&self.platform)?;
        let current = self.store.active().map(|a| a.version);
        Ok(match current {
            Some(c) if c == manifest.version => UpdateStatus::UpToDate { current: Some(c) },
            Some(c) if c > manifest.version => UpdateStatus::Older {
                current: c,
                offered: manifest.version.clone(),
            },
            _ => UpdateStatus::Available(Offer {
                version: manifest.version.clone(),
                channel: manifest.channel,
                size: file.size,
            }),
        })
    }

    /// Asks the server whether a newer version exists.
    pub fn check(&self, channel: Channel) -> Result<UpdateStatus> {
        self.judge(&self.fetch_manifest(channel)?)
    }

    /// Checks, downloads, verifies and installs the newer version. Returns the new version.
    pub fn update(&self, channel: Channel) -> Result<Version> {
        let manifest = self.fetch_manifest(channel)?;
        match self.judge(&manifest)? {
            UpdateStatus::Available(_) => {}
            UpdateStatus::UpToDate { .. } => {
                return Err(UpdateError::AlreadyCurrent(manifest.version.to_string()));
            }
            UpdateStatus::Older { current, offered } => {
                return Err(UpdateError::Downgrade {
                    current: current.to_string(),
                    offered: offered.to_string(),
                });
            }
        }
        let file = manifest.file_for(&self.platform)?;
        let part = self.download(&manifest.version, file)?;
        let result = self.store.install(&manifest.version, &part, false);
        let _ = std::fs::remove_file(&part);
        result.map(|()| manifest.version)
    }

    /// Switches back to the previous version.
    pub fn rollback(&self) -> Result<Slot> {
        self.store.rollback()
    }

    /// Downloads the archive to a file and checks size and hash (steps 4 and 5).
    /// The file is deleted when a check fails.
    fn download(&self, version: &Version, file: &PlatformFile) -> Result<std::path::PathBuf> {
        std::fs::create_dir_all(self.store.downloads_dir())?;
        let part = self
            .store
            .downloads_dir()
            .join(format!("{version}.tar.gz.part"));
        let result = self.download_to(file, &part);
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result.map(|()| part)
    }

    fn download_to(&self, file: &PlatformFile, part: &Path) -> Result<()> {
        let mut out = std::fs::File::create(part)?;
        let got = self.http.get(&file.url, file.size, &mut out)?;
        out.sync_all()?;
        drop(out);
        if got != file.size {
            return Err(UpdateError::SizeMismatch {
                expected: file.size,
                got,
            });
        }
        // Count the bytes on disk, not the number that the client reported.
        let on_disk = std::fs::metadata(part)?.len();
        if on_disk != file.size {
            return Err(UpdateError::SizeMismatch {
                expected: file.size,
                got: on_disk,
            });
        }
        if sha256_hex(std::fs::File::open(part)?)? != file.sha256 {
            return Err(UpdateError::HashMismatch);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::archive::tests::{build, file};
    use crate::http::MockHttp;
    use crate::sign;
    use crate::store::tests::{bundled, tmp, v};
    use crate::store::{Slot, Step};
    use ed25519_dalek::SigningKey;
    use std::collections::BTreeMap;

    pub const BASE: &str = "https://updates.example.org/m";
    pub const PLATFORM: &str = "linux-x86_64";

    pub fn key(seed: u8) -> SigningKey {
        sign::key_from_seed([seed; 32])
    }

    pub fn trusted(seeds: &[u8]) -> TrustedKeys {
        let hexes: Vec<String> = seeds.iter().map(|s| sign::public_hex(&key(*s))).collect();
        let refs: Vec<&str> = hexes.iter().map(String::as_str).collect();
        TrustedKeys::new(&refs).unwrap()
    }

    pub fn archive_bytes(ver: &str) -> Vec<u8> {
        build(&[
            file("kadakas-app", format!("app {ver}").as_bytes()),
            file("kadakas", b"cli"),
        ])
    }

    /// Serves a signed manifest and its archive. Returns the archive address.
    pub fn publish(http: &MockHttp, ver: &str, channel: Channel, signer: &SigningKey) -> String {
        let bytes = archive_bytes(ver);
        let url = format!("https://downloads.example.org/kadakas-{ver}.tar.gz");
        http.put(&url, bytes.clone());
        let mut platforms = BTreeMap::new();
        platforms.insert(
            PLATFORM.to_string(),
            PlatformFile {
                url: url.clone(),
                size: bytes.len() as u64,
                sha256: sha256_hex(&bytes[..]).unwrap(),
            },
        );
        let m = Manifest {
            version: v(ver),
            channel,
            platforms,
        };
        let signed = sign::sign_manifest(&m, signer);
        http.put(
            &format!("{BASE}/manifest-{}.json", channel.as_str()),
            signed.to_json().into_bytes(),
        );
        url
    }

    pub fn updater(
        root: &Path,
        http: Arc<MockHttp>,
        seeds: &[u8],
        installed: Option<&str>,
    ) -> Updater {
        let store = Store::new(root.join("r"), installed.map(|i| bundled(root, i)));
        Updater::new(store, http, trusted(seeds), format!("{BASE}/"), PLATFORM)
    }

    fn active_text(u: &Updater) -> String {
        let a = u.store().active().unwrap();
        std::fs::read_to_string(a.dir.join("kadakas-app")).unwrap()
    }

    #[test]
    fn check_and_update_happy_path() {
        let root = tmp("up-ok");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        assert_eq!(
            u.check(Channel::Stable).unwrap(),
            UpdateStatus::Available(Offer {
                version: v("1.1.0"),
                channel: Channel::Stable,
                size: archive_bytes("1.1.0").len() as u64
            })
        );
        assert_eq!(u.update(Channel::Stable).unwrap(), v("1.1.0"));
        assert_eq!(active_text(&u), "app 1.1.0");
        // The download file is gone.
        assert_eq!(
            std::fs::read_dir(u.store().downloads_dir())
                .unwrap()
                .count(),
            0
        );
        // Nothing newer now.
        assert_eq!(
            u.check(Channel::Stable).unwrap(),
            UpdateStatus::UpToDate {
                current: Some(v("1.1.0"))
            }
        );
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::AlreadyCurrent(_))
        ));
        // And the user can roll back.
        assert_eq!(u.rollback().unwrap(), Slot::Bundled);
        assert_eq!(active_text(&u), "app 1.0.0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn first_install_without_any_version() {
        let root = tmp("up-first");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.0.0", Channel::Stable, &key(1));
        let u = updater(&root, http, &[1], None);
        assert!(matches!(
            u.check(Channel::Stable).unwrap(),
            UpdateStatus::Available(_)
        ));
        u.update(Channel::Stable).unwrap();
        assert_eq!(active_text(&u), "app 1.0.0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn tampered_manifest_is_refused_before_any_download() {
        let root = tmp("up-tamper");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let murl = format!("{BASE}/manifest-stable.json");
        let mut body = Vec::new();
        http.get(&murl, 1 << 20, &mut body).unwrap();
        let text = String::from_utf8(body)
            .unwrap()
            .replace("downloads.example.org", "evil.example.org");
        http.put(&murl, text.into_bytes());
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::BadSignature)
        ));
        assert!(matches!(
            u.check(Channel::Stable),
            Err(UpdateError::BadSignature)
        ));
        // Only the manifest was asked for. Nobody fetched the archive.
        assert!(http.requests().iter().all(|r| r.starts_with(BASE)));
        assert_eq!(active_text(&u), "app 1.0.0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn manifest_from_an_unknown_key_is_refused() {
        let root = tmp("up-key");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(9));
        let u = updater(&root, http, &[1, 2], Some("1.0.0"));
        assert!(matches!(
            u.check(Channel::Stable),
            Err(UpdateError::BadSignature)
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn key_rotation_end_to_end() {
        let root = tmp("up-rot");
        let http = Arc::new(MockHttp::new());
        // The launcher trusts the old key (1) and the new key (2).
        let u = updater(&root, http.clone(), &[1, 2], Some("1.0.0"));
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        u.update(Channel::Stable).unwrap();
        // CI switches to the new key.
        publish(&http, "1.2.0", Channel::Stable, &key(2));
        u.update(Channel::Stable).unwrap();
        assert_eq!(active_text(&u), "app 1.2.0");
        // A later launcher drops the old key. It still accepts the new key only.
        let later = updater(&root, http.clone(), &[2], Some("1.0.0"));
        publish(&http, "1.3.0", Channel::Stable, &key(1));
        assert!(later.check(Channel::Stable).is_err());
        publish(&http, "1.3.0", Channel::Stable, &key(2));
        assert!(later.check(Channel::Stable).is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn wrong_hash_is_refused() {
        let root = tmp("up-hash");
        let http = Arc::new(MockHttp::new());
        let url = publish(&http, "1.1.0", Channel::Stable, &key(1));
        // The server delivers another file of the same size.
        let mut other = archive_bytes("1.1.0");
        let last = other.len() - 1;
        other[last] ^= 0xff;
        http.put(&url, other);
        let u = updater(&root, http, &[1], Some("1.0.0"));
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::HashMismatch)
        ));
        assert_eq!(active_text(&u), "app 1.0.0");
        assert_eq!(
            std::fs::read_dir(u.store().downloads_dir())
                .unwrap()
                .count(),
            0
        );
        assert!(!u.store().versions_dir().join("1.1.0").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn truncated_download_is_refused() {
        let root = tmp("up-cut");
        let http = Arc::new(MockHttp::new());
        let url = publish(&http, "1.1.0", Channel::Stable, &key(1));
        http.cut_after(&url, 20);
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        let e = u.update(Channel::Stable).unwrap_err();
        assert!(
            matches!(e, UpdateError::SizeMismatch { got: 20, .. }),
            "{e}"
        );
        assert_eq!(active_text(&u), "app 1.0.0");
        assert_eq!(
            std::fs::read_dir(u.store().downloads_dir())
                .unwrap()
                .count(),
            0
        );
        // A larger body than announced is refused too.
        http.cut_after(&url, 1 << 20);
        let mut longer = archive_bytes("1.1.0");
        longer.extend_from_slice(b"extra");
        http.put(&url, longer);
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::Http(_))
        ));
        // The server comes back: the update works.
        http.put(&url, archive_bytes("1.1.0"));
        u.update(Channel::Stable).unwrap();
        assert_eq!(active_text(&u), "app 1.1.0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn downgrade_is_refused() {
        let root = tmp("up-down");
        let http = Arc::new(MockHttp::new());
        publish(&http, "0.9.0", Channel::Stable, &key(1));
        let u = updater(&root, http, &[1], Some("1.0.0"));
        assert_eq!(
            u.check(Channel::Stable).unwrap(),
            UpdateStatus::Older {
                current: v("1.0.0"),
                offered: v("0.9.0")
            }
        );
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::Downgrade { .. })
        ));
        assert_eq!(active_text(&u), "app 1.0.0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn channels_are_kept_apart() {
        let root = tmp("up-chan");
        let http = Arc::new(MockHttp::new());
        publish(&http, "2.0.0-beta.1", Channel::Beta, &key(1));
        // A beta manifest copied to the stable address is refused.
        let mut beta = Vec::new();
        http.get(&format!("{BASE}/manifest-beta.json"), 1 << 20, &mut beta)
            .unwrap();
        http.put(&format!("{BASE}/manifest-stable.json"), beta);
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        assert!(matches!(
            u.check(Channel::Stable),
            Err(UpdateError::ChannelMismatch { .. })
        ));
        // The beta channel gets it.
        assert!(matches!(
            u.check(Channel::Beta).unwrap(),
            UpdateStatus::Available(_)
        ));
        u.update(Channel::Beta).unwrap();
        assert_eq!(active_text(&u), "app 2.0.0-beta.1");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_platform_and_network_errors() {
        let root = tmp("up-net");
        let http = Arc::new(MockHttp::new());
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        // No manifest on the server.
        assert!(matches!(
            u.check(Channel::Stable),
            Err(UpdateError::Http(_))
        ));
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let store = Store::new(root.join("r2"), None);
        let other = Updater::new(store, http.clone(), trusted(&[1]), BASE, "plan9-mips");
        assert!(matches!(
            other.check(Channel::Stable),
            Err(UpdateError::NoPlatform(_))
        ));
        // The archive is missing on the server.
        http.remove("https://downloads.example.org/kadakas-1.1.0.tar.gz");
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::Http(_))
        ));
        // Garbage in the manifest slot.
        http.put(&format!("{BASE}/manifest-stable.json"), b"<html>".to_vec());
        assert!(matches!(
            u.check(Channel::Stable),
            Err(UpdateError::Manifest(_))
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn crash_during_update_keeps_the_old_version() {
        let root = tmp("up-crash");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let store =
            Store::new(root.join("r"), Some(bundled(&root, "1.0.0"))).with_fault(Step::Moved);
        let u = Updater::new(store, http.clone(), trusted(&[1]), BASE, PLATFORM);
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::Interrupted(_))
        ));
        assert_eq!(active_text(&u), "app 1.0.0");
        // The next start of the launcher: clean up, then update again.
        let again = updater(&root, http, &[1], Some("1.0.0"));
        again.store().recover();
        again.update(Channel::Stable).unwrap();
        assert_eq!(active_text(&again), "app 1.1.0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sha256_known_value() {
        assert_eq!(
            sha256_hex(&b"abc"[..]).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(current_platform().matches('-').count(), 1);
    }
}
