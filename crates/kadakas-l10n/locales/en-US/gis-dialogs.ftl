## gis-dialogs: gis_dialogs.rs

gis-dialogs-wms-map = WMS map
gis-dialogs-wmts-map = WMTS map
gis-dialogs-wfs-features = WFS features
gis-dialogs-geoimage-file = Geoimage file
gis-dialogs-pick-at-least-one-layer-first = Pick at least one layer first
gis-dialogs-choose-a-picture-file-first = Choose a picture file first
gis-dialogs-server = Server
gis-dialogs-error = Error: { $e }
gis-dialogs-map-sources = Map Sources
gis-dialogs-file = File
gis-dialogs-browse = Browse...
gis-dialogs-a-png-or-jpeg-needs-a-world = A PNG or JPEG needs a world file next to it (.pgw, .jgw, .wld). A GeoTIFF carries its place.
gis-dialogs-server-address = Server address
gis-dialogs-load-layers = Load layers
gis-dialogs-loading = Loading...
gis-dialogs-error-2 = Error: { $message }
gis-dialogs-filter = Filter
gis-dialogs-crs = CRS
gis-dialogs-format = Format
gis-dialogs-pixels = Pixels
gis-dialogs-opacity = Opacity
gis-dialogs-the-map-covers-the-drawing-plus-a = The map covers the drawing plus a margin. It becomes an underlay below the vector objects.
gis-dialogs-the-features-become-vector-objects-their-attributes = The features become vector objects. Their attributes go into the object data.
gis-dialogs-add-to-drawing = Add to drawing
gis-dialogs-fetching = Fetching...
gis-dialogs-agents-and-the-cache = Agents and the cache
gis-dialogs-addresses-that-agents-may-fetch-one-per = Addresses that agents may fetch (one per line: host, *.domain or address prefix). Empty: none.
gis-dialogs-tile-cache-files-mb = Tile cache: { $files } files, { $megabytes } MB
gis-dialogs-clear = Clear
gis-dialogs-change-crs = Change CRS
gis-dialogs-the-document-has-no-crs-yet-give = The document has no CRS yet. Give one to start:
gis-dialogs-current-crs = Current CRS: { $current }
gis-dialogs-new-crs = New CRS
gis-dialogs-preview = Preview
gis-dialogs-set-the-crs-below-to-begin-no = Set the CRS below to begin (no objects are converted).
gis-dialogs-set-crs = Set CRS
gis-dialogs-convert = Convert
gis-dialogs-crs-is-now = CRS is now { $crs }
gis-dialogs-not-converted-object = Not converted: object { $id } ({ $kind }): { $reason }
gis-dialogs-error-3 = Error: { $m }
gis-dialogs-map-added-tiles-from-the-cache-crs =
    Map added: { $tiles ->
        [one] { $tiles } tile
       *[other] { $tiles } tiles
    } ({ $cached } from the cache), CRS { $crs }
gis-dialogs-objects-imported =
    { $count ->
        [one] { $count } object imported
       *[other] { $count } objects imported
    }
gis-dialogs-picture-added-x-px = Picture added ({ $width } x { $height } px)
gis-dialogs-done = Done
gis-dialogs-automatic = automatic
gis-dialogs-objects-change =
    { $count ->
        [one] { $count } object changes.
       *[other] { $count } objects change.
    } Scale factor { $factor }. Rotation { $new }° (was { $old }°).
gis-dialogs-new-origin = New origin: E { $east }, N { $north }. Largest shift { $shift }.
gis-dialogs-pick-a-layer-first = Pick a layer first
gis-dialogs-pick-a-feature-type-first = Pick a feature type first
gis-dialogs-loaded-feature-types =
    { $title }: { $count ->
        [one] { $count } feature type
       *[other] { $count } feature types
    }
gis-dialogs-loaded-layers =
    { $title }: { $count ->
        [one] { $count } layer
       *[other] { $count } layers
    }
gis-dialogs-pictures = Pictures
