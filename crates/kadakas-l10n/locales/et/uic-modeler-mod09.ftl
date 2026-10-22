## uic-modeler-mod09: tahkkehade tööriistad modelleris (modeling/solid.rs, modeling/messages.rs)

uic-solid-op-union = Ühend
uic-solid-op-subtract = Lahutus
uic-solid-op-intersect = Ühisosa
uic-solid-op-trim = Kärpimine
uic-solid-op-split = Lõhestamine
uic-solid-need-selection = Vali kõigepealt kahe tahkkeha tahud: klõpsa kummagi tahkkeha tahul, teisel Shifti hoides.
uic-solid-one-model = Mõlemad tahkkehad peavad olema samas mudelis.
uic-solid-need-two = Vali täpselt kahe tahkkeha tahud. Valitud tahud kuuluvad { $found ->
        [0] mitte ühelegi tahkkehale
        [one] ühele tahkkehale
       *[other] { $found } tahkkehale
    }.
uic-solid-done-union = Ühend: kaks tahkkeha said üheks tahkkehaks mahuga { $volume } m³.
uic-solid-done-subtract = Lahutus: teine tahkkeha võeti esimesest välja. Tulemus on { $volume } m³.
uic-solid-done-intersect = Ühisosa: alles jäi ainult ühine osa, { $volume } m³.
uic-solid-done-trim = Kärpimine: esimest tahkkeha lõigati teisega. Teine jäi alles, lõikekohast poolitatuna ja esimese külge kinni. Esimene on nüüd { $volume } m³.
uic-solid-done-split = Lõhestamine: kolm tahkkeha mahuga { $first } m³ (esimene miinus teine), { $common } m³ (ühine) ja { $second } m³ (teine miinus esimene).
uic-solid-refused-open = { $which ->
        [first] Esimene
       *[second] Teine
    } tahkkeha ei ole suletud, seepärast ei muudetud midagi. Kasuta tahkkeha kontrolli, et valida avatud servad.
uic-solid-refused-crossing = { $which ->
        [first] Esimene
       *[second] Teine
    } tahkkeha läbib iseennast, seepärast ei muudetud midagi. Kasuta kõigepealt tahkude lõikamist.
uic-solid-refused-inward = { $which ->
        [first] Esimese
       *[second] Teise
    } tahkkeha tahud vaatavad sissepoole, seepärast ei muudetud midagi. Pööra tema tahud kõigepealt ümber.
uic-solid-refused-flat = { $which ->
        [first] Esimesel
       *[second] Teisel
    } tahkkehal ei ole mahtu, seepärast ei muudetud midagi.
uic-solid-refused-empty = { $which ->
        [first] Esimesel
       *[second] Teisel
    } tahkkehal ei ole tahke, seepärast ei muudetud midagi.
uic-solid-refused-attached = { $which ->
        [first] Esimene
       *[second] Teine
    } tahkkeha jagab servi või tippe muu geomeetriaga, seepärast ei muudetud midagi. Tee temast kõigepealt rühm.
uic-solid-refused-empty-result = Tulemus oleks tühi, sest tahkkehadel ei ole ühist mahtu, seepärast ei muudetud midagi.
uic-solid-refused-cut = Tahkkehi ei saanud siin puhtalt lõigata, näiteks sellepärast, et lõikekoht on väiksem kui tolerants. Midagi ei muudetud. Nihuta ühte tahkkeha veidi.
uic-solid-refused-result = Tulemus ei ole suletud tahkkeha, seepärast ei muudetud midagi. Lõige jättis serva lahti.
uic-solid-refused-volume = Tulemuse maht ei sobi kahe tahkkeha mahtudega, seepärast ei kasutatud seda ja midagi ei muudetud.
uic-solid-refused-invalid = Tulemus annaks mudelile probleemi, mida tal enne ei olnud, seepärast ei muudetud midagi.
uic-solid-refused-limit = Tulemus ületaks mudeli piiri (tahud, servad või tipud), seepärast ei muudetud midagi.
uic-solid-check-nothing = Vali kõigepealt pindmudel või mõni tema tahk. Pole midagi kontrollida.
uic-solid-check-closed = Suletud: { $solids ->
        [one] üks tahkkeha
       *[other] { $solids } tahkkeha
    } mahuga kokku { $volume } m³.
uic-solid-check-open = Pole suletud: { $edges ->
        [one] üks serv on
       *[other] { $edges } serva on
    } avatud ja nüüd valitud. Suletud tahkkehi on { $solids }.
uic-solid-check-odd = Kõik ei ole suletud: { $count ->
        [one] üks kest vaatab
       *[other] { $count } kesta vaatavad
    } sissepoole, neil pole mahtu või nad läbivad iseennast. Suletud tahkkehi on { $solids }.
