//! The size of a worksheet drawn as a table in the drawing. The app and `kadakas-print` use it,
//! so the table looks the same on screen and on paper.

use crate::compute::Computed;
use crate::value::Value;

/// Most rows and columns that a table draws.
pub const MAX_ROWS: u32 = 500;
pub const MAX_COLS: u32 = 40;

/// Width of one character relative to the text height, for the column widths.
const CHAR_WIDTH: f64 = 0.55;
/// Padding inside a cell, left and right together, relative to the text height.
const PADDING: f64 = 0.9;
/// Row height relative to the text height.
const ROW: f64 = 1.6;

/// Column widths and row height of a table, in the unit of the text height.
#[derive(Clone, Debug, PartialEq)]
pub struct TableLayout {
    pub rows: u32,
    pub cols: u32,
    pub col_widths: Vec<f64>,
    pub row_height: f64,
    pub width: f64,
    pub height: f64,
}

/// The layout of the calculated worksheet at a text height. An empty worksheet gets one small cell.
pub fn layout(c: &Computed, text_height: f64) -> TableLayout {
    let h = if text_height.is_finite() && text_height > 0.0 {
        text_height
    } else {
        1.0
    };
    let rows = c.rows.clamp(1, MAX_ROWS);
    let cols = c.cols.clamp(1, MAX_COLS);
    let col_widths: Vec<f64> = (1..=cols)
        .map(|col| {
            let chars = (1..=rows)
                .map(|r| c.display(r, col).chars().count())
                .max()
                .unwrap_or(0);
            (chars as f64 * CHAR_WIDTH + PADDING).max(3.0) * h
        })
        .collect();
    let width = col_widths.iter().sum();
    TableLayout {
        rows,
        cols,
        col_widths,
        row_height: ROW * h,
        width,
        height: ROW * h * f64::from(rows),
    }
}

/// True if the cell holds a number, so a table can line it up on the right.
pub fn is_number(c: &Computed, row: u32, col: u32) -> bool {
    matches!(c.value(row, col), Value::Number(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::compute;
    use crate::def::WorksheetDef;
    use kadakas_core::model::Document;

    #[test]
    fn widths_follow_the_text() {
        let mut w = WorksheetDef::default();
        w.set_cell("A1", "Name", None).unwrap();
        w.set_cell("A2", "A long cell text", None).unwrap();
        w.set_cell("B1", "7", None).unwrap();
        w.set_cell("B3", "=B1*2", None).unwrap();
        let c = compute(&w, &Document::default());
        let l = layout(&c, 2.0);
        assert_eq!((l.rows, l.cols), (3, 2));
        assert!(l.col_widths[0] > l.col_widths[1]);
        assert_eq!(l.col_widths[1], 6.0, "a short column has the least width");
        assert!((l.col_widths[0] - (16.0 * 0.55 + 0.9) * 2.0).abs() < 1e-9);
        assert_eq!(l.row_height, 3.2);
        assert!(
            (l.height - 9.6).abs() < 1e-9
                && (l.width - l.col_widths.iter().sum::<f64>()).abs() < 1e-9
        );
        assert!(is_number(&c, 1, 2) && !is_number(&c, 1, 1));
    }

    #[test]
    fn empty_and_huge_sheets_are_bounded() {
        let c = compute(&WorksheetDef::default(), &Document::default());
        let l = layout(&c, 3.0);
        assert_eq!((l.rows, l.cols), (1, 1));
        assert!(l.width > 0.0 && l.height > 0.0);
        let mut w = WorksheetDef::default();
        w.set_cell("A9999", "x", None).unwrap();
        w.set_cell("ZZ1", "x", None).unwrap();
        let l = layout(&compute(&w, &Document::default()), 1.0);
        assert_eq!((l.rows, l.cols), (MAX_ROWS, MAX_COLS));
        assert_eq!(layout(&c, f64::NAN).row_height, 1.6);
        assert_eq!(layout(&c, -2.0).row_height, 1.6);
    }
}
