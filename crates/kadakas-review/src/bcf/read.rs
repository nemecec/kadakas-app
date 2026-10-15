//! Reading a BCF archive that nobody vouches for. See the module text of `bcf` for the limits.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{Cursor, Read};

use kadakas_core::change::Origin;
use kadakas_core::issue::{
    Comment, CommentId, Issue, IssueId, IssueStatus, MAX_COMMENTS, MAX_ISSUES, MAX_SNAPSHOT_BYTES,
    MAX_SNAPSHOT_PIXELS, MAX_VIEWPOINTS, ViewSpec, Viewpoint, ViewpointId,
};
use kadakas_core::model::ObjectId;
use kadakas_core::picture::{PictureFormat, jpeg_size, png_size, sha256_hex};
use kadakas_core::uid::Uid;
use roxmltree::{Document, Node, ParsingOptions};

use super::camera::{self, BcfCamera};
use super::write::generated_viewpoint_id;
use super::{BcfError, Imported, ImportedTopic, Picture, Skipped, Unfit, Version};
use crate::issues::{match_ifc, time};

/// The limits of the reader. A violation is an error, and nothing is imported.
#[derive(Clone, Debug)]
pub struct ReadLimits {
    /// The most entries in the archive.
    pub max_entries: usize,
    /// The largest entry, declared and real, in bytes.
    pub max_entry_bytes: u64,
    /// The largest XML file, in bytes.
    pub max_xml_bytes: u64,
    /// All entries together, declared and real, in bytes.
    pub max_total_bytes: u64,
    /// The largest ratio of the size of an entry to its compressed size (for entries over 64 KiB).
    pub max_ratio: u64,
    /// The longest name of an entry, in bytes.
    pub max_name_bytes: usize,
    /// The most nodes in an XML file.
    pub max_xml_nodes: u32,
    /// The most topics.
    pub max_topics: usize,
    /// The largest snapshot file, in bytes.
    pub max_picture_bytes: usize,
    /// The most pixels in a snapshot.
    pub max_picture_pixels: u64,
}

impl Default for ReadLimits {
    fn default() -> Self {
        ReadLimits {
            max_entries: 50_000,
            max_entry_bytes: 32 * 1024 * 1024,
            max_xml_bytes: 4 * 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
            max_ratio: 200,
            max_name_bytes: 256,
            max_xml_nodes: 200_000,
            max_topics: MAX_ISSUES,
            max_picture_bytes: MAX_SNAPSHOT_BYTES,
            max_picture_pixels: MAX_SNAPSHOT_PIXELS,
        }
    }
}

fn limit(why: impl Into<String>) -> BcfError {
    BcfError::Limit(why.into())
}

fn format_err(why: impl Into<String>) -> BcfError {
    BcfError::Format(why.into())
}

/// A name inside the archive in a safe form (slashes only), or why it is not safe.
fn safe_name(raw: &str, limits: &ReadLimits) -> Result<String, BcfError> {
    if raw.len() > limits.max_name_bytes {
        return Err(limit(format!(
            "an entry name is longer than {} bytes",
            limits.max_name_bytes
        )));
    }
    // Some writers use backslashes. They are separators here and nothing else.
    let name = raw.replace('\\', "/");
    let bad = name.is_empty()
        || name.contains('\0')
        || name.starts_with('/')
        || name.chars().any(|c| c.is_control())
        || name
            .split('/')
            .any(|p| p == ".." || (p.len() == 2 && p.ends_with(':')))
        || name.split('/').next().is_some_and(|p| p.contains(':'));
    if bad {
        return Err(format_err(format!(
            "the entry name {:?} is not allowed",
            name.chars().take(80).collect::<String>()
        )));
    }
    Ok(name)
}

struct Entries<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    /// Safe name to index, for files.
    index: BTreeMap<String, usize>,
    read_bytes: u64,
    limits: &'a ReadLimits,
}

