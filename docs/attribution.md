# Attribution

Kadakas uses the work of many other people. This page lists what we use, the license of each item and where to find the full text. The license texts that we ship are in `THIRD_PARTY_NOTICES.txt` (also in the installers and in Help > About Kadakas > Licenses). Paths on this page are paths in the source repository. The installers carry `THIRD_PARTY_NOTICES.txt` and this page; the icons and fonts are built into the program.

Kadakas itself is licensed under Apache-2.0 OR MIT (`LICENSE-APACHE`, `LICENSE-MIT`).

## Trademarks

Vectorworks is a trademark of Vectorworks, Inc. Kadakas is an independent project and is not affiliated with, endorsed by, or sponsored by Vectorworks, Inc.

AutoCAD, DWG and DXF are trademarks of Autodesk, Inc. We name these products only to describe the file formats that Kadakas reads or writes, or to compare workflows. Kadakas is not affiliated with their owners.

SketchUp is a trademark of Trimble Inc. The feature spec names a SketchUp-like user interface as a possible future flavor. Kadakas has no SketchUp import or export and is not affiliated with Trimble Inc.

## Rust crates

Every Rust crate that we ship is listed in `THIRD_PARTY_NOTICES.txt` with its license and its license text. The file is made by `scripts/notices.sh` (cargo-about). `cargo deny check` allows permissive licenses only (`deny.toml`).

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
| Khronos glTF 2.0 specification | `kadakas-x3d`: glTF and GLB export. https://www.khronos.org/gltf |
| Chaikin corner cutting, Douglas-Peucker line simplification, Sutherland-Hodgman polygon clipping, centripetal Catmull-Rom splines (textbook algorithms) | `kadakas-site` (contours, checks), `kadakas-core` (sheets, curves), `kadakas-x3d` (section cuts) |
| Wavefront OBJ and MTL formats | `kadakas-x3d`: OBJ export |
| OGC WMS 1.1.1 and 1.3.0, WMTS 1.0.0, WFS 2.0 (Open Geospatial Consortium) | `kadakas-gis`: map services. https://www.ogc.org |
| Esri Shapefile technical description, GeoJSON (RFC 7946) | `kadakas-io`: shapefile and GeoJSON import and export |
| Vectorworks 2026 public help pages and shortcut list (app-help.vectorworks.net) | Workflow, terms and default keys follow them. We use public information only and copy no text, icon or graphic. See `docs/keyboard-shortcuts.md`. |

## Tools that we use but do not ship

These tools build, test or check Kadakas. They are not part of the installers.

- pytest (MIT), ezdxf (MIT), ifcopenshell (LGPL-3.0, used only to read IFC files in tests), uv (Apache-2.0 OR MIT)
- cargo-llvm-cov (Apache-2.0 OR MIT), cargo-deny (Apache-2.0 OR MIT), cargo-about (Apache-2.0 OR MIT), cargo-packager (Apache-2.0 OR MIT), trunk (Apache-2.0 OR MIT)
- Optional DWG converters that the user installs: the ODA File Converter (Open Design Alliance, own license) and LibreDWG (GPL-3.0). Kadakas only starts them as separate programs. See `docs/dwg.md`.

## Keep this page current

Every new dependency, asset, dataset or cited formula changes this page. For a crate, run `scripts/notices.sh`. For anything else, edit `docs/notices-extra.txt` too.
