//! `synkopa mcp`: the command line as a Model Context Protocol server on stdin/stdout, so an
//! agent host (Claude Code, Cursor, VS Code, Codex …) can call Synkopa as tools without a shell.
//!
//! Every tool call runs this same executable again as a child (`synkopa <args>`), so a tool does
//! exactly what the command does, and a command that crashes (a hosted plugin, a model) or prints
//! to stdout cannot break the protocol stream. Calls run one at a time, in the order received:
//! two edits of one project file never race. A cancelled call's child is killed.
//!
//! Both protocol eras are served: the `initialize` handshake (revisions up to 2025-11-25) and
//! the stateless form of 2026-07-28 (`server/discover`, the version in each request's `_meta`).

use std::collections::HashSet;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use super::{Args, HELP};

/// The stateless revision: version and identity travel in every request's `_meta`.
const MODERN: &str = "2026-07-28";
/// Revisions opened with `initialize`, newest first.
const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const SERVER_INFO_KEY: &str = "io.modelcontextprotocol/serverInfo";
const HELP_URI: &str = "synkopa://cli-help";

/// How long the lists may be cached by a 2026-07-28 client: they only change with the app version.
const LIST_TTL_MS: u64 = 3_600_000;
/// After stdin closes, a running call gets this long to finish before its child is killed.
const EOF_GRACE: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(20);

const UNSUPPORTED_VERSION: i64 = -32022;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INVALID_REQUEST: i64 = -32600;
const PARSE_ERROR: i64 = -32700;
/// "Resource not found" before 2026-07-28 (which uses [`INVALID_PARAMS`]).
const LEGACY_RESOURCE_NOT_FOUND: i64 = -32002;

const INSTRUCTIONS: &str = "\
Synkopa is a digital audio workstation with the FL Studio workflow: Channel Rack, patterns, piano roll, Playlist, mixer. These tools run its `synkopa` command line and return its JSON.

