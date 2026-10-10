//! More import tests, with small inline files: profiles, curves, property kinds, georeference cases and mappings.

use glam::DVec2;
use kadakas_core::command::Editor;
use kadakas_core::model::{Document, Object, Shape};
use kadakas_core::record;
use kadakas_io::Report;

use super::*;

/// A file with millimeters and degrees, a Z direction (#4), a placement at the origin (#5, #7) and the data you give.
fn file(schema: &str, data: &str) -> Vec<u8> {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n\
#1=IFCPROJECT('p',$,'t',$,$,$,$,$,#2);\n#2=IFCUNITASSIGNMENT((#3,#8));\n#3=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n\
#8=IFCCONVERSIONBASEDUNIT(#9,.PLANEANGLEUNIT.,'DEGREE',#10);\n#9=IFCDIMENSIONALEXPONENTS(0,0,0,0,0,0,0);\n\
#10=IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE(0.0174532925199433),#11);\n#11=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);\n\
#4=IFCDIRECTION((0.,0.,1.));\n#5=IFCAXIS2PLACEMENT3D(#6,$,$);\n#6=IFCCARTESIANPOINT((0.,0.,0.));\n#7=IFCLOCALPLACEMENT($,#5);\n\
{data}\nENDSEC;\nEND-ISO-10303-21;\n"
    )
    .into_bytes()
}

/// A product with a body made of the given items. `n` and `n+1`, `n+2` are used. In `tail`, `@` stands for the
/// placement and the representation.
fn product(n: u64, class: &str, items: &str, tail: &str) -> String {
    let tail = tail.replace('@', &format!("#7,#{}", n + 2));
    format!(
        "#{a}=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',({items}));\n#{b}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{a}));\n#{n}={class}('g{n}',$,'N{n}',$,{tail});\n",
        a = n + 1,
        b = n + 2,
    )
}

/// An extruded solid of a profile (`#n` is the profile) with height `h`; the solid is `#n+1`.
fn extr(n: u64, profile: &str, h: f64) -> String {
    format!(
        "#{n}={profile};\n#{m}=IFCEXTRUDEDAREASOLID(#{n},#5,#4,{h});\n",
        m = n + 1
    )
}

fn run(data: &str, opts: &IfcImportOptions) -> (Editor, Report) {
    run_in(Document::with_sequential_ids(), "IFC4", data, opts)
}

fn run_in(doc: Document, schema: &str, data: &str, opts: &IfcImportOptions) -> (Editor, Report) {
    let r = import_bytes(file(schema, data), &doc, opts).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command)
        .unwrap_or_else(|e| panic!("the batch failed: {e}"));
    (ed, r.report)
}

fn objs(ed: &Editor) -> Vec<&Object> {
    ed.doc().objects.values().collect()
}

fn volume_of(o: &Object) -> f64 {
    match &o.shape {
        Shape::Mesh3d(m) => m.volume(),
        other => panic!("not a mesh: {}", other.kind()),
    }
}

fn slab(n: u64, items: &str) -> String {
    product(n, "IFCSLAB", items, "$,@,$,.FLOOR.")
}

fn one_volume(data: &str) -> f64 {
    let (ed, r) = run(data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
    volume_of(objs(&ed)[0])
}

#[test]
fn profile_kinds() {
    // Rounded rectangle: a little less than the full rectangle (2 x 1 m, corner radius 100 mm).
    let v = one_volume(&format!(
        "{}{}",
        extr(
            20,
            "IFCROUNDEDRECTANGLEPROFILEDEF(.AREA.,$,$,2000.,1000.,100.)",
            10.0
        ),
        slab(30, "#21")
    ));
    let exact = (2000.0 * 1000.0 - (4.0 - std::f64::consts::PI) * 100.0 * 100.0) * 10.0;
    assert!((v - exact).abs() < exact * 0.01, "{v} {exact}");
    // Hollow rectangle and hollow circle.
    let v = one_volume(&format!(
        "{}{}",
        extr(
            20,
            "IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,$,1000.,500.,50.,$,$)",
            10.0
        ),
        slab(30, "#21")
    ));
    assert!(
        (v - (1000.0 * 500.0 - 900.0 * 400.0) * 10.0).abs() < 1.0,
        "{v}"
    );
    let v = one_volume(&format!(
        "{}{}",
        extr(20, "IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,$,$,500.,100.)", 10.0),
        slab(30, "#21")
    ));
    let exact = std::f64::consts::PI * (500.0f64.powi(2) - 400.0f64.powi(2)) * 10.0;
    assert!((v - exact).abs() < exact * 0.03, "{v} {exact}");
    // Ellipse, I shape, and a profile that is placed by a 2D axis.
    let v = one_volume(&format!(
        "{}{}",
        extr(20, "IFCELLIPSEPROFILEDEF(.AREA.,$,$,600.,300.)", 10.0),
        slab(30, "#21")
    ));
    assert!((v - std::f64::consts::PI * 600.0 * 300.0 * 10.0).abs() < 0.01 * v);
    let v = one_volume(&format!(
        "{}{}",
        extr(
            20,
            "IFCISHAPEPROFILEDEF(.AREA.,$,$,200.,400.,10.,20.,$)",
            10.0
        ),
        slab(30, "#21")
    ));
    assert!(
        (v - (2.0 * 200.0 * 20.0 + 360.0 * 10.0) * 10.0).abs() < 1.0,
        "{v}"
    );
    let v = one_volume(&format!(
        "#19=IFCCARTESIANPOINT((500.,0.));\n#18=IFCAXIS2PLACEMENT2D(#19,$);\n{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,#18,100.,100.)", 10.0),
        slab(30, "#21")
    ));
    assert!((v - 100.0 * 100.0 * 10.0).abs() < 1.0);
    // A composite profile is the union of its parts.
    let v = one_volume(&format!(
        "#18=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#19=IFCCIRCLEPROFILEDEF(.AREA.,$,$,50.);\n#20=IFCCOMPOSITEPROFILEDEF(.AREA.,$,(#18,#19),$);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,10.);\n{}",
        slab(30, "#21")
    ));
    assert!(v > 100.0 * 100.0 * 10.0 * 1.5);
}

#[test]
fn bad_profiles_and_depths_are_reported() {
    let cases = [
        "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,0.,10.)",
        "IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,$,10.,10.,6.,$,$)",
        "IFCCIRCLEPROFILEDEF(.AREA.,$,$,-1.)",
        "IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,$,$,10.,10.)",
        "IFCELLIPSEPROFILEDEF(.AREA.,$,$,0.,3.)",
        "IFCISHAPEPROFILEDEF(.AREA.,$,$,10.,10.,20.,1.,$)",
        "IFCTSHAPEPROFILEDEF(.AREA.,$,$,10.,10.,1.,1.,$,$,$,$,$)",
        "IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#6)",
        "IFCCOMPOSITEPROFILEDEF(.AREA.,$,(#3),$)",
    ];
    for c in cases {
        let data = format!("{}{}", extr(20, c, 10.0), slab(30, "#21"));
        let (ed, r) = run(&data, &IfcImportOptions::default());
        assert!(objs(&ed).is_empty(), "{c}");
        assert_eq!(r.skipped.len(), 1, "{c}: {r:?}");
    }
    // A solid without depth.
    let data = format!(
        "{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,10.,10.)", 0.0),
        slab(30, "#21")
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert!(objs(&ed).is_empty());
    assert!(r.skipped[0].reason.contains("no depth"), "{r:?}");
}

