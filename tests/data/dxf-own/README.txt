These DXF files are written by the Kadakas project for its tests (IOP-15).

They are made by tests/data/make_own_files.py from the DXF specification and the ezdxf
library (MIT licence). They are small, so that the tests that read them run offline in CI.
They are under the licence of this repository (MIT or Apache-2.0).

No file of a CAD program, no file of Vectorworks, AutoCAD or SketchUp, and no file with an
unclear licence is copied here. The public sample files of ezdxf (MIT) and GDAL (MIT style) are
not in the repository. The tests fetch them at run time into tests/data/dxf-public/, which git
ignores. See docs/interop.md.
