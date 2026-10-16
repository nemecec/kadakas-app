## io-dialog: io_dialog.rs

io-dialog-dxf-dwg-ifc-shapefile-geojson = DXF, DWG, IFC, Shapefile, GeoJSON
io-dialog-import = Import
io-dialog-dxf-layers-become-design-layers-default-classes = DXF layers become design layers (default: classes)
io-dialog-unit = Unit
io-dialog-from-file = From file
io-dialog-explode-blocks = Explode blocks
io-dialog-import-hatch-boundaries = Import hatch boundaries
io-dialog-only-points-with-a-z-value = Only points with a Z value
io-dialog-ifc2x3-ifc4-or-ifc4x3 = IFC2x3, IFC4 or IFC4x3
io-dialog-building-storeys-become-design-layers-off-all = Building storeys become design layers (off: all on the active layer)
io-dialog-terrain-as-a-3d-mesh-default-a = Terrain as a 3D mesh (default: a site model)
io-dialog-file-crs-if-the-file-has-none = File CRS (if the file has none)
io-dialog-document-crs-default-utm-zone = Document CRS (default: UTM zone)
io-dialog-class-from-attribute = Class from attribute
io-dialog-keep-the-coordinates-of-the-file-default = Keep the coordinates of the file (default: center the drawing)
io-dialog-design-layer = Design layer
io-dialog-empty-a-new-layer-named-after-the-file = Empty: a new layer named after the file
io-dialog-each-dxf-layer-becomes-its-own-layer = Not used: each DXF layer becomes its own layer
io-dialog-keep-the-source-for-updates = Keep the source (for updates)
io-dialog-cancel = Cancel
io-dialog-imported-objects-skipped =
    Imported { $imported ->
        [one] { $imported } object
       *[other] { $imported } objects
    }, skipped { $skipped }
io-dialog-import-failed = Import failed: { $message }
io-dialog-export = Export
io-dialog-ifc4-ifc4-add2-tc1 = IFC4 (IFC4 ADD2 TC1)
io-dialog-classes-become-ifc-presentation-layers = Classes become IFC presentation layers
io-dialog-design-layers-become-building-storeys-default-everyt = Design layers become building storeys (default: everything in the site)
io-dialog-include-2d-objects-as-annotations = Include 2D objects as annotations
io-dialog-one-dxf-layer-per-design-layer-default = One DXF layer per design layer (default: per class)
io-dialog-only-visible-layers-and-classes = Only visible layers and classes
io-dialog-exported-objects-to =
    Exported { $count ->
        [one] { $count } object
       *[other] { $count } objects
    } to { $path }
io-dialog-export-failed = Export failed: { $message }
io-dialog-dwg-needs-a-converter = DWG (needs a converter, see docs/dwg.md)

# { $entities } is the number of entities of the drawing, { $cap } the most that Kadakas keeps for updates.
io-dialog-base-question = This drawing has { $entities } entities, more than Kadakas keeps for updates ({ $cap }). Keep a link to the original file for updates, or import without update data? A link stores the full path of the file, which can contain your user name, and the people who share this document can see it.
io-dialog-base-question-large = The update data of this drawing ({ $entities } entities) is { $size } MB, more than Kadakas keeps for updates ({ $cap } MB). Keep a link to the original file for updates, or import without update data? A link stores the full path of the file, which can contain your user name, and the people who share this document can see it.
io-dialog-base-question-title = Big drawing
io-dialog-base-keep-link = Keep a link
io-dialog-base-no-update-data = Import without update data
io-dialog-patterns = Unit patterns
io-dialog-patterns-hint = What to write of the paver or tile pattern of hardscapes. Units: every unit as a closed polyline on the layers <layer>-UNITS-WHOLE and <layer>-UNITS-CUT. Gap lines: center lines on <layer>-GAPS.
io-dialog-patterns-none = Outline only
io-dialog-patterns-gaps = Gap lines
io-dialog-patterns-units = Units
