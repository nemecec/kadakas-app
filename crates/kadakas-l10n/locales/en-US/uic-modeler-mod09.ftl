## uic-modeler-mod09: solid tools in the modeler (modeling/solid.rs, modeling/messages.rs)

uic-solid-op-union = Union
uic-solid-op-subtract = Subtract
uic-solid-op-intersect = Intersect
uic-solid-op-trim = Trim
uic-solid-op-split = Split
uic-solid-need-selection = Select the faces of two solids first: click a face of each solid, with Shift for the second.
uic-solid-one-model = The two solids must be in one model.
uic-solid-need-two = The solid tools work on exactly two solids. The selected faces belong to { $found ->
        [0] no solid
        [one] one solid
       *[other] { $found } solids
    }.
uic-solid-done-union = Union: the two solids make one solid of { $volume } m³.
uic-solid-done-subtract = Subtract: the second solid is taken out of the first. The result is { $volume } m³.
uic-solid-done-intersect = Intersect: only the common part stays, { $volume } m³.
uic-solid-done-trim = Trim: the first solid is cut by the second. The second stays, split along the cut and stuck to the first. The first is now { $volume } m³.
uic-solid-done-split = Split: three solids of { $first } m³ (first less second), { $common } m³ (common) and { $second } m³ (second less first).
uic-solid-refused-open = The { $which ->
        [first] first
       *[second] second
    } solid is not closed, so nothing was changed. Use Check Solid to select the open edges.
uic-solid-refused-crossing = The { $which ->
        [first] first
       *[second] second
    } solid passes through itself, so nothing was changed. Use Intersect Faces first.
uic-solid-refused-inward = The faces of the { $which } solid look inward, so nothing was changed. Use Reverse Faces on it first.
uic-solid-refused-flat = The { $which ->
        [first] first
       *[second] second
    } solid has no volume, so nothing was changed.
uic-solid-refused-empty = The { $which ->
        [first] first
       *[second] second
    } solid has no faces, so nothing was changed.
uic-solid-refused-attached = The { $which ->
        [first] first
       *[second] second
    } solid shares edges or vertices with other geometry, so nothing was changed. Make a group of it first.
uic-solid-refused-empty-result = The result would be empty because the solids have no common volume, so nothing was changed.
uic-solid-refused-cut = The solids could not be cut cleanly here, for example because a crossing is smaller than the tolerance. Nothing was changed. Move one solid a little.
uic-solid-refused-result = The result is not a closed solid, so nothing was changed. The cut left an open edge.
uic-solid-refused-volume = The volume of the result does not fit the volumes of the two solids, so it was not used and nothing was changed.
uic-solid-refused-invalid = The result would give the model a problem that it did not have, so nothing was changed.
uic-solid-refused-limit = The result would pass a limit of the model (faces, edges or vertices), so nothing was changed.
uic-solid-check-nothing = Select a surface model or some of its faces first. Nothing to check.
uic-solid-check-closed = Closed: { $solids -> 
        [one] one solid
       *[other] { $solids } solids
    } with a volume of { $volume } m³ in all.
uic-solid-check-open = Not closed: { $edges ->
        [one] one edge is
       *[other] { $edges } edges are
    } open and now selected. { $solids ->
        [one] One solid is
       *[other] { $solids } solids are
    } closed.
uic-solid-check-odd = Not all closed: { $count ->
        [one] one shell looks
       *[other] { $count } shells look
    } inward, { $count ->
        [one] has
       *[other] have
    } no volume or { $count ->
        [one] passes
       *[other] pass
    } through themselves. { $solids ->
        [one] One solid is
       *[other] { $solids } solids are
    } closed.
uic-solid-refused-near-touch = A vertex of one solid is { $distance } mm off a face of the other solid. That is too far to count as on the face and too close to cut it. { $count ->
        [one] One vertex can be snapped onto the face.
       *[other] { $count } vertices can be snapped onto the faces.
    } Nothing was changed.
uic-solid-refused-snap-shared = The snap was refused because a vertex is shared with geometry outside the two solids. Nothing was changed. Make a group of the solids first.
uic-solid-refused-snap-degenerate = The snap was refused because it would make a face degenerate. Nothing was changed.
uic-solid-refused-snap-move = The snap was refused because the model does not allow the move. Nothing was changed.
uic-solid-refused-snap-open = The snap was refused because it would leave a solid not closed. Nothing was changed.
uic-solid-refused-snap-new = The snap was refused because a vertex would stay near the plane of another face. Nothing was changed.
uic-solid-refused-snap-still = The operation is still refused after the snap, so nothing was changed.
uic-solid-snapped = Snapped { $count ->
        [one] one vertex
       *[other] { $count } vertices
    } by { $distance } mm at most. { $done }
