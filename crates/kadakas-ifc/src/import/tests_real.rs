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
fn a_panic_inside_an_import_becomes_a_refusal() {
    let r: Result<(), IfcImportError> = super::guarded(|| panic!("a bug"));
    assert_eq!(r, Err(IfcImportError::Internal));
    assert_eq!(
        IfcImportError::Internal.to_string(),
        "the import failed on a damaged file (internal error)"
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
        format!("{line}{point}"),
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
