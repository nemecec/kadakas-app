//! The Measurements box (MOD-04): what a person types while a modeling tool runs, and just after
//! it ends, turned into a typed value. Headless: the modeler window collects the characters and
//! calls [`parse`] when the person presses Enter (or on each key for a live preview).
//!
//! # What is accepted
//!
//! | Text | Value | Notes |
//! |---|---|---|
//! | `2.5`, `2.5m`, `250cm`, `3'6"`, `5' 6 1/2"` | [`Value::Length`] (mm) | a bare number counts in the document length unit |
//! | `3;4`, `3,4` | [`Value::Pair`] | `;` always; `,` only when the decimal separator is a point. A value may be empty: `,4` and `3;` |
//! | `45`, `45°`, `-30`, `8:12` | [`Value::Angle`] (degrees) | `8:12` is a slope, rise to run |
//! | `5x`, `x5`, `5*`, `*5` | [`Value::Count`] | copies after a copy |
//! | `/5`, `5/` | [`Value::Divide`] | divides the copies evenly |
//! | `12s` | [`Value::Sides`] | the number of segments or sides |
//! | `24r` | [`Value::Radius`] (mm) | a radius, after an arc |
//! | `[1,2,3]` | [`Value::PointAbs`] (mm) | an absolute point; `;` between values when the decimal is a comma |
//! | `<1,2,3>` | [`Value::PointRel`] (mm) | a point relative to the start |
//!
//! A bare number means a length, an angle or the first value of a pair, depending on the tool
//! ([`accepts`]). Lengths read with the units of the document ([`Units`]), so with a decimal
//! comma `2,5` is a length of two and a half. Which tool accepts which kinds is the table in
//! [`accepts`]; a kind the tool does not take is an [`InputError::NotAccepted`].
//!
//! Errors are values; [`InputError::message`] gives the text in the language of the UI.

mod parse;

pub use parse::{parse, parse_any};

use glam::DVec3;
use kadakas_l10n::tr;
use kadakas_units::Units;

/// Most copies of one array.
pub const MAX_COPIES: u32 = 1000;
/// Most parts of a division.
pub const MAX_PARTS: u32 = 1000;
/// Fewest sides of a circle, an arc or a polygon.
pub const MIN_SIDES: u32 = 3;
/// Most sides of a circle, an arc or a polygon.
pub const MAX_SIDES: u32 = 1000;

/// The tools that take typed values (and a few that take none, so the table is complete).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Line,
    Rectangle,
    Circle,
    Polygon,
    /// Center, start, end.
    Arc,
    /// Start, end, bulge.
    TwoPointArc,
    Move,
    Rotate,
    PushPull,
    Offset,
    TapeMeasure,
    Protractor,
    // No typed value:
    Select,
    Eraser,
    Freehand,
    FollowMe,
    Orbit,
    Pan,
    Zoom,
    Paint,
}

/// When the person types: while the tool runs, or just after it ended (until the next operation
/// starts or the tool changes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// The tool is running and waits for its next click. `step` counts the clicks so far.
    Step(u8),
    /// The operation ended: a new value redoes it. For Move and Rotate this means the last
    /// operation was a plain move, not a copy (no `5x` and no `/5`).
    Done,
    /// The operation ended and it was a copy (Move or Rotate with Ctrl/Option): `5x` and `/5`
    /// are now accepted. For the other tools it is the same as `Done`.
    Copied,
}

/// The kinds of typed values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Length,
    Pair,
    Angle,
    Count,
    Divide,
    Sides,
    Radius,
    PointAbs,
    PointRel,
}

impl Kind {
    /// The name in the language of the UI.
    pub fn name(self) -> String {
        match self {
            Kind::Length => tr!("uic-measure-kind-length"),
            Kind::Pair => tr!("uic-measure-kind-pair"),
            Kind::Angle => tr!("uic-measure-kind-angle"),
            Kind::Count => tr!("uic-measure-kind-count"),
            Kind::Divide => tr!("uic-measure-kind-divide"),
            Kind::Sides => tr!("uic-measure-kind-sides"),
            Kind::Radius => tr!("uic-measure-kind-radius"),
            Kind::PointAbs => tr!("uic-measure-kind-point-abs"),
            Kind::PointRel => tr!("uic-measure-kind-point-rel"),
        }
    }
}

