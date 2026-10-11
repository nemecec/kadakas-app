## rules-ui: rules_ui.rs

rules-ui-error = Error: { $message }
rules-ui-display-rules = Display rules
rules-ui-northing-easting-grid = Northing/easting grid
rules-ui-show = Show
rules-ui-needs-a-georeference-set-it-with-the = Needs a georeference (set it with the survey point below)
rules-ui-survey-point = Survey point
rules-ui-e = E
rules-ui-n = N
rules-ui-set-from-selected-point = Set from selected point
rules-ui-the-selected-survey-point-plant-or-circle = The selected survey point, plant or circle gets this map position. The drawing does not move.
rules-ui-phases = Phases
rules-ui-set-phase = Set phase
rules-ui-show-phase-display = Show phase display
rules-ui-existing-objects-grey-demolished-objects-red = Existing objects grey, demolished objects red
rules-ui-hide-demolished = Hide demolished
rules-ui-data-visualization = Data visualization
rules-ui-by-value = By value
rules-ui-by-range = By range
rules-ui-clear = Clear
rules-ui-colored-by = Colored by { $field }
rules-ui-object-s-stay-as-they-are =
    { $none ->
        [one] { $none } object stays
       *[other] { $none } objects stay
    } as { $none ->
        [one] it is
       *[other] they are
    }
