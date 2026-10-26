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
scripts/macos-app.sh target/release dist-app      # macOS: the inner app
cargo packager --release --config packager-macos.toml --formats app,dmg   # macOS
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

- `packaging/macos/Info.plist` is a template. `scripts/macos-app.sh` writes `dist-app/Info.plist` from it (the version keys), and the packager merges that file into the one that it writes. The short version is the numbers only (`0.1.0`), as Apple requires. The full version is in the key `KadakasVersion`. It declares the type `net.kadakas.drawing` (it conforms to `public.zip-archive`, because a `.kad` file is a ZIP container) and makes Kadakas the owner of `.kad` files.
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

`.github/workflows/release.yml` runs when you push a tag that starts with `v` (for example `v0.1.0` or `v0.1.0-beta.1`). It builds the
installers on macOS (`macos-14` for arm64, `macos-15-intel` for Intel), Windows and Linux, builds the web version and tests the installers (see below). Then it makes a draft GitHub release
with all files. You can also start it by hand from the Actions page. Nobody has run it on GitHub yet. Expect to fix
small problems at the first run (for example the runner image names).

The workflow has read-only rights, except the publish job (`contents: write`). All actions are pinned to commit SHAs, and `cargo-packager` to version 0.11.8 (`CARGO_PACKAGER_VERSION`). Update them on purpose, not by accident.

The workflow does not depend on the nightly workflow (`nightly.yml`: coverage, perf, fuzz). Check the last nightly run before you tag. The cargo-packager build is cached by version. The package jobs restore the Rust cache of `main`: start the workflow by hand on `main` to fill it.

Steps for a release: see "Make a release" below.

## Make a release

Use the workflow **Prepare release** (`.github/workflows/prepare-release.yml`). Do not edit versions by hand.

One time:

- Create the GitHub Environment `release` (Settings, Environments). Add a required reviewer.
- Limit the deployment branches of the environment `release` to `main`.
- Keep the repository setting "Workflow permissions" read-only. The workflows ask for their write rights themselves.
- Let `github-actions[bot]` push to `main` (no required pull request for it). If it cannot push, the push fails and nothing is left behind.

Steps:

1. Check that CI on `main` is green. Look at the last Nightly run.
2. Start **Prepare release** with `version` (for example `0.1.0-beta.1`) and `dry_run` on (the default). Nothing changes. Read the two diffs in the artifact `release-patch`. The plan job checks these points: the form of the version, a new tag, a version newer than the tags and newer than every version ever published, green CI, a filled Unreleased section in `CHANGELOG.md`, `cargo check`, the version check, the attribution check.
3. Start it again with `dry_run` off. Approve the environment `release`. The job commits `Release <version>` to `main`, creates the annotated tag `v<version>`, and pushes both in one atomic push. Then it starts **Release** on the tag. A push with the workflow token does not start other workflows, so the job starts **Release** itself. At the end it makes the second commit (see Development versions).
4. **Release** builds the installers and makes a **draft** release. A version with a dash is marked as pre-release. The notes are the section of this version in `CHANGELOG.md`.
5. Read the draft. Press **Publish**.
6. Publishing starts **Update manifest**. Approve the environment `update-signing`. A version with a dash goes to the beta channel only. A stable version goes to the stable and the beta channel.
7. Check `manifest-beta.json` (or `manifest-stable.json`) in the release `update-manifests`.
8. Test the update on a second machine. The default channel is Stable, and a beta never goes to it. To get a beta, install the previous build and open Preferences, Updates. Switch the channel to Beta and check that the launcher offers the new version. A build with the bundled version `0.1.0-beta.1` starts on every channel. It gets only the versions that its channel allows. A stable `0.1.0` is newer than `0.1.0-beta.N`, and Beta users get it too.

**The CI check.** The head of `main` often has no CI run (a `[skip ci]` commit, or a commit of the release bot). The plan job takes the newest commit among the last 20 that has a finished CI run on `main`. That run must be green. After that commit, only these changes are fine: Markdown files outside `docs/`, issue templates, and commits of the release bot that change only version files. Tests and installers read files in `docs/`, so a change there counts as a source change. Every other change is refused. Wait for CI, or start with `skip_ci_check`. A merge commit counts with the files that it changed against its first parent. The summary shows the run that the job used.

