use super::*;
use glam::DVec3;
use kadakas_units::{Decimal, Grouping, LengthUnit, NumberFormat};
use proptest::prelude::*;

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

fn inches() -> Units {
    Units {
        length: LengthUnit::FtIn,
        ..Units::METRIC
    }
}

fn run(text: &str, u: &Units, tool: Tool, phase: Phase) -> Result<Value, InputError> {
    parse(text, u, tool, phase)
}

fn mv(text: &str, u: &Units) -> Result<Value, InputError> {
    run(text, u, Tool::Move, Phase::Step(1))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * (1.0 + b.abs())
}

fn len(v: Result<Value, InputError>) -> f64 {
    match v {
        Ok(Value::Length(l)) => l,
        other => panic!("{other:?}"),
    }
}

#[test]
fn lengths_in_the_document_unit_and_with_units() {
    assert!(near(len(mv("2.5", &metric())), 2.5));
    assert!(near(len(mv("2.5", &meters())), 2500.0));
    assert!(near(len(mv("2.5m", &metric())), 2500.0));
    assert!(near(len(mv("250cm", &metric())), 2500.0));
    assert!(near(len(mv("250 cm", &metric())), 2500.0));
    assert!(near(len(mv("12mm", &meters())), 12.0));
    assert!(near(len(mv("-2", &meters())), -2000.0));
    assert!(near(len(mv("  7  ", &meters())), 7000.0));
    assert!(near(len(mv("3'6\"", &metric())), 1066.8));
    assert!(near(len(mv("5' 6 1/2\"", &metric())), 1689.1));
    assert!(near(len(mv("6", &inches())), 152.4));
    assert!(near(len(mv("1e3", &metric())), 1000.0));
}

#[test]
fn the_decimal_comma_of_a_locale() {
    let et = estonian();
    assert!(near(len(mv("2,5", &et)), 2500.0));
    assert!(near(len(mv("2,5m", &et)), 2500.0));
    assert!(
        near(len(mv("2.5", &et)), 2500.0),
        "a point is understood too"
    );
    // A point locale: the comma separates two values.
    assert_eq!(
        run("3,4", &metric(), Tool::Rectangle, Phase::Step(1)),
        Ok(Value::Pair(Some(3.0), Some(4.0)))
    );
    // A decimal comma typed for a tool that takes no pair is read as a decimal comma.
    assert!(near(len(mv("2,5", &meters())), 2500.0));
}

#[test]
fn pairs_with_both_separators() {
    let rect = |t: &str, u: &Units| run(t, u, Tool::Rectangle, Phase::Step(1));
    assert_eq!(
        rect("3;4", &metric()),
        Ok(Value::Pair(Some(3.0), Some(4.0)))
    );
    assert_eq!(
        rect("3,4", &metric()),
        Ok(Value::Pair(Some(3.0), Some(4.0)))
    );
    assert_eq!(
        rect("3;4", &estonian()),
        Ok(Value::Pair(Some(3000.0), Some(4000.0)))
    );
    // In a comma locale `3,4` is one number, and a single value is the length.
    assert_eq!(
        rect("3,4", &estonian()),
        Ok(Value::Pair(Some(3400.0), None))
    );
    assert_eq!(
        rect("3,5;4,5", &estonian()),
        Ok(Value::Pair(Some(3500.0), Some(4500.0)))
    );
    // One value alone sets the first; a missing value keeps what it is.
    assert_eq!(rect("8", &metric()), Ok(Value::Pair(Some(8.0), None)));
    assert_eq!(rect("8m", &metric()), Ok(Value::Pair(Some(8000.0), None)));
    assert_eq!(rect(",4", &metric()), Ok(Value::Pair(None, Some(4.0))));
    assert_eq!(rect("3,", &metric()), Ok(Value::Pair(Some(3.0), None)));
    assert_eq!(
        rect(" 3 ; -4 ", &metric()),
        Ok(Value::Pair(Some(3.0), Some(-4.0)))
    );
    assert_eq!(
        rect("8', 20'", &metric()),
        Ok(Value::Pair(Some(2438.4), Some(6096.0)))
    );
    assert_eq!(
        rect(",", &metric()).unwrap_err(),
        InputError::NotAValue { text: ",".into() }
    );
    assert_eq!(
        rect("1;2;3", &estonian()),
        Err(InputError::TooManyValues { separator: ';' })
    );
    assert_eq!(
        rect("1,2,3", &metric()),
        Err(InputError::TooManyValues { separator: ',' })
    );
    assert!(matches!(
        rect("a;4", &metric()),
        Err(InputError::NotAValue { .. })
    ));
}

