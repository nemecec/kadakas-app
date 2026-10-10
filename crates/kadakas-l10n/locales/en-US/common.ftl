## Words that many dialogs share.

common-ok = OK
common-cancel = Cancel
common-delete-n =
    { $count ->
        [one] Delete { $count } object
       *[other] Delete { $count } objects
    }
# Used by the tests of kadakas-l10n.
common-test-hello = Hello { $name }
    .title = Greeting