#[test]
fn curves_with_trimmed_arcs_in_both_directions_and_with_point_trims() {
    // A quarter disc: two lines and an arc given by points, running clockwise.
    let data = "#20=IFCCARTESIANPOINT((0.,0.));\n#21=IFCCARTESIANPOINT((1000.,0.));\n#22=IFCCARTESIANPOINT((0.,1000.));\n\
#23=IFCPOLYLINE((#22,#20,#21));\n#24=IFCAXIS2PLACEMENT2D(#20,$);\n#25=IFCCIRCLE(#24,1000.);\n\
#26=IFCTRIMMEDCURVE(#25,(#21),(#22),.T.,.CARTESIAN.);\n#27=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#23);\n#28=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#26);\n\
#29=IFCCOMPOSITECURVE((#27,#28),.F.);\n#30=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#29);\n#31=IFCEXTRUDEDAREASOLID(#30,#5,#4,10.);\n";
    let v = one_volume(&format!("{data}{}", slab(40, "#31")));
    let exact = std::f64::consts::PI * 1000.0 * 1000.0 / 4.0 * 10.0;
    assert!((v - exact).abs() < exact * 0.01, "{v} {exact}");
    // The same arc with the opposite sense and the segment reversed: the complement of the quarter in the circle is a big arc.
    let reversed = data.replace("(#21),(#22),.T.", "(#22),(#21),.F.").replace(
        "#28=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.",
        "#28=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.F.",
    );
    let (ed, _) = run(
        &format!("{reversed}{}", slab(40, "#31")),
        &IfcImportOptions::default(),
    );
    assert_eq!(objs(&ed).len(), 1);
    // A trimmed line, and an open curve that is not usable as a profile.
    let line = "#20=IFCCARTESIANPOINT((0.,0.));\n#21=IFCCARTESIANPOINT((1000.,0.));\n#22=IFCCARTESIANPOINT((1000.,500.));\n#23=IFCCARTESIANPOINT((0.,500.));\n\
#24=IFCDIRECTION((1.,0.));\n#25=IFCVECTOR(#24,1.);\n#26=IFCLINE(#20,#25);\n#27=IFCTRIMMEDCURVE(#26,(#20),(#21),.T.,.CARTESIAN.);\n\
#28=IFCPOLYLINE((#21,#22,#23,#20));\n#29=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#27);\n#30=IFCCOMPOSITECURVESEGMENT(.CONTINUOUS.,.T.,#28);\n\
#31=IFCCOMPOSITECURVE((#29,#30),.F.);\n#32=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#31);\n#33=IFCEXTRUDEDAREASOLID(#32,#5,#4,10.);\n";
    let v = one_volume(&format!("{line}{}", slab(40, "#33")));
    assert!((v - 1000.0 * 500.0 * 10.0).abs() < 1.0, "{v}");
    // A full circle as a curve, and parameters in degrees.
    let full = "#20=IFCCARTESIANPOINT((0.,0.));\n#21=IFCAXIS2PLACEMENT2D(#20,$);\n#22=IFCCIRCLE(#21,500.);\n\
#23=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#22);\n#24=IFCEXTRUDEDAREASOLID(#23,#5,#4,10.);\n\
#25=IFCTRIMMEDCURVE(#22,(IFCPARAMETERVALUE(90.)),(IFCPARAMETERVALUE(90.)),.F.,.PARAMETER.);\n#26=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#25);\n#27=IFCEXTRUDEDAREASOLID(#26,#5,#4,10.);\n";
    let v = one_volume(&format!("{full}{}", slab(40, "#24")));
    assert!(
        (v - std::f64::consts::PI * 500.0 * 500.0 * 10.0).abs() < 0.02 * v,
        "{v}"
    );
    let v = one_volume(&format!("{full}{}", slab(40, "#27")));
    assert!(
        (v - std::f64::consts::PI * 500.0 * 500.0 * 10.0).abs() < 0.02 * v,
        "{v}"
    );
    // An indexed poly curve without segments, and one with a bad index.
    let ip = "#20=IFCCARTESIANPOINTLIST2D(((0.,0.),(100.,0.),(100.,100.),(0.,100.)));\n#21=IFCINDEXEDPOLYCURVE(#20,$,.F.);\n#22=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#21);\n#23=IFCEXTRUDEDAREASOLID(#22,#5,#4,10.);\n\
#24=IFCINDEXEDPOLYCURVE(#20,(IFCLINEINDEX((1,9))),.F.);\n#25=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#24);\n#26=IFCEXTRUDEDAREASOLID(#25,#5,#4,10.);\n";
    assert!((one_volume(&format!("{ip}{}", slab(40, "#23"))) - 100000.0).abs() < 1.0);
    let (ed, r) = run(
        &format!("{ip}{}", slab(40, "#26")),
        &IfcImportOptions::default(),
    );
    assert!(objs(&ed).is_empty(), "{r:?}");
}

#[test]
fn skewed_extrusions_and_transforms() {
    // The solid leans: x moves with height. The volume of a sheared prism is the same as the straight one.
    let data = "#20=IFCDIRECTION((1.,0.,1.));\n#21=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#22=IFCEXTRUDEDAREASOLID(#21,#5,#20,100.);\n";
    let (ed, _) = run(
        &format!("{data}{}", slab(40, "#22")),
        &IfcImportOptions::default(),
    );
    let o = objs(&ed)[0];
    assert!(
        (volume_of(o) - 100.0 * 100.0 * 70.710678).abs() < 1.0 * 100.0,
        "{}",
        volume_of(o)
    );
    // A mirrored placement keeps the solid outward.
    let m = "#20=IFCDIRECTION((-1.,0.,0.));\n#21=IFCAXIS2PLACEMENT3D(#6,#4,#20);\n#22=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,50.);\n#23=IFCEXTRUDEDAREASOLID(#22,#21,#4,10.);\n";
    assert!((one_volume(&format!("{m}{}", slab(40, "#23"))) - 50000.0).abs() < 1.0);
}

