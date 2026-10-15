//! The reader: bounded, with every entry checked against the manifest.
//!
//! What the reader does before it reads any entry body: it finds the end record, reads the
//! central directory (bounded), checks every name, size, method and flag, reads the manifest and
//! compares it with the directory. A read of an entry stops at the declared size, so a lying
//! directory cannot make a big allocation, and it checks the CRC-32 and the SHA-256 at the end.
//! Nothing panics on a damaged file; each fault is an error that names the entry.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use flate2::Crc;
use sha2::{Digest, Sha256};

use crate::error::ContainerError;
use crate::limits::{Limits, MANIFEST_NAME, MAX_FIELD, check_name, shown};
use crate::manifest::{Manifest, hex, parse_hash};
use crate::medium::{Medium, MemMedium};

pub(crate) const LOCAL_SIG: u32 = 0x0403_4b50;
pub(crate) const CENTRAL_SIG: u32 = 0x0201_4b50;
pub(crate) const END_SIG: u32 = 0x0605_4b50;
pub(crate) const DESCRIPTOR_SIG: u32 = 0x0807_4b50;
pub(crate) const END_LEN: usize = 22;
pub(crate) const LOCAL_LEN: usize = 30;
pub(crate) const CENTRAL_LEN: usize = 46;

/// How an entry is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// The bytes as they are.
    Stored,
    /// Deflate.
    Deflate,
}

impl Method {
    pub(crate) fn code(self) -> u16 {
        match self {
            Method::Stored => 0,
            Method::Deflate => 8,
        }
    }

    fn from_code(code: u16) -> Option<Method> {
        match code {
            0 => Some(Method::Stored),
            8 => Some(Method::Deflate),
            _ => None,
        }
    }
}

/// An entry of an opened container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The size of the entry, uncompressed.
    pub size: u64,
    /// The size in the file.
    pub compressed: u64,
    pub method: Method,
    /// CRC-32 of the uncompressed bytes.
    pub crc32: u32,
    /// SHA-256 of the uncompressed bytes, from the manifest.
    pub sha256: [u8; 32],
    /// Where the local header starts.
    offset: u64,
}

/// An entry as it is in the file, for copying to another container without decoding it.
pub struct RawEntry {
    pub name: String,
    pub method: Method,
    pub crc32: u32,
    pub size: u64,
    pub compressed: u64,
    pub sha256: [u8; 32],
    /// The compressed bytes.
    pub data: Box<dyn Read + Send>,
}

impl std::fmt::Debug for RawEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawEntry")
            .field("name", &self.name)
            .field("size", &self.size)
            .field("compressed", &self.compressed)
            .finish_non_exhaustive()
    }
}

/// An opened container.
#[derive(Clone)]
pub struct Container {
    medium: Arc<dyn Medium>,
    // Shared, so that a copy of the container costs a few pointers: a source gives out a copy for
    // each read, and a container can have 65 534 entries.
    entries: Arc<Vec<Entry>>,
    index: Arc<HashMap<String, usize>>,
    /// Where the central directory starts: the data of every entry ends before it.
    dir_offset: u64,
    manifest: Arc<Manifest>,
    manifest_sha256: [u8; 32],
    manifest_entry: Entry,
    total: u64,
}

impl std::fmt::Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Container")
            .field("medium", &self.medium)
            .field("entries", &self.entries.len())
            .field("uncompressed", &self.total)
            .finish()
    }
}

/// True if the first bytes are those of a ZIP container: the signature of a local header. A plain
/// JSON file starts with `{` (after white space).
pub fn has_zip_magic(head: &[u8]) -> bool {
    head.starts_with(&LOCAL_SIG.to_le_bytes())
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ContainerError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.data.len())
            .ok_or_else(|| ContainerError::format("the central directory is cut short"))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16, ContainerError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, ContainerError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// One entry of the central directory, before the manifest is known.
struct Raw {
    name: String,
    method: Method,
    crc32: u32,
    size: u64,
    compressed: u64,
    offset: u64,
}

