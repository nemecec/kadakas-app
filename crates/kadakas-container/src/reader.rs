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
use crate::limits::{Limits, MANIFEST_NAME, check_name, shown};
use crate::manifest::{Manifest, hex, parse_hash};
use crate::medium::{Medium, MemMedium};

pub(crate) const LOCAL_SIG: u32 = 0x0403_4b50;
pub(crate) const CENTRAL_SIG: u32 = 0x0201_4b50;
pub(crate) const END_SIG: u32 = 0x0605_4b50;
pub(crate) const DESCRIPTOR_SIG: u32 = 0x0807_4b50;
pub(crate) const END64_SIG: u32 = 0x0606_4b50;
pub(crate) const LOCATOR_SIG: u32 = 0x0706_4b50;
/// The value of a 32 bit field that says: the real value is in the ZIP64 extra field.
pub(crate) const ZIP64_AT: u64 = 0xFFFF_FFFF;
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
    /// What the entry is for, from the manifest (see `ManifestEntry::role`).
    pub role: Option<String>,
    /// Where the local header starts.
    offset: u64,
    /// Where the next entry (or the central directory) starts: the data of this entry ends
    /// before it. The ranges of the entries are disjoint.
    limit: u64,
}

/// An entry as it is in the file, for copying to another container without decoding it.
pub struct RawEntry {
    pub name: String,
    pub method: Method,
    pub crc32: u32,
    pub size: u64,
    pub compressed: u64,
    pub sha256: [u8; 32],
    /// What the entry is for (see `ManifestEntry::role`).
    pub role: Option<String>,
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

fn le64(b: &[u8], at: usize) -> u64 {
    let mut x = [0u8; 8];
    x.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(x)
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
        let end_pos = len - tail_len + end_at as u64;
        let saturated = count == 0xFFFF || dir_size == 0xFFFF_FFFF || dir_offset == 0xFFFF_FFFF;
        // The ZIP64 locator is the 20 bytes before the end record.
        let locator = (end_pos >= 20)
            .then(|| end_at.checked_sub(20))
            .flatten()
            .filter(|at| le32(&tail, *at) == LOCATOR_SIG);
        let (count, dir_size, dir_offset, dir_end) = match locator {
            Some(at) => {
                let l = &tail[at..at + 20];
                let (l_disk, rec_at, disks) = (le32(l, 4), le64(l, 8), le32(l, 16));
                if l_disk != 0 || disks > 1 {
                    return Err(ContainerError::format(
                        "a file in several parts is not supported",
                    ));
                }
                let locator_pos = end_pos - 20;
                if rec_at.checked_add(56).is_none_or(|e| e > locator_pos) {
                    return Err(ContainerError::format(
                        "the ZIP64 end record is not where the locator says",
                    ));
                }
                let rec = read_exact(&*medium, rec_at, 56)?;
                if le32(&rec, 0) != END64_SIG || le64(&rec, 4) < 44 {
                    return Err(ContainerError::format("the ZIP64 end record is damaged"));
                }
                let (disk, dir_disk) = (le32(&rec, 16), le32(&rec, 20));
                let (on_disk, total) = (le64(&rec, 24), le64(&rec, 32));
                if disk != 0 || dir_disk != 0 || on_disk != total {
                    return Err(ContainerError::format(
                        "a file in several parts is not supported",
                    ));
                }
                let (zsize, zoff) = (le64(&rec, 40), le64(&rec, 48));
                // A 32 bit field that is not "see the ZIP64 record" must say the same as the
                // ZIP64 record: two readers must not find two directories.
                if (count != 0xFFFF && u64::from(count) != total)
                    || (dir_size != 0xFFFF_FFFF && u64::from(dir_size) != zsize)
                    || (dir_offset != 0xFFFF_FFFF && u64::from(dir_offset) != zoff)
                {
                    return Err(ContainerError::format(
                        "the end record and the ZIP64 end record say different things",
                    ));
                }
                (total, zsize, zoff, rec_at)
            }
            None if saturated => {
                return Err(ContainerError::format(
                    "the ZIP64 end record is missing (the end record says that it follows)",
                ));
            }
            None => (
                u64::from(count),
                u64::from(dir_size),
                u64::from(dir_offset),
                end_pos,
            ),
        };
        if dir_offset.checked_add(dir_size).is_none_or(|e| e > dir_end) {
            return Err(ContainerError::format(
                "the central directory is not where the end record says",
            ));
        }
        if count > limits.max_entries as u64 {
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
        let raws = parse_directory(&dir, count as usize, dir_offset, &limits)?;
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
        let limits_of = disjoint_ranges(&raws, dir_offset)?;
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
            role: None,
            offset: m.offset,
            limit: limits_of[pos],
        };
        let mut container = Container {
            medium,
            entries: Arc::default(),
            index: Arc::default(),
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
        let mut listed: HashMap<&str, (u64, [u8; 32], Option<String>)> = HashMap::new();
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
            if listed
                .insert(&e.name, (e.size, sha, e.role.clone()))
                .is_some()
            {
                return Err(ContainerError::entry(
                    &e.name,
                    "the manifest lists it twice",
                ));
            }
        }
        let mut entries = Vec::with_capacity(raws.len());
        for (i, r) in raws.into_iter().enumerate() {
            if r.name == MANIFEST_NAME {
                continue;
            }
            let (size, sha256, role) = listed
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
                role,
                offset: r.offset,
                limit: limits_of[i],
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

    /// Opens the local header of the entry and returns where its data starts. The header must
    /// agree with the directory: the name, the method, and (when the sizes are not left to a data
    /// descriptor) the CRC-32 and the sizes. An entry that the header says is encrypted is refused.
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
        let flags = le16(&head, 6);
        if flags & (1 | 1 << 6 | 1 << 13) != 0 {
            return Err(bad(
                "its local header says that it is encrypted, which is not supported",
            ));
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
        if flags & (1 << 3) == 0 {
            // The sizes are in the header: they are those of the directory.
            let (crc, comp32, size32) = (le32(&head, 14), le32(&head, 18), le32(&head, 22));
            let (mut comp, mut size) = (u64::from(comp32), u64::from(size32));
            if comp == ZIP64_AT || size == ZIP64_AT {
                let extra = read_exact(&*self.medium, e.offset + want, extra_len)
                    .map_err(|err| as_entry(err, &e.name))?;
                let big = zip64_values(&extra, [true, true, false, false], &e.name)?;
                size = big[0].unwrap_or(size);
                comp = big[1].unwrap_or(comp);
            }
            if crc != e.crc32 || comp != e.compressed || size != e.size {
                return Err(bad(
                    "its local header has another CRC-32 or size than the directory",
                ));
            }
        }
        let start = e.offset + want + extra_len;
        if start
            .checked_add(e.compressed)
            .is_none_or(|end| end > e.limit)
        {
            return Err(bad(
                "its data reaches into the next entry or into the central directory",
            ));
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

    /// An entry as it is in the file, for [`crate::Writer::copy_raw`]. A stored entry is checked
    /// while it is copied (size, CRC-32 and SHA-256), so a copy never carries damaged bytes into a
    /// new file. A deflated entry is not decoded: the copy keeps the CRC-32 and the hash, so a
    /// damaged entry is found when the new container is read.
    pub fn raw(&self, name: &str) -> Result<RawEntry, ContainerError> {
        let e = self
            .entry(name)
            .ok_or_else(|| ContainerError::entry(name, "it is not in the container"))?;
        self.medium.verify()?;
        let data: Box<dyn Read + Send> = match e.method {
            Method::Stored => Box::new(self.reader_of(e, Some(e.sha256))?),
            Method::Deflate => {
                let start = self.data_start(e)?;
                self.medium
                    .open(start, e.compressed)
                    .map_err(|err| as_entry(err, &e.name))?
            }
        };
        Ok(RawEntry {
            name: e.name.clone(),
            method: e.method,
            crc32: e.crc32,
            size: e.size,
            compressed: e.compressed,
            sha256: e.sha256,
            role: e.role.clone(),
            data,
        })
    }

    /// [`Container::raw`], and a deflated entry is checked while it is read: the compressed bytes
    /// pass to the caller and, at the same time, through a decoder that checks the size, the
    /// CRC-32 and the SHA-256. The bytes are read once (what is checked is what is copied), with
    /// no extra memory. The last read fails if the entry is damaged, so a copy that reads to the
    /// end never takes a bad entry as good. A stored entry is checked by [`Container::raw`].
    pub fn raw_verified(&self, name: &str) -> Result<RawEntry, ContainerError> {
        let mut raw = self.raw(name)?;
        if raw.method == Method::Deflate {
            let tee = TeeVerify {
                name: raw.name.clone(),
                size: raw.size,
                crc32: raw.crc32,
                sha256: raw.sha256,
                decoder: Some(flate2::write::DeflateDecoder::new(Check {
                    crc: Crc::new(),
                    sha: Sha256::new(),
                    got: 0,
                    limit: raw.size,
                })),
                inner: raw.data,
                done: false,
            };
            raw.data = Box::new(tee);
        }
        Ok(raw)
    }

    /// Reads the entry to its end and checks it (size, CRC-32, SHA-256), and drops the bytes.
    pub fn verify_entry(&self, name: &str) -> Result<(), ContainerError> {
        self.reader(name)?.finish()
    }

    /// The manifest entry in the directory (its size is the size of `manifest.json`).
    pub fn manifest_size(&self) -> u64 {
        self.manifest_entry.size
    }
}

/// What the decoder of [`TeeVerify`] writes to: the checks of the decoded bytes.
struct Check {
    crc: Crc,
    sha: Sha256,
    got: u64,
    limit: u64,
}

impl std::io::Write for Check {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.got += buf.len() as u64;
        if self.got > self.limit {
            return Err(std::io::Error::other(format!(
                "it has more than the {} bytes that the directory declares",
                self.limit
            )));
        }
        self.crc.update(buf);
        self.sha.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The compressed bytes of a deflated entry, passed on as they are, and checked on the way (see
/// [`Container::raw_verified`]).
struct TeeVerify {
    name: String,
    size: u64,
    crc32: u32,
    sha256: [u8; 32],
    decoder: Option<flate2::write::DeflateDecoder<Check>>,
    inner: Box<dyn Read + Send>,
    done: bool,
}

impl TeeVerify {
    fn fail(&mut self, reason: String) -> std::io::Error {
        self.done = true;
        ContainerError::entry(&self.name, reason).into()
    }
}

impl Read for TeeVerify {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        let n = self.inner.read(buf)?;
        if n > 0 {
            let fed = match self.decoder.as_mut() {
                Some(d) => std::io::Write::write_all(d, &buf[..n]),
                None => Ok(()),
            };
            if let Err(e) = fed {
                return Err(self.fail(format!("it cannot be decoded: {e}")));
            }
            return Ok(n);
        }
        // The end of the compressed bytes: the decoder must be at the end of the stream, and the
        // decoded bytes must be what the manifest says.
        self.done = true;
        let Some(d) = self.decoder.take() else {
            return Ok(0);
        };
        let check = match d.finish() {
            Ok(c) => c,
            Err(e) => return Err(self.fail(format!("it cannot be decoded: {e}"))),
        };
        if check.got != self.size {
            return Err(self.fail(format!(
                "it has {} bytes, the directory declares {} (the file is cut short or damaged)",
                check.got, self.size
            )));
        }
        if check.crc.sum() != self.crc32 {
            return Err(self.fail("its CRC-32 does not match (the data is damaged)".into()));
        }
        if <[u8; 32]>::from(check.sha.finalize()) != self.sha256 {
            return Err(self.fail(
                "its SHA-256 does not match the manifest (the data is damaged or was changed)"
                    .into(),
            ));
        }
        Ok(0)
    }
}

/// For each entry, where the next entry (or the directory) starts. No two entries may overlap:
/// the smallest possible extent of an entry (its local header, name and data) must end before the
/// next entry starts.
fn disjoint_ranges(raws: &[Raw], dir_offset: u64) -> Result<Vec<u64>, ContainerError> {
    let mut order: Vec<usize> = (0..raws.len()).collect();
    order.sort_by_key(|i| raws[*i].offset);
    let mut limit = vec![dir_offset; raws.len()];
    let least = |a: &Raw| {
        a.offset
            .saturating_add(LOCAL_LEN as u64 + a.name.len() as u64)
            .saturating_add(a.compressed)
    };
    for w in order.windows(2) {
        let (a, b) = (&raws[w[0]], &raws[w[1]]);
        if a.offset == b.offset || least(a) > b.offset {
            return Err(ContainerError::entry(
                &b.name,
                format!(
                    "it overlaps the entry \"{}\" (the file is damaged or was made to confuse a reader)",
                    shown(&a.name)
                ),
            ));
        }
        limit[w[0]] = b.offset;
    }
    if let Some(last) = order.last() {
        let a = &raws[*last];
        if least(a) > dir_offset {
            return Err(ContainerError::entry(
                &a.name,
                "its data reaches into the central directory",
            ));
        }
    }
    Ok(limit)
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

/// The 64 bit values of the ZIP64 extra field (id 1). A value is in the field only if the 32 bit
/// field next to it holds 0xFFFFFFFF, and the values come in this order: size, compressed size,
/// offset, and (4 bytes) the number of the disk. `want` says which of the four are there. A second
/// ZIP64 field is refused: two readers could take different values.
pub(crate) fn zip64_values(
    extra: &[u8],
    want: [bool; 4],
    name: &str,
) -> Result<[Option<u64>; 4], ContainerError> {
    let mut at = 0usize;
    let mut found: Option<[Option<u64>; 4]> = None;
    while at + 4 <= extra.len() {
        let (id, n) = (le16(extra, at), usize::from(le16(extra, at + 2)));
        let body = extra
            .get(at + 4..at + 4 + n)
            .ok_or_else(|| ContainerError::entry(name, "its extra field is cut short"))?;
        if id == 1 {
            if found.is_some() {
                return Err(ContainerError::entry(name, "it has two ZIP64 fields"));
            }
            let mut out = [None; 4];
            let mut pos = 0usize;
            for (i, w) in want.iter().enumerate() {
                if *w {
                    let len = if i == 3 { 4 } else { 8 };
                    let v = body.get(pos..pos + len).ok_or_else(|| {
                        ContainerError::entry(name, "its ZIP64 field is too short")
                    })?;
                    out[i] = Some(if i == 3 {
                        u64::from(le32(v, 0))
                    } else {
                        le64(v, 0)
                    });
                    pos += len;
                }
            }
            found = Some(out);
        }
        at += 4 + n;
    }
    match found {
        Some(f) => Ok(f),
        None if want.iter().any(|w| *w) => Err(ContainerError::entry(
            name,
            "a value needs a ZIP64 field, and the entry has none",
        )),
        None => Ok([None; 4]),
    }
}

fn parse_directory(
    dir: &[u8],
    count: usize,
    dir_offset: u64,
    limits: &Limits,
) -> Result<Vec<Raw>, ContainerError> {
    let mut c = Cursor { data: dir, pos: 0 };
    let mut out = Vec::with_capacity(count.min(1 << 16));
    let mut seen = HashSet::with_capacity(count.min(1 << 16));
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
        let extra = c.take(extra_len)?;
        c.take(comment_len)?;
        let name = std::str::from_utf8(name_bytes)
            .map_err(|_| ContainerError::format("an entry name is not UTF-8 text"))?
            .to_string();
        check_name(&name, limits.max_name_bytes)?;
        if !seen.insert(name.clone()) {
            return Err(ContainerError::entry(&name, "the name is used twice"));
        }
        if flags & (1 | 1 << 6 | 1 << 13) != 0 {
            return Err(ContainerError::entry(
                &name,
                "it is encrypted, which is not supported",
            ));
        }
        let want = [
            u64::from(size) == ZIP64_AT,
            u64::from(compressed) == ZIP64_AT,
            u64::from(offset) == ZIP64_AT,
            disk == 0xFFFF,
        ];
        let big = zip64_values(extra, want, &name)?;
        if big[3].unwrap_or(u64::from(disk)) != 0 {
            return Err(ContainerError::entry(&name, "it is on another disk"));
        }
        let size = big[0].unwrap_or(u64::from(size));
        let compressed = big[1].unwrap_or(u64::from(compressed));
        let offset = big[2].unwrap_or(u64::from(offset));
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
        if offset
            .checked_add(LOCAL_LEN as u64)
            .is_none_or(|e| e > dir_offset)
        {
            return Err(ContainerError::entry(
                &name,
                "its local header is not in the file",
            ));
        }
        out.push(Raw {
            name,
            method,
            crc32,
            size,
            compressed,
            offset,
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
