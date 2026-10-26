//! Real-world habits of exporters: escapes in names, lower-case entity names, repeated ids.

use super::tests_more::{extr, objs, product, run, slab};
use super::*;
use kadakas_core::command::Editor;

fn name_of(o: &kadakas_core::model::Object) -> String {
    o.data["ifc"]["name"].as_str().unwrap_or("").to_string()
}

fn body() -> String {
    extr(20, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,200.,100.)", 10.0)
}

#[test]
fn escaped_names_are_decoded() {
    // The buildingSMART landscaping scene writes "earth\X\27s" for the apostrophe.
    let data = format!(
        "{}{}",
        body(),
        slab(30, "#21").replace("N30", "earth\\X\\27s \\S\\d \\PB\\\\S\\1 \\X2\\00D8\\X0\\")
    );
    let (ed, _) = run(&data, &IfcImportOptions::default());
    let o = objs(&ed)[0];
    // \S\d in part 1 is U+00E4; after \PB\ the code \S\1 is part 2 U+0105.
    assert_eq!(name_of(o), "earth's \u{E4} \u{105} \u{D8}");
}

#[test]
fn lower_case_names_and_spaces_after_the_equals_sign() {
    let data = format!("{}{}", body(), slab(30, "#21"))
        .replace("#21=IFCEXTRUDEDAREASOLID", "#21= ifcextrudedareasolid")
        .replace("IFCSLAB(", "ifcslab(");
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
}

#[test]
fn repeated_global_ids_and_step_ids_are_counted() {
    let data =
        format!("{}{}{}", body(), slab(30, "#21"), slab(40, "#21")).replace("'g40'", "'g30'");
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 2);
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("1 products repeat the GlobalId")),
        "{:?}",
        r.notes
    );
    // A repeated step id: the first one wins and the report says so.
    let data = format!(
        "{}{}\n#20=IFCCARTESIANPOINT((9.,9.));",
        body(),
        slab(30, "#21")
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1);
    assert!(
        r.notes.iter().any(|n| n.contains("repeat an ID")),
        "{:?}",
        r.notes
    );
}

fn conv(entity: &str) -> String {
    format!(
        "#60=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#61=IFCPROJECTEDCRS('EPSG:3067',$,$,$,$,$,#60);\n{entity}\n{}{}",
        body(),
        slab(30, "#21")
    )
}

#[test]
fn grid_factors_and_linear_placements_are_reported_not_dropped() {
    let (_, r) = run(
        &conv(
            "#62=IFCMAPCONVERSIONSCALED(#10,#61,385000.,6672000.,0.,1.,0.,0.001,1.0002,1.0003,1.);",
        ),
        &IfcImportOptions::default(),
    );
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("grid factors (1.0002, 1.0003, 1)")),
        "{:?}",
        r.notes
    );
    // A linear placement stops the chain at the parent, and the report counts it.
    let data = format!("{}{}", body(), slab(30, "#21")).replace("#7,#32", "#99,#32");
    let data = format!("#99=IFCLINEARPLACEMENT($,$,$);\n{data}");
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1);
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("1 placements (IfcLinearPlacement")),
        "{:?}",
        r.notes
    );
}

#[test]
fn the_spelling_of_the_ifc2x3_geolocation_property_sets_does_not_matter() {
    for (conv, crs) in [
        ("ePSet_MapConversion", "ePSet_ProjectedCRS"),
        ("EPset_MapConversion", "EPset_ProjectedCRS"),
        ("epset_mapconversion", "epset_projectedcrs"),
    ] {
        let data = format!(
            "#60=IFCPROPERTYSINGLEVALUE('Eastings',$,IFCLENGTHMEASURE(385000.),$);\n#61=IFCPROPERTYSINGLEVALUE('Northings',$,IFCLENGTHMEASURE(6672000.),$);\n#62=IFCPROPERTYSET('p1',$,'{conv}',$,(#60,#61));\n#63=IFCPROPERTYSINGLEVALUE('Name',$,IFCLABEL('EPSG:3067'),$);\n#64=IFCPROPERTYSET('p2',$,'{crs}',$,(#63));\n{}{}",
            body(),
            slab(30, "#21")
        );
        let (ed, _) = super::tests_more::run_in(
            Document::with_sequential_ids(),
            "IFC2X3",
            &data,
            &IfcImportOptions::default(),
        );
        let g = ed
            .doc()
            .georef
            .clone()
            .unwrap_or_else(|| panic!("{conv}: no georeference"));
        assert_eq!(
            (g.crs.as_str(), g.origin.x, g.origin.y),
            ("EPSG:3067", 385000.0, 6672000.0),
            "{conv}"
        );
    }
}