/// A typed value. Lengths are millimeters, angles degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Length(f64),
    /// Two lengths; a missing one means "keep what it is" (`,4` changes only the second).
    Pair(Option<f64>, Option<f64>),
    Angle(f64),
    /// This many copies.
    Count(u32),
    /// Divide into this many parts.
    Divide(u32),
    Sides(u32),
    Radius(f64),
    PointAbs(DVec3),
    PointRel(DVec3),
}

impl Value {
    pub fn kind(&self) -> Kind {
        match self {
            Value::Length(_) => Kind::Length,
            Value::Pair(..) => Kind::Pair,
            Value::Angle(_) => Kind::Angle,
            Value::Count(_) => Kind::Count,
            Value::Divide(_) => Kind::Divide,
            Value::Sides(_) => Kind::Sides,
            Value::Radius(_) => Kind::Radius,
            Value::PointAbs(_) => Kind::PointAbs,
            Value::PointRel(_) => Kind::PointRel,
        }
    }
}

/// What a bare number (no unit, no mark) means for a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bare {
    Length,
    Angle,
    /// The first value of a pair (a rectangle takes a length first).
    PairFirst,
    /// The number of sides (Circle, Polygon and the arcs before the first click).
    Sides,
}

/// What a tool takes at a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accepts {
    pub kinds: &'static [Kind],
    /// The meaning of a bare number; `None` when the tool takes no bare number.
    pub bare: Option<Bare>,
    /// False when a negative length (a radius, a side length) makes no sense.
    pub signed: bool,
}

impl Accepts {
    pub fn takes_input(&self) -> bool {
        !self.kinds.is_empty()
    }

    pub fn takes(&self, k: Kind) -> bool {
        self.kinds.contains(&k)
    }
}

const NONE: Accepts = Accepts {
    kinds: &[],
    bare: None,
    signed: true,
};

/// Which tool accepts which kinds, at which phase. The modeler asks this to show what the box
/// accepts and [`parse`] uses it to refuse the rest. `Phase::Step(n)` counts the clicks so far.
///
/// | Tool | While it runs | After it ended |
/// |---|---|---|
/// | Line | length, point (absolute, relative) | the same |
/// | Rectangle | pair (a single length is the first value) | the same |
/// | Circle, Polygon | step 0 (no click): sides, a bare number is the sides; later: radius (a length), sides | the same |
/// | Arc | step 0: sides; step 1 (center clicked): radius, sides; step 2 (start clicked): angle, sides | sides, radius (no bare number) |
/// | Two-point arc | step 0: sides; step 1 (start clicked): chord length; step 2 (end clicked): bulge, radius, sides | bulge, radius, sides |
/// | Move | length, point | length, point (after a copy: also copies, division) |
/// | Rotate | angle | angle (after a copy: also copies, division) |
/// | Push/Pull, Offset, Tape measure | length | length |
/// | Protractor | angle | angle |
/// | the others | nothing | nothing |
pub fn accepts(tool: Tool, phase: Phase) -> Accepts {
    use Kind::*;
    let done = !matches!(phase, Phase::Step(_));
    let copied = phase == Phase::Copied;
    let step = match phase {
        Phase::Step(n) => n,
        Phase::Done | Phase::Copied => u8::MAX,
    };
    let a = |kinds: &'static [Kind], bare, signed| Accepts {
        kinds,
        bare: Some(bare),
        signed,
    };
    match tool {
        Tool::Line => a(&[Length, PointAbs, PointRel], Bare::Length, true),
        Tool::Rectangle => a(&[Pair], Bare::PairFirst, true),
        Tool::Circle | Tool::Polygon => match (done, step) {
            (false, 0) => a(&[Sides], Bare::Sides, false),
            _ => a(&[Length, Sides], Bare::Length, false),
        },
        // Click 1 is the center, click 2 the start (radius), click 3 the end (angle).
        Tool::Arc => match (done, step) {
            (true, _) => Accepts {
                kinds: &[Sides, Radius],
                bare: None,
                signed: false,
            },
            (false, 0) => a(&[Sides], Bare::Sides, false),
            (false, 1) => a(&[Length, Sides], Bare::Length, false),
            (false, _) => a(&[Angle, Sides], Bare::Angle, true),
        },
        // Click 1 is the start, click 2 the end of the chord, then the bulge.
        Tool::TwoPointArc => match (done, step) {
            (true, _) => a(&[Length, Radius, Sides], Bare::Length, true),
            (false, 0) => a(&[Sides], Bare::Sides, false),
            (false, 1) => a(&[Length], Bare::Length, false),
            (false, _) => a(&[Length, Radius, Sides], Bare::Length, true),
        },
        Tool::Move => {
            if copied {
                a(
                    &[Length, PointAbs, PointRel, Count, Divide],
                    Bare::Length,
                    true,
                )
            } else {
                a(&[Length, PointAbs, PointRel], Bare::Length, true)
            }
        }
        Tool::Rotate => {
            if copied {
                a(&[Angle, Count, Divide], Bare::Angle, true)
            } else {
                a(&[Angle], Bare::Angle, true)
            }
        }
        Tool::PushPull | Tool::Offset | Tool::TapeMeasure => a(&[Length], Bare::Length, true),
        Tool::Protractor => a(&[Angle], Bare::Angle, true),
        Tool::Select
        | Tool::Eraser
        | Tool::Freehand
        | Tool::FollowMe
        | Tool::Orbit
        | Tool::Pan
        | Tool::Zoom
        | Tool::Paint => NONE,
    }
}

