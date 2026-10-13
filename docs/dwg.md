# DWG in Kadakas

Kadakas reads and writes DWG through a converter program that you install yourself.
It has no DWG code of its own. It does not link any DWG library.

## How it works

- Import: Kadakas starts the converter. The converter turns the DWG file into a DXF file in a temporary folder.
  Kadakas reads that DXF file with its normal DXF importer. The temporary folder is deleted afterwards.
- Export: Kadakas writes a DXF file with its normal DXF exporter, then starts the converter to turn it into DWG.
- The converter has a time limit (300 seconds by default). A converter that hangs is stopped.
- All options of the DXF import and export apply (`layer_mode`, `unit`, `origin`, and so on).
  Two options are for DWG only. `converter_timeout_s` sets the time limit. `dwg_version` sets the file version of an export. Only the ODA File Converter uses it, and the default is `ACAD2018`.

An import keeps the source of each object (IOP-22), as for DXF.
The object data `import` holds the DWG file name, a hash of the header, the handle of the entity in the converted DXF, and a hash of the entity.
The objects go to a layer named after the DWG file. The option `track_source: false` turns this off.
The handles come from the converter. Another converter, or another version of it, can number the same drawing in a different way.
The matching of two revisions (IOP-23) then uses the geometry.

Because a DWG file becomes DXF first, anything that DXF cannot hold is lost.
The DXF importer reads 2D geometry, text, blocks and hatches. It skips 3D solids and proxy objects. The report lists them.

## Supported converters

| Converter | Reads DWG | Writes DWG | License |
|---|---|---|---|
| ODA File Converter | yes | yes | Free download from the Open Design Alliance. It has its own license. Read it before you use the program at work. |
| LibreDWG (`dwg2dxf`, `dxf2dwg`) | yes | yes, with `dxf2dwg` (less complete) | GPL-3.0-or-later |

Kadakas starts these programs as separate processes. It never links them, so their licenses do not apply to Kadakas.
Kadakas does not include either program. Get them from their own sites or from your package manager.

### Install hints

- macOS: download the ODA File Converter from opendesign.com (the app goes to `/Applications/ODAFileConverter.app`).
  LibreDWG: `brew install libredwg`.
- Windows: run the ODA File Converter installer. It goes to `C:\Program Files\ODA\...`.
- Linux: install the `.deb` or `.rpm` of the ODA File Converter (it puts `ODAFileConverter` in `/usr/bin`).
  The program needs a display. On a server, start Kadakas with `xvfb-run`.
  LibreDWG: `apt install libredwg-tools` (or the package of your system).

## How Kadakas finds the converter

1. The program named in the environment variable `KADAKAS_DWG_CONVERTER` (the path of `ODAFileConverter` or `dwg2dxf`).
   If you name `dwg2dxf`, `dxf2dwg` is taken from the same folder.
2. `ODAFileConverter` in its standard install folder, or on `PATH`.
3. `dwg2dxf` and `dxf2dwg` on `PATH`.

A request can never name a program. An agent cannot make Kadakas run something else. Agents may start the converter only when the host runs with `--allow-converters` (`kadakas serve`, `kadakas mcp` and the app). The default is off, and the call fails with error code 4 (kind `policy`). The file dialogs of the app are not limited.
`describe` (`io.dwg.converter`) shows which converter Kadakas found.

## Use

```
kadakas import survey.dwg -o survey.kad
kadakas export survey.kad -o out.dwg
```

```json
{"method": "import.file", "params": {"path": "/data/survey.dwg", "options": {"layer_mode": "layers"}}}
{"method": "export.file", "params": {"path": "/data/out.dwg", "format": "dwg", "overwrite": true,
                                      "options": {"dwg_version": "ACAD2013"}}}
```

In the app, File > Import and File > Export accept `.dwg` files.

## Errors

- `no DWG converter was found`: install one of the programs above. As a workaround, save the drawing as DXF in your CAD program.
- `... failed: exit status ...`: the converter refused the file. The message ends with the last lines that the program wrote.
  The usual cause is a DWG version that the converter does not know.
- `... did not finish in N s and was stopped`: raise `converter_timeout_s`.
- `... made no DXF file`: the converter ran but wrote nothing. The file can be damaged.
- `dxf2dwg is not installed`: LibreDWG was found without its writer. Install the full package, or use the ODA File Converter.

## Tests

The tests put stub scripts on `PATH` in place of a real converter (`crates/kadakas-io/src/dwg.rs`, `tests/behavior/test_dwg.py`).
They do not need a real converter.
