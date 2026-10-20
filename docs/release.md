# Release and installers

This page tells how to build the installers of Kadakas and where the files go on each system.

## Tool: cargo-packager

We use `cargo-packager` (settings in `packager.toml`). We chose it over `cargo-bundle` for these reasons.

- It builds all targets we need: `.app` and `.dmg` (macOS), NSIS `.exe` and WiX `.msi` (Windows), AppImage and `.deb` (Linux). `cargo-bundle` has no Windows installer and no AppImage.
- One file, `packager.toml`, holds the settings for all systems. It also holds the file type association (`.kad`) and the extra binary.
- It is maintained, and it can sign and update packages later without a change of tool.

## Build on your own machine

```sh
cargo install cargo-packager --locked
cargo build --release -p kadakas-app -p kadakas-modeler -p kadakas-cli -p kadakas-launcher
cargo packager --release --formats app,dmg        # macOS
cargo packager --release --formats nsis,wix       # Windows
cargo packager --release --formats appimage,deb   # Linux
```

The files go to `dist/`. If your cargo target folder is not `target/`, make `target` a link to it.
If the command finds no configuration (for example, when the checkout is inside a hidden folder), give the
full path: `cargo packager --release --config $PWD/packager.toml`.

The icon is our own. `python3 packaging/icons/make_icons.py` draws `icon.svg` and renders all PNG sizes, `icon.ico` and
`icon.icns` with the Python standard library only.

## License texts in the installers

Every installer ships `THIRD_PARTY_NOTICES.txt` (the licenses of all libraries in Kadakas), `LICENSE-APACHE` and
`LICENSE-MIT`. The list `resources` in `packager.toml` puts them in the package. `scripts/notices.sh` makes the notices
file with `cargo-about` (settings in `about.toml` and `about.hbs`). CI checks that the committed file is up to date, and
the release workflow makes it again before it packages. CI also runs `cargo deny check` (`deny.toml`): a dependency
with a license that is not permissive, such as GPL or AGPL, fails the build.

```sh
cargo install cargo-about --locked --features cli
scripts/notices.sh
```

## macOS notes

- `packaging/macos/Info.plist` is merged into the one that the packager writes. It declares the type `net.kadakas.drawing` (it conforms to `public.zip-archive`, because a `.kad` file is a ZIP container) and makes Kadakas the owner of `.kad` files.
- The `.app` build was checked on macOS 26 (Apple silicon). The `.dmg` step runs a Finder script for the window layout. It needs a normal desktop session, and it timed out in a headless run. Run it on a Mac with a user logged in.
- The bundle id is `net.kadakas.Kadakas`. Test the double click on a `.kad` file on a real Mac before the first release.

## Where is the `kadakas` command line tool?

The installer puts the `kadakas` tool next to the app. Agents and MCP clients can start `kadakas mcp` from there.

| System | Path of the tool |
|---|---|
| macOS | `/Applications/Kadakas.app/Contents/MacOS/kadakas` |
| Windows | `C:\Program Files\Kadakas\kadakas.exe` (per-user install: `%LOCALAPPDATA%\Kadakas\kadakas.exe`) |
| Linux `.deb` | `/usr/bin/kadakas` |
| Linux AppImage | inside the image at `usr/bin/kadakas`. Run `./Kadakas.AppImage --appimage-extract`, then use `squashfs-root/usr/bin/kadakas`. |

An MCP client config on macOS looks like this.

```json
{ "mcpServers": { "kadakas": { "command": "/Applications/Kadakas.app/Contents/MacOS/kadakas", "args": ["mcp"] } } }
```

## Release workflow

`.github/workflows/release.yml` runs when you push a tag that starts with `v` (for example `v0.1.0`). It builds the
installers on macOS (`macos-14` for arm64, `macos-15-intel` for Intel), Windows and Linux, builds the web version, and makes a draft GitHub release
with all files. You can also start it by hand from the Actions page. Nobody has run it on GitHub yet. Expect to fix
small problems at the first run (for example the runner image names).