fn read_exact(medium: &dyn Medium, offset: u64, len: u64) -> Result<Vec<u8>, ContainerError> {
    let mut buf = vec![0u8; usize::try_from(len).map_err(|_| ContainerError::format("too big"))?];
    medium
        .open(offset, len)?
        .read_exact(&mut buf)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ContainerError::format("the file is cut short")
            } else {
                ContainerError::from_io(e)
            }
        })?;
    Ok(buf)
}

impl Container {
    /// Opens a container from bytes in memory.
    pub fn from_bytes(
        bytes: impl Into<Arc<[u8]>>,
        limits: Limits,
    ) -> Result<Container, ContainerError> {
        Container::open(Arc::new(MemMedium::new(bytes)), limits)
    }

    /// Opens a container: reads the end record, the central directory and the manifest, and
    /// checks all of them. No entry body but the manifest is read.
    pub fn open(medium: Arc<dyn Medium>, limits: Limits) -> Result<Container, ContainerError> {
        let limits = limits.clamped();
        let len = medium.len();
        if len < END_LEN as u64 {
            return Err(ContainerError::format("the file is too short"));
        }
        if len > MAX_FIELD {
            return Err(ContainerError::Limit(format!(
                "the file has {len} bytes; a container is at most {MAX_FIELD} bytes (ZIP64 is not supported)"
            )));
        }
        // The end record is in the last 22 + 65535 bytes (the comment is at the end).
        let tail_len = len.min(END_LEN as u64 + 65_535);
        let tail = read_exact(&*medium, len - tail_len, tail_len)?;
        let end_at = (0..=tail.len() - END_LEN)
            .rev()
            .find(|&i| {
                le32(&tail, i) == END_SIG
                    && tail.len() - (i + END_LEN) == usize::from(le16(&tail, i + 20))
            })
            .ok_or_else(|| {
                ContainerError::format("the end of the central directory is missing; the file is cut short or is not a ZIP file")
            })?;
        let end = &tail[end_at..];
        let (disk, dir_disk) = (le16(end, 4), le16(end, 6));
        let (on_disk, count) = (le16(end, 8), le16(end, 10));
        let (dir_size, dir_offset) = (le32(end, 12), le32(end, 16));
        if disk != 0 || dir_disk != 0 || on_disk != count {
            return Err(ContainerError::format(
                "a file in several parts is not supported",
            ));
        }
        if count == 0xFFFF || dir_size == 0xFFFF_FFFF || dir_offset == 0xFFFF_FFFF {
            return Err(ContainerError::format("ZIP64 is not supported"));
        }
        let end_pos = len - tail_len + end_at as u64;
        let (dir_size, dir_offset) = (u64::from(dir_size), u64::from(dir_offset));
        if dir_offset + dir_size > end_pos {
            return Err(ContainerError::format(
                "the central directory is not where the end record says",
            ));
        }
        if usize::from(count) > limits.max_entries {
            return Err(ContainerError::Limit(format!(
                "the container has {count} entries; at most {} are read",
                limits.max_entries
            )));
        }
        if dir_size > limits.max_directory_bytes {
            return Err(ContainerError::Limit(format!(
                "the central directory has {dir_size} bytes; at most {} are read",
                limits.max_directory_bytes
            )));
        }
        let dir = read_exact(&*medium, dir_offset, dir_size)?;
        let raws = parse_directory(&dir, usize::from(count), dir_offset, &limits)?;
        Container::with_manifest(medium, raws, dir_offset, &limits)
    }

