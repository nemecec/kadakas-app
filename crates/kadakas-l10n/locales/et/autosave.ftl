## autosave: autosave.rs

autosave-less-than-a-minute-ago = vähem kui minut tagasi
autosave-browser-storage-is-not-available = brauseri salvestusruum ei ole saadaval
autosave-browser-storage-is-full = brauseri salvestusruum on täis
autosave-minutes-ago =
    { $n ->
        [one] { $n } minut tagasi
       *[other] { $n } minutit tagasi
    }
autosave-hours-ago =
    { $n ->
        [one] { $n } tund tagasi
       *[other] { $n } tundi tagasi
    }
autosave-days-ago =
    { $n ->
        [one] { $n } päev tagasi
       *[other] { $n } päeva tagasi
    }
autosave-missing-pictures =
    { $count ->
        [one] { $count } pilt
       *[other] { $count } pilti
    }
autosave-missing-versions = versioonid
autosave-missing-other =
    { $count ->
        [one] { $count } muu osa
       *[other] { $count } muud osa
    }
autosave-copy-missing = Fail { $file } muutus kettal ja selles ei ole enam { $what }. Taastekoopias on piltide asemel väikesed hallid kohatäited ja selles failis olnud versioone ei ole.
autosave-copy-as-json = Taastekoopia on tavaline JSON-vorm, sest konteineri vorm ületaks piiri: { $why }
autosave-over-missing = Automaatsalvestus ei asendanud sinu faili { $file }, sest see muutus kettal ja selles ei ole enam { $what }. Taastekoopia kirjutati. Salvesta ise, et otsustada, mida teha.
autosave-over-changed = Automaatsalvestus ei asendanud sinu faili { $file }, sest teine programm muutis seda pärast seda, kui sa selle avasid. Taastekoopia kirjutati. Salvesta ise, et otsustada, mida teha.