The workflow has read-only rights, except the publish job (`contents: write`). All actions are pinned to commit SHAs, and `cargo-packager` to version 0.11.8 (`CARGO_PACKAGER_VERSION`). Update them on purpose, not by accident.

Steps for a release: change `version` in `packager.toml` and in the crates, commit, tag (`git tag v0.1.0`), push the tag, check the draft release, publish it.

## Launcher and updates

The launcher (`kadakas-launcher`, PLAT-06) keeps the app up to date without a new installer. The installer puts the launcher next to the app. The launcher starts the app, and it can fetch newer versions of the app and the `kadakas` tool.

### The launcher is the entry point (PLAT-16)

On Windows and Linux, shortcuts, the taskbar icon and `.kad` files start `kadakas-launcher` (it is the main binary in `packager.toml`). **On macOS the app is the main binary** (`packager-macos.toml`) and the launcher does not install updates there: an update archive would hold bare executables, not a `.app` (generic Dock icon, wrong name, no document types), and Finder sends a double click on a `.kad` file as an Apple event that the launcher does not handle. Updates on macOS need a whole signed `Kadakas.app` as the update archive, a store that keeps `.app` bundles, and a launcher (or a bundle stub) that forwards the open-documents Apple event. Until then there is no macOS update archive (`release.yml`, `update-manifest.yml`), and `Updater::with_install_support(false)` makes the launcher ask nobody and install nothing on macOS. With no window, the launcher:

1. settles the runs that ended while no launcher watched (the crash marks, see below),
2. chooses the version (see "Which version starts") and **starts it at once**, with the files that were given as arguments (after `--`, so a name that starts with a dash is safe),
3. checks the manifest in a background thread, and downloads by the mode,
4. waits for the app. When the app asks for a restart and ended cleanly, it chooses again and starts the new version with the same documents.

`kadakas-launcher manage` opens the window (versions, channel, flavor, profile). `kadakas-launcher add PATH` puts a local build into the store (below). The app that the installer brought (the bundled app, next to the launcher) is the base version. It is never copied, never deleted, and always the last fallback.

### Where things are

- Updates go to the user's state folder, in `launcher/` (macOS `~/Library/Application Support/kadakas/launcher`, Windows `%LOCALAPPDATA%\kadakas\launcher`, Linux `~/.local/state/kadakas/launcher`). Set `KADAKAS_LAUNCHER_ROOT` to use another folder.
- Inside: `versions/<version>/` (one folder per version, with `.kadakas-meta.json`: label, git commit, time added, kept), `current` (the version that the person chose, and the one before it), `launcher.json` (the settings, below), `health.json` (versions that crashed or were rolled back), `runs/` (small files of each running app), `seen.json`, `lock` and `downloads/`.

### Settings (Preferences > Updates)

One schema (`kadakas_update::Settings`), one file (`launcher.json`). The app writes the update part (the write keeps flavor and profile). The launcher reads it at every start and before every check.

| Setting | Values | Default |
|---|---|---|
| Mode | Auto-update, Auto-download, Notify only | Auto-update |
| Channel | Stable, Nightly (the `beta` manifest channel), Local | Stable |
| Older versions to keep | 0 to 50 | 1 |
| Local update site (channel Local) | a `file://` address | empty |

- **Auto-update** downloads new versions and starts the newest one that did not crash at its last start. **Auto-download** downloads, and the person chooses when to switch (the "Restart to update" button). **Notify only** shows "Kadakas X is available" with a Download button.
- The launcher and the app talk with files only (`handshake.rs`). The app has no network code. It reads `runs/<id>.status.json`, and writes `<id>.download` (Notify only), `<id>.restart.json` (the open documents) and `<id>.clean` (the clean-exit mark).

### Which version starts

`choose.rs` is a pure function, tested without a window.

