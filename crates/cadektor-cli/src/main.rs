//! Headless Cadektor for scripts and AI agents.
//!
//! - `cadektor serve`: JSON-RPC 2.0 on stdin/stdout, one request per line.
//! - `cadektor mcp`: Model Context Protocol server on stdio, for Claude Code.
//! - `cadektor render <file> -o out.svg`: render a document to SVG.
//! - `cadektor import <file> -o doc.json`: import a DXF, DWG (needs a converter), IFC, Shapefile or GeoJSON file into a document.
//! - `cadektor export <doc> -o out.dxf`: export a document to DXF, DWG (needs a converter), IFC or GeoJSON.
//!
//! With `--connect`, `serve` and `mcp` forward every request to a running
//! desktop app instead of a headless document. `--connect` alone finds the
//! app through its runtime file. `--connect HOST:PORT` needs `--token` (or the
//! runtime file). The first message on the connection is always `auth`.

mod mcp_tools;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;

use cadektor_api::Session;
use cadektor_core::model::Document;
use cadektor_sys::agent::{self as sys_agent, AgentInfo};
use cadektor_sys::fsio;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

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
        /// List one tool per API method (about 280) instead of one tool per domain (about 20).
        #[arg(long = "mcp-flat")]
        mcp_flat: bool,
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
    /// A web address that agents may fetch for map services (WMS, WMTS, WFS): a host name
    /// (`maps.example.org`), a pattern (`*.example.org`) or an address prefix
    /// (`https://maps.example.org/wms/`). Repeat for more. Default: none, so agents cannot use the network.
    #[arg(long = "allow-url", value_name = "URL")]
    allow_url: Vec<String>,
    /// Let requests from agents run Python scripts (`script.run`, and plug-in objects with their own
    /// scripts). Default: off. The plug-in objects that ship with the app always run.
    #[arg(long = "allow-agent-scripts")]
    allow_agent_scripts: bool,
    /// Let requests from agents start external converter programs (DWG import and export).
    /// Default: off. The converter is a separate program that the user installed.
    #[arg(long = "allow-converters")]
    allow_converters: bool,
    /// Give new objects, layers and classes the IDs 2, 3, 4 and so on, as small numbers, so a test
    /// can predict them. Never use it for documents that are merged with others: these IDs are
    /// not unique in the world. Default: off (new IDs are UUIDs).
    #[arg(long = "sequential-ids")]
    sequential_ids: bool,
}

/// Where requests go: a local document, or a running app over TCP.
enum Backend {
    Local(Box<Session>),
    Remote(BufReader<TcpStream>),
}

impl Backend {
    fn new(file: Option<PathBuf>, remote: RemoteArgs) -> Result<Self, String> {
        if let Some(connect) = remote.connect {
            let info =
                sys_agent::agent_info_path().and_then(|p| sys_agent::read_agent_info(&p).ok());
            let (addr, token) = resolve_remote(&connect, remote.token.as_deref(), info)?;
            return connect_remote(&addr, &token).map(Backend::Remote);
        }
        let mut session = Session::default();
        let sequential = remote.sequential_ids;
        // Requests come from agents: they may only use the allowed folders.
        let dirs = if remote.allow_dir.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            remote.allow_dir
        };
        session.paths =
            cadektor_api::PathPolicy::restricted(dirs).with_converters(remote.allow_converters);
        session.gis.urls = cadektor_api::UrlPolicy::allow_list(remote.allow_url);
        session.scripting.allow_agent_scripts = remote.allow_agent_scripts;
        session.scripting.trust = cadektor_api::TrustStore::load_default();
        if let Some(f) = file {
            session.editor = cadektor_core::command::Editor::new(load(&f)?);
            session.doc_dir = doc_dir_of(&f);
        }
        session.set_sequential_ids(sequential);
        Ok(Backend::Local(Box::new(session)))
    }

    /// Calls one method and returns the JSON-RPC response object.
    fn call(&mut self, id: Value, method: &str, params: Value) -> Value {
        let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let line = match self {
            Backend::Local(s) => match method {
                "file.save" | "file.open" => {
                    Some(cadektor_api::files::handle_file_request(s, &req))
                }
                _ => s.handle_line(&req.to_string()),
            },
            Backend::Remote(conn) => remote_call(conn, &req),
        };
        line.and_then(|l| serde_json::from_str(&l).ok())
            .unwrap_or_else(|| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "no response"}}))
    }
}