**Options.** `skip_ci_check` skips the CI test. Use it only on purpose. `allow_empty_changelog` allows an empty Unreleased section. A stable release then has no notes and **Release** stops, so write the section first. `next_dev_version` sets the next development version (`X.Y.Z-dev`).

**Queue.** The workflows use the concurrency group `release`. A run waits for the run in progress and does not cancel it. When a third run is queued, GitHub replaces the one that waits. Start the next run only after the last one ended.

By hand, on a machine: run `cargo fetch`, then `scripts/set-version.sh 0.1.0-beta.1`, then `scripts/check-version.sh`. `set-version.sh` puts all files back if a step fails. The check also runs in CI (job `version`) and in **Release**.

### Published versions are never reused

The launcher refuses a manifest that is older than the newest one it has seen. So nobody reuses or lowers a published version number, even if the release is deleted. The sign job of **Update manifest** keeps the highest version per channel in the asset `highest-published.json` of the release `update-manifests`. The file only goes up. The sign job writes it before it uploads the manifest.

- **Prepare release** compares a stable version with the stable record and with the stable tags. So a hotfix `0.1.1` can follow `0.2.0-beta.1`.
- It compares a pre-release with all tags and with the highest version of all channels.
- The sign job refuses a lower version. It accepts the same version only for the weekly re-sign.
- A failed read of the record stops the job. A network error never counts as an empty record.
- **Delete release** never removes the file. After you delete a bad release, release a **higher** version.

The record is a guard against mistakes in the process. It is not a security control. Whoever can write to the release `update-manifests` can change it.

### Development versions

After a release, **Prepare release** makes a second commit on `main`: `Set next development version <v>-dev`. After `0.1.0` the version is `0.1.1-dev`. After `0.1.0-beta.1` it is `0.1.0-dev`. The input `next_dev_version` sets another one (exactly `X.Y.Z-dev`). The result has to be newer than the release. After a release candidate, `0.1.0-dev` is older than `0.1.0-rc.1` because letters compare as text. The workflow stops, and you give an explicit `next_dev_version`.

The tag stays on the release commit, so the build has the release version. A `-dev` version sorts below the release (`0.1.0-beta.2 < 0.1.0-dev < 0.1.0`). It is never tagged and never put in a manifest. `release.yml`, `update-manifest.yml`, `sign-manifest` and the launcher refuse it. A build of `main` carries `-dev`: MSI `0.1.1-20000` (built as `0.1.1.20000`), deb `0.1.1~dev`, Python `0.1.1rc0.dev0`.

If a step fails after the push, the release is tagged already. The run summary lists the commands for each case. Start the build by hand with `gh workflow run release.yml --ref <tag>`, or set the development version by hand (run `cargo fetch` first). The second commit also runs when the build did not start. If `main` moved, the job rebases once, runs the version check, and pushes again.

### Delete a bad release

Start **Delete release** with the `tag`. It deletes the draft or published release and the tag. It removes the signed manifest in `update-manifests` if the manifest names this version. It keeps `highest-published.json`. It refuses the latest stable release unless `force` is on.

The commit `Release <version>` stays on `main`. By then `main` has the next `-dev` version from the second commit. Do not reuse the number. Release a higher version. The next **Prepare release** run works as usual.

### Version forms

The workspace version in `Cargo.toml` is the only truth. Each package format gets the form it takes. `scripts/packager-version.sh` is the one place for the mappings.