#[test]
fn property_kinds_become_typed_record_fields() {
    let data = format!(
        "{}{}\
#50=IFCPROPERTYENUMERATEDVALUE('Surface',$,(IFCLABEL('Rough'),IFCLABEL('Wet')),$);\n\
#51=IFCPROPERTYLISTVALUE('Sizes',$,(IFCLENGTHMEASURE(1.),IFCLENGTHMEASURE(2.)),$);\n\
#52=IFCPROPERTYBOUNDEDVALUE('Range',$,IFCLENGTHMEASURE(9.),IFCLENGTHMEASURE(3.),$,$);\n\
#53=IFCPROPERTYSINGLEVALUE('Inner',$,IFCAREAMEASURE(4.),$);\n\
#54=IFCCOMPLEXPROPERTY('Group',$,'Use',(#53));\n\
#55=IFCPROPERTYSINGLEVALUE('Count',$,IFCINTEGER(7),$);\n\
#56=IFCPROPERTYSINGLEVALUE('Angle',$,IFCPLANEANGLEMEASURE(90.),$);\n\
#57=IFCPROPERTYSINGLEVALUE('Volume',$,IFCVOLUMEMEASURE(2.5),$);\n\
#58=IFCPROPERTYSINGLEVALUE('Plain',$,5,$);\n\
#59=IFCPROPERTYSINGLEVALUE('Real',$,1.5,$);\n\
#60=IFCPROPERTYSINGLEVALUE('Name',$,'text',$);\n\
#61=IFCPROPERTYSINGLEVALUE('Flag',$,.T.,$);\n\
#62=IFCPROPERTYSINGLEVALUE('Kind',$,IFCLABEL('x'),$);\n\
#63=IFCPROPERTYSINGLEVALUE('Unknown',$,IFCLOGICAL(.U.),$);\n\
#64=IFCPROPERTYSINGLEVALUE('Big',$,IFCREAL(1.E30),$);\n\
#65=IFCPROPERTYSINGLEVALUE('Thermal',$,IFCTHERMALTRANSMITTANCEMEASURE(0.5),$);\n\
#66=IFCPROPERTYSINGLEVALUE('',$,IFCLABEL('nameless'),$);\n\
#67=IFCPROPERTYSINGLEVALUE('_record',$,IFCLABEL('reserved'),$);\n\
#68=IFCPROPERTYSINGLEVALUE('Word',$,IFCTEXT('long'),$);\n\
#69=IFCPROPERTYSINGLEVALUE('Enum',$,.UP.,$);\n\
#70=IFCPROPERTYSET('s1',$,'ePset_Mixed',$,(#50,#51,#52,#54,#55,#56,#57,#58,#59,#60,#61,#62,#63,#64,#65,#66,#67,#68,#69));\n\
#71=IFCQUANTITYLENGTH('L',$,$,2.,$);\n#72=IFCQUANTITYAREA('A',$,$,3.,$);\n#73=IFCQUANTITYVOLUME('V',$,$,4.,$);\n#74=IFCQUANTITYCOUNT('C',$,$,5.,$);\n#75=IFCQUANTITYWEIGHT('W',$,$,6.,$);\n#76=IFCQUANTITYTIME('T',$,$,7.,$);\n#77=IFCQUANTITYNUMBER('X',$,$,1.,$);\n\
#78=IFCELEMENTQUANTITY('s2',$,'Qto_All',$,$,(#71,#72,#73,#74,#75,#76,#77));\n\
#79=IFCRELDEFINESBYPROPERTIES('r1',$,$,$,(#30),#70);\n#80=IFCRELDEFINESBYPROPERTIES('r2',$,$,$,(#30),(#78));\n\
#81=IFCPROPERTYSET('s3',$,'TypeSet',$,(#60));\n#82=IFCWALLTYPE('t',$,'T',$,$,(#81),$,$,$,.STANDARD.);\n#83=IFCRELDEFINESBYTYPE('r3',$,$,$,(#30),#82);\n#84=IFCPROPERTYSET('s4',$,'',$,(#60));\n#85=IFCPROPERTYSET('s5',$,'Empty',$,(#66));\n#86=IFCRELDEFINESBYPROPERTIES('r4',$,$,$,(#30),#84);\n#87=IFCRELDEFINESBYPROPERTIES('r5',$,$,$,(#30),#85);\n",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
        slab(30, "#21"),
    );
    let (ed, _) = run(&data, &IfcImportOptions::default());
    let o = objs(&ed)[0];
    let mixed = record::values(o, "Mixed").expect("Mixed");
    assert_eq!(mixed["Surface"], "Rough, Wet");
    assert_eq!(mixed["Sizes"], "1.0, 2.0");
    assert_eq!(mixed["Range"], "3.0 .. 9.0");
    assert_eq!(mixed["Group.Inner"], 4.0);
    assert_eq!(mixed["Count"], 7);
    assert!((mixed["Angle"].as_f64().unwrap() - 90.0).abs() < 1e-9);
    assert_eq!(mixed["Volume"], 2.5);
    assert_eq!(mixed["Plain"], 5);
    assert_eq!(mixed["Real"], 1.5);
    assert_eq!(mixed["Name"], "text");
    assert_eq!(mixed["Flag"], true);
    assert_eq!(mixed["Kind"], "x");
    assert_eq!(mixed["Thermal"], 0.5);
    assert_eq!(mixed["Word"], "long");
    assert_eq!(mixed["Enum"], "UP");
    for missing in ["Unknown", "Big"] {
        assert!(mixed.get(missing).is_none(), "{missing}");
    }
    let q = record::values(o, "Qto_All").expect("Qto_All");
    assert_eq!(q["L"], 2.0);
    assert_eq!(q["A"], 3.0);
    assert_eq!(q["V"], 4.0);
    assert_eq!(q["C"], 5.0);
    assert_eq!(q["W"], 6.0);
    assert!(q.get("X").is_none());
    // Sets of the type apply to the occurrence.
    assert_eq!(record::values(o, "TypeSet").unwrap()["Name"], "text");
    // A set without a name is dropped.
    assert!(record::values(o, "Empty").is_none());
}

