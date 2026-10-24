//! Hostile files that are small and ask for a lot of time or memory. Each one ends with a
//! refusal that names the limit, quickly. The times are generous bounds for a debug build.

use std::time::{Duration, Instant};

use kadakas_meshio::MeshError;
use kadakas_step::{StepLimits, StepOptions, read};

const HEAD: &str = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n";
const TAIL: &str = "ENDSEC;\nEND-ISO-10303-21;\n";

fn box_solid(w: &mut String, first: usize) -> usize {
    // A box of 10 mm made of six planes with shared corners, from entity number `first`.
    // Returns the number of the solid.
    let mut id = first;
    let mut pt = |w: &mut String, x: f64, y: f64, z: f64| {
        id += 1;
        w.push_str(&format!("#{id}=CARTESIAN_POINT('',({x:?},{y:?},{z:?}));\n"));
        id
    };
    let c: Vec<usize> = (0..8)
        .map(|i| {
            pt(
                w,
                (i & 1) as f64 * 10.0,
                ((i >> 1) & 1) as f64 * 10.0,
                ((i >> 2) & 1) as f64 * 10.0,
            )
        })
        .collect();
    let mut faces = Vec::new();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    for q in quads {
        id += 1;
        let l = id;
        w.push_str(&format!(
            "#{l}=POLY_LOOP('',({}));\n",
            q.iter()
                .map(|i| format!("#{}", c[*i]))
                .collect::<Vec<_>>()
                .join(",")
        ));
        id += 1;
        w.push_str(&format!("#{id}=FACE_OUTER_BOUND('',#{l},.T.);\n"));
        id += 1;
        w.push_str(&format!("#{id}=FACE('',(#{}));\n", id - 1));
        faces.push(id);
    }
    id += 1;
    w.push_str(&format!(
        "#{id}=CLOSED_SHELL('',({}));\n",
        faces
            .iter()
            .map(|f| format!("#{f}"))
            .collect::<Vec<_>>()
            .join(",")
    ));
    id += 1;
    w.push_str(&format!("#{id}=FACETED_BREP('b',#{});\n", id - 1));
    id
}

struct Ids(usize);
impl Ids {
    fn next(&mut self) -> usize {
        self.0 += 1;
        self.0
    }
}

/// A product definition with a name, returns (pd, pds).
fn product(w: &mut String, ids: &mut Ids, name: &str) -> (usize, usize) {
    let (app, pc, prod, pf, dc, pd, pds) = (
        ids.next(),
        ids.next(),
        ids.next(),
        ids.next(),
        ids.next(),
        ids.next(),
        ids.next(),
    );
    w.push_str(&format!(
        "#{app}=APPLICATION_CONTEXT('x');\n#{pc}=PRODUCT_CONTEXT('',#{app},'m');\n#{prod}=PRODUCT('{name}','{name}','',(#{pc}));\n\
#{pf}=PRODUCT_DEFINITION_FORMATION('','',#{prod});\n#{dc}=PRODUCT_DEFINITION_CONTEXT('p',#{app},'d');\n\
#{pd}=PRODUCT_DEFINITION('d','',#{pf},#{dc});\n#{pds}=PRODUCT_DEFINITION_SHAPE('','',#{pd});\n"
    ));
    (pd, pds)
}

fn within(
    limit: Duration,
    what: &str,
    f: impl FnOnce() -> Result<kadakas_step::StepImport, MeshError>,
) -> MeshError {
    let start = Instant::now();
    let r = f();
    assert!(start.elapsed() < limit, "{what} took {:?}", start.elapsed());
    r.expect_err(what)
}

#[test]
fn a_graph_of_products_with_two_usages_a_level_ends_at_the_instance_limit() {
    let mut w = String::from(HEAD);
    let mut ids = Ids(1000);
    let ctx = 999;
    w.push_str("#999=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#998)) REPRESENTATION_CONTEXT('',''));\n#998=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n");
    let n = 24;
    let mut pds = Vec::new();
    for i in 0..=n {
        pds.push(product(&mut w, &mut ids, &format!("P{i}")));
    }
    // Only the last product has a shape: a box. The others have no geometry at all.
    let solid = box_solid(&mut w, 100_000);
    let ax = ids.next();
    w.push_str(&format!("#{ax}=AXIS2_PLACEMENT_3D('',#{},$,$);\n", 100_001));
    let rep = ids.next();
    w.push_str(&format!("#{rep}=SHAPE_REPRESENTATION('',(#{ax},#{solid}),#{ctx});\n#{}=SHAPE_DEFINITION_REPRESENTATION(#{},#{rep});\n", ids.next(), pds[n].1));
    for i in 0..n {
        for _ in 0..2 {
            let u = ids.next();
            w.push_str(&format!(
                "#{u}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{},#{},$);\n",
                pds[i].0,
                pds[i + 1].0
            ));
        }
    }
    w.push_str(TAIL);
    assert!(w.len() < 20_000);
    let e = within(Duration::from_secs(5), "the product graph", || {
        read(w.as_bytes(), &StepOptions::default())
    });
    assert!(matches!(e, MeshError::TooManyInstances { .. }), "{e}");
}

