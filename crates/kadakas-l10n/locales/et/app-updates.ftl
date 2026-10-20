## app-updates: Preferences > Updates, the update notice and the restart (app_updates.rs)

app-updates-heading = Uuendused
app-updates-mode = Režiim
app-updates-mode-auto-update = Automaatne uuendus
app-updates-mode-auto-download = Automaatne allalaadimine
app-updates-mode-notify-only = Ainult teavitus
app-updates-hint-auto-update = Uued versioonid laaditakse ise alla. Kadakas käivitab uusima versiooni, mis eelmisel käivitusel kokku ei jooksnud.
app-updates-hint-auto-download = Uued versioonid laaditakse ise alla. Sina valid, millal neile üle minna.
app-updates-hint-notify-only = Kadakas annab teada, et uus versioon on olemas. Allalaadimise ja ülemineku teed sina.
app-updates-channel = Kanal
app-updates-channel-stable = Stabiilne
app-updates-channel-nightly = Öine
app-updates-channel-local = Kohalik (selle arvuti ehitused)
app-updates-site = Kohalik uuenduste koht
app-updates-site-hint = file:///kaust/kuhu/viib
app-updates-site-bad = Kasuta aadressi, mis algab file://
app-updates-local-hint = Kohalik kanal käivitab uusima ehituse, mille lisasid käivitajaga (kadakas-launcher add). Kaust koos manifestiga võib olla uuenduste koht.
app-updates-keep-older = Hoitavaid vanemaid versioone
app-updates-keep-older-hint = Uusim versioon ja nii mitu sellele eelnevat jäävad kettale. Paigaldajaga kaasa tulnud versiooni ei loeta.
app-updates-no-launcher = Need seaded toimivad, kui Kadakas käivitatakse käivitaja kaudu.

app-updates-ready = Kadakas { $version } on valmis.
app-updates-restart = Taaskäivita uuendamiseks
app-updates-later = Hiljem
app-updates-available = Kadakas { $version } on saadaval.
app-updates-download = Laadi alla
app-updates-downloading = Laen alla Kadakat { $version }...

app-updates-save-question = Sul on salvestamata muudatusi. Kas salvestada need enne Kadaka taaskäivitust?
app-updates-save-and-restart = Salvesta ja taaskäivita
app-updates-restart-without-saving = Taaskäivita salvestamata
app-updates-cancel = Loobu

app-updates-skipped = Kadakas { $running } käivitus. Versioon { $skipped } jäeti vahele: { $reason }.
app-updates-reason-crashed = see ei sulgunud eelmisel korral korralikult
app-updates-reason-rolled-back = pöörasid sellelt tagasi
app-updates-reason-not-started = seda ei õnnestunud käivitada

app-updates-request-failed = Käivitajale ei saanud teada anda: { $error }
app-updates-save-failed = Uuenduste seadeid ei saanud salvestada: { $error }

app-open-title = Ava joonis
app-open-unsaved-question = Sul on salvestamata muudatusi. Kas salvestada need enne faili { $name } avamist?
app-open-save-and-open = Salvesta ja ava
app-open-dont-save = Ära salvesta
app-open-cancelled = Faili { $name } avamisest loobuti. Sinu joonis jäi muutmata.
app-open-not-saved = Muudatusi ei saanud salvestada, seega faili { $name } ei avatud.
app-open-unsaved-new = Sul on salvestamata muudatusi. Kas salvestada need enne uue joonise alustamist?
app-open-unsaved-agent = Agent tahab avada faili { $name } ja sul on salvestamata muudatusi. Kas salvestada need enne?
app-open-refuse = Keeldu
app-open-new-drawing = uus joonis
app-open-dropped = Ei avatud: { $dropped }. Kadakas ootab esmalt sinu vastust: { $waiting }.
app-open-agent-refused = Sa keeldusid. Agent ei avanud faili { $name } ja sinu joonis jäi muutmata.
app-open-agent-load-refused = Agent üritas dokumenti laadida, kuid sai keelu, sest sul on salvestamata muudatusi. Salvesta joonis ja palu agendil uuesti proovida.
