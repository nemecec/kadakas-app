## uic-modeler-mod08: Polygon, Offset, Follow Me (modeler.rs, modeling/tools/more.rs, keymap.rs)

uic-modeler-tool-polygon = Polygon
uic-modeler-tool-follow-me = Follow Me
uic-modeler-tool-offset = Offset
uic-modeler-hint-polygon = Click the center, then a corner. Type a radius, or 8s for the number of sides. Ctrl (Option on a Mac) switches between a corner on the radius and a side on the radius.
uic-modeler-hint-offset = Click a face (or select a face, or edges in one plane), move the pointer and click again. Type a distance and press Enter; a negative distance goes the other way.
uic-modeler-hint-follow-me = Select the path (edges) and click the face to sweep. Or click the face, then an edge or a face whose outline is the path.
action-tool-polygon = Polygon tool
action-tool-follow-me = Follow Me tool
uic-modeling-offset-pick = Offset works on a face or on edges in one plane. Click a face, or select one face or a chain of edges first.
uic-modeling-follow-pick-profile = Follow Me sweeps a face. Click the face (the profile).
uic-modeling-follow-pick-path = Now click the path: an edge, or a face whose outline is the path.
uic-modeling-follow-other-model = The path and the face must be in one model.
uic-modeling-offset-one-face = Several faces are selected: click the one to offset, or select one face.
uic-modeling-hole-partial = The face reaches the opposite face but is not fully inside it: a hole through needs the whole face inside one face of the other side, away from its edges and holes.
uic-modeling-hole-stacked = The hole cannot go through: another solid stands under the slab there, back to back with its bottom.
uic-modeling-hole-cap-note = The face was pushed through: it went with its attributes; the new walls keep its paint.
uic-modeling-offset-too-large = The offset does not fit the face: at most about { $most } mm fits.
uic-modeling-offset-no-room = The offset does not fit: there is no room for an offset that way.
uic-modeling-follow-too-wide = The profile is too wide for a turn of the path: the solid would fold. At most about { $most } mm from the path fits.
uic-modeling-follow-along = The profile lies along the path: draw the profile across the path.
uic-modeling-follow-turns-back = The path turns back on itself.
uic-modeling-follow-not-closed = The closed path does not bring the profile back to its start: sweep along an open path instead.
uic-modeling-follow-hole-face = A face fills a hole of the profile: erase the face inside the hole first.
uic-modeling-follow-moved-note = The profile was not at a corner of the closed path: the sweep starts at the nearest corner and the profile went.
