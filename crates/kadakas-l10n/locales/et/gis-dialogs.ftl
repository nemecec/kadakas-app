## gis-dialogs: gis_dialogs.rs

gis-dialogs-wms-map = WMS-kaart
gis-dialogs-wmts-map = WMTS-kaart
gis-dialogs-wfs-features = WFS-objektid
gis-dialogs-geoimage-file = Georefereeritud pildifail
gis-dialogs-pick-at-least-one-layer-first = Vali kõigepealt vähemalt üks kiht
gis-dialogs-choose-a-picture-file-first = Vali kõigepealt pildifail
gis-dialogs-server = Server
gis-dialogs-error = Viga: { $e }
gis-dialogs-map-sources = Kaardiallikad
gis-dialogs-file = Fail
gis-dialogs-browse = Sirvi...
gis-dialogs-a-png-or-jpeg-needs-a-world = PNG- või JPEG-pilt vajab enda kõrvale maailmafaili (.pgw, .jgw, .wld). GeoTIFF kannab oma asukohta endaga kaasas.
gis-dialogs-server-address = Serveri aadress
gis-dialogs-load-layers = Laadi kihid
gis-dialogs-loading = Laadin...
gis-dialogs-error-2 = Viga: { $message }
gis-dialogs-filter = Filter
gis-dialogs-crs = Koordinaatsüsteem
gis-dialogs-format = Vorming
gis-dialogs-pixels = Pikslid
gis-dialogs-opacity = Läbipaistmatus
gis-dialogs-the-map-covers-the-drawing-plus-a = Kaart katab joonise ja varuala. See muutub vektorobjektide alla jäävaks aluskihiks.
gis-dialogs-the-features-become-vector-objects-their-attributes = Objektid muutuvad vektorobjektideks. Nende atribuudid lähevad objekti andmetesse.
gis-dialogs-add-to-drawing = Lisa joonisele
gis-dialogs-fetching = Toon...
gis-dialogs-agents-and-the-cache = Agendid ja vahemälu
gis-dialogs-addresses-that-agents-may-fetch-one-per = Aadressid, mida agendid võivad tuua (üks rea kohta: host, *.domeen või aadressi algus). Tühi: ükski.
gis-dialogs-tile-cache-files-mb = Kaardiplaatide vahemälu: { $files } faili, { $megabytes } MB
gis-dialogs-clear = Tühjenda
gis-dialogs-change-crs = Muuda koordinaatsüsteemi
gis-dialogs-the-document-has-no-crs-yet-give = Dokumendil pole veel koordinaatsüsteemi. Alustamiseks anna see:
gis-dialogs-current-crs = Praegune koordinaatsüsteem: { $current }
gis-dialogs-new-crs = Uus koordinaatsüsteem
gis-dialogs-preview = Eelvaade
gis-dialogs-set-the-crs-below-to-begin-no = Alustamiseks sea allpool koordinaatsüsteem (objekte ei teisendata).
gis-dialogs-set-crs = Sea koordinaatsüsteem
gis-dialogs-convert = Teisenda
gis-dialogs-crs-is-now = Koordinaatsüsteem on nüüd { $crs }
gis-dialogs-not-converted-object = Ei teisendatud: objekt { $id } ({ $kind }): { $reason }
gis-dialogs-error-3 = Viga: { $m }
gis-dialogs-map-added-tiles-from-the-cache-crs =
    Kaart lisatud: { $tiles ->
        [one] { $tiles } plaat
       *[other] { $tiles } plaati
    } ({ $cached } vahemälust), koordinaatsüsteem { $crs }
gis-dialogs-objects-imported =
    { $count ->
        [one] Imporditi { $count } objekt
       *[other] Imporditi { $count } objekti
    }
gis-dialogs-picture-added-x-px = Pilt lisatud ({ $width } x { $height } px)
gis-dialogs-done = Valmis
gis-dialogs-automatic = automaatne
gis-dialogs-objects-change =
    { $count ->
        [one] { $count } objekt muutub.
       *[other] { $count } objekti muutub.
    } Mõõtkavategur { $factor }. Pööre { $new }° (oli { $old }°).
gis-dialogs-new-origin = Uus nullpunkt: E { $east }, N { $north }. Suurim nihe { $shift }.
gis-dialogs-pick-a-layer-first = Vali kõigepealt kiht
gis-dialogs-pick-a-feature-type-first = Vali kõigepealt objektitüüp
gis-dialogs-loaded-feature-types =
    { $title }: { $count ->
        [one] { $count } objektitüüp
       *[other] { $count } objektitüüpi
    }
gis-dialogs-loaded-layers =
    { $title }: { $count ->
        [one] { $count } kiht
       *[other] { $count } kihti
    }
gis-dialogs-pictures = Pildid
