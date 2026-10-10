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
3. Refuse a manifest for another channel. Refuse a version that is older than the active one. Only the Roll back button goes to an older version.
4. Download the archive for this system and check that it has exactly the announced size and SHA-256.
5. Unpack it into `versions/.tmp-<version>`, rename it to `versions/<version>`, and then write `current`. The write of `current` is the switch. If the computer stops before it, the old version still runs, and the next start of the launcher removes the half-made folder.

The archive is a `.tar.gz` with `kadakas-app` and `kadakas` (with `.exe` on Windows) at the top level. The unpacker accepts plain files and folders only. It refuses links, absolute paths, `..`, backslashes and `:` in names.

A version folder may also hold `flavors.json` to list more programs (other UI flavors). Today only `kadakas-app` exists.

### How CI signs the manifest

- `release.yml` builds the installers and one update archive per system: `kadakas-update-<platform>.tar.gz`, with the platform names `macos-aarch64`, `macos-x86_64`, `windows-x86_64` and `linux-x86_64`. They go into the draft release.
- After you publish the release, `.github/workflows/update-manifest.yml` runs. It downloads the archives, runs `sign-manifest build` (size and SHA-256 come from the files), signs the result with `sign-manifest sign`, checks it with `sign-manifest verify` against the keys in `crates/kadakas-launcher/trusted-keys.txt`, and uploads `manifest-stable.json` or `manifest-beta.json` to the release `update-manifests`. A version with a dash (`1.2.0-beta.1`) goes to the beta channel.
- The private key is the secret `KADAKAS_UPDATE_SIGNING_KEY`: 64 hex characters (a 32-byte seed). Only the signing step sees it, as an environment variable. It never goes to disk, and the launcher never has it.

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

### Not done yet

- The launcher does not update itself. A new launcher comes with a new installer.
- There is no download progress bar.
- The installers are not signed (see the next section).

## TODO: signing

Nothing is signed. Users will see warnings on first start. Do this before a public release.

- macOS: join the Apple Developer Program. Sign the `.app` with a Developer ID Application certificate (`[macos] signing-identity` in `packager.toml`), then notarize it (`notarytool`) and staple the ticket. Keep the certificate and the API key in GitHub secrets.
- Windows: buy a code signing certificate (or use Azure Trusted Signing). Set the signing command for NSIS and WiX in `packager.toml`. Without it, SmartScreen warns.
- Linux: sign the `.deb` repository or publish a checksum file. AppImage signing is optional.
- Updates: the launcher has its own signed manifests (see "Launcher and updates"). The `cargo-packager` updater is not used.