/// A file of `slabs` slabs, each with its own profile, plus `filler` points that nothing uses.
fn big_file(slabs: usize, filler: usize) -> Vec<u8> {
    let mut s = String::from(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n\
#1=IFCPROJECT('p',$,'big',$,$,$,$,$,#2);\n#2=IFCUNITASSIGNMENT((#3));\n#3=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n\
#4=IFCDIRECTION((0.,0.,1.));\n#5=IFCAXIS2PLACEMENT3D(#6,$,$);\n#6=IFCCARTESIANPOINT((0.,0.,0.));\n#7=IFCLOCALPLACEMENT($,#5);\n",
    );
    for i in 0..slabs {
        let n = 100 + i * 10;
        s.push_str(&format!(
            "#{a}=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,{w}.,{w}.);\n#{b}=IFCEXTRUDEDAREASOLID(#{a},#5,#4,200.);\n#{c}=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#{b}));\n#{d}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{c}));\n#{e}=IFCSLAB('g{i}',$,'S{i}',$,$,#7,#{d},$,.FLOOR.);\n",
            a = n,
            b = n + 1,
            c = n + 2,
            d = n + 3,
            e = n + 4,
            w = 500 + i % 50,
        ));
    }
    for i in 0..filler {
        s.push_str(&format!(
            "#{}=IFCCARTESIANPOINT(({}.,{}.,0.5));\n",
            10_000_000 + i,
            i,
            i * 2
        ));
    }
    s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    s.into_bytes()
}

#[test]
fn a_big_file_imports_in_time_and_a_file_over_the_cap_is_refused_with_the_option_to_raise_it() {
    let started = std::time::Instant::now();
    let bytes = big_file(4000, 400_000);
    assert!(bytes.len() > 20_000_000, "{}", bytes.len());
    let doc = Document::with_sequential_ids();
    let r = import_bytes(bytes.clone(), &doc, &IfcImportOptions::default()).unwrap();
    assert_eq!(r.report.entities_read, 4000);
    assert_eq!(r.report.total(), 4000);
    assert!(
        started.elapsed().as_secs() < 600,
        "hang guard: {:?}",
        started.elapsed()
    );
    // The cap: a clear message that names the option.
    let mut o = IfcImportOptions::default();
    o.limits.max_bytes = 1 << 20;
    let e = import_bytes(bytes, &doc, &o).unwrap_err();
    assert!(
        e.to_string().contains("larger than the limit of 1 MB")
            && e.to_string().contains("max_file_mb"),
        "{e}"
    );
    // The entity cap: the scan stops and says so.
    let mut o = IfcImportOptions::default();
    o.limits.max_entities = 1000;
    let r = import_bytes(big_file(400, 0), &doc, &o).unwrap();
    assert!(
        r.report
            .notes
            .iter()
            .any(|n| n.contains("more than 1000 entities")),
        "{:?}",
        r.report.notes
    );
}

#[test]
fn a_panic_inside_an_import_becomes_a_refusal_with_its_place() {
    let r: Result<(), IfcImportError> = super::guarded(|| panic!("a bug"));
    let Err(IfcImportError::Internal(detail)) = r else {
        panic!("{r:?}")
    };
    assert!(detail.starts_with("a bug at "), "{detail}");
    assert!(detail.contains("tests_real.rs"), "{detail}");
    assert_eq!(
        IfcImportError::Internal("x".into()).to_string(),
        "the import failed on a damaged file (internal error: x)"
    );
}