#[test]
fn property_types_that_disagree_become_text() {
    let items = extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0);
    let data = format!(
        "{items}{}{}\
#50=IFCPROPERTYSINGLEVALUE('Value',$,IFCLABEL('abc'),$);\n#51=IFCPROPERTYSINGLEVALUE('Value',$,IFCREAL(2.5),$);\n\
#52=IFCPROPERTYSET('a',$,'Shared',$,(#50));\n#53=IFCPROPERTYSET('b',$,'Shared',$,(#51));\n\
#54=IFCRELDEFINESBYPROPERTIES('r1',$,$,$,(#30),#52);\n#55=IFCRELDEFINESBYPROPERTIES('r2',$,$,$,(#40),#53);\n",
        slab(30, "#21"),
        slab(40, "#21"),
    );
    let (ed, _) = run(&data, &IfcImportOptions::default());
    let format = ed
        .doc()
        .resources
        .iter()
        .find(|r| r.name == "Shared")
        .unwrap();
    let ResourceBody::RecordFormat(f) = &format.body else {
        panic!()
    };
    assert_eq!(f.fields.len(), 1);
    assert_eq!(f.fields[0].ty, record::FieldType::Text);
    let values: Vec<String> = objs(&ed)
        .iter()
        .map(|o| {
            record::values(o, "Shared").unwrap()["Value"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(
        values.contains(&"abc".to_string()) && values.contains(&"2.5".to_string()),
        "{values:?}"
    );
}

#[test]
fn many_sets_and_fields_are_limited() {
    let items = extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0);
    let mut data = format!("{items}{}{}", slab(30, "#21"), slab(40, "#21"));
    let mut props = Vec::new();
    for i in 0..130 {
        data += &format!(
            "#{}=IFCPROPERTYSINGLEVALUE('P{i}',$,IFCLABEL('v'),$);\n",
            1000 + i
        );
        props.push(format!("#{}", 1000 + i));
    }
    data += &format!(
        "#2000=IFCPROPERTYSET('s',$,'Wide',$,({}));\n#2001=IFCRELDEFINESBYPROPERTIES('r',$,$,$,(#30),#2000);\n",
        props.join(",")
    );
    // 310 sets with one property each.
    for i in 0..310 {
        // An element reads at most 200 sets, so the sets go to two elements.
        let target = if i < 150 { 30 } else { 40 };
        data += &format!(
            "#{}=IFCPROPERTYSET('s{i}',$,'Set{i}',$,(#1000));\n#{}=IFCRELDEFINESBYPROPERTIES('r{i}',$,$,$,(#{target}),#{});\n",
            3000 + 2 * i,
            3001 + 2 * i,
            3000 + 2 * i
        );
    }
    let (ed, r) = run(&data, &IfcImportOptions::default());
    let wide = ed
        .doc()
        .resources
        .iter()
        .find(|x| x.name == "Wide")
        .unwrap();
    let ResourceBody::RecordFormat(f) = &wide.body else {
        panic!()
    };
    assert_eq!(f.fields.len(), 120);
    assert!(
        r.notes.iter().any(|n| n.contains("limit is 120")),
        "{:?}",
        r.notes
    );
    // Sets that only one object has go into the object data once there are 40 record formats.
    assert!(
        r.notes.iter().any(|n| n.contains("went into the data")),
        "{:?}",
        r.notes
    );
    assert_eq!(ed.doc().resources.iter().count(), 40);
    let in_data = objs(&ed)
        .iter()
        .filter(|o| o.data.contains_key("ifc_psets"))
        .count();
    assert_eq!(in_data, 2);
}

#[test]
fn presentation_layers_can_be_off_and_colors_have_transparency() {
    let data = format!(
        "{}{}\
#50=IFCCOLOURRGB($,1.,0.5,0.);\n#51=IFCSURFACESTYLERENDERING(#50,0.25,$,$,$,$,$,$,.NOTDEFINED.);\n#52=IFCSURFACESTYLE('S',.BOTH.,(#51));\n#53=IFCSTYLEDITEM(#21,(#52),$);\n\
#54=IFCPRESENTATIONLAYERWITHSTYLE('Hidden',$,(#31),$,.F.,.F.,.F.,());\n#55=IFCPRESENTATIONLAYERWITHSTYLE('  ',$,(#31),$,.T.,.F.,.F.,());\n",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
        slab(30, "#21"),
    );
    let (ed, _) = run(&data, &IfcImportOptions::default());
    let o = objs(&ed)[0];
    match &o.shape {
        Shape::Mesh3d(m) => assert_eq!(
            m.color,
            Some(kadakas_core::model::Color([255, 128, 0, 191]))
        ),
        _ => panic!(),
    }
    let c = ed
        .doc()
        .classes
        .iter()
        .find(|c| c.name == "Hidden")
        .unwrap();
    assert!(!c.visible);
    assert_eq!(o.class, c.id);
}

#[test]
fn skipped_classes_and_helper_elements() {
    let items = extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0);
    let data = format!(
        "{items}{}{}{}{}{}",
        product(30, "IFCOPENINGELEMENT", "#21", "$,@,.OPENING."),
        product(40, "IFCSPACE", "#21", "$,@,$,.ELEMENT.,.INTERNAL."),
        product(50, "IFCGRID", "#21", "$,@"),
        product(60, "IFCVIRTUALELEMENT", "#21", "$,@,$"),
        product(70, "IFCBEAM", "#21", "$,@,$,.BEAM."),
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1);
    let reasons: Vec<&str> = r.skipped.iter().map(|s| s.entity.as_str()).collect();
    for c in [
        "IfcOpeningElement",
        "IfcSpace",
        "IfcGrid",
        "IfcVirtualElement",
    ] {
        assert!(reasons.contains(&c), "{c}: {reasons:?}");
    }
}

