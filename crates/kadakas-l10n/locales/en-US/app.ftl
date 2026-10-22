## app: app.rs

app-more-keymap-warnings-see-stderr =
    { $first } (+{ $count ->
        [one] { $count } more keymap warning
       *[other] { $count } more keymap warnings
    }, see stderr)
app-agent-server-listening-on-token-in-the = Agent server listening on { $addr } (token in the agent runtime file) { $key_note }
app-agent-server-failed-on-port = Agent server failed on port { $port }: { $e }
app-web-build-files-open-and-save-through = Web build: files open and save through the browser
app-ready = Ready
app-the-3d-view-needs-the-wgpu-renderer = The 3D view needs the wgpu renderer
app-grouped = Grouped
app-error = Error: { $e }
app-select-a-group-to-ungroup = Select a group to ungroup
app-ungrouped = Ungrouped
app-select-one-group-or-symbol-to-edit = Select one group or symbol to edit
app-editing-group-exit-with-the-exit-group = Editing group. Exit with the Exit group key
app-editing-the-symbol-exit-to-apply-the = Editing the symbol. Exit to apply the changes to all instances
app-you-are-not-in-a-group = You are not in a group
app-symbol-updated = Symbol updated
app-created-site-model = Created site model { $id }
app-created-texture-beds =
    Created { $count ->
        [one] { $count } texture bed
       *[other] { $count } texture beds
    }
app-cannot-start-the-session-for-scripts = Cannot start the session for scripts: { $e }
app-untitled = Untitled
app-kadakas = { $name } - Kadakas
app-downloading = Downloading { $name }
app-opened = Opened { $name }
app-cannot-open = Cannot open { $name }: { $e }
app-opened-2 = Opened { $name }
app-load-notes = The file was opened, but parts of it were repaired or left out: { $notes }
app-saved = Saved { $name }
app-exported = Exported { $name }
app-add-a-sheet-layer-first-navigation-sheet = Add a sheet layer first (Navigation > Sheet Layers > +)
app-exported-sheet-s-to =
    Exported { $count ->
        [one] { $count } sheet
       *[other] { $count } sheets
    } to { $path }
app-error-2 = Error: { $message }
app-restored-view = Restored view '{ $name }'
app-flyover-drag-to-orbit-shift-drag-or = Flyover: drag to orbit, Shift-drag or middle-drag to pan, wheel to zoom
app-select-the-objects-to-duplicate-first = Select the objects to duplicate first
app-is-planned-not-built-yet = { $name } ({ $keys }) is planned, not built yet
app-the-view-history-is-for-the-plan = The view history is for the plan. Switch to Top/Plan
app-no-earlier-view = No earlier view
app-no-later-view = No later view
app-select-the-objects-to-rotate-first = Select the objects to rotate first
app-select-the-objects-to-flip-first = Select the objects to flip first
app-select-two-or-more-shapes-first = Select two or more shapes first
app-done = Done
app-preferences = Preferences
app-export-svg = Export SVG
app-select-all = Select All
app-import = Import
app-export = Export
app-map-sources = Map Sources
app-change-crs = Change CRS
app-scripts = Scripts
app-export-pdf = Export PDF
app-theme = Theme: { $theme }
app-snapping-toggle = Snapping: toggle { $label }
app-show = Show { $name }
app-save-layout-as = Save Layout As
app-reset-layout = Reset Layout
app-no-layout-named = No layout named { $n }
app-name-of-the-layout = Name of the layout
app-save = Save
app-cancel = Cancel
app-saved-layout = Saved layout { $name }
app-objects-selected =
    { $objects ->
        [one] { $objects } object
       *[other] { $objects } objects
    }, { $selected } selected, { $view }, { $projection }
app-status-objects-zoom =
    { $objects ->
        [one] { $objects } object
       *[other] { $objects } objects
    }, { $selected } selected, zoom { $zoom } px/mm, 1:{ $scale }
app-smartcursor-cues-on = SmartCursor cues on
app-smartcursor-cues-off = SmartCursor cues off
app-angles-relative-to-the-prior-segment-on = Angles relative to the prior segment on
app-angles-relative-to-the-prior-segment-off = Angles relative to the prior segment off
app-snap-on = Snap { $name }: on
app-snap-off = Snap { $name }: off
app-select-2d-shapes-to-turn-into-a-solid-first = Select the 2D shapes first
app-select-a-line-for-the-axis-too = Select a line for the axis too
app-no-solid-made-reason = No solid made: { $reason }
app-solids-made =
    { $count ->
        [one] { $count } solid made
       *[other] { $count } solids made
    }
app-solids-made-skipped = Solids made: { $count }, skipped: { $skipped }
app-saved-with-notes = Saved { $name }. { $notes }
app-save-missing-title = Parts of the file cannot be found
app-save-missing-text =
    The file { $file } was changed outside Kadakas, and it no longer has { $what }. Nothing was saved.

    Save without them? The pictures become small gray placeholders and the versions that cannot be found are left out; everything else is saved. Or answer No, put the file (or its .bak copy) back, and save again.
app-save-missing-cancelled = Not saved: parts of the file cannot be found. Put the file (or its .bak copy) back and save again, or save without them.
app-save-missing-done = Not found, and left out of the file: { $what }.
app-save-overwrite-title = The file was changed by another program
app-save-overwrite-text =
    The file { $file } was changed by another program since you opened it (another copy of Kadakas, or a sync program). If you save, that work is replaced. The old file stays as the .bak copy next to it.

    Replace it?
app-save-overwrite-cancelled = Not saved: the file was changed by another program since you opened it. Open it again to see what changed, or save under another name.

app-opened-first-of-several =
    { $count ->
        [one] Opened { $name }. { $count } more file was not opened: Kadakas shows one drawing at a time.
       *[other] Opened { $name }. { $count } more files were not opened: Kadakas shows one drawing at a time.
    }
app-no-guides = There are no guides.
app-guides-deleted = { $count ->
    [one] 1 guide is deleted.
   *[other] { $count } guides are deleted.
}
app-guides-delete-title = Delete Guides
app-guides-delete-question = Delete all { $count } guides of the model?
app-guides-delete-yes = Delete the guides
app-nothing-hidden = Nothing is hidden.
app-unhid = { $count ->
    [one] 1 item shows again.
   *[other] { $count } items show again.
}
app-hidden-objects = { $count ->
    [one] 1 hidden object
   *[other] { $count } hidden objects
}
app-hidden-objects-tip = These objects were hidden in the 3D modeler. Click to show them again.
