## agent-view: agent_view_ui.rs (AGR-02, AGR-03: what an agent shows in the view)

agent-view-name = Agent
agent-view-back-count = Back ({ $count })
agent-view-back-hint = Return to the view you had before the agent moved it ({ $key })
agent-view-next = Next pointer
agent-view-next-hint = Zoom to the next pointer of the agent ({ $key })
agent-view-pointers = Agent pointers ({ $count })
agent-view-dismiss-mark = ×
agent-view-dismiss = Dismiss
agent-view-dismiss-hint = Remove this pointer
agent-view-clear-all = Clear all
agent-view-zoom-hint = Zoom to this pointer
agent-view-what-objects =
    { $count ->
        [one] 1 object
       *[other] { $count } objects
    }
agent-view-what-box = Area
agent-view-what-place = Place
agent-view-row = { $number }. { $what } ({ $color })
agent-view-row-text = { $number }. { $text } ({ $color })
agent-view-color-agent = slate diamond
agent-view-color-red = red triangle
agent-view-color-blue = blue square
agent-view-color-green = green circle
agent-view-color-orange = orange plus
agent-view-people-pointing =
    { $count ->
        [one] points at 1 pointer
       *[other] points at { $count } pointers
    }
agent-view-people-hint = The agent moves the view and points. You can always go Back or dismiss a pointer.
agent-view-restored = Back to the view before the agent's change.
agent-view-nothing-back = The agent has not changed your view.
agent-view-gone = That pointer is gone.
agent-view-allow-move = Agent may move my view
agent-view-allow-move-hint = Off: the agent can still point and read, but a request to move the view fails and nothing moves.
agent-view-close = Hide
agent-view-close-hint = Hide this panel. It comes back with the next pointer or view change of the agent.
agent-view-showed-view = Agent showed view { $name } (camera only; layers unchanged)