    fn with_manifest(
        medium: Arc<dyn Medium>,
        raws: Vec<Raw>,
        dir_offset: u64,
        limits: &Limits,
    ) -> Result<Container, ContainerError> {
        let mut total = 0u64;
        for r in &raws {
            if r.size > limits.max_entry_bytes {
                return Err(ContainerError::Limit(format!(
                    "the entry \"{}\" has {} bytes; at most {} bytes are read for one entry",
                    shown(&r.name),
                    r.size,
                    limits.max_entry_bytes
                )));
            }
            total = total.saturating_add(r.size);
            if total > limits.max_total_bytes {
                return Err(ContainerError::Limit(format!(
                    "the entries have more than {} bytes together (uncompressed); this is the limit for this file",
                    limits.max_total_bytes
                )));
            }
            if r.size > limits.ratio_floor && r.size / limits.max_ratio.max(1) > r.compressed {
                return Err(ContainerError::Limit(format!(
                    "the entry \"{}\" grows more than {} times when it is decompressed ({} bytes become {}); this looks like a compression bomb",
                    shown(&r.name),
                    limits.max_ratio,
                    r.compressed,
                    r.size
                )));
            }
        }
        let pos = raws
            .iter()
            .position(|r| r.name == MANIFEST_NAME)
            .ok_or_else(|| ContainerError::entry(MANIFEST_NAME, "the manifest is missing"))?;
        let m = &raws[pos];
        if m.size > limits.max_manifest_bytes {
            return Err(ContainerError::Limit(format!(
                "the manifest has {} bytes; at most {} are read",
                m.size, limits.max_manifest_bytes
            )));
        }
        // The manifest has no hash of its own to check against: its bytes are checked by the
        // CRC-32 of the directory, and everything it says is checked when an entry is read.
        let manifest_entry = Entry {
            name: m.name.clone(),
            size: m.size,
            compressed: m.compressed,
            method: m.method,
            crc32: m.crc32,
            sha256: [0; 32],
            offset: m.offset,
        };
        let mut container = Container {
            medium,
            entries: Arc::default(),
            index: Arc::default(),
            dir_offset,
            manifest: Arc::new(Manifest {
                format: String::new(),
                format_version: 0,
                generator: String::new(),
                entries: Vec::new(),
            }),
            manifest_sha256: [0; 32],
            manifest_entry: manifest_entry.clone(),
            total,
        };
        let bytes = container.read_checked(&manifest_entry, None)?;
        let manifest = Manifest::parse(&bytes)?;
        let mut listed: HashMap<&str, (u64, [u8; 32])> = HashMap::new();
        for e in &manifest.entries {
            if e.name == MANIFEST_NAME {
                return Err(ContainerError::entry(
                    MANIFEST_NAME,
                    "the manifest lists itself",
                ));
            }
            let sha = parse_hash(&e.sha256).ok_or_else(|| {
                ContainerError::entry(
                    &e.name,
                    "its hash in the manifest is not 64 lowercase hex digits",
                )
            })?;
            if listed.insert(&e.name, (e.size, sha)).is_some() {
                return Err(ContainerError::entry(
                    &e.name,
                    "the manifest lists it twice",
                ));
            }
        }
        let mut entries = Vec::with_capacity(raws.len());
        for r in raws {
            if r.name == MANIFEST_NAME {
                continue;
            }
            let (size, sha256) = listed
                .remove(r.name.as_str())
                .ok_or_else(|| ContainerError::entry(&r.name, "the manifest does not list it"))?;
            if size != r.size {
                return Err(ContainerError::entry(
                    &r.name,
                    format!(
                        "the manifest says {size} bytes, the directory says {}",
                        r.size
                    ),
                ));
            }
            entries.push(Entry {
                name: r.name,
                size: r.size,
                compressed: r.compressed,
                method: r.method,
                crc32: r.crc32,
                sha256,
                offset: r.offset,
            });
        }
        if let Some(name) = listed.keys().min() {
            return Err(ContainerError::entry(
                name,
                "the manifest lists it, but it is not in the container",
            ));
        }
        container.index = Arc::new(
            entries
                .iter()
                .enumerate()
                .map(|(i, e)| (e.name.clone(), i))
                .collect(),
        );
        container.entries = Arc::new(entries);
        container.manifest_sha256 = <[u8; 32]>::from(Sha256::digest(&bytes));
        container.manifest = Arc::new(manifest);
        Ok(container)
    }

