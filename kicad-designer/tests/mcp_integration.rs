//! MCP server end-to-end tests: spawn the real `kdesign serve` binary and
//! speak newline-delimited JSON-RPC over stdio, exactly as an MCP client would.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

struct Server {
    child: Child,
    stdin: std::process::ChildStdin,
    lines: Receiver<String>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_server(db_path: &std::path::Path) -> Server {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kdesign"))
        .arg("--db")
        .arg(db_path)
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn kdesign serve");

    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    Server {
        child,
        stdin,
        lines: rx,
    }
}

fn request(server: &mut Server, method: &str, params: serde_json::Value, id: u64) {
    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": method, "params": params
    });
    writeln!(server.stdin, "{}", req).expect("write request");
    server.stdin.flush().expect("flush");
}

fn read_response(server: &Server) -> serde_json::Value {
    let line = server
        .lines
        .recv_timeout(Duration::from_secs(30))
        .expect("timed out waiting for JSON-RPC response");
    serde_json::from_str(&line).expect("response is valid JSON")
}

fn fresh_db_path(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kdesign-mcp-test-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("components.db")
}

#[test]
fn handshake_and_tools_list() {
    let db = fresh_db_path("list");
    let mut srv = spawn_server(&db);

    // MCP initialize handshake
    request(&mut srv, "initialize", serde_json::json!({}), 1);
    let resp = read_response(&srv);
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(resp["result"]["serverInfo"]["name"], "kdesign");

    // Registry: every tool carries name/description/inputSchema
    request(&mut srv, "tools/list", serde_json::json!({}), 2);
    let resp = read_response(&srv);
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert!(
        tools.len() >= 40,
        "tool registry should be full, got {}",
        tools.len()
    );
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for key in [
        "query_components",
        "list_categories",
        "run_drc",
        "design_board",
        "generate_pcb",
    ] {
        assert!(names.contains(&key), "missing key tool: {}", key);
    }
    for t in tools {
        assert!(
            t["description"].is_string(),
            "tool {} missing description",
            t["name"]
        );
        assert!(
            t["inputSchema"].is_object(),
            "tool {} missing inputSchema",
            t["name"]
        );
    }
}

#[test]
fn tool_calls_on_fresh_db() {
    let db = fresh_db_path("call");
    let mut srv = spawn_server(&db);

    request(&mut srv, "initialize", serde_json::json!({}), 1);
    let _ = read_response(&srv);

    // Fresh db (auto-created schema): read-only tools must succeed with empty results
    request(
        &mut srv,
        "tools/call",
        serde_json::json!({ "name": "list_categories", "arguments": {} }),
        2,
    );
    let resp = read_response(&srv);
    assert!(
        resp["error"].is_null(),
        "list_categories error: {}",
        resp["error"]
    );
    assert!(resp["result"]["categories"].is_array());

    request(
        &mut srv,
        "tools/call",
        serde_json::json!({ "name": "query_components", "arguments": { "category": "passive/capacitor" } }),
        3,
    );
    let resp = read_response(&srv);
    assert!(
        resp["error"].is_null(),
        "query_components error: {}",
        resp["error"]
    );

    // Unknown tool → structured RPC error, not a crash
    request(
        &mut srv,
        "tools/call",
        serde_json::json!({ "name": "no_such_tool", "arguments": {} }),
        4,
    );
    let resp = read_response(&srv);
    assert_eq!(resp["error"]["code"], -32603);
}

#[test]
fn protocol_error_paths() {
    let db = fresh_db_path("err");
    let mut srv = spawn_server(&db);

    // Unknown method → RPC error
    request(&mut srv, "bogus/method", serde_json::json!({}), 1);
    let resp = read_response(&srv);
    assert_eq!(resp["error"]["code"], -32603);

    // Malformed JSON → -32700 parse error, connection stays alive
    writeln!(srv.stdin, "{{not json").expect("write garbage");
    srv.stdin.flush().expect("flush");
    let resp = read_response(&srv);
    assert_eq!(resp["error"]["code"], -32700);

    // Server still answers correctly afterwards
    request(&mut srv, "initialize", serde_json::json!({}), 2);
    let resp = read_response(&srv);
    assert_eq!(resp["result"]["serverInfo"]["name"], "kdesign");
}
