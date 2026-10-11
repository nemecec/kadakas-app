## uic-collab: collab/ (ühenduse olek, vead, vormi kontrollid)

uic-collab-link-offline = Ühenduseta
uic-collab-link-connecting = Ühendumine
uic-collab-link-connected = Ühendatud
uic-collab-link-reconnecting = Ühendumine uuesti
uic-collab-link-refused = Tagasi lükatud
uic-collab-link-stalled = Vastust pole
uic-collab-link-failed = Ühendus ebaõnnestus
# { $link } on ühenduse olekusõna, näiteks "Ühendatud".
uic-collab-status-line-pending =
    { $count ->
        [one] { $link }, ootab { $count } muudatus
       *[other] { $link }, ootab { $count } muudatust
    }
uic-collab-status-people =
    { $count ->
        [one] { $count } teine kasutaja siin
       *[other] { $count } teist kasutajat siin
    }
uic-collab-status-last-error = Viimane viga: { $error }
uic-collab-guest = Külaline { $id }

uic-collab-form-url-missing = Serveri aadress on puudu.
uic-collab-form-secret-in-url = Aadressis ei tohi olla kasutajanime, parooli, päringut ega fragmenti. Luba tuleb sisestada eraldi väljale.
uic-collab-form-doc-missing = Dokumendi ID on puudu.
uic-collab-form-doc-invalid = Dokumendi ID peab olema UUID, näiteks 01939a2e-7c1d-7a2b-9d3e-0123456789ab.

uic-collab-err-auth = Server lükkas loa tagasi. Uus luba tuleb küsida serveri haldurilt ja seejärel uuesti ühenduda.
uic-collab-err-protocol = Server kasutab protokolli versiooni, mida see programm ei toeta. Uuendada tuleb Kadakas või server.
uic-collab-err-document-full = Ühisdokument on täis. Server keeldus viimasest muudatusest ja see võeti tagasi.
uic-collab-err-rejected = Server keeldus muudatusest ja see võeti tagasi ({ $code }): { $message }
uic-collab-err-rate-limited = Server on hõivatud. Muudatused ootavad ja saadetakse uuesti.
uic-collab-err-server-fault = Serveris tekkis tõrge. Muudatused ootavad ja saadetakse uuesti.
uic-collab-err-renumber = Muudatuste numbrid olid varem kasutusel. Need nummerdatakse uuesti ja saadetakse.
uic-collab-err-unknown = Server teatas veast ({ $code }): { $message }
uic-collab-note-no-answer = Server ei vasta või ei võta ühendust vastu. Aadress ja luba tuleb üle kontrollida (brauser ei erista valet luba võrgukatkestusest).
uic-collab-note-connected = Ühisdokumendiga on ühendus loodud.
uic-collab-note-regressed = Server läks varasemasse seisu tagasi. Töö saadetakse uuesti.
uic-collab-note-renumbered = Ootel muudatused said uued numbrid.
uic-collab-note-store-failed = Võrguühenduseta logi salvestamine ebaõnnestus: { $error }
uic-collab-note-store-corrupt = Võrguühenduseta logi ei olnud loetav ja seda ei kasutatud: { $error }