impl<'a> Entries<'a> {
    fn open(bytes: &'a [u8], limits: &'a ReadLimits) -> Result<Entries<'a>, BcfError> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| BcfError::Archive(e.to_string()))?;
        if zip.len() > limits.max_entries {
            return Err(limit(format!(
                "the archive has {} entries (at most {})",
                zip.len(),
                limits.max_entries
            )));
        }
        let mut index = BTreeMap::new();
        let mut declared: u64 = 0;
        for i in 0..zip.len() {
            let f = zip
                .by_index_raw(i)
                .map_err(|e| BcfError::Archive(e.to_string()))?;
            let name = safe_name(f.name(), limits)?;
            if f.is_dir() || name.ends_with('/') {
                continue;
            }
            if f.encrypted() {
                return Err(format_err("the archive has an encrypted entry"));
            }
            if f.is_symlink() {
                return Err(format_err("the archive has a symbolic link"));
            }
            let (size, packed) = (f.size(), f.compressed_size());
            if size > limits.max_entry_bytes {
                return Err(limit(format!(
                    "an entry is {size} bytes (at most {})",
                    limits.max_entry_bytes
                )));
            }
            if size > 64 * 1024 && size / packed.max(1) > limits.max_ratio {
                return Err(limit(format!(
                    "an entry is compressed more than {} to 1",
                    limits.max_ratio
                )));
            }
            declared = declared.saturating_add(size);
            if declared > limits.max_total_bytes {
                return Err(limit(format!(
                    "the entries are more than {} bytes together",
                    limits.max_total_bytes
                )));
            }
            if index.insert(name.clone(), i).is_some() {
                return Err(format_err(format!("the name {name:?} is there twice")));
            }
        }
        Ok(Entries {
            zip,
            index,
            read_bytes: 0,
            limits,
        })
    }

    fn has(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// The bytes of an entry, at most `cap` of them (the real size is checked, not the declared).
    fn read(&mut self, name: &str, cap: u64) -> Result<Option<Vec<u8>>, BcfError> {
        let Some(&i) = self.index.get(name) else {
            return Ok(None);
        };
        let f = self
            .zip
            .by_index(i)
            .map_err(|e| BcfError::Archive(e.to_string()))?;
        let mut out = Vec::new();
        f.take(cap + 1)
            .read_to_end(&mut out)
            .map_err(|e| BcfError::Archive(format!("{name}: {e}")))?;
        if out.len() as u64 > cap {
            return Err(limit(format!("{name} is larger than {cap} bytes")));
        }
        self.read_bytes += out.len() as u64;
        if self.read_bytes > self.limits.max_total_bytes {
            return Err(limit(format!(
                "the entries are more than {} bytes together",
                self.limits.max_total_bytes
            )));
        }
        Ok(Some(out))
    }

    fn read_xml(&mut self, name: &str) -> Result<Option<String>, BcfError> {
        let cap = self.limits.max_xml_bytes;
        let Some(bytes) = self.read(name, cap)? else {
            return Ok(None);
        };
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| format_err(format!("{name} is not UTF-8 text")))
    }
}

/// The deepest nesting of elements that we read. BCF files nest to about 8 levels.
const MAX_XML_DEPTH: usize = 40;

/// Looks at the nesting of the elements without building anything, so that a file that nests
/// thousands of levels is refused before the parser (which calls itself for each level) runs out
/// of stack. It reads tags like an XML parser: comments, CDATA, instructions and quoted attribute
/// values are skipped. A text that is not well formed gives a wrong count at worst, and the
/// parser then refuses it.
fn check_depth(text: &str) -> Result<(), String> {
    let b = text.as_bytes();
    let (mut i, mut depth) = (0usize, 0usize);
    let find = |from: usize, pat: &[u8]| -> usize {
        if from >= b.len() {
            return b.len();
        }
        b[from..]
            .windows(pat.len())
            .position(|w| w == pat)
            .map_or(b.len(), |p| from + p + pat.len())
    };
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &b[i + 1..];
        if rest.starts_with(b"!--") {
            i = find(i + 4, b"-->");
        } else if rest.starts_with(b"![CDATA[") {
            i = find(i + 9, b"]]>");
        } else if rest.starts_with(b"?") {
            i = find(i + 2, b"?>");
        } else if rest.starts_with(b"!") {
            return Err("a DTD is not allowed".into());
        } else if rest.starts_with(b"/") {
            depth = depth.saturating_sub(1);
            i = find(i + 2, b">");
        } else {
            depth += 1;
            if depth > MAX_XML_DEPTH {
                return Err(format!(
                    "the elements are nested more than {MAX_XML_DEPTH} levels"
                ));
            }
            // To the end of the tag, outside of quotes.
            let mut quote = 0u8;
            i += 1;
            while i < b.len() {
                let c = b[i];
                if quote != 0 {
                    if c == quote {
                        quote = 0;
                    }
                } else if c == b'"' || c == b'\'' {
                    quote = c;
                } else if c == b'>' {
                    if b[i - 1] == b'/' {
                        depth -= 1;
                    }
                    break;
                }
                i += 1;
            }
            i += 1;
        }
    }
    Ok(())
}

