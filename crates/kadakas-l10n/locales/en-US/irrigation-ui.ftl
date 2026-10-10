## irrigation-ui: irrigation_ui.rs

irrigation-ui-model = Model
irrigation-ui-arc = Arc
irrigation-ui-direction = Direction
irrigation-ui-type = Type
irrigation-ui-material = Material
irrigation-ui-size = Size
irrigation-ui-name = Name
irrigation-ui-static-pressure = Static pressure
irrigation-ui-available-flow = Available flow
irrigation-ui-zone-0-next = Zone (0 = next)
irrigation-ui-flow-factor-kv = Flow factor kv
irrigation-ui-stations = Stations
irrigation-ui-select-an-irrigation-object-and-use-calculate = Select an irrigation object and use Calculate in Object Info.
irrigation-ui-position = Position
irrigation-ui-arc-deg = Arc (deg)
irrigation-ui-direction-deg = Direction (deg)
irrigation-ui-radius = Radius
irrigation-ui-zone = Zone
irrigation-ui-length = Length
irrigation-ui-static-pressure-kpa = Static pressure (kPa)
irrigation-ui-available-flow-l-min = Available flow (l/min)
irrigation-ui-supply-loss-kpa = Supply loss (kPa)
irrigation-ui-meter-loss-kpa = Meter loss (kPa)
irrigation-ui-controller = Controller
irrigation-ui-station-0-none = Station (0 = none)
irrigation-ui-calculated-no-zone-has-water-see-the = Calculated: no zone has water. See the warnings in Object Info.
irrigation-ui-calculated-zone-s-no-warnings =
    Calculated { $z ->
        [one] { $z } zone
       *[other] { $z } zones
    }: no warnings
irrigation-ui-calculated-zone-s-warning-s-see-object =
    Calculated { $z ->
        [one] { $z } zone
       *[other] { $z } zones
    }: { $n ->
        [one] { $n } warning
       *[other] { $n } warnings
    }. See Object Info.
irrigation-ui-irrigation = Irrigation
irrigation-ui-calculate = Calculate
irrigation-ui-flow-pressure-and-zones-of-the-whole = Flow, pressure and zones of the whole network
irrigation-ui-not-calculated-yet = Not calculated yet.
irrigation-ui-out-of-date-calculate-again = Out of date: calculate again.
irrigation-ui-the-last-calculation-stored-the-zone-in =
    The last calculation stored the zone in { $assigned ->
        [one] { $assigned } outlet
       *[other] { $assigned } outlets
    }.
irrigation-ui-critical-outlet-at-margin = Critical outlet: { $label } at { $pressure } (margin { $margin })
irrigation-ui-all-warnings = All warnings ({ $count })
irrigation-ui-none = None
irrigation-ui-catalog-model = Catalog model
irrigation-ui-catalog-pipe = Catalog pipe
irrigation-ui-text = { $material } { $size }
irrigation-ui-zone-flow-pressure = Zone { $zone }: { $flow } l/min, pressure { $pressure } (minimum { $minimum }, margin { $margin })
irrigation-ui-zone-total = Zone { $zone } total: { $flow } l/min of { $available } available, highest velocity { $velocity } m/s
