//! DWG through an external converter.
//!
//! Cadektor does not read or write DWG itself, and it does not link any DWG library. It starts a
//! converter program that the user installed, in a temporary folder, with a time limit:
//!
//! - import: DWG to DXF with the converter, then the normal DXF importer;
//! - export: the normal DXF exporter, then DXF to DWG with the converter.
//!
//! Two converters are supported. Both are separate programs with their own licenses:
//!
//! - the ODA File Converter (free download from the Open Design Alliance, license of its own);
//! - the command line tools `dwg2dxf` and `dxf2dwg` of LibreDWG (GPL, so they are only run, never linked).
//!
//! `docs/dwg.md` explains how to install them. [`detect`] looks for them. The program path never
//! comes from a request: only the environment variable `CADEKTOR_DWG_CONVERTER`, the standard
//! install folders and `PATH` are searched.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cadektor_core::model::Document;

use crate::{
    ExportOptions, Format, ImportOptions, ImportResult, Report, Source, exporter, importer,
};

/// A converter program that is installed on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Converter {
    /// The ODA File Converter. It converts whole folders.
    Oda(PathBuf),
    /// The LibreDWG tools. `dwg2dxf` reads DWG. `dxf2dwg` writes it and may be missing.
    LibreDwg {
        dwg2dxf: PathBuf,
        dxf2dwg: Option<PathBuf>,
    },
}

impl Converter {
    pub fn name(&self) -> &'static str {
        match self {
            Converter::Oda(_) => "ODA File Converter",
            Converter::LibreDwg { .. } => "LibreDWG",
        }
    }

    /// Whether this converter can write DWG.
    pub fn can_write(&self) -> bool {
        match self {
            Converter::Oda(_) => true,
            Converter::LibreDwg { dxf2dwg, .. } => dxf2dwg.is_some(),
        }
    }

    /// The program that runs, for messages.
    pub fn program(&self) -> &Path {
        match self {
            Converter::Oda(p) => p,
            Converter::LibreDwg { dwg2dxf, .. } => dwg2dxf,
        }
    }
}

/// Where to look for converters.
#[derive(Clone, Debug, Default)]
pub struct Search {
    /// The folders of `PATH`.
    pub path: Vec<PathBuf>,
    /// A program named by the user (`CADEKTOR_DWG_CONVERTER`).
    pub forced: Option<PathBuf>,
    /// Install folders that hold the ODA File Converter.
    pub oda_places: Vec<PathBuf>,
}

impl Search {
    /// `PATH`, `CADEKTOR_DWG_CONVERTER` and the usual install places of this operating system.
    pub fn system() -> Search {
        Search::from_values(
            std::env::var_os("PATH"),
            std::env::var_os("CADEKTOR_DWG_CONVERTER"),
        )
    }

