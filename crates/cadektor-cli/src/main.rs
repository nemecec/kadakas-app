//! Headless Cadektor for scripts and AI agents.
//!
//! - `cadektor serve`: JSON-RPC 2.0 on stdin/stdout, one request per line.
//! - `cadektor mcp`: Model Context Protocol server on stdio, for Claude Code.
//! - `cadektor render <file> -o out.svg`: render a document to SVG.
//! - `cadektor import <file> -o doc.json`: import a DXF, Shapefile or GeoJSON file into a document.
//! - `cadektor export <doc> -o out.dxf`: export a document to DXF or GeoJSON.
//!
//! With `--connect`, `serve` and `mcp` forward every request to a running
//! desktop app instead of a headless document. `--connect` alone finds the
//! app through its runtime file. `--connect HOST:PORT` needs `--token` (or the
//! runtime file). The first message on the connection is always `auth`.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;

use cadektor_api::{Session, METHODS};
use cadektor_core::fsio::{self, AgentInfo};
use cadektor_core::model::Document;
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "cadektor", version, about = "Cadektor CAD, headless")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// JSON-RPC 2.0 over stdio.
    Serve {
        /// Open this document at start.
        file: Option<PathBuf>,
        #[command(flatten)]
        remote: RemoteArgs,
    },
    /// MCP server over stdio.
    Mcp {
        file: Option<PathBuf>,
        #[command(flatten)]
        remote: RemoteArgs,
    },
    /// Render a document to SVG.
    Render {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value_t = 1200)]
        width: u32,
    },
    /// Import a DXF, Shapefile (.shp) or GeoJSON file. Saves the document and prints the report.
    Import {
        file: PathBuf,
        /// The document to write.
        #[arg(short, long)]
        output: PathBuf,
        /// Import into this document instead of an empty one.
        #[arg(long)]
        into: Option<PathBuf>,
        /// dxf, shp or geojson. Default: from the file name.
        #[arg(long)]
        format: Option<String>,
        /// Import options as JSON, for example '{"layer_mode": "layers"}'.
        #[arg(long)]
        options: Option<String>,
    },
    /// Export a document to DXF, GeoJSON or IFC4 (.ifc). Prints the report. The input file is never
    /// changed. GlobalIds come from the project GUID of the document.
    Export {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// dxf, geojson or ifc. Default: from the file name.
        #[arg(long)]
        format: Option<String>,
        /// Export options as JSON, for example '{"unit": "m"}' or, for IFC, '{"layers_as_storeys": true}'.
        #[arg(long)]
        options: Option<String>,
    },
}

#[derive(clap::Args)]
struct RemoteArgs {
    /// Forward to a running app. Without a value, find the app through its
    /// runtime file. Or give HOST:PORT, for example 127.0.0.1:7878.
    #[arg(long, num_args = 0..=1, default_missing_value = "")]
    connect: Option<String>,
    /// The app's access token, if it is not in the runtime file.
    #[arg(long, requires = "connect")]
    token: Option<String>,
    /// A folder that agents may read and write (repeat for more). Default: the current folder.
    /// The folder of the open document is always allowed. Not used with --connect: the app decides.
    #[arg(long = "allow-dir", value_name = "DIR")]
    allow_dir: Vec<PathBuf>,
}

/// Where requests go: a local document, or a running app over TCP.
enum Backend {
    Local(Box<Session>),
    Remote(BufReader<TcpStream>),
}

impl Backend {
    fn new(file: Option<PathBuf>, remote: RemoteArgs) -> Result<Self, String> {
        if let Some(connect) = remote.connect {
            let info = fsio::agent_info_path().and_then(|p| fsio::read_agent_info(&p).ok());
            let (addr, token) = resolve_remote(&connect, remote.token.as_deref(), info)?;
            return connect_remote(&addr, &token).map(Backend::Remote);
        }
        let mut session = Session::default();
        // Requests come from agents: they may only use the allowed folders.
        let dirs = if remote.allow_dir.is_empty() { vec![PathBuf::from(".")] } else { remote.allow_dir };
        session.paths = cadektor_api::PathPolicy::restricted(dirs);
        if let Some(f) = file {
            session.editor = cadektor_core::command::Editor::new(load(&f)?);
            session.doc_dir = doc_dir_of(&f);
        }
        Ok(Backend::Local(Box::new(session)))
    }

