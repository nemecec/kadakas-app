## surface-ui: pindmudelite, nende rühmade ja komponendi koopiate objektiinfo (surface_ui.rs)

surface-ui-kind-surface-model = Pindmudel
surface-ui-kind-component = Komponendi koopia
surface-ui-kind-group = Pindmudelite rühm
surface-ui-models = Mudeleid
surface-ui-edges = Servi
surface-ui-faces = Tahke
surface-ui-solid = Keha
surface-ui-solid-empty = Tahke pole
surface-ui-solid-unchecked = Kontrollimata (suur mudel)
surface-ui-solid-closed =
    { $count ->
        [one] Suletud keha
       *[other] { $count } suletud keha
    }
surface-ui-solid-open =
    { $count ->
        [one] Pole suletud (1 lahtine serv)
       *[other] Pole suletud ({ $count } lahtist serva)
    }
surface-ui-volume = Maht
surface-ui-area = Pindala
surface-ui-size = Mõõt
surface-ui-bounds-min = Madalaim nurk
surface-ui-bounds-max = Kõrgeim nurk
surface-ui-hidden = Peidetud
surface-ui-hidden-value =
    { $faces ->
        [one] { $faces } tahk
       *[other] { $faces } tahku
    }, { $edges ->
        [one] { $edges } serv
       *[other] { $edges } serva
    }
surface-ui-soft = Pehmeid servi
surface-ui-material = Materjal
surface-ui-material-value =
    { $count ->
        [one] { $name } (1 tahk)
       *[other] { $name } ({ $count } tahku)
    }
surface-ui-no-material =
    { $count ->
        [one] Puudub (1 tahk)
       *[other] Puudub ({ $count } tahku)
    }
surface-ui-check = Kontrolli keha
surface-ui-edited-in-modeler = Geomeetriat muudetakse 3D-modelleerijas.
surface-ui-open-in-modeler = Ava 3D-modelleerijas
surface-ui-save-first = Salvesta joonis enne. 3D-modelleerija avab salvestatud faili.
surface-ui-save-changes-first = Salvesta muudatused enne. 3D-modelleerija avab salvestatud faili.
surface-ui-opening-modeler = Joonis avatakse 3D-modelleerijas.
surface-ui-sent-to-modeler = 3D-modelleerija on avatud. Joonis saadeti talle.
surface-ui-modeler-missing = 3D-modelleerija pole paigaldatud.
surface-ui-modeler-failed = 3D-modelleerija ei käivitunud: { $error }
