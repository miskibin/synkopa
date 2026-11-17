//! The plugin safety check: a plugin is tried in a throw-away process before WAVE first loads
//! it into its own.
//!
//! A hosted plugin runs inside WAVE, so a plugin that crashes takes the whole app (and the
//! unsaved song) down with it. Most such crashes are not subtle: the plugin dies the first
//! time it is created, run, asked for its state or freed (Calf Vinyl on Ubuntu 24.04 dies
//! in lilv's own `lv2bench` too). So the first time a plugin is about to be loaded, WAVE
//! starts itself as a worker (the scan worker's protocol, `wave_plugin_scan::worker`)
//! that does exactly those things with it -- create, run a test signal (notes, for an
//! instrument) at several block sizes, save its state, create a second instance from that
//! state, run it, free both -- and reports back. A worker that crashes or hangs costs
//! nothing but the worker; the plugin is then not loaded into WAVE, and its slot says why,
//! with a way to load it anyway.
//!
//! The verdict is kept per plugin in `plugin-safety.json` (WAVE's data folder) with the
//! fingerprint (length + modification time) of the plugin's file, so it is checked again
//! only when the plugin is updated. A plugin that passed, or that the user chose to trust,
//! loads straight away. This is not a sandbox: a plugin that crashes later (in its editor,
//! or on one parameter value) still takes WAVE down; the autosave and recovery cover that.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use wave_engine::{InstEvent, InstMsg, PluginRef, ProcessCtx};
use wave_plugin_scan::{worker, Fingerprint, WorkerOutcome, WorkerSpawn};

/// The worker format tag (`WAVE_SCAN_WORKER=plugin-check`).
pub const FORMAT: &str = "plugin-check";
/// The prefix of the error a refused plugin's slot shows: the UI offers "Load anyway" on it.
pub const UNSAFE: &str = "UNSAFE_PLUGIN: ";
/// How long the check may take. Generous: a sample-based instrument may read a gigabyte
/// before it is ready; finite: a plugin waiting for a licence dialog nobody will answer.
const TIMEOUT: Duration = Duration::from_secs(45);
const FILE: &str = "plugin-safety.json";

/// What the check is asked to do, sent to the worker as JSON in its "module" variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Request {
    pub format: String,
    pub path: String,
    pub cid: String,
    pub instrument: bool,
}

impl Request {
    pub fn of(p: &PluginRef, path: String, instrument: bool) -> Self {
        Self { format: p.format.clone(), path, cid: p.cid.clone(), instrument }
    }
    fn key(&self) -> String {
        format!("{}|{}|{}|{}", self.format, if self.instrument { "inst" } else { "fx" }, self.cid, self.path)
    }
    /// The file whose change means "check again": the VST3 module, the LV2 bundle.
    fn fingerprint(&self) -> Option<Fingerprint> {
        let p = Path::new(&self.path);
        if self.format == PluginRef::VST3 && p.is_dir() {
            // a bundle: the binary inside changes on update, the folder's time may not
            let bin = vst3_binary(p);
            return bin.as_deref().and_then(Fingerprint::of).or_else(|| Fingerprint::of(p));
        }
        Fingerprint::of(p)
    }
}

fn vst3_binary(bundle: &Path) -> Option<PathBuf> {
    let stem = bundle.file_stem()?.to_owned();
    let dir = bundle.join("Contents");
    let candidates = [
        dir.join("x86_64-linux").join(&stem).with_extension("so"),
        dir.join("aarch64-linux").join(&stem).with_extension("so"),
        dir.join("x86_64-win").join(&stem).with_extension("vst3"),
        dir.join("MacOS").join(&stem),
    ];
    candidates.into_iter().find(|c| c.is_file())
}

/// The verdict on one plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "v", rename_all = "lowercase")]
pub enum Verdict {
    /// It survived the check.
    Passed,
    /// The check's process died: how, in a sentence.
    Crashed { why: String },
    /// The check did not finish in time.
    Hung,
    /// The user chose to load it anyway.
    Trusted,
    /// The user chose to load it anyway a moment ago, in WAVE process `pid`: it becomes
    /// `Trusted` once it has run for [`PROVISIONAL`]. Still `Trying` when another WAVE starts
    /// and that process is gone means WAVE did not survive it: it is then `Crashed` again,
    /// so a recovered project does not crash on it once more.
    Trying { pid: u32 },
}