    /// Calls one method and returns the JSON-RPC response object.
    fn call(&mut self, id: Value, method: &str, params: Value) -> Value {
        let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let line = match self {
            Backend::Local(s) => match method {
                "file.save" | "file.open" => Some(file_method(s, &req)),
                _ => s.handle_line(&req.to_string()),
            },
            Backend::Remote(conn) => remote_call(conn, &req),
        };
        line.and_then(|l| serde_json::from_str(&l).ok())
            .unwrap_or_else(|| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "no response"}}))
    }
}

/// Decides where to connect and which token to send.
fn resolve_remote(connect: &str, token: Option<&str>, info: Option<AgentInfo>) -> Result<(String, String), String> {
    if connect.is_empty() {
        let info = info.ok_or("no running Cadektor app found: start the app with --agent-port, or give --connect HOST:PORT --token TOKEN")?;
        return Ok((format!("127.0.0.1:{}", info.port), token.map_or(info.token, str::to_string)));
    }
    // The runtime file's token is only for the app that file describes.
    let from_file = info.filter(|i| connect == format!("127.0.0.1:{}", i.port)).map(|i| i.token);
    let token = token
        .map(str::to_string)
        .or(from_file)
        .ok_or("--connect HOST:PORT needs --token (the app writes its token to its runtime file)")?;
    Ok((connect.to_string(), token))
}

/// Opens the connection and sends `auth` as the first message.
fn connect_remote(addr: &str, token: &str) -> Result<BufReader<TcpStream>, String> {
    let s = TcpStream::connect(addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?;
    let mut conn = BufReader::new(s);
    let auth = json!({"jsonrpc": "2.0", "id": 0, "method": "auth", "params": {"token": token}});
    let reply = remote_call(&mut conn, &auth).and_then(|l| serde_json::from_str::<Value>(&l).ok());
    match reply {
        Some(r) if r.get("error").is_none() && r.get("result").is_some() => Ok(conn),
        _ => Err(format!("{addr} refused the token (is it the right app and token?)")),
    }
}

fn remote_call(conn: &mut BufReader<TcpStream>, req: &Value) -> Option<String> {
    writeln!(conn.get_mut(), "{req}").ok()?;
    let mut line = String::new();
    match conn.read_line(&mut line) {
        Ok(n) if n > 0 => Some(line),
        _ => None,
    }
}

fn load(path: &PathBuf) -> Result<Document, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut doc = Document::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    // An old file has no project GUID. It gets one in memory, and the next save writes it.
    doc.ensure_project_guid();
    Ok(doc)
}

/// The folder of a document file.
fn doc_dir_of(file: &std::path::Path) -> Option<PathBuf> {
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), PathBuf::from);
    std::fs::canonicalize(dir).ok()
}

/// `file.save {path}` and `file.open {path}` for a headless session. The path policy of the session applies.
fn file_method(s: &mut Session, req: &Value) -> String {
    let id = req["id"].clone();
    let path = req["params"]["path"].as_str().map(PathBuf::from);
    let result = match (req["method"].as_str(), path) {
        (_, None) => Err((-32602, "params.path is required".to_string())),
        (Some("file.save"), Some(p)) => s
            .check_path(&p, true, &["json"])
            .map_err(|e| (1, e))
            .and_then(|p| {
                let t = serde_json::to_string_pretty(&s.editor.doc()).map_err(|e| (1, e.to_string()))?;
                fsio::write_atomic(&p, t.as_bytes()).map_err(|e| (1, e.to_string()))?;
                s.doc_dir = doc_dir_of(&p);
                Ok(())
            }),
        (_, Some(p)) => s.check_path(&p, false, &["json"]).map_err(|e| (1, e)).and_then(|p| load(&p).map_err(|e| (1, e)).and_then(|d| {
            s.doc_dir = doc_dir_of(&p);
            let line = json!({"id": 0, "method": "document.load", "params": {"document": d}}).to_string();
            let r: Value = serde_json::from_str(&s.handle_line(&line).unwrap()).unwrap();
            match r.get("error") {
                Some(e) => Err((1, e["message"].as_str().unwrap_or("load failed").to_string())),
                None => Ok(()),
            }
        })),
    };
    match result {
        Ok(()) => json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true}}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    }
    .to_string()
}

