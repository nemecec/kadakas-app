## modeler-scenes: the Scenes and Styles panels of the 3D modeler (MOD-10)

modeler-tray-styles = Styles
modeler-scenes-add = Add
modeler-scenes-add-tip = Add a scene with the present view, tags, layers and style
modeler-scenes-update = Update
modeler-scenes-update-tip = Store the present view, tags, layers and style in the scene
modeler-scenes-rename = Rename
modeler-scenes-delete = Delete
modeler-scenes-up = Move the scene up
modeler-scenes-down = Move the scene down
modeler-scenes-none = No scenes yet. Add stores the present view as a scene.
modeler-scenes-animate = Animate the change of view
modeler-scenes-changed-tip = The view, the tags or the style changed since the scene was stored. Update stores them.
modeler-scenes-hidden = { $count ->
    [one] 1 hidden
   *[other] { $count } hidden
}
modeler-scenes-name-empty = A scene needs a name.
modeler-scenes-name-taken = Another scene has this name.
modeler-scenes-missing = There is no such scene.
modeler-scenes-bad-camera = The view has a value that cannot be stored: it is not a number or it is out of range.
modeler-scenes-added = Scene { $name } added.
modeler-scenes-updated = Scene { $name } updated.
modeler-scenes-renamed = Scene { $old } is now { $name }.
modeler-scenes-deleted = Scene { $name } deleted.
modeler-scenes-moved = Scene { $name } is now number { $place }.
modeler-scenes-shown = Scene { $name }.
modeler-scenes-failed = The scene command was refused. Nothing changed.
modeler-scenes-view-only = Scene { $name } has no 3D view of its own. The tags, layers and style are applied.
modeler-style-label = Style
modeler-style-wireframe = Wireframe
modeler-style-shaded = Shaded
modeler-style-realistic = Realistic
modeler-style-hidden-line = Hidden Line
modeler-style-dashed-hidden-line = Dashed Hidden Line
modeler-style-changed = Style: { $name }.
modeler-style-failed = The style could not be changed. Nothing changed.
modeler-scenes-style-missing = style { $name } is missing
modeler-view-animate-scenes = Animate Scene Changes