| Place | Form of `0.1.0-beta.1` | Form of `0.1.1-dev` |
|---|---|---|
| App, launcher, manifest, tag, NSIS, AppImage, `.dmg` | `0.1.0-beta.1` | `0.1.1-dev` |
| macOS `CFBundleShortVersionString` (digits and dots only, as Apple requires) | `0.1.0` | `0.1.1` |
| macOS `CFBundleVersion` of the inner app (a build number) | `20261026.103000` | `20261026.103000` |
| macOS key `KadakasVersion` (the full version) | `0.1.0-beta.1` | `0.1.1-dev` |
| Windows MSI (WiX). cargo-packager turns a numeric pre-release into the fourth number. | `0.1.0-10001`, built as `0.1.0.10001` | `0.1.1-20000`, built as `0.1.1.20000` |
| Debian `.deb` | `0.1.0~beta.1` (a tilde sorts before `0.1.0`, a dash sorts after) | `0.1.1~dev` |
| Python (PEP 440) | `0.1.0b1` | `0.1.1rc0.dev0` (above the betas, below the release) |

The numbers of the MSI keep the order: alpha is N, beta is 10000+N, dev is 20000, rc is 30000+N (N below 10000). Major and minor go up to 255 for an MSI.

`release.yml` builds the MSI and the `.deb` with a copy of the packager config that has the mapped version. `crates/kadakas-update/tests/package_versions.rs` runs the same semver conversion as cargo-packager 0.11.8 on every mapped value. `python/tests/test_version_forms.py` checks the PEP 440 order. Only CI confirms the real MSI and `.deb` builds. The MSI file name has `0.1.0.10001`. Windows Installer ignores the fourth number when it compares versions, so `0.1.0-beta.1` and `0.1.0` look the same to it. Beta users update through the launcher, not through the MSI.

GitHub renames some uploaded assets. A character such as `~` in the name of the `.deb` becomes a dot. The package inside keeps its version.

## Smoke test of the installers (PLAT-28)

After the build, the job `smoke` of `release.yml` installs every package on a clean runner of its own system. It starts the program the first time, and it checks the result. The job `publish` needs it: a failed smoke test blocks the release. The same job runs on a manual start, so a branch can rehearse it. The nightly pre-release is a tag with a dash (`v0.1.0-beta.1`). It goes through the same workflow and the same test. `nightly.yml` builds no installers.

| Package | Runner | How it is installed |
|---|---|---|
| MSI | `windows-latest` | `msiexec /i /qn` |
| NSIS `.exe` | `windows-latest` | `/S` |
| `.dmg` (arm64) | `macos-14` | mount, copy the app to `/Applications` |
| `.dmg` (Intel) | `macos-15-intel` | the same |
| `.deb` | `ubuntu-22.04` | `apt-get install ./file.deb` |
| AppImage | `ubuntu-22.04` | extracted into a work folder, then run without FUSE |

The logic is in `scripts/smoke/`, not in the YAML. A platform script (`linux.sh`, `macos.sh`, `windows.ps1`) installs the package and checks the files and the menu entry. At the end it removes the package. The shared Python helper `smoke_run.py` does the three starts.

1. **Files and menu entry.** The launcher, the app, the modeler and the `kadakas` tool exist. The license texts are in the package. The menu entry (`.desktop` file, Start menu shortcut, `Info.plist` with name, icon and the `.kad` type) starts the launcher.
2. **First start.** The launcher opens the welcome dialog. The test mode (`KADAKAS_SMOKE_STATE`, see `docs/welcome.md`) writes a state file. It holds the version, the kind of the dialog and the shown values. After a short hold the dialog answers "Use defaults" and ends. The test checks the state file, `welcome.json` and the settings files (`launcher.json`, `prefs.json`, the feedback `settings.json`). A screenshot is taken while the dialog waits.
3. **Second start.** No dialog opens. The launcher starts the default program (the 3D modeler) and writes its run record. The program stays up for 12 seconds.
4. **Third start.** The test sets the flavor to `kadakas-app` in `launcher.json` and starts the launcher with `KADAKAS_AGENT_PORT=0`. The app writes `agent.json`. The test sends `auth` and `describe` over TCP and checks that `describe` names the version of the package.
5. **Uninstall.** The uninstaller removes the programs and the menu entry. For the dmg the app is deleted. For the AppImage the extracted files are deleted.

Details.