/// How long a plugin loaded anyway must keep WAVE alive before it is trusted for good.
const PROVISIONAL: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    fp: Option<Fingerprint>,
    verdict: Verdict,
}

/// The verdicts, and how a check is run.
pub struct Safety {
    file: Option<PathBuf>,
    spawn: WorkerSpawn,
    known: Mutex<HashMap<String, Entry>>,
    /// The plugins being checked now: a second caller for the same plugin waits for the first
    /// (a project opening with the same plugin on five inserts checks it once).
    running: Mutex<std::collections::HashSet<String>>,
    done: parking_lot::Condvar,
}

impl Safety {
    /// The user's verdicts, checks in a WAVE worker process.
    pub fn load() -> Self {
        Self::with(wave_plugin_scan::data_file(FILE), WorkerSpawn::Auto)
    }

    pub fn with(file: Option<PathBuf>, spawn: WorkerSpawn) -> Self {
        let known: HashMap<String, Entry> =
            file.as_deref().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let s = Self { file, spawn, known: Mutex::new(known), running: Mutex::new(Default::default()), done: parking_lot::Condvar::new() };
        // a plugin loaded anyway that did not see its trial through took WAVE down with it
        let died: Vec<(String, Entry)> = s
            .known
            .lock()
            .iter()
            .filter(|(_, e)| matches!(e.verdict, Verdict::Trying { pid } if !process_alive(pid)))
            .map(|(k, e)| (k.clone(), Entry { fp: e.fp, verdict: Verdict::Crashed { why: DIED_IN_WAVE.into() } }))
            .collect();
        for (k, e) in died {
            log::warn!("plugin {k}: WAVE closed while it was being loaded anyway; it is not loaded again");
            s.known.lock().insert(k.clone(), e.clone());
            s.save(&k, &e);
        }
        s
    }

    /// Write `key`'s entry to the file, keeping what other WAVE processes (another window,
    /// the CLI) wrote there since this one read it.
    fn save(&self, key: &str, entry: &Entry) {
        let Some(f) = &self.file else { return };
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut on_disk = std::fs::read(f).ok().and_then(|b| serde_json::from_slice::<HashMap<String, Entry>>(&b).ok()).unwrap_or_default();
        on_disk.insert(key.to_owned(), entry.clone());
        if let Ok(b) = serde_json::to_vec_pretty(&on_disk) {
            let tmp = f.with_extension(format!("json.{}.tmp", std::process::id()));
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(&tmp, f);
            }
        }
    }

    /// The verdict still valid for `req` (same plugin file), if any.
    pub fn verdict(&self, req: &Request) -> Option<Verdict> {
        let fp = req.fingerprint();
        self.known.lock().get(&req.key()).filter(|e| e.fp == fp).map(|e| e.verdict.clone())
    }

    fn remember(&self, req: &Request, verdict: Verdict) {
        let entry = Entry { fp: req.fingerprint(), verdict };
        let mut known = self.known.lock();
        known.insert(req.key(), entry.clone());
        self.save(&req.key(), &entry);
    }

    /// `Ok` when the plugin may be loaded into WAVE: it passed (now or before), the user
    /// trusts it, or it cannot be checked here (no worker; the plugin refused to load in
    /// the check too, which loading it here will say again). `Err` with the sentence its
    /// slot shows, starting with [`UNSAFE`], when it crashed or hung.
    pub fn check(&self, req: &Request) -> Result<(), String> {
        if let Some(v) = self.verdict(req) {
            return refusal(&v);
        }
        {
            let mut running = self.running.lock();
            while running.contains(&req.key()) {
                self.done.wait(&mut running);
            }
            if let Some(v) = self.verdict(req) {
                return refusal(&v);
            }
            running.insert(req.key());
        }
        let result = self.check_now(req);
        self.running.lock().remove(&req.key());
        self.done.notify_all();
        result
    }

    fn check_now(&self, req: &Request) -> Result<(), String> {
        let Ok(module) = serde_json::to_string(req) else { return Ok(()) };
        let started = std::time::Instant::now();
        let outcome = worker::scan_in_child(&self.spawn, FORMAT, Path::new(&module), TIMEOUT);
        let verdict = match outcome {
            // no worker in this build or process (tests, a library user): load as before
            None => return Ok(()),
            Some(WorkerOutcome::Scanned(_)) => Verdict::Passed,
            // it would not even be created; loading it here reports that, and nothing is
            // remembered (a missing file or dependency may be fixed)
            Some(WorkerOutcome::Refused(why)) => {
                log::info!("plugin check {}: refused to load ({why})", req.cid);
                return Ok(());
            }
            Some(WorkerOutcome::Crashed(why)) => Verdict::Crashed { why },
            Some(WorkerOutcome::TimedOut) => Verdict::Hung,
        };
        log::info!("plugin check {} ({}): {:?} in {} ms", req.cid, req.format, verdict, started.elapsed().as_millis());
        self.remember(req, verdict.clone());
        refusal(&verdict)
    }

    /// Load it from now on, whatever the check said ("Load anyway"): provisionally, until it
    /// has kept WAVE alive for [`PROVISIONAL`] (see [`Verdict::Trying`]).
    pub fn trust(self: &std::sync::Arc<Self>, req: &Request) {
        self.remember(req, Verdict::Trying { pid: std::process::id() });
        let (me, req) = (std::sync::Arc::clone(self), req.clone());
        let _ = std::thread::Builder::new().name("wave-plugin-trial".into()).spawn(move || {
            std::thread::sleep(PROVISIONAL);
            me.survived(&req);
        });
    }

    /// The trial is over and WAVE is still here: the plugin is trusted for good.
    fn survived(&self, req: &Request) {
        if matches!(self.verdict(req), Some(Verdict::Trying { .. })) {
            self.remember(req, Verdict::Trusted);
        }
    }
}

