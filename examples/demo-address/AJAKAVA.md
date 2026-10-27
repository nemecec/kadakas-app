# Ajakava: aadressist mudelini, 10 kuni 12 minutit

Sihtrühm: ehitusinsenerid, maastikuarhitektid ja ehitusarhitektid. Teema: tehisintellekti agendi võimalused Kadakases.

Selles failis on aadress asendatud kohatäitjaga `<aadress>` ja vahemälu kaust kohatäitjaga `<vahemälu>`. Isiklik ajakava päris aadressidega on repositooriumist väljas.

## Ausalt alguses

Ütle esimesena: „See on skriptitud agendi voog. Pythoni skript kasutab sama agendi API-t, mida keelemudel kasutab MCP kaudu. Täna on sammud ette kirjutatud, et demo oleks korratav. Iga muudatus on logis päritoluga „agent“.“

Kui töötad ilma võrguta, lisa: „Andmed laadisin eile avalikest registritest alla.“

## Enne esinemist (15 minutit)

1. Ehita rakendus: `cargo build --release -p kadakas-app -p kadakas-cli`.
2. Soojenda vahemälu iga aadressiga, mida näitad, võrguga: `python3 demo_address.py "<aadress>" --cache-dir <vahemälu> --mode headless --pause 0 --yes --extras-budget 150`
3. Kontrolli ilma võrguta: sama käsk lisavõtmega `--offline`.
4. Käivita rakendus: `kadakas-app --agent-port 0 --allow-dir <vahemälu>`
5. Vali rakenduses Fail > Uus. Ava agendi tegevuse paneel (Fail > Agent Activity, paneeli nimi on inglise keeles). Pane aken suureks ja konsool selle kõrvale.
6. Kui võrk on kehv, käivita kõik käsud võtmega `--offline`.

Käsk esinemiseks: `python3 demo_address.py "<aadress>" --cache-dir <vahemälu> --mode app --pause enter`

Skript ütleb valjusti, kui ta ei ole rakendusega ühendatud. Siis jäta esinemine pooleli ja käivita rakendus.

## Kava

Konsool prindib 12 sammu. Kava on samas järjekorras.

| Aeg | Konsooli samm | Mida ütled | Mida teed | Mida publik näeb |
|---|---|---|---|---|
| 0:00 | Sissejuhatus | Ausalt alguses olev lõik. „Inimene vaatab iga muudatuse üle.“ | Näita tühja Kadakast ja konsooli. | Tühi dokument. |
| 1:00 | 1. Aadressi otsing | „Kirjutan aadressi. Agent otsib selle Maa- ja Ruumiameti registrist.“ | Käivita käsk. Kui vasteid on mitu, vali number. | Mitu vastet, parim esimesena. |
| 2:00 | 2. Hoone andmed | „Ehitisregister annab kontuuri, kõrguse ja korrused. Kui kõrgus puudub, ütleb agent seda valjusti.“ | Vajuta Enter. | Eestikeelsed teated, vajadusel „Kõrgus puudub registrist, eeldan ...“. |
| 3:00 | 3. Georeferents ja taustakaart | „Dokument saab koordinaadid L-EST97 (EPSG:3301). Taustaks on Maa- ja Ruumiameti ortofoto.“ | Vajuta Enter. | Kihid ja ortofoto aknas. |
| 4:00 | 4. Hoone mudel | „Kontuurist sai hoonemaht. Atribuudid on hoonel: EHR-kood, korrused, kõrgus, allikas.“ | Vajuta Enter. Vali hoone ja ava Objekti info. | 3D hoonemaht, atribuudid, aadressisilt. |
| 5:00 | 5. Krunt, 6. Maapind ja mullatööd | „Insenerile: krundi piir, maapinna mudel ja mullatööde mahud.“ | Vajuta Enter kaks korda. | Krundi piir, maapinna mudel, kaeve ja täite m3. Ilma allikata ütleb agent, et kasutab eeldatud piiri. |
| 6:00 | 7. Ümbrus | „Naabrid on hallid. Neid ei muuda keegi, need on kontekst.“ | Vajuta Enter. | Naabrihooned ja puud eraldi kihtidel. |
| 7:00 | 8. Õuekontseptsioon | „Maastikuarhitektile: agent pakub terrassi, tee, taimed ja taimede nimekirja.“ | Vajuta Enter. Ava taimede nimekiri. | Kõvakatted, puud, põõsad, hekk, nimekiri. |
| 8:00 | 9. Laiendus | „Ehitusarhitektile: agent pakub laienduse mahu kõige vabamale küljele. See on eraldi kihil ja üks samm.“ | Vajuta Enter. | Oranž laiendus, kiht „Laiendus (agent)“. |
| 9:00 | 10. Allikad ja eeldused, 11. Eksport | „Allikad ja eeldused on mudelis. Valmis fail läheb edasi: IFC koos georeferentsiga, PDF ja SVG leht.“ | Vajuta Enter kaks korda. Näita kausta `<vahemälu>/out`. | Aruande tekst, IFC, PDF, SVG, Kadakase fail. |
| 10:00 | 12. Ülevaatus | „See on kõige olulisem. Iga agendi samm on logis. Inimene võtab selle vastu või tühistab.“ | Oota teadet „Valmis.“ Alles siis ava agendi tegevuse paneel, vali laienduse samm ja vajuta Revert. | Esiletõstetud laiendus, laiendus kaob. |
| 11:00 | Küsimused | „Agent kogus andmed ja tegi ettepaneku. Otsused teeb inimene.“ | Ava küsimused. | |

