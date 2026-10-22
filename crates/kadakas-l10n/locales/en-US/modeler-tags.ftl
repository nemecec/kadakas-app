## modeler-tags: the Tags panel, Hide, Guides and the Tape Measure in the 3D modeler (MOD-05)

modeler-tray-tags = Tags
modeler-tags-untagged = Untagged
modeler-tags-layers = Layers
modeler-tags-new = New
modeler-tags-rename = Rename
modeler-tags-delete = Delete
modeler-tags-show-all = Show all
modeler-tags-eye-hide = Hide this tag
modeler-tags-eye-show = Show this tag
modeler-layers-eye-hide = Hide this layer
modeler-layers-eye-show = Show this layer
modeler-tags-active = Make this the active tag. New geometry goes to it.
modeler-layers-active = Make this the active layer. New geometry goes to it.
modeler-tags-objects = { $count ->
    [one] 1 object
   *[other] { $count } objects
}
modeler-tags-delete-title = Delete tag
modeler-tags-delete-objects = { $count ->
    [one] 1 object has the tag { $name }. It moves to Untagged.
   *[other] { $count } objects have the tag { $name }. They move to Untagged.
}
modeler-tags-delete-others = Also moved to Untagged: { $guides } guides, { $parts } parts of definitions, { $overrides } viewport overrides and annotations, { $views } saved views. { $slots } material slots of the tag become material slots of its objects.
modeler-tags-delete-ask = Delete the tag { $name }?
modeler-tags-delete-yes = Delete the tag
modeler-tags-cancel = Cancel
modeler-tags-deleted = { $count ->
    [one] The tag { $name } is deleted. 1 object moved to Untagged.
   *[other] The tag { $name } is deleted. { $count } objects moved to Untagged.
}
modeler-tags-deleted-empty = The tag { $name } is deleted.
modeler-tags-name-empty = A tag needs a name.
modeler-tags-name-taken = Another tag has this name.
modeler-tags-name-untagged = Untagged is the default tag. It keeps its name and cannot be deleted.
modeler-tags-name-missing = The tag is not in the model any more.
modeler-tags-pick-one = Click a tag in the list first.
modeler-tags-hidden-active = The active tag is hidden. New geometry will not show.
modeler-tags-entity = Tag
modeler-tags-mixed = Mixed
modeler-tags-changed = { $count ->
    [one] 1 object has the tag { $name } now.
   *[other] { $count } objects have the tag { $name } now.
}
modeler-tags-failed = The tag was not changed: { $reason }
modeler-edit-hide = Hide
modeler-edit-unhide = Unhide
modeler-edit-unhide-all = Unhide All
modeler-edit-delete-guides = Delete Guides
modeler-view-guides = Guides
modeler-view-hidden-geometry = Hidden Geometry
modeler-view-hidden-objects = Hidden Objects
modeler-hide-nothing = Select objects, faces or edges first.
modeler-unhide-nothing = Nothing hidden is selected. Show Hidden Geometry or Hidden Objects in the View menu to select hidden items.
modeler-unhide-all-nothing = Nothing is hidden.
modeler-hid = { $count ->
    [one] 1 item is hidden. Unhide All brings it back.
   *[other] { $count } items are hidden. Unhide All brings them back.
}
modeler-unhid = { $count ->
    [one] 1 item shows again.
   *[other] { $count } items show again.
}
modeler-hide-failed = Hide did not work: { $reason }
modeler-guides-none = There are no guides.
modeler-guides-deleted = { $count ->
    [one] 1 guide is deleted.
   *[other] { $count } guides are deleted.
}
modeler-guides-delete-title = Delete Guides
modeler-guides-delete-question = { $count ->
    [one] Delete the guide of the model?
   *[other] Delete all { $count } guides of the model?
}
modeler-guides-delete-yes = Delete the guides
modeler-guides-failed = The guides were not deleted: { $reason }
modeler-tape-distance = Distance: { $value }
modeler-tape-guides = Guides are made. Ctrl measures only.
modeler-tape-measure-only = Measure only. Ctrl makes guides.
modeler-resize-title = Resize the model?
modeler-resize-question = You measured { $measured } and typed { $typed }. This scales { $count } objects by { $factor } about the first point. { $flat } of them are plan objects: they keep their height. Resize the model?
modeler-resize-yes = Yes
modeler-resize-no = No
modeler-resize-done = The model is resized by { $factor }.
modeler-resize-failed = The model was not resized: { $reason }
modeler-resize-nothing = There is nothing to resize.
modeler-tags-parts-hint = Tags belong to objects. Select the object to change its tag. Faces keep the tag of their model.
modeler-resize-locked = The model was not resized. These layers are locked: { $layers }. Unlock them first.
modeler-resize-grey = The model was not resized. { $count } objects are on a tag that is grey. Make the tag normal first.