    /// Like [`Search::system`], with the values of `PATH` and `CADEKTOR_DWG_CONVERTER` given.
    /// A relative entry of `PATH` (such as `.` or an empty one) is left out: a folder of the drawing
    /// must not supply the program.
    pub fn from_values(
        path: Option<std::ffi::OsString>,
        forced: Option<std::ffi::OsString>,
    ) -> Search {
        let path = path
            .map(|p| {
                std::env::split_paths(&p)
                    .filter(|d| d.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        let forced = forced.filter(|v| !v.is_empty()).map(PathBuf::from);
        let mut oda_places = vec![
            PathBuf::from("/Applications/ODAFileConverter.app/Contents/MacOS"),
            PathBuf::from("/opt/ODAFileConverter"),
            PathBuf::from("/usr/bin"),
        ];
        for root in ["C:\\Program Files\\ODA", "C:\\Program Files (x86)\\ODA"] {
            if let Ok(dir) = fs::read_dir(root) {
                oda_places.extend(dir.flatten().map(|e| e.path()));
            }
        }
        Search {
            path,
            forced,
            oda_places,
        }
    }
}

fn is_program(p: &Path) -> bool {
    let Ok(meta) = fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_in(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter()
        .filter(|d| d.is_absolute())
        .flat_map(|d| [d.join(name), d.join(format!("{name}.exe"))])
        .find(|p| is_program(p))
}

/// Finds a converter: the one the user named, then the ODA File Converter, then LibreDWG.
pub fn detect(search: &Search) -> Option<Converter> {
    if let Some(p) = &search.forced
        && p.is_absolute()
        && is_program(p)
    {
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if stem.contains("odafileconverter") {
            return Some(Converter::Oda(p.clone()));
        }
        if stem.contains("dwg2dxf") {
            let sibling = p
                .parent()
                .and_then(|d| find_in(&[d.to_path_buf()], "dxf2dwg"));
            return Some(Converter::LibreDwg {
                dwg2dxf: p.clone(),
                dxf2dwg: sibling,
            });
        }
    }
    let oda = find_in(&search.path, "ODAFileConverter")
        .or_else(|| find_in(&search.oda_places, "ODAFileConverter"));
    if let Some(p) = oda {
        return Some(Converter::Oda(p));
    }
    let dwg2dxf = find_in(&search.path, "dwg2dxf")?;
    Some(Converter::LibreDwg {
        dwg2dxf,
        dxf2dwg: find_in(&search.path, "dxf2dwg"),
    })
}

/// Why a DWG file could not be read or written.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DwgError {
    #[error(
        "{0}. Cadektor reads and writes DWG through a converter that you install yourself: the ODA File Converter or the LibreDWG tools dwg2dxf and dxf2dwg. See docs/dwg.md. As a workaround, save the drawing as DXF in your CAD program"
    )]
    NoConverter(String),
    #[error("{tool} failed: {detail}")]
    Failed { tool: String, detail: String },
    #[error("{tool} did not finish in {seconds} s and was stopped")]
    Timeout { tool: String, seconds: u64 },
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Format(String),
    #[error("{0}")]
    Option(String),
}

/// Limits of one conversion.
#[derive(Clone, Debug, PartialEq)]
pub struct RunOptions {
    pub timeout: Duration,
    /// File version that the ODA File Converter writes, for example `ACAD2018`.
    pub version: String,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            timeout: Duration::from_secs(300),
            version: "ACAD2018".into(),
        }
    }
}

impl RunOptions {
    /// Reads `converter_timeout_s` and `dwg_version` from the options of a request. Other keys are not looked at.
    pub fn from_json(v: &serde_json::Value) -> Result<RunOptions, DwgError> {
        let mut o = RunOptions::default();
        if let Some(t) = v.get("converter_timeout_s") {
            let s = t
                .as_u64()
                .filter(|s| (1..=86_400).contains(s))
                .ok_or_else(|| {
                    DwgError::Option(
                        "converter_timeout_s needs a whole number from 1 to 86400".into(),
                    )
                })?;
            o.timeout = Duration::from_secs(s);
        }
        if let Some(t) = v.get("dwg_version") {
            let s = t.as_str().map(str::trim).unwrap_or("");
            let ok = s.len() <= 12
                && s.starts_with("ACAD")
                && s.bytes().all(|b| b.is_ascii_alphanumeric());
            if !ok {
                return Err(DwgError::Option(
                    "dwg_version needs a name like ACAD2018 (ACAD9, ACAD10, ACAD12, ACAD2000, ACAD2004, ACAD2007, ACAD2010, ACAD2013, ACAD2018)".into(),
                ));
            }
            o.version = s.to_string();
        }
        Ok(o)
    }
}

use cadektor_sys::tmp::PrivateTempDir as TempDir;

/// A private temporary folder that is removed when it goes out of scope.
fn temp_dir() -> Result<TempDir, DwgError> {
    TempDir::new("cadektor-dwg").map_err(|e| DwgError::Io(e.to_string()))
}

