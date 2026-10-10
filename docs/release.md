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
cargo build --release -p kadakas-app -p kadakas-cli -p kadakas-launcher
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

- `packaging/macos/Info.plist` is merged into the one that the packager writes. It declares the type `net.kadakas.drawing` (it conforms to `public.json`) and makes Kadakas the owner of `.kad` files.
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

### Where things are

- The installer ships the first version of the app. The launcher lists it as "came with the installer".
- Updates go to the user's state folder, in `launcher/` (macOS `~/Library/Application Support/kadakas/launcher`, Windows `%LOCALAPPDATA%\kadakas\launcher`, Linux `~/.local/state/kadakas/launcher`). Set `KADAKAS_LAUNCHER_ROOT` to use another folder.
- Inside: `versions/<version>/` (one folder per version), `current` (a small JSON file that names the active and the previous version), `launcher.json` (channel, UI flavor, profile) and `downloads/`.
- The launcher keeps the active version and the previous one. It deletes older folders after an install.

### What an update does

1. Download `manifest-<channel>.json` from the release `update-manifests` (`KADAKAS_UPDATE_URL` replaces the folder address, for tests).
2. Check the ed25519 signature with the public keys that are built into the launcher. Nothing in the file is used before the signature is good.
3. Refuse an expired manifest (`expires` is signed). This stops a server, or an attacker on the network, from keeping a launcher on old news by serving an old file for ever.
4. Refuse a manifest for another channel.
5. Refuse a replay: the launcher remembers the highest manifest version it has seen on each channel (`seen.json`). A signed manifest with a lower version is refused, even when the user has rolled back and the old version looks new. To publish an older build, release it with a higher version number.
6. Refuse a version that is older than the active one. Only the Roll back button goes to an older version.
7. Download the archive for this system. The size must be exactly the announced size. The download has a timeout for each chunk (30 seconds) and a time budget for the whole body (60 seconds plus the size at 100 kB/s), so a stalled server ends as an error and the window never hangs. Redirects are followed, but never from https to http.
8. Open the archive once. Compute its SHA-256 from that open file, then unpack the same file. The two steps cannot see different files.
9. Unpack it into `versions/.tmp-<version>`, write the marker file `.kadakas-complete` (the archive SHA-256), rename the folder to `versions/<version>`, and then write `current`. The write of `current` is the switch. If the computer stops before it, the old version still runs, and the next start of the launcher removes the half-made folder. An existing folder is reused only when its marker has the same SHA-256; otherwise it is unpacked again.

One install lock (`lock` in the install folder, an advisory file lock) lets only one launcher change the folder at a time. Install and rollback wait for it (up to 10 seconds, then they report "busy"). The clean-up at start-up skips when another launcher holds it.

When the launcher starts and the version next to it (from the installer) is newer than the one in `current`, the newer one runs. This is the case after the user installs a newer Kadakas over an old update.

The archive is a `.tar.gz` with `kadakas-app` and `kadakas` (with `.exe` on Windows) at the top level. The unpacker accepts plain files and folders only. It refuses links, absolute paths, `..`, backslashes and `:` in names.

A version folder may also hold `flavors.json` to list more programs (other UI flavors). Today only `kadakas-app` exists.

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