Revert alles pärast teadet „Valmis.“, sest enne seda kirjutab skript veel dokumenti. Pärast Revertit ei vasta eksporditud failid enam mudelile. Ütle seda, kui keegi küsib.

Revert töötab ainult laienduse sammul. See on üks samm, sest agent tegi selle ühe käsuga. Hoonel on atribuudid, seega kasuta hoone puhul käsku Undo to here.

## Kui midagi läheb valesti

| Probleem | Mida teha |
|---|---|
| Võrku pole või teenus ei vasta | Käivita sama käsk võtmega `--offline`. Vahemälu peab olema soojendatud. |
| „Võrguta režiim: ... pole vahemälus“ | Sellel aadressil pole eelsoojendust. Vali aadress, mille oled soojendanud. |
| „EI OLE RAKENDUSEGA ÜHENDATUD“ või „rakendus ei vasta“ | Käivita rakendus võtmega `--agent-port 0`. Hädaabi: `--mode headless` ja näita pilte kaustast `<vahemälu>/out`. |
| Eksport keelatud („outside the folders“) | Käivita rakendus uuesti võtmega `--allow-dir <vahemälu>`. |
| Dokumendis on vana sisu | Vali Fail > Uus ja käivita uuesti. Skript hoiatab selle kohta. |
| Vale aadress | Käivita uuesti võtmega `--pick N`. |
| Kõrgus puudub | See on normaalne. Ütle: „Register ei tea kõrgust. Agent ütleb seda valjusti ja märgib eelduse.“ |
| Samm annab teate „VIGA sammus ...“ | Skript jätkab, kui hilisemad sammud sellest ei sõltu. Ütle: „Agent teatab veast, mudel on alles.“ |

## Kolm küsimust, mida publik tõenäoliselt küsib

**1. Kas kõrgus on täpne?**
Ei ole alati. Register ei anna sageli kõrgust. Siis kasutab agent dokumenteeritud eeldust ja märgib selle hoone atribuutidesse. Täpse kõrguse annab mõõdistus.

**2. Kus on maa-alused kommunikatsioonid?**
Neid mudelis pole. Need andmed ei ole avalikud. Neid tuleb küsida võrguettevõtjatelt. Agent ei hakka neid arvama.

**3. Kas see on projekt?**
Ei ole. Mudel on mahumudel ja kontseptsioon. Õueplaan ja laiendus on ettepanekud. Iga agendi muudatuse vaatab inimene üle ja saab selle tagasi võtta.

Võimalik lisaküsimus: kas agent muudab kunagi minu faili salaja? Ei muuda. Kõik muudatused on logis ja neil on päritolu „agent“.

## Piirangud, mida ausalt öelda

- Mahumudel, mitte projekt.
- Registri kõrgus ja mõned väljad puuduvad sageli.
- Maa-aluseid kommunikatsioone pole.
- Krundi piir, maapind ja ümbrus tulevad valikuliste allikatena. Kui neid pole, kasutab agent eeldatud piiri ja ütleb seda.
- Andmed on Maa- ja Ruumiameti ja ehitisregistri avalikud andmed. Allikad on mudeli aruandes.
- Sammud on ette kirjutatud. See on skriptitud demo, mitte vaba vestlus.