    /// The entries (not the manifest), in the order of the directory.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry with this name.
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.index.get(name).map(|i| &self.entries[*i])
    }

    /// True if the container has an entry with this name.
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// The manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// SHA-256 of the bytes of `manifest.json`: names the content of the container.
    pub fn manifest_sha256(&self) -> String {
        hex(&self.manifest_sha256)
    }

    /// The bytes of all entries together, uncompressed, as the directory says (the manifest too).
    /// This is the size that a program needs when it holds the container in memory; the file
    /// itself can be much smaller.
    pub fn uncompressed_bytes(&self) -> u64 {
        self.total
    }

    /// The size of the file in bytes.
    pub fn file_bytes(&self) -> u64 {
        self.medium.len()
    }

    /// The medium (to make a lazy source from it).
    pub fn medium(&self) -> &Arc<dyn Medium> {
        &self.medium
    }

    /// Checks that the medium is still the one that was opened.
    pub fn verify(&self) -> Result<(), ContainerError> {
        self.medium.verify()
    }

    /// Opens the local header of the entry and returns where its data starts.
    fn data_start(&self, e: &Entry) -> Result<u64, ContainerError> {
        let bad = |why: &str| ContainerError::entry(&e.name, why);
        // The local header and the name in one read: the name must be the name of the directory,
        // so its length is known.
        let want = LOCAL_LEN as u64 + e.name.len() as u64;
        let head =
            read_exact(&*self.medium, e.offset, want).map_err(|err| as_entry(err, &e.name))?;
        if le32(&head, 0) != LOCAL_SIG {
            return Err(bad("its local header is missing"));
        }
        if Method::from_code(le16(&head, 8)) != Some(e.method) {
            return Err(bad(
                "its local header says another method than the directory",
            ));
        }
        let (name_len, extra_len) = (u64::from(le16(&head, 26)), u64::from(le16(&head, 28)));
        if name_len != e.name.len() as u64 || head[LOCAL_LEN..] != *e.name.as_bytes() {
            return Err(bad("its local header has another name than the directory"));
        }
        let start = e.offset + LOCAL_LEN as u64 + name_len + extra_len;
        if start + e.compressed > self.dir_offset {
            return Err(bad("its data reaches into the central directory"));
        }
        Ok(start)
    }

    /// A reader for an entry. It stops with an error at more bytes than the directory declares,
    /// and at the end it checks the size, the CRC-32 and the SHA-256 (of the manifest). Read it
    /// to the end; a reader that is dropped early has checked nothing.
    pub fn reader(&self, name: &str) -> Result<EntryReader, ContainerError> {
        let e = self
            .entry(name)
            .ok_or_else(|| ContainerError::entry(name, "it is not in the container"))?;
        self.reader_of(e, Some(e.sha256))
    }

    fn reader_of(&self, e: &Entry, sha: Option<[u8; 32]>) -> Result<EntryReader, ContainerError> {
        self.medium.verify()?;
        let start = self.data_start(e)?;
        let raw = self
            .medium
            .open(start, e.compressed)
            .map_err(|err| as_entry(err, &e.name))?;
        let inner = match e.method {
            Method::Stored => Inner::Stored(raw),
            Method::Deflate => Inner::Deflate(flate2::read::DeflateDecoder::new(raw)),
        };
        Ok(EntryReader {
            inner,
            name: e.name.clone(),
            size: e.size,
            crc32: e.crc32,
            sha256: sha,
            crc: Crc::new(),
            sha: Sha256::new(),
            got: 0,
            done: false,
        })
    }

    fn read_checked(&self, e: &Entry, sha: Option<[u8; 32]>) -> Result<Vec<u8>, ContainerError> {
        let mut r = self.reader_of(e, sha)?;
        let mut out = Vec::with_capacity(usize::try_from(e.size.min(16 << 20)).unwrap_or(0));
        r.read_to_end(&mut out)
            .map_err(|err| as_entry(ContainerError::from_io(err), &e.name))?;
        Ok(out)
    }

    /// The bytes of an entry, checked (see [`Container::reader`]).
    pub fn read(&self, name: &str) -> Result<Vec<u8>, ContainerError> {
        let e = self
            .entry(name)
            .ok_or_else(|| ContainerError::entry(name, "it is not in the container"))?;
        self.read_checked(e, Some(e.sha256))
    }

    /// The first `n` bytes of an entry (fewer if it is shorter). Only an entry that is stored can
    /// be read like this without decoding the rest; a deflated entry is decoded up to `n`. The
    /// bytes are not checked against the hash (that needs the whole entry): use it to look at the
    /// start of a part (is it a PNG?) and read the whole part before the data is used.
    pub fn read_prefix(&self, name: &str, n: usize) -> Result<Vec<u8>, ContainerError> {
        let e = self
            .entry(name)
            .ok_or_else(|| ContainerError::entry(name, "it is not in the container"))?;
        self.medium.verify()?;
        let start = self.data_start(e)?;
        let raw = self
            .medium
            .open(start, e.compressed)
            .map_err(|err| as_entry(err, &e.name))?;
        let want = (n as u64).min(e.size);
        let mut out = Vec::with_capacity(want as usize);
        let res = match e.method {
            Method::Stored => raw.take(want).read_to_end(&mut out),
            Method::Deflate => flate2::read::DeflateDecoder::new(raw)
                .take(want)
                .read_to_end(&mut out),
        };
        res.map_err(|err| as_entry(ContainerError::from_io(err), &e.name))?;
        Ok(out)
    }

    /// An entry as it is in the file, for [`crate::Writer::copy_raw`]. The data is not decoded or
    /// checked; the copy keeps the CRC-32 and the hash, so a damaged entry is found when the new
    /// container is read.
    pub fn raw(&self, name: &str) -> Result<RawEntry, ContainerError> {
        let e = self
            .entry(name)
            .ok_or_else(|| ContainerError::entry(name, "it is not in the container"))?;
        self.medium.verify()?;
        let start = self.data_start(e)?;
        let data = self
            .medium
            .open(start, e.compressed)
            .map_err(|err| as_entry(err, &e.name))?;
        Ok(RawEntry {
            name: e.name.clone(),
            method: e.method,
            crc32: e.crc32,
            size: e.size,
            compressed: e.compressed,
            sha256: e.sha256,
            data,
        })
    }

    /// The manifest entry in the directory (its size is the size of `manifest.json`).
    pub fn manifest_size(&self) -> u64 {
        self.manifest_entry.size
    }
}