fn parse<'a>(text: &'a str, limits: &ReadLimits) -> Result<Document<'a>, String> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    check_depth(text)?;
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: limits.max_xml_nodes,
            ..ParsingOptions::default()
        },
    )
    .map_err(|e| e.to_string())
}

fn kids<'a, 'i>(n: Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name)
}

fn kid<'a, 'i>(n: Node<'a, 'i>, name: &'static str) -> Option<Node<'a, 'i>> {
    kids(n, name).next()
}

fn text_of(n: Node<'_, '_>) -> Option<String> {
    n.text().map(str::to_string)
}

/// The text of a field as it is, except that the line breaks and the spaces around a text that a
/// pretty printer put in are cut. A text of our own with spaces at its ends comes back the same.
fn plain(n: Option<Node<'_, '_>>) -> Option<String> {
    let t = n?.text()?;
    let t = if t.contains('\n') { t.trim() } else { t };
    (!t.trim().is_empty()).then(|| t.to_string())
}

fn trimmed(n: Option<Node<'_, '_>>) -> Option<String> {
    let t = n?.text()?.trim().to_string();
    (!t.is_empty()).then_some(t)
}

fn number(n: Option<Node<'_, '_>>) -> Option<f64> {
    trimmed(n)?.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn point(n: Option<Node<'_, '_>>) -> Option<[f64; 3]> {
    let n = n?;
    Some([
        number(kid(n, "X"))?,
        number(kid(n, "Y"))?,
        number(kid(n, "Z"))?,
    ])
}

fn guid_attr(n: Node<'_, '_>) -> Option<Uid> {
    n.attribute("Guid").and_then(|g| Uid::parse(g.trim()))
}

/// Reads a status text of any BCF writer. Unknown words are open, and the caller keeps the word.
fn status_of(text: &str) -> Option<IssueStatus> {
    let t: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    match t.as_str() {
        "open" | "active" | "new" | "assigned" | "reopened" | "todo" => Some(IssueStatus::Open),
        "inprogress" | "started" | "doing" => Some(IssueStatus::InProgress),
        "resolved" | "fixed" | "done" | "completed" => Some(IssueStatus::Resolved),
        "closed" => Some(IssueStatus::Closed),
        _ => None,
    }
}

/// What the topics of one archive need while they are read.
struct Ctx<'a> {
    entries: Entries<'a>,
    warnings: Vec<String>,
    unfit: Vec<Unfit>,
    now_ms: i64,
    /// Counts of things that are not read, for one summary line each.
    unsupported: BTreeMap<&'static str, usize>,
}

impl Ctx<'_> {
    fn when(&mut self, text: Option<String>, what: &str) -> i64 {
        match text {
            Some(t) => match time::parse_iso(&t) {
                Some(ms) if (0..=253_402_300_799_000).contains(&ms) => ms,
                _ => {
                    self.warnings.push(format!(
                        "{what}: the date {t:?} is not valid: the time of the import is used"
                    ));
                    self.now_ms
                }
            },
            None => self.now_ms,
        }
    }

    fn unsupported(&mut self, what: &'static str) {
        *self.unsupported.entry(what).or_default() += 1;
    }
}

