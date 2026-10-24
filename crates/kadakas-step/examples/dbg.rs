use kadakas_step::{StepLimits, StepOptions, read};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).unwrap();
    let mut o = StepOptions::default();
    if let Ok(t) = std::env::var("MAXFACE") { o.step = StepLimits { max_face_triangles: t.parse().unwrap(), ..Default::default() }; }
    match read(&bytes, &o) {
        Ok(imp) => { println!("{:?}", imp.report.tessellated); println!("notes {:?}", imp.set.notes); println!("skipped {:?}", imp.report.skipped); }
        Err(e) => { println!("error {e:?}"); }
    }
}