#[test]
fn footprint_axis_and_box_representations() {
    // A product with only an axis representation has no body.
    let data = format!(
        "{}#40=IFCSHAPEREPRESENTATION($,'Axis','Curve3D',(#20));\n#41=IFCPRODUCTDEFINITIONSHAPE($,$,(#40));\n#42=IFCBEAM('b',$,'Axis only',$,$,#7,#41,$,.BEAM.);\n#20=IFCPOLYLINE((#6,#6));\n",
        extr(21, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert!(objs(&ed).is_empty());
    assert!(
        r.skipped
            .iter()
            .any(|s| s.entity == "IfcBeam" && s.reason.contains("no body")),
        "{r:?}"
    );
}

#[test]
fn site_terrain_from_an_ifcsite_and_irregular_networks() {
    let data = "#20=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1000.,0.,100.),(0.,1000.,200.)));\n#21=IFCTRIANGULATEDIRREGULARNETWORK(#20,$,.F.,((1,2,3)),$,(0));\n\
#22=IFCSHAPEREPRESENTATION($,'Body','Tessellation',(#21));\n#23=IFCPRODUCTDEFINITIONSHAPE($,$,(#22));\n#24=IFCSITE('s',$,'Hill',$,$,#7,#23,$,.ELEMENT.,$,$,$,$,$);\n";
    let (ed, r) = run(data, &IfcImportOptions::default());
    assert_eq!(objs(&ed)[0].shape.kind(), "site_model", "{r:?}");
    // A geographic element named TIN is terrain too.
    let data = "#20=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(1000.,0.,100.),(0.,1000.,200.)));\n#21=IFCTRIANGULATEDFACESET(#20,$,.F.,((1,2,3)),$);\n\
#22=IFCSHAPEREPRESENTATION($,'Body','Tessellation',(#21));\n#23=IFCPRODUCTDEFINITIONSHAPE($,$,(#22));\n#24=IFCGEOGRAPHICELEMENT('s',$,'TIN surface',$,'TIN',#7,#23,$,.USERDEFINED.);\n";
    let (ed, _) = run(data, &IfcImportOptions::default());
    assert_eq!(objs(&ed)[0].shape.kind(), "site_model");
    // Terrain on a storey above ground: the heights are relative to the layer.
    let data = format!(
        "#40=IFCBUILDINGSTOREY('st',$,'Roof',$,$,#7,$,$,.ELEMENT.,5000.);\n#41=IFCRELCONTAINEDINSPATIALSTRUCTURE('c',$,$,$,(#24),#40);\n{data}"
    );
    let (ed, _) = run(&data, &IfcImportOptions::default());
    let Shape::SiteModel { points, .. } = &objs(&ed)[0].shape else {
        panic!()
    };
    assert_eq!(points.iter().map(|p| p.z).fold(f64::MAX, f64::min), -5000.0);
}

#[test]
fn vegetation_variants() {
    // A planting area with a hole outside of it: the hole is left out, and the note says so.
    let data = "#20=IFCCARTESIANPOINT((0.,0.));\n#21=IFCCARTESIANPOINT((4000.,0.));\n#22=IFCCARTESIANPOINT((4000.,4000.));\n#23=IFCCARTESIANPOINT((0.,4000.));\n#24=IFCPOLYLINE((#20,#21,#22,#23,#20));\n\
#25=IFCCARTESIANPOINT((1000.,1000.));\n#26=IFCCARTESIANPOINT((2000.,1000.));\n#27=IFCCARTESIANPOINT((2000.,2000.));\n#28=IFCCARTESIANPOINT((1000.,2000.));\n#29=IFCPOLYLINE((#25,#26,#27,#28,#25));\n\
#30=IFCCARTESIANPOINT((9000.,9000.));\n#31=IFCCARTESIANPOINT((9500.,9000.));\n#32=IFCCARTESIANPOINT((9500.,9500.));\n#33=IFCPOLYLINE((#30,#31,#32,#30));\n\
#34=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#24,(#29,#33));\n#35=IFCEXTRUDEDAREASOLID(#34,#5,#4,100.);\n";
    let veg = |n: u64, solid: &str| {
        product(
            n,
            "IFCGEOGRAPHICELEMENT",
            solid,
            "'VEGETATION',@,$,.USERDEFINED.",
        )
    };
    let (ed, r) = run(
        &format!("{data}{}", veg(40, "#35")),
        &IfcImportOptions::default(),
    );
    let o = objs(&ed)[0];
    assert_eq!(o.shape.kind(), "landscape_area", "{r:?}");
    assert_eq!(kadakas_core::landscape::data(o).unwrap().holes.len(), 1);
    assert!(r.notes.iter().any(|n| n.contains("holes")), "{:?}", r.notes);
    // A thick solid is a plant when its shape is a plant's shape, else a mesh.
    let tall = "#36=IFCEXTRUDEDAREASOLID(#34,#5,#4,3000.);\n";
    let (ed, _) = run(
        &format!("{data}{tall}{}", veg(40, "#36")),
        &IfcImportOptions::default(),
    );
    assert_eq!(objs(&ed)[0].shape.kind(), "plant");
    let flat = "#36=IFCEXTRUDEDAREASOLID(#34,#5,#4,550.);\n";
    let (ed, _) = run(
        &format!("{data}{flat}{}", veg(40, "#36")),
        &IfcImportOptions::default(),
    );
    assert_eq!(objs(&ed)[0].shape.kind(), "mesh3d");
    // A slab-like area that is far below the layer reports the height it drops.
    let low = "#36=IFCAXIS2PLACEMENT3D(#37,$,$);\n#37=IFCCARTESIANPOINT((0.,0.,-500.));\n#38=IFCEXTRUDEDAREASOLID(#34,#36,#4,100.);\n";
    let (ed, r) = run(
        &format!("{data}{low}{}", veg(40, "#38")),
        &IfcImportOptions::default(),
    );
    assert_eq!(objs(&ed)[0].shape.kind(), "landscape_area");
    assert!(
        r.notes.iter().any(|n| n.contains("own height")),
        "{:?}",
        r.notes
    );
    // Mapped vegetation (a mapped item that is not a symbol) with two parts is a plant when it has a size.
    let rect = "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1000.,1000.);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,1500.);\n";
    let (ed, _) = run(
        &format!("{rect}{}", veg(40, "#21")),
        &IfcImportOptions::default(),
    );
    let Shape::Plant { spread, height, .. } = &objs(&ed)[0].shape else {
        panic!()
    };
    assert_eq!((*spread, *height), (1000.0, 1500.0));
}

#[test]
fn symbols_with_rotation_height_and_fallbacks() {
    let source = "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,200.,100.);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,500.);\n#22=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#21));\n#23=IFCREPRESENTATIONMAP(#5,#22);\n";
    let instance = |n: u64, op: &str| {
        let (a, o) = (n + 10, n + 11);
        format!(
            "{op}\n#{a}=IFCMAPPEDITEM(#23,#{o});\n{}",
            product(n, "IFCFURNISHINGELEMENT", &format!("#{a}"), "$,@,$"),
        )
    };
    let upright = |n: u64, x: f64, z: f64, dir: &str| {
        instance(
            n,
            &format!(
                "#{p}=IFCCARTESIANPOINT(({x},0.,{z}));\n#{d}=IFCDIRECTION({dir});\n#{o}=IFCCARTESIANTRANSFORMATIONOPERATOR3D(#{d},$,#{p},1.,$);",
                p = n + 12,
                d = n + 13,
                o = n + 11,
            ),
        )
    };
    // Two chairs turned by 90 degrees, one lifted by 300 mm: two symbols (one per height).
    let data = format!(
        "{source}{}{}{}",
        upright(30, 1000., 0., "(0.,1.,0.)"),
        upright(50, 3000., 0., "(1.,0.,0.)"),
        upright(70, 5000., 300., "(1.,0.,0.)"),
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(r.objects["symbol"], 3, "{r:?}");
    let defs = ed
        .doc()
        .resources
        .iter()
        .filter(|r| matches!(r.body, ResourceBody::Graphic(_)))
        .count();
    assert_eq!(defs, 2);
    let mut rot: Vec<f64> = objs(&ed)
        .iter()
        .filter_map(|o| match o.shape {
            Shape::Symbol { rotation, .. } => Some((rotation * 100.0).round() / 100.0),
            _ => None,
        })
        .collect();
    rot.sort_by(f64::total_cmp);
    assert_eq!(rot, vec![0.0, 0.0, 1.57]);
    // A scaled copy cannot be a symbol: it becomes a mesh, the others stay symbols.
    let scaled = instance(
        90,
        "#102=IFCCARTESIANPOINT((7000.,0.,0.));\n#101=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#102,2.,$);",
    );
    let (_, r) = run(&format!("{data}{scaled}"), &IfcImportOptions::default());
    assert_eq!((r.objects["symbol"], r.objects["mesh3d"]), (3, 1), "{r:?}");
    // A symbol far away is out of range and skipped, with a reason.
    let far = upright(110, 2.0e11, 0., "(1.,0.,0.)");
    let (_, r) = run(&format!("{data}{far}"), &IfcImportOptions::default());
    assert_eq!(r.objects["symbol"], 3);
    // A source that is used once is not a symbol.
    let (_, r) = run(
        &format!("{source}{}", upright(30, 1000., 0., "(1.,0.,0.)")),
        &IfcImportOptions::default(),
    );
    assert_eq!(r.objects["mesh3d"], 1);
    // A source with a hole or a tilted solid is not a symbol either.
    let holed = data.replace(
        "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,200.,100.);",
        "#20=IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,$,200.,100.,10.,$,$);",
    );
    let (_, r) = run(&holed, &IfcImportOptions::default());
    assert_eq!(r.objects.get("symbol"), None, "{r:?}");
    assert_eq!(r.objects["mesh3d"], 3);
}

#[test]
fn georeference_cases() {
    let geo = |crs: &str, scale: &str, height: &str, rot: &str| {
        format!(
            "#20=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#21=IFCPROJECTEDCRS('{crs}',$,$,$,$,$,#20);\n#22=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-5,#5,$);\n#23=IFCMAPCONVERSION(#22,#21,1000.,2000.,{height},{rot},{scale});\n"
        )
    };
    let body = format!(
        "{}{}",
        extr(40, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
        slab(50, "#41")
    );
    let (ed, r) = run(
        &format!("{}{body}", geo("EPSG:3067", "0.001", "0.", "1.,0.")),
        &IfcImportOptions::default(),
    );
    let g = ed.doc().georef.clone().unwrap();
    assert_eq!(
        (g.crs.as_str(), g.origin),
        ("EPSG:3067", DVec2::new(1000.0, 2000.0))
    );
    assert!(r.notes.is_empty(), "{:?}", r.notes);
    // A scale and a height that are not applied, a CRS that nobody knows, and a rotated frame.
    let (ed, r) = run(
        &format!("{}{body}", geo("Local grid", "0.5", "12.", "0.,1.")),
        &IfcImportOptions::default(),
    );
    let g = ed.doc().georef.clone().unwrap();
    assert_eq!(g.crs, "");
    assert!((g.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    for what in ["scale of", "orthogonal height", "not known"] {
        assert!(
            r.notes.iter().any(|n| n.contains(what)),
            "{what}: {:?}",
            r.notes
        );
    }
    // The file's frame turned against a document that is turned too.
    let mut doc = Document::with_sequential_ids();
    doc.georef = Some(kadakas_core::georef::Georeference {
        crs: "EPSG:3067".into(),
        origin: DVec2::new(1000.0, 2000.0),
        rotation: 0.3,
        user_origin: DVec2::ZERO,
    });
    let (ed, _) = run_in(
        doc,
        "IFC4",
        &format!("{}{body}", geo("EPSG:3067", "0.001", "0.", "1.,0.")),
        &IfcImportOptions::default(),
    );
    let o = objs(&ed)[0];
    let Shape::Mesh3d(m) = &o.shape else { panic!() };
    // The file frame is turned by -0.3 rad against the document: a 100 mm square becomes a rotated one.
    let (lo, hi) = m.bounds3();
    assert!(hi.x - lo.x > 120.0, "{lo} {hi}");
    // A document with a georeference without CRS keeps local coordinates.
    let mut doc = Document::with_sequential_ids();
    doc.georef = Some(kadakas_core::georef::Georeference {
        crs: String::new(),
        origin: DVec2::new(5.0, 5.0),
        rotation: 0.0,
        user_origin: DVec2::ZERO,
    });
    let (_, r) = run_in(
        doc,
        "IFC4",
        &format!("{}{body}", geo("EPSG:3067", "0.001", "0.", "1.,0.")),
        &IfcImportOptions::default(),
    );
    assert!(
        r.notes.iter().any(|n| n.contains("differs")),
        "{:?}",
        r.notes
    );
    // IFC2x3 has no map conversion.
    let (_, r) = run_in(
        Document::with_sequential_ids(),
        "IFC2X3",
        &body,
        &IfcImportOptions::default(),
    );
    assert!(
        r.notes.iter().any(|n| n.contains("IFC2x3")),
        "{:?}",
        r.notes
    );
    // A map conversion with absurd numbers is ignored.
    let (ed, _) = run(
        &format!(
            "{}{body}",
            geo("EPSG:3067", "0.001", "0.", "1.,0.").replace("1000.,2000.", "1.E30,2.")
        ),
        &IfcImportOptions::default(),
    );
    assert!(ed.doc().georef.is_none());
    // A CRS unit in feet.
    let feet = "#20=IFCCONVERSIONBASEDUNIT($,.LENGTHUNIT.,'FOOT',#24);\n#24=IFCMEASUREWITHUNIT(IFCLENGTHMEASURE(0.3048),#25);\n#25=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#21=IFCPROJECTEDCRS('EPSG:2263',$,$,$,$,$,#20);\n#22=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-5,#5,$);\n#23=IFCMAPCONVERSION(#22,#21,1000.,2000.,0.,1.,0.,0.0003048);\n";
    let (ed, r) = run(&format!("{feet}{body}"), &IfcImportOptions::default());
    assert!(
        (ed.doc().georef.clone().unwrap().origin.x - 304.8).abs() < 1e-9,
        "{:?}",
        r.notes
    );
}

#[test]
fn layer_and_class_reuse_and_unnamed_storeys() {
    let mut ed0 = Editor::new(Document::with_sequential_ids());
    ed0.execute(&kadakas_core::command::Command::AddLayer {
        name: "Ground".into(),
        elevation: 123.0,
        scale: None,
        kind: None,
    })
    .unwrap();
    let doc = ed0.doc().clone();
    let data = format!(
        "#40=IFCBUILDINGSTOREY('a',$,'Ground',$,$,#7,$,$,.ELEMENT.,0.);\n#41=IFCBUILDINGSTOREY('b',$,$,$,$,#7,$,$,.ELEMENT.,1.E30);\n#42=IFCBUILDINGSTOREY('c',$,'Third',$,$,#7,$,$,.ELEMENT.,$);\n\
#43=IFCRELCONTAINEDINSPATIALSTRUCTURE('c1',$,$,$,(#50),#40);\n#44=IFCRELCONTAINEDINSPATIALSTRUCTURE('c2',$,$,$,(#60),#41);\n#45=IFCRELCONTAINEDINSPATIALSTRUCTURE('c3',$,$,$,(#70),#42);\n{}{}{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
        slab(50, "#21"),
        slab(60, "#21"),
        slab(70, "#21"),
    );
    let (ed, r) = run_in(doc, "IFC4", &data, &IfcImportOptions::default());
    let by = |n: &str| ed.doc().layers.iter().find(|l| l.name == n).unwrap();
    // The layer of the same name is used as it is.
    assert_eq!(by("Ground").elevation, 123.0);
    assert!(
        r.layers_created.contains(&"Storey 41".to_string()),
        "{:?}",
        r.layers_created
    );
    assert_eq!(by("Storey 41").elevation, 0.0);
    assert_eq!(by("Third").elevation, 0.0);
}

#[test]
fn annotations_with_3d_curves_and_unsupported_items() {
    let data = "#20=IFCCARTESIANPOINT((0.,0.,0.));\n#21=IFCCARTESIANPOINT((1000.,0.,500.));\n#22=IFCCARTESIANPOINT((1000.,1000.,500.));\n#23=IFCPOLYLINE((#20,#21,#22,#20));\n\
#24=IFCSHAPEREPRESENTATION($,'Annotation','Curve3D',(#23));\n#25=IFCPRODUCTDEFINITIONSHAPE($,$,(#24));\n#26=IFCANNOTATION('a',$,'Slope line',$,$,#7,#25);\n\
#27=IFCTEXTLITERAL('hello',#5,.LEFT.);\n#28=IFCSHAPEREPRESENTATION($,'Annotation','Annotation2D',(#27));\n#29=IFCPRODUCTDEFINITIONSHAPE($,$,(#28));\n#30=IFCANNOTATION('b',$,'Label',$,$,#7,#29);\n";
    let (ed, r) = run(data, &IfcImportOptions::default());
    let o = objs(&ed);
    assert_eq!(o.len(), 1);
    // The curve slopes, so it is a 3D polyline, closed by the repeated first point.
    let Shape::Polyline3d { points, closed } = &o[0].shape else {
        panic!("{}", o[0].shape.kind())
    };
    assert!(*closed && points.len() == 3);
    assert!(
        r.skipped
            .iter()
            .any(|s| s.entity == "IfcAnnotation" && s.reason.contains("unsupported")),
        "{r:?}"
    );
}

#[test]
fn orphans_depth_and_budget() {
    // Shape items nested too deep.
    let mut data = String::from(
        "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,10.,10.);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,10.);\n",
    );
    let mut prev = 21;
    for i in 0..30 {
        let n = 100 + 3 * i;
        data += &format!("#{n}=IFCBOOLEANRESULT(.UNION.,#{prev},#21);\n");
        prev = n;
    }
    data += &slab(300, &format!("#{prev}"));
    let (_, r) = run(&data, &IfcImportOptions::default());
    assert!(
        r.notes.iter().any(|n| n.contains("nest too deep")),
        "{:?}",
        r.notes
    );
    // A triangle budget that is too small drops the geometry.
    let tiny = IfcImportOptions {
        max_triangles: 3,
        ..IfcImportOptions::default()
    };
    let data = format!(
        "{}{}",
        extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,10.,10.)", 10.0),
        slab(30, "#21")
    );
    let (ed, r) = run(&data, &tiny);
    assert!(objs(&ed).is_empty());
    assert!(r.skipped[0].reason.contains("triangle limit"), "{r:?}");
}

// ---------------------------------------------------------------- review fixes

/// Nested mapped items (every source holds two uses of the next one) and nested geometric sets. The work is
/// limited, and the answer comes quickly with a note.
#[test]
fn nested_items_stop_at_the_limits_and_do_not_hang() {
    let mut data = String::from(
        "#20=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#6,1.,$);\n#21=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,10.,10.);\n#22=IFCEXTRUDEDAREASOLID(#21,#5,#4,10.);\n",
    );
    let levels = 22u64;
    for k in 0..levels {
        let (r, m, a, b) = (1000 + 10 * k, 1001 + 10 * k, 1002 + 10 * k, 1003 + 10 * k);
        let items = if k + 1 == levels {
            "#22".to_string()
        } else {
            format!("#{a},#{b}")
        };
        data += &format!(
            "#{r}=IFCSHAPEREPRESENTATION($,'Body','Model',({items}));\n#{m}=IFCREPRESENTATIONMAP(#5,#{r});\n"
        );
        if k + 1 < levels {
            let next = 1001 + 10 * (k + 1);
            data +=
                &format!("#{a}=IFCMAPPEDITEM(#{next},#20);\n#{b}=IFCMAPPEDITEM(#{next},#20);\n");
        }
    }
    data += &slab(30, "#1002");
    // Nested geometric sets: every set holds the next one twice.
    for k in 0..40u64 {
        let next = if k == 39 { 6 } else { 5000 + k + 1 };
        data += &format!("#{}=IFCGEOMETRICSET((#{next},#{next}));\n", 5000 + k);
    }
    data += &product(60, "IFCANNOTATION", "#5000", "$,@");
    assert!(data.len() < 8000, "{}", data.len());
    let started = std::time::Instant::now();
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
    // The slab is cut down, not dropped without a word.
    assert!(
        r.notes
            .iter()
            .chain(r.skipped.iter().map(|s| &s.reason))
            .any(|n| n.contains("limit") || n.contains("nest too deep") || n.contains("too many")),
        "{r:?}"
    );
    let _ = ed;
}

#[test]
fn a_mapped_source_is_made_once_and_copied() {
    // Three uses of one source: the geometry is the same and the copies are placed by their own targets.
    let source = "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,50.);\n#22=IFCSHAPEREPRESENTATION($,'Body','Model',(#21));\n#23=IFCREPRESENTATIONMAP(#5,#22);\n";
    let mut data = source.to_string();
    for k in 0..3u64 {
        data += &format!(
            "#{p}=IFCCARTESIANPOINT(({x}.,0.,0.));\n#{o}=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#{p},1.,$);\n#{m}=IFCMAPPEDITEM(#23,#{o});\n",
            p = 100 + 3 * k,
            o = 101 + 3 * k,
            m = 102 + 3 * k,
            x = 1000 * (k + 1),
        );
    }
    data += &slab(30, "#102,#105,#108");
    let opts = IfcImportOptions {
        instances: false,
        ..IfcImportOptions::default()
    };
    let (ed, _) = run(&data, &opts);
    let o = objs(&ed)[0];
    assert!((volume_of(o) - 3.0 * 100.0 * 100.0 * 50.0).abs() < 1.0);
    let Shape::Mesh3d(m) = &o.shape else { panic!() };
    let (lo, hi) = m.bounds3();
    assert!(
        (lo.x - 950.0).abs() < 1e-6 && (hi.x - 3050.0).abs() < 1e-6,
        "{lo} {hi}"
    );
    // A source that contains itself is reported.
    let cyc = "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,50.);\n#22=IFCSHAPEREPRESENTATION($,'Body','Model',(#21,#24));\n#23=IFCREPRESENTATIONMAP(#5,#22);\n#24=IFCMAPPEDITEM(#23,#25);\n#25=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#6,1.,$);\n";
    let (_, r) = run(&format!("{cyc}{}", slab(30, "#24")), &opts);
    assert!(
        r.notes.iter().any(|n| n.contains("contains itself")),
        "{r:?}"
    );
}

#[test]
fn the_mapping_origin_moves_instances_like_ifcopenshell() {
    // The source is a 10 mm box around its own origin. The map origin is at x = 100 and the target at x = 1000:
    // IfcOpenShell puts the box at 1100 (the target times the origin).
    let data = "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,10.,10.);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,10.);\n#22=IFCSHAPEREPRESENTATION($,'Body','Model',(#21));\n\
#23=IFCCARTESIANPOINT((100.,0.,0.));\n#24=IFCAXIS2PLACEMENT3D(#23,$,$);\n#25=IFCREPRESENTATIONMAP(#24,#22);\n#26=IFCCARTESIANPOINT((1000.,0.,0.));\n#27=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#26,1.,$);\n#28=IFCMAPPEDITEM(#25,#27);\n";
    let opts = IfcImportOptions {
        instances: false,
        ..IfcImportOptions::default()
    };
    let (ed, _) = run(&format!("{data}{}", slab(30, "#28")), &opts);
    let Shape::Mesh3d(m) = &objs(&ed)[0].shape else {
        panic!()
    };
    let (lo, hi) = m.bounds3();
    assert!(((lo.x + hi.x) / 2.0 - 1100.0).abs() < 1e-6, "{lo} {hi}");
}

fn signed_volume(o: &Object) -> f64 {
    let Shape::Mesh3d(m) = &o.shape else { panic!() };
    m.triangles
        .iter()
        .map(|t| {
            let p = |i: u32| m.vertices[i as usize];
            p(t[0]).dot(p(t[1]).cross(p(t[2])))
        })
        .sum::<f64>()
        / 6.0
}

#[test]
fn mirrored_operators_keep_the_mirror_and_the_solid_outward() {
    let source = "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,300.);\n#22=IFCSHAPEREPRESENTATION($,'Body','Model',(#21));\n#23=IFCREPRESENTATIONMAP(#5,#22);\n";
    // Axis3 points down: the box hangs below the placement.
    let down = "#24=IFCDIRECTION((1.,0.,0.));\n#25=IFCDIRECTION((0.,1.,0.));\n#26=IFCDIRECTION((0.,0.,-1.));\n#27=IFCCARTESIANTRANSFORMATIONOPERATOR3D(#24,#25,#6,1.,#26);\n#28=IFCMAPPEDITEM(#23,#27);\n";
    let opts = IfcImportOptions {
        instances: false,
        ..IfcImportOptions::default()
    };
    let (ed, _) = run(&format!("{source}{down}{}", slab(30, "#28")), &opts);
    let o = objs(&ed)[0];
    let Shape::Mesh3d(m) = &o.shape else { panic!() };
    let (lo, hi) = m.bounds3();
    assert!(
        (lo.z + 300.0).abs() < 1e-6 && hi.z.abs() < 1e-6,
        "{lo} {hi}"
    );
    assert!(
        (signed_volume(o) - 100.0 * 100.0 * 300.0).abs() < 1.0,
        "{}",
        signed_volume(o)
    );
    // Mirrored in x (Axis1 = -X, Axis2 = +Y, Axis3 = +Z is a mirror): the box is at the other side.
    let mirror = "#24=IFCDIRECTION((-1.,0.,0.));\n#25=IFCDIRECTION((0.,1.,0.));\n#26=IFCCARTESIANPOINT((500.,0.,0.));\n#27=IFCCARTESIANTRANSFORMATIONOPERATOR3D(#24,#25,#26,1.,$);\n#28=IFCMAPPEDITEM(#23,#27);\n";
    let (ed, _) = run(&format!("{source}{mirror}{}", slab(30, "#28")), &opts);
    assert!((signed_volume(objs(&ed)[0]) - 100.0 * 100.0 * 300.0).abs() < 1.0);
    // Two mirrored uses are meshes, never symbols (a symbol cannot mirror).
    let two = format!(
        "{source}{mirror}#29=IFCMAPPEDITEM(#23,#27);\n{}{}",
        product(30, "IFCFURNISHINGELEMENT", "#28", "$,@,$"),
        product(40, "IFCFURNISHINGELEMENT", "#29", "$,@,$"),
    );
    let (_, r) = run(&two, &IfcImportOptions::default());
    assert_eq!(
        (r.objects.get("symbol"), r.objects["mesh3d"]),
        (None, 2),
        "{r:?}"
    );
}

#[test]
fn vegetation_is_matched_by_whole_words() {
    let rect = "#20=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1000.,1000.);\n#21=IFCEXTRUDEDAREASOLID(#20,#5,#4,1500.);\n";
    let kind = |object_type: &str, predefined: &str| {
        let data = format!(
            "{rect}{}",
            product(
                40,
                "IFCGEOGRAPHICELEMENT",
                "#21",
                &format!("{object_type},@,$,{predefined}")
            )
        );
        let (ed, _) = run(&data, &IfcImportOptions::default());
        objs(&ed)[0].shape.kind()
    };
    assert_eq!(kind("'Street tree'", ".USERDEFINED."), "plant");
    assert_eq!(kind("'TREES'", ".NOTDEFINED."), "plant");
    assert_eq!(kind("'Hedge-row'", ".USERDEFINED."), "plant");
    // "Street" contains "tree" and "planter" contains "plant", but they are not those words.
    assert_eq!(kind("'Street furniture'", ".USERDEFINED."), "mesh3d");
    assert_eq!(kind("'Planter box'", ".USERDEFINED."), "mesh3d");
    assert_eq!(kind("'Treeline sign'", ".USERDEFINED."), "mesh3d");
    // The predefined type decides when it says something.
    assert_eq!(kind("'Tree'", ".SOIL_BORING_POINT."), "mesh3d");
    assert_eq!(kind("$", ".VEGETATION."), "plant");
    assert_eq!(kind("'Tree'", ".TERRAIN."), "site_model");
}

#[test]
fn points_that_a_pnindex_leaves_out_do_not_count() {
    // Four points, but the only triangle uses PnIndex to pick three of them.
    let data = "#20=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(10.,0.,0.),(0.,10.,0.),(9999.,9999.,9999.)));\n#21=IFCTRIANGULATEDFACESET(#20,$,.F.,((1,2,3)),(1,2,3));\n";
    let (ed, _) = run(
        &format!("{data}{}", slab(30, "#21")),
        &IfcImportOptions::default(),
    );
    let Shape::Mesh3d(m) = &objs(&ed)[0].shape else {
        panic!()
    };
    assert_eq!(m.vertices.len(), 3);
    assert!(m.bounds3().1.x <= 10.0);
}

#[test]
fn looping_placements_are_reported() {
    let data = format!(
        "#20=IFCLOCALPLACEMENT(#21,#5);\n#21=IFCLOCALPLACEMENT(#20,#5);\n{}{}",
        extr(40, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
        product(50, "IFCSLAB", "#41", "$,@,$,.FLOOR.").replace("#7,", "#20,")
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1);
    assert!(r.notes.iter().any(|n| n.contains("loop")), "{:?}", r.notes);
}

#[test]
fn ifc2x3_reads_the_map_conversion_property_sets() {
    let data = format!(
        "#20=IFCPROPERTYSINGLEVALUE('Eastings',$,IFCLENGTHMEASURE(500000.),$);\n#21=IFCPROPERTYSINGLEVALUE('Northings',$,IFCLENGTHMEASURE(6700000.),$);\n#22=IFCPROPERTYSINGLEVALUE('XAxisAbscissa',$,IFCREAL(0.),$);\n#23=IFCPROPERTYSINGLEVALUE('XAxisOrdinate',$,IFCREAL(1.),$);\n#24=IFCPROPERTYSINGLEVALUE('Scale',$,IFCREAL(0.001),$);\n\
#25=IFCPROPERTYSET('a',$,'ePSet_MapConversion',$,(#20,#21,#22,#23,#24));\n#26=IFCPROPERTYSINGLEVALUE('Name',$,IFCLABEL('EPSG:3067'),$);\n#27=IFCPROPERTYSET('b',$,'ePSet_ProjectedCRS',$,(#26));\n{}{}",
        extr(40, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
        slab(50, "#41")
    );
    let (ed, r) = run_in(
        Document::with_sequential_ids(),
        "IFC2X3",
        &data,
        &IfcImportOptions::default(),
    );
    let g = ed.doc().georef.clone().expect("georeference");
    assert_eq!(g.crs, "EPSG:3067");
    assert_eq!(g.origin, DVec2::new(500_000.0, 6_700_000.0));
    assert!((g.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!(!r.notes.iter().any(|n| n.contains("no georeference")));
}

#[test]
fn the_species_comes_back_from_our_own_export() {
    use kadakas_core::command::Command;
    use kadakas_core::plants::style::PlantStyle;
    let mut ed = Editor::with_sequential_ids();
    let style = ed
        .execute(&Command::AddResource {
            name: "Maple".into(),
            body: ResourceBody::plant_style(PlantStyle {
                spread: 5000.0,
                height: 9000.0,
                id_code: "AC".into(),
                ..PlantStyle::default()
            }),
        })
        .unwrap()
        .created[0];
    ed.execute(&Command::AddObject {
        shape: Shape::Plant {
            at: DVec2::new(1000.0, 2000.0),
            species: "Acer campestre".into(),
            spread: 5000.0,
            height: 9000.0,
        },
        layer: None,
        class: None,
        attrs: Default::default(),
    })
    .unwrap();
    let _ = style;
    let (bytes, _) = crate::export_bytes(ed.doc(), &crate::IfcOptions::default()).unwrap();
    let r = import_bytes(
        bytes,
        &Document::with_sequential_ids(),
        &IfcImportOptions::default(),
    )
    .unwrap();
    let mut back = Editor::new(Document::with_sequential_ids());
    back.execute(&r.command).unwrap();
    let Shape::Plant { species, .. } = &objs(&back)[0].shape else {
        panic!("{}", objs(&back)[0].shape.kind())
    };
    assert_eq!(species, "Acer campestre");
}

#[test]
fn imported_meshes_can_be_turned() {
    let (mut ed, _) = run(
        &format!(
            "{}{}",
            extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,200.,100.)", 10.0),
            slab(30, "#21")
        ),
        &IfcImportOptions::default(),
    );
    let id = objs(&ed)[0].id;
    ed.execute(&kadakas_core::command::Command::RotateObjects {
        ids: vec![id],
        center: DVec2::ZERO,
        angle: std::f64::consts::FRAC_PI_2,
    })
    .unwrap();
    let Shape::Mesh3d(m) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    let (lo, hi) = m.bounds3();
    assert!((hi.x - lo.x - 100.0).abs() < 1e-6 && (hi.y - lo.y - 200.0).abs() < 1e-6);
    assert!((m.volume() - 200.0 * 100.0 * 10.0).abs() < 1.0);
}
