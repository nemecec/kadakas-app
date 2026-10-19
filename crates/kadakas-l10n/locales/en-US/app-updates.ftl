## app-updates: Preferences > Updates, the update notice and the restart (app_updates.rs)

app-updates-heading = Updates
app-updates-mode = Mode
app-updates-mode-auto-update = Auto-update
app-updates-mode-auto-download = Auto-download
app-updates-mode-notify-only = Notify only
app-updates-hint-auto-update = New versions download by themselves. Kadakas starts the newest version that did not crash at its last start.
app-updates-hint-auto-download = New versions download by themselves. You choose when to switch to them.
app-updates-hint-notify-only = Kadakas tells you that a new version exists. You start the download and the switch.
app-updates-channel = Channel
app-updates-channel-stable = Stable
app-updates-channel-nightly = Nightly
app-updates-channel-local = Local (builds on this computer)
app-updates-site = Local update site
app-updates-site-hint = file:///path/to/folder
app-updates-site-bad = Use an address that starts with file://
app-updates-local-hint = The channel Local starts the newest build that you added with the launcher (kadakas-launcher add). A folder with a manifest can be an update site.
app-updates-keep-older = Older versions to keep
app-updates-keep-older-hint = The newest version and this many before it stay on disk. The version that came with the installer is not counted.
app-updates-no-launcher = These settings work when Kadakas is started through the launcher.

app-updates-ready = Kadakas { $version } is ready.
app-updates-restart = Restart to update
app-updates-later = Later
app-updates-available = Kadakas { $version } is available.
app-updates-download = Download
app-updates-downloading = Downloading Kadakas { $version }...

app-updates-save-question = You have changes that are not saved. Save them before Kadakas restarts?
app-updates-save-and-restart = Save and restart
app-updates-restart-without-saving = Restart without saving
app-updates-cancel = Cancel

app-updates-skipped = Kadakas { $running } started. Version { $skipped } was skipped: { $reason }.
app-updates-reason-crashed = it did not exit cleanly the last time
app-updates-reason-rolled-back = you rolled back from it
app-updates-reason-not-started = it could not be started

app-updates-request-failed = Could not tell the launcher: { $error }
app-updates-save-failed = Could not save the update settings: { $error }

app-open-title = Open drawing
app-open-unsaved-question = You have changes that are not saved. Save them before opening { $name }?
app-open-save-and-open = Save and open
app-open-dont-save = Don't save
app-open-cancelled = Opening { $name } was cancelled. Your drawing is unchanged.
app-open-not-saved = The changes could not be saved, so { $name } was not opened.