/// A file in meters with the site at (650000, 6500000) and one object of every kind that the import makes.
fn far_file() -> String {
    let area = "#100=IFCCARTESIANPOINT((0.,0.));#101=IFCCARTESIANPOINT((100.,0.));#102=IFCCARTESIANPOINT((100.,100.));#103=IFCCARTESIANPOINT((0.,100.));\n\
#104=IFCPOLYLINE((#100,#101,#102,#103,#100));\n#105=IFCCARTESIANPOINT((40.,40.));#106=IFCCARTESIANPOINT((60.,40.));#107=IFCCARTESIANPOINT((60.,60.));#108=IFCCARTESIANPOINT((40.,60.));\n\
#109=IFCPOLYLINE((#105,#106,#107,#108,#105));\n#110=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#104,(#109));\n#111=IFCEXTRUDEDAREASOLID(#110,#5,#4,0.1);\n";
    let tree = extr(120, "IFCCIRCLEPROFILEDEF(.AREA.,$,$,2.)", 6.0);
    let terrain = "#130=IFCCARTESIANPOINTLIST3D(((0.,0.,1.),(50.,0.,2.),(50.,50.,3.),(0.,50.,2.)));\n#131=IFCTRIANGULATEDFACESET(#130,$,.F.,((1,2,3),(1,3,4)),$);\n";
    let slab_item = extr(140, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,10.,10.)", 0.3);
    // A symbol used twice.
    let sym = "#150=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,1.);\n#151=IFCEXTRUDEDAREASOLID(#150,#5,#4,1.);\n#152=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#151));\n#153=IFCREPRESENTATIONMAP(#5,#152);\n\
#154=IFCCARTESIANPOINT((5.,5.,0.));\n#155=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#154,$,$);\n#156=IFCMAPPEDITEM(#153,#155);\n#157=IFCCARTESIANPOINT((9.,9.,0.));\n#158=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#157,$,$);\n#159=IFCMAPPEDITEM(#153,#158);\n";
    let line = "#170=IFCCARTESIANPOINT((0.,0.));#171=IFCCARTESIANPOINT((30.,20.));#172=IFCPOLYLINE((#170,#171));\n#173=IFCSHAPEREPRESENTATION($,'Annotation','Curve2D',(#172));\n#174=IFCPRODUCTDEFINITIONSHAPE($,$,(#173));\n#175=IFCANNOTATION('ga',$,'Line',$,$,#7,#174);\n";
    let point = "#180=IFCCARTESIANPOINT((12.,13.,2.));\n#181=IFCSHAPEREPRESENTATION($,'Annotation','Point',(#180));\n#182=IFCPRODUCTDEFINITIONSHAPE($,$,(#181));\n#183=IFCANNOTATION('gb',$,'Point',$,$,#7,#182);\n";
    let lines_and_points = format!("{line}{point}");
    let products = format!(
        "{}{}{}{}{}{}{}{}{}{}{}",
        area,
        product(
            112,
            "IFCGEOGRAPHICELEMENT",
            "#111",
            "'vegetation',@,$,.USERDEFINED."
        ),
        tree,
        product(
            124,
            "IFCGEOGRAPHICELEMENT",
            "#121",
            "'vegetation',@,$,.USERDEFINED."
        ),
        terrain,
        product(132, "IFCGEOGRAPHICELEMENT", "#131", "$,@,$,.TERRAIN."),
        slab_item,
        slab(144, "#141"),
        sym,
        product(160, "IFCBUILDINGELEMENTPROXY", "#156", "$,@,$,$")
            .replace("'SweptSolid'", "'MappedRepresentation'")
            + &product(164, "IFCBUILDINGELEMENTPROXY", "#159", "$,@,$,$"),
        lines_and_points,
    );
    // The place of every product: the site at (650000, 6500000).
    let products = products.replace("#7,#", "#70,#").replace("(#7,", "(#70,");
    format!("#70=IFCLOCALPLACEMENT($,#71);\n#71=IFCAXIS2PLACEMENT3D(#72,$,$);\n#72=IFCCARTESIANPOINT((650000.,6500000.,0.));\n{products}")
        .replace("#175=IFCANNOTATION('ga',$,'Line',$,$,#7,", "#175=IFCANNOTATION('ga',$,'Line',$,$,#70,")
        .replace("#183=IFCANNOTATION('gb',$,'Point',$,$,#7,", "#183=IFCANNOTATION('gb',$,'Point',$,$,#70,")
}

#[test]
fn every_kind_of_object_far_from_the_origin_is_imported_near_it() {
    let data = far_file();
    let bytes = super::tests_more::file("IFC4", &data);
    let text = String::from_utf8(bytes)
        .unwrap()
        .replace(".LENGTHUNIT.,.MILLI.,.METRE.", ".LENGTHUNIT.,$,.METRE.");
    let doc = Document::with_sequential_ids();
    let r = import_bytes(text.into_bytes(), &doc, &IfcImportOptions::default()).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command)
        .unwrap_or_else(|e| panic!("the batch failed: {e}\n{:?}", r.report));
    let kinds: std::collections::BTreeSet<&str> =
        ed.doc().objects.values().map(|o| o.shape.kind()).collect();
    for k in [
        "landscape_area",
        "plant",
        "site_model",
        "mesh3d",
        "symbol",
        "polyline",
        "locus3d",
    ] {
        assert!(kinds.contains(k), "no {k}: {kinds:?} {:?}", r.report);
    }
    // The shift is in the georeference, and nothing is far from the origin (millimeters).
    let g = ed.doc().georef.as_ref().expect("a georeference");
    assert!(
        (g.origin.x - 650_000.0).abs() < 200.0 && (g.origin.y - 6_500_000.0).abs() < 200.0,
        "{:?}",
        g.origin
    );
    for o in ed.doc().objects.values() {
        let (lo, hi) = o.shape.bounds();
        assert!(
            lo.abs().max_element() < 1.0e6 && hi.abs().max_element() < 1.0e6,
            "{} {lo} {hi}",
            o.shape.kind()
        );
    }
    // The hole of the landscape area is still inside it.
    let holes = ed.doc().objects.values().find_map(|o| match &o.shape {
        Shape::LandscapeArea { .. } => Some(o.id),
        _ => None,
    });
    assert!(holes.is_some());
}

#[test]
fn a_lower_case_header_keeps_its_schema() {
    // The schema decides, for example, which notes about the georeference make sense.
    let bytes = super::tests_more::file("IFC2X3", &format!("{}{}", body(), slab(30, "#21")));
    let text = String::from_utf8(bytes)
        .unwrap()
        .replace("FILE_SCHEMA", "file_schema");
    let r = import_bytes(
        text.into_bytes(),
        &Document::with_sequential_ids(),
        &IfcImportOptions::default(),
    )
    .unwrap();
    assert!(
        r.report
            .notes
            .iter()
            .any(|n| n.contains("IFC2x3 has no IfcMapConversion")),
        "{:?}",
        r.report.notes
    );
}

