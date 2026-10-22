## app: app.rs

app-more-keymap-warnings-see-stderr =
    { $first } (+{ $count ->
        [one] { $count } klahvikaardi hoiatus
       *[other] { $count } klahvikaardi hoiatust
    } veel, vaata stderr-i)
app-agent-server-listening-on-token-in-the = Agendi server kuulab aadressil { $addr } (võti on agendi käitusfailis) { $key_note }
app-agent-server-failed-on-port = Agendi server ebaõnnestus pordil { $port }: { $e }
app-web-build-files-open-and-save-through = Veebiversioon: failid avatakse ja salvestatakse brauseri kaudu
app-ready = Valmis
app-the-3d-view-needs-the-wgpu-renderer = 3D-vaade vajab wgpu-renderdajat
app-grouped = Rühmitatud
app-error = Viga: { $e }
app-select-a-group-to-ungroup = Vali rühm, mille rühmitus tühistada
app-ungrouped = Rühmitus tühistatud
app-select-one-group-or-symbol-to-edit = Vali muutmiseks üks rühm või sümbol
app-editing-group-exit-with-the-exit-group = Rühma muutmine. Välju klahviga „Välju rühmast“
app-editing-the-symbol-exit-to-apply-the = Sümboli muutmine. Välju, et rakendada muudatused kõigile eksemplaridele
app-you-are-not-in-a-group = Sa ei ole rühmas
app-symbol-updated = Sümbol uuendatud
app-created-site-model = Maapinnamudel loodud: { $id }
app-created-texture-beds = Loodi { $count } tekstuuriala
app-cannot-start-the-session-for-scripts = Skriptide seanssi ei saa käivitada: { $e }
app-untitled = Nimetu
app-kadakas = { $name } - Kadakas
app-downloading = Laen alla: { $name }
app-opened = Avatud: { $name }
app-cannot-open = Ei saa avada: { $name }: { $e }
app-opened-2 = Avatud: { $name }
app-load-notes = Fail avati, aga mõnda selle osa parandati või jäeti välja: { $notes }
app-saved = Salvestatud: { $name }
app-exported = Eksporditud: { $name }
app-add-a-sheet-layer-first-navigation-sheet = Lisa kõigepealt lehekiht (Navigeerimine > Lehekihid > +)
app-exported-sheet-s-to =
    Eksporditi { $count ->
        [one] { $count } leht
       *[other] { $count } lehte
    } faili { $path }
app-error-2 = Viga: { $message }
app-restored-view = Vaade „{ $name }“ taastatud
app-flyover-drag-to-orbit-shift-drag-or = Ülelend: lohista ringi liikumiseks, Shift-lohistamine või keskmise nupuga lohistamine nihutab, ratas suurendab
app-select-the-objects-to-duplicate-first = Vali kõigepealt objektid, mida dubleerida
app-is-planned-not-built-yet = { $name } ({ $keys }) on planeeritud, aga pole veel valmis
app-the-view-history-is-for-the-plan = Vaadete ajalugu on plaani jaoks. Lülitu vaatele Plaan (ülalt)
app-no-earlier-view = Varasemat vaadet pole
app-no-later-view = Hilisemat vaadet pole
app-select-the-objects-to-rotate-first = Vali kõigepealt objektid, mida pöörata
app-select-the-objects-to-flip-first = Vali kõigepealt objektid, mida peegeldada
app-select-two-or-more-shapes-first = Vali kõigepealt kaks või rohkem kujundit
app-done = Valmis
app-preferences = Eelistused
app-export-svg = Ekspordi SVG
app-select-all = Vali kõik
app-import = Import
app-export = Eksport
app-map-sources = Kaardiallikad
app-change-crs = Muuda koordinaatsüsteemi
app-scripts = Skriptid
app-export-pdf = Ekspordi PDF
app-theme = Teema: { $theme }
app-snapping-toggle = Haaramine: lülita { $label }
app-show = Näita: { $name }
app-save-layout-as = Salvesta paigutus nimega
app-reset-layout = Lähtesta paigutus
app-no-layout-named = Paigutust nimega { $n } pole
app-name-of-the-layout = Paigutuse nimi
app-save = Salvesta
app-cancel = Katkesta
app-saved-layout = Paigutus salvestatud: { $name }
app-objects-selected =
    { $objects ->
        [one] { $objects } objekt
       *[other] { $objects } objekti
    }, { $selected } valitud, { $view }, { $projection }