/// Reads a BCF archive. `now_ms` is the time that stands for a date that is missing or not valid.
pub fn read(bytes: &[u8], limits: &ReadLimits, now_ms: i64) -> Result<Imported, BcfError> {
    let mut entries = Entries::open(bytes, limits)?;
    let version = {
        let text = entries
            .read_xml("bcf.version")?
            .ok_or_else(|| format_err("bcf.version is missing: this is not a BCF file"))?;
        let doc = parse(&text, limits).map_err(|e| format_err(format!("bcf.version: {e}")))?;
        let id = doc
            .root_element()
            .attribute("VersionId")
            .or_else(|| kid(doc.root_element(), "DetailedVersion").and_then(|d| d.text()))
            .ok_or_else(|| format_err("bcf.version has no VersionId"))?
            .trim()
            .to_string();
        Version::parse(&id).ok_or(BcfError::Version(id))?
    };
    // The folders of the topics: the top-level folders that hold a markup.bcf.
    let folders: Vec<String> = entries
        .index
        .keys()
        .filter_map(|n| {
            let (folder, file) = n.split_once('/')?;
            (file == "markup.bcf" && !folder.is_empty()).then(|| folder.to_string())
        })
        .collect();
    if folders.len() > limits.max_topics {
        return Err(limit(format!(
            "the archive has {} topics (at most {})",
            folders.len(),
            limits.max_topics
        )));
    }
    let mut cx = Ctx {
        entries,
        warnings: Vec::new(),
        unfit: Vec::new(),
        now_ms,
        unsupported: BTreeMap::new(),
    };
    let mut topics: Vec<ImportedTopic> = Vec::new();
    let mut skipped = Vec::new();
    let mut seen: HashSet<IssueId> = HashSet::new();
    for folder in &folders {
        let mark = cx.unfit.len();
        match read_topic(&mut cx, version, folder) {
            Ok(Some(t)) => {
                if !seen.insert(t.issue.id) {
                    cx.unfit.truncate(mark);
                    skipped.push(Skipped {
                        topic: folder.clone(),
                        reason: format!("the GUID {} is used by an earlier topic", t.issue.id),
                    });
                } else {
                    topics.push(t);
                }
            }
            Ok(None) => {}
            Err(Fail::Skip(reason)) => {
                cx.unfit.truncate(mark);
                skipped.push(Skipped {
                    topic: folder.clone(),
                    reason,
                });
            }
            Err(Fail::Hard(e)) => return Err(e),
        }
    }
    let mut warnings = std::mem::take(&mut cx.warnings);
    for (what, n) in &cx.unsupported {
        warnings.push(format!("{n} {what}: not imported"));
    }
    Ok(Imported {
        version,
        topics,
        skipped,
        unfit: std::mem::take(&mut cx.unfit),
        warnings,
    })
}

enum Fail {
    /// This topic cannot be imported. The others can.
    Skip(String),
    /// The archive cannot be read.
    Hard(BcfError),
}

impl From<BcfError> for Fail {
    fn from(e: BcfError) -> Fail {
        Fail::Hard(e)
    }
}

fn skip(why: impl Into<String>) -> Fail {
    Fail::Skip(why.into())
}

/// One viewpoint entry of a markup file.
struct VpRef {
    guid: Option<Uid>,
    file: Option<String>,
    snapshot: Option<String>,
}