#[test]
fn a_far_site_placement_deep_in_the_file_is_found() {
    // 400 points with small numbers come before the point of the site placement (the sample of the
    // first points would not see it); the placements of the products are what finds the file.
    let junk: String = (0..400u64)
        .map(|i| format!("#{}=IFCCARTESIANPOINT((1.,2.,3.));\n", 20_000 + i))
        .collect();
    let data = far_file().replace("#72", "#900072");
    let bytes = super::tests_more::file("IFC4", &format!("{junk}{data}"));
    let text = String::from_utf8(bytes)
        .unwrap()
        .replace(".LENGTHUNIT.,.MILLI.,.METRE.", ".LENGTHUNIT.,$,.METRE.");
    let doc = Document::with_sequential_ids();
    let r = import_bytes(text.into_bytes(), &doc, &IfcImportOptions::default()).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command).unwrap();
    let g = ed.doc().georef.as_ref().expect("a georeference");
    assert!(
        g.origin.x > 600_000.0 && g.origin.y > 6_000_000.0,
        "{:?}",
        g.origin
    );
    assert!(ed.doc().objects.len() >= 7);
}

/// Points far from the origin in a surface model of polyloops and in an annotation, with every
/// placement at the origin (only the points say where the file is).
fn far_points_file(x: f64, y: f64, b: u64) -> String {
    let p = |i: u64, dx: f64, dy: f64, z: f64| {
        format!(
            "#{}=IFCCARTESIANPOINT(({}.,{}.,{}.));\n",
            b + i,
            x + dx,
            y + dy,
            z
        )
    };
    let mut s = String::new();
    s.push_str(&p(0, 0.0, 0.0, 1.0));
    s.push_str(&p(1, 2000.0, 0.0, 2.0));
    s.push_str(&p(2, 2000.0, 2000.0, 3.0));
    s.push_str(&p(3, 0.0, 2000.0, 2.0));
    let r = |n: u64| b + n;
    s.push_str(&format!(
        "#{l1}=IFCPOLYLOOP((#{p0},#{p1},#{p2}));\n#{b1}=IFCFACEOUTERBOUND(#{l1},.T.);\n#{f1}=IFCFACE((#{b1}));\n\
#{l2}=IFCPOLYLOOP((#{p0},#{p2},#{p3}));\n#{b2}=IFCFACEOUTERBOUND(#{l2},.T.);\n#{f2}=IFCFACE((#{b2}));\n\
#{sh}=IFCOPENSHELL((#{f1},#{f2}));\n#{sm}=IFCSHELLBASEDSURFACEMODEL((#{sh}));\n",
        p0 = r(0), p1 = r(1), p2 = r(2), p3 = r(3),
        l1 = r(10), b1 = r(11), f1 = r(12), l2 = r(13), b2 = r(14), f2 = r(15), sh = r(16), sm = r(17)
    ));
    // The products: ids of their own, the surface model and the polyline are items.
    let item = r(17);
    s.push_str(&product(
        b + 20,
        "IFCGEOGRAPHICELEMENT",
        &format!("#{item}"),
        "$,@,$,.TERRAIN.",
    ));
    s.push_str(&format!(
        "#{pl}=IFCPOLYLINE((#{p0},#{p2}));\n#{sr}=IFCSHAPEREPRESENTATION($,'Annotation','Curve2D',(#{pl}));\n#{pd}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{sr}));\n#{an}=IFCANNOTATION('ga',$,'Line',$,$,#7,#{pd});\n",
        pl = r(30), sr = r(31), pd = r(32), an = r(33), p0 = r(0), p2 = r(2)
    ));
    s
}

#[test]
fn points_far_from_the_origin_find_the_file_when_every_placement_is_at_the_origin() {
    // Millimeters: (6.5e8, 6.5e9) is 6,500 km from the origin; meters: (650000, 6500000).
    for (x, y, metres) in [(6.5e8, 6.5e9, false), (650_000.0, 6_500_000.0, true)] {
        let bytes = super::tests_more::file("IFC4", &far_points_file(x, y, 300));
        let mut text = String::from_utf8(bytes).unwrap();
        if metres {
            text = text.replace(".LENGTHUNIT.,.MILLI.,.METRE.", ".LENGTHUNIT.,$,.METRE.");
        }
        let doc = Document::with_sequential_ids();
        let r = import_bytes(text.into_bytes(), &doc, &IfcImportOptions::default()).unwrap();
        let mut ed = Editor::new(doc);
        ed.execute(&r.command)
            .unwrap_or_else(|e| panic!("{e} {:?}", r.report));
        let kinds: std::collections::BTreeSet<&str> =
            ed.doc().objects.values().map(|o| o.shape.kind()).collect();
        assert!(
            kinds.contains("site_model") && kinds.contains("polyline3d"),
            "metres={metres}: {kinds:?} {:?}",
            r.report
        );
        let g = ed.doc().georef.as_ref().expect("a georeference");
        assert!(
            (g.origin.y - 6_500_000.0).abs() < 5_000.0,
            "metres={metres}: {:?}",
            g.origin
        );
        for o in ed.doc().objects.values() {
            let (lo, hi) = o.shape.bounds();
            assert!(
                lo.abs().max_element() < 1.0e7 && hi.abs().max_element() < 1.0e7,
                "{} {lo} {hi}",
                o.shape.kind()
            );
        }
    }
}