app-status-objects-zoom =
    { $objects ->
        [one] { $objects } objekt
       *[other] { $objects } objekti
    }, { $selected } valitud, suum { $zoom } px/mm, 1:{ $scale }
app-smartcursor-cues-on = SmartCursor-vihjed sees
app-smartcursor-cues-off = SmartCursor-vihjed väljas
app-angles-relative-to-the-prior-segment-on = Nurgad eelmise lõigu suhtes: sees
app-angles-relative-to-the-prior-segment-off = Nurgad eelmise lõigu suhtes: väljas
app-snap-on = Haaramine { $name }: sees
app-snap-off = Haaramine { $name }: väljas
app-select-2d-shapes-to-turn-into-a-solid-first = Esmalt tuleb valida 2D-kujundid
app-select-a-line-for-the-axis-too = Teljeks tuleb valida ka joon
app-no-solid-made-reason = Keha ei tehtud: { $reason }
app-solids-made =
    { $count ->
        [one] Tehti { $count } keha
       *[other] Tehti { $count } keha
    }
app-solids-made-skipped = Tehtud kehi: { $count }, vahele jäetud: { $skipped }
app-saved-with-notes = Salvestatud: { $name }. { $notes }
app-save-missing-title = Faili osi ei leita
app-save-missing-text =
    Faili { $file } muudeti väljaspool Kadakast ja selles ei ole enam { $what }. Midagi ei salvestatud.

    Kas salvestada ilma nendeta? Pildid asendatakse väikeste hallide kohatäidetega ja versioonid, mida ei leita, jäetakse välja; kõik muu salvestatakse. Või vasta Ei, pane fail (või selle .bak koopia) tagasi ja salvesta uuesti.
app-save-missing-cancelled = Ei salvestatud: faili osi ei leita. Pane fail (või selle .bak koopia) tagasi ja salvesta uuesti või salvesta ilma nendeta.
app-save-missing-done = Ei leitud ja jäeti failist välja: { $what }.
app-save-overwrite-title = Faili muutis teine programm
app-save-overwrite-text =
    Faili { $file } muutis teine programm pärast seda, kui sa selle avasid (teine Kadakase aken või sünkroonimisprogramm). Kui salvestad, asendatakse see töö. Vana fail jääb kõrvale .bak koopiana.

    Kas asendada?
app-save-overwrite-cancelled = Ei salvestatud: teine programm muutis faili pärast seda, kui sa selle avasid. Ava fail uuesti, et näha, mis muutus, või salvesta teise nimega.

app-opened-first-of-several =
    { $count ->
        [one] Avasin faili { $name }. { $count } fail jäi avamata: Kadakas näitab korraga ühte joonist.
       *[other] Avasin faili { $name }. { $count } faili jäi avamata: Kadakas näitab korraga ühte joonist.
    }
app-no-guides = Abijooni ei ole.
app-guides-deleted = { $count ->
    [one] 1 abijoon on kustutatud.
   *[other] { $count } abijoont on kustutatud.
}
app-guides-delete-title = Kustuta abijooned
app-guides-delete-question = Kas kustutada mudeli kõik { $count } abijoont?
app-guides-delete-yes = Kustuta abijooned
app-nothing-hidden = Midagi pole peidetud.
app-unhid = { $count ->
    [one] 1 asi paistab jälle.
   *[other] { $count } asja paistab jälle.
}
app-hidden-objects = { $count ->
    [one] 1 peidetud objekt
   *[other] { $count } peidetud objekti
}
app-hidden-objects-tip = Need objektid peideti 3D-modelleerijas. Klõpsa, et need jälle nähtavale tuua.
