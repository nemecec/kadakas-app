//! Damaged geometry entities: each one is skipped with a reason, the rest of the file is read,
//! and no input makes the reader panic.

use kadakas_step::{StepOptions, read};

const HEAD: &str = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n";
const TAIL: &str = "ENDSEC;\nEND-ISO-10303-21;\n";

/// Common entities: points #1 to #4, a direction #5, a zero direction #6, a vector #7, a zero vector
/// #8, an axis #9, a plane #10, a circle #11 and a line #12.
const BASE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(10.,0.,0.));\n\
#3=CARTESIAN_POINT('',(10.,10.,0.));\n#4=CARTESIAN_POINT('',(0.,10.,0.));\n\
#5=DIRECTION('',(0.,0.,1.));\n#6=DIRECTION('',(0.,0.,0.));\n#7=VECTOR('',#5,5.);\n#8=VECTOR('',#5,0.);\n\
#9=AXIS2_PLACEMENT_3D('',#1,#5,$);\n#10=PLANE('',#9);\n#11=CIRCLE('',#9,4.);\n#12=LINE('',#1,#7);\n";

fn curves(list: &str) -> kadakas_step::StepImport {
    let body = format!("{BASE}{list}");
    let ids: Vec<String> = (100..100 + list.lines().count())
        .map(|i| format!("#{i}"))
        .collect();
    let set = format!("#999=GEOMETRIC_SET('s',(#11,{}));\n", ids.join(","));
    read(
        format!("{HEAD}{body}{set}{TAIL}").as_bytes(),
        &StepOptions::default(),
    )
    .unwrap()
}

#[test]
fn damaged_curves_are_skipped_one_by_one() {
    let bad = [
        "#100=LINE('',#1,#8);",
        "#101=LINE('',#99,#7);",
        "#102=LINE('',#1,#2);",
        "#103=CIRCLE('',#9,-4.);",
        "#104=CIRCLE('',#6,4.);",
        "#105=CIRCLE('',#1,4.);",
        "#106=ELLIPSE('',#9,3.,0.);",
        "#107=ELLIPSE('',#9,3.);",
        "#108=POLYLINE('',(#1));",
        "#109=POLYLINE('',(#1,#99));",
        "#110=TRIMMED_CURVE('',#11,(),(),.T.,.PARAMETER.);",
        "#111=TRIMMED_CURVE('',#99,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);",
        "#112=TRIMMED_CURVE('',#11,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.E30)),.T.,.PARAMETER.);",
        "#113=COMPOSITE_CURVE('',(),.F.);",
        "#114=COMPOSITE_CURVE('',(#1),.F.);",
        "#115=B_SPLINE_CURVE_WITH_KNOTS('',0,(#1,#2),.UNSPECIFIED.,.F.,.F.,(1,1),(0.,1.),.UNSPECIFIED.);",
        "#116=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#2),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,0.),.UNSPECIFIED.);",
        "#117=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#2),.UNSPECIFIED.,.F.,.F.,(2,2),(1.,0.),.UNSPECIFIED.);",
        "#118=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.UNSPECIFIED.);",
        "#119=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#2),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.));",
        "#120=(BOUNDED_CURVE() B_SPLINE_CURVE(1,(#1,#2),.UNSPECIFIED.,.F.,.F.) B_SPLINE_CURVE_WITH_KNOTS((2,2),(0.,1.),.UNSPECIFIED.) RATIONAL_B_SPLINE_CURVE((1.,-1.)));",
        "#121=(BOUNDED_CURVE() B_SPLINE_CURVE(1,(#1,#2),.UNSPECIFIED.,.F.,.F.) RATIONAL_B_SPLINE_CURVE((1.)));",
        "#122=SURFACE_CURVE('',#99,(),.CURVE_3D.);",
        "#123=HYPERBOLA('',#9,1.,1.);",
        "#124=CARTESIAN_POINT('',(1.E400,0.,0.));",
        "#125=CARTESIAN_POINT('',());",
        "#126=DIRECTION('',(1.));",
        "#127=CIRCLE('',#128,3.);",
        "#128=AXIS2_PLACEMENT_3D('',#124,#5,$);",
        "#129=CIRCLE('',#130,3.);",
        "#130=AXIS2_PLACEMENT_3D('',#1,#6,$);",
        "#131=CIRCLE('',#132,3.);",
        "#132=AXIS2_PLACEMENT_3D('',#1,#5,#6);",
        "#133=POLYLINE('',(#1,#2,#126));",
    ];
    let i = curves(&bad.join("\n"));
    // The good circle, a trimmed circle with no usable trims (the whole circle), one with a huge
    // trim (wrapped into one turn) and a spline with a missing knot spec (not needed).
    assert_eq!(i.curves.len(), 4);
    assert!(i.report.entities_read == i.report.entities_used + i.report.entities_skipped);
    assert!(
        i.report.skipped.iter().map(|s| s.2).sum::<u64>() >= 20,
        "{:?}",
        i.report.skipped
    );
}