#[test]
fn a_file_that_the_placements_did_not_show_is_found_by_a_second_pass() {
    // No placement says where the file is (all at the origin) and the points are far, in meters:
    // the objects fail the range test, and the second pass, which only measures, finds the file.
    let s = far_points_file(650_000.0, 6_500_000.0, 5_000_000);
    let bytes = super::tests_more::file("IFC4", &s);
    let text = String::from_utf8(bytes)
        .unwrap()
        .replace(".LENGTHUNIT.,.MILLI.,.METRE.", ".LENGTHUNIT.,$,.METRE.");
    let doc = Document::with_sequential_ids();
    let r = import_bytes(text.into_bytes(), &doc, &IfcImportOptions::default()).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command).unwrap();
    assert!(
        ed.doc().georef.is_some() && !ed.doc().objects.is_empty(),
        "{:?}",
        r.report
    );
    assert!(
        r.report
            .notes
            .iter()
            .any(|n| n.contains("read again with a shift")),
        "{:?}",
        r.report.notes
    );
}

#[test]
fn profiles_with_huge_numbers_are_refused_not_a_panic() {
    // 1E308 in a circle of a void, in a hollow circle and in a hollow rectangle: one product is
    // skipped with a reason, and the other product of the file is imported.
    for bad in [
        "IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,$,$,1.E308,200.)",
        "IFCCIRCLEHOLLOWPROFILEDEF(.AREA.,$,$,5000.,1.E308)",
        "IFCRECTANGLEHOLLOWPROFILEDEF(.AREA.,$,$,1.E308,2000.,200.)",
        "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.E308,1.E308)",
        "IFCCIRCLEPROFILEDEF(.AREA.,$,$,1.E308)",
    ] {
        let data = format!(
            "{}{}{}{}",
            extr(20, bad, 3000.0),
            slab(30, "#21"),
            extr(40, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
            slab(50, "#41")
        );
        let (ed, r) = run(&data, &IfcImportOptions::default());
        assert_eq!(objs(&ed).len(), 1, "{bad}: {r:?}");
        // Skipped with a line: out of range, or no body (a wall thicker than the radius).
        assert!(!r.skipped.is_empty(), "{bad}: {r:?}");
    }
    // A void whose circle is huge, in an arbitrary profile with voids.
    let data = format!(
        "#100=IFCCARTESIANPOINT((0.,0.));#101=IFCCARTESIANPOINT((1000.,0.));#102=IFCCARTESIANPOINT((1000.,1000.));#103=IFCCARTESIANPOINT((0.,1000.));\n\
#104=IFCPOLYLINE((#100,#101,#102,#103,#100));\n#105=IFCAXIS2PLACEMENT2D(#100,$);\n#106=IFCCIRCLE(#105,1.E308);\n\
#110=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#104,(#106));\n#111=IFCEXTRUDEDAREASOLID(#110,#5,#4,100.);\n{}",
        slab(120, "#111")
    );
    let (_, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(r.entities_read, 1);
}

#[test]
fn a_panic_in_one_product_skips_that_product_only() {
    super::PANIC_AT.with(|c| c.set(1));
    let data = format!(
        "{}{}{}{}",
        body(),
        slab(30, "#21"),
        extr(40, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
        slab(50, "#41")
    );
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
    assert_eq!(r.entities_read, 2);
    assert_eq!((r.entities_imported, r.entities_skipped), (1, 1));
    assert!(
        r.skipped
            .iter()
            .any(|s| s.reason == "internal error in this product"),
        "{r:?}"
    );
    assert!(
        r.notes.iter().any(
            |n| n.starts_with("1 products were skipped after an internal error")
                && n.contains("a bug in the geometry of a product at ")
        ),
        "{:?}",
        r.notes
    );
}

fn two_products() -> String {
    format!(
        "{}{}{}{}",
        body(),
        slab(30, "#21"),
        extr(40, "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)", 10.0),
        slab(50, "#41")
    )
}

#[test]
fn the_unguarded_entry_does_not_catch_a_panic_in_a_product() {
    // The tests of damaged files use this entry: they must see a panic in a product.
    super::PANIC_AT.with(|c| c.set(1));
    let bytes = super::tests_more::file("IFC4", &two_products());
    let doc = Document::with_sequential_ids();
    let seen = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = import_bytes_unguarded(bytes.clone(), &doc, &IfcImportOptions::default());
    }));
    super::PANIC_AT.with(|c| c.set(0));
    assert!(seen.is_err(), "the panic was caught inside the import");
    // The guarded entry catches it for the product and says so.
    super::PANIC_AT.with(|c| c.set(1));
    let r = import_bytes(bytes, &doc, &IfcImportOptions::default()).unwrap();
    assert_eq!(r.report.handled_panics.len(), 1);
    assert!(r.report.handled_panics[0].contains("a bug in the geometry of a product"));
}

