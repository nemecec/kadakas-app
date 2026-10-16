## issues-ui: issues_ui.rs, bcf_dialog.rs, app/issues.rs

issues-ui-new-from-selection = New Issue...
issues-ui-new-here = New Issue Here...
issues-ui-new-at-click = New Issue at a Click
issues-ui-pick-hint = Click in the drawing to place the issue.
issues-ui-hide-pins = Hide the pins
issues-ui-bcf = BCF...
issues-ui-lost-title = Comments of an issue that is not in the file
issues-ui-delete-pictures-note = The snapshot pictures of the issue stay in the drawing after it is deleted.
issues-ui-delete-purge = Also remove the pictures that no other issue uses (one undo step)
issues-ui-delete-purge-shared = Not possible in a shared document: another user may use a picture at the same time.
issues-ui-many = This document has { $count } issues. Above { $warn } the list gets slow and the file grows. You can go on; the cap is { $cap }.
issues-ui-hidden = Shared issue data that is not shown ({ $count })
issues-ui-hidden-help = The shared document holds this data, but this program does not show it, because it is over a limit. Nothing was deleted.
issues-ui-none = There are no issues yet. Select objects and choose New Issue, or use New Issue at a Click.
issues-ui-none-match = No issue matches the filters.
issues-ui-count = { $shown } of { $total } issues
issues-ui-show-done = Show resolved and closed issues
issues-ui-filter-assignee = Assignee
issues-ui-filter-label = Label
issues-ui-filter-text = Search
issues-ui-clear-filters = Clear
issues-ui-all = All
issues-ui-row-comments =
    { $count ->
        [one] 1 comment
       *[other] { $count } comments
    }
issues-ui-row-objects =
    { $count ->
        [one] 1 object
       *[other] { $count } objects
    }
issues-ui-due = due { $date }
issues-ui-due-soon = due soon: { $date }
issues-ui-due-overdue = overdue: { $date }
issues-ui-author-default = unknown
issues-ui-unknown-time = time not known
issues-ui-origin-user = a person
issues-ui-origin-agent = an agent
issues-ui-origin-script = a script
issues-ui-origin-remote = another user
issues-ui-type-issue = Issue
issues-ui-type-error = Error
issues-ui-type-warning = Warning
issues-ui-type-request = Request
issues-ui-type-clash = Clash
issues-ui-type-remark = Remark
issues-ui-priority-low = Low
issues-ui-priority-normal = Normal
issues-ui-priority-high = High
issues-ui-priority-critical = Critical
issues-ui-detail-by = By { $author } ({ $origin }), { $time }
issues-ui-detail-status = Status
issues-ui-detail-priority = Priority
issues-ui-detail-type = Type
issues-ui-detail-assignee = Assignee
issues-ui-detail-labels = Labels
issues-ui-detail-due = Due
issues-ui-objects =
    { $count ->
        [one] The issue is about 1 object.
       *[other] The issue is about { $count } objects.
    }
issues-ui-objects-missing =
    { $count ->
        [one] 1 of them is not in the drawing.
       *[other] { $count } of them are not in the drawing.
    }
issues-ui-viewpoints = Viewpoints
issues-ui-no-viewpoints = This issue has no viewpoint.
issues-ui-viewpoint-default-name = View { $number }
issues-ui-viewpoint-go = Go to { $name }
issues-ui-viewpoint-add = Add the Current View
issues-ui-viewpoint-delete = Delete
issues-ui-thread = Comments
issues-ui-no-comments = No comments yet.
issues-ui-comment-hint = Write a comment
issues-ui-comment-send = Comment
issues-ui-comment-by = { $author } ({ $origin }), { $time }
issues-ui-comment-edited = edited
issues-ui-resolve = Resolve
issues-ui-reopen = Reopen
issues-ui-delete = Delete Issue
issues-ui-confirm-delete = Delete the issue "{ $title }" with its comments? Undo brings it back.
issues-ui-confirm-delete-others = The issue "{ $title }" is the work of somebody else ({ $why }). Delete it anyway? Undo brings it back.
issues-ui-new-title = New Issue
issues-ui-new-intro-objects =
    { $count ->
        [one] About the selected object.
       *[other] About the { $count } selected objects.
    }
issues-ui-new-intro-point = At a point of the drawing.
issues-ui-new-intro-none = Not tied to an object or a point.
issues-ui-field-title = Title
issues-ui-field-description = Description
issues-ui-field-labels-hint = separated by commas
issues-ui-field-due-hint = YYYY-MM-DD
issues-ui-keep-view = Keep the current view with the issue
issues-ui-keep-snapshot = Keep a picture of the model
issues-ui-create = Create
issues-ui-cancel = Cancel
issues-ui-created = Made the issue "{ $title }".
issues-ui-opened = Opened the issue "{ $title }".
issues-ui-opened-missing =
    { $count ->
        [one] Opened the issue "{ $title }". 1 object of the issue is not in the drawing.
       *[other] Opened the issue "{ $title }". { $count } objects of the issue are not in the drawing.
    }
issues-ui-deleted = Deleted the issue "{ $title }".
issues-ui-error = Issue: { $e }
issues-ui-bcf-title = BCF Issues
issues-ui-bcf-intro = BCF is an open file format for issues (.bcfzip). Use it to send issues to other programs and to read theirs. The layers and classes of a viewpoint are not part of BCF.
issues-ui-bcf-filter = BCF files
issues-ui-bcf-export-heading = Export
issues-ui-bcf-version = Version
issues-ui-bcf-only-open = Only issues that are not resolved or closed
issues-ui-bcf-export = Export...
issues-ui-bcf-import-heading = Import
issues-ui-bcf-import = Import...
issues-ui-bcf-exported = Wrote { $topics } issues to { $path }.
issues-ui-bcf-imported = Imported { $imported } issues. { $replaced } replaced, { $unchanged } were the same already, { $skipped } skipped.
issues-ui-bcf-notes = Notes ({ $count })
issues-ui-bcf-error = BCF: { $e }
issues-ui-bcf-conflict-title = Issues that are in the drawing already
issues-ui-bcf-conflict-text = { $count } issues of the file have the same ID as an issue in the drawing, but other content. Nothing was changed. Choose what to do.
issues-ui-bcf-conflict-row = Here: { $ours }. In the file: { $file }.
issues-ui-bcf-keep-mine = Keep mine
issues-ui-bcf-take-file = Take the file
issues-ui-bcf-keep-both = Keep both
issues-ui-bcf-unfit-title = Items that do not fit
issues-ui-bcf-unfit-text = { $count } items of the file do not fit the limits of Kadakas. Nothing was imported. You can import the file without these items, or cancel.
issues-ui-bcf-unfit-row = { $topic }: { $item }. { $reason }
issues-ui-bcf-unfit-comment = a comment
issues-ui-bcf-unfit-snapshot = a snapshot
issues-ui-bcf-unfit-camera = a camera
issues-ui-bcf-unfit-skip = Import without these items