- **Channel.** `stable` starts stable and bundled versions. `nightly` also starts nightly ones. `local` starts all, and prefers the newest local build. A local build never starts by itself on another channel.
- **Mode.** Auto-update: the newest eligible version. The other two modes: the version the person chose (`current`), so a download does not change what starts.
- **Your own choice.** A version that the person chose by hand ("Use this version") starts first in every mode, also in Auto-update, until a newer version that the channel allows is added. Then the newest wins again. A rollback is a choice too, and skips the version it left.
- **Crash marker.** The app writes `<id>.clean` when it closes in the normal way. A run without it can also be a shutdown, a logout, a force quit or a power loss, so one long run without the mark is not enough: a version is skipped when the run was short (under 60 seconds) or when two runs in a row ended without the mark. A clean run clears the count. The marks are kept by version, also for the bundled app, so a new installer starts clean. The launcher starts the next version at the next start. It tells the app which version it skipped and why (a line in the status bar). A clean run clears the mark. In the launcher window, "Try again" clears it by hand. A rollback also skips the version that was left. If every version is skipped, the bundled app starts anyway.
- **Crash reports (PLAT-20).** The app writes `runs/<id>.crash.json` when it panics or crashes natively. The supervisor moves the file to `crash-reports/<id>.json` when the run ends without the clean-exit mark (at most 10 files, the oldest go first), and writes a small report of its own when the system names a crash signal and the app left no file. A clean end deletes the file. The next start of the app shows the reports and asks the person. See `docs/crash-reports.md`. The launcher window says that reports wait.
- A supervisor that dies leaves its run record. The supervisor holds a lock on its record, and the app holds a lock of its own (`<id>.app.lock`). The next launcher settles the records whose two locks are free (under the install lock, reading the record again there), and never takes a running app for a crash or prunes its folder.
- The choice of the version and the run record are made under the install lock, so a prune cannot delete the chosen folder. If the lock stays busy, or the record cannot be written, the app starts all the same (without the update features) and the launcher logs why. The restart also goes on when the switch to the new version cannot be written.

### Pruning

After an install, a download or `add`, the launcher keeps the newest version and N older ones (N is "Older versions to keep"; "newest" is by the time of adding). It never deletes the running version (its lock is held), the version in `current`, a version that the person marked "Keep", the first version below the newest ones that is not skipped (the fallback, also with N = 0), or the bundled app (it is not a folder in the store and is not counted). N = 0 keeps the newest one only.

### Restart to update

When a newer version is ready, the app shows "Kadakas X is ready." with **Restart to update** and **Later** (Auto-update and Auto-download). If the drawing has unsaved changes, a question comes first (Save and restart, Restart without saving, Cancel). A failed or cancelled save stops the restart. The window then closes through the normal close path, so the question about unsent collaboration edits still applies. On exit the app writes the restart request with the open document and the clean-exit mark. The launcher sees the clean end, switches to the new version (in Auto-download and Notify only, the accept is the switch), and starts it with the document as an argument.

### Local versions for developers (PLAT-17)

- `kadakas-launcher add PATH [--version X.Y.Z]` copies `kadakas-app`, `kadakas` and the flavors of a build folder (`kadakas-modeler` among them) (or of the folder of a program) into `versions/<X.Y.Z>-local.g<commit>/` with the label `local` and the git commit (`+dirty` for a changed tree). A changed tree or a tree without git also puts the time in the version. A local version is always a pre-release of its base number, so it cannot be taken for a release, and a release folder is never overwritten. A local build is swapped in (the old folder is renamed away, the new one renamed in, then the old one deleted), and the version in `current` is never replaced in place. `scripts/install-local-build.sh` builds in release mode and runs `add`.
- A local folder can be an update site: a `file://` address in Preferences > Updates (channel Local), or `kadakas-launcher --site file:///folder`. The folder holds `manifest-<channel>.json` and the archives (the same format as the release site). Addresses in the manifest may be relative to the folder.
- **Unsigned builds** are allowed only from these two places: an unsigned manifest (an empty `signature`) in a `file://` site, and `add`. An unsigned manifest must be for the channel `local` and may name local files only. A `file://` address with a host or a network path (`file:////server/share`) is refused. It has no expiry and no replay check. A network site with an unsigned manifest, a local file address or the channel `local` is refused (in the manifest code, and again at the download). A signature that is there must be good, also in a local folder.
- Auto-update picks a local version only on the channel Local. The window lists the label and the commit, and a local version can be started ("Use this version"), kept or removed like any other.