#[test]
fn a_panic_after_the_objects_of_a_product_were_added_takes_them_back() {
    super::PANIC_AFTER_ADD.with(|c| c.set(true));
    let (ed, r) = run(&two_products(), &IfcImportOptions::default());
    // The first product panicked after its object was added: it is gone, and the counts agree.
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
    assert_eq!(r.total(), 1, "{r:?}");
    assert_eq!(r.entities_read, 2);
    assert_eq!((r.entities_imported, r.entities_skipped), (1, 1));
    assert_eq!(r.entities_read, r.entities_imported + r.entities_skipped);
    assert!(
        r.skipped
            .iter()
            .any(|s| s.reason == "internal error in this product" && s.count == 1),
        "{r:?}"
    );
}

#[test]
fn the_note_of_a_panic_has_no_absolute_path() {
    super::PANIC_AT.with(|c| c.set(1));
    let (_, r) = run(&two_products(), &IfcImportOptions::default());
    let note = r
        .notes
        .iter()
        .find(|n| n.contains("internal error"))
        .expect("a note");
    assert!(!note.contains('/') && !note.contains('\\'), "{note}");
    assert!(note.contains(".rs:"), "{note}");
    assert_eq!(
        super::place_only("boom at /home/x/.cargo/registry/src/y/z.rs:10:5"),
        "boom at z.rs:10:5"
    );
    assert_eq!(super::place_only("boom"), "boom");
}

/// Annotation points: `near` at the origin and `far` at (6.5e9, 6.5e9) mm.
fn annotation_points(near: usize, far: usize) -> String {
    let mut s = String::new();
    for i in 0..(near + far) as u64 {
        let x = if (i as usize) < near { 1000.0 } else { 6.5e9 };
        let b = 9000 + i * 10;
        s.push_str(&format!(
            "#{b}=IFCCARTESIANPOINT(({x}.,{x}.,0.));\n#{r}=IFCSHAPEREPRESENTATION($,'Annotation','Point',(#{b}));\n#{d}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{r}));\n#{a}=IFCANNOTATION('g{i}',$,'P{i}',$,$,#7,#{d});\n",
            r = b + 1,
            d = b + 2,
            a = b + 3
        ));
    }
    s
}

#[test]
fn a_second_pass_that_finds_less_is_not_used() {
    // Three points near the origin and four far away: the middle of the two groups is out of
    // range for both, so a shift to the middle would lose all seven. The first result stays.
    let (ed, r) = run(&annotation_points(3, 4), &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 3, "{r:?}");
    assert!(
        !r.notes
            .iter()
            .any(|n| n.contains("read again with a shift")),
        "{:?}",
        r.notes
    );
    assert_eq!(r.skipped_total(), 4, "{:?}", r.skipped);
}

#[test]
fn a_second_pass_that_finds_more_is_used() {
    let (ed, r) = run(&annotation_points(0, 4), &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 4, "{r:?}");
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("read again with a shift")),
        "{:?}",
        r.notes
    );
}

