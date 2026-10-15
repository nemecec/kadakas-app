//! File formats for Kadakas: DXF, Shapefile and GeoJSON.
//!
//! The core never parses files. An [`Importer`] turns the bytes of a file into
//! one `Command::Batch` plus a [`Report`]. The caller runs the batch, so an
//! import is one undo step. An [`Exporter`] turns a `Document` into bytes.
//!
//! All readers are tolerant. They never panic on bad input. They return an
//! error for files that are not of the format, and they skip single bad
//! entities with a reason in the report.

pub mod builder;
pub mod dwg;
pub mod dxf;
mod encoding;
pub use encoding::decode_text;
pub mod geojson;
pub mod gis;
pub mod options;
pub mod report;
pub mod shapefile;

use std::collections::BTreeMap;
use std::path::Path;

use kadakas_core::command::Command;
use kadakas_core::model::{ClassId, Document, Layer, LayerId};
use kadakas_core::provenance::FileLink;

pub use options::{
    BaseCaps, BaseMode, ExportOptions, ExportOrigin, FaceMode, ImportOptions, LayerMode, Lineage,
    MeshEntity, OriginMode, SiteMesh, Unit,
};
pub use report::{Report, Skipped};

/// A file format that this crate can read or write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Dxf,
    Shapefile,
    GeoJson,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Dxf => "dxf",
            Format::Shapefile => "shapefile",
            Format::GeoJson => "geojson",
        }
    }

    /// Finds a format by name (`dxf`, `shp`, `shapefile`, `geojson`, `json`).
    pub fn from_name(name: &str) -> Option<Format> {
        match name
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "dxf" => Some(Format::Dxf),
            "shp" | "shapefile" => Some(Format::Shapefile),
            "geojson" | "json" => Some(Format::GeoJson),
            _ => None,
        }
    }

    /// Finds a format by file extension.
    pub fn from_path(path: &Path) -> Option<Format> {
        Format::from_name(path.extension()?.to_str()?)
    }
}

/// Why a file could not be read or written.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IoError {
    /// The bytes are not a file of this format.
    #[error("{0}")]
    Format(String),
    /// A file could not be read or written.
    #[error("{0}")]
    File(String),
    /// An option has a bad value.
    #[error("{0}")]
    Option(String),
    /// The update data of the drawing does not fit the limits of the document, and the option
    /// `base` is `auto`. The person (or the agent) must choose `link` or `none`.
    #[error("{}", .limit.message(*.entities))]
    BaseChoice { entities: usize, limit: BaseLimit },
}

/// Which limit of the update data a drawing is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseLimit {
    /// More entities than `cap`.
    TooMany { cap: usize },
    /// The summaries need `bytes` of JSON, more than the `cap` bytes that a base may hold.
    TooLarge { bytes: u64, cap: u64 },
}

impl BaseLimit {
    /// The message for the person or the agent, with the choices.
    pub fn message(&self, entities: usize) -> String {
        const CHOICES: &str = "Set the import option base to \"link\" (keep a link to the original file for updates) or \"none\" (import without update data)";
        match self {
            BaseLimit::TooMany { cap } => format!(
                "this drawing has {entities} entities, more than the {cap} that Kadakas keeps for updates. {CHOICES}"
            ),
            BaseLimit::TooLarge { bytes, cap } => format!(
                "the summaries of the {entities} entities of this drawing need {bytes} bytes, more than the {cap} bytes that Kadakas keeps for updates. {CHOICES}"
            ),
        }
    }
}

/// The file to import: its name, its bytes, and files next to it (a shapefile has
/// `dbf` and `prj` parts). The keys of `extra` are lower-case extensions.
#[derive(Clone, Debug, Default)]
pub struct Source {
    /// File name without extension.
    pub name: String,
    pub data: Vec<u8>,
    pub extra: BTreeMap<String, Vec<u8>>,
}

impl Source {
    /// Reads a file, and for a shapefile its `dbf`, `prj` and `cpg` parts.
    pub fn read(path: &Path) -> Result<Source, IoError> {
        Source::read_limited(path, u64::MAX)
    }

    /// Like [`Source::read`], but refuses a file bigger than `max_bytes`. The size is checked
    /// before the file is read.
    pub fn read_limited(path: &Path, max_bytes: u64) -> Result<Source, IoError> {
        let data =
            read_regular(path, max_bytes, &path.display().to_string()).map_err(IoError::File)?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut extra = BTreeMap::new();
        if Format::from_path(path) == Some(Format::Shapefile) {
            for ext in ["dbf", "prj", "cpg"] {
                for e in [ext.to_string(), ext.to_ascii_uppercase()] {
                    if let Ok(bytes) = std::fs::read(path.with_extension(&e)) {
                        extra.insert(ext.to_string(), bytes);
                        break;
                    }
                }
            }
        }
        Ok(Source { name, data, extra })
    }
}

