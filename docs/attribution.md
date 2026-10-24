# Attribution

Kadakas uses the work of many other people. This page lists what we use, the license of each item and where to find the full text. The license texts that we ship are in `THIRD_PARTY_NOTICES.txt` (also in the installers and in Help > About Kadakas > Licenses). Paths on this page are paths in the source repository. The installers carry `THIRD_PARTY_NOTICES.txt` and this page; the icons and fonts are built into the program.

Kadakas itself is licensed under Apache-2.0 OR MIT (`LICENSE-APACHE`, `LICENSE-MIT`).

## Trademarks

Vectorworks is a trademark of Vectorworks, Inc. Kadakas is an independent project and is not affiliated with, endorsed by, or sponsored by Vectorworks, Inc.

AutoCAD, DWG and DXF are trademarks of Autodesk, Inc. We name these products only to describe the file formats that Kadakas reads or writes, to compare workflows, or to say what a workspace profile is like (the built-in "AutoCAD-like" profile). Kadakas is not affiliated with their owners. The profile uses general public knowledge of common command aliases. It contains no Autodesk text, icon or file.

SketchUp is a trademark of Trimble Inc. The feature spec plans a 3D-first modeler in the style of SketchUp (section 4.23). Kadakas has no SketchUp import or export and is not affiliated with Trimble Inc.

## Rust crates

Every Rust crate that we ship is listed in `THIRD_PARTY_NOTICES.txt` with its license and its license text. The file is made by `scripts/notices.sh` (cargo-about). `cargo deny check` allows permissive licenses only (`deny.toml`).

## WebAssembly plug-in runtime

