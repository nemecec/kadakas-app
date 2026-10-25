//! Reads a STEP file and prints the time, the work units and the size of the result. It is
//! the tool that measured the budgets in `docs/interop.md`:
//! `/usr/bin/time -l cargo run --release -p kadakas-step --example probe -- FILE [TOLERANCE_MM|-] [MAX_WORK]`.

use std::time::Instant;

use kadakas_step::{StepOptions, read};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("a STEP file");
    let tolerance_mm = args.next().and_then(|t| t.parse::<f64>().ok());
    let max_work = args.next().and_then(|t| t.parse::<u64>().ok());
    let bytes = std::fs::read(&path).expect("the file can be read");
    let start = Instant::now();
    let opts = StepOptions {
        tolerance_mm,
        step: kadakas_step::StepLimits {
            max_work: max_work.unwrap_or(kadakas_step::StepLimits::default().max_work),
            ..Default::default()
        },
        ..Default::default()
    };
    match read(&bytes, &opts) {
        Ok(i) => println!(
            "ok {} ms, work {}, parts {}, triangles {}, bytes {}",
            start.elapsed().as_millis(),
            i.report.work_units,
            i.set.parts.len(),
            i.set.totals.triangles,
            bytes.len()
        ),
        Err(e) => println!(
            "refused {} ms: {} ({})",
            start.elapsed().as_millis(),
            e,
            e.kind()
        ),
    }
}
