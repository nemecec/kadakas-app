## meshio-ui: võrkfailide import ja eksport (mesh_import.rs, io_dialog.rs, interop.rs)

meshio-ui-file-filter = 3D-võrgud: OBJ, STL, glTF, GLB
meshio-ui-all-filter = Kõik 3D-failid: OBJ, STL, glTF, GLB, IFC
meshio-ui-unit = Faili ühik
meshio-ui-unit-default = { $unit } (selle vormingu vaikimisi)
meshio-ui-up-axis = Faili ülestelg
meshio-ui-up-default-y = Y üles (selle vormingu vaikimisi)
meshio-ui-up-default-z = Z üles (selle vormingu vaikimisi)
meshio-ui-up-y = Y üles
meshio-ui-up-z = Z üles
meshio-ui-gltf-fixed = glTF-failid on meetrites ja Y on üles.
meshio-ui-center = Nihuta mudel alguspunkti
meshio-ui-soften-question = Kas pehmendada servad alla teatud nurga?
meshio-ui-soften-angle = Nurk (kraadi)
meshio-ui-soften-hint = Võrgust saab pinnamudel ja väiksema nurgaga tahkude vahelised servad muutuvad pehmeks. Väljas: faili servad jäävad nagu on ja võrk jääb võrguks.
meshio-ui-preview-counting = Loen servi...
meshio-ui-preview =
    { $softened ->
        [one] { $softened } serv
       *[other] { $softened } serva
    } { $edges }-st pehmendataks ({ $faces } tahku).
meshio-ui-preview-too-large = Võrk on pinnamudeli jaoks liiga suur ({ $reason }). See jääb võrguks ja ühtegi serva ei pehmendata.
meshio-ui-preview-not-convertible = Võrgust ei saa pinnamudelit ({ $reason }). See jääb võrguks ja ühtegi serva ei pehmendata.
meshio-ui-preview-skipped = Fail on suur ({ $megabytes } MB, servad loetakse kuni { $limit } MB). Import pehmendab servad ikkagi.
meshio-ui-preview-failed = Faili ei saa lugeda: { $message }
meshio-ui-imported =
    Imporditud { $count ->
        [one] { $count } objekt
       *[other] { $count } objekti
    }, { $triangles } kolmnurka, { $softened ->
        [one] { $softened } serv
       *[other] { $softened } serva
    } pehmendatud
meshio-ui-ifc-soften-hint = Faili võrgud muutuvad pinnamudeliteks seal, kus need mahuvad.
meshio-ui-export = Ekspordi
meshio-ui-export-ascii = ASCII-tekst (vaikimisi: kahendfail)
meshio-ui-export-stl-hint = STL sisaldab ainult kolmnurki, ilma värvide ja joonteta.
meshio-ui-export-ifc-hint = IFC4. Suletud pinnamudelid saavad kehadeks, avatud mudelid kestadeks.
meshio-ui-export-dxf-hint = DXF. Pinnamudelid kirjutatakse polyface-võrkudena.
meshio-ui-export-obj-hint = OBJ koos MTL-failiga, meetrites ja Y üles.
meshio-ui-export-gltf-hint = glTF 2.0, meetrites ja Y üles.
