//! The overlay is global in its process, so this test file has one test.

use kadakas_l10n::{overlay_tags, set_overlay, tr, with_language, with_overlay};

#[test]
fn the_overlay_changes_messages_and_can_be_removed() {
    assert_eq!(tr!("panels-class"), "Class");
    set_overlay(&[
        ("en-US", "panels-class = Fine\n"),
        ("xx", "panels-class = No\n"),
    ]);
    assert_eq!(tr!("panels-class"), "Fine");
    assert_eq!(overlay_tags(), vec!["en-US"]);
    // Estonian has no text in this overlay.
    with_language("et", || assert_eq!(tr!("panels-classes"), "Klassid"));
    // A second text for the same tag replaces the first one.
    set_overlay(&[("en-US", "a = 1\n"), ("en-us", "panels-class = Good\n")]);
    assert_eq!(tr!("panels-class"), "Good");
    // A message of a denied group is dropped.
    set_overlay(&[("en-US", "common-ok = Hacked\npanels-class = Layer\n")]);
    assert_eq!(tr!("common-ok"), "OK");
    assert_eq!(tr!("panels-class"), "Layer");
    set_overlay(&[("en-US", "panels-class = Good\n")]);
    // A thread overlay wins on its thread and goes away after the call.
    with_overlay(&[("en-US", "panels-class = Local\n")], || {
        assert_eq!(tr!("panels-class"), "Local");
        assert_eq!(tr!("panels-classes"), "Classes");
    });
    assert_eq!(tr!("panels-class"), "Good");
    set_overlay(&[]);
    assert_eq!(tr!("panels-class"), "Class");
    assert!(overlay_tags().is_empty());
}
