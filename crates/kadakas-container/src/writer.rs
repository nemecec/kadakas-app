//! The writer: a stream of entries into any `Write` (no seek), with the manifest at the end.
//!
//! An entry that is written from data goes through the CRC-32, the SHA-256 and (for deflate) the
//! encoder as it is written; its sizes follow the data in a data descriptor. An entry that is
//! copied from another container ([`Writer::copy_raw`]) is not decoded: the compressed bytes and
//! the CRC-32 and the hash from the manifest of the old container are written as they are.
//! The time stamps are fixed (1980-01-01), so the same content gives the same bytes.

use std::collections::HashSet;
use std::io::{Read, Write};

use flate2::Crc;
use flate2::write::DeflateEncoder;
use sha2::{Digest, Sha256};

use crate::error::ContainerError;
use crate::limits::{Limits, MANIFEST_NAME, MAX_ENTRIES, MAX_FIELD, check_name};
use crate::manifest::{FORMAT, Manifest, ManifestEntry, VERSION, hex};
use crate::reader::{
    CENTRAL_LEN, CENTRAL_SIG, DESCRIPTOR_SIG, END_SIG, LOCAL_SIG, Method, RawEntry,
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
    compressed: u32,
    size: u32,
    offset: u32,
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
        if self.size + buf.len() as u64 > MAX_FIELD {
            return Err(ContainerError::Limit(format!(
                "an entry is bigger than {MAX_FIELD} bytes, the most that the container format holds (ZIP64 is not supported)"
            ))
            .into());
        }
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

fn limit_err(what: &str) -> ContainerError {
    ContainerError::Limit(format!(
        "{what}; the container format holds at most {MAX_ENTRIES} entries and {MAX_FIELD} bytes (ZIP64 is not supported). Save the plain JSON form instead"
    ))
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
        if self.out.count > MAX_FIELD - 1024 * 1024 {
            return Err(limit_err("the file would be bigger than 4 GiB"));
        }
        Ok(())
    }

    fn local_header(
        &mut self,
        name: &str,
        method: Method,
        flags: u16,
        crc: u32,
        compressed: u32,
        size: u32,
    ) -> std::io::Result<()> {
        let out = &mut self.out;
        put32(out, LOCAL_SIG)?;
        put16(out, 20)?;
        put16(out, flags | FLAG_UTF8)?;
        put16(out, method.code())?;
        put16(out, DOS_TIME)?;
        put16(out, DOS_DATE)?;
        put32(out, crc)?;
        put32(out, compressed)?;
        put32(out, size)?;
        put16(out, name.len() as u16)?;
        put16(out, 0)?;
        out.write_all(name.as_bytes())
    }

    /// Writes an entry. `fill` writes its bytes into the writer it is given. If `fill` fails,
    /// the error is returned and the writer must be dropped (the output is incomplete).
    pub fn add_with(
        &mut self,
        name: &str,
        how: Compression,
        fill: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) -> Result<(), ContainerError> {
        self.begin(name)?;
        let offset = u32::try_from(self.out.count).map_err(|_| limit_err("the file is too big"))?;
        let method = match how {
            Compression::Stored => Method::Stored,
            Compression::Deflate(_) => Method::Deflate,
        };
        self.local_header(name, method, FLAG_DESCRIPTOR, 0, 0, 0)?;
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
                sink.finish()?;
                (crc.sum(), sha, size)
            }
        };
        let compressed = self.out.count - start;
        if compressed > MAX_FIELD || self.out.count > MAX_FIELD {
            return Err(limit_err("the file would be bigger than 4 GiB"));
        }
        let (c32, s32) = (compressed as u32, size as u32);
        put32(&mut self.out, DESCRIPTOR_SIG)?;
        put32(&mut self.out, crc)?;
        put32(&mut self.out, c32)?;
        put32(&mut self.out, s32)?;
        self.finish_entry(
            name,
            method,
            FLAG_DESCRIPTOR,
            crc,
            c32,
            s32,
            offset,
            size,
            sha,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_entry(
        &mut self,
        name: &str,
        method: Method,
        flags: u16,
        crc32: u32,
        compressed: u32,
        size32: u32,
        offset: u32,
        size: u64,
        sha: Sha256,
    ) {
        self.names.insert(name.to_string());
        self.manifest.push(ManifestEntry {
            name: name.to_string(),
            size,
            sha256: hex(&sha.finalize()),
        });
        self.central.push(Central {
            name: name.to_string(),
            method,
            flags,
            crc32,
            compressed,
            size: size32,
            offset,
        });
    }

    /// Writes an entry from bytes.
    pub fn add_bytes(
        &mut self,
        name: &str,
        how: Compression,
        bytes: &[u8],
    ) -> Result<(), ContainerError> {
        self.add_with(name, how, |w| w.write_all(bytes))
    }

    /// Copies an entry of another container without decoding it (see [`crate::Container::raw`]).
    /// The compressed bytes, the CRC-32 and the hash are those of the old container. The number
    /// of bytes copied must be the size in the old directory; if the old file is cut short, the
    /// copy fails and the writer must be dropped.
    pub fn copy_raw(&mut self, mut raw: RawEntry) -> Result<(), ContainerError> {
        self.begin(&raw.name)?;
        if raw.compressed > MAX_FIELD || raw.size > MAX_FIELD {
            return Err(limit_err("an entry is too big"));
        }
        let offset = u32::try_from(self.out.count).map_err(|_| limit_err("the file is too big"))?;
        self.local_header(
            &raw.name,
            raw.method,
            0,
            raw.crc32,
            raw.compressed as u32,
            raw.size as u32,
        )?;
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
        if self.out.count > MAX_FIELD {
            return Err(limit_err("the file would be bigger than 4 GiB"));
        }
        self.names.insert(raw.name.clone());
        self.manifest.push(ManifestEntry {
            name: raw.name.clone(),
            size: raw.size,
            sha256: hex(&raw.sha256),
        });
        self.central.push(Central {
            name: raw.name,
            method: raw.method,
            flags: 0,
            crc32: raw.crc32,
            compressed: raw.compressed as u32,
            size: raw.size as u32,
            offset,
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
        let offset = u32::try_from(self.out.count).map_err(|_| limit_err("the file is too big"))?;
        self.local_header(MANIFEST_NAME, Method::Deflate, FLAG_DESCRIPTOR, 0, 0, 0)?;
        let start = self.out.count;
        let mut crc = Crc::new();
        crc.update(&bytes);
        {
            let mut enc = DeflateEncoder::new(&mut self.out, flate2::Compression::new(6));
            enc.write_all(&bytes)?;
            enc.finish()?;
        }
        let compressed = (self.out.count - start) as u32;
        put32(&mut self.out, DESCRIPTOR_SIG)?;
        put32(&mut self.out, crc.sum())?;
        put32(&mut self.out, compressed)?;
        put32(&mut self.out, bytes.len() as u32)?;
        self.central.push(Central {
            name: MANIFEST_NAME.to_string(),
            method: Method::Deflate,
            flags: FLAG_DESCRIPTOR,
            crc32: crc.sum(),
            compressed,
            size: bytes.len() as u32,
            offset,
        });
        let dir_offset =
            u32::try_from(self.out.count).map_err(|_| limit_err("the file is too big"))?;
        let mut dir_size = 0u64;
        let central = std::mem::take(&mut self.central);
        for c in &central {
            let out = &mut self.out;
            put32(out, CENTRAL_SIG)?;
            put16(out, 20)?;
            put16(out, 20)?;
            put16(out, c.flags | FLAG_UTF8)?;
            put16(out, c.method.code())?;
            put16(out, DOS_TIME)?;
            put16(out, DOS_DATE)?;
            put32(out, c.crc32)?;
            put32(out, c.compressed)?;
            put32(out, c.size)?;
            put16(out, c.name.len() as u16)?;
            put16(out, 0)?;
            put16(out, 0)?;
            put16(out, 0)?;
            put16(out, 0)?;
            put32(out, 0)?;
            put32(out, c.offset)?;
            out.write_all(c.name.as_bytes())?;
            dir_size += CENTRAL_LEN as u64 + c.name.len() as u64;
        }
        if self.out.count > MAX_FIELD {
            return Err(limit_err("the file would be bigger than 4 GiB"));
        }
        let out = &mut self.out;
        put32(out, END_SIG)?;
        put16(out, 0)?;
        put16(out, 0)?;
        put16(out, central.len() as u16)?;
        put16(out, central.len() as u16)?;
        put32(out, dir_size as u32)?;
        put32(out, dir_offset)?;
        put16(out, 0)?;
        Ok(self.out.inner)
    }
}