/// The worker's sentence about how it died, said about this check (it is worded for a scan).
fn how_it_died(why: &str) -> String {
    if why.starts_with("the test process") || why == DIED_IN_WAVE {
        return why.to_owned();
    }
    if let Some(code) = why.split("exit code ").nth(1).and_then(|r| r.split(')').next()) {
        // a shell-style 128 + n is a signal; Windows says 0xC0000005 for an access violation
        return match code.trim().parse::<i32>() {
            Ok(c) if c > 128 && c < 160 => format!("the test process was killed by signal {}", c - 128),
            _ => format!("the test process exited with code {}", code.trim()),
        };
    }
    if why.contains("access violation") {
        "the test process crashed (access violation)".into()
    } else if why.contains("terminated") {
        "the test process was killed by a crash signal".into()
    } else if why.contains("without reporting") {
        "the test process ended without finishing".into()
    } else {
        "the test process crashed".into()
    }
}

/// Whether process `pid` still runs (a PID reused since is taken for alive: the trial then
/// simply ends later, in that process's own `survived`, or never, and stays allowed).
fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 only checks that the process exists.
        let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY: a query-only handle, closed here; `code` outlives the call.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok && code == STILL_ACTIVE as u32
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true
    }
}

/// The reason kept for a plugin that WAVE did not survive after it was loaded anyway.
const DIED_IN_WAVE: &str = "WAVE closed while it was loaded anyway";

fn refusal(v: &Verdict) -> Result<(), String> {
    match v {
        Verdict::Passed | Verdict::Trusted | Verdict::Trying { .. } => Ok(()),
        Verdict::Crashed { why } if why == DIED_IN_WAVE => Err(format!(
            "{UNSAFE}it crashed WAVE when it was loaded anyway, so it is not loaded again.              Update the plugin, or load it anyway once more at your own risk."
        )),
        Verdict::Crashed { why } => Err(format!(
            "{UNSAFE}it crashed in WAVE's safety check ({}), so it was not loaded: loaded here it would close WAVE. \
             Update the plugin, or load it anyway at your own risk.",
            how_it_died(why)
        )),
        Verdict::Hung => Err(format!(
            "{UNSAFE}it did not finish loading within {} s in WAVE's safety check (it may be waiting for a licence \
             window), so it was not loaded. Load it anyway if you expect it to take that long.",
            TIMEOUT.as_secs()
        )),
    }
}