fn read_topic(
    cx: &mut Ctx<'_>,
    version: Version,
    folder: &str,
) -> Result<Option<ImportedTopic>, Fail> {
    let path = format!("{folder}/markup.bcf");
    let text = cx
        .entries
        .read_xml(&path)?
        .ok_or_else(|| skip("markup.bcf is missing"))?;
    let doc = parse(&text, cx.entries.limits).map_err(|e| skip(format!("markup.bcf: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "Markup" {
        return Err(skip("markup.bcf has no Markup element"));
    }
    let topic = kid(root, "Topic").ok_or_else(|| skip("markup.bcf has no Topic element"))?;
    let id = guid_attr(topic)
        .map(IssueId)
        .ok_or_else(|| skip("the topic GUID is missing or not valid"))?;
    let title = plain(kid(topic, "Title")).ok_or_else(|| skip("the topic has no title"))?;
    let mut issue = Issue {
        id,
        title,
        ..Issue::default()
    };
    if let Some(t) = topic
        .attribute("TopicType")
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        issue.kind = t.to_string();
    }
    if let Some(t) = plain(kid(topic, "Priority")) {
        issue.priority = t;
    }
    if let Some(s) = topic
        .attribute("TopicStatus")
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        match status_of(s) {
            Some(st) => issue.status = st,
            None => {
                // The word is kept as a label, so it is not lost.
                let label: String = format!("bcf-status:{s}")
                    .chars()
                    .take(kadakas_core::issue::MAX_LABEL_CHARS)
                    .collect();
                issue.labels.push(label);
                cx.warnings.push(format!(
                    "topic {id}: the status {s:?} is not known: the issue is open, and the label \"bcf-status:{s}\" keeps the word"
                ));
            }
        }
    }
    // Labels: BCF 3.0 has `Labels/Label`, BCF 2.1 has several `Labels` with the text.
    for l in kids(topic, "Labels") {
        let inner: Vec<_> = kids(l, "Label").collect();
        let texts: Vec<Option<String>> = if inner.is_empty() {
            vec![plain(Some(l))]
        } else {
            inner.into_iter().map(|n| plain(Some(n))).collect()
        };
        for t in texts.into_iter().flatten() {
            if !issue
                .labels
                .iter()
                .any(|x| x.to_lowercase() == t.to_lowercase())
            {
                issue.labels.push(t);
            }
        }
    }
    issue.author = plain(kid(topic, "CreationAuthor")).unwrap_or_default();
    issue.created = cx.when(trimmed(kid(topic, "CreationDate")), &format!("topic {id}"));
    issue.modified = kid(topic, "ModifiedDate")
        .and_then(|n| trimmed(Some(n)))
        .map(|t| cx.when(Some(t), &format!("topic {id}")))
        .unwrap_or(issue.created)
        .max(issue.created);
    if let Some(d) = trimmed(kid(topic, "DueDate")) {
        match time::parse_iso(&d) {
            Some(ms) if (0..=253_402_300_799_000).contains(&ms) => issue.due = Some(time::date(ms)),
            _ => cx.warnings.push(format!(
                "topic {id}: the due date {d:?} is not valid and is left out"
            )),
        }
    }
    issue.assignee = plain(kid(topic, "AssignedTo"));
    issue.description = kid(topic, "Description")
        .and_then(text_of)
        .unwrap_or_default();
    if issue.description.trim().is_empty() {
        issue.description.clear();
    }
    for (name, what) in [
        ("BimSnippet", "BIM snippets"),
        ("DocumentReferences", "topics with document references"),
        ("DocumentReference", "topics with document references"),
        ("RelatedTopics", "topics with related topics"),
        ("RelatedTopic", "topics with related topics"),
        ("ReferenceLinks", "topics with reference links"),
        ("ReferenceLink", "topics with reference links"),
    ] {
        if kid(topic, name).is_some() || (version == Version::V21 && kid(root, name).is_some()) {
            cx.unsupported(what);
        }
    }

    // Comments: inside the topic in 3.0, next to it in 2.1.
    let comment_nodes: Vec<Node<'_, '_>> = match version {
        Version::V30 => kid(topic, "Comments")
            .map(|c| kids(c, "Comment").collect())
            .unwrap_or_default(),
        Version::V21 => kids(root, "Comment").collect(),
    };
    if comment_nodes.len() > MAX_COMMENTS {
        return Err(skip(format!(
            "the topic has {} comments (at most {MAX_COMMENTS})",
            comment_nodes.len()
        )));
    }
    // Viewpoint entries: inside the topic in 3.0 (`Viewpoints/ViewPoint`), next to it in 2.1
    // (`Viewpoints`, once for each viewpoint).
    let vp_nodes: Vec<Node<'_, '_>> = match version {
        Version::V30 => kid(topic, "Viewpoints")
            .map(|c| kids(c, "ViewPoint").collect())
            .unwrap_or_default(),
        Version::V21 => kids(root, "Viewpoints").collect(),
    };
    if vp_nodes.len() > MAX_VIEWPOINTS {
        return Err(skip(format!(
            "the topic has {} viewpoints (at most {MAX_VIEWPOINTS})",
            vp_nodes.len()
        )));
    }
    let mut refs: Vec<VpRef> = vp_nodes
        .iter()
        .map(|n| VpRef {
            guid: guid_attr(*n),
            file: trimmed(kid(*n, "Viewpoint")),
            snapshot: trimmed(kid(*n, "Snapshot")),
        })
        .collect();
    // Old files: no list, but the files are in the folder.
    if refs.is_empty() && cx.entries.has(&format!("{folder}/viewpoint.bcfv")) {
        refs.push(VpRef {
            guid: None,
            file: Some("viewpoint.bcfv".into()),
            snapshot: cx
                .entries
                .has(&format!("{folder}/snapshot.png"))
                .then(|| "snapshot.png".into()),
        });
    }
    let mut pictures: Vec<Picture> = Vec::new();
    let mut taken: HashSet<Uid> = HashSet::new();
    let mut objects: BTreeSet<ObjectId> = BTreeSet::new();
    let generated = generated_viewpoint_id(id);
    let mut vps: Vec<Viewpoint> = Vec::new();
    for (n, r) in refs.iter().enumerate() {
        let mut vp = Viewpoint::default();
        let mut vid = r.guid.unwrap_or(Uid::NIL);
        if vid == Uid::NIL || !taken.insert(vid) {
            vid = Uid::derive(id.0, 2, n as u64);
            taken.insert(vid);
        }
        vp.id = ViewpointId(vid);
        vp.time = issue.created + n as i64;
        let file = r.file.clone().unwrap_or_else(|| "viewpoint.bcfv".into());
        read_viewpoint_file(cx, folder, &file, &mut vp, &mut objects)?;
        if let Some(s) = &r.snapshot {
            vp.snapshot = read_snapshot(cx, folder, s, vp.id, &mut pictures)?;
        }
        vps.push(vp);
    }
    // A viewpoint that an export made for the objects goes back to the objects.
    vps.retain(|v| v.id.0 != generated);
    issue.objects = objects.into_iter().collect();
    issue.viewpoints = vps;

    let mut comments: Vec<Comment> = Vec::new();
    let mut ctaken: HashSet<Uid> = HashSet::new();
    for (n, c) in comment_nodes.iter().enumerate() {
        let Some(text) = kid(*c, "Comment")
            .and_then(text_of)
            .filter(|t| !t.trim().is_empty())
        else {
            cx.unfit.push(Unfit {
                topic: folder.to_string(),
                item: "comment".into(),
                reason: "the comment has no text (Kadakas comments need a text)".into(),
                viewpoint: None,
            });
            continue;
        };
        let mut cid = guid_attr(*c).unwrap_or(Uid::NIL);
        if cid == Uid::NIL || !ctaken.insert(cid) {
            cid = Uid::derive(id.0, 3, n as u64);
            ctaken.insert(cid);
        }
        let viewpoint = c
            .children()
            .find(|k| k.is_element() && k.tag_name().name() == "Viewpoint")
            .and_then(guid_attr)
            .map(ViewpointId)
            .filter(|v| issue.viewpoints.iter().any(|x| x.id == *v));
        let edited = kid(*c, "ModifiedDate")
            .and_then(|m| trimmed(Some(m)))
            .map(|t| cx.when(Some(t), &format!("a comment of topic {id}")));
        let comment = Comment {
            id: CommentId(cid),
            issue: id,
            author: plain(kid(*c, "Author")).unwrap_or_default(),
            origin: Origin::User,
            time: cx.when(
                trimmed(kid(*c, "Date")),
                &format!("a comment of topic {id}"),
            ),
            text,
            viewpoint,
            edited,
        };
        match comment.validate() {
            Ok(()) => comments.push(comment),
            Err(why) => cx.unfit.push(Unfit {
                topic: folder.to_string(),
                item: "comment".into(),
                reason: why,
                viewpoint: None,
            }),
        }
    }
    comments.sort_by_key(|c| (c.time, c.id));
    if let Some(last) = comments.iter().map(|c| c.time).max() {
        issue.modified = issue.modified.max(last);
    }
    issue.validate().map_err(skip)?;
    Ok(Some(ImportedTopic {
        issue,
        comments,
        pictures,
    }))
}

