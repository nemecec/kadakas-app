# Ajakava: aadressist mudelini, 10 kuni 12 minutit

Sihtrühm: ehitusinsenerid, maastikuarhitektid ja ehitusarhitektid. Teema: tehisintellekti agendi võimalused Kadakases.

Selles failis on aadress asendatud kohatäitjaga `<aadress>`. Isiklik ajakava päris aadressidega on repositooriumist väljas.

## Enne esinemist (15 minutit)

1. Ehita rakendus: `cargo build --release -p kadakas-app -p kadakas-cli`.
2. Käivita rakendus kaustast `examples/demo-address`: `kadakas-app --agent-port 0 --allow-dir .`
3. Soojenda vahemälu iga aadressiga, mida näitad: `python3 demo_address.py "<aadress>" --mode headless --pause 0 --yes`
4. Kontrolli ilma võrguta: sama käsk lisavõtmega `--offline`.
5. Vali rakenduses Fail > Uus. Ava paneel Fail > Agent Activity. Pane aken kogu ekraanile ja konsool selle kõrvale.
6. Kui võrk on kehv, jäta kõik käsud võtmega `--offline`. Publik ei näe vahet.

Käsk esinemiseks: `python3 demo_address.py "<aadress>" --pause enter`

## Kava

| Aeg | Samm | Mida ütled | Mida teed | Mida publik näeb |
|---|---|---|---|---|
| 0:00 | Sissejuhatus | „Agent ei joonista ise. Ta toob avalikud andmed ja teeb mudeli. Inimene vaatab iga muudatuse üle.“ | Näita tühja Kadakast ja konsooli. | Tühi dokument. |
| 1:00 | 1. Aadress | „Kirjutan aadressi. Agent otsib selle Maa- ja Ruumiameti registrist.“ | Käivita käsk. Kui vasteid on mitu, vali number. | Mitu vastet, parim esimesena. Agent küsib kinnitust. |
| 2:00 | 2. Hoone andmed | „Ehitisregister annab kontuuri, kõrguse ja korrused. Vaata, mida agent ütleb, kui kõrgus puudub.“ | Vajuta Enter. | Eestikeelsed teated, vajadusel „Kõrgus puudub registrist, eeldan ...“. |
| 3:00 | 3. Georeferents ja kaart | „Dokument saab koordinaadid L-EST97 (EPSG:3301). Taustaks on Maa- ja Ruumiameti ortofoto.“ | Vajuta Enter. | Kihid ja ortofoto aknas. |
| 4:00 | 4. Hoone | „Kontuurist on tõmmatud mass. Atribuudid on hoonel: EHR kood, korrused, kõrgus, allikas.“ | Vajuta Enter. Vali hoone ja ava Object Info. | 3D mass, atribuudid, aadressisilt. |
| 5:00 | 5. Krunt ja maapind | „Insenerile: krundi piir, maapinna mudel ja mullatööde mahud.“ | Vajuta Enter. | Krundi piir. Kui allikas on olemas, siis maapind ja kaevamise ning täite m3. |
| 6:00 | 6. Maastikuarhitekt | „Agent pakub õuekontseptsiooni: terrass, tee, taimed ja taimede nimekiri.“ | Vajuta Enter. Ava taimede nimekiri. | Kõvakatted, puud, põõsad, heki read, nimekiri. |
| 7:30 | 7. Ehitusarhitekt | „Agent pakub laienduse massi kõige vabamale küljele. See on eraldi kihil.“ | Vajuta Enter. | Oranž laiendus, kihi nimi „Laiendus (agent)“. |
| 8:30 | 8. Ülevaatus | „See on kõige olulisem. Iga agendi samm on logis. Inimene võtab selle vastu või tühistab.“ | Ava Fail > Agent Activity. Vali laiendus ja vajuta Revert. | Esiletõstetud muudatused, Revert, mudel muutub. |
| 10:00 | 9. Eksport | „Valmis fail läheb edasi: IFC koos georeferentsiga, PDF ja SVG leht.“ | Näita väljundkausta `.cache/out`. | IFC, PDF, SVG, Kadakase fail. |
| 11:00 | Kokkuvõte | „Agent kogus andmed ja tegi ettepaneku. Otsused teeb inimene.“ | Ava küsimused. | |

Konsool jätkab sammude vahel kolme sekundi pausiga, kui kasutad `--pause 3`. Käsitsi pausi jaoks kasuta `--pause enter`.

## Kui midagi läheb valesti

| Probleem | Mida teha |
|---|---|
| Võrku pole või teenus ei vasta | Käivita sama käsk võtmega `--offline`. Vahemälu peab olema soojendatud. |
| „Võrguta režiim: ... pole vahemälus“ | Sellel aadressil pole eelsoojendust. Vali aadress, mille oled soojendanud. |
| Rakendus ei vasta | Kontrolli, et rakendus töötab võtmega `--agent-port 0`. Hädaabi: `--mode headless` ja näita pilte kaustast `.cache/out`. |
| Eksport keelatud („outside the folders“) | Käivita rakendus uuesti võtmega `--allow-dir .` kaustast `examples/demo-address`. |
| Dokumendis on vana sisu | Vali Fail > Uus ja käivita uuesti. |
| Vale aadress | Käivita uuesti võtmega `--pick N`. |
| Kõrgus puudub | See on normaalne. Ütle: „Register ei tea kõrgust. Agent ütleb seda valjusti ja märgib eelduse.“ |

## Kolm küsimust, mida publik tõenäoliselt küsib

**1. Kas kõrgus on täpne?**
Ei ole alati. Register ei anna sageli kõrgust. Siis kasutab agent dokumenteeritud eeldust ja märgib selle hoone atribuutidesse. Täpse kõrguse annab mõõdistus.

**2. Kus on maa-alused kommunikatsioonid?**
Neid mudelis pole. Need andmed ei ole avalikud. Neid tuleb küsida võrguettevõtjatelt. Agent ei hakka neid arvama.

**3. Kas see on projekt?**
Ei ole. Mudel on mass ja kontseptsioon. Õueplaan ja laiendus on ettepanekud. Iga agendi muudatuse vaatab inimene üle ja saab selle tagasi võtta.

Võimalik lisaküsimus: kas agent muudab kunagi minu faili salaja? Ei muuda. Kõik muudatused on logis ja neil on päritolu „agent“.

## Piirangud, mida ütle ausalt

- Mass, mitte projekt.
- Registri kõrgus ja mõned väljad puuduvad sageli.
- Maa-aluseid kommunikatsioone pole.
- Krundi piir ja maapind tulevad valikuliste allikatena. Kui neid pole, kasutab agent eeldatud piiri ja ütleb seda.
- Andmed on Maa- ja Ruumiameti ja ehitisregistri avalikud andmed. Allikad on mudeli aruandes.
