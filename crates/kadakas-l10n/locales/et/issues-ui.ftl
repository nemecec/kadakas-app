## issues-ui: issues_ui.rs, bcf_dialog.rs, app/issues.rs

issues-ui-new-from-selection = Uus probleem...
issues-ui-new-here = Uus probleem siia...
issues-ui-new-at-click = Uus probleem klõpsu kohta
issues-ui-pick-hint = Klõpsa joonisel, kuhu probleem panna.
issues-ui-hide-pins = Peida nööpnõelad
issues-ui-bcf = BCF...
issues-ui-hidden = Jagatud probleemide andmed, mida ei näidata ({ $count })
issues-ui-hidden-help = Jagatud dokument sisaldab neid andmeid, kuid see programm ei näita neid, sest need ületavad piiri. Midagi ei kustutatud.
issues-ui-none = Probleeme pole veel. Vali objektid ja vali Uus probleem või kasuta käsku Uus probleem klõpsu kohta.
issues-ui-none-match = Ükski probleem ei sobi filtritega.
issues-ui-count = { $shown } / { $total } probleemi
issues-ui-show-done = Näita lahendatud ja suletud probleeme
issues-ui-filter-assignee = Vastutaja
issues-ui-filter-label = Silt
issues-ui-filter-text = Otsi
issues-ui-clear-filters = Tühjenda
issues-ui-all = Kõik
issues-ui-row-comments =
    { $count ->
        [one] 1 kommentaar
       *[other] { $count } kommentaari
    }
issues-ui-row-objects =
    { $count ->
        [one] 1 objekt
       *[other] { $count } objekti
    }
issues-ui-due = tähtaeg { $date }
issues-ui-due-soon = tähtaeg varsti: { $date }
issues-ui-due-overdue = tähtaeg möödas: { $date }
issues-ui-author-default = tundmatu
issues-ui-unknown-time = aeg pole teada
issues-ui-origin-user = inimene
issues-ui-origin-agent = agent
issues-ui-origin-script = skript
issues-ui-origin-remote = teine kasutaja
issues-ui-type-issue = Probleem
issues-ui-type-error = Viga
issues-ui-type-warning = Hoiatus
issues-ui-type-request = Soov
issues-ui-type-clash = Põrge
issues-ui-type-remark = Märkus
issues-ui-priority-low = Madal
issues-ui-priority-normal = Tavaline
issues-ui-priority-high = Kõrge
issues-ui-priority-critical = Kriitiline
issues-ui-detail-by = Autor { $author } ({ $origin }), { $time }
issues-ui-detail-status = Olek
issues-ui-detail-priority = Prioriteet
issues-ui-detail-type = Liik
issues-ui-detail-assignee = Vastutaja
issues-ui-detail-labels = Sildid
issues-ui-detail-due = Tähtaeg
issues-ui-objects =
    { $count ->
        [one] Probleem puudutab 1 objekti.
       *[other] Probleem puudutab { $count } objekti.
    }
issues-ui-objects-missing =
    { $count ->
        [one] 1 neist pole joonisel.
       *[other] { $count } neist pole joonisel.
    }
issues-ui-viewpoints = Vaated
issues-ui-no-viewpoints = Sellel probleemil pole vaadet.
issues-ui-viewpoint-default-name = Vaade { $number }
issues-ui-viewpoint-go = Mine: { $name }
issues-ui-viewpoint-add = Lisa praegune vaade
issues-ui-viewpoint-delete = Kustuta
issues-ui-thread = Kommentaarid
issues-ui-no-comments = Kommentaare pole veel.
issues-ui-comment-hint = Kirjuta kommentaar
issues-ui-comment-send = Kommenteeri
issues-ui-comment-by = { $author } ({ $origin }), { $time }
issues-ui-comment-edited = muudetud
issues-ui-resolve = Lahenda
issues-ui-reopen = Ava uuesti
issues-ui-delete = Kustuta probleem
issues-ui-confirm-delete = Kas kustutada probleem "{ $title }" koos kommentaaridega? Tagasivõtmine toob selle tagasi.
issues-ui-confirm-delete-others = Probleem "{ $title }" on kellegi teise töö ({ $why }). Kas kustutada ikkagi? Tagasivõtmine toob selle tagasi.
issues-ui-new-title = Uus probleem
issues-ui-new-intro-objects =
    { $count ->
        [one] Valitud objekti kohta.
       *[other] { $count } valitud objekti kohta.
    }