/// Reads the `.bcfv` file of a viewpoint: the camera and the selection.
fn read_viewpoint_file(
    cx: &mut Ctx<'_>,
    folder: &str,
    file: &str,
    vp: &mut Viewpoint,
    objects: &mut BTreeSet<ObjectId>,
) -> Result<(), Fail> {
    let Ok(name) = safe_name(&format!("{folder}/{file}"), cx.entries.limits) else {
        cx.warnings.push(format!(
            "topic {folder}: the viewpoint file name {file:?} is not allowed"
        ));
        return Ok(());
    };
    let Some(text) = cx.entries.read_xml(&name)? else {
        cx.warnings.push(format!(
            "topic {folder}: the viewpoint file {file} is not in the archive"
        ));
        return Ok(());
    };
    let doc = match parse(&text, cx.entries.limits) {
        Ok(d) => d,
        Err(e) => {
            cx.warnings
                .push(format!("{name}: {e}: the viewpoint has no camera"));
            return Ok(());
        }
    };
    let root = doc.root_element();
    if root.tag_name().name() != "VisualizationInfo" {
        cx.warnings.push(format!(
            "{name}: no VisualizationInfo element: the viewpoint has no camera"
        ));
        return Ok(());
    }
    if let Some(c) = kid(root, "Components") {
        if let Some(sel) = kid(c, "Selection") {
            let mut bad = 0;
            for comp in kids(sel, "Component") {
                match comp.attribute("IfcGuid").and_then(match_ifc) {
                    Some(o) => {
                        if !vp.selection.contains(&o) {
                            vp.selection.push(o);
                        }
                        objects.insert(o);
                    }
                    None => bad += 1,
                }
            }
            if bad > 0 {
                cx.warnings.push(format!(
                    "{name}: {bad} selected component(s) have no valid IfcGuid and are left out"
                ));
            }
        }
        if kid(c, "Coloring").is_some() {
            cx.unsupported("viewpoints with coloring");
        }
        if kid(c, "Visibility").is_some_and(|v| {
            kid(v, "Exceptions").is_some() || v.attribute("DefaultVisibility") == Some("false")
        }) {
            cx.unsupported("viewpoints with hidden components");
        }
    }
    for (what, label) in [
        ("ClippingPlanes", "viewpoints with clipping planes"),
        ("Lines", "viewpoints with lines"),
        ("Bitmaps", "viewpoints with bitmaps"),
        ("Bitmap", "viewpoints with bitmaps"),
    ] {
        if kid(root, what).is_some() {
            cx.unsupported(label);
        }
    }
    let cam = if let Some(o) = kid(root, "OrthogonalCamera") {
        Some((true, o))
    } else {
        kid(root, "PerspectiveCamera").map(|p| (false, p))
    };
    if let Some((orthogonal, n)) = cam {
        let size = if orthogonal {
            number(kid(n, "ViewToWorldScale"))
        } else {
            number(kid(n, "FieldOfView"))
        };
        let parts = (
            point(kid(n, "CameraViewPoint")),
            point(kid(n, "CameraDirection")),
            point(kid(n, "CameraUpVector")),
            size,
        );
        if let (Some(eye), Some(direction), Some(up), Some(size)) = parts {
            let c = BcfCamera {
                orthogonal,
                eye,
                direction,
                up,
                size,
                aspect: number(kid(n, "AspectRatio")).unwrap_or(1.5),
            };
            match camera::from_bcf(&c) {
                Ok(v) => vp.view = Some(v),
                Err(why) => cx.unfit.push(Unfit {
                    topic: folder.to_string(),
                    item: "camera".into(),
                    reason: format!("{name}: {why}"),
                    viewpoint: Some(vp.id),
                }),
            }
            if !orthogonal && (size - camera::FOV_DEG).abs() > 0.5 {
                cx.unsupported(
                    "perspective cameras with another field of view than 45 degrees (they show with 45)",
                );
            }
        } else {
            cx.unfit.push(Unfit {
                topic: folder.to_string(),
                item: "camera".into(),
                reason: format!("{name}: the camera is not complete"),
                viewpoint: Some(vp.id),
            });
        }
    }
    if let Some(v) = &vp.view {
        let probe = Viewpoint {
            view: Some(v.clone()),
            ..Viewpoint::default()
        };
        if probe.validate().is_err() {
            vp.view = None;
            cx.unfit.push(Unfit {
                topic: folder.to_string(),
                item: "camera".into(),
                reason: format!("{name}: the camera is out of range"),
                viewpoint: Some(vp.id),
            });
        }
    }
    Ok(())
}

