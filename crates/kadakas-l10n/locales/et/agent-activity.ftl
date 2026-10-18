## agent-activity: agent_activity_ui.rs (AGR-05, AGR-06: mida agent muutis, tegevuste paneel)

agent-activity-title = Agendi tegevus
agent-activity-intro = Agendi sammud. Üks päring või üks skriptikäivitus on üks samm. Muudatused, mida sa pole nähtuks märkinud, on plaanil ja 3D-s esile tõstetud.
agent-activity-empty = Agent pole joonist veel muutnud.
agent-activity-show = Tõsta muudatused esile
agent-activity-show-hint = Tõsta plaanil ja 3D-s esile, mida agent lisas, muutis ja eemaldas ({ $key })
agent-activity-none = Ülevaatamiseks pole muudatusi.
agent-activity-scope-unseen =
    { $count ->
        [one] Näitan 1 sammu muudatusi, mida sa pole näinud
       *[other] Näitan { $count } sammu muudatusi, mida sa pole näinud
    }
agent-activity-scope-step = Näitan ainult sammu { $step } muudatusi
agent-activity-scope-all = Näita kõiki nägemata
agent-activity-next-change = Järgmine muudatus
agent-activity-next-change-hint = Suumi agendi järgmise muudatuse juurde ({ $key })
agent-activity-prev-change = Eelmine muudatus
agent-activity-prev-change-hint = Suumi agendi eelmise muudatuse juurde ({ $key })
agent-activity-zoom-all = Suumi muudatustele
agent-activity-mark-all = Märgi kõik nähtuks
agent-activity-mark-all-hint = Eemalda loetletud muudatuste esiletõst ({ $key })
agent-activity-mark-one = Nähtud
agent-activity-mark-one-hint = Märgi see samm nähtuks: selle muudatusi ei tõsteta enam esile
agent-activity-zoom = Suumi
agent-activity-zoom-hint = Suumi selle, mida see samm muutis
agent-activity-revert = Võta tagasi
agent-activity-revert-hint = Võta see samm tagasi ja jäta alles, mida sina ja agent pärast seda tegite
agent-activity-undo-to = Samm tagasi siia
agent-activity-undo-to-hint = Võta see samm ja kõik sellele järgnevad sammud tagasi
agent-activity-step = Samm { $id }
agent-activity-session = Agendi seanss { $session }
agent-activity-session-version = Enne seda salvestatud versioon: #{ $version } (eraldi tagasivõtmise samm)
agent-activity-session-no-version = Enne seda ei salvestatud versiooni: { $why }
agent-activity-state-undone = tagasi võetud
agent-activity-state-gone = pole tagasivõtmise ajaloos
agent-activity-state-seen = nähtud
agent-activity-state-dropped = üksikasjad kustutatud
agent-activity-objects = Selle sammu objektid
agent-activity-objects-more = ja veel { $count }
agent-activity-status-added = Lisatud
agent-activity-status-changed = Muudetud
agent-activity-status-removed = Eemaldatud
agent-activity-object-row = { $mark } { $kind } { $id }
agent-activity-object-gone = (praegu joonisel puudub)
agent-activity-before = Enne
agent-activity-after = Pärast
agent-activity-new-object = Uus objekt ({ $kind }) kihil { $layer }.
agent-activity-removed-object = Objekt ({ $kind }) kihilt { $layer } eemaldati.
agent-activity-prop = { $path }: { $old } → { $new }
agent-activity-prop-none = (puudub)
agent-activity-prop-more = Erinevaid omadusi on veel.
agent-activity-banner =
    { $count ->
        [one] Agent muutis joonist 1 sammuga
       *[other] Agent muutis joonist { $count } sammuga
    }