/// Decides where to connect and which token to send.
fn resolve_remote(
    connect: &str,
    token: Option<&str>,
    info: Option<AgentInfo>,
) -> Result<(String, String), String> {
    if connect.is_empty() {
        let info = info.ok_or("no running Cadektor app found: start the app with --agent-port, or give --connect HOST:PORT --token TOKEN")?;
        return Ok((
            format!("127.0.0.1:{}", info.port),
            token.map_or(info.token, str::to_string),
        ));
    }
    // The runtime file's token is only for the app that file describes.
    let from_file = info
        .filter(|i| connect == format!("127.0.0.1:{}", i.port))
        .map(|i| i.token);
    let token = token.map(str::to_string).or(from_file).ok_or(
        "--connect HOST:PORT needs --token (the app writes its token to its runtime file)",
    )?;
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
        _ => Err(format!(
            "{addr} refused the token (is it the right app and token?)"
        )),
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

fn load(path: &std::path::Path) -> Result<Document, String> {
    let mut doc = cadektor_api::files::read_document(path).map_err(|e| e.message)?;
    // An old file has no project GUID. It gets one in memory, and the next save writes it.
    doc.ensure_project_guid();
    Ok(doc)
}

/// The folder of a document file.
fn doc_dir_of(file: &std::path::Path) -> Option<PathBuf> {
    let dir = file
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    std::fs::canonicalize(dir).ok()
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
                let _ = writeln!(
                    out,
                    "{}",
                    json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}"), "data": {"kind": "parse_error"}}})
                );
                continue;
            }
        };
        let Some(method) = req["method"].as_str() else {
            let _ = writeln!(
                out,
                "{}",
                json!({"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32600, "message": "missing method"}})
            );
            continue;
        };
        let resp = backend.call(req["id"].clone(), method, req["params"].clone());
        if req.get("id").is_some() {
            let _ = writeln!(out, "{resp}");
            let _ = out.flush();
        }
    }
}

/// MCP protocol versions that this server speaks, newest first.
const MCP_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];

/// The version to answer `initialize` with: the client's if we speak it, else our latest.
fn mcp_version(requested: Option<&str>) -> &'static str {
    MCP_VERSIONS
        .iter()
        .copied()
        .find(|v| Some(*v) == requested)
        .unwrap_or(MCP_VERSIONS[0])
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

