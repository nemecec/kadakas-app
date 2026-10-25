## uic-modeler-mod15: täpse kirjeldusega ringid ja kaared (modeling/curve.rs, modeling/messages.rs)

uic-curve-note-dropped = Ring või kaar kaotas oma täpse kirjelduse: selle punktid ei asu enam ühel ringjoonel. Kõver ise jääb alles.
uic-curve-note-angles = Kaare algus- ja lõppnurk uuendati. Selle keskpunkt ja raadius jäävad samaks.
uic-curve-note-kind = Kõver on nüüd { $kind ->
        [arc] kaar
       *[other] ring
    }. Selle keskpunkt ja raadius jäävad samaks.
uic-curve-note-not-made = Kõverat ei tehtud, sest üks serv on juba kõveras. Ringi kirjeldus jäeti välja.
uic-curve-refused-no-curve = Seda kõverat pole olemas.
uic-curve-refused-no-description = Sellel kõveral pole täpset ringi, seega ei saa selle lõikude arvu muuta.
uic-curve-refused-range = Lõikude arv peab olema 3 kuni 1000.
uic-curve-refused-shared = Kõvera sees kohtub veel üks serv, seega ei saa kõverat uuesti ehitada.
uic-curve-refused-plane = Kõverat kasutab tahk teisel tasapinnal, näiteks väljatõmmatud ringi sein. Kõverat ei saa uuesti ehitada.
uic-curve-refused-loop = Tahk kasutab kõverat viisil, mida ei saa uuesti ehitada.
uic-curve-refused-invalid = Uued lõigud teeksid mudeli vigaseks, näiteks liiga lühikeste servadega.