/// A damaged read of a part that has no name yet gets the name of the entry.
fn as_entry(e: ContainerError, name: &str) -> ContainerError {
    match e {
        ContainerError::Format(r) => ContainerError::entry(name, r),
        ContainerError::Io(r) => ContainerError::entry(name, r),
        other => other,
    }
}

impl ContainerError {
    /// The container error inside an I/O error, else the I/O error.
    pub(crate) fn from_io(e: std::io::Error) -> ContainerError {
        match e.get_ref().and_then(|i| i.downcast_ref::<ContainerError>()) {
            Some(c) => c.clone(),
            None => ContainerError::Io(e.to_string()),
        }
    }
}

fn parse_directory(
    dir: &[u8],
    count: usize,
    dir_offset: u64,
    limits: &Limits,
) -> Result<Vec<Raw>, ContainerError> {
    let mut c = Cursor { data: dir, pos: 0 };
    let mut out = Vec::with_capacity(count);
    let mut seen = HashSet::with_capacity(count);
    for _ in 0..count {
        if c.u32()? != CENTRAL_SIG {
            return Err(ContainerError::format(
                "the central directory has a wrong entry header",
            ));
        }
        let _made_by = c.u16()?;
        let _needed = c.u16()?;
        let flags = c.u16()?;
        let method = c.u16()?;
        let _time = c.u16()?;
        let _date = c.u16()?;
        let crc32 = c.u32()?;
        let compressed = c.u32()?;
        let size = c.u32()?;
        let name_len = usize::from(c.u16()?);
        let extra_len = usize::from(c.u16()?);
        let comment_len = usize::from(c.u16()?);
        let disk = c.u16()?;
        let _internal = c.u16()?;
        let _external = c.u32()?;
        let offset = c.u32()?;
        let name_bytes = c.take(name_len)?;
        c.take(extra_len)?;
        c.take(comment_len)?;
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| ContainerError::format("an entry name is not UTF-8 text"))?
            .to_string();
        check_name(&name, limits.max_name_bytes)?;
        if !seen.insert(name.clone()) {
            return Err(ContainerError::entry(&name, "the name is used twice"));
        }
        if flags & 1 != 0 || flags & (1 << 6) != 0 {
            return Err(ContainerError::entry(
                &name,
                "it is encrypted, which is not supported",
            ));
        }
        if compressed == 0xFFFF_FFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
            return Err(ContainerError::entry(&name, "ZIP64 is not supported"));
        }
        if disk != 0 {
            return Err(ContainerError::entry(&name, "it is on another disk"));
        }
        let method = Method::from_code(method).ok_or_else(|| {
            ContainerError::entry(
                &name,
                format!("the method {method} is not supported (use stored or deflate)"),
            )
        })?;
        if method == Method::Stored && compressed != size {
            return Err(ContainerError::entry(
                &name,
                "it is stored, but its two sizes differ",
            ));
        }
        if u64::from(offset) + LOCAL_LEN as u64 > dir_offset {
            return Err(ContainerError::entry(
                &name,
                "its local header is not in the file",
            ));
        }
        out.push(Raw {
            name,
            method,
            crc32,
            size: u64::from(size),
            compressed: u64::from(compressed),
            offset: u64::from(offset),
        });
    }
    if c.pos != dir.len() {
        return Err(ContainerError::format(
            "the central directory has bytes after its last entry",
        ));
    }
    Ok(out)
}