/// Reads a snapshot file. Returns the hash if the file is a picture within the limits.
fn read_snapshot(
    cx: &mut Ctx<'_>,
    folder: &str,
    file: &str,
    viewpoint: ViewpointId,
    pictures: &mut Vec<Picture>,
) -> Result<Option<String>, Fail> {
    let refuse = |cx: &mut Ctx<'_>, reason: String| {
        cx.unfit.push(Unfit {
            topic: folder.to_string(),
            item: "snapshot".into(),
            reason,
            viewpoint: Some(viewpoint),
        });
    };
    let Ok(name) = safe_name(&format!("{folder}/{file}"), cx.entries.limits) else {
        cx.warnings.push(format!(
            "topic {folder}: the snapshot name {file:?} is not allowed"
        ));
        return Ok(None);
    };
    let cap = cx.entries.limits.max_picture_bytes as u64;
    let Some(&at) = cx.entries.index.get(&name) else {
        cx.warnings.push(format!(
            "topic {folder}: the snapshot {file} is not in the archive"
        ));
        return Ok(None);
    };
    // The declared size is checked first, so a large file is not read at all.
    let declared = cx
        .entries
        .zip
        .by_index_raw(at)
        .map(|f| f.size())
        .map_err(|e| BcfError::Archive(e.to_string()))?;
    if declared > cap {
        refuse(
            cx,
            format!("{name}: the snapshot is {declared} bytes (at most {cap})"),
        );
        return Ok(None);
    }
    let Some(bytes) = cx.entries.read(&name, cap)? else {
        return Ok(None);
    };
    let (w, h) = match PictureFormat::sniff(&bytes) {
        Some(PictureFormat::Png) => png_size(&bytes),
        Some(PictureFormat::Jpeg) => jpeg_size(&bytes),
        None => None,
    }
    .unwrap_or((0, 0));
    if w == 0 || h == 0 {
        refuse(
            cx,
            format!("{name}: the snapshot is not a PNG or JPEG picture"),
        );
        return Ok(None);
    }
    if u64::from(w) * u64::from(h) > cx.entries.limits.max_picture_pixels {
        let at_most = cx.entries.limits.max_picture_pixels;
        refuse(
            cx,
            format!("{name}: the snapshot has {w} x {h} pixels (at most {at_most})"),
        );
        return Ok(None);
    }
    let hash = sha256_hex(&bytes);
    if !pictures.iter().any(|p| p.hash == hash) {
        pictures.push(Picture {
            hash: hash.clone(),
            bytes,
        });
    }
    Ok(Some(hash))
}