// ---------------------------------------------------------------- the worker side

/// **Call first in `main`**, before the VST3 scan worker: when this process was started as
/// a plugin check, it runs the check, prints the result and exits.
pub fn run_if_requested() -> bool {
    match worker::worker_request() {
        Some((format, _)) if format == FORMAT => {
            worker::run_worker_if_requested(&[worker::WorkerHandler { format: FORMAT, scan: check_in_worker }]);
            true
        }
        _ => false,
    }
}

fn check_in_worker(module: &Path) -> Result<serde_json::Value, String> {
    let req: Request = serde_json::from_str(&module.to_string_lossy()).map_err(|e| format!("bad plugin-check request: {e}"))?;
    exercise(&req)?;
    Ok(serde_json::json!([]))
}

/// How the check asks a live effect for its state.
type StateOf = Box<dyn Fn() -> Result<Vec<u8>, String>>;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;

fn ctx(beat: f64) -> ProcessCtx {
    ProcessCtx { sample_rate: SR, bpm: 120.0, beat, playing: true }
}

/// What the check does with the plugin: everything a session does with it first, except
/// its editor (a window cannot be checked without a screen and a user).
pub fn exercise(req: &Request) -> Result<(), String> {
    if req.instrument {
        let (mut a, ha) = wave_vst3::instantiate_instrument(&req.path, &req.cid, None, SR, BLOCK, false).map_err(|e| e.to_string())?;
        play(&mut *a);
        let state = ha.get_state().map_err(|e| e.to_string())?;
        let (mut b, _hb) =
            wave_vst3::instantiate_instrument(&req.path, &req.cid, Some(&state), SR, BLOCK, false).map_err(|e| e.to_string())?;
        play(&mut *b);
        drop(b);
        drop(a);
        return Ok(());
    }
    let make = |state: Option<&[u8]>| -> Result<(Box<dyn wave_engine::Processor>, StateOf), String> {
        match req.format.as_str() {
            PluginRef::LV2 => {
                let (p, h) = wave_lv2::instantiate(&req.cid, state, SR, BLOCK)?;
                Ok((p, Box::new(move || h.get_state())))
            }
            PluginRef::VST3 => {
                let (p, h) = wave_vst3::instantiate(&req.path, &req.cid, state, SR, BLOCK).map_err(|e| e.to_string())?;
                Ok((p, Box::new(move || h.get_state().map_err(|e| e.to_string()))))
            }
            f => Err(format!("WAVE does not host `{f}` plugins")),
        }
    };
    let (mut a, state_a) = make(None)?;
    run(&mut *a);
    let state = state_a()?;
    let (mut b, _) = make(Some(&state))?;
    run(&mut *b);
    drop(b);
    drop(a);
    Ok(())
}

/// Half a second of a quiet sine with a click, at the block sizes a session uses.
fn run(p: &mut dyn wave_engine::Processor) {
    let mut beat = 0.0;
    let mut t = 0usize;
    for n in [BLOCK, 64, 333, 1, BLOCK, BLOCK, 128] {
        for _ in 0..8 {
            let mut l: Vec<f32> = (0..n).map(|i| 0.25 * ((t + i) as f32 * 0.0575).sin()).collect();
            if t == 0 {
                l[0] = 0.9;
            }
            let mut r = l.clone();
            p.process(&mut l, &mut r, &ctx(beat));
            t += n;
            beat += n as f64 / f64::from(SR) * 2.0;
        }
    }
}