#[test]
fn angles_with_marks_and_slopes() {
    let rot = |t: &str| run(t, &metric(), Tool::Rotate, Phase::Step(2));
    assert_eq!(rot("45"), Ok(Value::Angle(45.0)));
    assert_eq!(rot("45°"), Ok(Value::Angle(45.0)));
    assert_eq!(rot("45deg"), Ok(Value::Angle(45.0)));
    assert_eq!(rot("45 DEG"), Ok(Value::Angle(45.0)));
    assert_eq!(rot("-30"), Ok(Value::Angle(-30.0)));
    assert_eq!(rot("45,5°"), Ok(Value::Angle(45.5)));
    assert_eq!(
        run("45,5", &estonian(), Tool::Rotate, Phase::Step(2)),
        Ok(Value::Angle(45.5))
    );
    let Ok(Value::Angle(a)) = rot("8:12") else {
        panic!()
    };
    assert!(near(a, (8.0f64 / 12.0).atan().to_degrees()));
    assert!(matches!(rot("8:0"), Err(InputError::NotAValue { .. })));
    assert!(matches!(rot("a:b"), Err(InputError::NotAValue { .. })));
    let Ok(Value::Angle(a)) = rot("45°30'") else {
        panic!()
    };
    assert!(near(a, 45.5));
    assert!(matches!(rot("x°"), Err(InputError::NotAValue { .. })));
    // The protractor takes an angle and nothing else.
    assert_eq!(
        run("34.1", &metric(), Tool::Protractor, Phase::Done),
        Ok(Value::Angle(34.1))
    );
    // A length for an angle tool.
    assert_eq!(
        rot("2m"),
        Err(InputError::NotAccepted {
            kind: Kind::Length,
            accepted: &[Kind::Angle]
        })
    );
}

#[test]
fn copies_and_divisions() {
    let after = |t: &str| run(t, &metric(), Tool::Move, Phase::Copied);
    for t in ["5x", "5X", "x5", "*5", "5*", "5 x", "x 5", "5×"] {
        assert_eq!(after(t), Ok(Value::Count(5)), "{t}");
    }
    for t in ["/5", "5/", "/ 5", "5 /"] {
        assert_eq!(after(t), Ok(Value::Divide(5)), "{t}");
    }
    assert_eq!(after("0x"), Err(InputError::CopiesZero));
    assert_eq!(after("1001x"), Err(InputError::CopiesTooMany));
    assert_eq!(
        after("99999999999999999999x"),
        Err(InputError::CopiesTooMany)
    );
    assert_eq!(after("1000x"), Ok(Value::Count(1000)));
    assert_eq!(after("/1"), Err(InputError::DivideTooSmall));
    assert_eq!(after("/0"), Err(InputError::DivideTooSmall));
    assert_eq!(after("/1001"), Err(InputError::DivideTooMany));
    assert!(matches!(after("/x"), Err(InputError::NotAValue { .. })));
    assert!(matches!(after("/"), Err(InputError::NotAValue { .. })));
    assert!(matches!(after("2.5x"), Err(InputError::NotAValue { .. })));
    assert!(matches!(after("x"), Err(InputError::NotAValue { .. })));
    // Rotate arrays too; but not while the tool runs.
    assert_eq!(
        run("3x", &metric(), Tool::Rotate, Phase::Copied),
        Ok(Value::Count(3))
    );
    assert_eq!(
        run("3x", &metric(), Tool::Move, Phase::Step(1)),
        Err(InputError::NotAccepted {
            kind: Kind::Count,
            accepted: &[Kind::Length, Kind::PointAbs, Kind::PointRel]
        })
    );
    // Not after a plain move: only after a copy.
    assert_eq!(
        run("3x", &metric(), Tool::Move, Phase::Done),
        Err(InputError::NotAccepted {
            kind: Kind::Count,
            accepted: &[Kind::Length, Kind::PointAbs, Kind::PointRel]
        })
    );
    assert_eq!(
        run("/3", &metric(), Tool::Rotate, Phase::Done),
        Err(InputError::NotAccepted {
            kind: Kind::Divide,
            accepted: &[Kind::Angle]
        })
    );
    assert_eq!(
        run("/3", &metric(), Tool::Line, Phase::Done),
        Err(InputError::NotAccepted {
            kind: Kind::Divide,
            accepted: &[Kind::Length, Kind::PointAbs, Kind::PointRel]
        })
    );
}

