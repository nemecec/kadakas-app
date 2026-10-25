## io-dialog: io_dialog.rs

io-dialog-dxf-dwg-ifc-shapefile-geojson = DXF, DWG, IFC, Shapefile, GeoJSON
io-dialog-import = Import
io-dialog-dxf-layers-become-design-layers-default-classes = DXF-i kihid muutuvad joonestuskihtideks (vaikimisi: klassideks)
io-dialog-unit = Ühik
io-dialog-from-file = Failist
io-dialog-explode-blocks = Lahuta plokid
io-dialog-import-hatch-boundaries = Impordi viirutuse piirid
io-dialog-only-points-with-a-z-value = Ainult punktid, millel on Z-väärtus
io-dialog-ifc2x3-ifc4-or-ifc4x3 = IFC2x3, IFC4 või IFC4x3
io-dialog-building-storeys-become-design-layers-off-all = Hoone korrused muutuvad joonestuskihtideks (väljas: kõik aktiivsele kihile)
io-dialog-terrain-as-a-3d-mesh-default-a = Maastik 3D-võrguna (vaikimisi: maapinnamudel)
io-dialog-file-crs-if-the-file-has-none = Faili koordinaatsüsteem (kui failil seda pole)
io-dialog-document-crs-default-utm-zone = Dokumendi koordinaatsüsteem (vaikimisi: UTM-tsoon)
io-dialog-class-from-attribute = Klass atribuudist
io-dialog-keep-the-coordinates-of-the-file-default = Säilita faili koordinaadid (vaikimisi: joonis tsentreeritakse)
io-dialog-design-layer = Joonestuskiht
io-dialog-empty-a-new-layer-named-after-the-file = Tühi: uus kiht faili nime järgi
io-dialog-each-dxf-layer-becomes-its-own-layer = Ei kasutata: iga DXF-i kiht saab oma kihi
io-dialog-keep-the-source-for-updates = Säilita allikas (uuenduste jaoks)
io-dialog-cancel = Katkesta
io-dialog-imported-objects-skipped =
    Imporditi { $imported ->
        [one] { $imported } objekt
       *[other] { $imported } objekti
    }, vahele jäeti { $skipped }
io-dialog-import-failed = Import ebaõnnestus: { $message }
io-dialog-export = Eksport
io-dialog-ifc4-ifc4-add2-tc1 = IFC4 (IFC4 ADD2 TC1)
io-dialog-classes-become-ifc-presentation-layers = Klassid muutuvad IFC esituskihtideks
io-dialog-design-layers-become-building-storeys-default-everyt = Joonestuskihid muutuvad hoone korrusteks (vaikimisi: kõik asub ühe ala all)
io-dialog-include-2d-objects-as-annotations = Lisa 2D-objektid märkustena
io-dialog-one-dxf-layer-per-design-layer-default = Üks DXF-i kiht joonestuskihi kohta (vaikimisi: klassi kohta)
io-dialog-only-visible-layers-and-classes = Ainult nähtavad kihid ja klassid
io-dialog-exported-objects-to =
    Eksporditi { $count ->
        [one] { $count } objekt
       *[other] { $count } objekti
    } faili { $path }
io-dialog-export-failed = Eksport ebaõnnestus: { $message }
io-dialog-dwg-needs-a-converter = DWG (vajab teisendajat, vaata docs/dwg.md)

# { $entities } on joonise objektide arv, { $cap } on suurim arv, mida Kadakas uuenduste jaoks hoiab.
io-dialog-base-question = Selles joonises on { $entities } objekti, rohkem kui Kadakas uuenduste jaoks hoiab ({ $cap }). Kas hoida viidet algsele failile uuenduste jaoks või importida ilma uuendusandmeteta? Viide salvestab faili täispika tee, milles võib olla sinu kasutajanimi, ja seda näevad kõik, kellega see dokument on jagatud.
io-dialog-base-question-title = Suur joonis
io-dialog-base-keep-link = Hoia viide alles
io-dialog-base-no-update-data = Impordi ilma uuendusandmeteta
io-dialog-base-question-large = Selle joonise uuendusandmed ({ $entities } objekti) on { $size } MB, rohkem kui Kadakas uuenduste jaoks hoiab ({ $cap } MB). Kas hoida viidet algsele failile uuenduste jaoks või importida ilma uuendusandmeteta? Viide salvestab faili täispika tee, milles võib olla sinu kasutajanimi, ja seda näevad kõik, kellega see dokument on jagatud.
io-dialog-patterns = Ühikumustrid
io-dialog-patterns-hint = Mida kirjutada haljastuskatete sillutiskivide või plaatide mustrist. Ühikud: iga ühik suletud polüjoonena kihtidele <kiht>-UNITS-WHOLE ja <kiht>-UNITS-CUT. Vuugijooned: keskjooned kihile <kiht>-GAPS.
io-dialog-patterns-none = Ainult kontuur
io-dialog-patterns-gaps = Vuugijooned
io-dialog-patterns-units = Ühikud
io-dialog-version = DXF-i versioon
io-dialog-version-hint = Faili versioon. Vaikimisi on 2004. Versioonis 2000 pole RGB-värve. Versioone 2013 ja 2018 on kontrollitud ainult ezdxf-i lugejaga. Versioon 12 sisaldab jooni, ringe, kaari, punkte, teksti ja polüjooni. Aruanne loendab, mis jäi välja.
io-dialog-version-2004 = Versioon 2004 (AC1018, vaikimisi)
io-dialog-version-2000 = Versioon 2000 (AC1015)
io-dialog-version-2013 = Versioon 2013 (AC1027)
io-dialog-version-2018 = Versioon 2018 (AC1032)
io-dialog-version-r12 = Versioon 12 (AC1009, 2D lihtobjektid)
io-dialog-paper-space = Impordi paberiruumi küljendused lehekihtidena
io-dialog-paper-space-hint = Iga sisuga paberiruumi küljendus muutub lehekihiks koos vaateakende, märkuste ja tiitliplokiga. Väljas: paberiruum jäetakse vahele ja aruanne loendab selle.
io-dialog-sheets = Kirjuta lehekihid küljendustena
io-dialog-sheets-hint = Iga lehekiht muutub paberiruumi küljenduseks koos vaateakende ja märkustega. Väljas: lehekihid jäetakse failist välja.
io-dialog-ifczip-filter = IFC pakitud (ifcZIP)
