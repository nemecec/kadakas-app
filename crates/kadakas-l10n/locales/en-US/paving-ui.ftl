## paving_ui: Create Objects from Shapes, Split Hardscape into Zones, the pattern check, and the unit pattern in Object Info

paving-ui-error = Error: { $e }
paving-ui-done = Done.
paving-ui-select-a-hardscape = Select a hardscape first.
paving-ui-click-a-point = Click a point in the drawing. Escape stops.
paving-ui-click-an-edge = Click an edge. Escape stops.
paving-ui-no-edge-there = There is no edge there. Click nearer to an edge.
paving-ui-no-such-zone = The zone does not exist any more.
paving-ui-cancel = Cancel
paving-ui-show = Show
paving-ui-remove = Remove
paving-ui-edit = Edit
paving-ui-apply = Apply
paving-ui-back = Back
paving-ui-name = Name
paving-ui-none = (none)
paving-ui-unit = Unit

## Create Objects from Shapes
paving-ui-create-objects-from-shapes = Create Objects from Shapes
paving-ui-shapes-selected =
    { $count ->
        [one] { $count } shape is selected
       *[other] { $count } shapes are selected
    }
paving-ui-close-gaps-up-to = Close gaps up to
paving-ui-the-widest-gap-between-ends-that-is-closed = The widest gap between two ends that is closed. A wider gap stays open and no area is made behind it.
paving-ui-make = Make
paving-ui-delete-the-shapes-that-are-used-up = Delete the shapes that are used up
paving-ui-a-shape-goes-only-when-all-its-pieces-are-in-regions-that-are-made = A shape is deleted only when all its pieces are in regions that are made.
paving-ui-regions-found =
    { $count ->
        [one] { $count } region found
       *[other] { $count } regions found
    }
paving-ui-region-row =
    { $holes ->
        [0] Region { $number }: { $role }, { $area }
       *[other] Region { $number }: { $role }, { $area }, { $holes } holes
    }
paving-ui-no-closed-regions = The shapes enclose no region. Look at the report: a gap may be wider than the tolerance.
paving-ui-report = Report
paving-ui-click-to-show-the-place = Click to show the place.
paving-ui-create = Create
paving-ui-made-objects =
    { $kept ->
        [0] Made { $count } objects.
       *[other] Made { $count } objects. { $kept } shapes stay, because part of them is not in a region that was made.
    }

## Split Hardscape into Zones
paving-ui-split-hardscape-into-zones = Split Hardscape into Zones
paving-ui-draw-split-lines-help = Draw lines over the hardscape, or pick lines that are drawn already. A double click or Enter ends a line. Escape stops drawing.
paving-ui-draw-a-split-line = Draw a split line
paving-ui-stop-drawing = Stop drawing
paving-ui-finish-line = Finish line
paving-ui-use-the-selected-lines = Use the selected lines
paving-ui-lines-arcs-and-polylines-that-are-selected-become-split-lines = Lines, arcs and polylines that are selected become split lines.
paving-ui-lines-added =
    { $count ->
        [one] { $count } line added
       *[other] { $count } lines added
    }
paving-ui-split-line-object = Line { $number } (a drawn line)
paving-ui-split-line-drawn = Line { $number } (drawn here)
paving-ui-no-split-lines-yet = No split lines yet.
paving-ui-zones-follow-the-lines-that-were-picked = Zones follow the lines that were picked
paving-ui-when-such-a-line-moves-the-zones-move-with-it = When such a line moves, the zones move with it.
paving-ui-line-ends-join-an-edge-within = A line end joins an edge within
paving-ui-units-at-a-zone-edge = Units at a zone edge
paving-ui-zone-units-cut = Cut at the zone edge
paving-ui-zone-units-centroid = Whole, by their center
paving-ui-zones-made =
    { $count ->
        [one] { $count } zone
       *[other] { $count } zones
    }
paving-ui-zone-row = { $name }: { $area }
paving-ui-zone-edges-cut = The zone edges cut { $lost } whole units and add { $extra } pieces.
paving-ui-zone-edges-have-no-joint = Two zones meet with no joint at the zone edge. Put the edge in a joint of the pattern, or let each unit go whole to the zone of its center.
paving-ui-split = Split
paving-ui-split-into-zones =
    { $count ->
        [one] The hardscape has { $count } zone.
       *[other] The hardscape is split into { $count } zones.
    }

## The pattern check
paving-ui-check-unit-patterns = Check Unit Patterns
paving-ui-run-the-check = Run the check
paving-ui-the-check-looks-at-the-selected-hardscapes-or-at-all = The check looks at the selected hardscapes, or at all of them when none is selected. Click a finding to show it.
paving-ui-no-result-yet = No result yet.
paving-ui-the-drawing-changed-run-the-check-again = The drawing changed since the check. Run it again.
paving-ui-no-findings =
    { $objects ->
        [one] No findings in { $objects } hardscape ({ $units } units).
       *[other] No findings in { $objects } hardscapes ({ $units } units).
    }
