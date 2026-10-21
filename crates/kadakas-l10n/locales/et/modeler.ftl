## modeler: the kadakas-modeler program

modeler-window-title = Kadakas Modelleerija
modeler-title = { $name }{ $changed } – { $app }
modeler-untitled = Nimetu
modeler-needs-wgpu = 3D-vaade vajab GPU-joonistajat (wgpu), mida sellel arvutil ei ole.

modeler-menu-file = Fail
modeler-menu-edit = Redigeerimine
modeler-menu-camera = Kaamera
modeler-menu-tools = Tööriistad
modeler-menu-view = Vaade
modeler-menu-help = Abi

modeler-file-new = Uus
modeler-file-open = Ava...
modeler-file-save = Salvesta
modeler-file-save-as = Salvesta nimega...
modeler-file-quit = Välju
modeler-edit-undo = Võta tagasi
modeler-edit-redo = Tee uuesti
modeler-edit-select-all = Vali kõik
modeler-edit-select-none = Tühista valik
modeler-edit-make-group = Tee grupp
modeler-edit-make-component = Tee komponent
modeler-edit-explode = Lahuta
modeler-edit-make-unique = Tee ainulaadseks
modeler-edit-edit-selected = Muuda gruppi või komponenti
modeler-edit-close-context = Sulge grupp või komponent
modeler-edit-close-keep-mine = Sule komponent, jäta minu muudatus uue komponendina
modeler-edit-close-discard-mine = Sule komponent, loobu minu muudatusest
modeler-edit-close-overwrite = Sule komponent, asenda see minu muudatusega
modeler-component-name = Komponent { $n }
modeler-nothing-selected = Vali kõigepealt midagi.
modeler-select-one-to-edit = Vali muutmiseks üks grupp või komponent.
modeler-help-about = Kadakas Modelleerija info

modeler-camera-previous = Eelmine
modeler-camera-next = Järgmine
modeler-camera-standard-views = Standardvaated
modeler-camera-perspective = Perspektiiv
modeler-camera-parallel = Paralleelprojektsioon
modeler-camera-fov = Vaatenurk
modeler-camera-fov-parallel = Paralleelprojektsioonil ei ole vaatenurka.
modeler-camera-zoom-extents = Suumi kõik
modeler-camera-zoom-window = Suumi aknasse
modeler-view-top = Ülalt
modeler-view-bottom = Alt
modeler-view-front = Eest
modeler-view-back = Tagant
modeler-view-left = Vasakult
modeler-view-right = Paremalt
modeler-view-iso = Isomeetria
modeler-view-tray = Külgpaneel
modeler-view-theme = Teema
modeler-view-language = Keel
modeler-language-system = Süsteemi keel

modeler-tray-entity-info = Olemi info
modeler-tray-materials = Materjalid
modeler-tray-scenes = Stseenid
modeler-entity-nothing = Midagi ei ole valitud.
modeler-entity-active-layer = Aktiivne kiht
modeler-entity-active-class = Aktiivne klass
modeler-entity-type = Tüüp
modeler-entity-id = Tunnus
modeler-entity-layer = Kiht
modeler-entity-class = Klass
modeler-entity-size = Suurus
modeler-entity-corner = Madalaim nurk
modeler-entity-many =
    { $count ->
        [one] { $count } objekt valitud.
       *[other] { $count } objekti valitud.
    }
modeler-materials-later = Materjalid tulevad värvikopa tööriistaga, hilisemas versioonis.
modeler-scenes-later = Stseenid (salvestatud vaated) tulevad hilisemas versioonis.

modeler-tool-later = { $name }: { $note }
modeler-nothing-to-undo = Tagasivõtta ei ole midagi.
modeler-nothing-to-redo = Uuesti teha ei ole midagi.

modeler-opened = Avatud { $name }
modeler-opened-notes = Avatud { $name }. { $notes }
modeler-saved = Salvestatud { $name }
modeler-saved-notes = Salvestatud { $name }. { $notes }
modeler-open-error = Faili { $path } ei saa avada: { $error }
modeler-open-failed = Faili { $path } ei saa avada.
modeler-open-cancelled = Faili ei avatud.
modeler-open-big-title = See fail on suur
modeler-save-error = Faili { $path } ei saa salvestada: { $error }
modeler-save-failed = Faili { $path } ei saa salvestada.
modeler-save-cancelled = Faili ei salvestatud.
modeler-save-overwrite-title = Kas asendada fail?
modeler-save-overwrite-text = Mõni teine programm muutis faili { $file } pärast selle avamist. Kas asendada see selle mudeliga?
modeler-save-missing-title = Faili osad puuduvad
modeler-save-missing-text = Failis { $file } ei ole enam osi, millele see mudel viitab: { $what } Kas salvestada ilma nendeta?
modeler-prefs-ignored = Seadistuste faili { $path } ei arvestata: { $error }
modeler-prefs-cannot-write = Seadistuste faili { $path } ei saa kirjutada: { $error }

modeler-about-title = Kadakas Modelleerija info
modeler-about-description = 3D-keskne modelleerija tuttavate modelleerimisprogrammide paigutuse ja klahvidega. See avab ja salvestab samu .kad faile nagu Kadakas.
modeler-about-trademark = SketchUp on ettevõtte Trimble Inc. kaubamärk. Kadakas Modelleerija on sõltumatu projekt ning Trimble Inc. ei ole sellega seotud, ei toeta ega sponsoreeri seda. Nimi SketchUpi-sarnane ütleb vaid, millised on klahvid ja paigutus.

modeler-unsaved-title = Salvestamata muudatused
modeler-unsaved-open = Sul on salvestamata muudatusi. Kas salvestada need enne faili { $name } avamist?
modeler-unsaved-new = Sul on salvestamata muudatusi. Kas salvestada need enne uue mudeli alustamist?
modeler-unsaved-quit = Sul on salvestamata muudatusi. Kas salvestada need enne väljumist?
modeler-unsaved-save-and-open = Salvesta ja ava
modeler-unsaved-save-and-new = Salvesta ja alusta
modeler-unsaved-save-and-quit = Salvesta ja välju
modeler-unsaved-dont-save = Ära salvesta
modeler-unsaved-cancel = Loobu
modeler-open-new-model = uus mudel
modeler-open-quit = väljumine
modeler-open-dropped = Jäi tegemata: { $dropped }. Kadakas Modelleerija ootab esmalt sinu vastust küsimusele: { $waiting }.
modeler-open-not-saved = Muudatusi ei saanud salvestada, seega { $name } jäi tegemata ja mudel jääb alles.
modeler-open-cancelled-by-you = { $name } katkestati. Sinu mudel on muutmata.
modeler-opened-first-of-several = Avatud { $name }. Veel { $count } faili saadeti ja neid ei avatud.
