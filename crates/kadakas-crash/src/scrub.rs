//! What may leave the computer in a crash report, and what may not (PLAT-20).
//!
//! Never in a report: document content, file names and paths, user names, e-mail addresses,
//! tokens, and the text of the person. The functions here are pure: the same input and the same
//! [`Context`] give the same output, and nothing reads the disk or the environment (except
//! [`Context::from_env`], which the caller uses once).
//!
//! The rules:
//!
//! - An absolute path (`/Users/ann/My Plan.kad`, `C:\Users\ann\x`, `\\server\share\x`, `~/x`)
//!   becomes `<path>`. A path may hold spaces, so it runs to the end of the line, a quote, a
//!   closing bracket, a `: ` or a `, `. That removes more text than needed, never less.
//! - A URL keeps its scheme and host. The user and password, the path, the query and the
//!   fragment go (`https://host/<url-path>`). A `file:` URL becomes `file://<path>`.
//! - The user name of the computer (a whole word of 3 letters or more, in any case) becomes
//!   `<user>`.
//! - Then the rules of the feedback channel (`kadakas_feedback::scrub::harsh`): e-mail
//!   addresses, `token=`, `password=` and other keyed secrets, `Bearer` tokens, known token
//!   shapes, long mixed words, names in quotes, and the stems of file names (`<file>.kad`).
//! - A frame of a backtrace keeps its function and the line, and its file only if the file is
//!   part of Kadakas (`crates/kadakas-core/src/x.rs`), the Rust library (`rustc/library/...`) or a
//!   crate from the registry (`registry/serde-1.0.0/src/lib.rs`). Any other file becomes
//!   `<path>/<file>.ext`.

use crate::file::Frame;
use kadakas_feedback::scrub as fb;

/// The longest input that is looked at.
const MAX_INPUT: usize = 64 << 10;

/// What the scrubbing knows about the computer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// The login name.
    pub user: Option<String>,
    /// The home folder.
    pub home: Option<String>,
    /// Other names of a home folder (`USERPROFILE` next to `HOME`, a drive and a path).
    pub more_homes: Vec<String>,
}

impl Context {
    /// Reads the login name and the home folders from the environment.
    pub fn from_env() -> Context {
        Context::from_lookup(&|k| std::env::var(k).ok())
    }

    /// Like [`Context::from_env`], with the variables given. On Windows `HOME` is often not set
    /// and `USERPROFILE` is the home folder; both are used when both exist.
    pub fn from_lookup(get: &dyn Fn(&str) -> Option<String>) -> Context {
        let get = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let user = get("USER")
            .or_else(|| get("USERNAME"))
            .or_else(|| get("LOGNAME"));
        let mut homes: Vec<String> = Vec::new();
        let drive_path = match (get("HOMEDRIVE"), get("HOMEPATH")) {
            (Some(d), Some(p)) => Some(format!("{d}{p}")),
            _ => None,
        };
        for h in [get("HOME"), get("USERPROFILE"), drive_path]
            .into_iter()
            .flatten()
        {
            if !homes.contains(&h) {
                homes.push(h);
            }
        }
        let mut it = homes.into_iter();
        Context {
            user,
            home: it.next(),
            more_homes: it.collect(),
        }
    }

    pub fn new(user: &str, home: &str) -> Context {
        Context {
            user: Some(user.to_string()).filter(|u| !u.is_empty()),
            home: Some(home.to_string()).filter(|u| !u.is_empty()),
            more_homes: Vec::new(),
        }
    }
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// The first segments of an absolute Unix path that are folders of the system. A path that
/// starts with one of these is a path even when it has no second slash (`/tmp`).
const ROOTS: [&str; 20] = [
    "Users",
    "home",
    "tmp",
    "var",
    "etc",
    "opt",
    "private",
    "Volumes",
    "mnt",
    "root",
    "usr",
    "Library",
    "Applications",
    "run",
    "proc",
    "nix",
    "snap",
    "media",
    "srv",
    "dev",
];

/// True if `prev` may come before the start of a path or a URL.
fn boundary(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => c.is_whitespace() || "\"'`([{<=,;:>|".contains(c),
    }
}