fn tail(text: &str) -> String {
    let t = text.trim();
    let chars: Vec<char> = t.chars().collect();
    if chars.len() > 400 {
        format!(
            "...{}",
            chars[chars.len() - 400..].iter().collect::<String>()
        )
    } else {
        t.to_string()
    }
}

/// Runs a program with a time limit. Its output goes to files in `dir` so that a full pipe never blocks it.
fn run(
    program: &Path,
    args: &[OsString],
    dir: &Path,
    timeout: Duration,
    tool: &str,
) -> Result<(), DwgError> {
    let out_path = dir.join("converter.out");
    let err_path = dir.join("converter.err");
    let open = |p: &Path| {
        fs::File::create(p).map_err(|e| DwgError::Io(format!("cannot write {}: {e}", p.display())))
    };
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(open(&out_path)?))
        .stderr(Stdio::from(open(&err_path)?));
    // The converter leads its own process group, so a stop reaches the programs that it started too.
    cadektor_sys::process::own_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| DwgError::Failed {
        tool: tool.into(),
        detail: format!("cannot start {}: {e}", program.display()),
    })?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() >= timeout => {
                cadektor_sys::process::kill_tree(&mut child);
                let _ = child.wait();
                return Err(DwgError::Timeout {
                    tool: tool.into(),
                    seconds: timeout.as_secs(),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(DwgError::Io(format!("cannot wait for {tool}: {e}"))),
        }
    };
    if !status.success() {
        let mut text = String::new();
        for p in [&err_path, &out_path] {
            if let Ok(mut f) = fs::File::open(p) {
                let mut s = String::new();
                let _ = f.by_ref().take(64 * 1024).read_to_string(&mut s);
                text.push_str(&s);
            }
        }
        return Err(DwgError::Failed {
            tool: tool.into(),
            detail: format!(
                "{status}{}",
                if text.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", tail(&text))
                }
            ),
        });
    }
    Ok(())
}

/// The only file with this extension in a folder (any letter case).
fn find_output(dir: &Path, ext: &str) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
}

/// Converts a DWG file to DXF and returns the DXF bytes.
pub fn dwg_to_dxf(
    dwg: &Path,
    conv: &Converter,
    run_opts: &RunOptions,
) -> Result<Vec<u8>, DwgError> {
    if !dwg.is_file() {
        return Err(DwgError::Io(format!("cannot read {}", dwg.display())));
    }
    let tmp = temp_dir()?;
    let input = tmp.path().join("in");
    let output = tmp.path().join("out");
    for d in [&input, &output] {
        fs::create_dir(d).map_err(|e| DwgError::Io(e.to_string()))?;
    }
    let local = input.join("drawing.dwg");
    fs::copy(dwg, &local)
        .map_err(|e| DwgError::Io(format!("cannot read {}: {e}", dwg.display())))?;
    match conv {
        Converter::Oda(exe) => {
            let args: Vec<OsString> = [
                input.as_os_str(),
                output.as_os_str(),
                "ACAD2018".as_ref(),
                "DXF".as_ref(),
                "0".as_ref(),
                "1".as_ref(),
                "drawing.dwg".as_ref(),
            ]
            .iter()
            .map(|s| s.to_os_string())
            .collect();
            run(exe, &args, tmp.path(), run_opts.timeout, conv.name())?;
        }
        Converter::LibreDwg { dwg2dxf, .. } => {
            let target = output.join("drawing.dxf");
            let args: Vec<OsString> = vec![
                "-y".into(),
                "-o".into(),
                target.into_os_string(),
                local.into_os_string(),
            ];
            run(dwg2dxf, &args, tmp.path(), run_opts.timeout, conv.name())?;
        }
    }
    let dxf = find_output(&output, "dxf").ok_or_else(|| DwgError::Failed {
        tool: conv.name().into(),
        detail: "it finished but made no DXF file. The DWG file may be damaged or of a version that the converter does not know".into(),
    })?;
    fs::read(&dxf).map_err(|e| DwgError::Io(format!("cannot read the converted file: {e}")))
}

