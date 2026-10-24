use super::*;
use kadakas_core::pattern::{PatternSeg, hatch_segments};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn square(side: f64) -> Vec<DVec2> {
    vec![v(0.0, 0.0), v(side, 0.0), v(side, side), v(0.0, side)]
}

fn def_of(lines: Vec<HatchLine>) -> HatchDef {
    HatchDef {
        units: PatternUnits::World,
        lines,
    }
}

fn hatch_of(name: &str) -> HatchDef {
    def_of(
        standard(name)
            .unwrap()
            .lines
            .iter()
            .map(|l| l.to_hatch().unwrap())
            .collect(),
    )
}

/// The dashes of a DXF line family inside a square window, found the way the DXF reference says:
/// line k starts at `base + k * offset`, and the dash items run along the line from there, in
/// both directions. Returns the pieces (start, end).
fn reference(p: &PatLine, side: f64) -> Vec<(DVec2, DVec2)> {
    let u = p.dir();
    let n = DVec2::new(-u.y, u.x);
    let perp = p.offset.dot(n);
    let corners = square(side);
    let across: Vec<f64> = corners.iter().map(|c| n.dot(*c - p.base)).collect();
    let lo = across.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = across.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let (a, b) = (lo / perp, hi / perp);
    let (k0, k1) = (a.min(b).floor() as i64 - 1, a.max(b).ceil() as i64 + 1);
    let mut out = Vec::new();
    for k in k0..=k1 {
        let start = p.base + p.offset * k as f64;
        // The window along the line, as a range of t from `start`.
        let ts: Vec<f64> = corners.iter().map(|c| u.dot(*c - start)).collect();
        // A little more than the window: a piece at the edge is then not lost to rounding.
        let t0 = ts.iter().cloned().fold(f64::INFINITY, f64::min) - 0.5;
        let t1 = ts.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 0.5;
        if p.dashes.is_empty() {
            out.push((start + u * t0, start + u * t1));
            continue;
        }
        let period: f64 = p.dashes.iter().map(|d| d.abs()).sum();
        let mut cycle = (t0 / period).floor() * period;
        while cycle < t1 {
            let mut t = cycle;
            for d in &p.dashes {
                let len = d.abs();
                if *d >= 0.0 {
                    let (s, e) = (t.max(t0), (t + len).min(t1));
                    if e >= s {
                        out.push((start + u * s, start + u * e));
                    }
                }
                t += len;
            }
            cycle += period;
        }
    }
    out
}

fn on_segment(p: DVec2, a: DVec2, b: DVec2, tol: f64) -> bool {
    kadakas_core::geom::dist_to_segment(p, a, b) <= tol
}

fn length(s: &[PatternSeg]) -> f64 {
    s.iter().map(|g| g.a.distance(g.b)).sum()
}

#[test]
fn the_table_is_read_and_every_line_converts() {
    let names: Vec<_> = standard_names().collect();
    assert!(names.len() >= 100, "{}", names.len());
    for n in [
        "ANSI31", "ANSI32", "ANSI33", "ANSI34", "ANSI35", "ANSI36", "ANSI37", "ANSI38", "AR-B816",
        "AR-B88", "AR-CONC", "AR-HBONE", "AR-PARQ1", "AR-SAND", "EARTH", "GRASS", "GRAVEL",
        "STEEL", "NET", "BRICK", "DASH", "HEX", "ZIGZAG",
    ] {
        assert!(standard(n).is_some(), "{n} is not in the table");
    }
    assert!(standard("  ansi31 ").is_some());
    assert!(standard("NO-SUCH-PATTERN").is_none());
    for std in table() {
        assert!(!std.lines.is_empty(), "{}", std.name);
        assert!(std.lines.len() <= MAX_LINES, "{}", std.name);
        for l in &std.lines {
            let h = l.to_hatch();
            assert!(
                h.is_some(),
                "{} has a line that is not a hatch line",
                std.name
            );
            assert!(h.unwrap().dash.len() <= MAX_DASHES, "{}", std.name);
        }
    }
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len());
}

