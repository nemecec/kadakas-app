## irrigation-ui: irrigation_ui.rs

irrigation-ui-model = Mudel
irrigation-ui-arc = Kaar
irrigation-ui-direction = Suund
irrigation-ui-type = Tüüp
irrigation-ui-material = Materjal
irrigation-ui-size = Suurus
irrigation-ui-name = Nimi
irrigation-ui-static-pressure = Staatiline rõhk
irrigation-ui-available-flow = Saadaolev vooluhulk
irrigation-ui-zone-0-next = Tsoon (0 = järgmine)
irrigation-ui-flow-factor-kv = Vooluteguri kv
irrigation-ui-stations = Jaamu
irrigation-ui-select-an-irrigation-object-and-use-calculate = Vali kastmisobjekt ja kasuta objekti infos nuppu „Arvuta“.
irrigation-ui-position = Asukoht
irrigation-ui-arc-deg = Kaar (kraadi)
irrigation-ui-direction-deg = Suund (kraadi)
irrigation-ui-radius = Raadius
irrigation-ui-zone = Tsoon
irrigation-ui-length = Pikkus
irrigation-ui-static-pressure-kpa = Staatiline rõhk (kPa)
irrigation-ui-available-flow-l-min = Saadaolev vooluhulk (l/min)
irrigation-ui-supply-loss-kpa = Varustuskadu (kPa)
irrigation-ui-meter-loss-kpa = Mõõturi kadu (kPa)
irrigation-ui-controller = Kontroller
irrigation-ui-station-0-none = Jaam (0 = puudub)
irrigation-ui-calculated-no-zone-has-water-see-the = Arvutatud: ükski tsoon ei saa vett. Vaata hoiatusi objekti infos.
irrigation-ui-calculated-zone-s-no-warnings =
    Arvutatud { $z ->
        [one] { $z } tsoon
       *[other] { $z } tsooni
    }: hoiatusi pole
irrigation-ui-calculated-zone-s-warning-s-see-object =
    Arvutatud { $z ->
        [one] { $z } tsoon
       *[other] { $z } tsooni
    }: { $n ->
        [one] { $n } hoiatus
       *[other] { $n } hoiatust
    }. Vaata objekti infot.
irrigation-ui-irrigation = Kastmine
irrigation-ui-calculate = Arvuta
irrigation-ui-flow-pressure-and-zones-of-the-whole = Kogu võrgu vooluhulk, rõhk ja tsoonid
irrigation-ui-not-calculated-yet = Pole veel arvutatud.
irrigation-ui-out-of-date-calculate-again = Aegunud: arvuta uuesti.
irrigation-ui-the-last-calculation-stored-the-zone-in =
    Viimane arvutus salvestas tsooni { $assigned ->
        [one] { $assigned } pihustisse
       *[other] { $assigned } pihustisse
    }.
irrigation-ui-critical-outlet-at-margin = Kriitiline pihusti: { $label } rõhuga { $pressure } (varu { $margin })
irrigation-ui-all-warnings = Kõik hoiatused ({ $count })
irrigation-ui-none = Puudub
irrigation-ui-catalog-model = Kataloogi mudel
irrigation-ui-catalog-pipe = Kataloogi toru
irrigation-ui-text = { $material } { $size }
irrigation-ui-zone-flow-pressure = Tsoon { $zone }: { $flow } l/min, rõhk { $pressure } (miinimum { $minimum }, varu { $margin })
irrigation-ui-zone-total = Tsoon { $zone } kokku: { $flow } l/min, saadaval { $available } l/min, suurim kiirus { $velocity } m/s
