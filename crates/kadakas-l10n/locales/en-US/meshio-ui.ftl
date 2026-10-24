## meshio-ui: import and export of mesh files (mesh_import.rs in ui-core, io_dialog.rs in the app, interop.rs in the modeler)

meshio-ui-file-filter = 3D meshes: OBJ, STL, glTF, GLB
meshio-ui-all-filter = All 3D files: OBJ, STL, glTF, GLB, STEP, IFC
meshio-ui-unit = Unit of the file
meshio-ui-unit-default = { $unit } (default for this format)
meshio-ui-up-axis = Up axis of the file
meshio-ui-up-default-y = Y up (default for this format)
meshio-ui-up-default-z = Z up (default for this format)
meshio-ui-up-y = Y up
meshio-ui-up-z = Z up
meshio-ui-gltf-fixed = glTF files are in meters with Y up.
meshio-ui-center = Move the model to the origin
meshio-ui-soften-question = Soften edges under an angle?
meshio-ui-soften-angle = Angle (degrees)
meshio-ui-soften-hint = The mesh becomes a surface model, and edges between faces at a smaller angle become soft. Off: the edges of the file stay as they are, and the mesh stays a mesh.
meshio-ui-preview-counting = Counting the edges...
meshio-ui-preview =
    { $softened ->
        [one] { $softened } edge
       *[other] { $softened } edges
    } of { $edges } would be softened ({ $faces } faces).
meshio-ui-preview-too-large = The mesh is too large for a surface model ({ $reason }). It stays a mesh and no edge is softened.
meshio-ui-preview-not-convertible = The mesh cannot become a surface model ({ $reason }). It stays a mesh and no edge is softened.
meshio-ui-preview-skipped = The file is big ({ $megabytes } MB, the edges are counted up to { $limit } MB). The import softens the edges all the same.
meshio-ui-preview-failed = The file cannot be read: { $message }
meshio-ui-imported =
    Imported { $count ->
        [one] { $count } object
       *[other] { $count } objects
    }, { $triangles } triangles, { $softened ->
        [one] { $softened } edge
       *[other] { $softened } edges
    } softened
meshio-ui-ifc-soften-hint = The meshes of the file become surface models where they fit.
meshio-ui-export = Export
meshio-ui-export-ascii = ASCII text (default: binary)
meshio-ui-export-stl-hint = STL holds triangles only, without colors or lines.
meshio-ui-export-ifc-hint = IFC4. Closed surface models become solids, open ones become shells.
meshio-ui-export-dxf-hint = DXF. Surface models are written as polyface meshes.
meshio-ui-export-obj-hint = OBJ with an MTL file, in meters with Y up.
meshio-ui-export-gltf-hint = glTF 2.0, in meters with Y up.

## STEP files (IOP-11)
meshio-ui-step-filter = STEP files: STEP, STP
meshio-ui-unit-from-file = From the file (default for STEP)
meshio-ui-step-hint = STEP faces are cut into triangles on load. A smaller tolerance gives smoother curves and more triangles.
meshio-ui-step-tolerance-auto = Tolerance from the size of the part
meshio-ui-step-tolerance = Tolerance (mm)
meshio-ui-step-per-product = One mesh for each product (off: one for each body)
meshio-ui-step-keep-exact = Keep the exact surfaces next to the triangles
meshio-ui-exact-kept = Exact data kept: { $faces } faces ({ $kinds })
meshio-ui-exact-stale = The triangles were edited after the import. The exact data no longer fits them.
meshio-ui-kind-plane = plane
meshio-ui-kind-cylinder = cylinder
meshio-ui-kind-cone = cone
meshio-ui-kind-sphere = sphere
meshio-ui-kind-torus = torus
meshio-ui-kind-revolution = revolved surface
meshio-ui-kind-extrusion = extruded surface
meshio-ui-kind-spline = spline surface