/// Notes in (on, then off half a block later), at the block sizes a session uses.
fn play(i: &mut dyn wave_engine::Instrument) {
    let mut beat = 0.0;
    let mut id = 0i32;
    for n in [BLOCK, 64, 333, 1, BLOCK, BLOCK, 128] {
        for step in 0..8 {
            let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
            let key = 48 + (id % 24) as u8;
            let msg = if step % 2 == 0 { InstMsg::NoteOn { key, vel: 0.8, id } } else { InstMsg::NoteOff { key, id } };
            i.process(&[InstEvent { offset: (n / 2) as u32, msg }], &mut l, &mut r, &ctx(beat));
            id += i32::from(step % 2 == 1);
            beat += n as f64 / f64::from(SR) * 2.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::sync::Arc;

    fn req(path: &Path) -> Request {
        Request { format: "lv2".into(), path: path.to_string_lossy().into_owned(), cid: "urn:test:fx".into(), instrument: false }
    }

    /// A child that does what `body` says (a shell line), with its stdout piped like a worker's.
    fn child(body: &'static str) -> WorkerSpawn {
        WorkerSpawn::Custom(Arc::new(move |_, _| {
            #[cfg(unix)]
            let mut c = Command::new("sh");
            #[cfg(unix)]
            c.arg("-c").arg(body);
            #[cfg(windows)]
            let mut c = Command::new("cmd");
            #[cfg(windows)]
            {
                // cmd reads its own command line: Rust's quoting would reach `echo` as `\"`
                use std::os::windows::process::CommandExt;
                c.raw_arg("/C").raw_arg(body);
            }
            c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        }))
    }

    #[cfg(unix)]
    const CRASH: &str = "kill -SEGV $$";
    #[cfg(windows)]
    const CRASH: &str = "exit -1073741819";
    #[cfg(unix)]
    const PASS: &str = r#"echo 'WAVE-SCAN-RESULT {"status":"ok","v":1,"plugins":[]}'"#;
    #[cfg(windows)]
    const PASS: &str = r#"echo WAVE-SCAN-RESULT {"status":"ok","v":1,"plugins":[]}"#;
    #[cfg(unix)]
    const REFUSE: &str = r#"echo 'WAVE-SCAN-RESULT {"status":"error","v":1,"message":"no such plugin"}'"#;
    #[cfg(windows)]
    const REFUSE: &str = r#"echo WAVE-SCAN-RESULT {"status":"error","v":1,"message":"no such plugin"}"#;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wave-safety-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_plugin_that_crashes_the_check_is_refused_until_trusted_and_the_verdict_is_kept() {
        let dir = scratch("crash");
        let plugin = dir.join("bad.lv2");
        std::fs::write(&plugin, b"v1").unwrap();
        let file = dir.join(FILE);
        let s = Safety::with(Some(file.clone()), child(CRASH));
        let r = req(&plugin);
        let e = s.check(&r).unwrap_err();
        assert!(e.starts_with(UNSAFE) && e.contains("crashed"), "{e}");
        assert!(matches!(s.verdict(&r), Some(Verdict::Crashed { .. })));
        // remembered across a restart, without running anything (a passing child would be ignored)
        let again = std::sync::Arc::new(Safety::with(Some(file.clone()), child(PASS)));
        assert!(again.check(&r).is_err(), "the crash verdict is kept");
        again.trust(&r);
        assert!(again.check(&r).is_ok(), "load anyway");
        assert_eq!(again.verdict(&r), Some(Verdict::Trying { pid: std::process::id() }), "on trial until it has run a while");
        let other = Safety::with(Some(file.clone()), child(PASS));
        assert!(other.check(&r).is_ok(), "another WAVE starting meanwhile does not end this one's trial");
        again.survived(&r);
        assert_eq!(Safety::with(Some(file), child(CRASH)).verdict(&r), Some(Verdict::Trusted));
    }

    #[test]
    fn a_plugin_loaded_anyway_that_took_wave_down_is_refused_at_the_next_start() {
        let dir = scratch("trial");
        let plugin = dir.join("bad.lv2");
        std::fs::write(&plugin, b"v1").unwrap();
        let file = dir.join(FILE);
        let s = std::sync::Arc::new(Safety::with(Some(file.clone()), child(CRASH)));
        let r = req(&plugin);
        assert!(s.check(&r).is_err());
        s.trust(&r);
        assert!(s.check(&r).is_ok());
        // WAVE dies before the trial ends (a process that has exited stands for it)
        let dead = {
            let mut c = std::process::Command::new(std::env::current_exe().unwrap());
            let mut child = c.arg("--list").stdout(std::process::Stdio::null()).spawn().unwrap();
            let pid = child.id();
            child.wait().unwrap();
            pid
        };
        s.remember(&r, Verdict::Trying { pid: dead });
        let next = Safety::with(Some(file.clone()), child(PASS));
        let e = next.check(&r).unwrap_err();
        assert!(e.contains("crashed WAVE when it was loaded anyway"), "{e}");
        assert!(Safety::with(Some(file), child(PASS)).check(&r).is_err(), "kept as crashed");
    }

    #[test]
    fn verdicts_written_by_another_process_are_kept() {
        let dir = scratch("merge");
        let (a, b) = (dir.join("a.lv2"), dir.join("b.lv2"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let file = dir.join(FILE);
        let one = Safety::with(Some(file.clone()), child(CRASH));
        let two = std::sync::Arc::new(Safety::with(Some(file.clone()), child(CRASH)));
        assert!(one.check(&req(&a)).is_err());
        two.trust(&req(&b));
        two.survived(&req(&b));
        let fresh = Safety::with(Some(file), child(PASS));
        assert!(matches!(fresh.verdict(&req(&a)), Some(Verdict::Crashed { .. })), "the other window's verdict survives");
        assert_eq!(fresh.verdict(&req(&b)), Some(Verdict::Trusted));
    }

    #[test]
    fn an_updated_plugin_is_checked_again() {
        let dir = scratch("update");
        let plugin = dir.join("p.lv2");
        std::fs::write(&plugin, b"v1").unwrap();
        let s = Safety::with(None, child(CRASH));
        let r = req(&plugin);
        assert!(s.check(&r).is_err());
        std::fs::write(&plugin, b"version 2, longer").unwrap();
        assert_eq!(s.verdict(&r), None, "a new file is a new plugin");
        let fixed = Safety::with(None, child(PASS));
        assert!(fixed.check(&r).is_ok());
        assert_eq!(fixed.verdict(&r), Some(Verdict::Passed));
    }

    #[test]
    fn a_refusal_or_no_worker_loads_as_before_and_is_not_remembered() {
        let dir = scratch("refuse");
        let plugin = dir.join("p.lv2");
        std::fs::write(&plugin, b"x").unwrap();
        let r = req(&plugin);
        let s = Safety::with(None, child(REFUSE));
        assert!(s.check(&r).is_ok());
        assert_eq!(s.verdict(&r), None);
        let none = Safety::with(None, WorkerSpawn::InProcess);
        assert!(none.check(&r).is_ok(), "no worker: nothing to isolate with");
        assert_eq!(none.verdict(&r), None);
    }

    #[test]
    fn the_same_plugin_asked_for_at_once_is_checked_once() {
        let dir = scratch("once");
        let plugin = dir.join("p.lv2");
        std::fs::write(&plugin, b"x").unwrap();
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let spawn = WorkerSpawn::Custom(Arc::new(move |f, m| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match child(PASS) {
                WorkerSpawn::Custom(make) => make(f, m),
                _ => unreachable!(),
            }
        }));
        let s = Safety::with(None, spawn);
        let r = req(&plugin);
        std::thread::scope(|sc| {
            for _ in 0..4 {
                sc.spawn(|| assert!(s.check(&r).is_ok()));
            }
        });
        assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn crash_reasons_are_said_about_the_check() {
        assert_eq!(how_it_died("the scan worker crashed reading this module (exit code 139)"), "the test process was killed by signal 11");
        assert_eq!(how_it_died("the scan worker crashed reading this module (exit code 3)"), "the test process exited with code 3");
        assert_eq!(
            how_it_died("the scan worker was terminated while reading this module"),
            "the test process was killed by a crash signal"
        );
        assert_eq!(
            how_it_died("the scan worker crashed reading this module (access violation)"),
            "the test process crashed (access violation)"
        );
    }

    #[test]
    fn requests_travel_as_json_and_keys_tell_effects_from_instruments() {
        let r = Request { format: "vst3".into(), path: "/p/Synth.vst3".into(), cid: "ABC".into(), instrument: true };
        let back: Request = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        let fx = Request { instrument: false, ..r.clone() };
        assert_ne!(r.key(), fx.key());
    }
}
