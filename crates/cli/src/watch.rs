//! `synkopa watch FILE`: live coding. The script FILE (the same lines as `synkopa script`) describes
//! the song: on every save it is run on a copy of the project as it was when the watch started,
//! and the result replaces the open project as one undo step, while the loop keeps playing. So
//! the file always says what the song is (delete a line and its effect is gone), as in Sonic Pi
//! or Strudel, where the whole buffer is evaluated again on every run.
//!
//! The copy is a project held by this process ([`Doc`]): a line that fails stops that run
//! before anything reaches the app. With `--project`, the copy is the file's project (never
//! saved) and each run can be bounced with `--render`, with no app at all.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use wave_engine::errors::ERR_REVISION_CONFLICT;

use super::{abs, need_files, run, run_lines, session, tidy_numbers, ws, Args, Cx, Doc, Fail, Files};

/// How often the file is looked at.
const POLL: Duration = Duration::from_millis(150);
/// A changed file must hold still this long before it is read (an editor may write in steps).
const SETTLE: Duration = Duration::from_millis(120);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp(p: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(p).ok().filter(|m| m.is_file())?;
    Some(Stamp { len: m.len(), modified: m.modified().ok() })
}

/// Where the result goes.
enum Target {
    /// The running app; `rev` is the revision of the project it has from the last run (or
    /// from the start), so an edit made there meanwhile is not silently replaced.
    App { ep: ws::Endpoint, rev: u64 },
    /// `--project FILE`: the copy is the result (bounced with `--render`, never saved).
    File,
}

struct Watch {
    file: PathBuf,
    base: Value,
    scratch: Box<dyn Doc>,
    target: Target,
    render: Option<String>,
    force: bool,
}

pub(super) fn watch(files: Option<&dyn Files>, a: &Args) -> Result<Value, Fail> {
    let u = Fail::Usage;
    let files = need_files(files)?;
    let raw = a.pos.get(1).ok_or_else(|| u("watch <FILE> [--render OUT.wav] [--once] [--force]".into()))?;
    let file = abs(raw);
    if a.has("render") && a.get("render").is_none() {
        return Err(u("--render needs a file: --render preview.wav".into()));
    }
    let once = a.has("once");
    if once && !file.is_file() {
        return Err(u(format!("{raw} does not exist")));
    }
    let (scratch, base, target) = match a.get("project") {
        Some(p) => {
            if a.has("session") {
                return Err(u("--session talks to a running app and --project works on a file; pick one".into()));
            }
            let path = abs(p);
            if !path.is_file() {
                return Err(u(format!("{p} does not exist (create it with `synkopa new {p}`)")));
            }
            let (doc, warnings) = files.open(&path).map_err(|e| Fail::App(format!("OPEN_FAILED: {e}")))?;
            for w in &warnings {
                eprintln!("synkopa: warning: {w}");
            }
            let base = full(&*doc)?;
            (doc, base, Target::File)
        }
        None => {
            let ep = session(a)?;
            let rep = ws::call(&ep, "context", json!({ "full": true }), None, None)
                .map_err(|e| Fail::Conn(format!("could not reach Synkopa: {e}")))?;
            if !rep.ok {
                let e = rep.error.unwrap_or_else(|| ws::ErrorBody::new("ERROR", "unknown error"));
                return Err(Fail::App(format!("{}: {}", e.code, e.message)));
            }
            let base = rep.data.get("project").cloned().ok_or_else(|| Fail::App("REFUSED: no project in the reply".into()))?;
            let rev = rep.data.get("revision").and_then(Value::as_u64).unwrap_or(rep.revision);
            let doc = files.create("empty").map_err(|e| Fail::App(format!("OPEN_FAILED: {e}")))?;
            (doc, base, Target::App { ep, rev })
        }
    };
    let mut w = Watch { file, base, scratch, target, render: a.get("render").map(|r| abs(r).display().to_string()), force: a.has("force") };
    if once {
        let ev = w.run_once()?;
        return Ok(ev);
    }
    eprintln!("synkopa: watching {} (Ctrl+C stops; every save replaces the song)", w.file.display());
    let mut applied: Option<Stamp> = None;
    let mut told_missing = false;
    loop {
        match stamp(&w.file) {
            None => {
                if !told_missing {
                    eprintln!("synkopa: waiting for {} to exist", w.file.display());
                    told_missing = true;
                }
            }
            Some(s) if Some(s) != applied => {
                told_missing = false;
                std::thread::sleep(SETTLE);
                if stamp(&w.file) == Some(s) {
                    applied = Some(s);
                    let ev = match w.run_once() {
                        Ok(ev) => ev,
                        Err(e) => {
                            let m = match e {
                                Fail::Usage(m) | Fail::Conn(m) | Fail::App(m) => m,
                            };
                            eprintln!("synkopa: {m}");
                            json!({ "event": "failed", "file": w.file.display().to_string(), "error": m })
                        }
                    };
                    emit(ev);
                }
            }
            Some(_) => {}
        }
        std::thread::sleep(POLL);
    }
}

