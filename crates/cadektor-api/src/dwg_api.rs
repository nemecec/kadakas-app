//! `import.file` and `export.file` with the format `dwg`, through an external converter.
//! Native only: they start a program and read and write files. See `cadektor_io::dwg` and `docs/dwg.md`.

use std::path::{Path, PathBuf};

use cadektor_io::dwg::{self, DwgError, RunOptions, Search};
use cadektor_io::{ExportOptions, ImportOptions};
use serde_json::{Value, json};

use crate::{RpcError, Session};

/// True if the request asks for DWG: the format is `dwg`, or there is no format and the file ends in `.dwg`.
pub(crate) fn is_dwg(format: Option<&str>, path: &Path) -> bool {
    match format {
        Some(f) => f.trim().trim_start_matches('.').eq_ignore_ascii_case("dwg"),
        None => path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dwg")),
    }
}

/// What `describe` says about DWG.
pub(crate) fn describe() -> Value {
    let search = Search::system();
    let found = dwg::detect(&search);
    json!({
        "format": "dwg through an external converter that the user installs: the ODA File Converter, or the LibreDWG tools dwg2dxf and dxf2dwg (separate programs, never linked). Import runs DWG to DXF and then the DXF importer. Export writes DXF and then runs DXF to DWG.",
        "converter": found.as_ref().map_or(Value::Null, |c| json!({
            "name": c.name(),
            "program": c.program(),
            "can_write": c.can_write(),
        })),
        "options": {
            "...": "All options of the DXF import and export apply (see import_options and export_options)",
            "converter_timeout_s": "seconds before the converter is stopped. Default 300",
            "dwg_version": "export: file version for the ODA File Converter, for example ACAD2018 (default), ACAD2013, ACAD2010, ACAD2007, ACAD2004, ACAD2000"
        },
        "agent_policy": "Agents may use DWG only if the host started with --allow-converters (cadektor serve, cadektor mcp and the app). Default: off. The file dialogs of the app are not limited. Without the flag the call fails with error code 4 (kind policy).",
        "setup": "Install the ODA File Converter (free download, own license) or LibreDWG (GPL, a package of your system). Cadektor finds ODAFileConverter in its standard install folder or on PATH, then dwg2dxf and dxf2dwg on PATH. CADEKTOR_DWG_CONVERTER names a program explicitly. See docs/dwg.md. Without a converter the call fails with an error that says so.",
        "examples": [
            {"method": "import.file", "params": {"path": "/data/survey.dwg", "options": {"layer_mode": "layers"}}},
            {"method": "export.file", "params": {"path": "/data/out.dwg", "format": "dwg", "overwrite": true}}
        ]
    })
}

/// The error for an agent request that would start a converter while the host did not allow it.
fn converters_off() -> RpcError {
    RpcError::policy(
        "DWG needs an external converter program, and this host does not let agents start one. The owner can allow it with --allow-converters",
    )
}

fn rpc(e: DwgError) -> RpcError {
    match e {
        DwgError::Option(_) => RpcError::invalid_params(e),
        DwgError::Timeout { .. } => RpcError::limit(e),
        DwgError::Format(_) => RpcError::validation(e),
        e => RpcError::app(e),
    }
}

fn io_rpc(e: cadektor_io::IoError) -> RpcError {
    match e {
        cadektor_io::IoError::Option(_) => RpcError::invalid_params(e),
        e => RpcError::app(e),
    }
}

impl Session {
    pub(crate) fn import_dwg_file(
        &mut self,
        path: PathBuf,
        options: &Value,
        checked: bool,
    ) -> Result<Value, RpcError> {
        if checked && !self.paths.converters_allowed() {
            return Err(converters_off());
        }
        let opts = ImportOptions::from_json(options).map_err(io_rpc)?;
        let run = RunOptions::from_json(options).map_err(rpc)?;
        let path = if checked {
            self.check_path(&path, false, &["dwg"])
                .map_err(RpcError::policy)?
        } else {
            path
        };
        let result = dwg::import_file(&path, &opts, &run, self.editor.doc(), &Search::system())
            .map_err(rpc)?;
        self.editor
            .execute(&result.command)
            .map_err(RpcError::from)?;
        self.prune_selection();
        Ok(json!({
            "imported": result.report.total(),
            "skipped": result.report.skipped_total(),
            "report": result.report,
            "georeference": self.editor.doc().georef,
        }))
    }

    pub(crate) fn export_dwg_file(
        &mut self,
        path: PathBuf,
        options: &Value,
        overwrite: bool,
        checked: bool,
    ) -> Result<Value, RpcError> {
        if checked && !self.paths.converters_allowed() {
            return Err(converters_off());
        }
        let opts = ExportOptions::from_json(options).map_err(io_rpc)?;
        let run = RunOptions::from_json(options).map_err(rpc)?;
        let path = if checked {
            self.check_path(&path, true, &["dwg"])
                .map_err(RpcError::policy)?
        } else {
            path
        };
        if checked && self.paths.is_restricted() && path.exists() && !overwrite {
            return Err(RpcError::conflict(format!(
                "{} exists: pass \"overwrite\": true to replace it",
                path.display()
            )));
        }
        let report = dwg::export_file(&path, &opts, &run, self.editor.doc(), &Search::system())
            .map_err(rpc)?;
        Ok(json!({ "path": path, "exported": report.total(), "report": report }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_dwg_requests() {
        assert!(is_dwg(Some("DWG"), Path::new("a.dxf")));
        assert!(is_dwg(Some(".dwg"), Path::new("a")));
        assert!(is_dwg(None, Path::new("plan.DWG")));
        assert!(!is_dwg(None, Path::new("plan.dxf")));
        assert!(!is_dwg(Some("dxf"), Path::new("a.dwg")));
        assert!(describe()["setup"].is_string());
    }
}
