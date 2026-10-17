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
agent-activity-session-version = Enne seda salvestatud versioon: #{ $version }
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
agent-activity-why-locked = Selle sammu objekt on lukustatud kihil.
agent-activity-why-group = Hiljem tehtud asi hoiab selle sammu objekti.
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