- The test uses its own folders (`KADAKAS_LAUNCHER_ROOT`, `KADAKAS_PREFS_DIR`, `KADAKAS_FEEDBACK_DIR`, a private `HOME` and runtime folder). It never touches the settings of the person who runs it.
- Linux runs under `xvfb-run` with Mesa software rendering (lavapipe and llvmpipe). Windows uses the WARP renderer of Windows. macOS uses the virtual GPU of the runner. No variable forces the fallback adapter of `wgpu`. If a runner image needs one, add it to the app first and document it.
- Screenshots and logs are artifacts named `smoke-<system>-<kind>`. A failed screenshot is a warning. It never fails the job. macOS needs the screen recording right for `screencapture`. Without it the screenshot fails and the job goes on.
- A failure message names the step. The logs of the three starts (`first.log`, `second.log`, `third.log`) are in the artifact.
- Both Debian and MSI versions differ from the app version (`0.1.0~beta.1`, `0.1.0-10001`). The scripts get the app version from the workspace `Cargo.toml`.
- The `describe` method of the JSON-RPC API returns `version`. This is what the ping reads.

**Open issue (MSI).** In three rehearsal runs the MSI leg never ended. The runner was cut at the time limit and kept no log. The legs for NSIS, deb, AppImage and both dmg files passed. The MSI install is the main suspect. A hang on a clean runner can be a real defect of the package, so the leg stays strict. The script now writes the msiexec log to the artifacts folder and limits each step to 12 minutes. Run the workflow by hand once more and read `install-msi.log`.

A person still checks some points on a real machine. They stay on the human checklist in `docs/welcome.md` ("Check in a real window"). They are real GPU rendering, the double click and the Dock flow, the Gatekeeper and SmartScreen warnings, high DPI scaling and trackpads.

## Launcher and updates

The launcher (`kadakas-launcher`, PLAT-06) keeps the app up to date without a new installer. The installer puts the launcher next to the app. The launcher starts the app, and it can fetch newer versions of the app and the `kadakas` tool.

### The launcher is the entry point (PLAT-16)

Shortcuts, the Dock or taskbar icon and `.kad` files start `kadakas-launcher` on every system. It is the main binary in `packager.toml` (Windows, Linux) and the main executable of the outer bundle in `packager-macos.toml`.

**macOS has two bundles.** The outer `Kadakas.app` is what is installed. Its executable is the launcher, and it has no window and no Dock icon (`LSUIElement`); its `Info.plist` owns the `.kad` type. The inner `Kadakas Engine.app` (in `Contents/MacOS/`, put there by the packager as an embedded app) is the app: its main executable is `kadakas-app`, and its own `Info.plist` (`packaging/macos/App-Info.plist`) gives the name ("Kadakas"), the icon and the Dock tile. The inner bundle is the base version. An update archive is the same bundle, `Kadakas Engine.app/...` in a `.tar.gz` (`scripts/macos-app.sh`), and the version store keeps one `Kadakas Engine.app` per version folder. The launcher is not in the update archive: it is the outer executable, and only the installer changes it.

