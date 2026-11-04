## markup-ui: markup_ui.rs, app/markup.rs (REV-11)

markup-ui-mode = Markup mode
markup-ui-mode-hint = Draw on the view with the pointer. The normal tools wait until you switch this off.
markup-ui-new = New Markup
markup-ui-new-hint = Start a markup with the view as it is now. The marks belong to this view.
markup-ui-finish = Finish Markup
markup-ui-finish-hint = Save the markup with the viewpoint and stop drawing.
markup-ui-clear = Clear
markup-ui-clear-hint = Remove all marks of the open markup. One undo step.
markup-ui-delete-mark = Delete Mark
markup-ui-hide = Hide Marks
markup-ui-show = Show Marks
markup-ui-tool-pen = Pen
markup-ui-tool-highlighter = Highlighter
markup-ui-tool-arrow = Arrow
markup-ui-tool-rectangle = Rectangle
markup-ui-tool-ellipse = Ellipse
markup-ui-tool-note = Note
markup-ui-tool-select = Select
markup-ui-tool-eraser = Eraser
markup-ui-tool-select-hint = Click a mark to pick it. Then change its color and width, or delete it.
markup-ui-tool-eraser-hint = Click a mark to remove it.
markup-ui-color = Color
markup-ui-color-name =
    { $index ->
        [0] Red
        [1] Orange
        [2] Yellow
        [3] Green
        [4] Blue
        [5] Purple
        [6] Black
        [7] White
       *[other] Color
    }
markup-ui-color-custom = Custom color
markup-ui-width = Width
markup-ui-list-heading = Markups
markup-ui-none = No markups yet. Choose New Markup and draw on the view.
markup-ui-default-name = Markup { $number }
markup-ui-row-info =
    { $marks ->
        [one] { $mode }, 1 mark, { $author }
       *[other] { $mode }, { $marks } marks, { $author }
    }
markup-ui-mode-plan = Plan
markup-ui-mode-3d = 3D
markup-ui-agent = agent
markup-ui-go-to = Go to Viewpoint
markup-ui-go-to-hint = Show the view this markup was drawn in, with its layers.
markup-ui-resume = Continue Drawing
markup-ui-delete = Delete Markup
markup-ui-name = Name
markup-ui-note = Note
markup-ui-issue = Issue
markup-ui-no-issue = No issue
markup-ui-issue-hint = A markup of an issue goes into the BCF file of that issue.
markup-ui-view-differs = The view is not the view of "{ $name }", so its marks are hidden. Use Go to Viewpoint.
markup-ui-paused = The view moved. Use Go to Viewpoint to draw on this markup again.
markup-ui-badge = Markup: { $tool }
markup-ui-created = Markup started. Draw on the view.
markup-ui-finished = Markup saved with its viewpoint.
markup-ui-empty-removed = The markup had no marks and is removed.
markup-ui-opened = Showing { $name }.
markup-ui-limit = A markup holds at most { $marks } marks. Finish this markup and start a new one.
markup-ui-no-view = There is no view to mark yet. Open the drawing area first.
markup-ui-note-text = Text of the note
markup-ui-note-ok = Add Note
markup-ui-note-cancel = Cancel
markup-ui-delete-agent = This markup is the work of an agent. Delete it?
markup-ui-delete-yes = Delete
markup-ui-delete-no = Keep
markup-ui-no-fit = The window is too narrow for the frame of "{ $name }", so its marks are hidden. Make the window wider or lower.
markup-ui-limit-points = A markup holds at most { $points } points. Finish this markup and start a new one.
markup-ui-stale = Model changed
