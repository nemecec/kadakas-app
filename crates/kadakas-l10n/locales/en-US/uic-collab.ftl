## uic-collab: collab/ (the status of the link, errors, the form checks)

uic-collab-link-offline = Offline
uic-collab-link-connecting = Connecting
uic-collab-link-connected = Connected
uic-collab-link-reconnecting = Reconnecting
uic-collab-link-refused = Refused
uic-collab-link-failed = Connection failed
# { $link } is the word for the link, for example "Connected".
uic-collab-status-line-pending =
    { $count ->
        [one] { $link }, { $count } edit waiting
       *[other] { $link }, { $count } edits waiting
    }
uic-collab-status-people =
    { $count ->
        [one] { $count } other person here
       *[other] { $count } other people here
    }
uic-collab-status-last-error = Last error: { $error }
uic-collab-guest = Guest { $id }

uic-collab-form-url-missing = Give the address of the server.
uic-collab-form-secret-in-url = The address must not hold a user name, a password, a query or a fragment. Give the token in its own field.
uic-collab-form-doc-missing = Give the document ID.
uic-collab-form-doc-invalid = The document ID must be a UUID, for example 01939a2e-7c1d-7a2b-9d3e-0123456789ab.

uic-collab-err-auth = The server refused the token. Ask the administrator of the server for a new token, then connect again.
uic-collab-err-protocol = The server speaks a protocol version that this program does not support. Update Kadakas or the server.
uic-collab-err-document-full = The shared document is full. The server refused your last edit, and it was taken back.
uic-collab-err-rejected = The server refused an edit, and it was taken back ({ $code }): { $message }
uic-collab-err-rate-limited = The server is busy. Your edits wait and are sent again.
uic-collab-err-server-fault = The server had a fault. Your edits wait and are sent again.
uic-collab-err-renumber = The numbers of your edits were used before. They are numbered again and sent.
uic-collab-err-unknown = The server reported an error ({ $code }): { $message }
uic-collab-note-connected = Connected to the shared document.
uic-collab-note-regressed = The server went back to an earlier state. Your work is sent again.
uic-collab-note-renumbered = The edits that were waiting got new numbers.
uic-collab-note-store-failed = The offline log could not be saved: { $error }
uic-collab-note-store-corrupt = The offline log could not be read and was not used: { $error }
