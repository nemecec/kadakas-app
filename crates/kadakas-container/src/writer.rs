//! The writer: a stream of entries into any `Write` (no seek), with the manifest at the end.
//!
//! An entry that is written from bytes and stored has its CRC-32 and sizes in the local header (no
//! data descriptor: some readers, such as the Java stream reader, refuse that for a stored entry).
//! An entry that is deflated or streamed goes through the CRC-32, the SHA-256 and the encoder as it
//! is written; its sizes follow the data in a data descriptor, with 64 bit sizes and a ZIP64 extra
//! field in the local header, because the size is not known at the start. A file with more than
//! 65 534 entries, or with an offset or a size of 4 GiB or more, gets the ZIP64 end records.
//!
//! An entry that is copied from another container ([`Writer::copy_raw`]) is not decoded: the
//! compressed bytes and the CRC-32 and the hash from the manifest of the old container are written
//! as they are. The time stamps are fixed (1980-01-01), so the same content gives the same bytes.

use std::collections::HashSet;
use std::io::{Read, Write};

use flate2::Crc;
use flate2::write::DeflateEncoder;
use sha2::{Digest, Sha256};

use crate::error::ContainerError;
use crate::limits::{Limits, MANIFEST_NAME, MAX_ENTRIES, check_name};
use crate::manifest::{FORMAT, Manifest, ManifestEntry, VERSION, hex};
use crate::reader::{
    CENTRAL_LEN, CENTRAL_SIG, DESCRIPTOR_SIG, END_SIG, END64_SIG, LOCAL_SIG, LOCATOR_SIG, Method,
    RawEntry, ZIP64_AT,
};

/// How an entry is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    /// The bytes as they are (pictures: already compressed, or text that must stay readable).
    Stored,
    /// Deflate at this level, 1 (fast) to 9 (small).
    Deflate(u32),
}

const DOS_TIME: u16 = 0;
// 1980-01-01.
const DOS_DATE: u16 = (1 << 5) | 1;
const FLAG_DESCRIPTOR: u16 = 1 << 3;
const FLAG_UTF8: u16 = 1 << 11;

struct Counting<W> {
    inner: W,
    count: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.count += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct Central {
    name: String,
    method: Method,
    flags: u16,
    crc32: u32,
    compressed: u64,
    size: u64,
    offset: u64,
    /// The local header has a ZIP64 extra field.
    zip64_local: bool,
}

/// What a local header says.
struct Local<'a> {
    name: &'a str,
    method: Method,
    flags: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    /// Write the ZIP64 extra field (sizes that are 64 bit, or not known yet).
    zip64: bool,
}

/// Writes a container.
pub struct Writer<W: Write> {
    out: Counting<W>,
    central: Vec<Central>,
    manifest: Vec<ManifestEntry>,
    names: HashSet<String>,
    limits: Limits,
    generator: String,
}

/// The checks of an entry on their way into the file: CRC-32, SHA-256 and the size of the plain
/// bytes. `sink` is the file, or the encoder in front of the file.
struct Plain<S: Write> {
    sink: S,
    crc: Crc,
    sha: Sha256,
    size: u64,
}

impl<S: Write> Plain<S> {
    fn new(sink: S) -> Plain<S> {
        Plain {
            sink,
            crc: Crc::new(),
            sha: Sha256::new(),
            size: 0,
        }
    }
}

impl<S: Write> Write for Plain<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.crc.update(buf);
        self.sha.update(buf);
        self.size += buf.len() as u64;
        self.sink.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn put16(out: &mut impl Write, v: u16) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}

fn put32(out: &mut impl Write, v: u32) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}

fn put64(out: &mut impl Write, v: u64) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}

fn limit_err(what: &str) -> ContainerError {
    ContainerError::Limit(format!(
        "{what}; the container format holds at most {MAX_ENTRIES} entries. Save the plain JSON form instead"
    ))
}

/// Checks the number of entries that a program plans to write (`parts`, without the manifest),
/// before it writes anything.
pub fn check_entry_count(parts: usize) -> Result<(), ContainerError> {
    if parts + 1 > MAX_ENTRIES {
        return Err(limit_err(&format!(
            "the container would have {} entries",
            parts + 1
        )));
    }
    Ok(())
}