Where a call acts:
- Without `project`: the project open in the running Synkopa app. Every edit is one step in the app's undo history, so the musician can take it back with Ctrl+Z. If the app is not running the call fails and says so.
- With `project` (absolute path of a .synkopa file): the file is loaded, the command runs, and the file is saved back if it changed. No app, window or audio device is needed. Create a file with synkopa_run [\"new\", \"/abs/song.synkopa\", \"--template\", \"demo\"].

How to work:
1. Read first: synkopa_context. Its `revision` and `nextId` are needed for edits.
2. Look up before guessing: synkopa_run [\"help\"] lists every command; [\"schema\"] every op synkopa_apply accepts, with an example; [\"catalog\"] effect types, parameters, presets and instruments.
3. Edit with synkopa_run (one command), synkopa_apply (several ops as one atomic undo step) or synkopa_script (several commands, one load and one save on a file).
4. Check the result without speakers: synkopa_run [\"render\", \"--out\", \"/abs/check.wav\"] bounces offline and returns peak, RMS, loudness and clipping.

Units: Playlist time in beats (4 = one bar); pattern step and len in sixteenths; track and insert vol is a fader position (0.7937 = 0 dB); clip gain in dB; pan -1..1. New ids count up from context.nextId.

Errors read `CODE: message`. REVISION_CONFLICT: the musician changed the project meanwhile, read it again and decide; do not retry blindly. NEEDS_APP: play, stop, undo, redo and auditions need the running app. FILE_OPEN: that file is open in the app; drop `project` and edit it there. Paths are resolved against this server's working folder: pass absolute paths.";

/// What one tool call runs: `synkopa <args>`, with `stdin` piped in and `temp` written first.
#[derive(Debug, PartialEq)]
struct Invocation {
    args: Vec<String>,
    stdin: Option<String>,
    /// A file to write before the run and remove after it (the ops of `apply --file`).
    temp: Option<(PathBuf, String)>,
}

/// How a child ended.
struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// What to do with one received message.
enum Step {
    /// A notification, or something that needs no answer.
    Nothing,
    Reply(Value),
    Call {
        id: Value,
        modern: bool,
        inv: Invocation,
    },
    Cancel(Value),
}

fn server_info() -> Value {
    json!({ "name": "synkopa", "title": "Synkopa", "version": env!("CARGO_PKG_VERSION") })
}

fn capabilities() -> Value {
    json!({ "tools": {}, "resources": {} })
}

fn supported_versions() -> Vec<&'static str> {
    std::iter::once(MODERN).chain(LEGACY.iter().copied()).collect()
}

fn tools() -> Value {
    let project = json!({
        "type": "string",
        "description": "Absolute path of a .synkopa project file to work on without the app (loaded, edited, saved back). Omit to act on the project open in the running Synkopa app."
    });
    let session = json!({
        "type": "string",
        "description": "Only when several Synkopa apps are open: the id (or its first characters) of the one to use, from synkopa_run [\"sessions\"]."
    });
    json!([
        {
            "name": "synkopa_context",
            "title": "Read the project",
            "description": "Read the Synkopa project: tempo, key, Playlist tracks, mixer inserts with their effects and parameters, patterns, Channel Rack channels, audio sources, markers, plus `revision` and `nextId`. Call this before any edit. Changes nothing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "full": { "type": "boolean", "description": "true: the whole project JSON (every clip, note and parameter). Large; the default summary is enough for most edits." },
                    "project": project,
                    "session": session
                }
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "synkopa_run",
            "title": "Run a synkopa command",
            "description": "Run one `synkopa` command and return its JSON result. `args` are the words after `synkopa`, one array item each (no shell quoting), e.g. [\"bpm\", \"140\"], [\"pattern\", \"--id\", \"1\", \"--ch\", \"kick\", \"--steps\", \"x... x... x... x...\"], [\"add-fx\", \"--insert\", \"3\", \"--type\", \"reverb\"], [\"render\", \"--out\", \"/abs/mix.wav\"], [\"undo\"]. Covers everything the command line does: channels and instruments, patterns and notes, note generators, effects, routing, sidechain, automation, markers, audio import, stem separation, chord detection, pitch editing, auto vocal, mix assistant, MIDI files, rendering. [\"help\"] returns the full command reference; [\"schema\"] the ops of synkopa_apply; [\"catalog\"] effect types, parameters, presets and instruments. Each edit is one undo step.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "args": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "The command and its options, one word per item. Numbers are written as strings (\"140\")." },
                    "project": project,
                    "session": session
                },
                "required": ["args"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false }
        },
        {
            "name": "synkopa_apply",
            "title": "Apply project ops atomically",
            "description": "Apply a batch of project ops as one atomic change and one undo step: all of them apply, or none. Use it for related edits (a pattern with its notes, a clip and its automation). Op names, fields, units and an example of each come from synkopa_run [\"schema\"]. Example ops: [{\"op\": \"setBpm\", \"bpm\": 128}, {\"op\": \"setLoop\", \"loop\": {\"a\": 0, \"b\": 16}}].",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ops": { "type": "array", "items": { "type": "object" }, "minItems": 1, "description": "The ops, in order." },
                    "label": { "type": "string", "description": "Name of the undo step, as the musician sees it in the Edit menu (e.g. \"Busier hats\")." },
                    "ifRevision": { "type": "integer", "minimum": 0, "description": "The `revision` from your last read: the edit is refused with REVISION_CONFLICT if the project changed since." },
                    "dryRun": { "type": "boolean", "description": "With `project`: try the ops and report whether they change the project, without saving the file." },
                    "project": project,
                    "session": session
                },
                "required": ["ops"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false }
        },
        {
            "name": "synkopa_script",
            "title": "Run several synkopa commands",
            "description": "Run several `synkopa` commands in order, one per line, written as on a command line: words separated by spaces, '…' or \"…\" around a word with spaces (no escapes, so JSON goes in '…'). Example lines: \"bpm 128\", \"pattern --id 1 --ch hat --steps 'x.x. x.x. x.x5 x.x9'\", \"render --out /abs/preview.wav\". With `project` the file is loaded once and saved once at the end, and nothing is saved if a line fails. Without it the lines run in the app one by one: lines before a failing one stay applied, each as its own undo step. The result lists each line's reply.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "lines": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "One command per item, without the leading `synkopa`." },
                    "saveAs": { "type": "string", "description": "With `project`: save the result to this file instead, leaving the project file as it was." },
                    "dryRun": { "type": "boolean", "description": "With `project`: run the lines and save nothing." },
                    "project": project,
                    "session": session
                },
                "required": ["lines"]
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false }
        }
    ])
}

fn resources() -> Value {
    json!([{
        "uri": HELP_URI,
        "name": "cli-help",
        "title": "Synkopa command reference",
        "description": "Every `synkopa` command with its options: what synkopa_run accepts.",
        "mimeType": "text/plain"
    }])
}

fn error(id: &Value, code: i64, message: impl Into<String>, data: Option<Value>) -> Value {
    let mut e = json!({ "code": code, "message": message.into() });
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": e })
}