#[test]
fn a_hatch_line_and_a_pattern_line_are_the_same_lines() {
    let (mut negative, mut staggered) = (0, 0);
    for std in table() {
        for l in &std.lines {
            let h = l.to_hatch().unwrap();
            let back = PatLine::from(&h);
            // A family that runs to the other side comes back counted from the other end.
            assert!(
                (back.offset - l.offset).length() < 1e-9
                    || (back.offset + l.offset).length() < 1e-9,
                "{}",
                std.name
            );
            assert!((back.base - l.base).length() < 1e-9, "{}", std.name);
            assert_eq!(back.dashes.len(), dash_items(&dash_list(&l.dashes)).len());
            // The period is the same, also with dots.
            let period = |d: &[f64]| d.iter().map(|x| x.abs()).sum::<f64>();
            if !l.dashes.is_empty() && h.dash.len() >= 2 {
                assert!(
                    (period(&l.dashes) - h.dash.iter().sum::<f64>()).abs() < 1e-6,
                    "{}",
                    std.name
                );
                assert!(
                    (period(&l.dashes) - period(&back.dashes)).abs() < 1e-6,
                    "{}",
                    std.name
                );
            }
            let n = DVec2::from_angle(l.angle.to_radians()).perp();
            if l.offset.dot(n) < 0.0 {
                negative += 1;
            }
            if h.offset.abs() > 1e-9 {
                staggered += 1;
            }
        }
    }
    // The table has families that run to the other side and families with a stagger, so the
    // checks of this file cover both.
    assert!(negative > 0, "no line runs to the other side");
    assert!(staggered > 0, "no line has a stagger");
}

#[test]
fn the_lines_of_a_hatch_are_where_the_dxf_reference_puts_them() {
    // Each standard pattern in a 60 mm window: every dash of the DXF rule is on a line that the
    // hatch of the document draws, and the other way round. A dot is 0.1 mm long in the hatch.
    let side = 60.0;
    let tol = DOT_MM + 1e-6;
    let inside = |m: DVec2| m.x > 0.01 && m.y > 0.01 && m.x < side - 0.01 && m.y < side - 0.01;
    let mut checked = 0;
    for std in table() {
        let hatch = hatch_of(std.name);
        // A pattern with very fine lines is left out: it makes a huge list.
        if hatch.lines.iter().any(|h| h.spacing < 0.5) {
            continue;
        }
        let ours = hatch_segments(&hatch, &[square(side)], 1.0);
        let mut theirs: Vec<(DVec2, DVec2)> = Vec::new();
        for l in &std.lines {
            theirs.extend(reference(l, side));
        }
        for (a, b) in theirs.iter().step_by(1 + theirs.len() / 150) {
            let mid = (*a + *b) / 2.0;
            if a.distance(*b) < 1e-6 || !inside(mid) {
                continue;
            }
            assert!(
                ours.iter().any(|s| on_segment(mid, s.a, s.b, tol)),
                "{}: a dash at {mid:?} is missing",
                std.name
            );
        }
        for s in ours.iter().step_by(1 + ours.len() / 150) {
            let mid = (s.a + s.b) / 2.0;
            if !inside(mid) {
                continue;
            }
            assert!(
                theirs.iter().any(|(a, b)| on_segment(mid, *a, *b, tol)),
                "{}: our dash at {mid:?} is not in the file",
                std.name
            );
        }
        checked += 1;
    }
    assert!(checked > 60, "{checked} patterns were checked");
}

#[test]
fn a_family_that_runs_to_the_other_side_keeps_its_stagger() {
    let a = PatLine {
        angle: 0.0,
        base: v(0.0, 0.0),
        offset: v(1.0, 3.0),
        dashes: vec![4.0, -2.0],
    };
    // The same lines, listed from the other side: they go down, not up.
    let b = PatLine {
        angle: 0.0,
        base: v(0.0, 0.0),
        offset: v(-1.0, -3.0),
        dashes: vec![4.0, -2.0],
    };
    let (ha, hb) = (a.to_hatch().unwrap(), b.to_hatch().unwrap());
    assert_eq!((ha.spacing, ha.offset), (3.0, 1.0));
    assert_eq!((hb.spacing, hb.offset), (3.0, 1.0));
    for p in [&a, &b] {
        let h = def_of(vec![p.to_hatch().unwrap()]);
        let ours = hatch_segments(&h, &[square(12.0)], 1.0);
        for (s, e) in reference(p, 12.0) {
            let mid = (s + e) / 2.0;
            if s.distance(e) < 1e-6 || mid.min_element() < 0.01 || mid.max_element() > 11.99 {
                continue;
            }
            assert!(
                ours.iter().any(|g| on_segment(mid, g.a, g.b, 1e-6)),
                "{p:?} {mid:?}"
            );
        }
    }
}

