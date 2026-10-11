## collab-ui: collab_ui.rs (Fail > Koostöö, olekuüksus, paleti Inimesed)

collab-ui-connect = Ühenda...
collab-ui-disconnect = Katkesta ühendus
collab-ui-dialog-title = Ühendumine ühisdokumendiga
collab-ui-server = Serveri aadress
collab-ui-document = Dokumendi ID
collab-ui-token = Luba
collab-ui-name = Nimi
collab-ui-connect-button = Ühenda
collab-ui-default-token = Selle serveri jaoks kasutatakse luba muutujast KADAKAS_COLLAB_TOKEN või lipult --collab-token.
collab-ui-token-note = Luba jääb ainult mällu. Kadakas ei salvesta seda kunagi.
collab-ui-server-copy-note = Serveri koopia asendab avatud dokumendi.
collab-ui-replace-title = Kas asendada avatud dokument?
collab-ui-replace-text = Avatud dokumendis on salvestamata muudatusi. Ühisdokument asendab selle. Dokument tuleb enne salvestada või praegu asendada.
collab-ui-replace-button = Asenda
collab-ui-discard-title = Kas jätta ootel muudatused kõrvale?
collab-ui-discard-text = Võrguühenduseta tehtud muudatused ootavad teist dokumenti või teist kasutajat. Jätkamisel need kaovad.
collab-ui-discard-button = Jäta kõrvale ja ühenda
collab-ui-joining = Ühisdokumendiga liitumine... Dokumendi saabumist tuleb oodata.
collab-ui-status = Koostöö: { $state }
collab-ui-token-expired = Luba on aegunud. Uus luba tuleb küsida serveri haldurilt.
collab-ui-already-connected = Ühendus on juba olemas. Esmalt tuleb ühendus katkestada.
collab-ui-connect-failed = Ühendumine ebaõnnestus: { $error }
collab-ui-disconnected = Ühendus katkestati. Muudatused jätkuvad võrguühenduseta.
collab-ui-error = Koostöö viga: { $error }
collab-ui-problems =
    { $count ->
        [one] Ühte muudatust ei saanud jagada.
       *[other] { $count } muudatust ei saanud jagada.
    }
collab-ui-undo-skipped =
    { $count ->
        [one] Üks muudatus jäeti alles, sest keegi teine muutis seda hiljem.
       *[other] { $count } muudatust jäeti alles, sest teised muutsid neid hiljem.
    }
collab-ui-nothing-to-undo = Midagi pole võtta tagasi.
collab-ui-nothing-to-redo = Midagi pole uuesti teha.
collab-ui-blocked-replace = Enne teise dokumendi avamist või loomist tuleb ühisdokumendi ühendus katkestada.
collab-ui-web-joining = Liitumine serveriga { $server }...
collab-ui-you = Ise: { $name }
collab-ui-you-unnamed = Ise
collab-ui-nobody = Kedagi teist siin pole.
collab-ui-not-connected = Ühendus puudub. Ühendumiseks sobib Fail > Koostöö > Ühenda.
collab-ui-quiet = { $name } (vaikib)
collab-ui-selected =
    { $count ->
        [one] { $count } objekt valitud
       *[other] { $count } objekti valitud
    }