/// Where a path that starts at `i` ends (exclusive).
fn path_end(chars: &[char], i: usize, prev: Option<char>) -> usize {
    // A path in quotes or brackets ends at the matching mark. A quote that does not match the
    // start is part of the name (`/home/o'neil`).
    let closer = match prev {
        Some(q @ ('"' | '\'' | '`')) => Some(q),
        Some('(') => Some(')'),
        Some('[') => Some(']'),
        Some('{') => Some('}'),
        _ => None,
    };
    let mut j = i;
    while j < chars.len() {
        let c = chars[j];
        if c == '\n' || c == '\r' || Some(c) == closer {
            break;
        }
        if c == ':' {
            // `: ` ends the path (`/x/y: not found`); so does `:12` (a line number).
            let next = chars.get(j + 1).copied();
            if next.is_none_or(|n| n.is_whitespace() || n.is_ascii_digit()) && j > i + 1 {
                break;
            }
        }
        if c == ',' && chars.get(j + 1).is_none_or(|n| n.is_whitespace()) {
            break;
        }
        j += 1;
    }
    // Trailing marks of a sentence are not part of the path.
    while j > i && matches!(chars[j - 1], '.' | ',' | ';' | ':' | '!' | '?') {
        j -= 1;
    }
    j
}

/// True if a path starts at `i` (the caller has checked the boundary).
fn path_start(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    let at = |k: usize| chars.get(i + k).copied();
    // `C:\x` or `C:/x`
    if c.is_ascii_alphabetic() && at(1) == Some(':') && at(2).is_some_and(is_sep) {
        return true;
    }
    // `\\server\share`
    if c == '\\' && at(1) == Some('\\') && at(2).is_some_and(|n| !n.is_whitespace()) {
        return true;
    }
    // `~/x`
    if c == '~' && at(1).is_some_and(is_sep) {
        return true;
    }
    if c != '/' {
        return false;
    }
    let Some(n) = at(1) else { return false };
    if n == '/' || n.is_whitespace() {
        return false;
    }
    // A second slash, or a folder of the system.
    let rest: String = chars[i + 1..].iter().take(40).collect();
    let first = rest
        .split(|ch: char| ch == '/' || ch.is_whitespace())
        .next()
        .unwrap_or("");
    rest.contains('/') && !rest.split('/').next().unwrap_or("").contains(' ')
        || ROOTS.contains(&first)
}