#[test]
fn a_family_whose_lines_lie_on_each_other_is_not_a_hatch_line() {
    let flat = PatLine {
        angle: 0.0,
        base: DVec2::ZERO,
        offset: v(5.0, 0.0),
        dashes: vec![],
    };
    assert!(flat.to_hatch().is_none());
    let nan = PatLine {
        angle: f64::NAN,
        ..flat.clone()
    };
    assert!(nan.to_hatch().is_none());
    let far = PatLine {
        offset: v(0.0, 2.0e9),
        ..flat.clone()
    };
    assert!(far.to_hatch().is_none());
    let tiny = PatLine {
        offset: v(0.0, 1.0e-9),
        ..flat
    };
    assert!(tiny.to_hatch().is_none());
}

#[test]
fn dash_items_become_a_dash_list() {
    assert_eq!(dash_list(&[]), Vec::<f64>::new());
    assert_eq!(dash_list(&[5.0, -2.0]), vec![5.0, 2.0]);
    // A dot is a short dash.
    // The gap after it is shorter, so the period stays 3.
    assert_eq!(dash_list(&[0.0, -3.0]), vec![DOT_MM, 3.0 - DOT_MM]);
    // A list that starts with a gap starts with a dash of length 0. A list that ends with a dash gets a gap of 0.
    assert_eq!(dash_list(&[-2.0, 3.0]), vec![0.0, 2.0, 3.0, 0.0]);
    assert_eq!(dash_list(&[1.0, -2.0, 3.0]), vec![1.0, 2.0, 3.0, 0.0]);
    assert_eq!(dash_list(&[-2.0]), vec![0.0, 2.0]);
    // Items of one kind in a row add up.
    assert_eq!(dash_list(&[2.0, 3.0, -1.0, -1.0]), vec![5.0, 2.0]);
    // Dashes only: a solid line.
    assert!(dash_list(&[5.0]).is_empty());
    assert!(dash_list(&[5.0, 2.0]).is_empty());
    // Not numbers are skipped.
    assert_eq!(dash_list(&[f64::NAN, 1.0, -1.0]), vec![1.0, 1.0]);
    // Too long a list is cut to the limit.
    let long: Vec<f64> = (0..200).flat_map(|_| [1.0, -1.0]).collect();
    assert_eq!(dash_list(&long).len(), MAX_DASHES);
    // The other way.
    assert_eq!(dash_items(&[5.0, 2.0]), vec![5.0, -2.0]);
    assert_eq!(dash_items(&[DOT_MM, 3.0 - DOT_MM]), vec![0.0, -3.0]);
    assert_eq!(dash_items(&[4.0]), vec![4.0]);
    assert!(dash_items(&[]).is_empty());
    assert!(dash_items(&[0.0, 0.0]).is_empty());
}

#[test]
fn the_origin_is_reduced_without_changing_the_lines() {
    let line = HatchLine {
        angle: 30.0,
        spacing: 4.0,
        offset: 1.5,
        origin: v(1234.5, -987.25),
        dash: vec![3.0, 1.0],
        color: None,
        weight: None,
    };
    let r = reduced(&line);
    assert!(r.origin.length() < 10.0, "{:?}", r.origin);
    let a = hatch_segments(&def_of(vec![line]), &[square(50.0)], 1.0);
    let b = hatch_segments(&def_of(vec![r]), &[square(50.0)], 1.0);
    assert!((length(&a) - length(&b)).abs() < 1e-3);
    for s in a.iter().step_by(7) {
        let mid = (s.a + s.b) / 2.0;
        assert!(b.iter().any(|g| on_segment(mid, g.a, g.b, 1e-4)));
    }
    // A solid line has no origin along itself.
    let solid = HatchLine::solid(0.0, 2.0);
    let moved = HatchLine {
        origin: v(100.0, 7.0),
        ..solid.clone()
    };
    assert_eq!(reduced(&moved).origin, v(0.0, 1.0));
    // Two hatches that differ by a whole step have the same reduced origin.
    let again = HatchLine {
        origin: v(-55.0, 1.0 + 2.0 * 40.0),
        ..solid
    };
    assert_eq!(reduced(&again).origin, v(0.0, 1.0));
}