paving-ui-findings =
    { $count ->
        [one] { $count } finding in { $objects } hardscape(s) ({ $units } units)
       *[other] { $count } findings in { $objects } hardscape(s) ({ $units } units)
    }
paving-ui-could-not-be-checked =
    { $count ->
        [one] { $count } hardscape could not be checked.
       *[other] { $count } hardscapes could not be checked.
    }
paving-ui-all-kinds = All
paving-ui-kind-count = { $kind } ({ $count })
paving-ui-object-error = { $name }: { $e }
paving-ui-object-findings = { $name }: { $count }
paving-ui-finding-row = { $kind }: { $size } in { $group }
paving-ui-the-list-is-cut-the-counts-are-exact = The list is cut. The counts are exact.

## Object Info
paving-ui-unit-pattern = Unit pattern
paving-ui-pattern-bond = { $bond }: { $length } x { $width }, joint { $gap }
paving-ui-pattern-module = { $bond }, joint { $gap }
paving-ui-edit-pattern = Edit pattern...
paving-ui-the-pattern-belongs-to-the-style-it-changes-every-hardscape-with-it = The pattern belongs to the style. A change reaches every hardscape with that style.
paving-ui-pick-origin = Pick origin
paving-ui-click-a-point-in-the-drawing-where-the-pattern-starts = Click a point in the drawing where the pattern starts.
paving-ui-align-to-edge = Align to edge
paving-ui-click-an-edge-of-the-outline-the-pattern-starts-there-and-runs-along-it = Click an edge of the outline. The pattern starts there and runs along it.
paving-ui-count-the-units = Count the units
paving-ui-count-the-units-again = Count the units again
paving-ui-a-big-area-takes-time-to-count-so-it-is-counted-when-you-ask = A big area takes time to count, so it is counted when you ask.
paving-ui-the-counts-are-of-an-earlier-state = The counts are of an earlier state of the drawing.
paving-ui-whole-units = Whole units
paving-ui-cut-units = Cut units
paving-ui-cut-units-detail = { $count } ({ $slivers } slivers, { $unfillable } too small to lay)
paving-ui-order = Order (units)
paving-ui-order-range = { $min } to { $max }
paving-ui-laid-area = Laid area
paving-ui-whole-and-cut = { $whole } whole, { $cut } cut
paving-ui-zones-count =
    { $count ->
        [one] { $count } zone
       *[other] { $count } zones
    }
paving-ui-no-zones = No zones
paving-ui-split-into-zones-button = Split into zones...
paving-ui-remove-the-split-lines = Remove the split lines
paving-ui-zone-pattern = Pattern
paving-ui-the-pattern-of-the-field = The pattern of the field
paving-ui-a-pattern-of-its-own = A pattern of its own
paving-ui-use-the-field-s = Use the field's
paving-ui-as-the-field = { $angle } (as the field)
paving-ui-origin-as-the-field = The pattern starts as the field's does
paving-ui-aligned-to-edge = Aligned to edge { $number }
paving-ui-clear-alignment = Clear alignment
paving-ui-not-aligned-to-an-edge = Not aligned to an edge
paving-ui-align-to-split-line = Align to split line
paving-ui-split-line-number = Line { $number }
paving-ui-courses-count =
    { $count ->
        [one] { $count } course
       *[other] { $count } courses
    }
paving-ui-add-a-course = Add a course...
paving-ui-a-row-of-units-along-an-edge-for-example-a-soldier-course-along-a-kerb = A row of units along an edge, for example a soldier course along a kerb.
paving-ui-course-row = { $name }: { $units } units, { $length }
paving-ui-course-joints = , joints { $narrowest } to { $widest }
paving-ui-course-joints-range = Joints from { $narrowest } to { $widest }
paving-ui-course-preview = { $units } units, { $corners } corners, { $length } long
paving-ui-course-saved = Course { $name } saved.
paving-ui-along = Along
paving-ui-lies = Lies
paving-ui-size-along-the-row = Along the row
paving-ui-size-across-the-row = Across the row
paving-ui-offset = Offset
paving-ui-corner = Corner
paving-ui-start-the-field-at-the-course = Start the field at the course
paving-ui-moves-the-origin-and-angle-of-the-pattern-so-that-its-first-row-is-whole = Moves the origin and the angle of the pattern so that its first row is whole.
paving-ui-counting = Counting...
paving-ui-checking = Checking...
paving-ui-the-drawing-changed-the-pick-ended = The drawing changed, so the pick ended. Start it again.
paving-ui-remove-the-split-lines-and-the-zone-settings = Remove the split lines? The settings of the zones (pattern, start, angle) go with them. You can undo this.