#[test]
fn sides_and_radius() {
    let circle = |t: &str| run(t, &metric(), Tool::Circle, Phase::Step(1));
    assert_eq!(circle("12s"), Ok(Value::Sides(12)));
    assert_eq!(circle("12S"), Ok(Value::Sides(12)));
    assert_eq!(circle("2s"), Err(InputError::SidesRange));
    assert_eq!(circle("1001s"), Err(InputError::SidesRange));
    assert_eq!(circle("3s"), Ok(Value::Sides(3)));
    assert_eq!(circle("1000s"), Ok(Value::Sides(1000)));
    assert_eq!(circle("2.5m"), Ok(Value::Length(2500.0)));
    assert_eq!(circle("-2"), Err(InputError::Negative));
    let arc2 = |t: &str| run(t, &meters(), Tool::TwoPointArc, Phase::Done);
    assert_eq!(arc2("24r"), Ok(Value::Radius(24000.0)));
    assert_eq!(arc2("2.5 mr"), Ok(Value::Radius(2500.0)));
    assert_eq!(arc2("-24r"), Err(InputError::Negative));
    assert!(matches!(arc2("abcr"), Err(InputError::NotAValue { .. })));
    assert!(matches!(arc2("r"), Err(InputError::NotAValue { .. })));
    assert_eq!(arc2("20s"), Ok(Value::Sides(20)));
    assert_eq!(arc2("0.5"), Ok(Value::Length(500.0)));
    // A radius is refused where it makes no sense.
    assert_eq!(
        run("5r", &metric(), Tool::Circle, Phase::Step(1)),
        Err(InputError::NotAccepted {
            kind: Kind::Radius,
            accepted: &[Kind::Length, Kind::Sides]
        })
    );
}

