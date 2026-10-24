use kadakas_step::{StepOptions, read};
use std::collections::HashMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).unwrap();
    let mut o = StepOptions::default();
    if let Ok(t) = std::env::var("TOL") {
        o.tolerance_mm = Some(t.parse().unwrap());
    }
    let imp = read(&bytes, &o).unwrap();
    println!("{:?}", imp.report.tessellated);
    println!(
        "skipped {:?}",
        imp.report
            .skipped
            .iter()
            .filter(|s| s.1.contains("face") || s.1.contains("not"))
            .collect::<Vec<_>>()
    );
    println!("notes {:?}", imp.set.notes);
    for p in &imp.set.parts {
        println!(
            "part {} v={} t={}",
            p.name,
            p.positions.len(),
            p.triangles.len()
        );
        let mut c: HashMap<(u32, u32), (i32, i32)> = HashMap::new();
        for t in &p.triangles {
            for k in 0..3 {
                let (x, y) = (t[k], t[(k + 1) % 3]);
                let e = c.entry((x.min(y), x.max(y))).or_default();
                if x < y { e.0 += 1 } else { e.1 += 1 }
            }
        }
        let mut n = 0;
        for (k, v) in &c {
            if *v != (1, 1) {
                n += 1;
                if n <= 12 {
                    println!(
                        "open {:?} {:?} {:?} {:?}",
                        v, p.positions[k.0 as usize], p.positions[k.1 as usize], k
                    );
                }
            }
        }
        println!("open total {n}");
        if let Ok(v) = std::env::var("VTX") {
            let v: u32 = v.parse().unwrap();
            for t in &p.triangles {
                if t.contains(&v) {
                    println!("tri {:?} {:?}", t, t.map(|i| p.positions[i as usize]));
                }
            }
        }
    }
}
