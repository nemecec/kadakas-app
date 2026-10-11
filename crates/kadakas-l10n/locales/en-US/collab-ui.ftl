## collab-ui: collab_ui.rs (File > Collaboration, the status item, the People palette)

collab-ui-connect = Connect...
collab-ui-disconnect = Disconnect
collab-ui-dialog-title = Connect to a shared document
collab-ui-server = Server address
collab-ui-document = Document ID
collab-ui-token = Token
collab-ui-name = Your name
collab-ui-connect-button = Connect
collab-ui-default-token = The token from KADAKAS_COLLAB_TOKEN or --collab-token is used for this server.
collab-ui-token-note = The token stays in memory. Kadakas never saves it.
collab-ui-server-copy-note = The copy on the server replaces the open document.
collab-ui-replace-title = Replace the open document?
collab-ui-replace-text = The open document has changes that are not saved. The shared document replaces it. Save the document first, or replace it now.
collab-ui-replace-button = Replace
collab-ui-discard-title = Drop the waiting edits?
collab-ui-discard-text = Edits that you made offline wait for another document or another user. If you go on, they are dropped.
collab-ui-discard-button = Drop and connect
collab-ui-joining = Joining the shared document... Wait until it has arrived.
collab-ui-status = Collaboration: { $state }
collab-ui-token-expired = The token has run out. Ask the administrator of the server for a new one.
collab-ui-already-connected = Already connected. Disconnect first.
collab-ui-connect-failed = Could not connect: { $error }
collab-ui-disconnected = Disconnected. Your edits go on offline.
collab-ui-error = Collaboration error: { $error }
collab-ui-problems =
    { $count ->
        [one] One change could not be shared.
       *[other] { $count } changes could not be shared.
    }
collab-ui-undo-skipped =
    { $count ->
        [one] One change was kept, because another person changed it after you.
       *[other] { $count } changes were kept, because other people changed them after you.
    }
collab-ui-nothing-to-undo = Nothing of yours to undo.
collab-ui-nothing-to-redo = Nothing of yours to redo.
collab-ui-blocked-replace = Disconnect from the shared document before you open or create another document.
collab-ui-web-joining = Joining { $server }...
collab-ui-you = You: { $name }
collab-ui-you-unnamed = You
collab-ui-nobody = Nobody else is here.
collab-ui-not-connected = Not connected. Use File > Collaboration > Connect.
collab-ui-quiet = { $name } (quiet)
collab-ui-selected =
    { $count ->
        [one] { $count } object selected
       *[other] { $count } objects selected
    }
collab-ui-save-first = Save first
collab-ui-quick-connect = Collaboration: Connect...
collab-ui-quick-disconnect = Collaboration: Disconnect
collab-ui-leave-disconnect-title = Disconnect with waiting edits?
collab-ui-leave-disconnect-text =
    { $count ->
        [one] One edit has not reached the server yet. If you disconnect, nobody else sees it until you connect again. It stays in this program until you close it.
       *[other] { $count } edits have not reached the server yet. If you disconnect, nobody else sees them until you connect again. They stay in this program until you close it.
    }
collab-ui-leave-close-title = Close with waiting edits?
collab-ui-leave-close-text =
    { $count ->
        [one] One edit has not reached the server yet. If you close the program now, it is lost.
       *[other] { $count } edits have not reached the server yet. If you close the program now, they are lost.
    }
collab-ui-leave-close-button = Close anyway