#[test]
fn points_absolute_and_relative() {
    let line = |t: &str, u: &Units| run(t, u, Tool::Line, Phase::Step(1));
    assert_eq!(
        line("[1,2,3]", &metric()),
        Ok(Value::PointAbs(DVec3::new(1.0, 2.0, 3.0)))
    );
    assert_eq!(
        line("[1;2;3]", &metric()),
        Ok(Value::PointAbs(DVec3::new(1.0, 2.0, 3.0)))
    );
    assert_eq!(
        line("[1m, 2m, 3m]", &metric()),
        Ok(Value::PointAbs(DVec3::new(1000.0, 2000.0, 3000.0)))
    );
    assert_eq!(
        line("<1.5m,4m,2.75m>", &metric()),
        Ok(Value::PointRel(DVec3::new(1500.0, 4000.0, 2750.0)))
    );
    let Ok(Value::PointAbs(p)) = line("[3',5',7']", &metric()) else {
        panic!()
    };
    assert!(p.distance(DVec3::new(914.4, 1524.0, 2133.6)) < 1e-6);
    assert_eq!(
        line("[1,5;2;3]", &estonian()),
        Ok(Value::PointAbs(DVec3::new(1500.0, 2000.0, 3000.0)))
    );
    assert_eq!(
        line("[1,2,3]", &estonian()),
        Err(InputError::BadPoint { separator: ';' })
    );
    assert_eq!(
        line("[1,2]", &metric()),
        Err(InputError::BadPoint { separator: ',' })
    );
    assert_eq!(
        line("[]", &metric()),
        Err(InputError::BadPoint { separator: ',' })
    );
    assert_eq!(line("[1,2,3>", &metric()), Err(InputError::BadBrackets));
    assert_eq!(line("[1,2,3", &metric()), Err(InputError::BadBrackets));
    assert_eq!(line("[", &metric()), Err(InputError::BadBrackets));
    assert_eq!(line("<", &metric()), Err(InputError::BadBrackets));
    assert_eq!(line("[[1,2,3]]", &metric()), Err(InputError::BadBrackets));
    assert!(matches!(
        line("[a,b,c]", &metric()),
        Err(InputError::NotAValue { .. })
    ));
    // Move takes points too, a circle does not.
    assert!(run("<1,0,0>", &metric(), Tool::Move, Phase::Done).is_ok());
    assert!(matches!(
        run("[1,2,3]", &metric(), Tool::Circle, Phase::Step(1)),
        Err(InputError::NotAccepted {
            kind: Kind::PointAbs,
            ..
        })
    ));
}

#[test]
fn empty_and_unreadable_text() {
    for t in ["", "   ", "\t"] {
        assert_eq!(mv(t, &metric()), Err(InputError::Empty));
    }
    for t in ["abc", "1.2.3.4", "m", "--5", "5 5", "5mm 3cm 2in", "0x1F"] {
        assert!(
            matches!(mv(t, &metric()), Err(InputError::NotAValue { .. })),
            "{t}"
        );
    }
    // A number too large for a length.
    assert!(mv("1e999", &metric()).is_err());
    assert!(mv("1e308m", &metric()).is_err());
}

#[test]
fn the_non_finite_check() {
    use super::parse::finite;
    assert_eq!(
        finite(Value::Length(f64::INFINITY)),
        Err(InputError::TooLarge)
    );
    assert_eq!(
        finite(Value::Pair(Some(1.0), Some(f64::NAN))),
        Err(InputError::TooLarge)
    );
    assert_eq!(
        finite(Value::PointAbs(DVec3::new(0.0, f64::INFINITY, 0.0))),
        Err(InputError::TooLarge)
    );
    assert_eq!(finite(Value::Count(3)), Ok(Value::Count(3)));
}