/// A result message; a 2026-07-28 request also gets `resultType`, the server's identity and,
/// for the cacheable lists, how long they stay valid.
fn result(id: &Value, mut body: Value, modern: bool, cacheable: bool) -> Value {
    if modern {
        if let Value::Object(m) = &mut body {
            m.insert("resultType".into(), json!("complete"));
            m.insert("_meta".into(), json!({ SERVER_INFO_KEY: server_info() }));
            if cacheable {
                m.insert("ttlMs".into(), json!(LIST_TTL_MS));
                m.insert("cacheScope".into(), json!("public"));
            }
        }
    }
    json!({ "jsonrpc": "2.0", "id": id, "result": body })
}

/// A tool result that reports a failure the model can act on (bad arguments, a refused command).
fn tool_error(message: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": message.into() }], "isError": true })
}

fn tool_result(o: &Outcome) -> Value {
    let (out, err) = (o.stdout.trim(), o.stderr.trim());
    if o.code == Some(0) {
        let mut content = vec![json!({ "type": "text", "text": if out.is_empty() { "ok" } else { out } })];
        if !err.is_empty() {
            content.push(json!({ "type": "text", "text": format!("warnings:\n{err}") }));
        }
        return json!({ "content": content, "isError": false });
    }
    let mut text = if err.is_empty() { out.to_string() } else { err.to_string() };
    if text.is_empty() {
        text = match o.code {
            Some(c) => format!("synkopa exited with code {c}"),
            None => "synkopa was terminated".into(),
        };
    } else if !err.is_empty() && !out.is_empty() {
        text = format!("{text}\n{out}");
    }
    // exit 2: no running app answered. The command's own hint names a shell flag; say it in tool terms
    if o.code == Some(2) {
        text.push_str("\nTo work without the app, pass `project`: the absolute path of a .synkopa file.");
    }
    tool_error(text)
}

fn text_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s)),
        Some(Value::String(_)) => Ok(None),
        Some(_) => Err(format!("`{key}` must be a string")),
    }
}

fn flag_arg(args: &Map<String, Value>, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("`{key}` must be true or false")),
    }
}

fn words_arg(args: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    let list = args.get(key).and_then(Value::as_array).ok_or_else(|| format!("`{key}` must be an array of strings"))?;
    let words: Vec<String> = list
        .iter()
        .map(|w| match w {
            Value::String(s) => Ok(s.clone()),
            // a model often writes ["bpm", 140]: the number is unambiguous
            Value::Number(n) => Ok(n.to_string()),
            _ => Err(format!("`{key}` must be an array of strings")),
        })
        .collect::<Result<_, _>>()?;
    if words.is_empty() {
        return Err(format!("`{key}` is empty"));
    }
    Ok(words)
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_file() -> PathBuf {
    let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("synkopa-mcp-{}-{n}.json", std::process::id()))
}

/// The command a tool call stands for, or why it cannot run (shown to the model).
fn plan(tool: &str, args: &Map<String, Value>) -> Result<Invocation, String> {
    // where the call acts goes first: `--project`, `--session`, `--save-as` and `--dry-run`
    // belong to the whole invocation, also for `script`
    let mut argv: Vec<String> = vec![];
    if let Some(p) = text_arg(args, "project")? {
        argv.extend(["--project".into(), p.into()]);
    }
    if let Some(s) = text_arg(args, "session")? {
        argv.extend(["--session".into(), s.into()]);
    }
    let on_file = text_arg(args, "project")?.is_some();
    let mut file_only = |key: &str, flag: &str, value: Option<&str>| -> Result<(), String> {
        if !on_file {
            return Err(format!("`{key}` needs `project`: it is about saving a project file"));
        }
        argv.push(flag.into());
        argv.extend(value.map(str::to_owned));
        Ok(())
    };
    match tool {
        "synkopa_context" => {
            argv.push("context".into());
            if flag_arg(args, "full")? {
                argv.push("--full".into());
            }
            Ok(Invocation { args: argv, stdin: None, temp: None })
        }
        "synkopa_run" => {
            let words = words_arg(args, "args")?;
            let parsed = Args::parse(words.clone());
            match parsed.pos.first().map(String::as_str) {
                None => return Err("`args` names no command: the first word is the command, e.g. [\"status\"]".into()),
                Some("mcp") => return Err("`mcp` is this server; pick a command from synkopa_run [\"help\"]".into()),
                Some("watch") if !parsed.has("once") => {
                    return Err("`watch` runs until it is stopped, so it cannot be a tool call; add \"--once\" to apply the file once, or use synkopa_script".into())
                }
                Some("script") if parsed.pos.get(1).map(String::as_str) == Some("-") => {
                    return Err("`script -` reads the terminal's input; pass the lines to synkopa_script instead".into())
                }
                Some("synkopa" | "wave") => return Err("leave out the leading `synkopa`: `args` starts with the command, e.g. [\"status\"]".into()),
                _ => {}
            }
            argv.extend(words);
            Ok(Invocation { args: argv, stdin: None, temp: None })
        }
        "synkopa_apply" => {
            let ops = args
                .get("ops")
                .and_then(Value::as_array)
                .filter(|o| !o.is_empty())
                .ok_or("`ops` must be a non-empty array of ops (see synkopa_run [\"schema\"])")?;
            if flag_arg(args, "dryRun")? {
                file_only("dryRun", "--dry-run", None)?;
            }
            let file = temp_file();
            argv.extend(["apply".into(), "--file".into(), file.display().to_string()]);
            if let Some(l) = text_arg(args, "label")? {
                argv.extend(["--label".into(), l.into()]);
            }
            match args.get("ifRevision") {
                None | Some(Value::Null) => {}
                Some(v) => {
                    let n = v.as_u64().ok_or("`ifRevision` must be a whole number: the `revision` of your last read")?;
                    argv.extend(["--if-revision".into(), n.to_string()]);
                }
            }
            let text = serde_json::to_string(ops).map_err(|e| e.to_string())?;
            Ok(Invocation { args: argv, stdin: None, temp: Some((file, text)) })
        }
        "synkopa_script" => {
            let lines = words_arg(args, "lines")?;
            if let Some(i) = lines.iter().position(|l| l.contains('\n') || l.contains('\r')) {
                return Err(format!("line {} holds a line break: one command per item of `lines`", i + 1));
            }
            if let Some(f) = text_arg(args, "saveAs")? {
                file_only("saveAs", "--save-as", Some(f))?;
            }
            if flag_arg(args, "dryRun")? {
                file_only("dryRun", "--dry-run", None)?;
            }
            argv.extend(["script".into(), "-".into()]);
            Ok(Invocation { args: argv, stdin: Some(lines.join("\n")), temp: None })
        }
        _ => Err(String::new()),
    }
}