agent-activity-review = Vaata üle
agent-activity-review-hint = Ava agendi tegevuste paneel ({ $key })
agent-activity-dismiss-banner = Peida
agent-activity-status-line = Agendi samm { $id }: { $summary }
agent-activity-reverted = Samm { $id } võeti tagasi: { $summary }
agent-activity-reverted-some = Sammu { $id } objektidest võeti tagasi { $done } / { $total }. { $kept } jäävad nagu on.
agent-activity-undone-to = Võeti tagasi { $count } sammu kuni sammuni { $id }. Uuesti tegemine toob need tagasi.
agent-activity-refused = Sammu { $id } ei võetud tagasi: { $why }
agent-activity-revert-title = Võta samm { $id } tagasi
agent-activity-revert-partial =
    { $blocked ->
        [one] 1 objekti { $total }-st muudeti või kustutati pärast seda sammu. See jääb nagu on.
       *[other] { $blocked } objekti { $total }-st muudeti või kustutati pärast seda sammu. Need jäävad nagu on.
    }
agent-activity-revert-rest =
    { $free ->
        [one] Võta tagasi teine objekt
       *[other] Võta tagasi ülejäänud { $free } objekti
    }
agent-activity-undo-title = Samm tagasi sammuni { $id }
agent-activity-undo-body =
    { $count ->
        [one] See võtab tagasi 1 sammu tagasivõtmise ajaloost.
       *[other] See võtab tagasi { $count } sammu tagasivõtmise ajaloost.
    }
agent-activity-undo-others =
    { $other ->
        [one] Neist 1 on sinu enda samm.
       *[other] Neist { $other } on sinu enda sammud.
    } Uuesti tegemine toob kõik tagasi.
agent-activity-undo-snapshot = See samm muutis ka kihte, klasse, ressursse, sätteid või versioone, seega ei saa seda üksi tagasi võtta.
agent-activity-undo-go = Võta tagasi
agent-activity-cancel = Loobu
agent-activity-why-undone = See samm on juba tagasi võetud.
agent-activity-why-gone = See samm pole enam tagasivõtmise ajaloos (ajaloo piir kustutas selle või uuem muudatus tühjendas selle).
agent-activity-why-dropped = Logi ei hoia enam, mida see samm muutis.
agent-activity-why-snapshot = See samm muutis ka kihte, klasse, ressursse, sätteid või versioone. Kasuta käsku "Samm tagasi siia".
agent-activity-why-conflicts =
    { $count ->
        [one] 1 objekti muudeti või kustutati pärast seda sammu.
       *[other] { $count } objekti muudeti või kustutati pärast seda sammu.
    }
agent-activity-why-shared = See joonis on jagatud. Sammu objekte saab tagasi võtta ainult ükshaaval. Oma muudatusi võta tagasi käsuga Võta tagasi.
agent-activity-why-locked = Selle sammu objekt on lukustatud või halli kihil või halli klassis.
agent-activity-why-other = { $why }
agent-activity-sum-none = nähtavaid muudatusi pole
agent-activity-n-objects =
    { $count ->
        [one] 1 objekt
       *[other] { $count } objekti
    }
agent-activity-n-layers =
    { $count ->
        [one] 1 kiht
       *[other] { $count } kihti
    }
agent-activity-n-classes =
    { $count ->
        [one] 1 klass
       *[other] { $count } klassi
    }
agent-activity-n-resources =
    { $count ->
        [one] 1 ressurss
       *[other] { $count } ressurssi
    }