#[test]
fn which_tool_takes_which_kind() {
    use Kind::*;
    let a = |t, p| accepts(t, p);
    assert_eq!(
        a(Tool::Line, Phase::Step(1)).kinds,
        &[Length, PointAbs, PointRel]
    );
    assert_eq!(a(Tool::Rectangle, Phase::Done).kinds, &[Pair]);
    assert_eq!(a(Tool::Circle, Phase::Step(1)).kinds, &[Length, Sides]);
    assert_eq!(a(Tool::Polygon, Phase::Done).kinds, &[Length, Sides]);
    // Step(n) counts the clicks: center, start (radius), end (angle).
    assert_eq!(a(Tool::Circle, Phase::Step(0)).kinds, &[Sides]);
    assert_eq!(a(Tool::Arc, Phase::Step(0)).kinds, &[Sides]);
    assert_eq!(a(Tool::Arc, Phase::Step(1)).kinds, &[Length, Sides]);
    assert_eq!(a(Tool::Arc, Phase::Step(2)).kinds, &[Angle, Sides]);
    assert_eq!(a(Tool::Arc, Phase::Done).kinds, &[Sides, Radius]);
    // Start, end of the chord (length), then the bulge.
    assert_eq!(a(Tool::TwoPointArc, Phase::Step(0)).kinds, &[Sides]);
    assert_eq!(a(Tool::TwoPointArc, Phase::Step(1)).kinds, &[Length]);
    assert_eq!(
        a(Tool::TwoPointArc, Phase::Step(2)).kinds,
        &[Length, Radius, Sides]
    );
    assert_eq!(
        a(Tool::TwoPointArc, Phase::Done).kinds,
        &[Length, Radius, Sides]
    );
    assert!(
        a(Tool::Move, Phase::Copied).takes(Count)
            && !a(Tool::Move, Phase::Done).takes(Count)
            && !a(Tool::Move, Phase::Step(1)).takes(Count)
    );
    assert!(
        a(Tool::Rotate, Phase::Copied).takes(Divide)
            && !a(Tool::Rotate, Phase::Done).takes(Divide)
            && !a(Tool::Rotate, Phase::Step(1)).takes(Divide)
    );
    for t in [Tool::PushPull, Tool::Offset, Tool::TapeMeasure] {
        assert_eq!(a(t, Phase::Step(0)).kinds, &[Length]);
    }
    assert_eq!(a(Tool::Protractor, Phase::Step(0)).kinds, &[Angle]);
    for t in [
        Tool::Select,
        Tool::Eraser,
        Tool::Freehand,
        Tool::FollowMe,
        Tool::Orbit,
        Tool::Pan,
        Tool::Zoom,
        Tool::Paint,
    ] {
        let acc = a(t, Phase::Step(0));
        assert!(!acc.takes_input(), "{t:?}");
        assert_eq!(
            run("5", &metric(), t, Phase::Step(0)),
            Err(InputError::NotAccepted {
                kind: Length,
                accepted: &[]
            })
        );
    }
    assert_eq!(a(Tool::Move, Phase::Step(0)).bare, Some(Bare::Length));
    assert_eq!(a(Tool::Rotate, Phase::Step(0)).bare, Some(Bare::Angle));
    assert_eq!(
        a(Tool::Rectangle, Phase::Step(0)).bare,
        Some(Bare::PairFirst)
    );
    assert_eq!(Value::Divide(2).kind(), Divide);
    assert_eq!(Value::Sides(4).kind(), Sides);
    assert_eq!(Value::Radius(1.0).kind(), Kind::Radius);
    assert_eq!(Value::PointRel(DVec3::ZERO).kind(), PointRel);
}

#[test]
fn every_tool_phase_and_kind_agree_with_the_parser() {
    // For each tool and phase, a sample of each accepted kind is accepted and the others
    // are refused.
    let samples: [(Kind, &str); 9] = [
        (Kind::Length, "2m"),
        (Kind::Pair, "3;4"),
        (Kind::Angle, "45°"),
        (Kind::Count, "5x"),
        (Kind::Divide, "/5"),
        (Kind::Sides, "12s"),
        (Kind::Radius, "24r"),
        (Kind::PointAbs, "[1,2,3]"),
        (Kind::PointRel, "<1,2,3>"),
    ];
    let tools = [
        Tool::Line,
        Tool::Rectangle,
        Tool::Circle,
        Tool::Polygon,
        Tool::Arc,
        Tool::TwoPointArc,
        Tool::Move,
        Tool::Rotate,
        Tool::PushPull,
        Tool::Offset,
        Tool::TapeMeasure,
        Tool::Protractor,
        Tool::Select,
    ];
    for tool in tools {
        for phase in [Phase::Step(0), Phase::Step(1), Phase::Step(2), Phase::Done, Phase::Copied] {
            let acc = accepts(tool, phase);
            for (kind, text) in samples {
                let r = run(text, &metric(), tool, phase);
                // A rectangle turns a single length into a pair.
                let takes = acc.takes(kind) || (kind == Kind::Length && acc.takes(Kind::Pair));
                match r {
                    Ok(v) => assert!(
                        takes && acc.takes(v.kind()),
                        "{tool:?} {phase:?} {text}: {v:?}"
                    ),
                    Err(InputError::NotAccepted { .. }) => {
                        assert!(!takes, "{tool:?} {phase:?} {text}")
                    }
                    Err(e) => assert!(
                        !takes || matches!(e, InputError::Negative),
                        "{tool:?} {phase:?} {text}: {e:?}"
                    ),
                }
            }
        }
    }
}