const TOOL_NAMES: &[&str] = &["synkopa_context", "synkopa_run", "synkopa_apply", "synkopa_script"];

/// Decides what one parsed message asks for. Pure: no process is started here.
fn step(msg: &Value) -> Step {
    let Some(obj) = msg.as_object() else {
        return Step::Reply(error(&Value::Null, INVALID_REQUEST, "a message must be a JSON object", None));
    };
    let id = obj.get("id").cloned();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // a response to a request this server never sends, or junk without a method
        return match id {
            Some(id) if !obj.contains_key("result") && !obj.contains_key("error") => {
                Step::Reply(error(&id, INVALID_REQUEST, "the message has no method", None))
            }
            _ => Step::Nothing,
        };
    };
    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));
    let Some(id) = id else {
        return match method {
            "notifications/cancelled" => params.get("requestId").cloned().map_or(Step::Nothing, Step::Cancel),
            _ => Step::Nothing,
        };
    };
    let asked = params.get("_meta").and_then(|m| m.get(VERSION_KEY)).and_then(Value::as_str);
    let modern = asked.is_some();
    if let Some(v) = asked {
        if !supported_versions().contains(&v) {
            let data = json!({ "supported": supported_versions(), "requested": v });
            return Step::Reply(error(&id, UNSUPPORTED_VERSION, "Unsupported protocol version", Some(data)));
        }
    }
    match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = LEGACY.iter().find(|v| **v == asked).copied().unwrap_or(LEGACY[0]);
            let body = json!({ "protocolVersion": version, "capabilities": capabilities(), "serverInfo": server_info(), "instructions": INSTRUCTIONS });
            Step::Reply(result(&id, body, false, false))
        }
        "server/discover" => {
            let body = json!({ "supportedVersions": supported_versions(), "capabilities": capabilities(), "instructions": INSTRUCTIONS });
            Step::Reply(result(&id, body, true, true))
        }
        "ping" => Step::Reply(result(&id, json!({}), modern, false)),
        "tools/list" => Step::Reply(result(&id, json!({ "tools": tools() }), modern, true)),
        "resources/list" => Step::Reply(result(&id, json!({ "resources": resources() }), modern, true)),
        "resources/templates/list" => Step::Reply(result(&id, json!({ "resourceTemplates": [] }), modern, true)),
        "resources/read" => match params.get("uri").and_then(Value::as_str) {
            Some(HELP_URI) => {
                let body = json!({ "contents": [{ "uri": HELP_URI, "mimeType": "text/plain", "text": HELP }] });
                Step::Reply(result(&id, body, modern, true))
            }
            other => {
                let code = if modern { INVALID_PARAMS } else { LEGACY_RESOURCE_NOT_FOUND };
                Step::Reply(error(&id, code, "Resource not found", Some(json!({ "uri": other }))))
            }
        },
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if !TOOL_NAMES.contains(&name) {
                return Step::Reply(error(&id, INVALID_PARAMS, format!("Unknown tool: {name}"), None));
            }
            let empty = Map::new();
            let args = match params.get("arguments") {
                None | Some(Value::Null) => &empty,
                Some(Value::Object(m)) => m,
                Some(_) => return Step::Reply(result(&id, tool_error("`arguments` must be an object"), modern, false)),
            };
            match plan(name, args) {
                Ok(inv) => Step::Call { id, modern, inv },
                Err(why) => Step::Reply(result(&id, tool_error(why), modern, false)),
            }
        }
        other => Step::Reply(error(&id, METHOD_NOT_FOUND, format!("Method not found: {other}"), None)),
    }
}