- **Why "Kadakas Engine.app".** Copies of the inner bundle live in `versions/<version>/`. With the file name `Kadakas.app` Launch Services could take such a copy for the installed app (`open -a Kadakas`, Keep in Dock). The inner bundle has a different file name and identifier (`net.kadakas.Kadakas.app`), no document types, and the shown name stays "Kadakas". A nested bundle is not registered by `lsregister -f` of the outer one (tried with a demo bundle: "Kadakas Engine" stayed unknown). No `Info.plist` key hides an app from Launch Services. So "Keep in Dock" on the running app (it keeps the inner path) cannot be prevented; the app handles it: started without the launcher (no `KADAKAS_VIA_LAUNCHER`) from inside an `... Engine.app`, it runs `open -b net.kadakas.Kadakas` with its files and ends (`handover.rs`). If `open` fails, it runs alone. The launcher sets the variable for every app it starts, so there is no loop. The `NS...UsageDescription` keys, if ever needed, go in the outer `Info.plist` (macOS asks the program that the person started) and in the inner one.
- **How the app starts.** The launcher runs the inner executable by its path (`Kadakas Engine.app/Contents/MacOS/kadakas-app`), not with `open`: `open` cannot pass the run variables and the arguments in the way the handshake needs. Because the executable is inside a `.app`, the system finds the bundle (name, icon, Dock tile) from the path; `lsappinfo` shows it as its own Foreground app, and the launcher as a UIElement.
- **The archive unpacker.** It keeps executable bits and accepts symbolic links only when they stay inside the folder (a relative target, no `..` that climbs out). It writes nothing through a link, by the names in the archive and also by looking at the folders on disk before each write: APFS treats composed and decomposed Unicode (and upper and lower case) as one name, so a text check alone could be bypassed. `kadakas-launcher add` keeps links inside a local bundle too (frameworks have some) by the same rule.
- **The Apple event.** A double click on a `.kad` file starts the outer bundle (or, when it runs, goes to it) with the open-documents event. The launcher runs the AppKit event loop on the main thread (`objc2`, `appkit.rs`) and the supervisor on a worker thread, and starts the app as soon as the loop is up, with the files of the event. At the end the launcher stops with `NSApplication terminate:` on the main thread. The logic is in `events.rs` and `kadakas_update::inbox` and is tested with a scripted system; `appkit.rs` is only glue.
- **A double click while the app runs.** The launcher writes a request file for the run (`runs/<id>.open-<n>.json`) and brings the app to the front. The app reads it and opens the first file as File > Open does. It does not start a second app. The request files are deleted when the app reads them, before anything opens: the launcher must see at once that the app has the request, and the app then keeps the file in its own state while it asks the person. A crash in between loses the request, never the drawing. The app keeps an **alive stamp** (`runs/<id>.alive`, written every second by a timer thread, not by the window loop). An app with the stamp is slow, not gone (a file dialog blocks the window loop, a big file loads, the window is minimised), so the launcher waits for it as long as it lives; if it ends, the supervisor takes the request back and starts the app again with the file. Only an app without the stamp (an old version that never reads requests) gets the fallback: after 10 seconds the launcher takes the request back and starts another app, and waits for it (no zombie). That second app is not supervised (no run record, no crash mark). When the running app has no run record (the record could not be written), a file waits until it ends, and the launcher logs that.
- **A flavor that starts the main app.** The modeler's "Open in the Kadakas app" does not use an open request: there is no request for "open this file in flavor X", and the requests go to a running app of a run. When a `kadakas-app` run is alive (the alive stamp is fresh and `runs/<id>.program`, which every program writes beside it, names `kadakas-app`), the modeler sends the file to it as an open request (`send_open`) and starts nothing. Otherwise the modeler starts `kadakas-app` from its own folder with the file (`kadakas_update::launch::sibling_app_command`), without a run id and with `KADAKAS_VIA_LAUNCHER=1`. That app is not supervised (no crash mark, no update restart), like the second app of the 10 second fallback. It needs `kadakas-app` next to `kadakas-modeler`, which the installers and the archives give.
- **Unsaved changes.** File > Open, File > New and the request of the launcher ask first with Save and open / Don't save / Cancel when the drawing has unsaved changes. One question is open at a time; a second request does not replace it, the status line names the file that was not opened. A recovered copy (crash recovery) is not offered over unsaved changes; its files stay.
- **Agents never drop the unsaved drawing.** In the app the session has a guard (`Session::set_guard_unsaved`). With unsaved changes `file.open`, `document.load` and `collab.connect` (also with `replace: true`) are refused with code 5, `data.reason: "unsaved_changes"` and a message that tells the agent to ask the person to save. For `file.open` the app shows the person "An agent wants to open X" with Save and open / Don't save / Refuse; the agent's call has been refused already, so Save and open and Don't save open the file as the person's own action. For `document.load` and `collab.connect` the status line tells the person to save and ask the agent again. `version.restore` is not guarded: it is one undoable step that first keeps the drawing as a version. Headless sessions (`kadakas serve`, the library, the tests) keep the guard off: nobody is there to ask, and the document is the agent's own scratch drawing; an agent that wants a safe replace saves first. `Session::has_unsaved_changes` is the one test for "unsaved" (the window, the restart to update and the agent calls use it), and it counts a save by an agent too.
- **Plain builds.** `scripts/install-local-build.sh` makes the bundle on macOS, so a local build is a whole `Kadakas Engine.app` too (`kadakas-launcher add` accepts a folder with it, or the bundle).