#[test]
fn the_errors_speak_the_language_of_the_person() {
    let errors = vec![
        InputError::Empty,
        InputError::NotAValue { text: "abc".into() },
        InputError::NotAccepted {
            kind: Kind::Count,
            accepted: &[Kind::Length, Kind::Angle],
        },
        InputError::NotAccepted {
            kind: Kind::Count,
            accepted: &[],
        },
        InputError::TooManyValues { separator: ';' },
        InputError::BadPoint { separator: ',' },
        InputError::BadBrackets,
        InputError::CopiesZero,
        InputError::CopiesTooMany,
        InputError::DivideTooSmall,
        InputError::DivideTooMany,
        InputError::SidesRange,
        InputError::Negative,
        InputError::TooLarge,
        InputError::AmbiguousComma { text: "1,000".into() },
    ];
    for e in &errors {
        let en = e.message();
        let et = kadakas_l10n::with_language("et", || e.message());
        assert!(!en.is_empty() && !en.contains("uic-measure"), "{e:?}: {en}");
        assert!(!et.is_empty() && !et.contains("uic-measure"), "{e:?}: {et}");
        assert_ne!(en, et, "{e:?}");
        assert_eq!(e.to_string(), en);
    }
    assert_eq!(
        InputError::BadPoint { separator: ';' }.message(),
        "A point needs three values, for example [1;2;3]"
    );
    assert_eq!(
        kadakas_l10n::with_language("et", || InputError::CopiesTooMany.message()),
        "Koopiaid on kuni 1000."
    );
    let m = InputError::NotAccepted {
        kind: Kind::Count,
        accepted: &[Kind::Length, Kind::Angle],
    }
    .message();
    assert_eq!(
        m,
        "This tool does not take “a number of copies” now. It takes: a length, an angle."
    );
    let _: &dyn std::error::Error = &InputError::Empty;
    for k in [
        Kind::Length,
        Kind::Pair,
        Kind::Angle,
        Kind::Count,
        Kind::Divide,
        Kind::Sides,
        Kind::Radius,
        Kind::PointAbs,
        Kind::PointRel,
    ] {
        assert!(!k.name().starts_with("uic-"));
    }
    assert_eq!(value_separator(&metric()), ',');
    assert_eq!(value_separator(&estonian()), ';');
}

