// The source of the helper program of the tests (see `testtool.rs`). The tests compile this file
// with `rustc`; it is not a module of the crate. It runs a tiny script, one command per line, the
// same on every system:
//   out TEXT      write a line to the standard output (`${NAME}` is the environment variable)
//   err TEXT      write a line to the standard error
//   sleep MS      wait
//   exit N        end with this code
//   count N       write the lines 0 to N-1
//   cat           copy the standard input to the standard output
//   yes           write lines of `y` without end
//   errbytes N    write N bytes to the standard error
//   bg CMD        start this program with the one-line script CMD, do not wait for it
// Use: `tool [-u] FILE` or `tool -c SCRIPT`.
use std::io::{Read, Write};

fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find('}') {
            Some(j) => {
                out.push_str(&std::env::var(&after[..j]).unwrap_or_default());
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn run(script: &str) {
    for line in script.lines() {
        let line = line.trim_end_matches('\r');
        let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
        let num = || arg.trim().parse::<u64>().unwrap();
        match cmd {
            "" => {}
            "out" => {
                let mut o = std::io::stdout().lock();
                let _ = writeln!(o, "{}", expand(arg));
                let _ = o.flush();
            }
            "err" => {
                let mut e = std::io::stderr().lock();
                let _ = writeln!(e, "{}", expand(arg));
                let _ = e.flush();
            }
            "sleep" => std::thread::sleep(std::time::Duration::from_millis(num())),
            "exit" => std::process::exit(num() as i32),
            "count" => {
                let mut o = std::io::stdout().lock();
                for i in 0..num() {
                    let _ = writeln!(o, "{i}");
                }
                let _ = o.flush();
            }
            "cat" => {
                let mut data = Vec::new();
                let _ = std::io::stdin().read_to_end(&mut data);
                let mut o = std::io::stdout().lock();
                let _ = o.write_all(&data);
                let _ = o.flush();
            }
            "yes" => {
                let mut o = std::io::stdout().lock();
                while o.write_all(b"y\n").is_ok() {}
                std::process::exit(0);
            }
            "errbytes" => {
                let mut e = std::io::stderr().lock();
                let _ = e.write_all(&vec![b'e'; num() as usize]);
                let _ = e.flush();
            }
            "bg" => {
                let me = std::env::current_exe().unwrap();
                std::process::Command::new(me)
                    .arg("-c")
                    .arg(arg)
                    .spawn()
                    .unwrap();
            }
            other => {
                eprintln!("unknown command {other:?}");
                std::process::exit(99);
            }
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("-u") {
        args.remove(0);
    }
    match args.as_slice() {
        [c, script] if c == "-c" => run(script),
        [file, ..] => run(&std::fs::read_to_string(file).unwrap()),
        [] => std::process::exit(98),
    }
}