With no window, the launcher:

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

The file keeps keys that this version does not know, and every writer changes only its own keys under a lock. The welcome dialog writes `flavor` and `mode` here. The mark that the dialog was seen is in its own file, `welcome.json` (PLAT-26, `docs/welcome.md`), so an older version of the app cannot remove it.

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

The launcher lists `kadakas-app` as a flavor, and `kadakas-modeler` ("Kadakas Modeler", PLAT-11) when that program is in the version folder. No file is needed for these two. On macOS both programs are in `Kadakas Engine.app` (`scripts/macos-app.sh` puts `kadakas-modeler` there, so it is in the update archive too), and the launcher finds them inside the bundle. A version folder may also hold `flavors.json` to list more programs (other UI flavors).

### Not tried for real

The installers were not built here, and no restart ran on a real desktop. Open points: signing and notarizing the two macOS bundles; a release build on Windows uses the GUI subsystem for the app and the launcher, so `kadakas-launcher add` and `--help` show no text there (use `manage`, or the `kadakas` tool for the console).

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

Done on 2026-10-25:

- The key pair was made with `sign-manifest keygen`. The private key is in `~/.config/kadakas-release/update-signing.key` on the maintainer's computer (mode 600, outside the repository). Back it up in a password manager.
- `crates/kadakas-launcher/trusted-keys.txt` holds the public key.
- The GitHub Environment `update-signing` exists. It needs a required reviewer (the maintainer) and it allows deployments only from the branch `main` and the tags `v*`.

To do by hand (the private key must never pass through a chat or a log):

1. Put the key into the Environment secret. This reads the file and sends it to GitHub, and it prints nothing:
   `gh secret set KADAKAS_UPDATE_SIGNING_KEY --env update-signing -R nemecec/kadakas-app < ~/.config/kadakas-release/update-signing.key`
2. Check that the secret is there: `gh secret list --env update-signing -R nemecec/kadakas-app`.
3. Copy the key file to a password manager. Keep the file on disk or delete it, as you prefer. GitHub never shows a secret again.
4. To test the whole path, run the workflow Update manifest by hand with a published release tag. A required reviewer must approve the `sign` job.

If the key file is lost and the secret is still in GitHub, signing still works. If both are lost, make a new key and see Key rotation: launchers that trust only the old key need a new installer.

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

- **macOS update bundle (PLAT-19).** `release.yml` packs `dist-app/Kadakas Engine.app` before signing. For a release the update bundle must be signed inside-out (the programs first, then the bundle) with the hardened runtime, notarized, and the ticket stapled, and only then packed. The launcher does not set `com.apple.quarantine` on a download: the ed25519 signature of the manifest carries the trust, and a quarantine flag would make Gatekeeper check a bundle that the launcher starts by path. Do not add the flag without testing a notarized bundle.
- **Signing job.** `update-manifest.yml` stops with an error when a release lacks the update archive of any platform (before, a missing macOS archive was left out silently). A release from before this change must be built again.

Nothing is signed. Users will see warnings on first start. Do this before a public release.

- macOS: join the Apple Developer Program. Sign the `.app` with a Developer ID Application certificate (`[macos] signing-identity` in `packager-macos.toml`; the packager signs the embedded inner app as a bundle of its own). The update archive then holds the signed inner bundle, so sign it before `tar`, and notarize it too, then notarize it (`notarytool`) and staple the ticket. Keep the certificate and the API key in GitHub secrets.
- Windows: buy a code signing certificate (or use Azure Trusted Signing). Set the signing command for NSIS and WiX in `packager.toml`. Without it, SmartScreen warns.
- Linux: sign the `.deb` repository or publish a checksum file. AppImage signing is optional.
- Updates: the launcher has its own signed manifests (see "Launcher and updates"). The `cargo-packager` updater is not used.