fn serve(mut backend: Backend) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}}));
                continue;
            }
        };
        let Some(method) = req["method"].as_str() else {
            let _ = writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32600, "message": "missing method"}}));
            continue;
        };
        let resp = backend.call(req["id"].clone(), method, req["params"].clone());
        if req.get("id").is_some() {
            let _ = writeln!(out, "{resp}");
            let _ = out.flush();
        }
    }
}

/// Maps API method names to MCP tool names: `objects.list` -> `objects_list`.
fn tool_name(method: &str) -> String {
    method.replace('.', "_")
}

/// MCP protocol versions that this server speaks, newest first.
const MCP_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];

/// The version to answer `initialize` with: the client's if we speak it, else our latest.
fn mcp_version(requested: Option<&str>) -> &'static str {
    MCP_VERSIONS.iter().copied().find(|v| Some(*v) == requested).unwrap_or(MCP_VERSIONS[0])
}

fn mcp_tools() -> Value {
    let mut tools: Vec<Value> = METHODS
        .iter()
        .map(|(m, d)| json!({"name": tool_name(m), "description": d, "inputSchema": {"type": "object", "additionalProperties": true}}))
        .collect();
    tools.push(json!({"name": "file_save", "description": "Save the document. params: {path}.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}}));
    tools.push(json!({"name": "file_open", "description": "Open a document. params: {path}.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}}));
    json!(tools)
}

/// The content of a tool result. `render.png` gives an image block that a model can look at,
/// plus the other fields as text. Other methods give their result as text.
fn tool_content(method: &str, response: &Value, text: String) -> Value {
    if (method == "render.png" || method == "sheet.render")
        && let Some(data) = response["result"]["png_base64"].as_str()
    {
        let mut meta = response["result"].clone();
        meta.as_object_mut().map(|o| o.remove("png_base64"));
        return json!([{"type": "image", "data": data, "mimeType": "image/png"}, {"type": "text", "text": meta.to_string()}]);
    }
    json!([{"type": "text", "text": text}])
}

fn mcp(mut backend: Backend) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let req = match serde_json::from_str::<Value>(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}}));
                let _ = out.flush();
                continue;
            }
        };
        let Some(id) = req.get("id").cloned() else { continue }; // notifications need no reply
        let result = match req["method"].as_str().unwrap_or("") {
            "initialize" => Ok(json!({
                "protocolVersion": mcp_version(req["params"]["protocolVersion"].as_str()),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "cadektor", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Cadektor CAD. Call describe first. Units are mm, Y axis up. Change the drawing only with execute."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": mcp_tools()})),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = req["params"]["arguments"].clone();
                let method = METHODS
                    .iter()
                    .map(|(m, _)| *m)
                    .chain(["file.save", "file.open"])
                    .find(|m| tool_name(m) == name);
                match method {
                    None => Err(json!({"code": -32602, "message": format!("unknown tool: {name}")})),
                    Some(m) => {
                        let r = backend.call(json!(1), m, args);
                        let (text, is_error) = match r.get("error") {
                            Some(e) => (e["message"].as_str().unwrap_or("error").to_string(), true),
                            None => (r["result"].to_string(), false),
                        };
                        Ok(json!({"content": tool_content(m, &r, text), "isError": is_error}))
                    }
                }
            }
            m => Err(json!({"code": -32601, "message": format!("unknown method: {m}")})),
        };
        let resp = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        };
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
}

