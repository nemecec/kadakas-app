//! `sign-manifest`: the CI tool for update manifests. See `docs/release.md` and `lib.rs`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = std::io::stdout();
    if let Err(e) = kadakas_sign_manifest::run(&args, &|k| std::env::var(k).ok(), &mut out) {
        eprintln!("sign-manifest: {e}");
        std::process::exit(1);
    }
}