enum Inner {
    Stored(Box<dyn Read + Send>),
    Deflate(flate2::read::DeflateDecoder<Box<dyn Read + Send>>),
}

/// A checked reader of one entry (see [`Container::reader`]).
pub struct EntryReader {
    inner: Inner,
    name: String,
    size: u64,
    crc32: u32,
    sha256: Option<[u8; 32]>,
    crc: Crc,
    sha: Sha256,
    got: u64,
    done: bool,
}

impl EntryReader {
    /// The name of the entry.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The size the entry should have.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Reads what is left and runs the final checks. Call it after a parser that stops before the
    /// end (a JSON parser stops at the end of the value).
    pub fn finish(&mut self) -> Result<(), ContainerError> {
        let mut sink = [0u8; 8192];
        loop {
            match self.read(&mut sink) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(e) => return Err(ContainerError::from_io(e)),
            }
        }
    }

    fn fail(&mut self, reason: String) -> std::io::Error {
        self.done = true;
        ContainerError::entry(&self.name, reason).into()
    }
}

impl Read for EntryReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        let n = match &mut self.inner {
            Inner::Stored(r) => r.read(buf),
            Inner::Deflate(r) => r.read(buf),
        }
        .map_err(|e| {
            self.done = true;
            // A damaged deflate stream or a failed read: name the entry.
            match e.get_ref().and_then(|i| i.downcast_ref::<ContainerError>()) {
                Some(c) => std::io::Error::from(c.clone()),
                None => std::io::Error::from(ContainerError::entry(
                    &self.name,
                    format!("it cannot be decoded: {e}"),
                )),
            }
        })?;
        if n > 0 {
            self.got += n as u64;
            if self.got > self.size {
                let msg = format!(
                    "it has more than the {} bytes that the directory declares",
                    self.size
                );
                return Err(self.fail(msg));
            }
            self.crc.update(&buf[..n]);
            self.sha.update(&buf[..n]);
            return Ok(n);
        }
        self.done = true;
        if self.got != self.size {
            let msg = format!(
                "it has {} bytes, the directory declares {} (the file is cut short or damaged)",
                self.got, self.size
            );
            return Err(self.fail(msg));
        }
        if self.crc.sum() != self.crc32 {
            return Err(self.fail("its CRC-32 does not match (the data is damaged)".into()));
        }
        if let Some(want) = self.sha256 {
            let got = <[u8; 32]>::from(self.sha.clone().finalize());
            if got != want {
                return Err(self.fail(
                    "its SHA-256 does not match the manifest (the data is damaged or was changed)"
                        .into(),
                ));
            }
        }
        Ok(0)
    }
}
