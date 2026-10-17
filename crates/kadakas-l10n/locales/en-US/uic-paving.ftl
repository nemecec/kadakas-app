## Texts of the paving logic (kadakas-ui-core): regions from shapes, zones, courses, the pattern check.

uic-paving-make-hardscape = Hardscape
uic-paving-make-landscape-area = Landscape area
uic-paving-make-polyline = Closed polyline

uic-paving-role-area = Area
uic-paving-role-island = Island (a hole of an area)
uic-paving-role-doubtful = Area or island? Check it

uic-paving-note-gaps =
    { $count ->
        [one] { $count } gap was closed (the ends moved { $total } in all)
       *[other] { $count } gaps were closed (the ends moved { $total } in all)
    }
uic-paving-note-gap = A gap of { $size } was closed here
uic-paving-note-open-ends =
    { $count ->
        [one] { $count } shape does not close. The area behind it is not made
       *[other] { $count } shapes do not close. The areas behind them are not made
    }
uic-paving-note-stubs =
    { $count ->
        [one] { $count } line overshoots a corner or stops at a junction (harmless)
       *[other] { $count } lines overshoot a corner or stop at a junction (harmless)
    }
uic-paving-note-dead-end = A dead end of { $length } is open here
uic-paving-note-dead-end-near = A dead end of { $length } is open here. The nearest other end is { $distance } away
uic-paving-note-short =
    { $count ->
        [one] { $count } curve was shorter than the tolerance and was left out
       *[other] { $count } curves were shorter than the tolerance and were left out
    }
uic-paving-note-duplicates =
    { $count ->
        [one] { $count } curve lay on top of another and was merged
       *[other] { $count } curves lay on top of others and were merged
    }
uic-paving-note-slivers =
    { $count ->
        [one] { $count } sliver region was dropped
       *[other] { $count } sliver regions were dropped
    }
uic-paving-note-skipped = An object was not used: { $reason }
uic-paving-note-api = Note: { $text }
uic-paving-no-shapes = Select lines, arcs, polylines or other shapes first.
uic-paving-bad-tolerance = The tolerance must be more than 0 and at most 1000 mm.
uic-paving-no-regions = No region is switched on.

uic-paving-no-object = The object does not exist any more.
uic-paving-not-a-hardscape = Select a hardscape.
uic-paving-no-pattern = The hardscape has no joint pattern. Give it a style with joints first.
uic-paving-no-lines = Draw or pick at least one split line.

uic-paving-along-outline = Outline
uic-paving-along-hole = Hole { $number }
uic-paving-placing-paving = On the paving side
uic-paving-placing-opposite = On the other side
uic-paving-placing-centered = Centered on the edge
uic-paving-orientation-soldier = Soldier (long side across)
uic-paving-orientation-stretcher = Stretcher (long side along)
uic-paving-corner-miter = Mitre
uic-paving-corner-butt = Butt
uic-paving-corner-unit = Corner unit
uic-paving-course-name = Course { $number }
uic-paving-course-not-editable = This course follows a drawn line or a path of its own. Remove it here and make it again with the API, or edit it there.
uic-paving-course-needs-name = A course needs a name.

uic-paving-kind-overlap = Overlap
uic-paving-kind-joint-narrow = Joint too narrow
uic-paving-kind-joint-wide = Joint too wide
uic-paving-kind-small-piece = Small piece
uic-paving-kind-wrong-size = Wrong size
uic-paving-kind-outside = Outside the area
uic-paving-kind-coverage = Coverage
uic-paving-kind-zone-edge-joint = Narrow joint at a zone edge
uic-paving-check-nothing = There is no hardscape with a joint pattern to check.
uic-paving-bad-reply = The answer of the program could not be read.