agent-activity-sum-added = lisatud { $what }
agent-activity-sum-changed = muudetud { $what }
agent-activity-sum-removed = eemaldatud { $what }
agent-activity-sum-kinds = { $what } ({ $kinds })
agent-activity-sum-other = { $count } muud
agent-activity-sum-layer-named = lisatud kiht { $names }
agent-activity-sum-document = muudetud joonise sätteid
agent-activity-sum-versions = muudetud versioone
agent-activity-sum-issues = muudetud märkusi
agent-activity-3d-added = Lisatud { $count }
agent-activity-3d-changed = Muudetud { $count }
agent-activity-3d-removed = Eemaldatud { $count }
agent-activity-state-reverted = tagasi võetud
agent-activity-state-partial = salvestatud ainult osaliselt
agent-activity-kind-undo = Agendi tagasivõtmine
agent-activity-kind-redo = Agendi uuestitegemine
agent-activity-why-partial = Tagasivõtmise ajaloo piir kustutas osa selle sammu andmetest, seega seda ei saa tagasi võtta.
agent-activity-why-reverted = See samm on juba tagasi võetud.
agent-activity-why-not-call = See samm on agendi tagasivõtmine või uuestitegemine, mitte muudatus.
agent-activity-why-nothing = Pole midagi tagasi võtta: selle sammu objektid on tagasi endises seisus või puuduvad.
agent-activity-why-used = Hiljem tehtud töö toetub selle sammu objektidele (näiteks andmesilt, mõõde või rühm). Midagi ei eemaldatud.
agent-activity-why-missing = Midagi, millele selle sammu objekt viitab, enam pole (ressurss, rühm või ankur).
agent-activity-missing =
    { $count ->
        [one] 1 sammu üksikasjad kustutati (liiga palju samme). Selle muudatusi ei tõsteta esile.
       *[other] { $count } sammu üksikasjad kustutati (liiga palju samme). Nende muudatusi ei tõsteta esile.
    }
agent-activity-edited-later =
    { $count ->
        [one] 1 objekti muudeti pärast agenti uuesti.
       *[other] { $count } objekti muudeti pärast agenti uuesti.
    }
agent-activity-object-edited = hiljem muudetud
agent-activity-3d-cap = Näitan { $shown } muudatust { $total }-st
agent-activity-hold = Agent ootab: sa võtsid tagasi oma samme ja agendi muudatus kustutaks need nüüd.
agent-activity-hold-redo = Tee kõik uuesti
agent-activity-hold-release = Lase agendil edasi minna
agent-activity-hold-release-hint = Tagasi võetud samme ei saa pärast agendi järgmist muudatust enam uuesti teha.
agent-activity-changed-meanwhile = Tagasivõtmise ajalugu muutus pärast sinu küsimust. Küsi uuesti.
agent-activity-sum-detail = { $kind }{ $label }: { $changes }
agent-activity-change-both = { $field } { $before } → { $after }
agent-activity-change-set = { $field } määrati: { $after }
agent-activity-change-cleared = { $field } eemaldati (oli { $before })
agent-activity-change-plain = { $field } muutus
agent-activity-change-more = veel { $count }
agent-activity-scope-zone = tsoon
agent-activity-scope-course = rida
agent-activity-scope-split = jaotusjoon
agent-activity-field-angle = nurk
agent-activity-field-origin = alguspunkt
agent-activity-field-elevation = kõrgus
agent-activity-field-thickness = paksus
agent-activity-field-material = materjal
agent-activity-field-style = stiil
agent-activity-field-gap = vuuk
agent-activity-field-name = nimi
agent-activity-field-pattern = muster
agent-activity-field-boundary = kontuur
agent-activity-field-holes = augud
agent-activity-field-offset = nihe
agent-activity-field-corner = nurk (liitekoht)
agent-activity-field-side = külg
agent-activity-field-bond = sidumisviis
agent-activity-field-seed = tugipunkt
agent-activity-field-layer = kiht
agent-activity-field-class = klass
agent-activity-resource-hardscape-style = kõvakatte stiil
agent-activity-resource-landscape-style = haljasala stiil
agent-activity-resource-plant-style = taimestiil
agent-activity-sum-resource = { $kind } '{ $name }': { $changes }
agent-status-listening = Agent: ootab
agent-status-connected =
    { $count ->
        [one] Agent ühendatud
       *[other] Agente ühendatud ({ $count })
    }
agent-status-working = Agent töötab…
agent-status-tip =
    Agendi server aadressil { $addr }
    Kliente: { $clients }
    Viimane päring: { $last }
    Agent võib minu vaadet liigutada: { $lock }
    Klõps avab agendi tegevuse paneeli.
agent-status-last-ago = { $secs } s tagasi
agent-status-last-none = päringuid pole veel olnud
agent-status-yes = jah
agent-status-no = ei
agent-status-notice-connected = Agent ühendus ({ $count } ühendatud).
agent-status-notice-disconnected = Agent katkestas ühenduse ({ $count } on veel ühendatud).
