## agent-activity: agent_activity_ui.rs (AGR-05, AGR-06: what the agent changed, the Activity panel)

agent-activity-title = Agent Activity
agent-activity-intro = The steps of the agent. One call or one script run is one step. Changes that you have not marked as seen are highlighted in the plan and in 3D.
agent-activity-empty = The agent has not changed the drawing yet.
agent-activity-show = Highlight changes
agent-activity-show-hint = Highlight what the agent added, changed and removed, in the plan and in 3D ({ $key })
agent-activity-none = No changes to review.
agent-activity-scope-unseen =
    { $count ->
        [one] Showing the changes of 1 step that you have not seen
       *[other] Showing the changes of { $count } steps that you have not seen
    }
agent-activity-scope-step = Showing the changes of step { $step } only
agent-activity-scope-all = Show all unseen
agent-activity-next-change = Next change
agent-activity-next-change-hint = Zoom to the next change of the agent ({ $key })
agent-activity-prev-change = Previous change
agent-activity-prev-change-hint = Zoom to the previous change of the agent ({ $key })
agent-activity-zoom-all = Zoom to changes
agent-activity-mark-all = Mark all as seen
agent-activity-mark-all-hint = Remove the highlight of the changes that are listed ({ $key })
agent-activity-mark-one = Seen
agent-activity-mark-one-hint = Mark this step as seen: its changes are no longer highlighted
agent-activity-zoom = Zoom
agent-activity-zoom-hint = Zoom to what this step changed
agent-activity-revert = Revert
agent-activity-revert-hint = Take this step back and keep what you and the agent did after it
agent-activity-undo-to = Undo to here
agent-activity-undo-to-hint = Undo this step and every step after it
agent-activity-step = Step { $id }
agent-activity-session = Agent session { $session }
agent-activity-session-version = Version saved before it: #{ $version }
agent-activity-session-no-version = No version was saved before it: { $why }
agent-activity-state-undone = undone
agent-activity-state-gone = not in the undo history
agent-activity-state-seen = seen
agent-activity-state-dropped = details dropped
agent-activity-objects = Objects of this step
agent-activity-objects-more = and { $count } more
agent-activity-status-added = Added
agent-activity-status-changed = Changed
agent-activity-status-removed = Removed
agent-activity-object-row = { $mark } { $kind } { $id }
agent-activity-object-gone = (not in the drawing now)
agent-activity-before = Before
agent-activity-after = After
agent-activity-new-object = A new { $kind } on layer { $layer }.
agent-activity-removed-object = A { $kind } on layer { $layer } was removed.
agent-activity-prop = { $path }: { $old } → { $new }
agent-activity-prop-none = (none)
agent-activity-prop-more = More properties differ.
agent-activity-banner =
    { $count ->
        [one] The agent changed the drawing in 1 step
       *[other] The agent changed the drawing in { $count } steps
    }
agent-activity-review = Review
agent-activity-review-hint = Open the Activity panel ({ $key })
agent-activity-dismiss-banner = Hide
agent-activity-status-line = Step { $id } of the agent: { $summary }
agent-activity-reverted = Reverted step { $id }: { $summary }
agent-activity-reverted-some = Reverted { $done } of { $total } objects of step { $id }. { $kept } stay as they are.
agent-activity-undone-to = Undid { $count } steps back to step { $id }. Redo brings them back.
agent-activity-refused = Step { $id } was not reverted: { $why }
agent-activity-revert-title = Revert step { $id }
agent-activity-revert-partial =
    { $blocked ->
        [one] 1 of { $total } objects was changed or deleted after this step. It stays as it is.
       *[other] { $blocked } of { $total } objects were changed or deleted after this step. They stay as they are.
    }
agent-activity-revert-rest =
    { $free ->
        [one] Revert the other object
       *[other] Revert the other { $free } objects
    }
agent-activity-undo-title = Undo back to step { $id }
agent-activity-undo-body =
    { $count ->
        [one] This takes back 1 step of the undo history.
       *[other] This takes back { $count } steps of the undo history.
    }
agent-activity-undo-others =
    { $other ->
        [one] 1 of them is a step of your own.
       *[other] { $other } of them are steps of your own.
    } Redo brings everything back.
agent-activity-undo-snapshot = This step also changed layers, classes, resources, settings or versions, so it cannot be reverted alone.
agent-activity-undo-go = Undo
agent-activity-cancel = Cancel
agent-activity-why-undone = This step is undone already.
agent-activity-why-gone = This step is no longer in the undo history (the history limit dropped it, or a newer change cleared it).
agent-activity-why-dropped = The log no longer holds what this step changed.
agent-activity-why-snapshot = This step also changed layers, classes, resources, settings or versions. Use Undo to here.
agent-activity-why-conflicts =
    { $count ->
        [one] 1 object was changed or deleted after this step.
       *[other] { $count } objects were changed or deleted after this step.
    }
agent-activity-why-shared = This document is shared. Only the objects of a step can be reverted one by one. Use Undo for your own edits.
agent-activity-why-locked = An object of this step is on a locked layer.
agent-activity-why-group = Something made later holds an object of this step.
agent-activity-why-other = { $why }
agent-activity-sum-none = no visible change
agent-activity-n-objects =
    { $count ->
        [one] 1 object
       *[other] { $count } objects
    }
agent-activity-n-layers =
    { $count ->
        [one] 1 layer
       *[other] { $count } layers
    }
agent-activity-n-classes =
    { $count ->
        [one] 1 class
       *[other] { $count } classes
    }
agent-activity-n-resources =
    { $count ->
        [one] 1 resource
       *[other] { $count } resources
    }
agent-activity-sum-added = added { $what }
agent-activity-sum-changed = changed { $what }
agent-activity-sum-removed = removed { $what }
agent-activity-sum-kinds = { $what } ({ $kinds })
agent-activity-sum-other = { $count } other
agent-activity-sum-layer-named = added layer { $names }
agent-activity-sum-document = changed document settings
agent-activity-sum-versions = changed the versions
agent-activity-sum-issues = changed issues
agent-activity-3d-added = Added { $count }
agent-activity-3d-changed = Changed { $count }
agent-activity-3d-removed = Removed { $count }