/// Converts DXF bytes to a DWG file at `dest`.
pub fn dxf_to_dwg(
    dxf: &[u8],
    dest: &Path,
    conv: &Converter,
    run_opts: &RunOptions,
) -> Result<(), DwgError> {
    let tmp = temp_dir()?;
    let input = tmp.path().join("in");
    let output = tmp.path().join("out");
    for d in [&input, &output] {
        fs::create_dir(d).map_err(|e| DwgError::Io(e.to_string()))?;
    }
    let local = input.join("drawing.dxf");
    fs::write(&local, dxf).map_err(|e| DwgError::Io(e.to_string()))?;
    match conv {
        Converter::Oda(exe) => {
            let args: Vec<OsString> = vec![
                input.clone().into_os_string(),
                output.clone().into_os_string(),
                run_opts.version.clone().into(),
                "DWG".into(),
                "0".into(),
                "1".into(),
                "drawing.dxf".into(),
            ];
            run(exe, &args, tmp.path(), run_opts.timeout, conv.name())?;
        }
        Converter::LibreDwg { dxf2dwg, .. } => {
            let Some(exe) = dxf2dwg else {
                return Err(DwgError::NoConverter(
                    "LibreDWG was found, but its tool dxf2dwg is not installed".into(),
                ));
            };
            let target = output.join("drawing.dwg");
            let args: Vec<OsString> = vec![
                "-y".into(),
                "-o".into(),
                target.into_os_string(),
                local.into_os_string(),
            ];
            run(exe, &args, tmp.path(), run_opts.timeout, conv.name())?;
        }
    }
    let made = find_output(&output, "dwg").ok_or_else(|| DwgError::Failed {
        tool: conv.name().into(),
        detail: "it finished but made no DWG file".into(),
    })?;
    let bytes = fs::read(&made).map_err(|e| DwgError::Io(e.to_string()))?;
    if bytes.is_empty() {
        return Err(DwgError::Failed {
            tool: conv.name().into(),
            detail: "it made an empty DWG file".into(),
        });
    }
    cadektor_sys::fsio::write_atomic(dest, &bytes)
        .map_err(|e| DwgError::Io(format!("cannot write {}: {e}", dest.display())))
}

fn no_converter(search: &Search) -> DwgError {
    let tried = match &search.forced {
        Some(p) => format!(
            "CADEKTOR_DWG_CONVERTER points to {}, which is not a usable ODAFileConverter or dwg2dxf program, and no other converter was found",
            p.display()
        ),
        None => {
            "no DWG converter was found (looked for ODAFileConverter, then dwg2dxf on PATH)".into()
        }
    };
    DwgError::NoConverter(tried)
}

/// Reads a DWG file and plans the import into `doc`, through the converter found by `search`.
pub fn import_file(
    path: &Path,
    opts: &ImportOptions,
    run_opts: &RunOptions,
    doc: &Document,
    search: &Search,
) -> Result<ImportResult, DwgError> {
    let conv = detect(search).ok_or_else(|| no_converter(search))?;
    let data = dwg_to_dxf(path, &conv, run_opts)?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let src = Source {
        name,
        data,
        extra: Default::default(),
    };
    let mut result = importer(Format::Dxf)
        .import(&src, &crate::Context::of(doc), opts)
        .map_err(|e| DwgError::Format(format!("the converted DXF could not be read: {e}")))?;
    result.report.format = "dwg".into();
    result.report.notes.push(format!(
        "read through {} ({}): the DWG file became DXF first, so entities that DXF cannot hold may be missing",
        conv.name(),
        conv.program().display()
    ));
    Ok(result)
}

