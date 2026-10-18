## area-ui: area_ui.rs

area-ui-hardscape-style = Kõvakatte stiil
area-ui-none-plain-slab = (puudub: lihtne plaat)
area-ui-slab = Plaat
area-ui-path = Rada
area-ui-width = Laius
area-ui-join = Nurga ühendus: { $join }
area-ui-landscape-area-style = Haljastusala stiil
area-ui-hardscape = Kõvakate
area-ui-style = Stiil
area-ui-mode = Režiim
area-ui-draw-a-path-with-the-hardscape-tool = Joonista rada tööriistaga Kõvakate režiimis Rada
area-ui-slope = Kalle
area-ui-edges-set-by-the-api = servad (seatud API kaudu)
area-ui-height-above-ground = Kõrgus maapinnast
area-ui-elevation = Absoluutkõrgus
area-ui-drape-on-site-model = Aseta maapinnamudelile
area-ui-site-modifier-acts-as-a-pad = Maapinna muutja (toimib tasandina)
area-ui-the-formation-level-under-the-build-up = Katendi aluspinna tase muudab selle kihi maapinnamudeli kavandatud pinda
area-ui-plan-area = Plaanipind
area-ui-draped-area = Maapinnale kantud pind
area-ui-surface-area = Pinna pindala
area-ui-perimeter = Ümbermõõt
area-ui-volume = Ruumala
area-ui-landscape-area = Haljastusala
area-ui-show-plants = Näita taimi
area-ui-hole-s =
    { $count ->
        [one] { $count } auk
       *[other] { $count } auku
    }
area-ui-clear-holes = Eemalda augud
area-ui-net-area = Puhaspind
area-ui-net-area-holes = { $area }  (augud { $holes })
area-ui-plants-percent-density =
    { $count ->
        [one] { $count } taim
       *[other] { $count } taime
    }  ({ $percent } %, { $density })
area-ui-pattern-origin = Muster algab punktis { $x }, { $y }
area-ui-pattern-origin-middle = Muster algab ala keskelt
area-ui-clear-origin = Kasuta keskpunkti
area-ui-pattern-angle = Mustri nurk
area-ui-set-angle = Sea nurk
area-ui-clear-angle = Kasuta stiili nurka
joint-ui-title = Vuugimuster: { $name }
joint-ui-bond-running-bond = Poolkivisidus
joint-ui-bond-stack = Ridasidus
joint-ui-bond-herringbone = Kalaluumuster
joint-ui-bond-basket-weave = Korvmuster
joint-ui-bond-module = Moodul (oma kujud)
joint-ui-bond-generator = Generaator (skript)
joint-ui-unit-length = Kivi pikkus
joint-ui-unit-width = Kivi laius
joint-ui-gap = Vuugi laius
joint-ui-angle = Nurk
joint-ui-preset-hexagon = Kuusnurk
joint-ui-preset-octagon-square = Kaheksanurk ja ruut
joint-ui-preset-i-shape = I-kujuline sillutiskivi
joint-ui-from-selection = Valikust
joint-ui-side = Külg
joint-ui-across-flats = Külgede vahe
joint-ui-notch = Sisselõige
joint-ui-shapes =
    { $count ->
        [one] { $count } kuju joonisest
       *[other] { $count } kuju joonisest
    }
joint-ui-save = Salvesta
joint-ui-close = Sulge

## area-ui: plan_draw.rs (the pattern plan on the canvas)

area-ui-show-pattern-check = Näita mustri kontrolli plaanil
area-ui-the-check-marks-overlaps-small-pieces-wrong-joints-and-holes = Märgib unit_pattern.check leiud vaikesätetega (mitte varasema kutse sätetega): kattuvused, väikesed tükid, valed vuugid ja augud katvuses. Igast liigist märgitakse kuni 200; silt ütleb, kui palju neid on
plan-draw-preview = eelvaade: suumi sisse, et näha tegelikku mustrit
plan-draw-preview-too-fine = { $label } (kivid on liiga peenikesed: mõned vuugid jäetakse välja)
plan-draw-not-drawn = Mustrit ei joonistata: { $reason }
plan-draw-check-failed = Mustri kontroll ebaõnnestus: { $reason }
plan-draw-hidden = muster peidetud sellel suumil (kivid < 3 px)
plan-draw-updating = muster uueneb...
plan-draw-laying = muster paigutatakse...
plan-draw-checking = mustrit kontrollitakse...
plan-draw-check-summary = Mustri kontroll (vaikesätted): { $total } leidu, { $listed } märgitud
plan-draw-check-summary-cut = Mustri kontroll (vaikesätted): { $total } leidu, { $listed } märgitud (liikide { $kinds } nimekiri on lõigatud 200 juures iga liigi kohta)
plan-draw-checking-pauses = mustrit kontrollitakse: leht on hetkeks peatatud...
plan-draw-no-joint-pattern = Objektil pole vuugimustrit.