### What an update does

1. Download `manifest-<channel>.json` from the release `update-manifests` (`KADAKAS_UPDATE_URL` replaces the folder address, for tests).
2. Check the ed25519 signature with the public keys that are built into the launcher. Nothing in the file is used before the signature is good.
3. Refuse an expired manifest (`expires` is signed). This stops a server, or an attacker on the network, from keeping a launcher on old news by serving an old file for ever.
4. Refuse a manifest for another channel.
5. Refuse a replay: the launcher remembers the highest manifest version it has seen on each channel (`seen.json`). A signed manifest with a lower version is refused, even when the user has rolled back and the old version looks new. To publish an older build, release it with a higher version number.
6. Refuse a version that is older than the newest installed one for the channel. Only the Roll back button goes to an older version.
7. Download the archive for this system. The size must be exactly the announced size. The download has a timeout for each chunk (30 seconds) and a time budget for the whole body (60 seconds plus the size at 100 kB/s), so a stalled server ends as an error and the window never hangs. Redirects are followed, but never from https to http.
8. Open the archive once. Compute its SHA-256 from that open file, then unpack the same file. The two steps cannot see different files.
9. Unpack it into `versions/.tmp-<version>`, write the marker file `.kadakas-complete` (the archive SHA-256) and the meta file, rename the folder to `versions/<version>`, and then (Auto-update and the Update button) write `current`. The write of `current` is the switch. If the computer stops before it, the old version still runs, and the next start of the launcher removes the half-made folder. An existing folder is reused only when its marker has the same SHA-256; otherwise it is unpacked again.

One install lock (`lock` in the install folder, an advisory file lock) lets only one launcher change the folder at a time. Install and rollback wait for it (up to 10 seconds, then they report "busy"). The clean-up at start-up waits a moment, then skips when another launcher holds the lock.

When the launcher starts and the version next to it (from the installer) is newer than the one in `current`, the newer one runs, if `current` was written while an older installer was there. This is the case after the user installs a newer Kadakas over an old update. A pointer that was written with the same bundled version stands, so the person can choose an older version or a local build.

The archive is a `.tar.gz` with `kadakas-app`, `kadakas-modeler` and `kadakas` (with `.exe` on Windows) at the top level. The unpacker accepts plain files and folders only. It refuses links, absolute paths, `..`, backslashes and `:` in names.

The launcher lists `kadakas-app` as a flavor, and `kadakas-modeler` ("Kadakas Modeler (SketchUp-like)", PLAT-11) when that program is in the version folder. No file is needed for these two. A version folder may also hold `flavors.json` to list more programs (other UI flavors).

### Not tried for real

The installers were not built here, and no restart ran on a real desktop. Open points: updates on macOS (see above); a release build on Windows uses the GUI subsystem for the app and the launcher, so `kadakas-launcher add` and `--help` show no text there (use `manage`, or the `kadakas` tool for the console).

### How CI signs the manifest

- `release.yml` builds the installers and one update archive per system: `kadakas-update-<platform>.tar.gz`, with the platform names `macos-aarch64`, `macos-x86_64`, `windows-x86_64` and `linux-x86_64`. They go into the draft release.
- `.github/workflows/update-manifest.yml` runs when a release is **published**, every Monday, and by hand. It has three jobs:
  1. `plan` (no secret) checks the tag against a strict version pattern and chooses the tags to sign: the published tag, or on the weekly run the latest stable and the latest beta release.
  2. `tool` (no secret) builds `sign-manifest` once. The job that holds the key only runs the finished binary.
  3. `sign` runs in the GitHub Environment `update-signing`. It checks that the tag version equals the workspace version at that tag, downloads the archives, runs `sign-manifest build` (size and SHA-256 come from the files), `newer` (see below), `sign` and `verify` (against the keys in `crates/kadakas-launcher/trusted-keys.txt`), and uploads `manifest-stable.json` or `manifest-beta.json` to the release `update-manifests`. A version with a dash (`1.2.0-beta.1`) goes to the beta channel only. A stable release also updates `manifest-beta.json` when it is newer than the beta manifest, so beta users never fall behind stable users.
