## plugin-ui: plugin_ui.rs

plugin-ui-plug-ins = Plug-ins
plugin-ui-intro = A plug-in is a small program that runs in a sandbox. It has no files, no network and no clock. Install only plug-ins from sources that you trust.
plugin-ui-kept = Installed plug-ins and your trust are kept for the next session.
plugin-ui-session-only = This browser has no storage for plug-ins. They stay only until you close the page.
plugin-ui-install = Install plug-in...
plugin-ui-none = No plug-ins are installed.
plugin-ui-trusted = Trusted
plugin-ui-not-trusted = Not trusted
plugin-ui-in-use =
    { $count ->
        [one] Used by { $count } object
       *[other] Used by { $count } objects
    }
plugin-ui-review = Review...
plugin-ui-stop-trusting = Stop trusting
plugin-ui-remove = Remove
plugin-ui-remove-blocked = Objects in the drawing use this plug-in. Delete them first.
plugin-ui-removed = Removed { $name }.
plugin-ui-installed = Installed { $name }.
plugin-ui-replaced =
    { $count ->
        [one] Installed a new build of { $name }. { $count } object still has shapes from the old build.
       *[other] Installed a new build of { $name }. { $count } objects still have shapes from the old build.
    }
plugin-ui-not-kept = Not kept for the next session: { $message }
plugin-ui-install-failed = Cannot install the plug-in: { $message }
plugin-ui-read-failed = Cannot read the file: { $message }
plugin-ui-too-big = The file is too big for a plug-in. The limit is { $max } bytes.
plugin-ui-stored-problem = Stored plug-in { $name } was not loaded: { $reason }
plugin-ui-trust-title = Trust this plug-in?
plugin-ui-version = Version
plugin-ui-author = Author (stated by the plug-in)
plugin-ui-no-author = not stated
plugin-ui-hash = SHA-256
plugin-ui-size = Size
plugin-ui-bytes = { $size } bytes
plugin-ui-can-do = It can
plugin-ui-kind-object = make parametric objects
plugin-ui-kind-tool = draw with a tool
plugin-ui-no-permissions = It gets no permissions. It cannot read your drawing, your files, the network or the clock. It only turns the values and points that Kadakas sends into shapes.
plugin-ui-warning = It can still draw wrong shapes or use up time. Your trust is for this exact file. A new build needs your trust again.
plugin-ui-agents = An agent that uses the API cannot trust a plug-in for you.
plugin-ui-trust = Trust
plugin-ui-cancel = Cancel
plugin-ui-trusted-status = Trusted { $name }.
plugin-ui-object-heading = Plug-in object: { $name } { $version }
plugin-ui-object-not-installed = The plug-in is not installed. The shapes stay as they are.
plugin-ui-object-stale = The shapes are from another build of the plug-in. Run again to use the installed build.
plugin-ui-needs-trust = This plug-in is not trusted, so its parameters cannot change.
plugin-ui-object-error = The plug-in failed: { $message }
plugin-ui-object-error-note = The shapes are from the last good run.
plugin-ui-run-again = Run again
plugin-ui-run-again-hint = Make the shapes again with the installed plug-in.
plugin-ui-default = Default
plugin-ui-tool-set = Plug-ins
plugin-ui-tool-none = No plug-in has a tool. Install one in File > Plug-ins.
plugin-ui-tool-untrusted = Not trusted yet. Click to review.
plugin-ui-tool-started = Plug-in tool { $name }. Press Esc to stop.
plugin-ui-tool-failed = The plug-in tool stopped: { $message }
plugin-ui-tool-done-object = Placed a plug-in object.
plugin-ui-tool-done-shapes =
    { $count ->
        [one] Added { $count } shape.
       *[other] Added { $count } shapes.
    }