#[test]
fn damaged_surfaces_skip_their_faces() {
    // A face on each damaged surface, with an outline from the four points.
    let surfaces = [
        "#200=PLANE('',#99);",
        "#201=CYLINDRICAL_SURFACE('',#9,0.);",
        "#202=CYLINDRICAL_SURFACE('',#9,$);",
        "#203=CONICAL_SURFACE('',#9,3.,0.);",
        "#204=CONICAL_SURFACE('',#9,3.,1.6);",
        "#205=CONICAL_SURFACE('',#9,-3.,0.5);",
        "#206=SPHERICAL_SURFACE('',#9,-1.);",
        "#207=TOROIDAL_SURFACE('',#9,2.,3.);",
        "#208=TOROIDAL_SURFACE('',#9,2.,0.);",
        "#209=SURFACE_OF_REVOLUTION('',#99,#9);",
        "#210=SURFACE_OF_REVOLUTION('',#12,#9);",
        "#211=SURFACE_OF_LINEAR_EXTRUSION('',#11,#8);",
        "#212=SURFACE_OF_LINEAR_EXTRUSION('',#12,#7);",
        "#213=SURFACE_OF_LINEAR_EXTRUSION('',#99,#7);",
        "#214=RECTANGULAR_TRIMMED_SURFACE('',#99,0.,1.,0.,1.,.T.,.T.);",
        "#215=B_SPLINE_SURFACE_WITH_KNOTS('',0,1,((#1,#2),(#4,#3)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);",
        "#216=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#1,#2),(#4)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);",
        "#217=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,(),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);",
        "#218=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#1,#2),(#4,#3)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(1.,0.),(0.,1.),.UNSPECIFIED.);",
        "#219=(BOUNDED_SURFACE() B_SPLINE_SURFACE(1,1,((#1,#2),(#4,#3)),.UNSPECIFIED.,.F.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS((2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.) RATIONAL_B_SPLINE_SURFACE(((1.,1.),(1.,-1.))));",
        "#220=(BOUNDED_SURFACE() B_SPLINE_SURFACE(1,1,((#1,#2),(#4,#3)),.UNSPECIFIED.,.F.,.F.,.F.) RATIONAL_B_SPLINE_SURFACE(((1.),(1.))));",
        "#221=(BOUNDED_SURFACE() B_SPLINE_SURFACE(1,1,((#1,#2),(#4,#3)),.UNSPECIFIED.,.F.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS((2,2),(2,2),(0.,1.)));",
        "#222=B_SPLINE_SURFACE_WITH_KNOTS('',99,1,((#1,#2),(#4,#3)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);",
        "#223=HELICOIDAL_SURFACE('',#9);",
    ];
    // Each surface gets a face on a loop of four edges.
    let mut body = String::from(BASE);
    body.push_str("#20=VERTEX_POINT('',#1);\n#21=VERTEX_POINT('',#2);\n#22=VERTEX_POINT('',#3);\n#23=VERTEX_POINT('',#4);\n");
    let pts = ["#20", "#21", "#22", "#23"];
    let lines = ["#12", "#12", "#12", "#12"];
    for k in 0..4 {
        body.push_str(&format!(
            "#{}=EDGE_CURVE('',{},{},{},.T.);\n#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n",
            30 + 2 * k,
            pts[k],
            pts[(k + 1) % 4],
            lines[k],
            31 + 2 * k,
            30 + 2 * k
        ));
    }
    body.push_str("#40=EDGE_LOOP('',(#31,#33,#35,#37));\n#41=FACE_OUTER_BOUND('',#40,.T.);\n");
    let mut faces = Vec::new();
    for (k, s) in surfaces.iter().enumerate() {
        body.push_str(s);
        body.push('\n');
        let id = 200 + k;
        body.push_str(&format!(
            "#{}=ADVANCED_FACE('',(#41),#{id},.T.);\n",
            300 + k
        ));
        faces.push(format!("#{}", 300 + k));
    }
    body.push_str(&format!(
        "#400=OPEN_SHELL('',({}));\n#401=SHELL_BASED_SURFACE_MODEL('bad',(#400));\n",
        faces.join(",")
    ));
    let text = format!("{HEAD}{body}{TAIL}");
    // Nothing can be cut, but the file is not refused for it: it is reported.
    let err = read(text.as_bytes(), &StepOptions::default()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("no geometry") || msg.contains("no triangles"),
        "{msg}"
    );
}
