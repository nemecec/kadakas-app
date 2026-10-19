## crash-ui: crash reports (PLAT-20): the question after a crash, Preferences > Privacy, and the launcher note (crash_ui.rs, privacy_ui.rs)

crash-ui-title = Kadakas stopped unexpectedly
crash-ui-counter = Crash report { $n } of { $total }
crash-ui-intro = The last time Kadakas ran, it stopped because of an error. A crash report helps the developers to find and fix the cause. Nothing has been sent yet. You decide.
crash-ui-what = The report holds the version, the system, the graphics adapter, the error, a technical trace, the last log lines, the names (not the details) of the last commands, the memory use and the run time. File names, folders, user names, e-mail addresses, tokens and the content of your documents are removed.
crash-ui-summary = Kadakas { $version } ({ $channel }), { $kind }. Crash signature { $signature }.
crash-ui-kind-panic = program error
crash-ui-kind-native = crash in the system, a driver or a library
crash-ui-note-label = What were you doing? (optional)
crash-ui-contact-label = Contact, if we may ask you (optional)
crash-ui-exact = The exact text that will be sent
crash-ui-send = Send
crash-ui-do-not-send = Do not send
crash-ui-always-send = Always send
crash-ui-always-hint = Always send only changes the setting in Preferences > Privacy > Crash reports. You can take it back there at any time.
crash-ui-close-hint = Close this window to decide next time. The report stays on this computer until then.
crash-ui-store-failed = The crash report could not be stored: { $error }
crash-ui-queued = The crash report waits to be sent.

crash-ui-prefs-privacy = Privacy
crash-ui-prefs-crash-reports = Crash reports
crash-ui-policy-ask = Ask each time
crash-ui-policy-always = Always send
crash-ui-policy-never = Never send
crash-ui-prefs-crash-hint = After an unexpected stop, the next start shows the report before anything is sent.
crash-ui-prefs-installation-id = Installation ID
crash-ui-prefs-reset = Reset
crash-ui-prefs-include-id = Include the installation ID in reports
crash-ui-prefs-id-hint = A random number made on this computer. It lets us see that reports come from the same installation. It is not made from your hardware, your name or the network. Under the GDPR it is pseudonymous personal data: Reset makes a new one, and reports with the old one can no longer be linked to the new ones.
crash-ui-prefs-id-error = The ID could not be changed: { $error }

crash-ui-launcher-waiting = { $count ->
    [one] One crash report waits. Start Kadakas to look at it and decide whether to send it.
   *[other] { $count } crash reports wait. Start Kadakas to look at them and decide whether to send them.
}