/// Reads a regular file of at most `max_bytes` bytes (the capped reader of `kadakas_sys::fsio`):
/// the kind and the size are checked before the file is read, and a file that grows while it is
/// read ends in an error. `what` names the file in the message.
pub(crate) fn read_regular(path: &Path, max_bytes: u64, what: &str) -> Result<Vec<u8>, String> {
    use kadakas_sys::fsio::{OpenError, open_regular};
    use std::io::Read;
    let mut reader = open_regular(path, max_bytes).map_err(|e| match e {
        OpenError::TooLarge { len, max } => {
            format!("{what} is {len} bytes, more than the limit of {max} (option max_bytes)")
        }
        e => format!("cannot read {what}: {e}"),
    })?;
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| format!("cannot read {what}: {e} (option max_bytes)"))?;
    Ok(data)
}

/// What an importer needs to know about the document that receives the import.
/// It lets the importer plan IDs and reuse layers and classes by name.
#[derive(Clone, Debug)]
pub struct Context {
    pub ids: kadakas_core::uid::IdGen,
    pub layers: Vec<Layer>,
    pub classes: Vec<(ClassId, String)>,
    pub active_layer: LayerId,
    pub active_class: ClassId,
    pub georef: Option<kadakas_core::georef::Georeference>,
    /// Layers that are grey (an import onto one lets it go visible and makes it grey again).
    pub grey_layers: Vec<LayerId>,
    /// The (drawing, revision) of the import bases that the document keeps.
    pub bases: Vec<(String, String)>,
}

impl Context {
    pub fn of(doc: &Document) -> Context {
        Context {
            ids: doc.id_plan(),
            layers: doc.layers.clone(),
            classes: doc.classes.iter().map(|c| (c.id, c.name.clone())).collect(),
            active_layer: doc.active_layer,
            active_class: doc.active_class,
            georef: doc.georef.clone(),
            grey_layers: doc.drawing.grey.layers.iter().copied().collect(),
            bases: doc
                .resources
                .iter()
                .filter_map(|r| match &r.body {
                    kadakas_core::resource::ResourceBody::ImportBase(v) => Some((
                        v["drawing"].as_str()?.to_string(),
                        v["revision"].as_str()?.to_string(),
                    )),
                    _ => None,
                })
                .collect(),
        }
    }
}

/// The result of an import: one batch to execute, and the report.
#[derive(Clone, Debug)]
pub struct ImportResult {
    /// A `Command::Batch`. Executing it is one undo step.
    pub command: Command,
    pub report: Report,
}

/// Reads one file format.
pub trait Importer {
    fn format(&self) -> Format;
    fn import(
        &self,
        src: &Source,
        ctx: &Context,
        opts: &ImportOptions,
    ) -> Result<ImportResult, IoError>;
}

/// The result of an export.
#[derive(Clone, Debug)]
pub struct ExportResult {
    pub bytes: Vec<u8>,
    pub report: Report,
}

/// Writes one file format.
pub trait Exporter {
    fn format(&self) -> Format;
    fn export(&self, doc: &Document, opts: &ExportOptions) -> Result<ExportResult, IoError>;
}

/// The importer for a format.
pub fn importer(format: Format) -> Box<dyn Importer> {
    match format {
        Format::Dxf => Box::new(dxf::DxfFormat),
        Format::Shapefile => Box::new(shapefile::ShapefileFormat),
        Format::GeoJson => Box::new(geojson::GeoJsonFormat),
    }
}

/// The model of a document: without sheet layers and their objects. Sheets are paper space (mm on paper),
/// so exporters must never write them as model geometry.
pub(crate) fn model_only(doc: &Document) -> std::borrow::Cow<'_, Document> {
    use kadakas_core::model::LayerKind;
    if !doc.layers.iter().any(|l| l.kind == LayerKind::Sheet) {
        return std::borrow::Cow::Borrowed(doc);
    }
    let mut d = doc.clone();
    let sheets: Vec<_> = d
        .layers
        .iter()
        .filter(|l| l.kind == LayerKind::Sheet)
        .map(|l| l.id)
        .collect();
    d.objects.retain(|_, o| !sheets.contains(&o.layer));
    d.layers.retain(|l| l.kind != LayerKind::Sheet);
    if d.layer(d.active_layer).is_none() {
        d.active_layer = d.layers[0].id;
    }
    std::borrow::Cow::Owned(d)
}

