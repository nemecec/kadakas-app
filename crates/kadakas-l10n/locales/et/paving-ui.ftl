## paving_ui: Loo objektid kujudest, Jaga kõvakate tsoonideks, mustri kontroll ja kivimuster objekti infos

paving-ui-error = Viga: { $e }
paving-ui-done = Valmis.
paving-ui-select-a-hardscape = Vali kõigepealt kõvakate.
paving-ui-click-a-point = Klõpsa joonisel punktil. Escape lõpetab.
paving-ui-click-an-edge = Klõpsa servale. Escape lõpetab.
paving-ui-no-edge-there = Seal pole serva. Klõpsa servale lähemal.
paving-ui-no-such-zone = Tsooni pole enam olemas.
paving-ui-cancel = Loobu
paving-ui-show = Näita
paving-ui-remove = Eemalda
paving-ui-edit = Muuda
paving-ui-apply = Rakenda
paving-ui-back = Tagasi
paving-ui-name = Nimi
paving-ui-none = (puudub)
paving-ui-unit = Kivi

paving-ui-create-objects-from-shapes = Loo objektid kujudest
paving-ui-shapes-selected =
    { $count ->
        [one] { $count } kuju on valitud
       *[other] { $count } kuju on valitud
    }
paving-ui-close-gaps-up-to = Sulge vahed kuni
paving-ui-the-widest-gap-between-ends-that-is-closed = Kõige laiem vahe kahe otsa vahel, mis suletakse. Laiem vahe jääb lahti ja selle taha ala ei looda.
paving-ui-make = Loo
paving-ui-delete-the-shapes-that-are-used-up = Kustuta kasutatud kujud
paving-ui-a-shape-goes-only-when-all-its-pieces-are-in-regions-that-are-made = Kuju kustutatakse ainult siis, kui kõik selle tükid on loodud piirkondades.
paving-ui-regions-found =
    { $count ->
        [one] { $count } piirkond leitud
       *[other] { $count } piirkonda leitud
    }
paving-ui-region-row =
    { $holes ->
        [0] Piirkond { $number }: { $role }, { $area }
       *[other] Piirkond { $number }: { $role }, { $area }, auke { $holes }
    }
paving-ui-no-closed-regions = Kujud ei piira ühtegi piirkonda. Vaata aruannet: mõni vahe võib olla tolerantsist laiem.
paving-ui-report = Aruanne
paving-ui-click-to-show-the-place = Klõpsa, et koht nähtavale tuua.
paving-ui-create = Loo
paving-ui-made-objects =
    { $kept ->
        [0] Loodi { $count } objekti.
       *[other] Loodi { $count } objekti. { $kept } kuju jäi alles, sest osa neist pole loodud piirkonnas.
    }

paving-ui-split-hardscape-into-zones = Jaga kõvakate tsoonideks
paving-ui-draw-split-lines-help = Joonista jooned kõvakatte peale või vali juba joonistatud jooned. Topeltklõps või Enter lõpetab joone. Escape lõpetab joonistamise.
paving-ui-draw-a-split-line = Joonista jagamisjoon
paving-ui-stop-drawing = Lõpeta joonistamine
paving-ui-finish-line = Lõpeta joon
paving-ui-use-the-selected-lines = Kasuta valitud jooni
paving-ui-lines-arcs-and-polylines-that-are-selected-become-split-lines = Valitud jooned, kaared ja polüjooned saavad jagamisjoonteks.
paving-ui-lines-added =
    { $count ->
        [one] { $count } joon lisatud
       *[other] { $count } joont lisatud
    }
paving-ui-split-line-object = Joon { $number } (joonistatud joon)
paving-ui-split-line-drawn = Joon { $number } (joonistatud siin)
paving-ui-no-split-lines-yet = Jagamisjooni pole veel.
paving-ui-zones-follow-the-lines-that-were-picked = Tsoonid järgivad valitud jooni
paving-ui-when-such-a-line-moves-the-zones-move-with-it = Kui selline joon liigub, liiguvad tsoonid sellega.
paving-ui-line-ends-join-an-edge-within = Joone ots liitub servaga kuni
paving-ui-units-at-a-zone-edge = Kivid tsooni serval
paving-ui-zone-units-cut = Lõigatud tsooni servas
paving-ui-zone-units-centroid = Tervelt, keskpunkti järgi
paving-ui-zones-made =
    { $count ->
        [one] { $count } tsoon
       *[other] { $count } tsooni
    }
paving-ui-zone-row = { $name }: { $area }
paving-ui-zone-edges-cut = Tsooni servad lõikavad { $lost } tervet kivi ja lisavad { $extra } tükki.
paving-ui-zone-edges-have-no-joint = Kaks tsooni kohtuvad tsooni serval ilma vuugita. Pane serv mustri vuugi kohale või lase iga kivil tervelt minna oma keskpunkti tsooni.
paving-ui-split = Jaga
paving-ui-split-into-zones =
    { $count ->
        [one] Kõvakattel on { $count } tsoon.
       *[other] Kõvakate on jagatud { $count } tsooniks.
    }

paving-ui-check-unit-patterns = Kontrolli kivimustreid
paving-ui-run-the-check = Käivita kontroll
paving-ui-the-check-looks-at-the-selected-hardscapes-or-at-all = Kontroll vaatab valitud kõvakatteid või kõiki, kui ühtegi pole valitud. Klõpsa leiul, et see nähtavale tuua.
paving-ui-no-result-yet = Tulemust veel pole.
paving-ui-the-drawing-changed-run-the-check-again = Joonis muutus pärast kontrolli. Käivita see uuesti.
paving-ui-no-findings =
    { $objects ->
        [one] Leide pole { $objects } kõvakattes ({ $units } kivi).
       *[other] Leide pole { $objects } kõvakattes ({ $units } kivi).
    }