/// One received line: a message or (2025-03-26 only) a batch of them.
fn parse(line: &str) -> Result<Vec<Value>, Value> {
    match serde_json::from_str::<Value>(line) {
        Ok(Value::Array(a)) if !a.is_empty() => Ok(a),
        Ok(Value::Array(_)) => Err(error(&Value::Null, INVALID_REQUEST, "an empty batch", None)),
        Ok(v) => Ok(vec![v]),
        Err(e) => Err(error(&Value::Null, PARSE_ERROR, format!("Parse error: {e}"), None)),
    }
}

/// The call being run and the ids whose answer is no longer wanted.
#[derive(Default)]
struct Running {
    id: Option<String>,
    child: Option<Child>,
    cancelled: HashSet<String>,
}

struct Job {
    id: Value,
    modern: bool,
    inv: Invocation,
}

fn key(id: &Value) -> String {
    id.to_string()
}

fn write_line(out: &Mutex<std::io::Stdout>, msg: &Value) {
    let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
    // a client that went away closes the pipe: there is nobody left to tell
    let _ = writeln!(o, "{msg}");
    let _ = o.flush();
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = vec![];
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// `synkopa` as this process was started: the app binary is the command line behind `--cli`.
fn relaunch() -> Result<Command, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate the synkopa executable: {e}"))?;
    let mut cmd = Command::new(exe);
    if std::env::args().nth(1).as_deref() == Some("--cli") {
        cmd.arg("--cli");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: a host that starts this server hidden must not get a console per call
        cmd.creation_flags(0x0800_0000);
    }
    Ok(cmd)
}

