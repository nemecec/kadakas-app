## surface-ui: Object Info of surface models, groups of them and component instances (surface_ui.rs)

surface-ui-kind-surface-model = Surface model
surface-ui-kind-component = Component instance
surface-ui-kind-group = Group of surface models
surface-ui-models = Models
surface-ui-edges = Edges
surface-ui-faces = Faces
surface-ui-solid = Solid
surface-ui-solid-empty = No faces
surface-ui-solid-unchecked = Not checked (large model)
surface-ui-solid-closed =
    { $count ->
        [one] Closed solid
       *[other] { $count } closed solids
    }
surface-ui-solid-open =
    { $count ->
        [one] Not closed (1 open edge)
       *[other] Not closed ({ $count } open edges)
    }
surface-ui-volume = Volume
surface-ui-area = Area
surface-ui-size = Size
surface-ui-bounds-min = Lowest corner
surface-ui-bounds-max = Highest corner
surface-ui-hidden = Hidden
surface-ui-hidden-value =
    { $faces ->
        [one] { $faces } face
       *[other] { $faces } faces
    }, { $edges ->
        [one] { $edges } edge
       *[other] { $edges } edges
    }
surface-ui-soft = Soft edges
surface-ui-material = Material
surface-ui-material-value =
    { $count ->
        [one] { $name } (1 face)
       *[other] { $name } ({ $count } faces)
    }
surface-ui-no-material =
    { $count ->
        [one] None (1 face)
       *[other] None ({ $count } faces)
    }
surface-ui-check = Check Solid
surface-ui-edited-in-modeler = The geometry is edited in the 3D modeler.
surface-ui-open-in-modeler = Open in the 3D Modeler
surface-ui-save-first = Save the drawing first. The 3D modeler opens the saved file.
surface-ui-save-changes-first = Save your changes first. The 3D modeler opens the saved file.
surface-ui-opening-modeler = Opening the drawing in the 3D modeler.
surface-ui-sent-to-modeler = The 3D modeler is open. The drawing was sent to it.
surface-ui-modeler-missing = The 3D modeler is not installed.
surface-ui-modeler-failed = The 3D modeler did not start: { $error }