/// Calls `import.file` or `export.file` on a session and returns the result.
fn io_call(session: &mut Session, method: &str, path: &std::path::Path, format: Option<String>, options: Option<String>) -> Result<Value, String> {
    let options = match options {
        Some(t) => serde_json::from_str::<Value>(&t).map_err(|e| format!("--options is not JSON: {e}"))?,
        None => Value::Null,
    };
    let mut params = json!({"path": path, "options": options});
    if let Some(f) = format {
        params["format"] = json!(f);
    }
    session.handle(method, params).map_err(|e| e.message)
}

fn save(doc: &Document, path: &std::path::Path) -> Result<(), String> {
    let text = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    fsio::write_atomic(path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

fn import_command(file: &std::path::Path, output: &std::path::Path, base: Option<PathBuf>, format: Option<String>, options: Option<String>) -> Result<(), String> {
    let mut session = Session::default();
    if let Some(base) = base {
        session.editor = cadektor_core::command::Editor::new(load(&base)?);
    }
    let result = io_call(&mut session, "import.file", file, format, options)?;
    save(session.editor.doc(), output)?;
    println!("{}", serde_json::to_string_pretty(&result["report"]).unwrap_or_default());
    Ok(())
}

fn export_command(file: &std::path::Path, output: &std::path::Path, format: Option<String>, options: Option<String>) -> Result<(), String> {
    let mut session = Session::with_document(load(&file.to_path_buf())?);
    let result = io_call(&mut session, "export.file", output, format, options)?;
    println!("{}", serde_json::to_string_pretty(&result["report"]).unwrap_or_default());
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Serve { file, remote } => Backend::new(file, remote).map(serve),
        Cmd::Mcp { file, remote } => Backend::new(file, remote).map(mcp),
        Cmd::Import { file, output, into, format, options } => import_command(&file, &output, into, format, options),
        Cmd::Export { file, output, format, options } => export_command(&file, &output, format, options),
        Cmd::Render { file, output, width } => load(&file).and_then(|doc| {
            fsio::write_atomic(&output, cadektor_core::svg::render(&doc, width).as_bytes()).map_err(|e| e.to_string())
        }),
    };
    if let Err(e) = result {
        eprintln!("cadektor: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn info() -> Option<AgentInfo> {
        Some(AgentInfo { port: 4000, token: "file-token".into() })
    }

    #[test]
    fn connect_without_address_uses_the_runtime_file() {
        assert_eq!(resolve_remote("", None, info()), Ok(("127.0.0.1:4000".into(), "file-token".into())));
        assert_eq!(resolve_remote("", Some("mine"), info()), Ok(("127.0.0.1:4000".into(), "mine".into())));
        assert!(resolve_remote("", None, None).unwrap_err().contains("no running Cadektor app"));
    }

    #[test]
    fn connect_with_address_needs_a_token_from_the_flag_or_the_file() {
        assert_eq!(resolve_remote("10.0.0.5:9", Some("t"), None), Ok(("10.0.0.5:9".into(), "t".into())));
        assert!(resolve_remote("10.0.0.5:9", None, info()).unwrap_err().contains("--token"));
        assert!(resolve_remote("127.0.0.1:4001", None, info()).unwrap_err().contains("--token"));
        assert_eq!(resolve_remote("127.0.0.1:4000", None, info()), Ok(("127.0.0.1:4000".into(), "file-token".into())));
        assert_eq!(resolve_remote("10.0.0.5:9", Some("t"), info()).unwrap().1, "t");
        assert!(resolve_remote("10.0.0.5:9", None, None).unwrap_err().contains("--token"));
    }

    #[test]
    fn render_png_tool_result_is_an_image() {
        let ok = json!({"result": {"png_base64": "AAAA", "width": 2, "view": "top"}});
        let c = tool_content("render.png", &ok, ok["result"].to_string());
        assert_eq!(c[0], json!({"type": "image", "data": "AAAA", "mimeType": "image/png"}));
        let meta: Value = serde_json::from_str(c[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(meta, json!({"width": 2, "view": "top"}));
        // Errors and other methods stay text.
        let err = json!({"error": {"message": "bad"}});
        assert_eq!(tool_content("render.png", &err, "bad".into()), json!([{"type": "text", "text": "bad"}]));
        assert_eq!(tool_content("view.list", &ok, "x".into()), json!([{"type": "text", "text": "x"}]));
    }

    #[test]
    fn mcp_version_negotiation() {
        assert_eq!(mcp_version(Some("2025-06-18")), "2025-06-18");
        assert_eq!(mcp_version(Some("2025-03-26")), "2025-03-26");
        assert_eq!(mcp_version(Some("2024-11-05")), "2025-06-18");
        assert_eq!(mcp_version(Some("2099-01-01")), "2025-06-18");
        assert_eq!(mcp_version(None), "2025-06-18");
    }

    /// A one-shot server that checks the first line is `auth` and answers.
    fn fake_app(ok: bool) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let h = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut s = s;
            if ok {
                writeln!(s, r#"{{"jsonrpc":"2.0","id":0,"result":{{"ok":true}}}}"#).unwrap();
            }
            line
        });
        (addr, h)
    }

    #[test]
    fn connect_remote_sends_auth_first() {
        let (addr, h) = fake_app(true);
        let _conn = connect_remote(&addr, "sekret").unwrap();
        let first: Value = serde_json::from_str(&h.join().unwrap()).unwrap();
        assert_eq!(first["method"], "auth");
        assert_eq!(first["params"]["token"], "sekret");
    }

    #[test]
    fn connect_remote_reports_refusal_and_unreachable_apps() {
        let (addr, h) = fake_app(false);
        assert!(connect_remote(&addr, "bad").unwrap_err().contains("refused"));
        h.join().unwrap();
        let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        assert!(connect_remote(&free, "t").unwrap_err().contains("cannot connect"));
    }

    #[test]
    fn import_and_export_commands() {
        let dir = std::env::temp_dir().join(format!("cadektor-cli-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("in.geojson");
        std::fs::write(&src, r#"{"type":"Point","coordinates":[24.9,60.1,3]}"#).unwrap();
        let doc = dir.join("doc.cdk.json");
        import_command(&src, &doc, None, None, None).unwrap();
        let loaded = load(&doc).unwrap();
        assert_eq!(loaded.objects.len(), 1);
        assert!(loaded.georef.is_some());
        // Import a second time into the saved document.
        import_command(&src, &doc, Some(doc.clone()), Some("geojson".into()), Some(r#"{"layer": "GIS"}"#.into())).unwrap();
        assert_eq!(load(&doc).unwrap().objects.len(), 2);
        let out = dir.join("out.dxf");
        export_command(&doc, &out, None, Some(r#"{"unit": "m"}"#.into())).unwrap();
        assert!(std::fs::read_to_string(&out).unwrap().contains("ENTITIES"));
        assert!(import_command(&src, &doc, None, None, Some("{nope".into())).unwrap_err().contains("--options"));
        assert!(import_command(&dir.join("missing.dxf"), &doc, None, None, None).is_err());
        assert!(export_command(&dir.join("missing.json"), &out, None, None).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_reports_file_errors_with_the_path() {
        let dir = std::env::temp_dir().join(format!("cadektor-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.cdk.json");
        std::fs::write(&bad, "{nope").unwrap();
        assert!(load(&bad).unwrap_err().contains("bad.cdk.json"));
        assert!(load(&dir.join("missing")).is_err());
        let good = dir.join("good.cdk.json");
        std::fs::write(&good, serde_json::to_string(&Document::default()).unwrap()).unwrap();
        assert!(load(&good).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