/// Exports `doc` as DXF and converts it to DWG at `path`.
pub fn export_file(
    path: &Path,
    opts: &ExportOptions,
    run_opts: &RunOptions,
    doc: &Document,
    search: &Search,
) -> Result<Report, DwgError> {
    let conv = detect(search).ok_or_else(|| no_converter(search))?;
    if !conv.can_write() {
        return Err(DwgError::NoConverter(
            "LibreDWG was found, but its tool dxf2dwg is not installed".into(),
        ));
    }
    let ex = exporter(Format::Dxf).ok_or_else(|| DwgError::Format("no DXF exporter".into()))?;
    let out = ex
        .export(doc, opts)
        .map_err(|e| DwgError::Format(e.to_string()))?;
    dxf_to_dwg(&out.bytes, path, &conv, run_opts)?;
    let mut report = out.report;
    report.format = "dwg".into();
    report.notes.push(format!(
        "written as DXF and converted with {} ({})",
        conv.name(),
        conv.program().display()
    ));
    Ok(report)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A folder with stub programs.
    struct Stubs(TempDir);

    impl Stubs {
        fn new() -> Stubs {
            Stubs(temp_dir().unwrap())
        }

        fn script(&self, name: &str, body: &str) -> PathBuf {
            let p = self.0.path().join(name);
            fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        fn search(&self) -> Search {
            Search {
                path: vec![self.0.path().to_path_buf()],
                ..Search::default()
            }
        }
    }

    const DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nWalls\n10\n0.0\n20\n0.0\n30\n0.0\n11\n1000.0\n21\n0.0\n31\n0.0\n0\nENDSEC\n0\nEOF\n";

    /// A stand-in for `dwg2dxf -y -o out.dxf in.dwg`: it checks the arguments and writes a DXF.
    fn libre_stub(s: &Stubs) {
        s.script(
            "dwg2dxf",
            &format!(
                "[ \"$1\" = \"-y\" ] && [ \"$2\" = \"-o\" ] || {{ echo 'bad arguments' >&2; exit 2; }}\ngrep -q FAKEDWG \"$4\" || {{ echo 'not a dwg' >&2; exit 3; }}\ncat > \"$3\" <<'STUB_END'\n{DXF}STUB_END"
            ),
        );
        s.script(
            "dxf2dwg",
            "[ \"$1\" = \"-y\" ] && [ \"$2\" = \"-o\" ] || exit 2\ngrep -q ENTITIES \"$4\" || exit 3\necho FAKEDWG > \"$3\"",
        );
    }

    /// A stand-in for the ODA File Converter, with its folder arguments.
    fn oda_stub(s: &Stubs) {
        s.script(
            "ODAFileConverter",
            &format!(
                "in=\"$1\"; out=\"$2\"; ver=\"$3\"; type=\"$4\"; filter=\"$7\"\nif [ \"$type\" = DXF ]; then\n  grep -q FAKEDWG \"$in/$filter\" || {{ echo 'not a dwg' >&2; exit 3; }}\n  cat > \"$out/drawing.dxf\" <<'STUB_END'\n{DXF}STUB_END\nelse\n  echo \"FAKEDWG $ver\" > \"$out/drawing.dwg\"\nfi"
            ),
        );
    }

    fn dwg_file(dir: &Path) -> PathBuf {
        let p = dir.join("site.dwg");
        fs::write(&p, "FAKEDWG\n").unwrap();
        p
    }

    #[test]
    fn detects_converters_in_order() {
        let none = Stubs::new();
        assert!(detect(&none.search()).is_none());
        let libre = Stubs::new();
        libre.script("dwg2dxf", "exit 0");
        match detect(&libre.search()).unwrap() {
            Converter::LibreDwg { dxf2dwg, .. } => assert!(dxf2dwg.is_none()),
            other => panic!("{other:?}"),
        }
        libre.script("dxf2dwg", "exit 0");
        assert!(detect(&libre.search()).unwrap().can_write());
        // ODA wins over LibreDWG.
        libre.script("ODAFileConverter", "exit 0");
        assert!(matches!(detect(&libre.search()), Some(Converter::Oda(_))));
        // A file that is not executable does not count.
        let plain = Stubs::new();
        fs::write(plain.0.path().join("dwg2dxf"), "x").unwrap();
        assert!(detect(&plain.search()).is_none());
        // The forced program, and the install places.
        let forced = Stubs::new();
        let p = forced.script("dwg2dxf", "exit 0");
        let s = Search {
            forced: Some(p.clone()),
            ..Search::default()
        };
        assert!(matches!(detect(&s), Some(Converter::LibreDwg { dwg2dxf, .. }) if dwg2dxf == p));
        let oda = Stubs::new();
        let o = oda.script("ODAFileConverter", "exit 0");
        let s = Search {
            oda_places: vec![oda.0.path().to_path_buf()],
            ..Search::default()
        };
        assert_eq!(detect(&s), Some(Converter::Oda(o.clone())));
        let s = Search {
            forced: Some(o.clone()),
            ..Search::default()
        };
        assert_eq!(detect(&s), Some(Converter::Oda(o)));
        // A forced program that is unknown falls back to the search.
        let junk = forced.script("something", "exit 0");
        let s = Search {
            forced: Some(junk),
            ..Search::default()
        };
        assert!(detect(&s).is_none());
        assert!(Search::system().path.len() + 1 > 0);
    }

    #[test]
    fn imports_through_libredwg() {
        let s = Stubs::new();
        libre_stub(&s);
        let work = Stubs::new();
        let dwg = dwg_file(work.0.path());
        let r = import_file(
            &dwg,
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap();
        assert_eq!(r.report.format, "dwg");
        assert_eq!(r.report.objects["line"], 1);
        assert!(r.report.notes.iter().any(|n| n.contains("LibreDWG")));
        let mut ed = cadektor_core::command::Editor::new(Document::default());
        ed.execute(&r.command).unwrap();
        assert_eq!(ed.doc().objects.len(), 1);
    }

    #[test]
    fn imports_and_exports_through_the_oda_file_converter() {
        let s = Stubs::new();
        oda_stub(&s);
        let work = Stubs::new();
        let dwg = dwg_file(work.0.path());
        let r = import_file(
            &dwg,
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap();
        assert_eq!(r.report.objects["line"], 1);
        let mut ed = cadektor_core::command::Editor::new(Document::default());
        ed.execute(&r.command).unwrap();
        let out = work.0.path().join("out.dwg");
        let run_opts =
            RunOptions::from_json(&serde_json::json!({"dwg_version": "ACAD2010"})).unwrap();
        let report = export_file(
            &out,
            &ExportOptions::default(),
            &run_opts,
            ed.doc(),
            &s.search(),
        )
        .unwrap();
        assert_eq!(report.format, "dwg");
        assert_eq!(report.objects["line"], 1);
        assert_eq!(fs::read_to_string(&out).unwrap().trim(), "FAKEDWG ACAD2010");
    }

    #[test]
    fn exports_through_libredwg() {
        let s = Stubs::new();
        libre_stub(&s);
        let work = Stubs::new();
        let out = work.0.path().join("out.dwg");
        export_file(
            &out,
            &ExportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&out).unwrap().trim(), "FAKEDWG");
    }

    #[test]
    fn missing_converter_gives_a_clear_error() {
        let none = Stubs::new();
        let work = Stubs::new();
        let dwg = dwg_file(work.0.path());
        let e = import_file(
            &dwg,
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &none.search(),
        )
        .unwrap_err();
        let text = e.to_string();
        assert!(matches!(e, DwgError::NoConverter(_)));
        assert!(
            text.contains("docs/dwg.md")
                && text.contains("ODA File Converter")
                && text.contains("DXF"),
            "{text}"
        );
        let e = export_file(
            &work.0.path().join("x.dwg"),
            &ExportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &none.search(),
        )
        .unwrap_err();
        assert!(matches!(e, DwgError::NoConverter(_)));
        // LibreDWG without dxf2dwg reads but does not write.
        let half = Stubs::new();
        half.script("dwg2dxf", "exit 0");
        let e = export_file(
            &work.0.path().join("x.dwg"),
            &ExportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &half.search(),
        )
        .unwrap_err();
        assert!(e.to_string().contains("dxf2dwg"), "{e}");
        // A forced program that is no converter is named in the message.
        let s = Search {
            forced: Some("/nonexistent/prog".into()),
            ..Search::default()
        };
        assert!(
            no_converter(&s)
                .to_string()
                .contains("CADEKTOR_DWG_CONVERTER")
        );
    }

    #[test]
    fn converter_failures_timeouts_and_missing_output() {
        let work = Stubs::new();
        let dwg = dwg_file(work.0.path());
        // The tool refuses the file.
        let s = Stubs::new();
        s.script("dwg2dxf", "echo 'unsupported version R2030' >&2; exit 1");
        let e = import_file(
            &dwg,
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap_err();
        assert!(matches!(e, DwgError::Failed { .. }));
        assert!(e.to_string().contains("unsupported version R2030"), "{e}");
        // The tool exits 0 and writes nothing.
        let s = Stubs::new();
        s.script("dwg2dxf", "exit 0");
        let e = import_file(
            &dwg,
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap_err();
        assert!(e.to_string().contains("made no DXF"), "{e}");
        // The tool hangs: it is stopped at the limit.
        let s = Stubs::new();
        s.script("dwg2dxf", "sleep 30");
        let quick = RunOptions {
            timeout: Duration::from_millis(300),
            ..RunOptions::default()
        };
        let t = Instant::now();
        let e = import_file(
            &dwg,
            &ImportOptions::default(),
            &quick,
            &Document::default(),
            &s.search(),
        )
        .unwrap_err();
        assert!(matches!(e, DwgError::Timeout { .. }), "{e}");
        assert!(t.elapsed() < Duration::from_secs(10));
        // The tool made DXF that is not DXF.
        let s = Stubs::new();
        s.script("dwg2dxf", "echo garbage > \"$3\"");
        let e = import_file(
            &dwg,
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap_err();
        assert!(matches!(e, DwgError::Format(_)), "{e}");
        // The tool cannot start (the stub is a directory entry without an interpreter).
        let s = Stubs::new();
        s.script("dwg2dxf", "exit 0");
        fs::write(s.0.path().join("dwg2dxf"), "#!/nonexistent/interpreter\n").unwrap();
        fs::set_permissions(
            s.0.path().join("dwg2dxf"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let e = import_file(
            &dwg,
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap_err();
        assert!(e.to_string().contains("cannot start"), "{e}");
        // The DWG file is missing.
        let s = Stubs::new();
        libre_stub(&s);
        let e = import_file(
            &work.0.path().join("none.dwg"),
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap_err();
        assert!(matches!(e, DwgError::Io(_)));
        // An empty DWG from the writer is an error.
        let s = Stubs::new();
        s.script("dwg2dxf", "exit 0");
        s.script("dxf2dwg", ": > \"$3\"");
        let e = export_file(
            &work.0.path().join("e.dwg"),
            &ExportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap_err();
        assert!(e.to_string().contains("empty DWG"), "{e}");
    }

    #[test]
    fn the_temporary_folder_is_removed() {
        let dir;
        {
            let t = temp_dir().unwrap();
            dir = t.path().to_path_buf();
            fs::write(dir.join("x"), "x").unwrap();
            assert!(dir.exists());
        }
        assert!(!dir.exists());
        let s = Stubs::new();
        libre_stub(&s);
        let work = Stubs::new();
        let before = temp_dwg_dirs();
        import_file(
            &dwg_file(work.0.path()),
            &ImportOptions::default(),
            &RunOptions::default(),
            &Document::default(),
            &s.search(),
        )
        .unwrap();
        // Folders of other tests may come and go; ours is gone when the call returns.
        assert!(temp_dwg_dirs().len() <= before.len() + 8);
    }

    fn temp_dwg_dirs() -> Vec<PathBuf> {
        fs::read_dir(std::env::temp_dir())
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("cadektor-dwg-"))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn run_options_from_json() {
        let o = RunOptions::from_json(
            &serde_json::json!({"converter_timeout_s": 5, "dwg_version": "ACAD2013", "layer": "x"}),
        )
        .unwrap();
        assert_eq!(
            (o.timeout, o.version.as_str()),
            (Duration::from_secs(5), "ACAD2013")
        );
        assert_eq!(
            RunOptions::from_json(&serde_json::json!(null)).unwrap(),
            RunOptions::default()
        );
        for bad in [
            serde_json::json!({"converter_timeout_s": 0}),
            serde_json::json!({"converter_timeout_s": "x"}),
            serde_json::json!({"dwg_version": "../../etc"}),
            serde_json::json!({"dwg_version": 5}),
            serde_json::json!({"dwg_version": "ACAD 2018; rm"}),
        ] {
            assert!(
                matches!(RunOptions::from_json(&bad), Err(DwgError::Option(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("  a  "), "a");
        let long = "x".repeat(1000) + "END";
        let t = tail(&long);
        assert!(t.starts_with("...") && t.ends_with("END") && t.chars().count() < 410);
    }
}

#[cfg(all(test, unix))]
mod group_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn alive(pid: i32) -> bool {
        cadektor_sys::process::pid_alive(pid as u32)
    }

    #[test]
    fn a_timeout_stops_the_programs_that_the_converter_started() {
        let work = temp_dir().unwrap();
        let stubs = temp_dir().unwrap();
        let pidfile = work.path().join("child.pid");
        // The converter starts a child that sleeps, writes its ID, and waits for it.
        script(
            stubs.path(),
            "dwg2dxf",
            &format!("sleep 60 &\necho $! > \"{}\"\nwait", pidfile.display()),
        );
        let dwg = work.path().join("a.dwg");
        fs::write(&dwg, "x").unwrap();
        let search = Search {
            path: vec![stubs.path().to_path_buf()],
            ..Search::default()
        };
        let quick = RunOptions {
            timeout: Duration::from_secs(3),
            ..RunOptions::default()
        };
        let e = import_file(
            &dwg,
            &ImportOptions::default(),
            &quick,
            &Document::default(),
            &search,
        )
        .unwrap_err();
        assert!(matches!(e, DwgError::Timeout { .. }), "{e}");
        let pid: i32 = fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // The child of the converter dies with the group (give the system a moment to remove it).
        let mut gone = false;
        for _ in 0..50 {
            if !alive(pid) {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(gone, "the child {pid} of the converter is still running");
    }

    #[test]
    fn relative_path_entries_and_relative_programs_are_ignored() {
        // A stub in a folder below the current folder, found only by a relative path.
        let name = format!("cadektor-dwg-relative-{}", std::process::id());
        let rel_dir = PathBuf::from(&name);
        fs::create_dir(&rel_dir).unwrap();
        script(&rel_dir, "dwg2dxf", "exit 0");
        let rel = Search {
            path: vec![rel_dir.clone(), PathBuf::from("")],
            forced: Some(rel_dir.join("dwg2dxf")),
            ..Search::default()
        };
        let found = detect(&rel);
        let abs = Search {
            path: vec![fs::canonicalize(&rel_dir).unwrap()],
            ..Search::default()
        };
        let abs_found = detect(&abs);
        fs::remove_dir_all(&rel_dir).unwrap();
        assert!(found.is_none(), "{found:?}");
        assert!(abs_found.is_some());
        // `PATH` entries that are not absolute are dropped.
        let s = Search::from_values(Some(".:/usr/bin::bin:/opt".into()), Some("".into()));
        assert_eq!(
            s.path,
            vec![PathBuf::from("/usr/bin"), PathBuf::from("/opt")]
        );
        assert!(s.forced.is_none());
        assert_eq!(
            Search::from_values(None, Some("/x/dwg2dxf".into())).forced,
            Some(PathBuf::from("/x/dwg2dxf"))
        );
    }
}