#[test]
fn projected_coordinates_in_a_profile_with_placements_at_the_origin_are_found() {
    // The coordinates are in the profile (6.5e9 mm), the placements are at the origin: the
    // limit of a profile is the one of the overlay, not 10 km, and the second pass moves the file.
    let profile = "IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#30)";
    let pts = "#31=IFCCARTESIANPOINT((6500000000.,6500000000.));\n#32=IFCCARTESIANPOINT((6500002000.,6500000000.));\n#33=IFCCARTESIANPOINT((6500002000.,6500002000.));\n#34=IFCCARTESIANPOINT((6500000000.,6500002000.));\n#30=IFCPOLYLINE((#31,#32,#33,#34,#31));\n";
    let data = format!("{pts}{}{}", extr(20, profile, 3000.0), slab(40, "#21"));
    let (ed, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(objs(&ed).len(), 1, "{r:?}");
    assert!(ed.doc().georef.is_some(), "{r:?}");
    assert!(
        r.notes
            .iter()
            .any(|n| n.contains("read again with a shift")),
        "{:?}",
        r.notes
    );
}

#[test]
fn faces_with_numbers_beyond_the_overlay_are_counted_not_dropped_silently() {
    // A surface model whose polyloop has a hole with a point at 1E15: the face is left out and the
    // report says so.
    let data = "#110=IFCCARTESIANPOINT((0.,0.,0.));\n#111=IFCCARTESIANPOINT((4000.,0.,0.));\n#112=IFCCARTESIANPOINT((4000.,4000.,0.));\n#113=IFCCARTESIANPOINT((0.,4000.,0.));\n\
#114=IFCCARTESIANPOINT((1.E15,1000.,0.));\n#115=IFCCARTESIANPOINT((2000.,1000.,0.));\n#116=IFCCARTESIANPOINT((2000.,2000.,0.));\n\
#120=IFCPOLYLOOP((#110,#111,#112,#113));\n#121=IFCPOLYLOOP((#114,#115,#116));\n#122=IFCFACEOUTERBOUND(#120,.T.);\n#123=IFCFACEBOUND(#121,.T.);\n#124=IFCFACE((#122,#123));\n\
#125=IFCOPENSHELL((#124));\n#126=IFCSHELLBASEDSURFACEMODEL((#125));\n";
    let data = format!(
        "{data}{}",
        product(30, "IFCBUILDINGELEMENTPROXY", "#126", "$,@,$,$")
    );
    let (_, r) = run(&data, &IfcImportOptions::default());
    let said = r
        .skipped
        .iter()
        .any(|s| s.reason.contains("faces with numbers out of range"))
        || r.notes
            .iter()
            .any(|n| n.contains("faces with numbers out of range"));
    assert!(said, "{:?} {:?}", r.skipped, r.notes);
}

/// A far site (650,000 m, 6,500,000 m) turned by about 0.6435 radians, with one product placed
/// relative to it. `application`: the name in the `IfcApplication`; `sites`: how many sites.
fn turned_site(application: &str, sites: usize) -> String {
    let mut s = String::from(
        "#200=IFCCARTESIANPOINT((650000123.,6500000456.,0.));\n#201=IFCDIRECTION((0.8,0.6,0.));\n#202=IFCAXIS2PLACEMENT3D(#200,$,#201);\n#203=IFCLOCALPLACEMENT($,#202);\n#204=IFCSITE('s',$,'Site',$,$,#203,$,$,.ELEMENT.,$,$,$,$,$);\n#205=IFCLOCALPLACEMENT(#203,#5);\n",
    );
    if sites > 1 {
        s.push_str("#206=IFCSITE('s2',$,'Second',$,$,#203,$,$,.ELEMENT.,$,$,$,$,$);\n");
    }
    s.push_str(&format!(
        "#207=IFCORGANIZATION($,'{application}',$,$,$);\n#208=IFCAPPLICATION(#207,'1','{application}','{application}');\n"
    ));
    s.push_str(&extr(
        20,
        "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,200.,100.)",
        10.0,
    ));
    s.push_str(&product(30, "IFCBUILDINGELEMENTPROXY", "#21", "$,@,$,$").replace("#7,", "#205,"));
    s
}

#[test]
fn only_a_file_of_kadakas_has_its_turned_site_turned_back() {
    let origin_of = |application: &str, sites: usize| {
        let (ed, r) = run(
            &turned_site(application, sites),
            &IfcImportOptions::default(),
        );
        assert_eq!(objs(&ed).len(), 1, "{r:?}");
        let g = ed.doc().georef.clone().expect("a georeference");
        (g.rotation, g.origin, r.notes.clone())
    };
    // Written by Kadakas: the turn is the rotation of the georeference, the origin is in whole meters.
    let (rotation, origin, notes) = origin_of("Kadakas", 1);
    assert!((rotation - 0.6f64.atan2(0.8)).abs() < 1e-9, "{rotation}");
    assert_eq!(
        (origin.x.fract(), origin.y.fract()),
        (0.0, 0.0),
        "{origin:?}"
    );
    assert!(notes.iter().any(|n| n.contains("turned")), "{notes:?}");
    // The same file from another program: the shift is in the georeference, the turn stays in the
    // geometry (the rotation of the georeference is zero).
    let (rotation, _, notes) = origin_of("Revit", 1);
    assert_eq!(rotation, 0.0);
    assert!(!notes.iter().any(|n| n.contains("turned")), "{notes:?}");
    // Several sites: no turn back either, also for Kadakas.
    let (rotation, _, _) = origin_of("Kadakas", 2);
    assert_eq!(rotation, 0.0);
}

/// A surface model of `n` faces, each with a hole that has a point at 1E15: the overlay refuses all.
fn bad_faces_product(base: u64, n: u64, with_solid: bool) -> String {
    let mut s = String::new();
    let mut faces = Vec::new();
    for i in 0..n {
        let b = base + i * 10;
        s.push_str(&format!(
            "#{b}=IFCCARTESIANPOINT((0.,0.,0.));\n#{p1}=IFCCARTESIANPOINT((4000.,0.,0.));\n#{p2}=IFCCARTESIANPOINT((4000.,4000.,0.));\n#{p3}=IFCCARTESIANPOINT((0.,4000.,0.));\n\
#{h1}=IFCCARTESIANPOINT((1.E15,1000.,0.));\n#{h2}=IFCCARTESIANPOINT((2000.,1000.,0.));\n#{h3}=IFCCARTESIANPOINT((2000.,2000.,0.));\n\
#{l1}=IFCPOLYLOOP((#{b},#{p1},#{p2},#{p3}));\n#{l2}=IFCPOLYLOOP((#{h1},#{h2},#{h3}));\n#{o}=IFCFACEOUTERBOUND(#{l1},.T.);\n#{i}=IFCFACEBOUND(#{l2},.T.);\n#{f}=IFCFACE((#{o},#{i}));\n",
            p1 = b + 1, p2 = b + 2, p3 = b + 3, h1 = b + 4, h2 = b + 5, h3 = b + 6,
            l1 = b + 7, l2 = b + 8, o = b + 9, i = b + 9000, f = b + 9001
        ));
        faces.push(format!("#{}", b + 9001));
    }
    let shell = base + 5000;
    s.push_str(&format!(
        "#{shell}=IFCOPENSHELL(({}));\n#{m}=IFCSHELLBASEDSURFACEMODEL((#{shell}));\n",
        faces.join(","),
        m = shell + 1
    ));
    let mut items = format!("#{}", shell + 1);
    if with_solid {
        s.push_str(&extr(
            base + 7000,
            "IFCRECTANGLEPROFILEDEF(.AREA.,$,$,100.,100.)",
            10.0,
        ));
        items.push_str(&format!(",#{}", base + 7001));
    }
    s.push_str(&product(
        base + 6000,
        "IFCBUILDINGELEMENTPROXY",
        &items,
        "$,@,$,$",
    ));
    s
}

#[test]
fn the_same_kind_of_note_is_one_line_whatever_the_counts() {
    // Two products with 1 and 2 faces out of range: one line with the sum, not one line for each
    // number.
    let data = format!(
        "{}{}",
        bad_faces_product(20_000, 1, true),
        bad_faces_product(40_000, 2, true)
    );
    let (_, r) = run(&data, &IfcImportOptions::default());
    let lines: Vec<&String> = r
        .notes
        .iter()
        .filter(|n| n.contains("faces with numbers out of range"))
        .collect();
    assert_eq!(lines.len(), 1, "{:?} {:?}", r.notes, r.skipped);
    assert!(
        lines[0].contains("3 faces") && lines[0].contains("2 products"),
        "{}",
        lines[0]
    );
}

#[test]
fn products_that_lose_all_their_faces_give_one_skipped_line() {
    let data = format!(
        "{}{}",
        bad_faces_product(20_000, 1, false),
        bad_faces_product(40_000, 2, false)
    );
    let (_, r) = run(&data, &IfcImportOptions::default());
    assert_eq!(r.skipped.len(), 1, "{:?}", r.skipped);
    assert_eq!(r.skipped[0].count, 2);
    assert!(
        r.skipped[0]
            .reason
            .starts_with("faces with numbers out of range")
    );
}

#[test]
fn a_note_sentence_splits_into_its_count_and_the_rest() {
    assert_eq!(
        super::split_count("3 faces were left out"),
        (3, "faces were left out")
    );
    assert_eq!(
        super::split_count("faces were left out"),
        (0, "faces were left out")
    );
    assert_eq!(super::split_count("12 "), (12, ""));
    assert_eq!(super::split_count("3x"), (0, "3x"));
}

#[test]
fn big_solids_of_a_file_without_units_are_bounded_before_they_are_made() {
    // A 4 KB file whose project has no unit assignment: 300 and 290 are meters, so the spheres have
    // 256 sides and 65,000 triangles each, and a boolean of two of them is refused before the work.
    let mut s = String::from(
        "#10=IFCCARTESIANPOINT((0.,0.,0.));\n#11=IFCAXIS2PLACEMENT3D(#10,$,$);\n#12=IFCCARTESIANPOINT((100.,0.,0.));\n#13=IFCAXIS2PLACEMENT3D(#12,$,$);\n",
    );
    for i in 0..4u64 {
        let b = 100 + i * 20;
        s.push_str(&format!(
            "#{a}=IFCSPHERE(#11,300.);\n#{c}=IFCSPHERE(#13,290.);\n#{d}=IFCBOOLEANRESULT(.DIFFERENCE.,#{a},#{c});\n#{e}=IFCCSGSOLID(#{d});\n",
            a = b,
            c = b + 1,
            d = b + 2,
            e = b + 3
        ));
        s.push_str(&product(
            b + 4,
            "IFCBUILDINGELEMENTPROXY",
            &format!("#{}", b + 3),
            "$,@,$,$",
        ));
    }
    // The same without the unit: take the unit assignment out of the header of the test file.
    let bytes = super::tests_more::file("IFC4", &s);
    let text = String::from_utf8(bytes)
        .unwrap()
        .replace("$,$,$,$,$,#2);", "$,$,$,$,$,$);");
    let started = std::time::Instant::now();
    let doc = Document::with_sequential_ids();
    let r = import_bytes(text.into_bytes(), &doc, &IfcImportOptions::default()).unwrap();
    assert_eq!(r.report.total(), 4, "{:?}", r.report);
    assert!(
        r.report.notes.iter().any(
            |n| n.contains("a boolean is not applied: the operands have")
                && n.contains("triangles, more than 20000")
        ),
        "{:?}",
        r.report.notes
    );
    assert!(started.elapsed().as_secs() < 600, "hang guard");
    // A solid that would need too many triangles is not made: a sphere of 100 m with a fine
    // tolerance in a file whose unit is the millimeter, with the limit of the import at 1000.
    let opts = IfcImportOptions {
        max_triangles: 1000,
        ..Default::default()
    };
    let text = String::from_utf8(super::tests_more::file(
        "IFC4",
        &format!(
            "#20=IFCSPHERE(#5,100000.);\n#21=IFCCSGSOLID(#20);\n{}",
            product(22, "IFCBUILDINGELEMENTPROXY", "#21", "$,@,$,$")
        ),
    ))
    .unwrap();
    let r = import_bytes(text.into_bytes(), &Document::with_sequential_ids(), &opts).unwrap();
    assert_eq!(r.report.total(), 0);
    assert!(
        r.report
            .skipped
            .iter()
            .any(|s| s.reason.contains("would need about")),
        "{:?}",
        r.report
    );
}
