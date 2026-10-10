//! Runs the `kadakas` program: `serve`, `mcp`, `render`, `import`, `export`, the flags and the
//! connection to a running app (a stand-in server).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_kadakas");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kadakas-cli-it-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Runs the program with these lines on stdin and returns the output.
fn run(args: &[&str], input: &str, dir: &Path) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn lines(out: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect()
}

fn requests(reqs: &[Value]) -> String {
    reqs.iter().map(|r| format!("{r}\n")).collect()
}

fn rpc(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

#[test]
fn serve_answers_requests_and_reports_bad_lines() {
    let dir = temp("serve");
    let input = format!(
        "\n{}{}not json\n{}{}{}",
        requests(&[rpc(
            1,
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}})
        )]),
        // A notification has no id and gets no answer.
        "{\"jsonrpc\":\"2.0\",\"method\":\"undo\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":7}\n",
        requests(&[rpc(2, "document.summary", Value::Null)]),
        requests(&[rpc(3, "no.such.method", json!({}))]),
    );
    let out = run(&["serve"], &input, &dir);
    assert!(out.status.success());
    let l = lines(&out);
    assert_eq!(l.len(), 5, "{l:?}");
    assert!(l[0]["result"]["created"].is_array());
    assert_eq!(l[1]["error"]["code"], -32700);
    assert_eq!(l[2]["error"]["code"], -32600);
    assert_eq!(l[3]["id"], 2);
    assert!(l[4]["error"].is_object());
}

#[test]
fn serve_saves_and_opens_documents_inside_the_allowed_folder() {
    let dir = temp("files");
    let other = temp("outside");
    let doc = dir.join("a.kad");
    let input = requests(&[
        rpc(
            1,
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}),
        ),
        rpc(2, "file.save", json!({"path": doc})),
        rpc(3, "file.save", json!({})),
        rpc(4, "file.save", json!({"path": other.join("b.kad")})),
        rpc(5, "file.open", json!({"path": doc})),
        rpc(6, "file.open", json!({"path": dir.join("missing.kad")})),
        rpc(7, "objects.list", json!({})),
    ]);
    let out = run(
        &["serve", "--allow-dir", dir.to_str().unwrap()],
        &input,
        &dir,
    );
    let l = lines(&out);
    assert_eq!(l[1]["result"], json!({"ok": true}));
    assert_eq!(l[2]["error"]["code"], -32602);
    assert!(l[3]["error"]["message"].as_str().unwrap().len() > 3);
    assert_eq!(l[4]["result"], json!({"ok": true}));
    assert!(l[5]["error"].is_object());
    assert_eq!(l[6]["result"].as_array().map_or(1, |a| a.len()), 1);
    assert!(doc.exists());

    // A document given at start is loaded, and its folder is allowed.
    let out = run(
        &["serve", doc.to_str().unwrap()],
        &requests(&[rpc(1, "document.summary", Value::Null)]),
        &dir,
    );
    assert!(lines(&out)[0]["result"].is_object());
    // A file that does not open is a startup error.
    let out = run(&["serve", dir.join("nope.kad").to_str().unwrap()], "", &dir);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("kadakas:"));
    let bad = dir.join("bad.kad");
    std::fs::write(&bad, "{nope").unwrap();
    let out = run(&["mcp", bad.to_str().unwrap()], "", &dir);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn serve_options_for_urls_and_scripts_are_accepted() {
    let dir = temp("opts");
    let out = run(
        &[
            "serve",
            "--allow-url",
            "maps.example.org",
            "--allow-url",
            "*.example.net",
            "--allow-agent-scripts",
        ],
        &requests(&[rpc(1, "document.summary", Value::Null)]),
        &dir,
    );
    assert!(out.status.success());
    assert!(lines(&out)[0]["result"].is_object());
}