/// True if an issue in the document and an issue from a file are the same for BCF: the same text
/// fields, labels, objects, viewpoints (by ID: snapshot and camera) and comments (by ID). The
/// pin, the origin, the layer and class visibility and the time of the last change are not in BCF
/// and do not count, so an export that comes back is not a conflict.
pub fn same_content(existing: &Issue, comments: &[&Comment], imported: &ImportedTopic) -> bool {
    let b = &imported.issue;
    let lower = |v: &[String]| -> BTreeSet<String> { v.iter().map(|s| s.to_lowercase()).collect() };
    let universe = |i: &Issue| -> BTreeSet<ObjectId> {
        i.objects
            .iter()
            .chain(i.viewpoints.iter().flat_map(|v| v.selection.iter()))
            .copied()
            .collect()
    };
    let generated = generated_viewpoint_id(existing.id);
    let ex_vps: Vec<&Viewpoint> = existing
        .viewpoints
        .iter()
        .filter(|v| v.id.0 != generated)
        .collect();
    if existing.title != b.title
        || existing.description != b.description
        || existing.kind != b.kind
        || existing.status != b.status
        || existing.priority != b.priority
        || existing.assignee != b.assignee
        || existing.due != b.due
        || existing.created != b.created
        || existing.author.trim() != b.author.trim()
            && !(existing.author.trim().is_empty() && b.author == "unknown")
        || lower(&existing.labels) != lower(&b.labels)
        || universe(existing) != universe(b)
        || ex_vps.len() != b.viewpoints.len()
        || comments.len() != imported.comments.len()
    {
        return false;
    }
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-6 * (1.0 + a.abs().max(b.abs()));
    let same_view = |a: &Option<ViewSpec>, b: &Option<ViewSpec>| match (a, b) {
        (None, None) => true,
        (
            Some(ViewSpec::Plan {
                center: c1,
                zoom: z1,
            }),
            Some(ViewSpec::Plan {
                center: c2,
                zoom: z2,
            }),
        ) => near(c1[0], c2[0]) && near(c1[1], c2[1]) && near(*z1, *z2),
        (
            Some(ViewSpec::Camera {
                target: t1,
                yaw: y1,
                pitch: p1,
                distance: d1,
                perspective: s1,
            }),
            Some(ViewSpec::Camera {
                target: t2,
                yaw: y2,
                pitch: p2,
                distance: d2,
                perspective: s2,
            }),
        ) => {
            s1 == s2
                && near(*p1, *p2)
                // A perspective camera has no distance or target in BCF.
                && (*s1 || near(*d1, *d2))
                && (*s1 || t1.iter().zip(t2).all(|(a, b)| near(*a, *b)))
                // The yaw does not count at the poles.
                && (near(*y1, *y2) || p1.abs() > std::f64::consts::FRAC_PI_2 - 1e-4)
        }
        // A view that BCF cannot hold (a viewpoint without a camera) comes back as a plan view.
        (None, Some(_)) => true,
        _ => false,
    };
    for v in &ex_vps {
        let Some(w) = b.viewpoints.iter().find(|w| w.id == v.id) else {
            return false;
        };
        if v.snapshot != w.snapshot || !same_view(&v.view, &w.view) {
            return false;
        }
    }
    for c in comments {
        let Some(d) = imported.comments.iter().find(|d| d.id == c.id) else {
            return false;
        };
        if c.text != d.text
            || c.time != d.time
            || c.viewpoint != d.viewpoint
            || c.edited != d.edited
            || c.author.trim() != d.author.trim()
                && !(c.author.trim().is_empty() && d.author == "unknown")
        {
            return false;
        }
    }
    true
}
