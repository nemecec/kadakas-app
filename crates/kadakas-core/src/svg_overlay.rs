//! A way to change how objects look in the SVG of the drawing, for the compare overlay (REV-03).
//!
//! The SVG writer knows nothing of versions or diffs. A caller gives it an [`SvgOverlay`]: for each
//! object, keep the normal look, hide it, dim it or tint it, plus some extra objects (ghosts) to
//! draw on top. `kadakas-review` builds one from a compare overlay; any other change list can do
//! the same (IOP-23, IOP-24).

use glam::DVec2;

use crate::model::{Object, ObjectId};

/// The line pattern of a tinted outline. A pattern tells the status without color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgPattern {
    Solid,
    Dashed,
    Dotted,
}

/// A tint for an object: its pens and fills take this color.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgTint {
    pub rgb: [u8; 3],
    pub pattern: SvgPattern,
    /// The line weight is multiplied by this.
    pub weight_factor: f64,
    /// Opacity of the fill of a closed shape that has a fill (0 to 1).
    pub fill_alpha: f64,
    /// Opacity of the whole object (0 to 1).
    pub opacity: f64,
    /// A small mark at the top right corner of the object: `+`, `-` or `~`.
    pub symbol: Option<char>,
    /// The name of the status, written as `data-compare` on the group of the object.
    pub status: &'static str,
}

/// What the SVG does with one object.
#[derive(Clone, Debug, PartialEq)]
pub enum SvgLook {
    /// The normal look.
    Keep,
    /// The object is left out.
    Hide,
    /// The object is drawn with this opacity (0 to 1) and its normal colors.
    Dim(f64),
    /// The object is drawn in the tint.
    Tint(SvgTint),
}

/// An extra object drawn on top of the drawing, in a tint (a ghost of a removed object).
#[derive(Clone, Debug, PartialEq)]
pub struct SvgGhost {
    pub object: Object,
    pub tint: SvgTint,
    /// `removed` for the ghost of a removed object, `previous` for the old shape of a changed one.
    pub kind: &'static str,
}

/// One row of the legend that the SVG shows: a swatch in a tint, and a text.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgLegendRow {
    /// `None`: a row of text only (a note).
    pub tint: Option<SvgTint>,
    pub text: String,
}

/// The changes to the look of a drawing. See the module text.
pub trait SvgOverlay {
    /// What to do with an object of the document.
    fn look(&self, id: ObjectId) -> SvgLook;
    /// Extra objects, drawn after the document.
    fn ghosts(&self) -> &[SvgGhost];
    /// The part of the model to show (low and high corner, used as it is, without a margin).
    /// `None`: the box around the drawing and the ghosts, with a margin. Two renders that must
    /// line up (side by side) give the same box.
    fn view(&self) -> Option<(DVec2, DVec2)> {
        None
    }
    /// The rows of a legend, drawn at the top left corner of the picture. Empty: no legend.
    fn legend(&self) -> &[SvgLegendRow] {
        &[]
    }
}