paving-ui-findings =
    { $count ->
        [one] { $count } leid { $objects } kõvakattes ({ $units } kivi)
       *[other] { $count } leidu { $objects } kõvakattes ({ $units } kivi)
    }
paving-ui-could-not-be-checked =
    { $count ->
        [one] { $count } kõvakatet ei saanud kontrollida.
       *[other] { $count } kõvakatet ei saanud kontrollida.
    }
paving-ui-all-kinds = Kõik
paving-ui-kind-count = { $kind } ({ $count })
paving-ui-object-error = { $name }: { $e }
paving-ui-object-findings = { $name }: { $count }
paving-ui-finding-row = { $kind }: { $size } grupis { $group }
paving-ui-the-list-is-cut-the-counts-are-exact = Nimekiri on lühendatud. Arvud on täpsed.

paving-ui-unit-pattern = Kivimuster
paving-ui-pattern-bond = { $bond }: { $length } x { $width }, vuuk { $gap }
paving-ui-pattern-module = { $bond }, vuuk { $gap }
paving-ui-edit-pattern = Muuda mustrit...
paving-ui-the-pattern-belongs-to-the-style-it-changes-every-hardscape-with-it = Muster kuulub stiilile. Muudatus mõjutab iga selle stiiliga kõvakatet.
paving-ui-pick-origin = Vali algus
paving-ui-click-a-point-in-the-drawing-where-the-pattern-starts = Klõpsa joonisel punktil, kust muster algab.
paving-ui-align-to-edge = Joonda servale
paving-ui-click-an-edge-of-the-outline-the-pattern-starts-there-and-runs-along-it = Klõpsa kontuuri servale. Muster algab seal ja kulgeb piki serva.
paving-ui-count-the-units = Loe kivid kokku
paving-ui-a-big-area-takes-time-to-count-so-it-is-counted-when-you-ask = Suure ala lugemine võtab aega, seega loetakse see sinu käsul.
paving-ui-the-counts-are-of-an-earlier-state = Arvud on joonise varasema seisu kohta.
paving-ui-whole-units = Terved kivid
paving-ui-cut-units = Lõigatud kivid
paving-ui-cut-units-detail = { $count } ({ $slivers } kitsast riba, { $unfillable } liiga väikest panemiseks)
paving-ui-order = Tellimus (kivi)
paving-ui-order-range = { $min } kuni { $max }
paving-ui-laid-area = Sillutatud pind
paving-ui-whole-and-cut = { $whole } tervet, { $cut } lõigatud
paving-ui-zones-count =
    { $count ->
        [one] { $count } tsoon
       *[other] { $count } tsooni
    }
paving-ui-no-zones = Tsoone pole
paving-ui-split-into-zones-button = Jaga tsoonideks...
paving-ui-remove-the-split-lines = Eemalda jagamisjooned
paving-ui-zone-pattern = Muster
paving-ui-the-pattern-of-the-field = Välja muster
paving-ui-a-pattern-of-its-own = Oma muster
paving-ui-use-the-field-s = Kasuta välja nurka
paving-ui-as-the-field = { $angle } (nagu väljal)
paving-ui-origin-as-the-field = Muster algab nagu väljal
paving-ui-aligned-to-edge = Joondatud servale { $number }
paving-ui-clear-alignment = Tühista joondus
paving-ui-not-aligned-to-an-edge = Servale joondamata
paving-ui-align-to-split-line = Joonda jagamisjoonele
paving-ui-split-line-number = Joon { $number }
paving-ui-courses-count =
    { $count ->
        [one] { $count } rida
       *[other] { $count } rida
    }
paving-ui-add-a-course = Lisa rida...
paving-ui-a-row-of-units-along-an-edge-for-example-a-soldier-course-along-a-kerb = Rida kive piki serva, näiteks püstkivide rida piki äärekivi.
paving-ui-course-row = { $name }: { $units } kivi, { $length }
paving-ui-course-joints = , vuugid { $narrowest } kuni { $widest }
paving-ui-course-joints-range = Vuugid { $narrowest } kuni { $widest }
paving-ui-course-preview = { $units } kivi, { $corners } nurka, { $length } pikk
paving-ui-course-saved = Rida { $name } salvestatud.
paving-ui-along = Piki
paving-ui-lies = Asukoht
paving-ui-size-along-the-row = Piki rida
paving-ui-size-across-the-row = Risti rida
paving-ui-offset = Nihe
paving-ui-corner = Nurk
paving-ui-start-the-field-at-the-course = Alusta väli reast
paving-ui-moves-the-origin-and-angle-of-the-pattern-so-that-its-first-row-is-whole = Nihutab mustri algust ja nurka nii, et selle esimene rida on terve.
paving-ui-counting = Loen...
paving-ui-checking = Kontrollin...
paving-ui-the-drawing-changed-the-pick-ended = Joonis muutus, seega valik lõppes. Alusta uuesti.
paving-ui-remove-the-split-lines-and-the-zone-settings = Kas eemaldada jagamisjooned? Tsoonide sätted (muster, algus, nurk) lähevad nendega. Seda saab tagasi võtta.