fn paths(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        if boundary(prev) && path_start(&chars, i) {
            let end = path_end(&chars, i, prev);
            if end > i {
                out.push_str("<path>");
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Cuts every URL to its scheme and host.
fn urls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("://") {
        // The scheme: letters, digits, `+`, `-`, `.` right before `://`.
        let before = &rest[..p];
        // The character found may take more than one byte: start after all of it.
        let scheme_start = before
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
            .map_or(0, |i| {
                i + before[i..].chars().next().map_or(1, char::len_utf8)
            });
        let scheme = &before[scheme_start..];
        let after = &rest[p + 3..];
        // The URL ends at white space or a quote or a closing bracket.
        let end = after
            .find(|c: char| c.is_whitespace() || "\"'`<>)]}".contains(c))
            .unwrap_or(after.len());
        let url = &after[..end];
        out.push_str(&rest[..scheme_start]);
        if scheme.is_empty() {
            // Not a URL: `://` alone. Keep it and go on.
            out.push_str("://");
            rest = after;
            continue;
        }
        out.push_str(scheme);
        out.push_str("://");
        if scheme.eq_ignore_ascii_case("file") {
            out.push_str("<path>");
        } else {
            let (authority, tail) = match url.find(['/', '?', '#']) {
                Some(i) => (&url[..i], &url[i..]),
                None => (url, ""),
            };
            // No user or password. Only the official sites keep their name.
            let host = authority.rsplit('@').next().unwrap_or(authority);
            if official_host(host) {
                out.push_str(host);
            } else {
                out.push_str("<host>");
            }
            if !tail.is_empty() && tail != "/" {
                out.push_str("/<url-path>");
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// The domain of the official Kadakas servers: the last two labels of the address of the
/// feedback server (`feedback.kadakas.invalid` gives `kadakas.invalid`).
fn official_domain() -> String {
    let url = kadakas_feedback::DEFAULT_URL;
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', ':'])
        .next()
        .unwrap_or("");
    let labels: Vec<&str> = host.split('.').collect();
    labels[labels.len().saturating_sub(2)..].join(".")
}

/// True for the official Kadakas domain and its subdomains (with or without a port).
fn official_host(host: &str) -> bool {
    let host = host.rsplit_once(':').map_or(host, |(h, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            h
        } else {
            host
        }
    });
    let host = host.to_ascii_lowercase();
    let domain = official_domain();
    !domain.is_empty()
        && (host == domain
            || host
                .strip_suffix(domain.as_str())
                .is_some_and(|h| h.ends_with('.')))
}

/// File extensions that make a name a file name.
const FILE_EXTS: [&str; 33] = [
    "kad", "json", "dxf", "dwg", "ifc", "shp", "geojson", "png", "jpg", "jpeg", "svg", "pdf",
    "csv", "py", "wasm", "txt", "gltf", "glb", "obj", "xml", "gpkg", "kml", "kmz", "tif", "tiff",
    "bcfzip", "zip", "js", "toml", "ini", "bak", "tmp", "log",
];

/// Replaces a file name that has a known extension with `<file>`, also when the name has spaces
/// and no quotes (`opening My Garden Plan.kad failed`). Where the name starts is not known, so
/// the text before the extension goes back to the last mark (a quote, a bracket, `:`, `,`, `;`,
/// `=`, `>` or the start of the line). That removes more than the name, never less.
fn file_names(s: &str) -> String {
    s.split('\n').map(line_names).collect::<Vec<_>>().join("\n")
}

fn line_names(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let is_mark = |c: char| "\"'`()[]{}<>:,;=|".contains(c);
    let mut out = String::with_capacity(line.len());
    let mut done = 0; // chars before this index are in `out`
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '.' && i > done && !chars[i - 1].is_whitespace() {
            let mut e = i + 1;
            while e < chars.len() && chars[e].is_ascii_alphanumeric() {
                e += 1;
            }
            let ext: String = chars[i + 1..e]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            let after_ok = e >= chars.len() || !(chars[e].is_alphanumeric() || chars[e] == '_');
            if after_ok && FILE_EXTS.contains(&ext.as_str()) {
                // Back to the last mark; the white space after the mark stays.
                let mut start = i;
                while start > done && !is_mark(chars[start - 1]) {
                    start -= 1;
                }
                while start < i && chars[start].is_whitespace() {
                    start += 1;
                }
                out.extend(&chars[done..start]);
                out.push_str("<file>");
                done = e;
                i = e;
                continue;
            }
        }
        i += 1;
    }
    out.extend(&chars[done..]);
    out
}

/// Replaces a word everywhere it stands alone, ignoring the case.
fn replace_word(s: &str, word: &str, with: &str) -> String {
    if word.chars().count() < 3 {
        return s.to_string();
    }
    let w: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<char> = s.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let end = i + w.len();
        let hit = end <= chars.len()
            && (i == 0 || !is_word(chars[i - 1]))
            && (end == chars.len() || !is_word(chars[end]))
            && chars[i..end]
                .iter()
                .flat_map(|c| c.to_lowercase())
                .eq(w.iter().copied());
        if hit {
            out.push_str(with);
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Scrubs a text that Kadakas fills in itself: a panic message, a log line.
pub fn text(s: &str, ctx: &Context) -> String {
    let s: String = s
        .chars()
        .take(MAX_INPUT)
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    let (s, kept) = protect(&s);
    let s = urls(&s);
    let s = file_names(&strings(&paths(&s)));
    let mut s = s;
    for home in ctx.home.iter().chain(ctx.more_homes.iter()) {
        // A home folder that is written without its path rules (a bare mention).
        let home = home.trim_end_matches(is_sep);
        if home.chars().count() >= 4 {
            s = s.replace(home, "<home>");
        }
    }
    let mut s = fb::harsh(&s);
    // The user name goes after the rules for e-mail addresses, which need it whole.
    if let Some(user) = &ctx.user {
        s = replace_word(&s, user, "<user>");
    }
    restore(&s, &kept)
}

/// Fragments of the standard panic messages. They name Rust types and values, never text of a
/// person, and the rules for quoted names would take them for names.
const STANDARD: [&str; 8] = [
    "`Option::unwrap()`",
    "`Result::unwrap()`",
    "`Option::expect()`",
    "`Result::expect()`",
    "`None`",
    "`Some`",
    "`Err`",
    "`Ok`",
];

/// Puts a marker in place of each standard fragment (private use characters, which no rule
/// touches). Returns the text and the fragments.
fn protect(s: &str) -> (String, Vec<&'static str>) {
    let mut out = s.to_string();
    let mut kept = Vec::new();
    for (i, frag) in STANDARD.iter().enumerate() {
        let mark = marker(i);
        if out.contains(frag) {
            out = out.replace(frag, &mark);
            kept.push(*frag);
        }
    }
    (out, kept)
}

fn marker(i: usize) -> String {
    // U+E000 and up are private use: they are not letters, digits or white space.
    format!(
        "\u{E000}{}\u{E001}",
        char::from_u32(0xE010 + i as u32).unwrap_or('x')
    )
}

fn restore(s: &str, kept: &[&'static str]) -> String {
    let mut out = s.to_string();
    for (i, frag) in STANDARD.iter().enumerate() {
        if kept.contains(frag) {
            out = out.replace(&marker(i), frag);
        }
    }
    out
}

/// In a line with two or more double quotes, everything from the first to the last quote
/// becomes `"<text>"`. A Debug print of an error holds strings, and strings hold names and
/// paths, also with escaped quotes inside; this takes all of it, and a little more.
fn strings(s: &str) -> String {
    s.split('\n')
        .map(|line| {
            let first = line.find('"');
            let last = line.rfind('"');
            match (first, last) {
                (Some(a), Some(b)) if b > a => {
                    format!("{}\"<text>\"{}", &line[..a], &line[b + 1..])
                }
                _ => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Scrubs the source file of a frame (see the module text).
pub fn source_path(path: &str) -> String {
    let n = path.replace('\\', "/");
    // Our own sources, from the build computer or relative to the repository.
    if let Some(i) = n.find("/crates/kadakas-") {
        return clean_tail(&n[i + 1..]);
    }
    if n.starts_with("crates/kadakas-") {
        return clean_tail(&n);
    }
    if let Some(i) = n.find("/registry/src/") {
        // `<index>/<name>-<version>/src/lib.rs`
        let tail = &n[i + "/registry/src/".len()..];
        let mut parts = tail.splitn(2, '/');
        let _index = parts.next();
        if let Some(rest) = parts.next()
            && registry_crate(rest)
        {
            return format!("registry/{}", clean_tail(rest));
        }
    }
    if let Some(i) = n.find("/.cargo/git/checkouts/") {
        // `<repo>-<hash>/<rev>/<path>`
        let tail = &n[i + "/.cargo/git/checkouts/".len()..];
        let parts: Vec<&str> = tail.splitn(3, '/').collect();
        if parts.len() == 3 {
            let repo = parts[0].rsplit_once('-').map_or(parts[0], |(r, _)| r);
            if repo
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return format!("git/{repo}/{}", clean_tail(parts[2]));
            }
        }
    }
    if let Some(rest) = n.strip_prefix("/rustc/") {
        // `/rustc/<hash>/library/core/src/x.rs`
        if let Some((hash, tail)) = rest.split_once('/')
            && hash.len() >= 7
            && hash.chars().all(|c| c.is_ascii_hexdigit())
            && tail.starts_with("library/")
        {
            return format!("rustc/{}", clean_tail(tail));
        }
    }
    let file = n.rsplit('/').next().unwrap_or("");
    match file.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 5
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            format!("<path>/<file>.{ext}")
        }
        _ => "<path>".to_string(),
    }
}

/// `name-1.2.3/...`: the first segment is a crate and a version.
fn registry_crate(rest: &str) -> bool {
    let first = rest.split('/').next().unwrap_or("");
    match first.rsplit_once('-') {
        Some((name, ver)) => {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                && ver.chars().next().is_some_and(|c| c.is_ascii_digit())
                && ver
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
        }
        None => false,
    }
}

/// The part of a known source path after its root: only plain path characters stay.
fn clean_tail(p: &str) -> String {
    p.split('/')
        .map(|seg| {
            if seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
            {
                seg.to_string()
            } else {
                "<x>".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The function name of a frame: no hash suffix (`::h0123456789abcdef`), no control characters,
/// at most 200 characters.
pub fn function(f: &str) -> String {
    let f: String = f.chars().filter(|c| !c.is_control()).take(200).collect();
    strip_hash(&f).to_string()
}

/// Removes the `::h` and 16 hex digits that the Rust compiler adds to a legacy symbol.
pub fn strip_hash(f: &str) -> &str {
    if let Some(i) = f.rfind("::h") {
        let h = &f[i + 3..];
        if h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return &f[..i];
        }
    }
    f
}

/// Scrubs one frame.
pub fn frame(f: &Frame) -> Frame {
    Frame {
        function: function(&f.function),
        file: f.file.as_deref().map(source_path),
        line: f.line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context::new("annika", "/Users/annika")
    }

    fn t(s: &str) -> String {
        text(s, &ctx())
    }

    #[test]
    fn the_context_can_be_read_from_the_environment() {
        let c = Context::from_env();
        // The values depend on the computer; they are never empty strings.
        assert!(c.user.as_deref().is_none_or(|u| !u.is_empty()));
        assert!(c.home.as_deref().is_none_or(|h| !h.is_empty()));
        assert_eq!(Context::new("", ""), Context::default());
    }

    #[test]
    fn unix_home_folders_and_documents_go() {
        assert_eq!(
            t("cannot open /Users/annika/Documents/Garden Plan.kad"),
            "cannot open <path>"
        );
        assert_eq!(
            t("at /home/bob/work/x.dxf: not found"),
            "at <path>: not found"
        );
        assert_eq!(t("(/var/folders/zz/abc/T/x.tmp)"), "(<path>)");
        assert_eq!(t("path=/tmp"), "path=<path>");
        assert_eq!(t("~/Desktop/plan.kad broke"), "<path>");
        // A Linux user with a dot, a space or a non-ASCII letter in the name.
        assert_eq!(t("open /home/j.o'neil/x.kad now"), "open <path>");
        assert_eq!(t("open /home/jüri õun/x.kad"), "open <path>");
    }

    #[test]
    fn windows_paths_go() {
        assert_eq!(
            t(r"failed C:\Users\Ann Lee\Documents\Plan.kad"),
            "failed <path>"
        );
        assert_eq!(t(r"D:/data/x.ifc: bad"), "<path>: bad");
        assert_eq!(
            t(r"opened \\fileserver\projects\kadakas\x.kad"),
            "opened <path>"
        );
        assert_eq!(t(r"quoted 'C:\Users\a\b c.kad' end"), "quoted '<path>' end");
        assert_eq!(t(r"C:\Users\ann\a.rs:12:5"), "<path>:12:5");
    }

    #[test]
    fn macos_places_go() {
        assert_eq!(t("/Volumes/Backup Disk/Projects/x.kad"), "<path>");
        assert_eq!(
            t("/Users/annika/Library/Application Support/kadakas/x"),
            "<path>"
        );
        assert_eq!(
            t("file:///Users/annika/x.kad failed"),
            "file://<path> failed"
        );
    }

    #[test]
    fn plain_text_with_slashes_stays() {
        assert_eq!(t("a/b and 1/2 and and/or"), "a/b and 1/2 and and/or");
        assert_eq!(t("x / y"), "x / y");
        assert_eq!(
            t("index out of bounds: the len is 3 but the index is 7"),
            "index out of bounds: the len is 3 but the index is 7"
        );
        assert_eq!(
            t("called `Option::unwrap()` on a `None` value"),
            "called `Option::unwrap()` on a `None` value"
        );
    }

    #[test]
    fn urls_keep_the_scheme_and_only_the_official_host() {
        assert_eq!(
            t("get https://tiles.example.com/v1/12/3/4.png?key=abc123def456 failed"),
            "get https://<host>/<url-path> failed"
        );
        assert_eq!(
            t("https://user:s3cret@host.example:8443/x"),
            "https://<host>/<url-path>"
        );
        assert_eq!(t("see http://localhost:8080/"), "see http://<host>");
        assert_eq!(t("https://a.b/c#frag"), "https://<host>/<url-path>");
        assert_eq!(
            t("wss://collab.example.org/room/abc?token=zzz"),
            "wss://<host>/<url-path>"
        );
        assert_eq!(t("(https://x.y/z?a=b)"), "(https://<host>/<url-path>)");
        assert_eq!(t("bare :// is not a url"), "bare :// is not a url");
        // The official servers keep their name (the feedback server and its relatives).
        assert_eq!(
            t("POST https://feedback.kadakas.invalid/v1/feedback failed"),
            "POST https://feedback.kadakas.invalid/<url-path> failed"
        );
        assert_eq!(
            t("https://KADAKAS.invalid:8443"),
            "https://KADAKAS.invalid:8443"
        );
        // A name that only looks alike does not.
        assert_eq!(
            t("https://evil-kadakas.invalid/x"),
            "https://<host>/<url-path>"
        );
        assert_eq!(
            t("https://kadakas.invalid.example.com/x"),
            "https://<host>/<url-path>"
        );
        assert_eq!(
            t("https://user:pw@feedback.kadakas.invalid/x"),
            "https://feedback.kadakas.invalid/<url-path>"
        );
    }

    #[test]
    fn file_names_with_spaces_and_no_quotes_go_whole() {
        for (input, want) in [
            ("opening My Garden Plan.kad failed", "<file> failed"),
            ("could not read Smith house survey.DXF now", "<file> now"),
            ("error: Oak Row.json: not found", "error: <file>: not found"),
            ("saved Plan v2.final.kad.", "<file>."),
            ("a, My Plan.pdf, b", "a, <file>, b"),
            ("lines.txt and Other Plan.kad", "<file> <file>"),
        ] {
            let out = t(input);
            assert_eq!(out, want, "{input}");
        }
        // Words that only have a dot stay.
        assert_eq!(
            t("version 0.4.0 of kadakas.app"),
            "version 0.4.0 of kadakas.app"
        );
        assert_eq!(t("e.g. a note. The end"), "e.g. a note. The end");
        assert_eq!(t("file .kad alone"), "file .kad alone");
        assert_eq!(t("data.kadx stays"), "data.kadx stays");
    }

    #[test]
    fn the_home_folder_comes_from_userprofile_without_home() {
        let env = |k: &str| match k {
            "USERNAME" => Some("Annika".to_string()),
            "USERPROFILE" => Some(r"C:\Users\Annika".to_string()),
            _ => None,
        };
        let c = Context::from_lookup(&env);
        assert_eq!(c.user.as_deref(), Some("Annika"));
        assert_eq!(c.home.as_deref(), Some(r"C:\Users\Annika"));
        let out = text(r"profile is C:\Users\Annika and Annika", &c);
        assert!(!out.to_lowercase().contains("annika"), "{out}");
        // Both HOME and USERPROFILE (a Git Bash on Windows), and a drive with a path.
        let env2 = |k: &str| match k {
            "HOME" => Some("/c/Users/Bob".to_string()),
            "USERPROFILE" => Some(r"C:\Users\Bob".to_string()),
            "HOMEDRIVE" => Some("D:".to_string()),
            "HOMEPATH" => Some(r"\Users\Bob".to_string()),
            _ => None,
        };
        let c = Context::from_lookup(&env2);
        assert_eq!(c.more_homes.len(), 2);
        let out = text(r"x D:\Users\Bob / C:\Users\Bob", &c);
        assert!(!out.contains("Bob"), "{out}");
        assert_eq!(Context::from_lookup(&|_| None), Context::default());
    }

    #[test]
    fn unc_paths_and_homes_go() {
        let c = Context::new("ann", r"\\fileserver\home\ann");
        for s in [
            r"open \\fileserver\home\ann\Plans\My Garden.kad now",
            r"home is \\fileserver\home\ann.",
            r"at \\nas\share\Smith Job\x.dxf",
        ] {
            let out = text(s, &c);
            for leak in ["fileserver", "ann", "Garden", "Smith", "share", "nas"] {
                assert!(!out.contains(leak), "{leak} in {out} (from {s})");
            }
        }
    }

    #[test]
    fn tokens_and_secrets_go() {
        for s in [
            "Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123456789",
            "token=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            r#"{"password": "hunter2hunter2"}"#,
            "api_key=AKIAIOSFODNN7EXAMPLE",
            "cookie: session=abc123",
        ] {
            let out = t(s);
            for secret in ["abcdefghijklmnop", "ghp_", "hunter2", "AKIA", "abc123"] {
                assert!(!out.contains(secret), "{s} -> {out}");
            }
        }
    }

    #[test]
    fn urls_with_tokens_in_the_query_lose_them() {
        let out = t("GET https://api.example.com/x?access_token=SECRETVALUE&a=b done");
        assert!(
            !out.contains("SECRETVALUE") && !out.contains("access_token"),
            "{out}"
        );
        assert!(
            out.contains("<host>") && !out.contains("api.example.com"),
            "{out}"
        );
    }

    #[test]
    fn email_addresses_go() {
        for s in [
            "contact ann.lee+kadakas@example.co.uk now",
            "ANN@EXAMPLE.COM",
            "a@b.io,c@d.io",
            "mailto:someone@host.example",
        ] {
            let out = t(s);
            assert!(
                !out.contains('@') || out.contains("<email>"),
                "{s} -> {out}"
            );
            assert!(
                !out.contains("example") || out.contains("<email>"),
                "{s} -> {out}"
            );
        }
        assert!(t("ann@example.com wrote").contains("<email>"));
    }

    #[test]
    fn the_user_name_goes_in_any_case_and_only_as_a_word() {
        assert_eq!(t("hello Annika and ANNIKA"), "hello <user> and <user>");
        assert_eq!(t("annikas house"), "annikas house");
        assert_eq!(t("user_annika"), "user_annika");
        // Short names are left alone: they would cut ordinary words.
        let c = Context::new("al", "");
        assert_eq!(text("al and alice", &c), "al and alice");
        // No context: nothing to replace.
        assert_eq!(text("annika", &Context::default()), "annika");
    }

    #[test]
    fn the_home_folder_goes_when_it_stands_alone() {
        let c = Context::new("zed", "C:\\Users\\zed");
        let out = text("home is C:\\Users\\zed", &c);
        assert!(!out.contains("zed"), "{out}");
        let c = Context::new("", "/Users/annika/");
        assert!(!text("HOME=/Users/annika", &c).contains("annika"));
    }

    #[test]
    fn names_in_quotes_and_file_names_go() {
        let out = t("no layer 'Oak trees north' in Garden.kad");
        assert!(!out.contains("Oak") && !out.contains("Garden"), "{out}");
        let out = t("Err(Parse(\"bad object \\\"Smith house\\\"\"))");
        assert_eq!(out, "Err(Parse(\"<text>\"))");
        let out = t("read plan.dxf failed");
        assert!(!out.contains("plan.dxf"), "{out}");
    }

    #[test]
    fn control_characters_and_huge_input_are_handled() {
        assert_eq!(t("a\u{0}b\u{7}c"), "abc");
        let big = "x ".repeat(MAX_INPUT);
        assert!(text(&big, &ctx()).len() <= MAX_INPUT + 8);
        // A path that never ends does not take quadratic time.
        let long = format!("/a/{}", "b/".repeat(40_000));
        assert_eq!(t(&long), "<path>");
    }

    #[test]
    fn source_paths_of_kadakas_are_kept_relative() {
        assert_eq!(
            source_path("/Users/ci/work/kadakas/crates/kadakas-core/src/geom.rs"),
            "crates/kadakas-core/src/geom.rs"
        );
        assert_eq!(
            source_path(r"C:\Users\ann\kadakas\crates\kadakas-app\src\app.rs"),
            "crates/kadakas-app/src/app.rs"
        );
        assert_eq!(
            source_path("./crates/kadakas-api/src/lib.rs"),
            "crates/kadakas-api/src/lib.rs"
        );
        assert_eq!(
            source_path("crates/kadakas-api/src/lib.rs"),
            "crates/kadakas-api/src/lib.rs"
        );
        // A folder with strange characters does not get through.
        assert_eq!(
            source_path("/h/crates/kadakas-core/src/Ann Lee/x.rs"),
            "crates/kadakas-core/src/<x>/x.rs"
        );
    }

    #[test]
    fn source_paths_of_dependencies_and_the_compiler() {
        assert_eq!(
            source_path(
                "/Users/annika/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serde-1.0.200/src/de/mod.rs"
            ),
            "registry/serde-1.0.200/src/de/mod.rs"
        );
        assert_eq!(
            source_path(
                r"C:\Users\annika\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\egui-0.35.0\src\lib.rs"
            ),
            "registry/egui-0.35.0/src/lib.rs"
        );
        assert_eq!(
            source_path(
                "/rustc/0123456789abcdef0123456789abcdef01234567/library/core/src/option.rs"
            ),
            "rustc/library/core/src/option.rs"
        );
        assert_eq!(
            source_path("/home/bob/.cargo/git/checkouts/foo-1a2b3c4d5e6f7a8b/abc1234/src/lib.rs"),
            "git/foo/src/lib.rs"
        );
    }

    #[test]
    fn other_source_paths_lose_folder_and_name() {
        assert_eq!(
            source_path("/Users/annika/Documents/Secret Project/script.rs"),
            "<path>/<file>.rs"
        );
        assert_eq!(source_path("/Users/annika/Garden.kad"), "<path>/<file>.kad");
        assert_eq!(source_path("/Users/annika/noext"), "<path>");
        assert_eq!(source_path(""), "<path>");
        // A fake registry path does not fool the rules.
        assert_eq!(
            source_path("/Users/annika/registry/src/x/Secret Plan/a.rs"),
            "<path>/<file>.rs"
        );
    }

    #[test]
    fn frames_lose_hashes_and_private_files() {
        let f = Frame {
            function: "kadakas_core::run::h0123456789abcdef".into(),
            file: Some("/Users/annika/proj/x.rs".into()),
            line: Some(7),
        };
        let s = frame(&f);
        assert_eq!(s.function, "kadakas_core::run");
        assert_eq!(s.file.as_deref(), Some("<path>/<file>.rs"));
        assert_eq!(s.line, Some(7));
        assert_eq!(function("a\u{1}b"), "ab");
        assert_eq!(function(&"x".repeat(500)).len(), 200);
        assert_eq!(strip_hash("f::hZZZ"), "f::hZZZ");
    }

    #[test]
    fn nothing_personal_survives_a_mixed_message() {
        let msg = "thread 'main' panicked: failed to load C:\\Users\\Annika Tamm\\Documents\\Oak Garden.kad for annika@example.com via https://user:pw@files.example.com/p/x?token=abcDEF123456 (owner Annika, /Users/annika/.config/kadakas/prefs.json)";
        let out = text(msg, &Context::new("annika", "/Users/annika"));
        for needle in [
            "Annika",
            "annika",
            "Oak",
            "Garden",
            "example.com/",
            "pw@",
            "token",
            "abcDEF",
            "prefs.json",
            "Documents",
        ] {
            assert!(!out.contains(needle), "{needle} in {out}");
        }
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn random_paths_and_user_names_never_leak(
            // "zq" never occurs in the text around the path, nor in the placeholders.
            user in "zq[a-z]{3,7}",
            folders in proptest::collection::vec("Qz[A-Za-z0-9 ._-]{3,10}", 1..5),
            stem in "Qz[A-Za-z0-9 _-]{3,12}",
            style in 0usize..4,
        ) {
            let ctx = Context::new(&user, "");
            let dir = folders.join(if style == 1 { "\\" } else { "/" });
            let path = match style {
                0 => format!("/Users/{user}/{dir}/{stem}.kad"),
                1 => format!("C:\\Users\\{user}\\{dir}\\{stem}.kad"),
                2 => format!("/home/{user}/{dir}/{stem}.dxf"),
                _ => format!("\\\\server\\{user}\\{dir}\\{stem}.ifc"),
            };
            let msg = format!("failed to open {path} for {user}");
            let out = text(&msg, &ctx);
            prop_assert!(!out.to_lowercase().contains(&user), "{out} from {msg}");
            prop_assert!(!out.contains(stem.trim()) || stem.trim().len() < 2 || "<path><user>".contains(stem.trim()), "{out} from {msg}");
            prop_assert!(!out.contains(".kad") && !out.contains(".dxf") && !out.contains(".ifc"), "{out}");
            for f in folders.iter().map(|f| f.trim()).filter(|f| f.len() > 3) {
                prop_assert!(!out.contains(f) || "<path><user><file>".contains(f), "{f} in {out}");
            }
        }

        #[test]
        fn scrubbing_never_panics_and_is_idempotent_enough(s in "\\PC{0,200}") {
            let c = ctx();
            let once = text(&s, &c);
            // The output holds no path start that the input had: running again changes nothing
            // about placeholders, and never panics.
            let twice = text(&once, &c);
            prop_assert!(twice.len() <= once.len() + 16);
        }

        #[test]
        fn a_generated_home_path_never_leaks(name in "[A-Za-z][A-Za-z0-9_ .-]{2,12}", file in "[A-Za-z0-9 _-]{1,12}") {
            let s = format!("open /Users/{name}/Documents/{file}.kad failed");
            let out = text(&s, &Context::default());
            prop_assert!(!out.contains("Documents"));
            prop_assert!(!out.contains(".kad"));
            prop_assert!(out.contains("<path>"));
        }
    }
}