fn mcp_session(args: &[&str], reqs: &[Value], dir: &Path) -> Vec<Value> {
    let out = run(args, &requests(reqs), dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    lines(&out)
}

#[test]
fn mcp_lists_domain_tools_and_runs_methods() {
    let dir = temp("mcp");
    let call = |id: u64, tool: &str, args: Value| {
        rpc(id, "tools/call", json!({"name": tool, "arguments": args}))
    };
    let l = mcp_session(
        &["mcp"],
        &[
            rpc(1, "initialize", json!({"protocolVersion": "2025-03-26"})),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            rpc(2, "ping", json!({})),
            rpc(3, "tools/list", json!({})),
            call(
                4,
                "document",
                json!({"method": "execute", "params": {"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [100, 50]}}}}),
            ),
            call(5, "objects", json!({"method": "objects.list"})),
            call(6, "objects", json!({"method": "plants.place"})),
            call(7, "nothing", json!({})),
            call(
                8,
                "render_view",
                json!({"method": "render.png", "params": {"width": 16, "height": 12}}),
            ),
            rpc(9, "resources/list", json!({})),
            call(
                10,
                "document",
                json!({"method": "file.save", "params": {"path": "saved.kad"}}),
            ),
            call(
                11,
                "document",
                json!({"method": "file.open", "params": {"path": "saved.kad"}}),
            ),
            call(12, "selection_undo", json!({"method": "undo"})),
        ],
        &dir,
    );
    assert_eq!(l[0]["result"]["protocolVersion"], "2025-03-26");
    assert!(
        l[0]["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("method, params")
    );
    assert_eq!(l[1]["result"], json!({}));
    let tools = l[2]["result"]["tools"].as_array().unwrap();
    assert!((15..=25).contains(&tools.len()), "{}", tools.len());
    assert!(
        tools
            .iter()
            .all(|t| t["annotations"]["readOnlyHint"].is_boolean())
    );
    assert_eq!(l[3]["result"]["isError"], false);
    assert_eq!(l[4]["result"]["isError"], false);
    assert!(
        l[4]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("rect")
    );
    assert_eq!(l[5]["result"]["isError"], true);
    assert_eq!(l[6]["error"]["code"], -32602);
    assert_eq!(l[7]["result"]["content"][0]["type"], "image");
    assert_eq!(l[8]["error"]["code"], -32601);
    assert_eq!(l[9]["result"]["isError"], false);
    assert_eq!(l[10]["result"]["isError"], false);
    assert_eq!(l[11]["result"]["isError"], false);
}

#[test]
fn mcp_flat_lists_every_method_as_a_tool() {
    let dir = temp("flat");
    let l = mcp_session(
        &["mcp", "--mcp-flat"],
        &[
            rpc(1, "initialize", json!({})),
            rpc(2, "tools/list", json!({})),
            rpc(
                3,
                "tools/call",
                json!({"name": "document_summary", "arguments": {}}),
            ),
            rpc(
                4,
                "tools/call",
                json!({"name": "file_save", "arguments": {}}),
            ),
            rpc(
                5,
                "tools/call",
                json!({"name": "document", "arguments": {}}),
            ),
        ],
        &dir,
    );
    assert!(
        l[0]["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("execute tool")
    );
    let tools = l[1]["result"]["tools"].as_array().unwrap();
    assert!(tools.len() > 250);
    assert!(tools.iter().any(|t| t["name"] == "file_open"));
    assert_eq!(l[2]["result"]["isError"], false);
    assert_eq!(l[3]["result"]["isError"], true);
    assert_eq!(l[4]["error"]["code"], -32602);
}

#[test]
fn mcp_reports_bad_json_lines() {
    let dir = temp("mcpbad");
    let out = run(&["mcp"], "garbage\n\n", &dir);
    let l = lines(&out);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0]["error"]["code"], -32700);
}

#[test]
fn render_import_and_export_commands() {
    let dir = temp("convert");
    let geo = dir.join("in.geojson");
    std::fs::write(
        &geo,
        r#"{"type":"LineString","coordinates":[[24.9,60.1],[24.91,60.11]]}"#,
    )
    .unwrap();
    let doc = dir.join("doc.kad");
    let out = run(
        &["import", geo.to_str().unwrap(), "-o", doc.to_str().unwrap()],
        "",
        &dir,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["format"], "geojson");

    // Import into the saved document, with a format and options.
    let out = run(
        &[
            "import",
            geo.to_str().unwrap(),
            "-o",
            doc.to_str().unwrap(),
            "--into",
            doc.to_str().unwrap(),
            "--format",
            "geojson",
            "--options",
            "{}",
        ],
        "",
        &dir,
    );
    assert!(out.status.success());

    let svg = dir.join("out.svg");
    let out = run(
        &[
            "render",
            doc.to_str().unwrap(),
            "-o",
            svg.to_str().unwrap(),
            "--width",
            "300",
        ],
        "",
        &dir,
    );
    assert!(out.status.success());
    assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));

    for (name, marker) in [
        ("out.dxf", "ENTITIES"),
        ("out.geojson", "FeatureCollection"),
    ] {
        let target = dir.join(name);
        let out = run(
            &[
                "export",
                doc.to_str().unwrap(),
                "-o",
                target.to_str().unwrap(),
            ],
            "",
            &dir,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(std::fs::read_to_string(&target).unwrap().contains(marker));
    }
    let ifc = dir.join("out.ifc");
    let out = run(
        &[
            "export",
            doc.to_str().unwrap(),
            "-o",
            ifc.to_str().unwrap(),
            "--format",
            "ifc",
            "--options",
            r#"{"layers_as_storeys": true}"#,
        ],
        "",
        &dir,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(ifc.exists());

    // Errors end with a message and exit code 1.
    for args in [
        vec!["render", "missing.kad", "-o", "x.svg"],
        vec!["import", "missing.dxf", "-o", "x.kad"],
        vec!["export", "missing.kad", "-o", "x.dxf"],
    ] {
        let out = run(&args, "", &dir);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("kadakas:"));
    }
    let out = run(
        &[
            "import",
            geo.to_str().unwrap(),
            "-o",
            "x.kad",
            "--options",
            "{nope",
        ],
        "",
        &dir,
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("--options"));
    // An unknown command is a usage error.
    let out = run(&["frobnicate"], "", &dir);
    assert_eq!(out.status.code(), Some(2));
    let out = run(&["--version"], "", &dir);
    assert!(String::from_utf8_lossy(&out.stdout).contains("kadakas"));
}

/// A stand-in app: answers `auth` with ok when the token is right, then echoes the method.
fn fake_app(token: &'static str) -> (u16, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut w = s;
        let mut seen = Vec::new();
        let mut line = String::new();
        while r.read_line(&mut line).unwrap_or(0) > 0 {
            let req: Value = serde_json::from_str(&line).unwrap();
            line.clear();
            let ok = req["method"] != "auth" || req["params"]["token"] == token;
            let reply = if ok {
                json!({"jsonrpc": "2.0", "id": req["id"], "result": {"echo": req["method"]}})
            } else {
                json!({"jsonrpc": "2.0", "id": req["id"], "error": {"code": 1, "message": "bad token"}})
            };
            seen.push(req);
            writeln!(w, "{reply}").unwrap();
            if !ok {
                break;
            }
        }
        seen
    });
    (port, handle)
}

#[test]
fn connect_forwards_requests_to_a_running_app() {
    let dir = temp("connect");
    let (port, app) = fake_app("s3cret");
    let addr = format!("127.0.0.1:{port}");
    let out = run(
        &["serve", "--connect", &addr, "--token", "s3cret"],
        &requests(&[rpc(1, "document.summary", json!({}))]),
        &dir,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        lines(&out)[0]["result"],
        json!({"echo": "document.summary"})
    );
    let seen = app.join().unwrap();
    assert_eq!(seen[0]["method"], "auth");

    // MCP through the app: the file tools go to the app too.
    let (port, app) = fake_app("s3cret");
    let addr = format!("127.0.0.1:{port}");
    let l = mcp_session(
        &["mcp", "--connect", &addr, "--token", "s3cret"],
        &[rpc(
            1,
            "tools/call",
            json!({"name": "document", "arguments": {"method": "file.save", "params": {"path": "/x"}}}),
        )],
        &dir,
    );
    assert_eq!(l[0]["result"]["isError"], false);
    let seen = app.join().unwrap();
    assert!(
        seen.iter()
            .any(|r| r["method"] == "file.save" && r["params"] == json!({"path": "/x"}))
    );

    // A wrong token, a missing token and an unreachable app are startup errors.
    let (port, app) = fake_app("right");
    let addr = format!("127.0.0.1:{port}");
    let out = run(&["serve", "--connect", &addr, "--token", "wrong"], "", &dir);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refused the token"));
    app.join().unwrap();
    let out = run(&["serve", "--connect", "127.0.0.1:9"], "", &dir);
    assert!(String::from_utf8_lossy(&out.stderr).contains("--token"));
    let out = run(
        &["serve", "--connect", "127.0.0.1:9", "--token", "t"],
        "",
        &dir,
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot connect"));
}

#[test]
fn connect_without_an_address_reads_the_runtime_file() {
    let dir = temp("runtime");
    let (port, app) = fake_app("file-token");
    // The runtime folder is named by XDG_RUNTIME_DIR on Unix and by LOCALAPPDATA on Windows.
    let var = if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "XDG_RUNTIME_DIR"
    };
    let run_dir = dir.join("runtime");
    std::fs::create_dir_all(run_dir.join("kadakas")).unwrap();
    std::fs::write(
        run_dir.join("kadakas").join("agent.json"),
        json!({"port": port, "token": "file-token"}).to_string(),
    )
    .unwrap();
    let mut child = Command::new(BIN)
        .args(["serve", "--connect"])
        .env(var, &run_dir)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(requests(&[rpc(1, "ping", json!({}))]).as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(lines(&out)[0]["result"], json!({"echo": "ping"}));
    app.join().unwrap();

    // No runtime file: a clear message.
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let out = Command::new(BIN)
        .args(["serve", "--connect"])
        .env(var, &empty)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("no running Kadakas app"));
}

// ----- collaboration over WebSocket -----

/// A running `kadakas serve` that the test talks to line by line.
struct Agent {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Agent {
    fn start(args: &[&str], env: &[(&str, &str)], dir: &Path) -> Agent {
        let mut cmd = Command::new(BIN);
        cmd.arg("serve")
            .args(args)
            .current_dir(dir)
            .env_remove("KADAKAS_COLLAB_URL")
            .env_remove("KADAKAS_COLLAB_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Agent {
            child,
            out,
            next_id: 1,
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", rpc(id, method, params)).unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{line}: {e}"));
        assert_eq!(v["id"], id);
        v
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params);
        assert!(v.get("error").is_none(), "{method}: {v}");
        v["result"].clone()
    }

    /// Syncs until the document has `n` objects. Returns their IDs, sorted.
    fn wait_objects(&mut self, n: usize) -> Vec<String> {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            self.ok("collab.sync", json!({}));
            let list = self.ok("objects.list", json!({}));
            let mut ids: Vec<String> = list["objects"]
                .as_array()
                .or_else(|| list.as_array())
                .unwrap()
                .iter()
                .map(|o| o["id"].as_str().unwrap().to_string())
                .collect();
            ids.sort();
            if ids.len() == n {
                return ids;
            }
            assert!(std::time::Instant::now() < end, "timeout: {n} objects");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const COLLAB_DOC: &str = "0190f1c2-7a3b-7c1d-8e2f-123456789abc";

#[test]
fn serve_connects_to_a_collaboration_server_with_env_defaults() {
    use kadakas_collab::docsync;
    use kadakas_collab::testkit::Sequencer;
    use kadakas_collab_ws::testserver::TestServer;
    use kadakas_core::model::Document;
    use kadakas_core::uid::Uid;

    let srv = TestServer::start_with(Sequencer::new(docsync::replica_of(&Document::default())));
    let secret = "env.token-SECRET-42";
    srv.require_token(secret, Uid::from_num(1));
    let dir = temp("collab");
    let base = srv.base_url();
    let env = [
        ("KADAKAS_COLLAB_URL", base.as_str()),
        ("KADAKAS_COLLAB_TOKEN", secret),
    ];
    let mut a = Agent::start(&[], &env, &dir);
    let st = a.ok(
        "collab.connect",
        json!({"doc": COLLAB_DOC, "user": Uid::from_num(1).to_string(), "name": "Ann"}),
    );
    assert!(!st.to_string().contains(secret));
    // The shared document replaces the open one: edit after the client is online.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while a.ok("collab.sync", json!({}))["status"]["state"] != "online" {
        assert!(std::time::Instant::now() < end, "never online");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    a.ok(
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [100, 50]}}}),
    );
    // Wait until the server has the edit.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while srv.with(|s| s.seq()) == 0 {
        let last = a.ok("collab.sync", json!({}));
        assert!(
            std::time::Instant::now() < end,
            "the edit never arrived: {last}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let seen = srv.seen();
    assert_eq!(seen[0].path, format!("/ws/{COLLAB_DOC}"));
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(format!("Bearer {secret}").as_str())
    );

    // A second program with the flags sees the object. (The test server checks one token, so
    // the second user connects to a second server with the same state.)
    let srv2 = TestServer::start_with(Sequencer::new(srv.with(|s| s.replica().clone())));
    srv2.require_token("flag-token", Uid::from_num(2));
    let mut b = Agent::start(
        &[
            "--collab-url",
            &srv2.base_url(),
            "--collab-token",
            "flag-token",
        ],
        &[],
        &dir,
    );
    b.ok(
        "collab.connect",
        json!({"doc": COLLAB_DOC, "user": Uid::from_num(2).to_string(), "name": "Bo"}),
    );
    b.wait_objects(1);
}

#[test]
fn serve_refuses_a_server_that_is_not_on_this_computer() {
    let dir = temp("collab-policy");
    let mut a = Agent::start(&[], &[], &dir);
    let v = a.call(
        "collab.connect",
        json!({"url": "wss://collab.example.org", "doc": COLLAB_DOC}),
    );
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.contains("--allow-url collab.example.org"), "{msg}");
    drop(a);
    // With the flag the policy passes and the connection fails later (no such host).
    let mut a = Agent::start(&["--allow-url", "collab.invalid"], &[], &dir);
    let v = a.call(
        "collab.connect",
        json!({"url": "wss://collab.invalid", "doc": COLLAB_DOC}),
    );
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(!msg.contains("allow-url"), "{msg}");
}