fn mcp(mut backend: Backend, flat: bool) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let req = match serde_json::from_str::<Value>(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(
                    out,
                    "{}",
                    json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}"), "data": {"kind": "parse_error"}}})
                );
                let _ = out.flush();
                continue;
            }
        };
        let Some(id) = req.get("id").cloned() else {
            continue;
        }; // notifications need no reply
        let result = match req["method"].as_str().unwrap_or("") {
            "initialize" => Ok(json!({
                "protocolVersion": mcp_version(req["params"]["protocolVersion"].as_str()),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "cadektor", "version": env!("CARGO_PKG_VERSION")},
                "instructions": if flat { "Cadektor CAD. Call describe first. Units are mm, Y axis up. Most changes to the drawing go through the execute tool; many other tools change it too (plants, hardscape, annotation and so on)." } else { "Cadektor CAD. Call describe first. Units are mm, Y axis up. Each tool is a domain and takes {method, params}; its description lists the methods. The execute method of the document tool runs one command; many other methods change the drawing too." }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": mcp_tools::tools(flat)})),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = req["params"]["arguments"].clone();
                match mcp_tools::resolve(flat, name, &args) {
                    mcp_tools::Call::UnknownTool => {
                        Err(json!({"code": -32602, "message": format!("unknown tool: {name}")}))
                    }
                    mcp_tools::Call::Invalid(text) => {
                        Ok(json!({"content": [{"type": "text", "text": text}], "isError": true}))
                    }
                    mcp_tools::Call::Method(m, params) => {
                        let r = backend.call(json!(1), m, params);
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
fn io_call(
    session: &mut Session,
    method: &str,
    path: &std::path::Path,
    format: Option<String>,
    options: Option<String>,
) -> Result<Value, String> {
    let options = match options {
        Some(t) => {
            serde_json::from_str::<Value>(&t).map_err(|e| format!("--options is not JSON: {e}"))?
        }
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

fn import_command(
    file: &std::path::Path,
    output: &std::path::Path,
    base: Option<PathBuf>,
    format: Option<String>,
    options: Option<String>,
) -> Result<(), String> {
    let mut session = Session::default();
    if let Some(base) = base {
        session.editor = cadektor_core::command::Editor::new(load(&base)?);
    }
    let result = io_call(&mut session, "import.file", file, format, options)?;
    save(session.editor.doc(), output)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result["report"]).unwrap_or_default()
    );
    Ok(())
}

fn export_command(
    file: &std::path::Path,
    output: &std::path::Path,
    format: Option<String>,
    options: Option<String>,
) -> Result<(), String> {
    let mut session = Session::with_document(load(file)?);
    let result = io_call(&mut session, "export.file", output, format, options)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result["report"]).unwrap_or_default()
    );
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Serve { file, remote } => Backend::new(file, remote).map(serve),
        Cmd::Mcp {
            file,
            remote,
            mcp_flat,
        } => Backend::new(file, remote).map(|b| mcp(b, mcp_flat)),
        Cmd::Import {
            file,
            output,
            into,
            format,
            options,
        } => import_command(&file, &output, into, format, options),
        Cmd::Export {
            file,
            output,
            format,
            options,
        } => export_command(&file, &output, format, options),
        Cmd::Render {
            file,
            output,
            width,
        } => load(&file).and_then(|doc| {
            let svg = Session::with_document(doc).render_svg(width);
            fsio::write_atomic(&output, svg.as_bytes())
                .map_err(|e| format!("{}: {e}", output.display()))
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
        Some(AgentInfo {
            port: 4000,
            token: "file-token".into(),
        })
    }

    #[test]
    fn connect_without_address_uses_the_runtime_file() {
        assert_eq!(
            resolve_remote("", None, info()),
            Ok(("127.0.0.1:4000".into(), "file-token".into()))
        );
        assert_eq!(
            resolve_remote("", Some("mine"), info()),
            Ok(("127.0.0.1:4000".into(), "mine".into()))
        );
        assert!(
            resolve_remote("", None, None)
                .unwrap_err()
                .contains("no running Cadektor app")
        );
    }

    #[test]
    fn connect_with_address_needs_a_token_from_the_flag_or_the_file() {
        assert_eq!(
            resolve_remote("10.0.0.5:9", Some("t"), None),
            Ok(("10.0.0.5:9".into(), "t".into()))
        );
        assert!(
            resolve_remote("10.0.0.5:9", None, info())
                .unwrap_err()
                .contains("--token")
        );
        assert!(
            resolve_remote("127.0.0.1:4001", None, info())
                .unwrap_err()
                .contains("--token")
        );
        assert_eq!(
            resolve_remote("127.0.0.1:4000", None, info()),
            Ok(("127.0.0.1:4000".into(), "file-token".into()))
        );
        assert_eq!(
            resolve_remote("10.0.0.5:9", Some("t"), info()).unwrap().1,
            "t"
        );
        assert!(
            resolve_remote("10.0.0.5:9", None, None)
                .unwrap_err()
                .contains("--token")
        );
    }

    #[test]
    fn render_png_tool_result_is_an_image() {
        let ok = json!({"result": {"png_base64": "AAAA", "width": 2, "view": "top"}});
        let c = tool_content("render.png", &ok, ok["result"].to_string());
        assert_eq!(
            c[0],
            json!({"type": "image", "data": "AAAA", "mimeType": "image/png"})
        );
        let meta: Value = serde_json::from_str(c[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(meta, json!({"width": 2, "view": "top"}));
        // Errors and other methods stay text.
        let err = json!({"error": {"message": "bad"}});
        assert_eq!(
            tool_content("render.png", &err, "bad".into()),
            json!([{"type": "text", "text": "bad"}])
        );
        assert_eq!(
            tool_content("view.list", &ok, "x".into()),
            json!([{"type": "text", "text": "x"}])
        );
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
        assert!(
            connect_remote(&addr, "bad")
                .unwrap_err()
                .contains("refused")
        );
        h.join().unwrap();
        let free = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .to_string();
        assert!(
            connect_remote(&free, "t")
                .unwrap_err()
                .contains("cannot connect")
        );
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
        import_command(
            &src,
            &doc,
            Some(doc.clone()),
            Some("geojson".into()),
            Some(r#"{"layer": "GIS"}"#.into()),
        )
        .unwrap();
        assert_eq!(load(&doc).unwrap().objects.len(), 2);
        let out = dir.join("out.dxf");
        export_command(&doc, &out, None, Some(r#"{"unit": "m"}"#.into())).unwrap();
        assert!(std::fs::read_to_string(&out).unwrap().contains("ENTITIES"));
        assert!(
            import_command(&src, &doc, None, None, Some("{nope".into()))
                .unwrap_err()
                .contains("--options")
        );
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
