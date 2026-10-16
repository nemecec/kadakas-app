## autosave: autosave.rs

autosave-less-than-a-minute-ago = less than a minute ago
autosave-browser-storage-is-not-available = browser storage is not available
autosave-browser-storage-is-full = browser storage is full
autosave-minutes-ago =
    { $n ->
        [one] { $n } minute ago
       *[other] { $n } minutes ago
    }
autosave-hours-ago =
    { $n ->
        [one] { $n } hour ago
       *[other] { $n } hours ago
    }
autosave-days-ago =
    { $n ->
        [one] { $n } day ago
       *[other] { $n } days ago
    }
autosave-missing-pictures =
    { $count ->
        [one] { $count } picture
       *[other] { $count } pictures
    }
autosave-missing-versions = the versions
autosave-missing-other =
    { $count ->
        [one] { $count } other part
       *[other] { $count } other parts
    }
autosave-copy-missing = The file { $file } changed on disk and no longer has { $what }. The recovery copy has small gray placeholders for the pictures and no versions that were in that file.
autosave-copy-as-json = The recovery copy is the plain JSON form, because the container form would be over a limit: { $why }
autosave-over-missing = The autosave did not replace your file { $file }, because it changed on disk and no longer has { $what }. The recovery copy was written. Save yourself to decide what to do.
autosave-over-changed = The autosave did not replace your file { $file }, because another program changed it since you opened it. The recovery copy was written. Save yourself to decide what to do.
