## crash-ui: kokkujooksmise aruanded (PLAT-20): küsimus pärast kokkujooksmist, Eelistused > Privaatsus ja käivitaja märkus (crash_ui.rs, privacy_ui.rs)

crash-ui-title = Kadakas peatus ootamatult
crash-ui-counter = Kokkujooksmise aruanne { $n } / { $total }
crash-ui-intro = Eelmisel korral peatus Kadakas vea tõttu. Kokkujooksmise aruanne aitab arendajatel põhjuse leida ja parandada. Midagi pole veel saadetud. Otsustad sina.
crash-ui-what = Aruandes on versioon, süsteem, graafikakaart, viga, tehniline jälg, viimased logiread, viimaste käskude nimed (mitte nende üksikasjad), mälukasutus ja tööaeg. Failinimed, kaustad, kasutajanimed, e-posti aadressid, tunnused ja sinu dokumentide sisu on eemaldatud.
crash-ui-summary = Kadakas { $version } ({ $channel }), { $kind }. Kokkujooksmise allkiri { $signature }.
crash-ui-kind-panic = programmi viga
crash-ui-kind-native = kokkujooksmine süsteemis, draiveris või teegis
crash-ui-note-label = Mida sa tegid? (valikuline)
crash-ui-contact-label = Kontakt, kui tohime sinult küsida (valikuline)
crash-ui-exact = Täpne tekst, mis saadetakse
crash-ui-send = Saada
crash-ui-do-not-send = Ära saada
crash-ui-always-send = Saada alati
crash-ui-always-hint = Saada alati muudab ainult seadet kohas Eelistused > Privaatsus > Kokkujooksmise aruanded. Seal saad selle igal ajal tagasi võtta.
crash-ui-close-hint = Sulge see aken, et otsustada järgmisel korral. Aruanne jääb selle ajani sellesse arvutisse.
crash-ui-store-failed = Kokkujooksmise aruannet ei saanud salvestada: { $error }
crash-ui-text-changed = Saadetav tekst muutus, näiteks seetõttu, et muutsid privaatsuse seadet. Loe see uuesti läbi ja vajuta nuppu uuesti.
crash-ui-queued = Kokkujooksmise aruanne ootab saatmist.

crash-ui-prefs-privacy = Privaatsus
crash-ui-prefs-crash-reports = Kokkujooksmise aruanded
crash-ui-policy-ask = Küsi iga kord
crash-ui-policy-always = Saada alati
crash-ui-policy-never = Ära kunagi saada
crash-ui-prefs-crash-hint = Pärast ootamatut peatumist näitab järgmine käivitus aruannet enne, kui midagi saadetakse.
crash-ui-prefs-installation-id = Paigalduse ID
crash-ui-prefs-reset = Lähtesta
crash-ui-prefs-include-id = Lisa paigalduse ID aruannetele
crash-ui-prefs-id-hint = Juhuslik number, mis tehti selles arvutis. See aitab meil näha, et aruanded tulevad samast paigaldusest. Seda ei tehta sinu riistvarast, nimest ega võrgust. IKÜM-i (GDPR) järgi on see pseudonüümitud isikuandmed: lähtestamine teeb uue ja vana ID-ga aruandeid ei saa uute külge siduda.
crash-ui-prefs-id-error = ID-d ei saanud muuta: { $error }

crash-ui-launcher-waiting = { $count ->
    [one] Üks kokkujooksmise aruanne ootab. Käivita Kadakas, vaata see üle ja otsusta, kas saata.
   *[other] { $count } kokkujooksmise aruannet ootab. Käivita Kadakas, vaata need üle ja otsusta, kas saata.
}
