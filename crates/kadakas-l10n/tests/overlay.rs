//! The overlay is global in its process, so this test file has one test.

use kadakas_l10n::{overlay_tags, set_overlay, tr, with_language, with_overlay};

#[test]
fn the_overlay_changes_messages_and_can_be_removed() {
    assert_eq!(tr!("common-ok"), "OK");
    set_overlay(&[("en-US", "common-ok = Fine\n"), ("xx", "common-ok = No\n")]);
    assert_eq!(tr!("common-ok"), "Fine");
    assert_eq!(overlay_tags(), vec!["en-US"]);
    // Estonian has no text in this overlay.
    with_language("et", || assert_eq!(tr!("common-cancel"), "Katkesta"));
    // A second text for the same tag replaces the first one.
    set_overlay(&[("en-US", "a = 1\n"), ("en-us", "common-ok = Good\n")]);
    assert_eq!(tr!("common-ok"), "Good");
    // A thread overlay wins on its thread and goes away after the call.
    with_overlay(&[("en-US", "common-ok = Local\n")], || {
        assert_eq!(tr!("common-ok"), "Local");
        assert_eq!(tr!("common-cancel"), "Cancel");
    });
    assert_eq!(tr!("common-ok"), "Good");
    set_overlay(&[]);
    assert_eq!(tr!("common-ok"), "OK");
    assert!(overlay_tags().is_empty());
}
