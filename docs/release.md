# Release and installers

This page tells how to build the installers of Cadektor and where the files go on each system.

## Tool: cargo-packager

We use `cargo-packager` (settings in `packager.toml`). We chose it over `cargo-bundle` for these reasons.

- It builds all targets we need: `.app` and `.dmg` (macOS), NSIS `.exe` and WiX `.msi` (Windows), AppImage and `.deb` (Linux). `cargo-bundle` has no Windows installer and no AppImage.
- One file, `packager.toml`, holds the settings for all systems. It also holds the file type association (`.cdk.json`) and the extra binary.
- It is maintained, and it can sign and update packages later without a change of tool.

## Build on your own machine

```sh
cargo install cargo-packager --locked
cargo build --release -p cadektor-app -p cadektor-cli
cargo packager --release --formats app,dmg        # macOS
cargo packager --release --formats nsis,wix       # Windows
cargo packager --release --formats appimage,deb   # Linux
```

The files go to `dist/`. If your cargo target folder is not `target/`, make `target` a link to it.
If the command finds no configuration (for example, when the checkout is inside a hidden folder), give the
full path: `cargo packager --release --config $PWD/packager.toml`.

The icon is our own. `python3 packaging/icons/make_icons.py` draws `icon.svg` and renders all PNG sizes, `icon.ico` and
`icon.icns` with the Python standard library only.

## macOS notes

- `packaging/macos/Info.plist` is merged into the one that the packager writes. It declares the type `net.cadektor.drawing` (it conforms to `public.json`) and makes Cadektor the owner of `.cdk.json` files.
- The `.app` build was checked on macOS 26 (Apple silicon). The `.dmg` step runs a Finder script for the window layout. It needs a normal desktop session, and it timed out in a headless run. Run it on a Mac with a user logged in.
- macOS looks at the last part of a name when it chooses an app, so `drawing.cdk.json` can look like a plain `.json` file to some tools. Test the double click on a real Mac before the first release.

## Where is the `cadektor` command line tool?

The installer puts the `cadektor` tool next to the app. Agents and MCP clients can start `cadektor mcp` from there.

| System | Path of the tool |
|---|---|
| macOS | `/Applications/Cadektor.app/Contents/MacOS/cadektor` |
| Windows | `C:\Program Files\Cadektor\cadektor.exe` (per-user install: `%LOCALAPPDATA%\Cadektor\cadektor.exe`) |
| Linux `.deb` | `/usr/bin/cadektor` |
| Linux AppImage | inside the image at `usr/bin/cadektor`. Run `./Cadektor.AppImage --appimage-extract`, then use `squashfs-root/usr/bin/cadektor`. |

An MCP client config on macOS looks like this.

```json
{ "mcpServers": { "cadektor": { "command": "/Applications/Cadektor.app/Contents/MacOS/cadektor", "args": ["mcp"] } } }
```

## Release workflow

`.github/workflows/release.yml` runs when you push a tag that starts with `v` (for example `v0.1.0`). It builds the
installers on macOS (arm64 and x86_64), Windows and Linux, builds the web version, and makes a draft GitHub release
with all files. You can also start it by hand from the Actions page. Nobody has run it on GitHub yet. Expect to fix
small problems at the first run (for example the runner image names).

Steps for a release: change `version` in `packager.toml` and in the crates, commit, tag (`git tag v0.1.0`), push the tag, check the draft release, publish it.

## TODO: signing

Nothing is signed. Users will see warnings on first start. Do this before a public release.

- macOS: join the Apple Developer Program. Sign the `.app` with a Developer ID Application certificate (`[macos] signing-identity` in `packager.toml`), then notarize it (`notarytool`) and staple the ticket. Keep the certificate and the API key in GitHub secrets.
- Windows: buy a code signing certificate (or use Azure Trusted Signing). Set the signing command for NSIS and WiX in `packager.toml`. Without it, SmartScreen warns.
- Linux: sign the `.deb` repository or publish a checksum file. AppImage signing is optional.
- Updates: `cargo-packager` can also make signed update files. Not set up.
