//! The process language. This test has its own program, because it changes a global setting.

use kadakas_l10n::{language, set_language, tr, with_language};

#[test]
fn set_language_is_process_wide_and_negotiated() {
    assert_eq!(language(), "en-US");
    assert_eq!(set_language("et_EE.UTF-8"), "et");
    assert_eq!(language(), "et");
    assert_eq!(tr!("common-cancel"), "Katkesta");
    // Another thread sees the same language.
    let other = std::thread::spawn(|| tr!("common-cancel")).join().unwrap();
    assert_eq!(other, "Katkesta");
    // The override of one thread wins over the process language.
    with_language("en-US", || {
        assert_eq!(language(), "en-US");
        assert_eq!(tr!("common-cancel"), "Cancel");
    });
    // An unknown language falls back to en-US.
    assert_eq!(set_language("xx"), "en-US");
    assert_eq!(language(), "en-US");
    assert_eq!(tr!("common-cancel"), "Cancel");
}