/// The exporter for a format, if the format can be written.
pub fn exporter(format: Format) -> Option<Box<dyn Exporter>> {
    match format {
        Format::Dxf => Some(Box::new(dxf::DxfFormat)),
        Format::GeoJson => Some(Box::new(geojson::GeoJsonFormat)),
        Format::Shapefile => None,
    }
}

/// Reads a file and plans the import into `doc`. `format` defaults to the file extension.
pub fn import_file(
    path: &Path,
    format: Option<Format>,
    opts: &ImportOptions,
    doc: &Document,
) -> Result<ImportResult, IoError> {
    let format = format.or_else(|| Format::from_path(path)).ok_or_else(|| {
        IoError::Format("unknown file type: give a format (dxf, shp or geojson)".into())
    })?;
    let src = Source::read_limited(path, opts.max_bytes)?;
    // A link to the original file is made only when it is asked for (it needs the hash).
    let link = (format == Format::Dxf && opts.track_source && opts.base == BaseMode::Link)
        .then(|| {
            let abs = absolute_path_text(path)?;
            let link = FileLink::of_bytes(&abs, &src.data);
            match link.problem() {
                Some(why) => Err(IoError::Option(format!(
                    "cannot keep a link to the file: {why}"
                ))),
                None => Ok(link),
            }
        })
        .transpose()?;
    // The tags of imported objects name the file with its extension.
    let named;
    let opts = match path.file_name() {
        Some(n) if opts.source_name.is_none() => {
            named = ImportOptions {
                source_name: Some(n.to_string_lossy().into_owned()),
                ..opts.clone()
            };
            &named
        }
        _ => opts,
    };
    match (format, &link) {
        (Format::Dxf, Some(_)) => dxf::import_linked(&src, &Context::of(doc), opts, link.as_ref()),
        _ => importer(format).import(&src, &Context::of(doc), opts),
    }
}

/// The link to a file for a drawing that keeps a link and not the summaries of its entities
/// (`base: "link"`): the absolute path, the size and the SHA-256 of `data`, which must be the bytes
/// of the file. An update uses it for the new revision (IOP-23).
pub fn file_link(path: &Path, data: &[u8]) -> Result<FileLink, IoError> {
    let link = FileLink::of_bytes(&absolute_path_text(path)?, data);
    match link.problem() {
        Some(why) => Err(IoError::Option(format!(
            "cannot keep a link to the file: {why}"
        ))),
        None => Ok(link),
    }
}

/// The absolute path of `path` as text, for a [`FileLink`]: the canonical path, else the path made
/// absolute. A path that is not text cannot be kept.
pub(crate) fn absolute_path_text(path: &Path) -> Result<String, IoError> {
    let abs = std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .map_err(|e| {
            IoError::Option(format!(
                "cannot keep a link to {}: no absolute path ({e})",
                path.display()
            ))
        })?;
    abs.to_str().map(strip_verbatim).ok_or_else(|| {
        IoError::Option(format!(
            "cannot keep a link to {}: the path is not text",
            path.display()
        ))
    })
}

