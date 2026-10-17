## Texts of the paving logic (kadakas-ui-core): regions from shapes, zones, courses, the pattern check.

uic-paving-make-hardscape = Kõvakate
uic-paving-make-landscape-area = Haljasala
uic-paving-make-polyline = Suletud polüjoon

uic-paving-role-area = Ala
uic-paving-role-island = Saar (ala auk)
uic-paving-role-doubtful = Ala või saar? Kontrolli
uic-paving-note-gaps =
    { $count ->
        [one] { $count } vahe suleti (otsad liikusid kokku { $total })
       *[other] { $count } vahet suleti (otsad liikusid kokku { $total })
    }
uic-paving-note-gap = Siin suleti { $size } vahe
uic-paving-note-open-ends =
    { $count ->
        [one] { $count } kuju ei sulgu. Selle taga olevat ala ei looda
       *[other] { $count } kuju ei sulgu. Nende taga olevaid alasid ei looda
    }
uic-paving-note-stubs =
    { $count ->
        [one] { $count } joon ulatub nurgast üle või lõpeb ristumiskohas (kahjutu)
       *[other] { $count } joont ulatuvad nurgast üle või lõpevad ristumiskohas (kahjutu)
    }
uic-paving-note-dead-end = Siin on { $length } pikkune lahtine ots
uic-paving-note-dead-end-near = Siin on { $length } pikkune lahtine ots. Lähim teine ots on { $distance } kaugusel
uic-paving-note-short =
    { $count ->
        [one] { $count } kõver oli tolerantsist lühem ja jäeti välja
       *[other] { $count } kõverat olid tolerantsist lühemad ja jäeti välja
    }
uic-paving-note-duplicates =
    { $count ->
        [one] { $count } kõver oli teise peal ja liideti sellega
       *[other] { $count } kõverat olid teiste peal ja liideti nendega
    }
uic-paving-note-slivers =
    { $count ->
        [one] { $count } kitsas riba jäeti välja
       *[other] { $count } kitsast riba jäeti välja
    }
uic-paving-note-skipped = Objekti ei kasutatud: { $reason }
uic-paving-note-api = Märkus: { $text }
uic-paving-no-shapes = Vali kõigepealt jooned, kaared, polüjooned või muud kujud.
uic-paving-bad-tolerance = Tolerants peab olema üle 0 ja kuni 1000 mm.
uic-paving-no-regions = Ükski piirkond pole sisse lülitatud.

uic-paving-no-object = Objekti pole enam olemas.
uic-paving-not-a-hardscape = Vali kõvakate.
uic-paving-no-pattern = Kõvakattel pole vuugimustrit. Anna sellele kõigepealt vuukidega stiil.
uic-paving-no-lines = Joonista või vali vähemalt üks jagamisjoon.

uic-paving-along-outline = Kontuur
uic-paving-along-hole = Auk { $number }
uic-paving-placing-paving = Sillutise poolel
uic-paving-placing-opposite = Teisel pool
uic-paving-placing-centered = Serva keskel
uic-paving-orientation-soldier = Püstkivid (pikk külg risti)
uic-paving-orientation-stretcher = Pikikivid (pikk külg piki)
uic-paving-corner-miter = Kaldlõige
uic-paving-corner-butt = Külgsulg
uic-paving-corner-unit = Nurgakivi
uic-paving-course-name = Rida { $number }
uic-paving-course-not-editable = See rida järgib joonistatud joont või oma rada. Eemalda see siin ja tee uuesti API-ga või muuda seda seal.
uic-paving-course-needs-name = Reale on vaja nime.

uic-paving-kind-overlap = Kattuvus
uic-paving-kind-joint-narrow = Vuuk liiga kitsas
uic-paving-kind-joint-wide = Vuuk liiga lai
uic-paving-kind-small-piece = Väike tükk
uic-paving-kind-wrong-size = Vale suurus
uic-paving-kind-outside = Väljaspool ala
uic-paving-kind-coverage = Katvus
uic-paving-check-nothing = Kontrollimiseks pole ühtegi vuugimustriga kõvakatet.
uic-paving-bad-reply = Programmi vastust ei saanud lugeda.
