//! The logic of the welcome dialog (PLAT-26). The launcher shows the dialog once, before it
//! starts any version of Kadakas. This crate has no window code:
//!
//! - [`choices`]: what the person can choose, and the defaults from the system;
//! - [`sysloc`]: what the system says about the region and the number format (the regional
//!   formats, not the language of the interface), per operating system;
//! - [`gate`]: when the dialog is due (first start, a newer dialog, never in tests or without a
//!   display), and what each button does;
//! - [`stores`]: where each choice is stored, and how it is read back;
//! - [`smoke`]: the test mode for the smoke test of the installers (PLAT-28).
//!
//! Every file is changed key by key: a key that this version does not know stays.

pub mod choices;
pub mod gate;
pub mod smoke;
pub mod stores;
pub mod sysloc;

pub use choices::{Choices, Mode, Navigation, NumberStyle, Theme, UnitSystem};
pub use gate::{Button, ENV_SKIP, Gate, Kind, WELCOME_VERSION};
pub use stores::Dirs;
pub use sysloc::SystemLocale;
