## modeler: the kadakas-modeler program

modeler-window-title = Kadakas Modeler
modeler-title = { $name }{ $changed } - { $app }
modeler-untitled = Untitled
modeler-needs-wgpu = The 3D view needs the GPU renderer (wgpu), and this machine does not have one.

modeler-menu-file = File
modeler-menu-edit = Edit
modeler-menu-camera = Camera
modeler-menu-tools = Tools
modeler-menu-view = View
modeler-menu-help = Help

modeler-file-new = New
modeler-file-open = Open...
modeler-file-save = Save
modeler-file-save-as = Save As...
modeler-file-quit = Quit
modeler-edit-undo = Undo
modeler-edit-redo = Redo
modeler-edit-select-all = Select All
modeler-edit-select-none = Select None
modeler-help-about = About Kadakas Modeler

modeler-camera-previous = Previous
modeler-camera-next = Next
modeler-camera-standard-views = Standard Views
modeler-camera-perspective = Perspective
modeler-camera-parallel = Parallel Projection
modeler-camera-fov = Field of View
modeler-camera-fov-parallel = The parallel projection has no field of view.
modeler-camera-zoom-extents = Zoom Extents
modeler-camera-zoom-window = Zoom Window
modeler-view-top = Top
modeler-view-bottom = Bottom
modeler-view-front = Front
modeler-view-back = Back
modeler-view-left = Left
modeler-view-right = Right
modeler-view-iso = Iso
modeler-view-tray = Tray
modeler-view-theme = Theme
modeler-view-language = Language
modeler-language-system = System language

modeler-tray-entity-info = Entity Info
modeler-tray-materials = Materials
modeler-tray-scenes = Scenes
modeler-entity-nothing = Nothing is selected.
modeler-entity-active-layer = Active layer
modeler-entity-active-class = Active class
modeler-entity-type = Type
modeler-entity-id = Id
modeler-entity-layer = Layer
modeler-entity-class = Class
modeler-entity-size = Size
modeler-entity-corner = Lowest corner
modeler-entity-many =
    { $count ->
        [one] { $count } object selected.
       *[other] { $count } objects selected.
    }
modeler-materials-later = Materials come with the Paint Bucket tool, in a later version.
modeler-scenes-later = Scenes (saved views) come in a later version.

modeler-tool-later = { $name }: { $note }
modeler-nothing-to-undo = There is nothing to undo.
modeler-nothing-to-redo = There is nothing to redo.

modeler-opened = Opened { $name }
modeler-opened-notes = Opened { $name }. { $notes }
modeler-saved = Saved { $name }
modeler-saved-notes = Saved { $name }. { $notes }
modeler-open-error = Cannot open { $path }: { $error }
modeler-open-failed = Cannot open { $path }.
modeler-open-cancelled = The file was not opened.
modeler-open-big-title = This file is big
modeler-save-error = Cannot save { $path }: { $error }
modeler-save-failed = Cannot save { $path }.
modeler-save-cancelled = The file was not saved.
modeler-save-overwrite-title = Replace the file?
modeler-save-overwrite-text = Another program changed { $file } since you opened it. Replace it with this model?
modeler-save-missing-title = Parts of the file are missing
modeler-save-missing-text = The file { $file } no longer has parts that this model refers to: { $what } Save without them?
modeler-discard-title = Unsaved changes
modeler-discard-text = This model has changes that are not saved. Go on and lose them?
modeler-prefs-ignored = The settings file { $path } is ignored: { $error }
modeler-prefs-cannot-write = Cannot write the settings file { $path }: { $error }

modeler-about-title = About Kadakas Modeler
modeler-about-description = A 3D-first modeler with the layout and the keys of familiar modeling programs. It opens and saves the same .kad files as Kadakas.
modeler-about-trademark = SketchUp is a trademark of Trimble Inc. Kadakas Modeler is an independent project and is not affiliated with, endorsed by, or sponsored by Trimble Inc. The name SketchUp-like only says what the keys and the layout are like.
