//! Tests of the values of Scale and the three values of Move (MOD-05).

use super::*;
use kadakas_units::{Decimal, Grouping, LengthUnit, NumberFormat};

fn metric() -> Units {
    Units::METRIC
}

fn meters() -> Units {
    Units {
        length: LengthUnit::M,
        ..Units::METRIC
    }
}

fn estonian() -> Units {
    Units {
        length: LengthUnit::M,
        number: NumberFormat {
            decimal: Decimal::Comma,
            grouping: Grouping::Space,
        },
        ..Units::METRIC
    }
}

fn run(text: &str, u: &Units, tool: Tool, phase: Phase) -> Result<Value, InputError> {
    parse(text, u, tool, phase)
}

fn scale(text: &str, u: &Units) -> Result<Value, InputError> {
    run(text, u, Tool::Scale, Phase::Step(1))
}

fn mv(text: &str, u: &Units) -> Result<Value, InputError> {
    run(text, u, Tool::Move, Phase::Step(1))
}

fn len(v: Result<Value, InputError>) -> f64 {
    match v {
        Ok(Value::Length(l)) => l,
        other => panic!("{other:?}"),
    }
}

#[test]
fn scale_takes_a_factor_three_factors_or_a_length() {
    // A bare number is a factor, in any unit setting.
    assert_eq!(scale("2", &metric()), Ok(Value::Factor(2.0)));
    assert_eq!(scale("2", &meters()), Ok(Value::Factor(2.0)));
    assert_eq!(scale("-1", &metric()), Ok(Value::Factor(-1.0)));
    assert_eq!(scale("1.5", &metric()), Ok(Value::Factor(1.5)));
    // A decimal comma that was typed by habit is still a factor.
    assert_eq!(scale("1,5", &metric()), Ok(Value::Factor(1.5)));
    assert_eq!(scale("1,5", &estonian()), Ok(Value::Factor(1.5)));
    // One factor for each axis; a missing value stays as it is.
    assert_eq!(
        scale("1;1;2", &metric()),
        Ok(Value::Triple([Some(1.0), Some(1.0), Some(2.0)]))
    );
    assert_eq!(
        scale("2;;", &estonian()),
        Ok(Value::Triple([Some(2.0), None, None]))
    );
    // A number with a unit is a length.
    assert!((len(scale("2m", &metric())) - 2000.0).abs() < 1e-9);
    assert!((len(scale("250cm", &metric())) - 2500.0).abs() < 1e-9);
    // Other kinds are refused with the list of what Scale takes.
    assert!(matches!(
        scale("45°", &metric()),
        Err(InputError::NotAccepted {
            kind: Kind::Angle,
            ..
        })
    ));
    assert!(matches!(
        scale("5x", &metric()),
        Err(InputError::NotAccepted {
            kind: Kind::Count,
            ..
        })
    ));
    assert_eq!(
        scale("1;2;3;4", &metric()),
        Err(InputError::TooManyValues { separator: ',' })
    );
}

#[test]
fn move_takes_three_lengths_as_a_relative_distance() {
    assert_eq!(
        mv("100;200;300", &metric()),
        Ok(Value::Triple([Some(100.0), Some(200.0), Some(300.0)]))
    );
    assert_eq!(
        mv("1,2,3", &metric()),
        Ok(Value::Triple([Some(1.0), Some(2.0), Some(3.0)]))
    );
    assert_eq!(
        mv(";;5", &estonian()),
        Ok(Value::Triple([None, None, Some(5000.0)]))
    );
    // A pair is still not a move.
    assert!(matches!(
        mv("1;2", &metric()),
        Err(InputError::NotAccepted {
            kind: Kind::Pair,
            ..
        })
    ));
    // Three values are too many for the tools that take no triple.
    assert!(matches!(
        run("1;2;3", &metric(), Tool::Rotate, Phase::Step(2)),
        Err(InputError::TooManyValues { .. })
    ));
}

#[test]
fn the_table_lists_the_new_kinds_for_move_and_scale() {
    assert!(accepts(Tool::Move, Phase::Step(1)).takes(Kind::Triple));
    assert!(accepts(Tool::Move, Phase::Copied).takes(Kind::Triple));
    let s = accepts(Tool::Scale, Phase::Step(1));
    assert!(s.takes(Kind::Factor) && s.takes(Kind::Triple) && s.takes(Kind::Length));
    assert_eq!(s.bare, Some(Bare::Factor));
    assert!(!accepts(Tool::Scale, Phase::Copied).takes(Kind::Count));
    for k in [Kind::Triple, Kind::Factor] {
        assert!(!k.name().is_empty());
    }
}