/// The canonical path on Windows has the prefix `\\?\` (`\\?\C:\x`, `\\?\UNC\server\share`). The
/// link keeps the usual form (`C:\x`, `\\server\share`), which a person can read and which the
/// checks of a link know.
pub(crate) fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = path.strip_prefix("\\\\?\\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// Exports `doc` to a file. `format` defaults to the file extension.
pub fn export_file(
    path: &Path,
    format: Option<Format>,
    opts: &ExportOptions,
    doc: &Document,
) -> Result<Report, IoError> {
    let format = format.or_else(|| Format::from_path(path)).ok_or_else(|| {
        IoError::Format("unknown file type: give a format (dxf or geojson)".into())
    })?;
    let ex = exporter(format)
        .ok_or_else(|| IoError::Format(format!("export to {} is not supported", format.name())))?;
    let out = ex.export(doc, opts)?;
    // Atomic: a crash never leaves a half-written file. An old file stays as `<name>.bak`.
    kadakas_sys::fsio::write_atomic(path, &out.bytes)
        .map_err(|e| IoError::File(format!("cannot write {}: {e}", path.display())))?;
    Ok(out.report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verbatim_prefix_of_a_windows_path_is_taken_off() {
        for (given, want) in [
            ("\\\\?\\C:\\data\\a.dxf", "C:\\data\\a.dxf"),
            (
                "\\\\?\\UNC\\server\\share\\a.dxf",
                "\\\\server\\share\\a.dxf",
            ),
            ("\\\\?\\Volume{1}\\a.dxf", "\\\\?\\Volume{1}\\a.dxf"),
            ("C:\\data\\a.dxf", "C:\\data\\a.dxf"),
            ("/data/a.dxf", "/data/a.dxf"),
        ] {
            assert_eq!(strip_verbatim(given), want, "{given}");
        }
    }

    #[test]
    fn a_link_to_a_file_is_made_from_its_bytes_and_a_bad_path_is_refused() {
        let dir = kadakas_sys::tmp::PrivateTempDir::new("kadakas-io-file-link").unwrap();
        let path = dir.path().join("plan.dxf");
        std::fs::write(&path, b"0\nEOF\n").unwrap();
        let link = file_link(&path, b"0\nEOF\n").unwrap();
        assert!(link.path.ends_with("plan.dxf") && link.bytes == 6);
        assert_eq!(link.problem(), None);
        // A file that is not there yet still has an absolute path.
        assert!(file_link(&dir.path().join("later.dxf"), b"x").is_ok());
        // A path that is too long is not kept.
        let long = dir.path().join("a".repeat(5000));
        let err = file_link(&long, b"x").unwrap_err();
        assert!(err.to_string().contains("cannot keep a link"), "{err}");
    }

    #[test]
    fn formats_by_name_and_path() {
        assert_eq!(Format::from_name(".DXF"), Some(Format::Dxf));
        assert_eq!(Format::from_name("shp"), Some(Format::Shapefile));
        assert_eq!(Format::from_name("json"), Some(Format::GeoJson));
        assert_eq!(Format::from_name("png"), None);
        assert_eq!(
            Format::from_path(Path::new("a/b.GeoJSON")),
            Some(Format::GeoJson)
        );
        assert_eq!(Format::from_path(Path::new("noext")), None);
        for f in [Format::Dxf, Format::Shapefile, Format::GeoJson] {
            assert_eq!(Format::from_name(f.name()), Some(f));
            assert_eq!(importer(f).format(), f);
        }
        assert!(exporter(Format::Shapefile).is_none());
        assert_eq!(exporter(Format::Dxf).unwrap().format(), Format::Dxf);
    }

    #[test]
    fn a_file_over_the_size_limit_is_refused_before_it_is_read() {
        let dir = kadakas_sys::tmp::PrivateTempDir::new("kadakas-io-size").unwrap();
        let path = dir.path().join("big.dxf");
        std::fs::write(&path, "0\nSECTION\n").unwrap();
        let doc = Document::with_sequential_ids();
        let small = ImportOptions {
            max_bytes: 4,
            ..ImportOptions::default()
        };
        let e = import_file(&path, None, &small, &doc).unwrap_err();
        assert!(
            matches!(&e, IoError::File(m) if m.contains("max_bytes")),
            "{e}"
        );
        assert!(Source::read_limited(&path, 4).is_err());
        // Only a regular file is read.
        let e = Source::read_limited(dir.path(), 100).unwrap_err();
        assert!(
            matches!(&e, IoError::File(m) if m.contains("regular file")),
            "{e}"
        );
        assert!(Source::read_limited(&dir.path().join("none.dxf"), 100).is_err());
        assert_eq!(Source::read_limited(&path, 100).unwrap().data.len(), 10);
        // The name with its extension goes to the tags unless the caller names the source.
        let r = import_file(&path, None, &ImportOptions::default(), &doc).unwrap();
        assert!(r.report.drawing.unwrap().starts_with("big.dxf#"));
    }

    #[test]
    fn file_errors() {
        let doc = Document::with_sequential_ids();
        let o = ImportOptions::default();
        assert!(matches!(
            import_file(Path::new("/nonexistent/x.dxf"), None, &o, &doc),
            Err(IoError::File(_))
        ));
        assert!(matches!(
            import_file(Path::new("/nonexistent/x"), None, &o, &doc),
            Err(IoError::Format(_))
        ));
        let e = ExportOptions::default();
        assert!(matches!(
            export_file(Path::new("x"), None, &e, &doc),
            Err(IoError::Format(_))
        ));
        assert!(matches!(
            export_file(Path::new("x.shp"), None, &e, &doc),
            Err(IoError::Format(_))
        ));
        assert!(matches!(
            export_file(Path::new("/nonexistent/dir/x.dxf"), None, &e, &doc),
            Err(IoError::File(_))
        ));
    }
}