fn emit(mut ev: Value) {
    use std::io::Write;
    tidy_numbers(&mut ev);
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", serde_json::to_string(&ev).unwrap_or_default());
    let _ = out.flush();
}

fn full(doc: &dyn Doc) -> Result<Value, Fail> {
    let v = doc.call("context", &json!({ "full": true }), None).map_err(|e| Fail::App(super::app_error(e)))?;
    v.get("project").cloned().ok_or_else(|| Fail::App("REFUSED: no project".into()))
}

impl Watch {
    /// Runs the file once on a fresh copy and hands the result on. Nothing reaches the app when
    /// a line fails.
    fn run_once(&mut self) -> Result<Value, Fail> {
        let text = std::fs::read_to_string(&self.file).map_err(|e| Fail::Usage(format!("{}: {e}", self.file.display())))?;
        self.scratch.call("set_project", &json!({ "project": self.base }), None).map_err(|e| Fail::App(super::app_error(e)))?;
        let cx = Cx { doc: Some(&*self.scratch) };
        // on the copy a failing line "saved nothing"; what matters here is that nothing reached the song
        let not_applied = |m: String| m.replace("; nothing was saved", "; nothing was applied (the song stays as the last good save)");
        let results = run_lines(&cx, &Args::parse(vec![]), &text).map_err(|e| match e {
            Fail::Usage(m) => Fail::Usage(not_applied(m)),
            Fail::Conn(m) => Fail::Conn(not_applied(m)),
            Fail::App(m) => Fail::App(not_applied(m)),
        })?;
        let mut ev = json!({ "event": "applied", "file": self.file.display().to_string(), "lines": results.len() });
        if let Target::App { ep, rev } = &mut self.target {
            let project = full(&*self.scratch)?;
            let label = format!("synkopa watch: {}", self.file.file_name().unwrap_or_default().to_string_lossy());
            let guard = if self.force { None } else { Some(*rev) };
            let rep = ws::call(ep, "set_project", json!({ "project": project }), guard, Some(label))
                .map_err(|e| Fail::Conn(format!("could not reach Synkopa: {e}")))?;
            if !rep.ok {
                let e = rep.error.unwrap_or_else(|| ws::ErrorBody::new("ERROR", "unknown error"));
                if e.code == "REVISION_CONFLICT" {
                    return Err(Fail::App(format!(
                        "{ERR_REVISION_CONFLICT} the project changed in Synkopa since the watch last applied {} (an edit in the app or another command); the file was not applied, so that edit is kept. Restart `synkopa watch` to build on the project as it is now, or add --force to let the file replace it (the replaced state stays one undo away)",
                        self.file.display()
                    )));
                }
                return Err(Fail::App(format!("{}: {}", e.code, e.message)));
            }
            let new_rev = rep.data.get("revision").and_then(Value::as_u64).unwrap_or(rep.revision);
            ev["changed"] = json!(new_rev != *rev);
            *rev = new_rev;
            ev["revision"] = json!(new_rev);
        } else {
            ev["revision"] = json!(self.scratch.revision());
        }
        if let Some(out) = &self.render {
            let r = run(&cx, "render", &Args::parse(vec!["render".into(), "--out".into(), out.clone()]))?;
            ev["render"] = r;
        }
        Ok(ev)
    }
}