#[test]
fn the_grammar_without_a_tool() {
    assert_eq!(
        parse_any("5", &metric(), Bare::Angle),
        Ok(Value::Angle(5.0))
    );
    assert_eq!(
        parse_any("5", &metric(), Bare::Length),
        Ok(Value::Length(5.0))
    );
    assert_eq!(
        parse_any("5", &metric(), Bare::PairFirst),
        Ok(Value::Pair(Some(5.0), None))
    );
    assert_eq!(
        parse_any("", &metric(), Bare::Length),
        Err(InputError::Empty)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// A length that the units crate writes reads back as the same length.
    #[test]
    fn written_lengths_read_back(mm in -1.0e6f64..1.0e6, which in 0usize..3) {
        let u = [metric(), meters(), estonian()][which];
        let text = u.length_compact(mm);
        let back = len(mv(&text, &u));
        prop_assert!((back - mm).abs() <= 0.002 * (1.0 + mm.abs() * 1e-6) + 1e-3, "{text}: {back} vs {mm}");
    }

    /// Two values written with the separator of the locale read back.
    #[test]
    fn written_pairs_read_back(a in -1000.0f64..1000.0, b in -1000.0f64..1000.0, which in 0usize..2) {
        let u = [metric(), estonian()][which];
        let nf = u.number.plain();
        let (ta, tb) = (nf.format(a, 3), nf.format(b, 3));
        let sep = value_separator(&u);
        let v = run(&format!("{ta}{sep}{tb}"), &u, Tool::Rectangle, Phase::Step(1)).unwrap();
        let Value::Pair(Some(x), Some(y)) = v else { panic!("{v:?}") };
        let scale = if which == 1 { 1000.0 } else { 1.0 };
        prop_assert!((x - a * scale).abs() <= 6e-4 * scale);
        prop_assert!((y - b * scale).abs() <= 6e-4 * scale);
    }

    #[test]
    fn counts_and_divisions_round_trip(n in 1u32..=1000, form in 0usize..4) {
        let t = [format!("{n}x"), format!("x{n}"), format!("*{n}"), format!("{n}*")][form].clone();
        prop_assert_eq!(run(&t, &metric(), Tool::Move, Phase::Copied), Ok(Value::Count(n)));
        if n >= 2 {
            prop_assert_eq!(run(&format!("/{n}"), &metric(), Tool::Move, Phase::Copied), Ok(Value::Divide(n)));
            prop_assert_eq!(run(&format!("{n}/"), &metric(), Tool::Rotate, Phase::Copied), Ok(Value::Divide(n)));
        }
    }

    #[test]
    fn angles_round_trip(a in -720.0f64..720.0) {
        let text = format!("{a:.4}°");
        let Ok(Value::Angle(v)) = run(&text, &metric(), Tool::Rotate, Phase::Step(2)) else { panic!("{text}") };
        prop_assert!((v - a).abs() < 1e-3);
    }

    #[test]
    fn points_round_trip(x in -1.0e5f64..1.0e5, y in -1.0e5f64..1.0e5, z in -1.0e5f64..1.0e5, rel in any::<bool>(), which in 0usize..2) {
        let u = [metric(), estonian()][which];
        let nf = u.number.plain();
        let s = value_separator(&u);
        let body = format!("{}{s}{}{s}{}", nf.format(x, 2), nf.format(y, 2), nf.format(z, 2));
        let text = if rel { format!("<{body}>") } else { format!("[{body}]") };
        let v = run(&text, &u, Tool::Move, Phase::Step(1)).unwrap();
        let scale = if which == 1 { 1000.0 } else { 1.0 };
        let (Value::PointAbs(p) | Value::PointRel(p)) = v else { panic!("{v:?}") };
        prop_assert_eq!(matches!(v, Value::PointRel(_)), rel);
        prop_assert!(p.distance(DVec3::new(x, y, z) * scale) < 0.01 * scale * 3.0);
    }

    /// Whatever is typed, the parser answers and never panics; every error has a message.
    #[test]
    fn any_text_gives_a_value_or_an_error(text in "\\PC{0,24}", which in 0usize..3, tool in 0usize..13, step in 0u8..3, done in any::<bool>()) {
        let tools = [Tool::Line, Tool::Rectangle, Tool::Circle, Tool::Polygon, Tool::Arc, Tool::TwoPointArc, Tool::Move, Tool::Rotate, Tool::PushPull, Tool::Offset, Tool::TapeMeasure, Tool::Protractor, Tool::Select];
        let u = [metric(), estonian(), inches()][which];
        let phase = if done { [Phase::Done, Phase::Copied][usize::from(step % 2)] } else { Phase::Step(step) };
        match run(&text, &u, tools[tool], phase) {
            Ok(v) => prop_assert!(accepts(tools[tool], phase).takes(v.kind())),
            Err(e) => prop_assert!(!e.message().is_empty()),
        }
    }

    /// Texts built from the characters of the grammar.
    #[test]
    fn grammar_characters_never_panic(text in "[0-9a-z.,;:'\"°/*x<>\\[\\] +-]{0,16}", which in 0usize..2) {
        let u = [metric(), estonian()][which];
        for tool in [Tool::Move, Tool::Rectangle, Tool::Rotate, Tool::Circle] {
            let _ = run(&text, &u, tool, Phase::Done);
        }
    }
}

#[test]
fn a_bare_number_before_the_first_click_is_the_sides() {
    for tool in [Tool::Circle, Tool::Polygon, Tool::Arc, Tool::TwoPointArc] {
        let at = |t: &str| run(t, &metric(), tool, Phase::Step(0));
        assert_eq!(at("24"), Ok(Value::Sides(24)), "{tool:?}");
        assert_eq!(at("24s"), Ok(Value::Sides(24)), "{tool:?}");
        assert_eq!(at("2"), Err(InputError::SidesRange), "{tool:?}");
        assert!(
            matches!(at("2m"), Err(InputError::NotAccepted { kind: Kind::Length, .. })),
            "{tool:?}"
        );
        assert!(
            matches!(at("2.5"), Err(InputError::NotAccepted { kind: Kind::Length, .. })),
            "{tool:?}"
        );
    }
    // After the center is clicked a bare number is the radius.
    assert_eq!(
        run("24", &meters(), Tool::Circle, Phase::Step(1)),
        Ok(Value::Length(24000.0))
    );
    assert_eq!(
        run("24", &meters(), Tool::Arc, Phase::Step(1)),
        Ok(Value::Length(24000.0))
    );
    // The arc's angle comes after the second click.
    assert_eq!(
        run("90", &metric(), Tool::Arc, Phase::Step(2)),
        Ok(Value::Angle(90.0))
    );
    assert!(run("90", &metric(), Tool::Arc, Phase::Step(1)).is_ok_and(|v| v.kind() == Kind::Length));
}

#[test]
fn a_bare_number_after_an_arc_is_refused_and_says_what_is_taken() {
    assert_eq!(accepts(Tool::Arc, Phase::Done).bare, None);
    assert_eq!(
        run("5", &metric(), Tool::Arc, Phase::Done),
        Err(InputError::NotAccepted {
            kind: Kind::Length,
            accepted: &[Kind::Sides, Kind::Radius]
        })
    );
    assert_eq!(run("5r", &metric(), Tool::Arc, Phase::Done), Ok(Value::Radius(5.0)));
}

#[test]
fn one_comma_and_three_digits_is_not_guessed() {
    // A decimal point: `1,000` may be a thousands group, so a length refuses it.
    for t in ["1,000", "12,345", "-1,000"] {
        assert!(
            matches!(mv(t, &metric()), Err(InputError::AmbiguousComma { .. })),
            "{t}"
        );
        assert!(
            matches!(
                run(t, &metric(), Tool::Circle, Phase::Step(1)),
                Err(InputError::AmbiguousComma { .. })
            ),
            "{t}"
        );
    }
    // A decimal comma habit still works with other digit counts.
    assert!(near(len(mv("2,5", &metric())), 2.5));
    assert!(near(len(mv("1,50", &metric())), 1.5));
    assert!(near(len(mv("1,0000", &metric())), 1.0));
    // Where a pair is accepted it is a pair.
    assert_eq!(
        run("1,000", &metric(), Tool::Rectangle, Phase::Step(1)),
        Ok(Value::Pair(Some(1.0), Some(0.0)))
    );
    // A decimal comma: `1,000` is exactly one.
    assert!(near(len(mv("1,000", &estonian())), 1000.0));
    // The message is in both languages and names what was typed.
    let e = mv("1,000", &metric()).unwrap_err();
    assert!(e.message().contains("1,000"));
    assert_ne!(e.message(), kadakas_l10n::with_language("et", || e.message()));
}