/// Runs one invocation to its end; `None` when it was cancelled meanwhile.
fn execute(job_key: &str, inv: &Invocation, running: &Mutex<Running>) -> Result<Option<Outcome>, String> {
    if let Some((file, text)) = &inv.temp {
        std::fs::write(file, text).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    let spawned = relaunch().and_then(|mut cmd| {
        cmd.args(&inv.args)
            .stdin(if inv.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start synkopa: {e}"))
    });
    let outcome = spawned.map(|mut child| {
        if let (Some(mut pipe), Some(text)) = (child.stdin.take(), inv.stdin.clone()) {
            std::thread::spawn(move || {
                let _ = pipe.write_all(text.as_bytes());
            });
        }
        let out = child.stdout.take().map(drain);
        let err = child.stderr.take().map(drain);
        running.lock().unwrap_or_else(|e| e.into_inner()).child = Some(child);
        let code = loop {
            let mut r = running.lock().unwrap_or_else(|e| e.into_inner());
            match r.child.as_mut().map(Child::try_wait) {
                Some(Ok(None)) => {}
                Some(Ok(Some(status))) => break status.code(),
                Some(Err(_)) | None => break None,
            }
            drop(r);
            std::thread::sleep(POLL);
        };
        let read = |h: Option<std::thread::JoinHandle<String>>| h.and_then(|h| h.join().ok()).unwrap_or_default();
        Outcome { code, stdout: read(out), stderr: read(err) }
    });
    if let Some((file, _)) = &inv.temp {
        let _ = std::fs::remove_file(file);
    }
    let mut r = running.lock().unwrap_or_else(|e| e.into_inner());
    r.child = None;
    r.id = None;
    if r.cancelled.remove(job_key) {
        return Ok(None);
    }
    outcome.map(Some)
}

fn work(jobs: mpsc::Receiver<Job>, running: &Mutex<Running>, out: &Mutex<std::io::Stdout>) {
    for job in jobs {
        let k = key(&job.id);
        {
            let mut r = running.lock().unwrap_or_else(|e| e.into_inner());
            if r.cancelled.remove(&k) {
                continue;
            }
            r.id = Some(k.clone());
        }
        let body = match execute(&k, &job.inv, running) {
            Ok(Some(o)) => tool_result(&o),
            Ok(None) => continue,
            Err(why) => tool_error(why),
        };
        write_line(out, &result(&job.id, body, job.modern, false));
    }
}

/// Stops waiting for `id`: kills its child if it runs, or drops it from the queue when its turn comes.
fn cancel(id: &Value, running: &Mutex<Running>) {
    let k = key(id);
    let mut r = running.lock().unwrap_or_else(|e| e.into_inner());
    if r.id.as_deref() == Some(k.as_str()) {
        if let Some(child) = r.child.as_mut() {
            let _ = child.kill();
        }
    }
    r.cancelled.insert(k);
}

/// Serves until stdin closes.
pub(super) fn serve() -> ExitCode {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let running = Arc::new(Mutex::new(Running::default()));
    let (queue, jobs) = mpsc::channel::<Job>();
    let worker = {
        let (running, out) = (running.clone(), out.clone());
        std::thread::spawn(move || work(jobs, &running, &out))
    };
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let messages = match parse(&line) {
            Ok(m) => m,
            Err(e) => {
                write_line(&out, &e);
                continue;
            }
        };
        for msg in &messages {
            match step(msg) {
                Step::Nothing => {}
                Step::Reply(v) => write_line(&out, &v),
                Step::Cancel(id) => cancel(&id, &running),
                Step::Call { id, modern, inv } => {
                    let _ = queue.send(Job { id, modern, inv });
                }
            }
        }
    }
    // the host is gone: quick calls still answer (`echo … | synkopa mcp`), a long one is stopped
    drop(queue);
    let deadline = Instant::now() + EOF_GRACE;
    while !worker.is_finished() && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
    if !worker.is_finished() {
        let mut r = running.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(k) = r.id.clone() {
            r.cancelled.insert(k);
        }
        if let Some(child) = r.child.as_mut() {
            let _ = child.kill();
        }
        // calls still queued are answered by nobody: the process ends here
        return ExitCode::SUCCESS;
    }
    let _ = worker.join();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(msg: Value) -> Value {
        match step(&msg) {
            Step::Reply(v) => v,
            Step::Nothing => panic!("no reply to {msg}"),
            Step::Call { .. } => panic!("{msg} would run a command"),
            Step::Cancel(_) => panic!("{msg} is a cancellation"),
        }
    }

    fn call(tool: &str, arguments: Value) -> Result<Invocation, String> {
        let msg = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": tool, "arguments": arguments } });
        match step(&msg) {
            Step::Call { inv, .. } => Ok(inv),
            Step::Reply(v) => {
                assert_eq!(v["result"]["isError"], json!(true), "a refused call is a tool error the model can read: {v}");
                Err(v["result"]["content"][0]["text"].as_str().unwrap().to_string())
            }
            _ => panic!("tools/call gave neither a call nor a reply"),
        }
    }

    fn words(inv: &Invocation) -> Vec<&str> {
        inv.args.iter().map(String::as_str).collect()
    }

    fn modern_meta() -> Value {
        json!({ VERSION_KEY: MODERN })
    }

    #[test]
    fn the_handshake_answers_with_the_clients_revision_or_the_newest_legacy_one() {
        let r = reply(
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {} } }),
        );
        assert_eq!(r["id"], json!(1));
        assert_eq!(r["result"]["protocolVersion"], json!("2025-06-18"));
        assert_eq!(r["result"]["serverInfo"]["name"], json!("synkopa"));
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert!(r["result"]["instructions"].as_str().unwrap().contains("synkopa_context"));
        // a handshake reply carries no 2026 fields
        assert!(r["result"].get("resultType").is_none());

        let r = reply(json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "2099-01-01" } }));
        assert_eq!(r["result"]["protocolVersion"], json!(LEGACY[0]));
    }

    #[test]
    fn a_stateless_client_discovers_the_server_and_gets_complete_cacheable_results() {
        let r = reply(json!({ "jsonrpc": "2.0", "id": "d", "method": "server/discover", "params": { "_meta": modern_meta() } }));
        let res = &r["result"];
        assert_eq!(res["resultType"], json!("complete"));
        assert_eq!(res["supportedVersions"][0], json!(MODERN));
        assert_eq!(res["_meta"][SERVER_INFO_KEY]["name"], json!("synkopa"));
        assert_eq!(res["cacheScope"], json!("public"));
        assert!(res["ttlMs"].as_u64().unwrap() > 0);

        let r = reply(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": { "_meta": modern_meta() } }));
        assert_eq!(r["result"]["resultType"], json!("complete"));
        assert!(r["result"]["ttlMs"].is_u64());
    }

    #[test]
    fn an_unknown_revision_is_refused_with_the_supported_ones() {
        let r = reply(json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": { "_meta": { VERSION_KEY: "1900-01-01" } } }));
        assert_eq!(r["error"]["code"], json!(UNSUPPORTED_VERSION));
        assert_eq!(r["error"]["data"]["requested"], json!("1900-01-01"));
        assert!(r["error"]["data"]["supported"].as_array().unwrap().contains(&json!(MODERN)));
    }

    #[test]
    fn the_tool_list_is_stable_and_every_tool_has_an_object_schema() {
        let r = reply(json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/list" }));
        let tools = r["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, TOOL_NAMES);
        for t in tools {
            assert_eq!(t["inputSchema"]["type"], json!("object"), "{}", t["name"]);
            assert!(t["description"].as_str().unwrap().len() > 40);
            assert!(t["inputSchema"]["properties"]["project"].is_object(), "{} works on a file too", t["name"]);
        }
        // a handshake-era list carries no 2026 fields
        assert!(r["result"].get("ttlMs").is_none());
    }

    #[test]
    fn context_and_run_become_the_same_command_line() {
        assert_eq!(words(&call("synkopa_context", json!({})).unwrap()), ["context"]);
        let inv = call("synkopa_context", json!({ "full": true, "project": "C:/songs/a.synkopa" })).unwrap();
        assert_eq!(words(&inv), ["--project", "C:/songs/a.synkopa", "context", "--full"]);

        // every word is passed as it is: no shell splits the step string or the JSON
        let inv = call(
            "synkopa_run",
            json!({ "args": ["pattern", "--id", 1, "--ch", "kick", "--steps", "x... x... x... x..."], "session": "ab12" }),
        )
        .unwrap();
        assert_eq!(words(&inv), ["--session", "ab12", "pattern", "--id", "1", "--ch", "kick", "--steps", "x... x... x... x..."]);
        assert_eq!(inv.stdin, None);
    }

    #[test]
    fn run_refuses_what_cannot_be_a_tool_call_and_says_what_to_do() {
        assert!(call("synkopa_run", json!({ "args": ["watch", "song.txt"] })).unwrap_err().contains("--once"));
        assert!(call("synkopa_run", json!({ "args": ["watch", "song.txt", "--once"] })).is_ok());
        assert!(call("synkopa_run", json!({ "args": ["script", "-"] })).unwrap_err().contains("synkopa_script"));
        assert!(call("synkopa_run", json!({ "args": ["script", "edits.txt"] })).is_ok());
        assert!(call("synkopa_run", json!({ "args": ["mcp"] })).is_err());
        assert!(call("synkopa_run", json!({ "args": ["synkopa", "status"] })).unwrap_err().contains("leave out"));
        assert!(call("synkopa_run", json!({ "args": [] })).is_err());
        assert!(call("synkopa_run", json!({ "args": "status" })).is_err());
        assert!(call("synkopa_run", json!({})).is_err());
        // options alone name no command
        assert!(call("synkopa_run", json!({ "args": ["--pretty"] })).is_err());
    }

    #[test]
    fn apply_sends_the_ops_through_a_file_with_label_and_revision() {
        let ops = json!([{ "op": "setBpm", "bpm": 128 }, { "op": "putNotebook", "notebook": { "pages": [{ "text": "it's 'quoted'\nline 2" }] } }]);
        let inv = call("synkopa_apply", json!({ "ops": ops, "label": "Busier hats", "ifRevision": 31 })).unwrap();
        let (file, text) = inv.temp.clone().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), ops, "quotes and line breaks survive: no command line carries them");
        assert_eq!(words(&inv), ["apply", "--file", file.to_str().unwrap(), "--label", "Busier hats", "--if-revision", "31"]);

        let other = call("synkopa_apply", json!({ "ops": [{ "op": "setBpm", "bpm": 90 }] })).unwrap();
        assert_ne!(other.temp.unwrap().0, file, "two calls never share a file");

        assert!(call("synkopa_apply", json!({ "ops": [] })).is_err());
        assert!(call("synkopa_apply", json!({ "ops": [{}], "ifRevision": "soon" })).is_err());
    }

    #[test]
    fn saving_options_need_a_project_file_and_go_before_the_command() {
        let why = call("synkopa_apply", json!({ "ops": [{}], "dryRun": true })).unwrap_err();
        assert!(why.contains("project"), "{why}");
        let inv = call("synkopa_apply", json!({ "ops": [{}], "dryRun": true, "project": "/s/a.synkopa" })).unwrap();
        assert_eq!(&words(&inv)[..4], ["--project", "/s/a.synkopa", "--dry-run", "apply"]);

        assert!(call("synkopa_script", json!({ "lines": ["bpm 90"], "saveAs": "/s/b.synkopa" })).is_err());
        let inv = call(
            "synkopa_script",
            json!({ "lines": ["bpm 90", "render --out /s/a.wav"], "project": "/s/a.synkopa", "saveAs": "/s/b.synkopa" }),
        )
        .unwrap();
        assert_eq!(words(&inv), ["--project", "/s/a.synkopa", "--save-as", "/s/b.synkopa", "script", "-"]);
        assert_eq!(inv.stdin.as_deref(), Some("bpm 90\nrender --out /s/a.wav"));
    }

    #[test]
    fn a_script_line_with_a_line_break_is_refused() {
        let why = call("synkopa_script", json!({ "lines": ["bpm 90", "apply --json '[\n]'"] })).unwrap_err();
        assert!(why.contains("line 2"), "{why}");
    }

    #[test]
    fn unknown_tools_methods_and_resources_are_protocol_errors() {
        let r = reply(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "format_disk", "arguments": {} } }));
        assert_eq!(r["error"]["code"], json!(INVALID_PARAMS));
        let r = reply(json!({ "jsonrpc": "2.0", "id": 2, "method": "prompts/list" }));
        assert_eq!(r["error"]["code"], json!(METHOD_NOT_FOUND));
        let r = reply(json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": { "uri": "synkopa://nope" } }));
        assert_eq!(r["error"]["code"], json!(LEGACY_RESOURCE_NOT_FOUND));
        let r = reply(
            json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": { "uri": "synkopa://nope", "_meta": modern_meta() } }),
        );
        assert_eq!(r["error"]["code"], json!(INVALID_PARAMS));
    }

    #[test]
    fn the_command_reference_is_a_resource() {
        let r = reply(json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }));
        assert_eq!(r["result"]["resources"][0]["uri"], json!(HELP_URI));
        let r = reply(json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/read", "params": { "uri": HELP_URI } }));
        assert!(r["result"]["contents"][0]["text"].as_str().unwrap().contains("PATTERNS & CHANNELS"));
    }

    #[test]
    fn notifications_get_no_answer_and_a_cancellation_names_its_request() {
        assert!(matches!(step(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })), Step::Nothing));
        assert!(matches!(step(&json!({ "jsonrpc": "2.0", "id": 1, "result": {} })), Step::Nothing));
        match step(&json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 12 } })) {
            Step::Cancel(id) => assert_eq!(id, json!(12)),
            _ => panic!("not a cancellation"),
        }
        let r = reply(json!([1, 2]));
        assert_eq!(r["error"]["code"], json!(INVALID_REQUEST));
        assert_eq!(parse("{not json").unwrap_err()["error"]["code"], json!(PARSE_ERROR));
        assert_eq!(parse("[]").unwrap_err()["error"]["code"], json!(INVALID_REQUEST));
        assert_eq!(parse(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","id":2,"method":"ping"}]"#).unwrap().len(), 2);
    }

    #[test]
    fn a_finished_command_becomes_a_tool_result() {
        let ok = tool_result(&Outcome { code: Some(0), stdout: "{\"bpm\":140}\n".into(), stderr: String::new() });
        assert_eq!(ok["isError"], json!(false));
        assert_eq!(ok["content"][0]["text"], json!("{\"bpm\":140}"));

        let warned = tool_result(&Outcome { code: Some(0), stdout: "{}".into(), stderr: "synkopa: warning: kick.wav is missing\n".into() });
        assert!(warned["content"][1]["text"].as_str().unwrap().contains("kick.wav"));

        let refused = tool_result(&Outcome {
            code: Some(3),
            stdout: String::new(),
            stderr: "synkopa: REVISION_CONFLICT: the project is at 32\n".into(),
        });
        assert_eq!(refused["isError"], json!(true));
        assert!(refused["content"][0]["text"].as_str().unwrap().contains("REVISION_CONFLICT"));

        let no_app = tool_result(&Outcome { code: Some(2), stdout: String::new(), stderr: "synkopa: Synkopa is not running\n".into() });
        assert!(no_app["content"][0]["text"].as_str().unwrap().contains("pass `project`"));

        let silent = tool_result(&Outcome { code: None, stdout: String::new(), stderr: String::new() });
        assert_eq!(silent["content"][0]["text"], json!("synkopa was terminated"));
        let quiet_ok = tool_result(&Outcome { code: Some(0), stdout: String::new(), stderr: String::new() });
        assert_eq!(quiet_ok["content"][0]["text"], json!("ok"));
    }
}
