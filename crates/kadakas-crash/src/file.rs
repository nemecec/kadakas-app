//! The crash file: what the app knows when it dies.
//!
//! It is JSON, written next to the run record (`<run id>.crash.json`). It holds raw facts: the
//! scrubbing happens when a report is assembled ([`crate::report`]), so the rules can get better
//! without losing information, and so the file never leaves the computer as it is.

use serde::{Deserialize, Serialize};

/// The version of the file format.
pub const FORMAT: u32 = 1;
/// The most frames that a file keeps.
pub const MAX_FRAMES: usize = 64;
/// The most log lines and command names that a file keeps.
pub const MAX_LINES: usize = 50;
pub const MAX_COMMANDS: usize = 40;
/// The longest text of one field in a file.
pub const MAX_FIELD: usize = 2000;
/// The largest crash file that is read.
pub const MAX_FILE_BYTES: u64 = 256 << 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashKind {
    /// A Rust panic: message, thread and the whole backtrace.
    #[default]
    Panic,
    /// A signal or an exception (a driver, a library, unsafe code): the kind and the thread.
    Native,
}

impl CrashKind {
    pub fn id(self) -> &'static str {
        match self {
            CrashKind::Panic => "panic",
            CrashKind::Native => "native",
        }
    }
}

/// One frame of a backtrace.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    pub function: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// The graphics adapter, if the app knew it when it died.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gpu {
    pub adapter: String,
    pub driver: String,
    /// The wgpu backend: `Metal`, `Vulkan`, `Dx12`, `Gl`.
    pub backend: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrashFile {
    pub format: u32,
    pub kind: CrashKind,
    /// The run that wrote it (`runs/<id>`), as the launcher knows it.
    pub run_id: String,
    /// Seconds since 1970.
    pub created: u64,
    pub version: String,
    pub channel: String,
    /// For example `macos aarch64`; the version of the system when the app knew it.
    pub os: String,
    pub gpu: Option<Gpu>,
    /// The panic message, or the signal or exception of a native crash.
    pub message: String,
    pub thread: String,
    pub frames: Vec<Frame>,
    /// The last log lines, oldest first.
    pub log: Vec<String>,
    /// The names of the last commands and API methods (never their parameters), oldest first.
    pub commands: Vec<String>,
    /// The peak resident memory of the process, in MiB.
    pub memory_peak_mib: Option<u64>,
    pub uptime_secs: u64,
}

impl CrashFile {
    pub fn new(kind: CrashKind) -> CrashFile {
        CrashFile {
            format: FORMAT,
            kind,
            ..CrashFile::default()
        }
    }

    /// Cuts every part to its limit, so a damaged or hostile file cannot be large.
    pub fn bounded(mut self) -> CrashFile {
        let cut = |s: &mut String| {
            if s.len() > MAX_FIELD {
                let mut end = MAX_FIELD;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
            }
        };
        cut(&mut self.message);
        cut(&mut self.thread);
        cut(&mut self.version);
        cut(&mut self.channel);
        cut(&mut self.os);
        cut(&mut self.run_id);
        self.frames.truncate(MAX_FRAMES);
        for f in &mut self.frames {
            cut(&mut f.function);
            if let Some(file) = &mut f.file {
                cut(file);
            }
        }
        let keep_last = |v: &mut Vec<String>, n: usize| {
            if v.len() > n {
                v.drain(..v.len() - n);
            }
        };
        keep_last(&mut self.log, MAX_LINES);
        keep_last(&mut self.commands, MAX_COMMANDS);
        for l in self.log.iter_mut().chain(self.commands.iter_mut()) {
            cut(l);
        }
        if let Some(g) = &mut self.gpu {
            cut(&mut g.adapter);
            cut(&mut g.driver);
            cut(&mut g.backend);
        }
        self
    }

    /// Reads a file's text. `None` if it is not a crash file of a format that we know.
    pub fn parse(bytes: &[u8]) -> Option<CrashFile> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return None;
        }
        let f: CrashFile = serde_json::from_slice(bytes).ok()?;
        (f.format >= 1 && f.format <= FORMAT).then(|| f.bounded())
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).unwrap_or_default()
    }
}