issues-ui-new-intro-point = Joonise ühe punkti juures.
issues-ui-new-intro-none = Pole seotud objekti ega punktiga.
issues-ui-field-title = Pealkiri
issues-ui-field-description = Kirjeldus
issues-ui-field-labels-hint = eraldatud komadega
issues-ui-field-due-hint = AAAA-KK-PP
issues-ui-keep-view = Salvesta probleemiga praegune vaade
issues-ui-keep-snapshot = Salvesta mudelist pilt
issues-ui-create = Loo
issues-ui-cancel = Loobu
issues-ui-created = Probleem "{ $title }" on tehtud.
issues-ui-opened = Probleem "{ $title }" on avatud.
issues-ui-opened-missing =
    { $count ->
        [one] Probleem "{ $title }" on avatud. 1 probleemi objekt pole joonisel.
       *[other] Probleem "{ $title }" on avatud. { $count } probleemi objekti pole joonisel.
    }
issues-ui-deleted = Probleem "{ $title }" on kustutatud.
issues-ui-error = Probleem: { $e }
issues-ui-bcf-title = BCF-probleemid
issues-ui-bcf-intro = BCF on probleemide avatud failivorming (.bcfzip). Kasuta seda probleemide saatmiseks teistele programmidele ja nende probleemide lugemiseks. Vaate kihid ja klassid ei kuulu BCF-i.
issues-ui-bcf-filter = BCF-failid
issues-ui-bcf-export-heading = Eksport
issues-ui-bcf-version = Versioon
issues-ui-bcf-only-open = Ainult probleemid, mis pole lahendatud ega suletud
issues-ui-bcf-export = Ekspordi...
issues-ui-bcf-import-heading = Import
issues-ui-bcf-import = Impordi...
issues-ui-bcf-exported = Kirjutasin { $topics } probleemi faili { $path }.
issues-ui-bcf-imported = Importisin { $imported } probleemi. { $replaced } asendatud, { $unchanged } olid juba samasugused, { $skipped } vahele jäetud.
issues-ui-bcf-notes = Märkused ({ $count })
issues-ui-bcf-error = BCF: { $e }
issues-ui-bcf-conflict-title = Probleemid, mis on joonisel juba olemas
issues-ui-bcf-conflict-text = Faili { $count } probleemil on sama ID mis joonise probleemil, kuid sisu on teine. Midagi ei muudetud. Vali, mida teha.
issues-ui-bcf-conflict-row = Siin: { $ours }. Failis: { $file }.
issues-ui-bcf-keep-mine = Jäta minu oma
issues-ui-bcf-take-file = Võta faili oma
issues-ui-bcf-keep-both = Jäta mõlemad
issues-ui-bcf-unfit-title = Kirjed, mis ei mahu piiridesse
issues-ui-bcf-unfit-text = Faili { $count } kirjet ei mahu Kadakase piiridesse. Midagi ei imporditud. Võid faili importida ilma nende kirjeteta või loobuda.
issues-ui-bcf-unfit-row = { $topic }: { $item }. { $reason }
issues-ui-bcf-unfit-comment = kommentaar
issues-ui-bcf-unfit-snapshot = hetktõmmis
issues-ui-bcf-unfit-camera = kaamera
issues-ui-bcf-unfit-skip = Impordi ilma nende kirjeteta
