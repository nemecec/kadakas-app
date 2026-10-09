//! Worksheets: a spreadsheet grid with formulas, and database rows that list the objects of a
//! document. The crate is pure (no files, no threads), so it also builds for WebAssembly.
//!
//! - [`formula`] parses and prints formulas. [`eval`] evaluates them.
//! - [`def`] is the saved definition of a worksheet (a document resource).
//! - [`compute`] turns a definition and a document into values. Results are computed, never saved.
//! - [`cache`] keeps results by document revision.
//! - [`templates`] has the built-in reports. [`export`] writes CSV and XLSX.

pub mod cache;
pub mod compute;
pub mod db;
pub mod def;
pub mod eval;
pub mod export;
pub mod formula;
pub mod layout;
pub mod templates;
pub mod value;

pub use cache::SheetCache;
pub use compute::{Computed, compute, compute_with};
pub use def::{CellDef, CellFormat, ColumnDef, DatabaseDef, GroupMode, RowSource, SortKey, Summary, WorksheetDef};
pub use layout::{TableLayout, layout};
pub use value::{ErrorKind, Value};
