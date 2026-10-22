## modeler-tags: siltide paneel, peitmine, abijooned ja mõõdulint 3D-modelleerijas (MOD-05)

modeler-tray-tags = Sildid
modeler-tags-untagged = Sildita
modeler-tags-layers = Kihid
modeler-tags-new = Uus
modeler-tags-rename = Nimeta ümber
modeler-tags-delete = Kustuta
modeler-tags-show-all = Näita kõiki
modeler-tags-eye-hide = Peida see silt
modeler-tags-eye-show = Näita seda silti
modeler-layers-eye-hide = Peida see kiht
modeler-layers-eye-show = Näita seda kihti
modeler-tags-active = Tee see aktiivseks sildiks. Uus geomeetria saab selle sildi.
modeler-layers-active = Tee see aktiivseks kihiks. Uus geomeetria läheb sellele kihile.
modeler-tags-objects = { $count ->
    [one] 1 objekt
   *[other] { $count } objekti
}
modeler-tags-delete-title = Kustuta silt
modeler-tags-delete-objects = { $count ->
    [one] 1 objektil on silt { $name }. See läheb sildita objektide hulka.
   *[other] { $count } objektil on silt { $name }. Need lähevad sildita objektide hulka.
}
modeler-tags-delete-others = Sildita hulka läheb ka: { $guides } abijoont, { $parts } definitsioonide osa, { $overrides } vaateakna alistust ja märkust, { $views } salvestatud vaadet. Sildi { $slots } materjalipesa muutub selle objektide materjalipesadeks.
modeler-tags-delete-ask = Kas kustutada silt { $name }?
modeler-tags-delete-yes = Kustuta silt
modeler-tags-cancel = Loobu
modeler-tags-deleted = { $count ->
    [one] Silt { $name } on kustutatud. 1 objekt läks sildita objektide hulka.
   *[other] Silt { $name } on kustutatud. { $count } objekti läks sildita objektide hulka.
}
modeler-tags-deleted-empty = Silt { $name } on kustutatud.
modeler-tags-name-empty = Sildil peab olema nimi.
modeler-tags-name-taken = Teisel sildil on see nimi juba olemas.
modeler-tags-name-untagged = Sildita on vaikesilt. Selle nimi jääb samaks ja seda ei saa kustutada.
modeler-tags-name-missing = Silt ei ole enam mudelis.
modeler-tags-pick-one = Klõpsa kõigepealt loendis sildil.
modeler-tags-hidden-active = Aktiivne silt on peidetud. Uus geomeetria ei paista.
modeler-tags-entity = Silt
modeler-tags-mixed = Segatud
modeler-tags-changed = { $count ->
    [one] 1 objektil on nüüd silt { $name }.
   *[other] { $count } objektil on nüüd silt { $name }.
}
modeler-tags-failed = Silti ei muudetud: { $reason }
modeler-edit-hide = Peida
modeler-edit-unhide = Too nähtavale
modeler-edit-unhide-all = Too kõik nähtavale
modeler-edit-delete-guides = Kustuta abijooned
modeler-view-guides = Abijooned
modeler-view-hidden-geometry = Peidetud geomeetria
modeler-view-hidden-objects = Peidetud objektid
modeler-hide-nothing = Vali kõigepealt objektid, tahud või servad.
modeler-unhide-nothing = Peidetud asju pole valitud. Peidetud asjade valimiseks lülita vaate menüüs sisse peidetud geomeetria või peidetud objektid.
modeler-unhide-all-nothing = Midagi pole peidetud.
modeler-hid = { $count ->
    [one] 1 asi on peidetud. Käsk Too kõik nähtavale toob selle tagasi.
   *[other] { $count } asja on peidetud. Käsk Too kõik nähtavale toob need tagasi.
}
modeler-unhid = { $count ->
    [one] 1 asi paistab jälle.
   *[other] { $count } asja paistab jälle.
}
modeler-hide-failed = Peitmine ei õnnestunud: { $reason }
modeler-guides-none = Abijooni ei ole.
modeler-guides-deleted = { $count ->
    [one] 1 abijoon on kustutatud.
   *[other] { $count } abijoont on kustutatud.
}
modeler-guides-delete-title = Kustuta abijooned
modeler-guides-delete-question = { $count ->
    [one] Kas kustutada mudeli abijoon?
   *[other] Kas kustutada mudeli kõik { $count } abijoont?
}
modeler-guides-delete-yes = Kustuta abijooned
modeler-guides-failed = Abijooni ei kustutatud: { $reason }
modeler-tape-distance = Kaugus: { $value }
modeler-tape-guides = Abijooned tehakse. Ctrl ainult mõõdab.
modeler-tape-measure-only = Ainult mõõtmine. Ctrl teeb abijooni.
modeler-resize-title = Kas muuta mudeli suurust?
modeler-resize-question = Sa mõõtsid { $measured } ja trükkisid { $typed }. Kas skaleerida selle taseme kõik objektid teguriga { $factor } esimese punkti ümber?
modeler-resize-yes = Jah
modeler-resize-no = Ei
modeler-resize-done = Mudeli suurust muudeti teguriga { $factor }.
modeler-resize-failed = Mudeli suurust ei muudetud: { $reason }
modeler-resize-nothing = Pole midagi suuruse muutmiseks.
modeler-tags-parts-hint = Sildid kuuluvad objektidele. Vali objekt, et muuta selle silti. Tahud säilitavad oma mudeli sildi.