/// Why the text was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum InputError {
    /// Nothing was typed.
    Empty,
    /// Not a number, a length or an angle.
    NotAValue {
        text: String,
    },
    /// A valid value of a kind that the tool does not take now.
    NotAccepted {
        kind: Kind,
        accepted: &'static [Kind],
    },
    /// More than two values; `separator` is the one to use.
    TooManyValues {
        separator: char,
    },
    /// A point without three values; `separator` is the one to use.
    BadPoint {
        separator: char,
    },
    /// A point with brackets that do not match (`[1,2,3>`).
    BadBrackets,
    CopiesZero,
    CopiesTooMany,
    DivideTooSmall,
    DivideTooMany,
    SidesRange,
    /// A negative value for a tool that needs a positive one.
    Negative,
    /// A number too large to use.
    TooLarge,
    /// `1,000` in a setting with a decimal point: it could be a thousands group or a decimal
    /// comma, so it is refused. `text` is what was typed.
    AmbiguousComma {
        text: String,
    },
}

impl InputError {
    /// The message for the person, in the language of the UI.
    pub fn message(&self) -> String {
        match self {
            InputError::Empty => tr!("uic-measure-empty"),
            InputError::NotAValue { text } => tr!("uic-measure-not-a-value", text = text.as_str()),
            InputError::NotAccepted { kind, accepted } => {
                let list = if accepted.is_empty() {
                    tr!("uic-measure-kind-none")
                } else {
                    accepted
                        .iter()
                        .map(|k| k.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                tr!(
                    "uic-measure-not-accepted",
                    kind = kind.name(),
                    accepted = list
                )
            }
            InputError::TooManyValues { separator } => {
                tr!(
                    "uic-measure-too-many-values",
                    separator = separator.to_string()
                )
            }
            InputError::BadPoint { separator } => {
                let s = separator;
                tr!("uic-measure-bad-point", example = format!("[1{s}2{s}3]"))
            }
            InputError::BadBrackets => tr!("uic-measure-bad-brackets"),
            InputError::CopiesZero => tr!("uic-measure-copies-zero"),
            InputError::CopiesTooMany => {
                tr!("uic-measure-copies-large", max = MAX_COPIES.to_string())
            }
            InputError::DivideTooSmall => tr!("uic-measure-divide-small"),
            InputError::DivideTooMany => {
                tr!("uic-measure-divide-large", max = MAX_PARTS.to_string())
            }
            InputError::SidesRange => tr!(
                "uic-measure-sides-range",
                min = MIN_SIDES.to_string(),
                max = MAX_SIDES.to_string()
            ),
            InputError::Negative => tr!("uic-measure-negative"),
            InputError::TooLarge => tr!("uic-measure-too-large"),
            InputError::AmbiguousComma { text } => {
                tr!("uic-measure-ambiguous-comma", text = text.as_str())
            }
        }
    }
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for InputError {}

/// The separator between values for a unit setting: `;` with a decimal comma, else `,`.
pub fn value_separator(units: &Units) -> char {
    match units.number.decimal {
        kadakas_units::Decimal::Comma => ';',
        kadakas_units::Decimal::Point => ',',
    }
}

#[cfg(test)]
mod tests;