#[test]
fn mapped_items_that_multiply_are_paid_for_before_they_are_copied() {
    // Five levels of ten mapped items each: 100,000 copies of a box if nothing stops it.
    let mut w = String::from(HEAD);
    let ctx = 999;
    w.push_str("#999=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#998)) REPRESENTATION_CONTEXT('',''));\n#998=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n");
    let solid = box_solid(&mut w, 100_000);
    w.push_str("#2000=CARTESIAN_POINT('',(0.,0.,0.));\n#2001=AXIS2_PLACEMENT_3D('',#2000,$,$);\n");
    let mut ids = Ids(3000);
    let mut ids_p = Ids(900_000);
    let mut prev = ids.next();
    w.push_str(&format!(
        "#{prev}=SHAPE_REPRESENTATION('l0',(#2001,#{solid}),#{ctx});\n"
    ));
    let (pd0, pds0) = product(&mut w, &mut ids_p, "top");
    let _ = pd0;
    for level in 1..=5 {
        let map = ids.next();
        w.push_str(&format!("#{map}=REPRESENTATION_MAP(#2001,#{prev});\n"));
        let mut items = Vec::new();
        for _ in 0..10 {
            let m = ids.next();
            w.push_str(&format!("#{m}=MAPPED_ITEM('',#{map},#2001);\n"));
            items.push(format!("#{m}"));
        }
        let rep = ids.next();
        w.push_str(&format!(
            "#{rep}=SHAPE_REPRESENTATION('l{level}',({}),#{ctx});\n",
            items.join(",")
        ));
        prev = rep;
    }
    w.push_str(&format!(
        "#{}=SHAPE_DEFINITION_REPRESENTATION(#{pds0},#{prev});\n",
        ids.next()
    ));
    w.push_str(TAIL);
    let before = Instant::now();
    let e = within(Duration::from_secs(10), "the mapped items", || {
        read(w.as_bytes(), &StepOptions::default())
    });
    assert!(
        matches!(
            e,
            MeshError::TooManyCopies { .. }
                | MeshError::TooManyInstances { .. }
                | MeshError::TooMuchMemory { .. }
                | MeshError::TooMuchWork { .. }
                | MeshError::TooManyTriangles { .. }
        ),
        "{e}"
    );
    assert!(before.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_loop_that_names_one_big_edge_thousands_of_times_is_refused_before_it_is_built() {
    let mut w = String::from(HEAD);
    w.push_str(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(1.E9,0.,0.));\n#3=AXIS2_PLACEMENT_3D('',#1,$,$);\n#4=CIRCLE('',#3,1.E9);\n\
#5=VERTEX_POINT('',#2);\n#6=EDGE_CURVE('',#5,#5,#4,.T.);\n#7=ORIENTED_EDGE('',*,*,#6,.T.);\n",
    );
    let items = vec!["#7"; 5000].join(",");
    w.push_str(&format!(
        "#8=EDGE_LOOP('',({items}));\n#9=FACE_OUTER_BOUND('',#8,.T.);\n#10=PLANE('',#3);\n#11=ADVANCED_FACE('',(#9),#10,.T.);\n\
#12=OPEN_SHELL('',(#11));\n#13=SHELL_BASED_SURFACE_MODEL('s',(#12));\n"
    ));
    w.push_str(TAIL);
    let e = within(Duration::from_secs(10), "the big loop", || {
        read(w.as_bytes(), &StepOptions::default())
    });
    assert!(e.to_string().contains("more than"), "{e}");
}

#[test]
fn many_faces_that_produce_nothing_are_charged_and_stop_at_the_work_limit() {
    // 3000 faces share one loop on a surface of revolution: each costs the search along the curve
    // for every point, and none gives a triangle.
    let mut w = String::from(HEAD);
    w.push_str(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(10.,0.,0.));\n#3=AXIS2_PLACEMENT_3D('',#1,$,$);\n#4=CIRCLE('',#3,10.);\n\
#5=VERTEX_POINT('',#2);\n#6=EDGE_CURVE('',#5,#5,#4,.T.);\n#7=ORIENTED_EDGE('',*,*,#6,.T.);\n#8=EDGE_LOOP('',(#7));\n#9=FACE_OUTER_BOUND('',#8,.T.);\n\
#10=DIRECTION('',(1.,0.,0.));\n#11=VECTOR('',#10,1.);\n#12=LINE('',#2,#11);\n#13=AXIS1_PLACEMENT('',#1,$);\n#14=SURFACE_OF_REVOLUTION('',#12,#13);\n",
    );
    let mut faces = Vec::new();
    for k in 0..3000 {
        w.push_str(&format!("#{}=ADVANCED_FACE('',(#9),#14,.T.);\n", 100 + k));
        faces.push(format!("#{}", 100 + k));
    }
    w.push_str(&format!(
        "#5000=OPEN_SHELL('',({}));\n#5001=SHELL_BASED_SURFACE_MODEL('s',(#5000));\n",
        faces.join(",")
    ));
    w.push_str(TAIL);
    let opts = StepOptions {
        step: StepLimits {
            max_work: 20_000_000,
            ..Default::default()
        },
        ..Default::default()
    };
    let e = within(
        Duration::from_secs(15),
        "the faces without triangles",
        || read(w.as_bytes(), &opts),
    );
    assert!(matches!(e, MeshError::TooMuchWork { .. }), "{e}");
    assert_eq!(e.kind(), "mesh_too_much_work");
}
