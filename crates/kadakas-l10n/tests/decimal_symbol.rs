//! The decimal symbol of the process (its own test program, because it is process wide).

use kadakas_l10n::{decimal_symbol, set_decimal_symbol, tr, with_decimal_symbol};

#[test]
fn the_process_symbol_can_be_set_and_a_thread_can_override_it() {
    assert_eq!(decimal_symbol(), '.');
    set_decimal_symbol(',');
    assert_eq!(decimal_symbol(), ',');
    assert_eq!(tr!("common-delete-n", count = 0.5), "Delete 0,5 objects");
    // A thread override wins, and the process symbol is back after it.
    with_decimal_symbol('.', || {
        assert_eq!(tr!("common-delete-n", count = 0.5), "Delete 0.5 objects");
    });
    assert_eq!(decimal_symbol(), ',');
    set_decimal_symbol('.');
    assert_eq!(tr!("common-delete-n", count = 0.5), "Delete 0.5 objects");
}