- **No backports.** Before `--clobber` replaces a published manifest, the job downloads it, checks its signature and fails when the new version is not higher (`sign-manifest newer`). The weekly re-sign allows an equal version.
- **Validity.** Each manifest has `published` and `expires`. The validity is `KADAKAS_MANIFEST_VALID_DAYS` days (repository variable, default 30). The weekly run signs again, so a manifest is never older than about a week. If the weekly job stops, manifests expire after the validity time and launchers stop offering updates (they show the "expired" error). Check the Actions page after a failed run.
- The private key is the secret `KADAKAS_UPDATE_SIGNING_KEY`: 64 hex characters (a 32-byte seed). Only the signing step sees it, as an environment variable. It never goes to disk, and the launcher never has it.

Protect the key: create the Environment `update-signing` (Settings, Environments), put the secret there and not in the repository secrets, add **required reviewers**, and under "Deployment branches and tags" allow only the default branch and the tag pattern `v*`. Then a changed workflow on another branch cannot read the key, and each signing run needs a person to approve it.

You can run the tool by hand:

```sh
cargo run -p kadakas-sign-manifest -- keygen --out signing.key        # prints the public key
cargo run -p kadakas-sign-manifest -- build --version 0.2.0 --channel stable --out manifest.json \
    --platform linux-x86_64=kadakas-update-linux-x86_64.tar.gz=https://example.org/kadakas-update-linux-x86_64.tar.gz
cargo run -p kadakas-sign-manifest -- sign --manifest manifest.json --key-file signing.key --out manifest-stable.json
cargo run -p kadakas-sign-manifest -- verify --signed manifest-stable.json --pubkey <public key>
```

### Set up the key (before the first release)

`trusted-keys.txt` has a placeholder key. Its private half was thrown away, so every update is refused until you replace it.

1. Run `sign-manifest keygen --out signing.key`. Copy the public key it prints.
2. Put the content of `signing.key` into the repository secret `KADAKAS_UPDATE_SIGNING_KEY`. Store a backup in a password manager. Delete the file.
3. Replace the key line in `crates/kadakas-launcher/trusted-keys.txt` with the public key.

### Key rotation

The launcher trusts one or two keys. To change the key without breaking old launchers:

1. Make a new key. Release a launcher that trusts both the old and the new public key (two lines in `trusted-keys.txt`). Keep signing with the old key.
2. When most users have that launcher, change the secret to the new key. Both keys work.
3. In a later launcher, remove the old key. After that, a manifest that the old key signed is refused.

Launchers that are older than step 1 cannot follow a rotation. They need a new installer.

**Revocation also needs a new installer.** The launcher does not update itself, and its keys are built in. If a signing key leaks, the new key reaches users only with a new installer (a new launcher). Until then, an attacker with the old key can sign manifests that those launchers accept (an attacker still needs a download address, and the version must be higher than the highest one seen). So: protect the key as described above, keep the validity short, and plan the rotation before the first release (ship two keys from the start if you can, with the second key kept offline as a spare).

### Not done yet

- The launcher does not update itself. A new launcher comes with a new installer.
- There is no download progress bar, and no cancel button. A stuck download ends by itself through the timeouts.
- The launcher does not check the free disk space before a download (the standard library has no portable call for it, and we did not add a dependency for it). A full disk gives a normal file error, and the old version keeps working.
- The installers are not signed (see the next section).

## TODO: signing

Nothing is signed. Users will see warnings on first start. Do this before a public release.

- macOS: join the Apple Developer Program. Sign the `.app` with a Developer ID Application certificate (`[macos] signing-identity` in `packager.toml`), then notarize it (`notarytool`) and staple the ticket. Keep the certificate and the API key in GitHub secrets.
- Windows: buy a code signing certificate (or use Azure Trusted Signing). Set the signing command for NSIS and WiX in `packager.toml`. Without it, SmartScreen warns.
- Linux: sign the `.deb` repository or publish a checksum file. AppImage signing is optional.
- Updates: the launcher has its own signed manifests (see "Launcher and updates"). The `cargo-packager` updater is not used.
