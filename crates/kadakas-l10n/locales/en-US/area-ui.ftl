## area-ui: area_ui.rs

area-ui-hardscape-style = Hardscape style
area-ui-none-plain-slab = (none: plain slab)
area-ui-slab = Slab
area-ui-path = Path
area-ui-width = Width
area-ui-join = { $join } join
area-ui-landscape-area-style = Landscape area style
area-ui-hardscape = Hardscape
area-ui-style = Style
area-ui-mode = Mode
area-ui-draw-a-path-with-the-hardscape-tool = Draw a path with the Hardscape tool in Path mode
area-ui-slope = Slope
area-ui-edges-set-by-the-api = edges (set by the API)
area-ui-height-above-ground = Height above ground
area-ui-elevation = Elevation
area-ui-drape-on-site-model = Drape on site model
area-ui-site-modifier-acts-as-a-pad = Site modifier (acts as a pad)
area-ui-the-formation-level-under-the-build-up = The formation level under the build-up changes the proposed surface of the site model on this layer
area-ui-plan-area = Plan area
area-ui-draped-area = Draped area
area-ui-surface-area = Surface area
area-ui-perimeter = Perimeter
area-ui-volume = Volume
area-ui-landscape-area = Landscape area
area-ui-show-plants = Show plants
area-ui-hole-s =
    { $count ->
        [one] { $count } hole
       *[other] { $count } holes
    }
area-ui-clear-holes = Clear holes
area-ui-net-area = Net area
area-ui-net-area-holes = { $area }  (holes { $holes })
area-ui-plants-percent-density =
    { $count ->
        [one] { $count } plant
       *[other] { $count } plants
    }  ({ $percent } %, { $density })
area-ui-pattern-origin = Pattern starts at { $x }, { $y }
area-ui-pattern-origin-middle = Pattern starts in the middle of the area
area-ui-clear-origin = Use the middle
area-ui-pattern-angle = Pattern angle
area-ui-set-angle = Set angle
area-ui-clear-angle = Use the style angle
joint-ui-title = Joint pattern of { $name }
joint-ui-bond-running-bond = Running bond
joint-ui-bond-stack = Stack
joint-ui-bond-herringbone = Herringbone
joint-ui-bond-basket-weave = Basket weave
joint-ui-bond-module = Module (own shapes)
joint-ui-unit-length = Unit length
joint-ui-unit-width = Unit width
joint-ui-gap = Joint width
joint-ui-angle = Angle
joint-ui-preset-hexagon = Hexagon
joint-ui-preset-octagon-square = Octagon with square
joint-ui-preset-i-shape = I-shaped paver
joint-ui-from-selection = From selection
joint-ui-side = Side
joint-ui-across-flats = Across flats
joint-ui-notch = Notch
joint-ui-shapes =
    { $count ->
        [one] { $count } unit shape from the drawing
       *[other] { $count } unit shapes from the drawing
    }
joint-ui-save = Save
joint-ui-close = Close

## area-ui: plan_draw.rs (the pattern plan on the canvas)

area-ui-show-pattern-check = Show pattern check on the plan
area-ui-the-check-marks-overlaps-small-pieces-wrong-joints-and-holes = Marks the findings of unit_pattern.check with its default options (not the options of an earlier call): overlaps, small pieces, wrong joints and holes in the coverage. At most 200 of each kind are marked; the label says how many there are
plan-draw-preview = preview: zoom in for the real pattern
plan-draw-preview-too-fine = { $label } (the units are too fine: some joints are left out)
plan-draw-not-drawn = Pattern not drawn: { $reason }
plan-draw-check-failed = Pattern check failed: { $reason }
plan-draw-hidden = pattern hidden at this zoom (units < 3 px)
plan-draw-updating = updating pattern...
plan-draw-laying = laying the pattern...
plan-draw-checking = checking pattern...
plan-draw-check-summary = Pattern check (default options): { $total } findings, { $listed } marked
plan-draw-check-summary-cut = Pattern check (default options): { $total } findings, { $listed } marked (the list of { $kinds } is cut at 200 for each kind)
plan-draw-checking-pauses = checking pattern: the page pauses...