/// Reads the text of a Rust backtrace (`std::backtrace::Backtrace`, the `Display` form):
///
/// ```text
///    0: kadakas_core::thing::run
///              at ./crates/kadakas-core/src/thing.rs:12:5
///    1: <unknown>
/// ```
///
/// Lines that do not look like frames are ignored. At most [`MAX_FRAMES`] frames are read.
pub fn parse_backtrace(text: &str) -> Vec<Frame> {
    let mut frames: Vec<Frame> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if let Some((num, rest)) = line.split_once(':')
            && !num.is_empty()
            && num.bytes().all(|b| b.is_ascii_digit())
            && !rest.starts_with(':')
        {
            if frames.len() >= MAX_FRAMES {
                break;
            }
            frames.push(Frame {
                function: rest.trim().to_string(),
                file: None,
                line: None,
            });
        } else if let Some(at) = line.strip_prefix("at ")
            && let Some(last) = frames.last_mut()
            && last.file.is_none()
        {
            // `path:line:column`; a path may hold `:` (a Windows drive), so cut from the right.
            let mut parts: Vec<&str> = at.rsplitn(3, ':').collect();
            parts.reverse();
            let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
            match parts.as_slice() {
                [path, l, c] if numeric(l) && numeric(c) => {
                    last.file = Some((*path).to_string());
                    last.line = l.parse().ok();
                }
                [path, l] if numeric(l) => {
                    last.file = Some((*path).to_string());
                    last.line = l.parse().ok();
                }
                [_, path_tail, l] if numeric(l) && !numeric(path_tail) => {
                    // `path:line` where the path holds one `:`.
                    let (head, _) = at.rsplit_once(':').unwrap_or((at, ""));
                    last.file = Some(head.to_string());
                    last.line = l.parse().ok();
                }
                _ => last.file = Some(at.to_string()),
            }
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "   0: std::backtrace_rs::backtrace::libunwind::trace
             at /rustc/abc123/library/std/src/backtrace.rs:117:9
   1: kadakas_core::thing::run
             at ./crates/kadakas-core/src/thing.rs:12:5
   2: <unknown>
  10: main
             at C:\\Users\\ann\\src\\main.rs:3:1
";

    #[test]
    fn a_backtrace_is_read_into_frames() {
        let f = parse_backtrace(SAMPLE);
        assert_eq!(f.len(), 4);
        assert_eq!(f[1].function, "kadakas_core::thing::run");
        assert_eq!(
            f[1].file.as_deref(),
            Some("./crates/kadakas-core/src/thing.rs")
        );
        assert_eq!(f[1].line, Some(12));
        assert_eq!(f[2].function, "<unknown>");
        assert_eq!(f[2].file, None);
        // A Windows drive letter is part of the path.
        assert_eq!(f[3].file.as_deref(), Some("C:\\Users\\ann\\src\\main.rs"));
        assert_eq!(f[3].line, Some(3));
    }

    #[test]
    fn the_place_of_a_frame_has_several_forms() {
        let f = parse_backtrace(
            "0: a\n   at src/a.rs:3\n1: b\n   at C:\\x\\b.rs:9\n2: c\n   at weird place\n",
        );
        assert_eq!(
            (f[0].file.as_deref(), f[0].line),
            (Some("src/a.rs"), Some(3))
        );
        assert_eq!(
            (f[1].file.as_deref(), f[1].line),
            (Some("C:\\x\\b.rs"), Some(9))
        );
        assert_eq!(
            (f[2].file.as_deref(), f[2].line),
            (Some("weird place"), None)
        );
    }

    #[test]
    fn the_adapter_fields_are_cut_too() {
        let mut f = CrashFile::new(CrashKind::Native);
        f.gpu = Some(Gpu {
            adapter: "a".repeat(5000),
            driver: "d".repeat(5000),
            backend: "b".repeat(5000),
        });
        let g = f.bounded().gpu.unwrap();
        assert_eq!(
            (g.adapter.len(), g.driver.len(), g.backend.len()),
            (MAX_FIELD, MAX_FIELD, MAX_FIELD)
        );
    }

    #[test]
    fn garbage_and_long_backtraces_are_safe() {
        assert!(parse_backtrace("").is_empty());
        assert!(parse_backtrace("hello\nat x:1:2\n::: 3").is_empty());
        let long: String = (0..500).map(|i| format!("{i}: f{i}\n")).collect();
        assert_eq!(parse_backtrace(&long).len(), MAX_FRAMES);
    }

    #[test]
    fn a_file_round_trips_and_is_bounded() {
        let mut f = CrashFile::new(CrashKind::Panic);
        f.message = "x".repeat(10_000);
        f.log = (0..200).map(|i| format!("line {i}")).collect();
        f.commands = (0..200).map(|i| format!("cmd{i}")).collect();
        f.frames = vec![Frame::default(); 500];
        let b = f.clone().bounded();
        assert_eq!(b.message.len(), MAX_FIELD);
        assert_eq!(b.log.len(), MAX_LINES);
        assert_eq!(b.log.last().unwrap(), "line 199");
        assert_eq!(b.commands.len(), MAX_COMMANDS);
        assert_eq!(b.frames.len(), MAX_FRAMES);
        let back = CrashFile::parse(&b.to_json()).unwrap();
        assert_eq!(back, b);
    }

    #[test]
    fn unknown_formats_and_junk_are_refused() {
        assert!(CrashFile::parse(b"nope").is_none());
        assert!(CrashFile::parse(br#"{"format":0}"#).is_none());
        assert!(CrashFile::parse(br#"{"format":99}"#).is_none());
        assert!(CrashFile::parse(br#"{"format":1,"message":"m"}"#).is_some());
        assert!(CrashFile::parse(&vec![b' '; (MAX_FILE_BYTES + 1) as usize]).is_none());
    }

    #[test]
    fn a_multibyte_message_is_cut_on_a_character() {
        let mut f = CrashFile::new(CrashKind::Panic);
        f.message = "ä".repeat(MAX_FIELD);
        let b = f.bounded();
        assert!(b.message.len() <= MAX_FIELD);
        assert!(b.message.chars().all(|c| c == 'ä'));
    }
}