impl<W: Write> Writer<W> {
    pub fn new(out: W) -> Writer<W> {
        Writer {
            out: Counting {
                inner: out,
                count: 0,
            },
            central: Vec::new(),
            manifest: Vec::new(),
            names: HashSet::new(),
            limits: Limits::default(),
            generator: String::new(),
        }
    }

    /// The program that writes the container, for the manifest.
    pub fn generator(mut self, text: &str) -> Writer<W> {
        self.generator = text.to_string();
        self
    }

    /// Sets the role of an entry that was written (see `ManifestEntry::role`).
    pub fn set_role(&mut self, name: &str, role: &str) {
        if let Some(e) = self.manifest.iter_mut().find(|e| e.name == name) {
            e.role = Some(role.to_string());
        }
    }

    /// True if an entry with this name was written.
    pub fn has(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// The number of entries written so far.
    pub fn len(&self) -> usize {
        self.central.len()
    }

    /// True if no entry was written yet.
    pub fn is_empty(&self) -> bool {
        self.central.is_empty()
    }

    fn begin(&mut self, name: &str) -> Result<(), ContainerError> {
        check_name(name, self.limits.max_name_bytes)?;
        if name == MANIFEST_NAME {
            return Err(ContainerError::entry(name, "the name is for the manifest"));
        }
        if self.names.contains(name) {
            return Err(ContainerError::entry(name, "the name is used twice"));
        }
        if self.central.len() + 2 > MAX_ENTRIES {
            return Err(limit_err(&format!(
                "the container would have more than {} entries",
                MAX_ENTRIES - 1
            )));
        }
        Ok(())
    }

    fn local_header(&mut self, l: &Local<'_>) -> std::io::Result<()> {
        let out = &mut self.out;
        let (comp32, size32) = if l.zip64 {
            (ZIP64_AT as u32, ZIP64_AT as u32)
        } else {
            (l.compressed as u32, l.size as u32)
        };
        put32(out, LOCAL_SIG)?;
        put16(out, if l.zip64 { 45 } else { 20 })?;
        put16(out, l.flags | FLAG_UTF8)?;
        put16(out, l.method.code())?;
        put16(out, DOS_TIME)?;
        put16(out, DOS_DATE)?;
        put32(out, l.crc)?;
        put32(out, comp32)?;
        put32(out, size32)?;
        put16(out, l.name.len() as u16)?;
        put16(out, if l.zip64 { 20 } else { 0 })?;
        out.write_all(l.name.as_bytes())?;
        if l.zip64 {
            put16(out, 1)?;
            put16(out, 16)?;
            put64(out, l.size)?;
            put64(out, l.compressed)?;
        }
        Ok(())
    }

    /// Writes an entry. `fill` writes its bytes into the writer it is given. If `fill` fails,
    /// the error is returned and the writer must be dropped (the output is incomplete). The
    /// sizes follow the data in a data descriptor (64 bit).
    pub fn add_with(
        &mut self,
        name: &str,
        how: Compression,
        fill: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) -> Result<(), ContainerError> {
        self.begin(name)?;
        let offset = self.out.count;
        let method = match how {
            Compression::Stored => Method::Stored,
            Compression::Deflate(_) => Method::Deflate,
        };
        self.local_header(&Local {
            name,
            method,
            flags: FLAG_DESCRIPTOR,
            crc: 0,
            compressed: 0,
            size: 0,
            zip64: true,
        })?;
        let start = self.out.count;
        let (crc, sha, size) = match how {
            Compression::Stored => {
                let mut plain = Plain::new(&mut self.out);
                fill(&mut plain).map_err(ContainerError::from_io)?;
                (plain.crc.sum(), plain.sha, plain.size)
            }
            Compression::Deflate(level) => {
                let enc =
                    DeflateEncoder::new(&mut self.out, flate2::Compression::new(level.min(9)));
                let mut plain = Plain::new(enc);
                fill(&mut plain).map_err(ContainerError::from_io)?;
                let Plain {
                    sink,
                    crc,
                    sha,
                    size,
                } = plain;
                sink.finish().map_err(ContainerError::from_io)?;
                (crc.sum(), sha, size)
            }
        };
        let compressed = self.out.count - start;
        put32(&mut self.out, DESCRIPTOR_SIG)?;
        put32(&mut self.out, crc)?;
        put64(&mut self.out, compressed)?;
        put64(&mut self.out, size)?;
        self.names.insert(name.to_string());
        self.manifest.push(ManifestEntry {
            name: name.to_string(),
            size,
            sha256: hex(&sha.finalize()),
            role: None,
        });
        self.central.push(Central {
            name: name.to_string(),
            method,
            flags: FLAG_DESCRIPTOR,
            crc32: crc,
            compressed,
            size,
            offset,
            zip64_local: true,
        });
        Ok(())
    }

    /// Writes an entry from bytes. A stored entry has its CRC-32 and size in the local header.
    pub fn add_bytes(
        &mut self,
        name: &str,
        how: Compression,
        bytes: &[u8],
    ) -> Result<(), ContainerError> {
        if how != Compression::Stored {
            return self.add_with(name, how, |w| w.write_all(bytes));
        }
        self.begin(name)?;
        let offset = self.out.count;
        let mut crc = Crc::new();
        crc.update(bytes);
        let size = bytes.len() as u64;
        let zip64 = size >= ZIP64_AT;
        self.local_header(&Local {
            name,
            method: Method::Stored,
            flags: 0,
            crc: crc.sum(),
            compressed: size,
            size,
            zip64,
        })?;
        self.out.write_all(bytes)?;
        self.names.insert(name.to_string());
        self.manifest.push(ManifestEntry {
            name: name.to_string(),
            size,
            sha256: hex(&Sha256::digest(bytes)),
            role: None,
        });
        self.central.push(Central {
            name: name.to_string(),
            method: Method::Stored,
            flags: 0,
            crc32: crc.sum(),
            compressed: size,
            size,
            offset,
            zip64_local: zip64,
        });
        Ok(())
    }

    /// Copies an entry of another container without decoding it (see [`crate::Container::raw`]).
    /// The compressed bytes, the CRC-32 and the hash are those of the old container. The number
    /// of bytes copied must be the size in the old directory; if the old file is cut short, the
    /// copy fails and the writer must be dropped.
    pub fn copy_raw(&mut self, mut raw: RawEntry) -> Result<(), ContainerError> {
        self.begin(&raw.name)?;
        let offset = self.out.count;
        let zip64 = raw.compressed >= ZIP64_AT || raw.size >= ZIP64_AT;
        self.local_header(&Local {
            name: &raw.name,
            method: raw.method,
            flags: 0,
            crc: raw.crc32,
            compressed: raw.compressed,
            size: raw.size,
            zip64,
        })?;
        let copied = std::io::copy(&mut (&mut raw.data).take(raw.compressed), &mut self.out)
            .map_err(ContainerError::from_io)?;
        if copied != raw.compressed {
            return Err(ContainerError::entry(
                &raw.name,
                format!(
                    "the old file has {copied} bytes of it, not {} (the file is cut short)",
                    raw.compressed
                ),
            ));
        }
        // A checked source (a stored entry) runs its final checks when it is read to the end.
        let mut probe = [0u8; 1];
        let more = raw.data.read(&mut probe).map_err(ContainerError::from_io)?;
        if more != 0 {
            return Err(ContainerError::entry(
                &raw.name,
                "the old file has more bytes of it than its directory says",
            ));
        }
        self.names.insert(raw.name.clone());
        self.manifest.push(ManifestEntry {
            name: raw.name.clone(),
            size: raw.size,
            sha256: hex(&raw.sha256),
            role: raw.role.clone(),
        });
        self.central.push(Central {
            name: raw.name,
            method: raw.method,
            flags: 0,
            crc32: raw.crc32,
            compressed: raw.compressed,
            size: raw.size,
            offset,
            zip64_local: zip64,
        });
        Ok(())
    }

    /// Writes the manifest and the central directory, and returns the output. The output is not
    /// flushed.
    pub fn finish(mut self) -> Result<W, ContainerError> {
        let manifest = Manifest {
            format: FORMAT.to_string(),
            format_version: VERSION,
            generator: self.generator.clone(),
            entries: std::mem::take(&mut self.manifest),
        };
        let bytes = manifest.to_bytes();
        // The manifest takes the last entry slot; `begin` kept one free.
        let offset = self.out.count;
        self.local_header(&Local {
            name: MANIFEST_NAME,
            method: Method::Deflate,
            flags: FLAG_DESCRIPTOR,
            crc: 0,
            compressed: 0,
            size: 0,
            zip64: true,
        })?;
        let start = self.out.count;
        let mut crc = Crc::new();
        crc.update(&bytes);
        {
            let mut enc = DeflateEncoder::new(&mut self.out, flate2::Compression::new(6));
            enc.write_all(&bytes)?;
            enc.finish()?;
        }
        let compressed = self.out.count - start;
        put32(&mut self.out, DESCRIPTOR_SIG)?;
        put32(&mut self.out, crc.sum())?;
        put64(&mut self.out, compressed)?;
        put64(&mut self.out, bytes.len() as u64)?;
        self.central.push(Central {
            name: MANIFEST_NAME.to_string(),
            method: Method::Deflate,
            flags: FLAG_DESCRIPTOR,
            crc32: crc.sum(),
            compressed,
            size: bytes.len() as u64,
            offset,
            zip64_local: true,
        });
        let dir_offset = self.out.count;
        let mut dir_size = 0u64;
        let central = std::mem::take(&mut self.central);
        for c in &central {
            // The values that do not fit in 32 bits go to the ZIP64 extra field, in this order.
            let mut extra: Vec<u8> = Vec::new();
            let mut field = |v: u64| -> u32 {
                if v >= ZIP64_AT {
                    extra.extend_from_slice(&v.to_le_bytes());
                    ZIP64_AT as u32
                } else {
                    v as u32
                }
            };
            let size32 = field(c.size);
            let comp32 = field(c.compressed);
            let off32 = field(c.offset);
            let extra_len = if extra.is_empty() { 0 } else { extra.len() + 4 };
            let out = &mut self.out;
            put32(out, CENTRAL_SIG)?;
            put16(out, if extra_len > 0 { 45 } else { 20 })?;
            put16(out, if extra_len > 0 || c.zip64_local { 45 } else { 20 })?;
            put16(out, c.flags | FLAG_UTF8)?;
            put16(out, c.method.code())?;
            put16(out, DOS_TIME)?;
            put16(out, DOS_DATE)?;
            put32(out, c.crc32)?;
            put32(out, comp32)?;
            put32(out, size32)?;
            put16(out, c.name.len() as u16)?;
            put16(out, extra_len as u16)?;
            put16(out, 0)?;
            put16(out, 0)?;
            put16(out, 0)?;
            put32(out, 0)?;
            put32(out, off32)?;
            out.write_all(c.name.as_bytes())?;
            if extra_len > 0 {
                put16(out, 1)?;
                put16(out, extra.len() as u16)?;
                out.write_all(&extra)?;
            }
            dir_size += (CENTRAL_LEN + c.name.len() + extra_len) as u64;
        }
        let count = central.len() as u64;
        let big = count >= 0xFFFF || dir_size >= ZIP64_AT || dir_offset >= ZIP64_AT;
        if big {
            let rec_at = self.out.count;
            let out = &mut self.out;
            put32(out, END64_SIG)?;
            put64(out, 44)?;
            put16(out, 45)?;
            put16(out, 45)?;
            put32(out, 0)?;
            put32(out, 0)?;
            put64(out, count)?;
            put64(out, count)?;
            put64(out, dir_size)?;
            put64(out, dir_offset)?;
            put32(out, LOCATOR_SIG)?;
            put32(out, 0)?;
            put64(out, rec_at)?;
            put32(out, 1)?;
        }
        let out = &mut self.out;
        put32(out, END_SIG)?;
        put16(out, 0)?;
        put16(out, 0)?;
        let c16 = if count >= 0xFFFF { 0xFFFF } else { count as u16 };
        put16(out, c16)?;
        put16(out, c16)?;
        put32(out, dir_size.min(ZIP64_AT) as u32)?;
        put32(out, dir_offset.min(ZIP64_AT) as u32)?;
        put16(out, 0)?;
        Ok(self.out.inner)
    }
}
