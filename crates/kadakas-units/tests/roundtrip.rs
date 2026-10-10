//! Property tests: what the formatter writes, the parser reads again; and the parsers never
//! panic, whatever the text.

use kadakas_units::*;
use proptest::prelude::*;

fn number_format() -> impl Strategy<Value = NumberFormat> {
    (
        prop_oneof![Just(Decimal::Point), Just(Decimal::Comma)],
        prop_oneof![
            Just(Grouping::None),
            Just(Grouping::Comma),
            Just(Grouping::Point),
            Just(Grouping::Space)
        ],
    )
        .prop_filter("the same symbol twice is not a format", |(d, g)| {
            !matches!(
                (d, g),
                (Decimal::Point, Grouping::Point) | (Decimal::Comma, Grouping::Comma)
            )
        })
        .prop_map(|(decimal, grouping)| NumberFormat { decimal, grouping })
}

fn length_unit() -> impl Strategy<Value = LengthUnit> {
    prop::sample::select(LengthUnit::ALL.to_vec())
}

fn angle_format() -> impl Strategy<Value = AngleFormat> {
    prop::sample::select(AngleFormat::ALL.to_vec())
}

fn fraction() -> impl Strategy<Value = u32> {
    prop::sample::select(vec![0u32, 2, 4, 8, 16, 32, 64])
}

/// The difference of two angles in degrees, on the circle.
fn circle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

proptest! {
    #[test]
    fn numbers_round_trip(v in -1e9f64..1e9, d in 0usize..6, nf in number_format()) {
        let text = nf.format(v, d);
        let back = nf.parse(&text).expect("the formatter's text parses");
        prop_assert!((back - round_half_away(v, d)).abs() <= 1e-9 * v.abs().max(1.0), "{text} {back}");
    }

    #[test]
    fn plain_numbers_round_trip_in_every_format(v in -1e6f64..1e6, d in 0usize..6, nf in number_format()) {
        let text = nf.plain().format(v, d);
        let back = nf.parse(&text).expect("parses");
        prop_assert!((back - round_half_away(v, d)).abs() <= 1e-9 * v.abs().max(1.0));
        // The other format reads a plain number of its own kind as well.
        let other = NumberFormat { decimal: if nf.decimal == Decimal::Point { Decimal::Comma } else { Decimal::Point }, ..nf.plain() };
        prop_assert!(other.parse(&text).is_some());
    }

    #[test]
    fn lengths_round_trip(
        mm in -1e7f64..1e7,
        unit in length_unit(),
        frac in fraction(),
        d in 0usize..4,
        nf in number_format(),
    ) {
        let text = format_length(mm, unit, d, frac, &nf);
        let back = parse_length(&text, unit, &nf).expect("the formatter's text parses");
        // The text is rounded to a step; the parse gives the step back (half a step at most).
        let step_mm = match unit {
            LengthUnit::In | LengthUnit::FtIn if frac > 0 => 25.4 / f64::from(frac),
            LengthUnit::Ft => 304.8 * 10f64.powi(-(d.max(1) as i32)),
            u => u.mm_per_unit() * 10f64.powi(-(d as i32)),
        };
        prop_assert!((back - mm).abs() <= step_mm / 2.0 + 1e-6, "{text}: {mm} -> {back}");
        // Writing the parsed value gives the same text.
        prop_assert_eq!(format_length(back, unit, d, frac, &nf), text);
    }

    #[test]
    fn typed_units_win_over_the_default(mm in -1e6f64..1e6, default in length_unit(), nf in number_format()) {
        let text = format_length(mm, LengthUnit::Mm, 3, 16, &nf);
        let back = parse_length(&text, default, &nf).expect("parses");
        prop_assert!((back - mm).abs() <= 0.0005 + 1e-9);
    }

    #[test]
    fn edit_fields_round_trip(mm in -1e7f64..1e7, unit in length_unit(), d in 1usize..4, nf in number_format()) {
        let u = Units { length: unit, number: nf, ..Units::METRIC };
        let text = u.length_field(mm, d);
        let back = u.parse_length(&text).expect("parses");
        let step = match unit {
            LengthUnit::In | LengthUnit::FtIn => 25.4 / 16.0,
            u => u.mm_per_unit() * 10f64.powi(-(d as i32)),
        };
        prop_assert!((back - mm).abs() <= step / 2.0 + 1e-6, "{text}");
    }

    #[test]
    fn areas_round_trip(mm2 in -1e14f64..1e14, unit in prop::sample::select(AreaUnit::ALL.to_vec()), d in 0usize..5, nf in number_format()) {
        let text = format_area(mm2, unit, d, &nf);
        let back = parse_area(&text, AreaUnit::M2, &nf).expect("parses");
        let step = unit.mm2_per_unit() * 10f64.powi(-(d as i32));
        prop_assert!((back - mm2).abs() <= step / 2.0 + 1e-3 + 1e-12 * mm2.abs(), "{text}");
    }

    #[test]
    fn angles_round_trip(deg in -720f64..720.0, fmt in angle_format(), d in 0usize..4, nf in number_format()) {
        let text = format_angle(deg, fmt, d, &nf);
        let back = parse_angle(&text, &nf).expect("the formatter's text parses");
        let step = match fmt {
            AngleFormat::Decimal => 10f64.powi(-(d as i32)),
            _ => 10f64.powi(-(d as i32)) / 3600.0,
        };
        prop_assert!(circle_diff(back, deg) <= step / 2.0 + 1e-9, "{text}: {deg} -> {back}");
        if fmt == AngleFormat::Decimal {
            prop_assert!((back - deg).abs() <= step / 2.0 + 1e-9);
        }
    }

    #[test]
    fn dates_round_trip(days in -700_000i64..2_000_000, fmt in prop::sample::select(DateFormat::ALL.to_vec())) {
        let d = Date::from_days(days);
        prop_assert_eq!(d.to_days(), days);
        prop_assert!(Date::new(d.year, d.month, d.day).is_some());
        let text = fmt.format(d);
        prop_assert_eq!(fmt.parse(&text), Some(d));
        prop_assert_eq!(DateFormat::Iso.parse(&DateFormat::Iso.format(d)), Some(d));
        prop_assert!(d.weekday() < 7);
    }

    #[test]
    fn no_parser_panics(s in "\\PC{0,24}", nf in number_format(), unit in length_unit()) {
        let _ = nf.parse(&s);
        let _ = parse_length(&s, unit, &nf);
        let _ = parse_area(&s, AreaUnit::Ha, &nf);
        let _ = parse_angle(&s, &nf);
        let _ = DateFormat::Dmy.parse(&s);
    }

    #[test]
    fn parsers_that_accept_give_finite_numbers(s in "[0-9.,'\" /a-zA-Z°-]{0,16}", nf in number_format(), unit in length_unit()) {
        for v in [nf.parse(&s), parse_length(&s, unit, &nf), parse_area(&s, AreaUnit::M2, &nf), parse_angle(&s, &nf)].into_iter().flatten() {
            prop_assert!(v.is_finite());
        }
    }
}