#[test]
fn a_standard_pattern_is_found_again_at_a_scale_and_a_turn() {
    let scale = 2.5;
    let turn = 30.0;
    let r = DVec2::from_angle(f64::to_radians(turn)) * scale;
    let cols = [r, DVec2::new(-r.y, r.x)];
    let lines: Vec<HatchLine> = standard("ANSI32")
        .unwrap()
        .lines
        .iter()
        .map(|l| l.mapped(cols, v(10.0, 5.0)).to_hatch().unwrap())
        .collect();
    let n = identify(&def_of(lines.clone())).unwrap();
    assert_eq!(n.name, "ANSI32");
    assert!(
        (n.scale - scale).abs() < 1e-9 && (n.angle - turn).abs() < 1e-6,
        "{n:?}"
    );
    assert_eq!(display_name(&n), "ANSI32 x2.5 30 deg");
    // The name in the file wins when the lines agree with it.
    let plain = hatch_of("ANSI31");
    assert_eq!(identify_as(&plain, Some("ansi31")).unwrap().name, "ANSI31");
    let n = identify(&plain).unwrap();
    assert_eq!((n.name, n.scale, n.angle), ("ANSI31", 1.0, 0.0));
    assert_eq!(display_name(&n), "ANSI31");
    // A pattern that is not in the table is not named.
    assert!(identify(&def_of(vec![HatchLine::solid(13.0, 7.77)])).is_none());
    assert!(identify(&def_of(vec![])).is_none());
    // A different number of lines is not the same pattern.
    let mut two = lines;
    two.push(HatchLine::solid(0.0, 1.0));
    assert!(identify(&def_of(two)).is_none());
    // Every pattern of the table is found under its own name.
    for std in table() {
        let n = identify_as(&hatch_of(std.name), Some(std.name)).unwrap();
        assert_eq!(n.name, std.name);
    }
}

#[test]
fn definitions_are_compared_by_their_lines() {
    let a = def_of(vec![HatchLine::solid(45.0, 3.0)]);
    let mut b = a.clone();
    assert!(same_def(&a, &b));
    b.lines[0].spacing = 3.000_000_1;
    assert!(same_def(&a, &b));
    b.lines[0].spacing = 3.1;
    assert!(!same_def(&a, &b));
    let mut c = a.clone();
    c.lines[0].dash = vec![1.0, 1.0];
    assert!(!same_def(&a, &c));
    let mut d = a.clone();
    d.lines[0].color = Some(kadakas_core::model::Color::BLACK);
    assert!(!same_def(&a, &d));
    let paper = HatchDef {
        units: PatternUnits::Paper,
        ..a.clone()
    };
    assert!(!same_def(&a, &paper));
    assert!(!same_def(&a, &def_of(vec![])));
}

#[test]
fn a_map_turns_stretches_and_moves_a_family() {
    let l = PatLine {
        angle: 0.0,
        base: v(1.0, 0.0),
        offset: v(0.0, 2.0),
        dashes: vec![3.0, -1.0],
    };
    // Scale 2 and a quarter turn, then a move.
    let m = l.mapped([v(0.0, 2.0), v(-2.0, 0.0)], v(5.0, 5.0));
    assert!((m.angle - 90.0).abs() < 1e-9);
    assert!((m.base - v(5.0, 7.0)).length() < 1e-9);
    assert!((m.offset - v(-4.0, 0.0)).length() < 1e-9);
    assert_eq!(m.dashes, vec![6.0, -2.0]);
    // A mirror turns 30 degrees into 150.
    let mirror = PatLine {
        angle: 30.0,
        ..l.clone()
    }
    .mapped([v(-1.0, 0.0), v(0.0, 1.0)], DVec2::ZERO);
    assert!((mirror.angle - 150.0).abs() < 1e-9);
    // A map that flattens everything gives angle 0 and no length.
    let flat = l.mapped([DVec2::ZERO, DVec2::ZERO], DVec2::ZERO);
    assert_eq!(flat.angle, 0.0);
    assert!(flat.to_hatch().is_none());
    // The second half of a double hatch.
    let t = l.turned_90();
    assert_eq!(t.angle, 90.0);
    assert!((t.base - v(0.0, 1.0)).length() < 1e-9);
    assert!((t.offset - v(-2.0, 0.0)).length() < 1e-9);
}
