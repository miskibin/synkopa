//! `synkopa mcp` as a host runs it: the real binary, JSON-RPC lines on stdin, replies on stdout.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

/// Sends `requests` to a fresh server and returns its replies, in the order written.
fn talk(requests: &[Value]) -> Vec<Value> {
    // no Synkopa app of this machine may answer: the tests need the "not running" path
    let sessions = std::env::temp_dir().join(format!("synkopa-mcp-it-{}", std::process::id()));
    std::fs::create_dir_all(&sessions).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_synkopa"))
        .arg("mcp")
        .env("WAVE_SESSION_DIR", &sessions)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
    }
    // closing stdin is the shutdown signal: the server answers what it has and exits
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("stdout must hold only JSON-RPC messages, got `{l}`: {e}")))
        .collect()
}

fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } })
}

fn text(reply: &Value) -> &str {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("no text in {reply}"))
}

#[test]
fn a_host_shakes_hands_lists_the_tools_and_runs_commands_in_order() {
    let replies = talk(&[
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        call(3, "synkopa_run", json!({ "args": ["sessions"] })),
        call(4, "synkopa_run", json!({ "args": ["help"] })),
        call(5, "synkopa_context", json!({})),
        call(6, "synkopa_script", json!({ "lines": ["status"] })),
        call(7, "synkopa_run", json!({ "args": ["no-such-command"] })),
    ]);
    let ids: Vec<u64> = replies.iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6, 7], "one reply per request, calls in the order sent, none for the notification");

    assert_eq!(replies[0]["result"]["protocolVersion"], json!("2025-11-25"));
    assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 4);

    // a command's JSON comes back as it prints it
    assert_eq!(replies[2]["result"]["isError"], json!(false));
    assert_eq!(serde_json::from_str::<Value>(text(&replies[2])).unwrap(), json!([]));
    // `help` is text, not JSON: it still arrives whole
    assert!(text(&replies[3]).contains("PATTERNS & CHANNELS"));

    // no app: the failure is a tool error that says what to do next
    assert_eq!(replies[4]["result"]["isError"], json!(true));
    assert!(text(&replies[4]).contains("not running"), "{}", text(&replies[4]));
    assert!(text(&replies[4]).contains("pass `project`"));
    // the script's lines reach the command through its stdin
    assert_eq!(replies[5]["result"]["isError"], json!(true));
    assert!(text(&replies[5]).contains("line 1 (`status`)"), "{}", text(&replies[5]));
    assert_eq!(replies[6]["result"]["isError"], json!(true));
}

#[test]
fn a_stateless_client_needs_no_handshake() {
    let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28" });
    let replies = talk(&[
        json!({ "jsonrpc": "2.0", "id": "d", "method": "server/discover", "params": { "_meta": meta } }),
        json!({ "jsonrpc": "2.0", "id": "c", "method": "tools/call", "params": { "name": "synkopa_run", "arguments": { "args": ["sessions"] }, "_meta": meta } }),
        json!({ "jsonrpc": "2.0", "id": "x", "method": "tools/list", "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "2031-01-01" } } }),
    ]);
    let by_id = |id: &str| replies.iter().find(|r| r["id"] == json!(id)).unwrap_or_else(|| panic!("no reply to {id}"));
    assert_eq!(by_id("d")["result"]["supportedVersions"][0], json!("2026-07-28"));
    assert_eq!(by_id("c")["result"]["resultType"], json!("complete"));
    assert_eq!(by_id("c")["result"]["isError"], json!(false));
    assert_eq!(by_id("x")["error"]["code"], json!(-32022));
}

#[test]
fn a_broken_line_is_answered_and_the_server_keeps_serving() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_synkopa"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{{not json").unwrap();
    writeln!(stdin).unwrap();
    writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "id": 9, "method": "ping" })).unwrap();
    drop(stdin);
    let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    let replies: Vec<Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["error"]["code"], json!(-32700));
    assert_eq!(replies[1]["id"], json!(9));
}