Plug-ins (PLAT-10) run in `wasmtime`, the WebAssembly runtime of the Bytecode Alliance (https://github.com/bytecodealliance/wasmtime). Native builds only. License: Apache-2.0 WITH LLVM-exception. We use wasmtime 49 with the features `std`, `runtime` and `cranelift` only (no WASI, no cache, no async). Its dependencies come with it: the Cranelift code generator (`cranelift-*`, `regalloc2`, `target-lexicon`, `pulley-*`, Apache-2.0 WITH LLVM-exception), `wasmparser`, `wasm-encoder` and `wasmprinter` of the `wasm-tools` project (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT), `gimli`, `object`, `addr2line` (MIT OR Apache-2.0), and a few small helpers (`postcard`, `cobs`, `embedded-io`, `memfd`, `mach2`, `anyhow`, `arbitrary`, `fnv`, `leb128fmt`). All are listed in `THIRD_PARTY_NOTICES.txt`; the LLVM exception text is part of the Apache 2.0 text there.

The browser engine (PLAT-10) uses `wasm-encoder` 0.258 (already a dependency of wasmtime) directly, to rewrite a module before it is compiled: it adds a fuel counter and a memory maximum (`kadakas-plugin`, `harden.rs`). It also uses `js-sys`, `web-sys` and `wasm-bindgen` (MIT OR Apache-2.0), which the web build of the app had already, to call the `WebAssembly` API and the `localStorage` of the page, and `base64` (MIT OR Apache-2.0). They are in `THIRD_PARTY_NOTICES.txt`.

The example plug-ins `plugins/raised-planter` and `plugins/stone-row` are our own work (MIT OR Apache-2.0, like the rest of Kadakas). Their compiled modules in `crates/kadakas-plugin/tests/fixtures/` (`raised_planter.wasm`, `stone_row.wasm`) are built from that source with `kadakas-plugin-abi`, `serde`, `serde_json`, `itoa`, `memchr` and `zmij` (MIT OR Apache-2.0) and the Rust standard library (MIT OR Apache-2.0), which are compiled into them. The build script remaps the paths of the build computer, so the modules hold no user name. The adversarial test modules in `crates/kadakas-plugin/tests/fixtures/probes/` are text files (WebAssembly text format) written for the security review of this feature; they are our own work. The web build of the app also uses `wasm-bindgen-futures` (MIT OR Apache-2.0) to compile plug-in modules asynchronously.

The test crate `wat` (and `wast`, `wasm-encoder` 0.261, `wasmparser` 0.261 behind it) turns WebAssembly text into modules in the tests of `kadakas-plugin`. It is a development dependency, so it does not ship, but `cargo deny` checks it: the license is the same (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT).

The example plug-in `plugins/raised-planter` and its module `crates/kadakas-plugin/tests/fixtures/raised_planter.wasm` are own work, under the project license (Apache-2.0 OR MIT).

New with the launcher (PLAT-06, `kadakas-update`): `ed25519-dalek` and `curve25519-dalek` (BSD-3-Clause, by the dalek-cryptography authors) check the signature of update manifests. `sha2` (RustCrypto, MIT OR Apache-2.0) makes the SHA-256 of downloads. `tar` and `flate2` (MIT OR Apache-2.0) unpack the update archives. Their helper crates (`ed25519`, `signature`, `pkcs8`, `spki`, `der`, `const-oid`, `base64ct`, `fiat-crypto`, `digest`, `block-buffer`, `crypto-common`, `generic-array`, `cpufeatures`, `rand_core`, `filetime`) are listed in `THIRD_PARTY_NOTICES.txt` too. `ureq` (HTTP client) was already in use by `kadakas-gis`.

New with the launcher on macOS (PLAT-16): `objc2`, `objc2-app-kit` and `objc2-foundation` (MIT, by the objc2 authors, Mads Marquart and others) let the launcher receive the open-documents Apple event of a double click on a `.kad` file and bring the running app to the front. They were already in the build through `winit`; the launcher now uses them directly, on macOS only. The helper crates (`objc2-encode`, `block2`, `objc2-core-foundation`) are listed in `THIRD_PARTY_NOTICES.txt` too.

New with the collaboration WebSocket (PLAT-09, `kadakas-collab-ws`): `tungstenite` (MIT OR Apache-2.0, the Snapview contributors) speaks the WebSocket protocol; its helpers are `http`, `httparse`, `bytes`, `sha1`, `data-encoding` and `utf-8`. `rustls` (Apache-2.0 OR ISC OR MIT) with the crypto provider `ring` makes the TLS connection, and `webpki-roots` (CDLA-Permissive-2.0, the Mozilla list of root certificates as data) is the list of roots; both were already in use through `ureq`. The optional feature `native-roots` adds `rustls-native-certs` (Apache-2.0 OR ISC OR MIT) and its helpers (`openssl-probe`, `security-framework`, `schannel`). For the browser build: `web-sys`, `js-sys` and `wasm-bindgen` (MIT OR Apache-2.0), already in use by the app. The test crate `rcgen` (MIT OR Apache-2.0) makes a certificate for the TLS test; it is a development dependency and does not ship. The protocol text names the WebSocket sub-protocol convention (RFC 6455) and `Authorization: Bearer` (RFC 6750).

New with feedback (API-47, API-48, `kadakas-feedback`): no new third-party crate. The HTTP sender uses `ureq` (MIT OR Apache-2.0), which `kadakas-gis` and `kadakas-update` already use, and the payload uses `base64`, `serde` and `serde_json` (MIT OR Apache-2.0). The scrubbing rules name the `Bearer` scheme (RFC 6750) and the shapes of common access tokens only as patterns to remove; the code is our own. The test crate `proptest` (MIT OR Apache-2.0) is a development dependency and does not ship. The default feedback address `feedback.kadakas.invalid` uses the reserved top-level name `.invalid` (RFC 2606), which never resolves.

New with the container file (DOC-20 to DOC-22, `kadakas-container`, `kadakas-file`): no new third-party crate. The ZIP reader and writer are our own code over `flate2` (MIT OR Apache-2.0, with its Rust backend `miniz_oxide`, MIT OR Zlib OR Apache-2.0), which `kadakas-update` already uses, and `sha2` (see below). They implement the ZIP file format of PKWARE (the public APPNOTE; stored and deflate entries, no encryption, ZIP64) and DEFLATE (RFC 1951) from the public descriptions.

**Why our own code and not the `zip` crate (evaluated for DOC-20; the user decided: the `zip` crate or our own code, no rc-zip).** Version 8.6.0 (the current exact version, more than two weeks old), `default-features = false` with `deflate-flate2` only, in an empty project. Facts: `cargo deny check advisories bans sources` with our `deny.toml` is clean (no RustSec advisory for `zip` or its tree; the licenses of the tree pass: `zip` is MIT); the tree is `flate2` and `crc32fast` (ours already), `indexmap` (already in our tree), `memchr` and `typed-path` (new); it builds for `wasm32-unknown-unknown`; it reads and writes ZIP64; `raw_copy_file` copies an entry without recompressing, also into a writer that cannot seek (`new_stream`); the seekable writer puts the CRC-32 in the local header of a stored entry. What the checks of the hostile container suite need and the crate does not do: an archive with two entries at the same offset opens, and a read of the second fails only inside the deflate stream ("corrupt deflate stream"), so overlap is not found; a directory that declares 10 bytes for an entry of 100 000 bytes reads all 100 000 bytes (a read does not stop at the declared size), so the size, ratio and total limits would need a wrapper around every read; the local header is not compared with the directory (CRC-32, sizes, the encryption bit); the manifest of sizes and SHA-256 and its comparison with the directory, the SHA-256 check while an entry is copied raw, and the streaming write with the manifest last are ours in any case. We did not run the whole suite on `zip` after these two failures. The decision: keep our code (about 2 000 lines with ZIP64, all reads bounded, every fault an error that names the entry), add ZIP64 (EOCD64 and locator, extra fields, more than 65 534 entries, 64 bit offsets and sizes), and fix the findings of the reviews (disjoint entries, local header checks, stored entries with their CRC-32 in the local header, a checked raw copy). The container is tested against Info-ZIP `unzip`, `bsdtar`, `ditto`, the Java stream reader and Python `zipfile`, so a standard tool reads it. If a later release of `zip` closes the two gaps we can look again; the only new dependencies would be `memchr` and `typed-path`.

## Localization and formats (PLAT-05)

`kadakas-l10n` uses Project Fluent, the localization system of Mozilla (https://projectfluent.org): `fluent-bundle`, `fluent-syntax`, `fluent-langneg` and `intl-memoizer` (from https://github.com/projectfluent/fluent-rs, Apache-2.0 OR MIT), and `unic-langid`, `unic-langid-impl` and `intl_pluralrules` (by Zibi Braniecki, MIT OR Apache-2.0; the plural rules come from the Unicode CLDR). `tinystr` (Unicode-3.0, ICU4X) comes with them. The app reads the system language with `sys-locale` (1Password, MIT OR Apache-2.0; native builds only). All are in `THIRD_PARTY_NOTICES.txt`. The Fluent file format is an open specification. The plural categories of Estonian and English follow the Unicode CLDR plural rules. The texts in `crates/kadakas-l10n/locales/` (English and Estonian) are own work, under the project license (Apache-2.0 OR MIT).

`kadakas-units` has no third-party code except `serde`. The conversion factors are exact definitions: 1 in = 25.4 mm (international yard and pound agreement, 1959), 1 ft = 12 in, 1 yd = 3 ft, 1 ac = 43 560 ft², 1 ha = 10 000 m². The calendar conversion (days and civil dates) follows the public domain algorithms of Howard Hinnant (https://howardhinnant.github.io/date_algorithms.html). The bearing forms (quadrant bearings, degrees-minutes-seconds) are common surveying practice.

## Icons

| Item | License | Source |
|---|---|---|
| Tabler Icons 3.49.0 (`assets/icons/tabler/`) | MIT, copyright 2020-2026 Paweł Kuna | https://tabler.io/icons (license text: `assets/icons/tabler/LICENSE`) |
| Kadakas icons (`assets/icons/kadakas/`) | Own work, under the project license (Apache-2.0 OR MIT) | Drawn from written ideas, see `docs/icons.md` |
| Application icons (`packaging/icons/*`) | Own work, under the project license (Apache-2.0 OR MIT) | Made by `packaging/icons/make_icons.py` |

## Fonts

The app uses the default fonts of egui (the `epaint_default_fonts` crate, "default_fonts" feature):

| Font | License | Link |
|---|---|---|
| Ubuntu-Light | Ubuntu Font Licence 1.0, copyright 2011 Canonical Ltd. | https://design.ubuntu.com/font |
| Hack Regular | MIT, copyright 2018 Source Foundry Authors. Built on DejaVu (public domain) and Bitstream Vera Sans Mono (Bitstream Vera License, copyright 2003 Bitstream, Inc.; full text in `THIRD_PARTY_NOTICES.txt`) | https://sourcefoundry.org/hack/ |
| Noto Emoji | SIL Open Font License 1.1, copyright 2013 Google Inc. | https://github.com/googlefonts/noto-emoji |
| emoji-icon-font | MIT, copyright 2014 John Slegers | https://github.com/jslegers/emoji-icon-font |

## Data

| Item | Terms | Where used |
|---|---|---|
| Coordinate system definitions derived from the EPSG Geodetic Parameter Dataset (https://epsg.org), owned by IOGP | Includes definitions derived from the EPSG Geodetic Parameter Dataset through PostGIS spatial_ref_sys and the `crs-definitions` crate (CC0-1.0). These definitions are not the EPSG Dataset and may differ from it. EPSG Terms of Use: https://epsg.org/terms-of-use.html | `kadakas-crs`: coordinate system lookup by EPSG code |
| GeographicLib geodesic routines (C code in `proj4rs-geodesic`), copyright (c) Charles Karney (2012-2022) | MIT/X11 License; full text in `THIRD_PARTY_NOTICES.txt`. Native builds only (the web build leaves it out). | `kadakas-crs` through `proj4rs`: azimuthal equidistant projection |
| Plant sample catalog (`crates/kadakas-plants/src/sample_catalog.csv`) | Own sample data (botanical names are facts). Prices and sizes are examples only. | Plant catalog default |
| Irrigation catalog (built-in) | Generic parts of our own. They are not data of a manufacturer. | `kadakas-irrigation` |

## Methods, standards and specifications

We implement these from the public descriptions. No code or data is copied unless it is listed above.

| Item | Where used |
|---|---|
| NOAA solar position algorithm (NOAA solar calculator), based on Jean Meeus, "Astronomical Algorithms" | `kadakas-core` `sun.rs`: sun position for shadow studies |
| Hazen-Williams head loss equation and the Christiansen factor for lines with equally spaced outlets | `kadakas-irrigation`: pipe pressure loss |
| British Standard BS 5837:2012, root protection area of a single stem (12 x stem diameter, at most a 15 m radius) | `kadakas-core` `plants/tree.rs`: root protection zone of existing trees |
| buildingSMART IFC (ISO 16739): export is IFC4 only; import reads IFC2x3, IFC4 and IFC4x3. STEP physical file (ISO 10303-21) | `kadakas-ifc`: IFC import and export. https://www.buildingsmart.org |
| Autodesk DXF reference | `kadakas-io`: DXF import and export. DXF is a format of Autodesk, Inc. |
| Khronos glTF 2.0 specification | `kadakas-x3d`: glTF and GLB export. `kadakas-meshio`: glTF and GLB import. https://www.khronos.org/gltf |
| Chaikin corner cutting, Douglas-Peucker line simplification, Sutherland-Hodgman polygon clipping, centripetal Catmull-Rom splines (textbook algorithms) | `kadakas-site` (contours, checks), `kadakas-core` (sheets, curves), `kadakas-x3d` (section cuts) |
| Wavefront OBJ and MTL formats | `kadakas-x3d`: OBJ export. `kadakas-meshio`: OBJ and MTL import |
| STL file format (the public descriptions of the binary and ASCII forms) | `kadakas-meshio`: STL import and export |
| OGC WMS 1.1.1 and 1.3.0, WMTS 1.0.0, WFS 2.0 (Open Geospatial Consortium) | `kadakas-gis`: map services. https://www.ogc.org |
| Esri Shapefile technical description, GeoJSON (RFC 7946) | `kadakas-io`: shapefile and GeoJSON import and export |
| Last-writer-wins register and a central sequencer that orders an operation log (common techniques of replicated data, for example Shapiro et al., "A comprehensive study of Convergent and Commutative Replicated Data Types", 2011). UUID version 7 (RFC 9562) | `kadakas-collab` and `docs/collaboration.md`: merge rules and IDs of the collaboration protocol. No dependency or code is added |
| Fractional indexing for ordered lists, in the form of the `fractional-indexing` library of Rocicorp (CC0 / public domain; the technique is known from the editor of Figma and the write-up of Evan Wallace). We wrote our own code and use no code of the library | `kadakas-collab`: `order.rs` (keys of the stacking order) |
| buildingSMART BCF (BIM Collaboration Format) 2.1 and 3.0, `.bcfzip` (buildingSMART International Ltd.). We wrote our own reader and writer from the public specification and the public XSD files. The XSD files are in `tests/behavior/bcf_schemas/` as unchanged copies for the tests only: they are not part of the program and not shipped. They are under the Creative Commons Attribution-NoDerivatives 4.0 International License (CC BY-ND 4.0, "(c) buildingSMART International Ltd."; text in `tests/behavior/bcf_schemas/LICENSE`). Source: https://github.com/buildingSMART/BCF-XML, the commits `bc48611d0d7a1587f028a2b69677a1aafd5cd0a8` (branch `release_3_0`, folder `Schemas`) and `92fdcf4bfaa8f0576e7562730f341fc7781e2065` (branch `release_2_1`, folder `Schemas`; this commit has no license file, the license of the repository applies). Three official test files of BCF 2.1 from the same repository (`Test Cases/v2.1`: MinimumInformation, OrthogonalCamera and MaximumInformation) are in `tests/behavior/bcf_cases/`, unchanged and under the same license (text in `tests/behavior/bcf_cases/LICENSE`); a behavior test reads them. BCF and IFC are standards of buildingSMART International; Kadakas is not affiliated with it. | `kadakas-review`: `bcf` (REV-06); `tests/behavior/test_issues.py` |
| Okabe and Ito, "Color Universal Design" (2008): a published palette of colors that stay apart for the common kinds of color blindness. Only the color values are used (vermilion, blue, bluish green, grey). | `kadakas-ui-core` `issues`: the colors of the status of an issue |
| ISO 8601 dates and times (xs:dateTime in BCF). The ZIP file format (PKWARE APPNOTE) through the `zip` crate | `kadakas-review`: BCF files |
| SHA-256 (FIPS 180-4, NIST) through the `sha2` crate 0.10.9 (RustCrypto, MIT OR Apache-2.0), the same version that `kadakas-update` uses | `kadakas-collab`: `blob.rs` (hash of pictures), `kadakas-review` (the pictures of BCF snapshots), `kadakas-container` (the manifest of the container, the hash of every entry) and `kadakas-core`: `blob.rs` and `picture.rs` (hash of plant pictures, PLT-16; no new crate). It is in `THIRD_PARTY_NOTICES.txt` already |
| ZIP file format (PKWARE APPNOTE, public) and DEFLATE (RFC 1951), CRC-32 of ZIP | `kadakas-container`: the container of a `.kad` file; ZIP64 is read and written, encryption is not used. Info-ZIP `unzip`, `bsdtar`, `ditto`, the Java stream reader and Python's `zipfile` read the files in the tests |
| Vectorworks 2026 public help pages and shortcut list (app-help.vectorworks.net) | Workflow, terms and default keys follow them. We use public information only and copy no text, icon or graphic. See `docs/keyboard-shortcuts.md`. |

## JavaScript script engine (SCR-08)

Scripts (`script.eval`, generator patterns) run on **quickjs-ng 0.17.0** (https://github.com/quickjs-ng/quickjs), a maintained fork of QuickJS by Fabrice Bellard and Charlie Gordon; the authors of quickjs-ng are Ben Noordhuis, Saúl Ibarra Corretgé and others. License: MIT. The C source is vendored in `crates/kadakas-script-js/quickjs/` (tag `v0.17.0`, hash of the archive in `quickjs/README.md`) and is built to a WebAssembly module by `crates/kadakas-script-js/build.rs`. The module runs inside the plug-in host (`wasmtime` on the desktop, the `WebAssembly` API in the browser, see above).

The module contains the C library of the toolchain: **wasi-libc** of wasi-sdk 34.0 (https://github.com/WebAssembly/wasi-libc, commit 2e6fb9d8ee0c; Apache-2.0 WITH LLVM-exception, Apache-2.0 and MIT, with musl (MIT), cloudlibc (BSD-2-Clause) and dlmalloc (CC0) inside). The module imports nothing from the system. The license texts are in `THIRD_PARTY_NOTICES.txt` (from `docs/notices-extra.txt`).

The toolchain itself is **wasi-sdk 34.0** (https://github.com/WebAssembly/wasi-sdk; clang and LLVM: Apache-2.0 WITH LLVM-exception). It builds Kadakas and is not shipped. The installers get the finished module only. The version and the SHA-256 of each archive are in `crates/kadakas-script-js/wasi-sdk.env`; `scripts/install-wasi-sdk.sh` checks them. Developers may use the Homebrew packages `llvm`, `lld`, `wasi-libc` and `wasi-runtimes` instead (the module may differ byte for byte).

The example generators in `docs/scripts/` (`random-ashlar.js`, `radial-fan.js`) are own work, under the project license (Apache-2.0 OR MIT). The small random generator in `random-ashlar.js` is xorshift32 (G. Marsaglia, "Xorshift RNGs", 2003, public); `Math.random` in a script is sfc32 (Chris Doty-Humphrey, PractRand, public domain).

## New Rust crates for review (REV-06)

`zip` 8.6.0 (MIT; features `deflate-flate2` only, so it uses `flate2` and `typed-path`, `crc32fast`, `indexmap` and `memchr`) reads and writes the BCF archives, and `roxmltree` 0.21.1 (MIT OR Apache-2.0, already used by `kadakas-gis`) reads the XML. Both are in `THIRD_PARTY_NOTICES.txt`. The container format of DOC-20 uses the same `zip` crate.

## Tools that we use but do not ship

These tools build, test or check Kadakas. They are not part of the installers.

- lxml (BSD-3-Clause; `uv run --with lxml` in the behavior test that validates BCF files against the buildingSMART schema files), pytest (MIT), ezdxf (MIT), ifcopenshell (LGPL-3.0, used only to read IFC files in tests), uv (Apache-2.0 OR MIT)
- cargo-llvm-cov (Apache-2.0 OR MIT), cargo-deny (Apache-2.0 OR MIT), cargo-about (Apache-2.0 OR MIT), cargo-packager (Apache-2.0 OR MIT), trunk (Apache-2.0 OR MIT), wasi-sdk (Apache-2.0 WITH LLVM-exception; see the section on the JavaScript script engine)
- Optional DWG converters that the user installs: the ODA File Converter (Open Design Alliance, own license) and LibreDWG (GPL-3.0). Kadakas only starts them as separate programs. See `docs/dwg.md`.

## DXF test data and the AutoCAD color table (IOP-15)

- The table of the 256 AutoCAD colors in `crates/kadakas-io/src/dxf/aci.rs` has the values 1 to 248 of the table of the ezdxf library (https://github.com/mozman/ezdxf, MIT, Copyright (c) 2020 Manfred Moitzi). It is compiled into Kadakas. The MIT notice is in `THIRD_PARTY_NOTICES.txt`.
- The table of hatch patterns in `crates/kadakas-io/src/dxf/hatch/std_patterns.txt` (106 standard patterns, `tests/data/make_std_patterns.py`) has the lines of the metric pattern table of the ezdxf library (MIT, Copyright (c) 2020 Manfred Moitzi). It is compiled into Kadakas. The MIT notice is in `THIRD_PARTY_NOTICES.txt`.
- Some of the DXF import follows the way that ezdxf works: the outline of a WIPEOUT, uniform knots for a spline without knots, the decoding of caret codes, the clip of a block reference (XCLIP) and the twist of a viewport. The code is written for Kadakas. ezdxf is not linked and not shipped.
- The tests fetch public sample files at run time from fixed commits of ezdxf (MIT) and GDAL (https://github.com/OSGeo/gdal, MIT style licence, `LICENSE.TXT` of that repository). They go into `tests/data/dxf-public/`, which git ignores. They are not in the repository and not shipped. Nothing of LibreCAD (GPL) is used.
- `crates/kadakas-io/tests/fixtures/dxf_public_counts.json` is derived from those files with the ezdxf reader (`tests/data/make_public_counts.py`): counts of entities, layouts and viewports, short MTEXT strings, and points on splines. It holds numbers and short strings, no drawing.
- The files in `tests/data/dxf-own/` are our own work (`tests/data/make_own_files.py`), under the project license.

## Asset list (checked by CI)

The table below is the same list as `docs/attribution-assets.tsv`. The script `scripts/check-attribution.sh` fails when a file, a row or a licence id does not fit. Make the table with `scripts/check-attribution.sh --table`. Own work has the author "Kadakas project". Rows with the licence UNKNOWN wait for a decision. Each file of such a row is named in `docs/attribution-open.txt`. The column Use says `ships` for what goes into the installers and `test` for the rest. A path that starts with `crate:` is data inside a Cargo crate. A path that starts with `ext:` is data that is not in the repository.

| Item | Path | Author | Licence | Source | Use |
|---|---|---|---|---|---|
| Tabler Icons | `assets/icons/tabler/**` | Paweł Kuna | MIT | https://tabler.io/icons | ships |
| quickjs-ng | `crates/kadakas-script-js/quickjs/**` | Fabrice Bellard, Charlie Gordon, Ben Noordhuis, Saúl Ibarra Corretgé | MIT | https://github.com/quickjs-ng/quickjs | ships |
| ezdxf | `crates/kadakas-io/src/dxf/aci.rs` | Manfred Moitzi | MIT | https://github.com/mozman/ezdxf | ships |
| ezdxf | `crates/kadakas-io/src/dxf/hatch/std_patterns.txt` | Manfred Moitzi | MIT | https://github.com/mozman/ezdxf | ships |
| Ubuntu-Light | `crate:epaint_default_fonts` | Canonical Ltd. | Ubuntu-font-1.0 | https://design.ubuntu.com/font | ships |
| Hack | `crate:epaint_default_fonts` | Source Foundry Authors (built on DejaVu and Bitstream Vera Sans Mono) | MIT AND Bitstream-Vera | https://sourcefoundry.org/hack/ | ships |
| Noto Emoji | `crate:epaint_default_fonts` | Google Inc. | OFL-1.1 | https://github.com/googlefonts/noto-emoji | ships |
| emoji-icon-font | `crate:epaint_default_fonts` | John Slegers | MIT | https://github.com/jslegers/emoji-icon-font | ships |
| EPSG Geodetic Parameter Dataset | `crate:crs-definitions` | crs-definitions crate (definitions derived from the EPSG dataset of IOGP) | CC0-1.0 AND LicenseRef-EPSG-Terms | https://tangled.org/rwell.org/crs-definitions | ships |
| GeographicLib geodesic routines | `crate:proj4rs-geodesic` | Charles Karney | MIT | https://geographiclib.sourceforge.io/ | ships |
| wasi-libc | `ext:wasi-libc` | WebAssembly community and contributors | (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) | https://github.com/WebAssembly/wasi-libc | ships |
| musl | `ext:wasi-libc-musl` | musl authors (inside wasi-libc) | MIT | https://musl.libc.org/ | ships |
| cloudlibc | `ext:wasi-libc-cloudlibc` | Nuxi (inside wasi-libc) | BSD-2-Clause | https://github.com/NuxiNL/cloudlibc | ships |
| dlmalloc | `ext:wasi-libc-dlmalloc` | Doug Lea (inside wasi-libc) | CC0-1.0 | https://gee.cs.oswego.edu/dl/html/malloc.html | ships |
| emmalloc | `ext:wasi-libc-emmalloc` | Emscripten authors (inside wasi-libc) | MIT | https://github.com/emscripten-core/emscripten | ships |
| musl-fts | `ext:wasi-libc-musl-fts` | Void Linux contributors (inside wasi-libc) | BSD-3-Clause | https://github.com/void-linux/musl-fts | ships |
| compiler-rt | `ext:compiler-rt` | LLVM project | Apache-2.0 WITH LLVM-exception | https://github.com/llvm/llvm-project | ships |
| ezdxf public sample files | `ext:ezdxf-public-dxf` | Manfred Moitzi | MIT | https://github.com/mozman/ezdxf | test |
| GDAL public sample files (MIT-style general terms, verified in LICENSE.TXT at the pinned commit 739fcf03, terms of single files not verified) | `ext:gdal-public-dxf` | OSGeo and contributors | MIT | https://github.com/OSGeo/gdal | test |
| buildingSMART BCF schemas | `tests/behavior/bcf_schemas/**` | buildingSMART International Ltd. | CC-BY-ND-4.0 | https://github.com/buildingSMART/BCF-XML | test |
| buildingSMART BCF 2.1 test files | `tests/behavior/bcf_cases/**` | buildingSMART International Ltd. | CC-BY-ND-4.0 | https://github.com/buildingSMART/BCF-XML | test |
| Kadakas icons | `assets/icons/kadakas/**` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Kadakas modeler icons | `assets/icons/modeler/**` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Kadakas application icons | `packaging/icons/**` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Kadakas macOS bundle files | `packaging/macos/*` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Kadakas web page | `web/index.html` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Script engine prelude | `crates/kadakas-script-js/c/prelude.js` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Kadakas translations | `crates/kadakas-l10n/locales/**` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Kadakas workspace profiles | `crates/kadakas-ui-core/profiles/*` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Plant sample catalog | `crates/kadakas-plants/src/sample_catalog.csv` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Update manifest public keys | `crates/kadakas-launcher/trusted-keys.txt` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Plug-in test modules | `crates/kadakas-plugin/tests/fixtures/**` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | ships |
| Container golden files | `crates/kadakas-api/tests/data/*.kad` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Command schema snapshot | `crates/kadakas-core/tests/command_schema.json` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Public DXF corpus counts and hashes | `crates/kadakas-io/tests/fixtures/dxf_public_*.json` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| IOP-27 DXF facts from the ezdxf reader | `crates/kadakas-io/tests/fixtures/dxf_iop27.json` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Own DXF test files | `tests/data/dxf-own/*` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| DXF test data scripts | `tests/data/*.py` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Hand written IFC test files | `tests/fixtures/ifc/*.ifc` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Messy DXF test corpus | `tests/behavior/fixtures/messy/*` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Parking lot DXF test files | `tests/behavior/fixtures/parking_lot*.dxf` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Parking lot DXF scripts | `tests/behavior/fixtures/make_parking_lot*.py` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| glTF validation script | `tests/behavior/gltf_validate.js` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Example generator scripts | `docs/scripts/*.js` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Icon gallery | `docs/icons.html` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Test picture red_blue.jpg (16x16, made by an agent with Python and sips) | `crates/kadakas-render/testdata/red_blue.jpg` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Test file site.dxf (written by an agent) | `crates/kadakas-io/tests/fixtures/site.dxf` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Test file trees.geojson (written by an agent) | `crates/kadakas-io/tests/fixtures/trees.geojson` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Test files survey_rev1.dxf and survey_rev2.dxf (made by make_survey.py) | `tests/behavior/fixtures/survey_rev?.dxf` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |
| Script make_survey.py (made survey_rev1.dxf and survey_rev2.dxf) | `tests/behavior/fixtures/make_survey.py` | Kadakas project | Apache-2.0 OR MIT | https://github.com/nemecec/kadakas-app | test |

## Keep this page current

Every new dependency, asset, dataset or cited formula changes this page. For a crate, run `scripts/notices.sh`. For anything else, add a row to `docs/attribution-assets.tsv`, run `scripts/check-attribution.sh --table` and edit `docs/notices-extra.txt` too.

## Crash reports (PLAT-20, `kadakas-crash`)

New with crash reports: `crash-handler` (Embark Studios, MIT OR Apache-2.0, https://github.com/EmbarkStudios/crash-handling) catches native crashes (a signal on Linux, a Mach exception on macOS, an exception on Windows) in the app. Its helpers `crash-context` (same authors and license) and `mach2` (macOS, BSD-2-Clause OR MIT OR Apache-2.0), with `cfg-if`, `libc` and `parking_lot` (MIT OR Apache-2.0), come with it. All are in `THIRD_PARTY_NOTICES.txt`. `sha2` (RustCrypto, MIT OR Apache-2.0) makes the signature of a crash; `kadakas-update` used it already. The scrubbing rules, the signature, the queue and the dialog are our own code. We did not add `minidumper` (same authors and license): see `docs/crash-reports.md`, "Limits".
