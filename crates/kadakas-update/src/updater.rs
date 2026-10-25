//! The update flow: fetch the signed manifest, verify it, download, check size and hash, install.
//!
//! Order of checks (nothing later runs when an earlier check fails):
//!
//! 1. The signature of the manifest, with the trusted keys.
//! 2. The manifest is not expired (`expires`), so old signed files cannot be served for ever.
//! 3. The channel in the manifest is the channel that was asked for.
//! 4. The version is not lower than the highest one seen on this channel (no replay).
//! 5. The version is newer than the active one (no downgrade).
//! 6. The download has exactly the announced size.
//! 7. The download has the announced SHA-256. The store checks this on the same open file that it
//!    unpacks.
//! 8. The archive paths are safe (see `archive`) and the install is atomic (see `store`).

use crate::error::{Result, UpdateError};
use crate::hex;
use crate::http::Http;
use crate::keys::TrustedKeys;
use crate::manifest::{
    Channel, MAX_ENVELOPE_BYTES, Manifest, PlatformFile, SignedManifest, Trust, is_local_url,
};
use crate::store::{ArchiveFile, Slot, Store};
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

/// The time source: seconds since 1970.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

fn system_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub struct Updater {
    clock: Clock,
    store: Store,
    http: Arc<dyn Http>,
    keys: TrustedKeys,
    manifest_base: String,
    platform: String,
    /// The site is a local folder (`file://`): unsigned manifests and the channel `local` are
    /// allowed here, and nowhere else.
    local_site: bool,
    /// False on a system where an update archive cannot be installed as it is (macOS needs a
    /// whole `.app`): the updater then asks nobody and installs nothing.
    install_supported: bool,
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
        let manifest_base = manifest_base.into().trim_end_matches('/').to_string();
        Updater {
            clock: Arc::new(system_clock),
            store,
            http,
            keys,
            local_site: manifest_base.starts_with("file://"),
            install_supported: true,
            manifest_base,
            platform: platform.into(),
        }
    }

    /// Says whether this system can install update archives (default: yes).
    pub fn with_install_support(mut self, supported: bool) -> Updater {
        self.install_supported = supported;
        self
    }

    /// True when the update site is a local folder.
    pub fn is_local_site(&self) -> bool {
        self.local_site
    }

    /// False when there is no site to ask (the channel `local` without a local site).
    pub fn can_check(&self) -> bool {
        self.install_supported && !self.manifest_base.is_empty()
    }

    /// An address from a manifest. A name without a scheme is relative to the site folder.
    fn resolve_url(&self, url: &str) -> String {
        if is_local_url(url) && !url.starts_with("file://") {
            format!("{}/{}", self.manifest_base, url.trim_start_matches('/'))
        } else {
            url.to_string()
        }
    }

    /// Replaces the clock. For tests.
    pub fn with_clock(mut self, clock: Clock) -> Updater {
        self.clock = clock;
        self
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn manifest_url(&self, channel: Channel) -> String {
        format!("{}/manifest-{}.json", self.manifest_base, channel.as_str())
    }

    /// Downloads the manifest of a channel and checks it (steps 1 and 2).
    pub fn fetch_manifest(&self, channel: Channel) -> Result<Manifest> {
        self.fetch_trusted(channel).map(|(m, _)| m)
    }

    /// Like [`Updater::fetch_manifest`], and says whether the manifest was signed. An unsigned
    /// manifest is possible on a local site only. It has no expiry and no replay check: a
    /// developer builds the same version again and again.
    pub fn fetch_trusted(&self, channel: Channel) -> Result<(Manifest, Trust)> {
        if !self.install_supported {
            return Err(UpdateError::Unsupported);
        }
        if channel == Channel::Local && !self.local_site {
            return Err(UpdateError::LocalOnly(
                "the channel local needs a local update site".into(),
            ));
        }
        let mut body = Vec::new();
        self.http
            .get(&self.manifest_url(channel), MAX_ENVELOPE_BYTES, &mut body)?;
        let (manifest, trust) = SignedManifest::parse(&body)?.open(&self.keys, self.local_site)?;
        if manifest.channel != channel {
            return Err(UpdateError::ChannelMismatch {
                expected: channel.as_str().into(),
                got: manifest.channel.as_str().into(),
            });
        }
        if trust == Trust::Unsigned {
            return Ok((manifest, trust));
        }
        manifest.check_fresh((self.clock)())?;
        if let Some(seen) = self.store.seen(channel)
            && manifest.version < seen
        {
            return Err(UpdateError::Replayed {
                seen: seen.to_string(),
                offered: manifest.version.to_string(),
            });
        }
        self.store.note_seen(channel, &manifest.version)?;
        Ok((manifest, trust))
    }

    /// The newest installed version that the channel may start (see `choose::eligible`).
    pub fn newest_installed(&self, channel: Channel) -> Option<Version> {
        self.store
            .list()
            .into_iter()
            .filter(|i| crate::choose::eligible(&i.meta.label, channel))
            .map(|i| i.version)
            .max()
    }

    fn judge(&self, manifest: &Manifest) -> Result<UpdateStatus> {
        let file = manifest.file_for(&self.platform)?;
        let current = self.newest_installed(manifest.channel);
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

    /// Checks, downloads, verifies and installs the newer version, and switches to it. Returns
    /// the new version. A newer version that is installed but not active (Auto-download) is
    /// only switched to.
    pub fn update(&self, channel: Channel) -> Result<Version> {
        let (manifest, trust) = self.fetch_trusted(channel)?;
        match self.judge(&manifest)? {
            UpdateStatus::Available(_) => {}
            UpdateStatus::UpToDate { .. } => {
                let slot = Slot::Version(manifest.version.clone());
                if self.store.active().is_some_and(|a| a.slot == slot)
                    || !self.store.list().iter().any(|i| i.slot == slot)
                {
                    return Err(UpdateError::AlreadyCurrent(manifest.version.to_string()));
                }
                self.store.activate(&slot)?;
                return Ok(manifest.version);
            }
            UpdateStatus::Older { current, offered } => {
                return Err(UpdateError::Downgrade {
                    current: current.to_string(),
                    offered: offered.to_string(),
                });
            }
        }
        let archive = self.fetch_archive(&manifest)?;
        let label = Self::label_for(&manifest, trust);
        let result = self
            .store
            .install_with(&manifest.version, &archive, false, label, true);
        let _ = std::fs::remove_file(&archive.path);
        result.map(|()| manifest.version)
    }

    /// Checks and downloads a newer version into the store, and does not switch to it.
    /// Returns the version when a download happened, `None` when there is nothing newer.
    pub fn stage(&self, channel: Channel) -> Result<Option<Version>> {
        let (manifest, trust) = self.fetch_trusted(channel)?;
        match self.judge(&manifest)? {
            UpdateStatus::Available(_) => {}
            UpdateStatus::UpToDate { .. } | UpdateStatus::Older { .. } => return Ok(None),
        }
        let archive = self.fetch_archive(&manifest)?;
        let label = Self::label_for(&manifest, trust);
        let result = self.store.stage(&manifest.version, &archive, label);
        let _ = std::fs::remove_file(&archive.path);
        result.map(|()| Some(manifest.version))
    }

    /// The label of a version that came from a manifest: its channel, or `local` when the
    /// manifest was unsigned.
    fn label_for(manifest: &Manifest, trust: Trust) -> &'static str {
        match trust {
            Trust::Signed => manifest.channel.label(),
            Trust::Unsigned => Channel::Local.label(),
        }
    }

    fn fetch_archive(&self, manifest: &Manifest) -> Result<ArchiveFile> {
        let file = manifest.file_for(&self.platform)?;
        let part = self.download(&manifest.version, file)?;
        Ok(ArchiveFile {
            path: part,
            sha256: file.sha256.clone(),
        })
    }

    /// Switches back to the previous version.
    pub fn rollback(&self) -> Result<Slot> {
        self.store.rollback()
    }

    /// Downloads the archive to a file and checks the size (step 6). The hash is checked by the
    /// store when it installs (step 7). The file is deleted when a check fails.
    fn download(&self, version: &Version, file: &PlatformFile) -> Result<std::path::PathBuf> {
        std::fs::create_dir_all(self.store.downloads_dir())?;
        let part = self
            .store
            .downloads_dir()
            .join(format!("{version}.tar.gz.part"));
        let url = self.resolve_url(&file.url);
        let result = if url.starts_with("file://") && !self.local_site {
            // The manifest check refuses this already. A second wall: a network manifest never
            // makes the launcher read a file of this machine.
            Err(UpdateError::LocalOnly(format!("file address {url}")))
        } else {
            self.download_to(&url, file, &part)
        };
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result.map(|()| part)
    }

    fn download_to(&self, url: &str, file: &PlatformFile, part: &Path) -> Result<()> {
        let mut out = std::fs::File::create(part)?;
        let got = self.http.get(url, file.size, &mut out)?;
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
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::archive::tests::{build, file};
    use crate::http::MockHttp;
    use crate::sign;
    use crate::store::tests::{bundled, exe_name, tmp, v};
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
            file(&exe_name("kadakas-app"), format!("app {ver}").as_bytes()),
            file(&exe_name("kadakas"), b"cli"),
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
            published: 1_000,
            expires: 1_000 + 30 * 86_400,
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
            .with_clock(Arc::new(|| NOW))
    }

    /// The clock of the tests: inside the validity of the sample manifests.
    pub const NOW: u64 = 5_000;

    fn active_text(u: &Updater) -> String {
        let a = u.store().active().unwrap();
        std::fs::read_to_string(a.dir.join(exe_name("kadakas-app"))).unwrap()
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
        crate::store::tests::rm_dir(&root);
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
        crate::store::tests::rm_dir(&root);
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
        crate::store::tests::rm_dir(&root);
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
        crate::store::tests::rm_dir(&root);
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
        crate::store::tests::rm_dir(&root);
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
        crate::store::tests::rm_dir(&root);
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
        crate::store::tests::rm_dir(&root);
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
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn a_development_build_is_not_offered_the_older_release() {
        // A build of main (0.1.1-dev) is newer than the last release 0.1.0.
        let root = tmp("up-dev");
        let http = Arc::new(MockHttp::new());
        publish(&http, "0.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http, &[1], Some("0.1.1-dev"));
        assert_eq!(
            u.check(Channel::Stable).unwrap(),
            UpdateStatus::Older {
                current: v("0.1.1-dev"),
                offered: v("0.1.0")
            }
        );
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::Downgrade { .. })
        ));
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn a_beta_build_follows_beta_and_stable() {
        // The rehearsal: bundled 0.1.0-beta.1. The next beta and the stable release are offered.
        let root = tmp("up-beta1");
        let http = Arc::new(MockHttp::new());
        publish(&http, "0.1.0-beta.2", Channel::Beta, &key(1));
        let u = updater(&root, http, &[1], Some("0.1.0-beta.1"));
        assert!(matches!(
            u.check(Channel::Beta).unwrap(),
            UpdateStatus::Available(_)
        ));
        crate::store::tests::rm_dir(&root);
        let root = tmp("up-beta2");
        let http = Arc::new(MockHttp::new());
        publish(&http, "0.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http, &[1], Some("0.1.0-beta.1"));
        assert!(matches!(
            u.check(Channel::Stable).unwrap(),
            UpdateStatus::Available(_)
        ));
        crate::store::tests::rm_dir(&root);
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
        crate::store::tests::rm_dir(&root);
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
        let other = Updater::new(store, http.clone(), trusted(&[1]), BASE, "plan9-mips")
            .with_clock(Arc::new(|| NOW));
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
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn crash_during_update_keeps_the_old_version() {
        let root = tmp("up-crash");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let store =
            Store::new(root.join("r"), Some(bundled(&root, "1.0.0"))).with_fault(Step::Moved);
        let u = Updater::new(store, http.clone(), trusted(&[1]), BASE, PLATFORM)
            .with_clock(Arc::new(|| NOW));
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
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn an_expired_manifest_is_refused() {
        let root = tmp("up-expired");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        assert!(u.check(Channel::Stable).is_ok());
        // 31 days later the same file is a freeze attack: the server serves an old manifest.
        let late =
            updater(&root, http, &[1], Some("1.0.0")).with_clock(Arc::new(|| 1_000 + 31 * 86_400));
        let e = late.check(Channel::Stable).unwrap_err();
        assert!(matches!(e, UpdateError::Expired { .. }), "{e}");
        assert!(matches!(
            late.update(Channel::Stable),
            Err(UpdateError::Expired { .. })
        ));
        assert_eq!(active_text(&late), "app 1.0.0");
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn a_replayed_older_manifest_is_refused_even_after_a_rollback() {
        let root = tmp("up-replay");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        u.update(Channel::Stable).unwrap();
        // The user rolls back to the bundled 1.0.0. Now 1.0.5 looks newer than the active version.
        u.rollback().unwrap();
        assert_eq!(active_text(&u), "app 1.0.0");
        // An attacker (or a confused server) serves a genuine, signed, older manifest.
        publish(&http, "1.0.5", Channel::Stable, &key(1));
        let e = u.check(Channel::Stable).unwrap_err();
        assert!(matches!(e, UpdateError::Replayed { .. }), "{e}");
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::Replayed { .. })
        ));
        assert_eq!(active_text(&u), "app 1.0.0");
        // The memory is on disk: a new launcher process refuses it too.
        let again = updater(&root, http.clone(), &[1], Some("1.0.0"));
        assert!(again.check(Channel::Stable).is_err());
        // The same version again is not a replay (CI signs it again before it expires).
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        // 1.1.0 is still installed (the rollback only switched away from it), so it is not
        // "available" again; Auto-update does not start it either (it was rolled back from).
        assert!(matches!(
            again.check(Channel::Stable).unwrap(),
            UpdateStatus::UpToDate { .. }
        ));
        // Another channel has its own memory.
        publish(&http, "1.0.2-beta.1", Channel::Beta, &key(1));
        assert!(again.check(Channel::Beta).is_ok());
        crate::store::tests::rm_dir(&root);
    }

    // ---- Local update sites and unsigned manifests (PLAT-17) ----

    use crate::http::Http;
    use crate::local::RoutingHttp;
    use crate::local::path_to_file_url;
    use crate::manifest::SignedManifest;

    /// A folder with `manifest-<channel>.json` (unsigned when `signer` is `None`) and an archive.
    fn site(dir: &Path, ver: &str, channel: Channel, signer: Option<&SigningKey>, url: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let bytes = archive_bytes(ver);
        std::fs::write(dir.join("kadakas.tar.gz"), &bytes).unwrap();
        let mut platforms = BTreeMap::new();
        platforms.insert(
            PLATFORM.to_string(),
            PlatformFile {
                url: url.to_string(),
                size: bytes.len() as u64,
                sha256: sha256_hex(&bytes[..]).unwrap(),
            },
        );
        let m = Manifest {
            version: v(ver),
            channel,
            published: 1_000,
            expires: 1_000 + 30 * 86_400,
            platforms,
        };
        let envelope = match signer {
            Some(k) => sign::sign_manifest(&m, k),
            None => SignedManifest {
                payload: m.to_payload(),
                signature: String::new(),
            },
        };
        std::fs::write(
            dir.join(format!("manifest-{}.json", channel.as_str())),
            envelope.to_json(),
        )
        .unwrap();
    }

    fn local_updater(root: &Path, site_dir: &Path, installed: &str) -> Updater {
        let store = Store::new(root.join("r"), Some(bundled(root, installed)));
        let http: Arc<dyn Http> = Arc::new(RoutingHttp::new(Arc::new(MockHttp::new())));
        Updater::new(
            store,
            http,
            trusted(&[1]),
            path_to_file_url(site_dir),
            PLATFORM,
        )
        .with_clock(Arc::new(|| NOW))
    }

    #[test]
    fn an_unsigned_manifest_works_from_a_local_folder_and_installs_as_local() {
        let root = tmp("up-local");
        let dir = root.join("site");
        // The addresses are relative to the folder. An unsigned local manifest has no expiry:
        // it works with a clock far past `expires`.
        site(&dir, "1.1.0", Channel::Local, None, "kadakas.tar.gz");
        let u = local_updater(&root, &dir, "1.0.0").with_clock(Arc::new(|| 9_999_999_999));
        assert!(u.is_local_site() && u.can_check());
        let (m, trust) = u.fetch_trusted(Channel::Local).unwrap();
        assert_eq!((m.version, trust), (v("1.1.0"), Trust::Unsigned));
        assert!(matches!(
            u.check(Channel::Local).unwrap(),
            UpdateStatus::Available(_)
        ));
        assert_eq!(u.update(Channel::Local).unwrap(), v("1.1.0"));
        let item = u
            .store()
            .list()
            .into_iter()
            .find(|i| i.version == v("1.1.0"))
            .unwrap();
        assert_eq!(item.meta.label, "local");
        // The same build again is up to date, and there is no replay memory for unsigned local
        // manifests (a developer builds the same version again and again).
        assert!(matches!(
            u.check(Channel::Local).unwrap(),
            UpdateStatus::UpToDate { .. }
        ));
        assert_eq!(u.store().seen(Channel::Local), None);
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn an_unsigned_manifest_is_refused_everywhere_else() {
        let root = tmp("up-unsigned-net");
        let http = Arc::new(MockHttp::new());
        // On a network site: an unsigned manifest, with a good-looking envelope.
        let unsigned = SignedManifest {
            payload: Manifest {
                version: v("1.1.0"),
                channel: Channel::Stable,
                published: 1_000,
                expires: 9_000,
                platforms: BTreeMap::new(),
            }
            .to_payload(),
            signature: String::new(),
        };
        http.put(
            &format!("{BASE}/manifest-stable.json"),
            unsigned.to_json().into_bytes(),
        );
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        assert!(matches!(
            u.check(Channel::Stable),
            Err(UpdateError::LocalOnly(_))
        ));
        // The channel local needs a local site, and is never asked of the network.
        assert!(!u.is_local_site());
        let e = u.check(Channel::Local).unwrap_err();
        assert!(matches!(e, UpdateError::LocalOnly(_)), "{e}");
        assert!(!http.requests().iter().any(|r| r.contains("local")));
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn a_signed_manifest_of_the_local_channel_is_refused_on_a_network_site() {
        let m = crate::manifest::tests::sample("1.1.0", Channel::Local);
        let signed = sign::sign_manifest(&m, &key(1));
        let e = signed.verify(&trusted(&[1])).unwrap_err();
        assert!(matches!(e, UpdateError::LocalOnly(_)), "{e}");
        // On a local site it is fine.
        let (got, trust) = signed.open(&trusted(&[1]), true).unwrap();
        assert_eq!((got.channel, trust), (Channel::Local, Trust::Signed));
    }

    #[test]
    fn a_local_unsigned_manifest_may_name_local_files_only() {
        let root = tmp("up-unsigned-https");
        let dir = root.join("site");
        site(
            &dir,
            "1.1.0",
            Channel::Local,
            None,
            "https://evil.example/kadakas.tar.gz",
        );
        let u = local_updater(&root, &dir, "1.0.0");
        let e = u.check(Channel::Local).unwrap_err();
        assert!(matches!(e, UpdateError::Manifest(_)), "{e}");
        // The same file with a file:// address is fine.
        let url = path_to_file_url(&dir.join("kadakas.tar.gz"));
        site(&dir, "1.1.0", Channel::Local, None, &url);
        assert_eq!(u.update(Channel::Local).unwrap(), v("1.1.0"));
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn a_signed_manifest_on_a_local_site_is_checked_like_any_other() {
        let root = tmp("up-local-signed");
        let dir = root.join("site");
        // Signed with the right key: the label is the channel, the usual rules apply (expiry).
        site(
            &dir,
            "1.1.0",
            Channel::Stable,
            Some(&key(1)),
            "kadakas.tar.gz",
        );
        let u = local_updater(&root, &dir, "1.0.0");
        assert_eq!(u.fetch_trusted(Channel::Stable).unwrap().1, Trust::Signed);
        let late = local_updater(&root, &dir, "1.0.0").with_clock(Arc::new(|| 1_000 + 31 * 86_400));
        assert!(matches!(
            late.check(Channel::Stable),
            Err(UpdateError::Expired { .. })
        ));
        // A wrong key is refused on a local site too: only an empty signature means unsigned.
        site(
            &dir,
            "1.1.0",
            Channel::Stable,
            Some(&key(9)),
            "kadakas.tar.gz",
        );
        assert!(matches!(
            u.check(Channel::Stable),
            Err(UpdateError::BadSignature)
        ));
        // A broken signature is not "unsigned".
        let mut env: SignedManifest =
            serde_json::from_slice(&std::fs::read(dir.join("manifest-stable.json")).unwrap())
                .unwrap();
        env.signature = "zz".into();
        std::fs::write(dir.join("manifest-stable.json"), env.to_json()).unwrap();
        assert!(matches!(
            u.check(Channel::Stable),
            Err(UpdateError::BadKey(_))
        ));
        // A missing folder is an error, not a panic.
        let gone = local_updater(&root, &root.join("nothing"), "1.0.0");
        assert!(gone.check(Channel::Local).is_err());
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn a_network_updater_never_reads_a_local_file() {
        let root = tmp("up-wall");
        let http = Arc::new(MockHttp::new());
        let u = updater(&root, http, &[1], Some("1.0.0"));
        let file = PlatformFile {
            url: "file:///etc/hosts".into(),
            size: 10,
            sha256: "ab".repeat(32),
        };
        let e = u.download(&v("1.1.0"), &file).unwrap_err();
        assert!(matches!(e, UpdateError::LocalOnly(_)), "{e}");
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn stage_downloads_without_switching_and_update_then_switches() {
        let root = tmp("up-stage");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http.clone(), &[1], Some("1.0.0"));
        assert_eq!(u.stage(Channel::Stable).unwrap(), Some(v("1.1.0")));
        assert_eq!(active_text(&u), "app 1.0.0", "the pointer did not move");
        let item = u
            .store()
            .list()
            .into_iter()
            .find(|i| i.version == v("1.1.0"))
            .unwrap();
        assert_eq!(item.meta.label, "stable");
        // Nothing new to download now.
        assert_eq!(u.stage(Channel::Stable).unwrap(), None);
        assert_eq!(u.newest_installed(Channel::Stable), Some(v("1.1.0")));
        // The window's Update only switches (no second download).
        let before = http.requests().len();
        assert_eq!(u.update(Channel::Stable).unwrap(), v("1.1.0"));
        assert_eq!(active_text(&u), "app 1.1.0");
        assert_eq!(http.requests().len(), before + 1, "only the manifest");
        // Active already: refused as before.
        assert!(matches!(
            u.update(Channel::Stable),
            Err(UpdateError::AlreadyCurrent(_))
        ));
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn nightly_versions_do_not_count_for_the_stable_channel() {
        let root = tmp("up-chan");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.2.0-beta.1", Channel::Beta, &key(1));
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http, &[1], Some("1.0.0"));
        assert_eq!(u.stage(Channel::Beta).unwrap(), Some(v("1.2.0-beta.1")));
        assert_eq!(u.newest_installed(Channel::Stable), Some(v("1.0.0")));
        assert_eq!(u.newest_installed(Channel::Beta), Some(v("1.2.0-beta.1")));
        // Stable 1.1.0 is still available for a stable user, although a nightly 1.2.0 is there.
        assert!(matches!(
            u.check(Channel::Stable).unwrap(),
            UpdateStatus::Available(_)
        ));
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn an_unsigned_manifest_must_be_for_the_channel_local() {
        let root = tmp("up-unsigned-stable");
        let dir = root.join("site");
        site(&dir, "1.1.0", Channel::Stable, None, "kadakas.tar.gz");
        let u = local_updater(&root, &dir, "1.0.0");
        let e = u.check(Channel::Stable).unwrap_err();
        assert!(matches!(e, UpdateError::LocalOnly(_)), "{e}");
        site(&dir, "1.1.0", Channel::Beta, None, "kadakas.tar.gz");
        assert!(matches!(
            u.check(Channel::Beta),
            Err(UpdateError::LocalOnly(_))
        ));
        // The channel local is fine.
        site(&dir, "1.1.0", Channel::Local, None, "kadakas.tar.gz");
        assert!(u.check(Channel::Local).is_ok());
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn a_system_without_install_support_asks_nobody_and_installs_nothing() {
        let root = tmp("up-unsupported");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http.clone(), &[1], Some("1.0.0")).with_install_support(false);
        assert!(!u.can_check());
        for r in [
            u.check(Channel::Stable).map(|_| ()),
            u.update(Channel::Stable).map(|_| ()),
            u.stage(Channel::Stable).map(|_| ()),
        ] {
            assert!(matches!(r, Err(UpdateError::Unsupported)));
        }
        assert!(http.requests().is_empty());
        assert_eq!(active_text(&u), "app 1.0.0");
        crate::store::tests::rm_dir(&root);
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
