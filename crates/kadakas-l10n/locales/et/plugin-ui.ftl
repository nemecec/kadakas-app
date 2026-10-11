## plugin-ui: plugin_ui.rs

plugin-ui-plug-ins = Lisamoodulid
plugin-ui-intro = Lisamoodul on väike programm, mis töötab liivakastis. Sellel ei ole ligipääsu failidele, võrgule ega kellale. Lisamooduleid tuleks paigaldada ainult allikatest, mida usaldatakse.
plugin-ui-kept = Paigaldatud lisamoodulid ja antud usaldus jäävad järgmiseks seansiks alles.
plugin-ui-session-only = See brauser ei paku lisamoodulitele salvestusruumi. Need jäävad alles ainult lehe sulgemiseni.
plugin-ui-install = Paigalda lisamoodul...
plugin-ui-none = Ühtegi lisamoodulit pole paigaldatud.
plugin-ui-trusted = Usaldatud
plugin-ui-not-trusted = Usaldamata
plugin-ui-in-use =
    { $count ->
        [one] Kasutab { $count } objekt
       *[other] Kasutab { $count } objekti
    }
plugin-ui-review = Vaata üle...
plugin-ui-stop-trusting = Lõpeta usaldamine
plugin-ui-remove = Eemalda
plugin-ui-remove-blocked = Joonise objektid kasutavad seda lisamoodulit. Need tuleb enne kustutada.
plugin-ui-removed = Eemaldati { $name }.
plugin-ui-installed = Paigaldati { $name }.
plugin-ui-replaced =
    { $count ->
        [one] Paigaldati lisamooduli { $name } uus järk. { $count } objektil on veel vana järgu kujundid.
       *[other] Paigaldati lisamooduli { $name } uus järk. { $count } objektil on veel vana järgu kujundid.
    }
plugin-ui-not-kept = Järgmiseks seansiks ei jäänud alles: { $message }
plugin-ui-install-failed = Lisamoodulit ei saa paigaldada: { $message }
plugin-ui-read-failed = Faili ei saa lugeda: { $message }
plugin-ui-too-big = Fail on lisamooduli jaoks liiga suur. Piir on { $max } baiti.
plugin-ui-stored-problem = Salvestatud lisamoodulit { $name } ei laaditud: { $reason }
plugin-ui-trust-title = Kas usaldada seda lisamoodulit?
plugin-ui-version = Versioon
plugin-ui-author = Autor (lisamooduli enda väide)
plugin-ui-no-author = pole näidatud
plugin-ui-hash = SHA-256
plugin-ui-size = Suurus
plugin-ui-bytes = { $size } baiti
plugin-ui-can-do = Mida see teeb
plugin-ui-kind-object = loob parameetrilisi objekte
plugin-ui-kind-tool = pakub joonestustööriista
plugin-ui-no-permissions = Lisamoodul ei saa ühtegi luba. See ei saa lugeda joonist, faile, võrku ega kella. See muudab ainult Kadakase saadetud väärtused ja punktid kujunditeks.
plugin-ui-warning = See võib siiski joonistada valesid kujundeid või kulutada aega. Usaldus kehtib just selle faili kohta. Uus järk vajab uut usaldust.
plugin-ui-agents = API kaudu töötav agent ei saa lisamoodulit kasutaja eest usaldada.
plugin-ui-trust = Usalda
plugin-ui-cancel = Loobu
plugin-ui-trusted-status = Usaldati { $name }.
plugin-ui-object-heading = Lisamooduli objekt: { $name } { $version }
plugin-ui-object-not-installed = Lisamoodul ei ole paigaldatud. Kujundid jäävad endiseks.
plugin-ui-object-stale = Kujundid on lisamooduli teisest järgust. Paigaldatud järgu kasutamiseks tuleb käivitada uuesti.
plugin-ui-needs-trust = Seda lisamoodulit ei usaldata, seega parameetreid ei saa muuta.
plugin-ui-object-error = Lisamoodul ebaõnnestus: { $message }
plugin-ui-object-error-note = Kujundid on viimasest õnnestunud käivitusest.
plugin-ui-run-again = Käivita uuesti
plugin-ui-run-again-hint = Loo kujundid uuesti paigaldatud lisamooduliga.
plugin-ui-default = Vaikeväärtus
plugin-ui-tool-set = Lisamoodulid
plugin-ui-tool-none = Ühelgi lisamoodulil pole tööriista. Lisamooduli saab paigaldada menüüs Fail > Lisamoodulid.
plugin-ui-tool-untrusted = Pole veel usaldatud. Ülevaatamiseks tuleb klõpsata.
plugin-ui-tool-started = Lisamooduli tööriist { $name }. Peatamiseks tuleb vajutada Esc.
plugin-ui-tool-failed = Lisamooduli tööriist peatus: { $message }
plugin-ui-tool-done-object = Paigutati lisamooduli objekt.
plugin-ui-tool-done-shapes =
    { $count ->
        [one] Lisati { $count } kujund.
       *[other] Lisati { $count } kujundit.
    }
plugin-ui-changed = Lisamoodul muutus, kui dialoog oli lahti. See tuleb uuesti üle vaadata.
plugin-ui-problem = Seda lisamoodulit ei saa usaldada: { $message }
plugin-ui-old-abi = See lisamoodul on ehitatud lisamooduli liidese teise versiooni jaoks ja seda ei laaditud. Uue järgu saab küsida autorilt. { $message }
