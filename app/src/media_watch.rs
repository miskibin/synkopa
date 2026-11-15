//! Hot reload of the project's audio files: a file re-bounced or edited in another app while
//! the project is open plays its new audio within about a second, waveforms included.
//!
//! One thread (`wave-media-watch`) watches the folders of the project's source files
//! (`notify`, not recursive) and decides when a file is done changing ([`Tracker`]): it waits
//! for the events to stop (a writer appends in many steps), then for the file's size and time
//! to hold still, and only then has the engine decode it ([`Engine::reload_source_file`], which
//! swaps the audio in through the usual timeline ring, never blocking the audio thread). A file
//! that does not decode yet (still being written, or locked by its writer) is tried again with
//! a growing pause; a deleted file keeps playing from memory and is reported once.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use serde::Serialize;
use wave_engine::reload::file_key;
use wave_engine::{Engine, Id};

/// Events must stop for this long before a file is looked at.
pub const QUIET: Duration = Duration::from_millis(300);
/// Then its size and time must hold still for this long.
pub const SETTLE: Duration = Duration::from_millis(250);
/// First pause before decoding a file again that did not decode; doubled each time.
pub const RETRY: Duration = Duration::from_millis(500);
/// Decodes tried for one change before it is reported as failed (~8 s in all).
pub const TRIES: u32 = 5;
/// Folders watched at most (one OS watch each); the files of further folders are not watched.
pub const MAX_DIRS: usize = 1024;

/// What tells one version of a file from another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

/// The file's stamp, `None` when there is no such file.
pub fn stamp(p: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(p).ok().filter(|m| m.is_file())?;
    Some(Stamp { len: m.len(), modified: m.modified().ok() })
}

/// What a [`Tracker::poll`] found to do.
#[derive(Debug, PartialEq)]
pub enum Due {
    /// The file changed and held still: decode it.
    Reload(PathBuf, Stamp),
    /// The file is gone (deleted, or renamed away).
    Gone(PathBuf),
}

struct Entry {
    path: PathBuf,
    /// The version the engine plays (`None`: none, the file was missing).
    stamp: Option<Stamp>,
    /// When to look at the file next.
    due: Option<Instant>,
    /// What it looked like the last time, while waiting for it to hold still.
    sample: Option<Stamp>,
    /// Decodes of the current change that failed.
    tries: u32,
}

/// When a watched file is done changing. Pure (the clock and the file system are passed
/// in), so its decisions are tested without timing.
#[derive(Default)]
pub struct Tracker {
    files: HashMap<String, Entry>,
}

impl Tracker {
    /// The files to watch now. Files already watched keep what is known of them; new ones are
    /// stamped as they are (the engine just decoded them).
    pub fn set_files(&mut self, files: &[PathBuf], stat: &dyn Fn(&Path) -> Option<Stamp>) {
        let mut next = HashMap::with_capacity(files.len());
        for f in files {
            let k = file_key(f);
            let e = self.files.remove(&k).unwrap_or_else(|| Entry { path: f.clone(), stamp: stat(f), due: None, sample: None, tries: 0 });
            next.insert(k, e);
        }
        self.files = next;
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.files.len()
    }

    /// The folders that hold the watched files.
    pub fn dirs(&self) -> BTreeSet<PathBuf> {
        self.files.values().filter_map(|e| e.path.parent().map(Path::to_path_buf)).collect()
    }

    /// Something happened to `path`. False when it is not a watched file.
    pub fn touched(&mut self, path: &Path, now: Instant) -> bool {
        let Some(e) = self.files.get_mut(&file_key(path)) else { return false };
        e.due = Some(now + QUIET);
        true
    }

    /// The watch lost events (an overflow): look at every file.
    pub fn touch_all(&mut self, now: Instant) {
        for e in self.files.values_mut() {
            e.due = Some(now + QUIET);
        }
    }

    /// When [`Self::poll`] has something to look at next.
    pub fn next_due(&self) -> Option<Instant> {
        self.files.values().filter_map(|e| e.due).min()
    }

    /// Looks at the files that are due.
    pub fn poll(&mut self, now: Instant, stat: &dyn Fn(&Path) -> Option<Stamp>) -> Vec<Due> {
        let mut out = vec![];
        for e in self.files.values_mut().filter(|e| e.due.is_some_and(|d| d <= now)) {
            let Some(s) = stat(&e.path) else {
                (e.due, e.sample, e.tries) = (None, None, 0);
                // told once; it plays from memory until it comes back
                if e.stamp.take().is_some() {
                    out.push(Due::Gone(e.path.clone()));
                }
                continue;
            };
            if e.tries == 0 && e.stamp == Some(s) {
                // touched, not changed (read, attributes, a save of the same bytes and time)
                (e.due, e.sample) = (None, None);
            } else if e.sample != Some(s) {
                // still being written: wait for it to hold still
                (e.due, e.sample) = (Some(now + SETTLE), Some(s));
            } else {
                (e.due, e.sample) = (None, None);
                out.push(Due::Reload(e.path.clone(), s));
            }
        }
        out
    }

    /// The engine plays version `s` of the file now (or does not play the file any more).
    pub fn loaded(&mut self, path: &Path, s: Stamp) {
        if let Some(e) = self.files.get_mut(&file_key(path)) {
            (e.stamp, e.tries) = (Some(s), 0);
        }
    }

    /// Version `s` of the file did not decode. True when this change is given up on (it is
    /// tried again when the file changes again); otherwise it is tried again later.
    pub fn failed(&mut self, path: &Path, s: Stamp, now: Instant) -> bool {
        let Some(e) = self.files.get_mut(&file_key(path)) else { return true };
        e.tries += 1;
        if e.tries >= TRIES {
            (e.stamp, e.tries) = (Some(s), 0);
            return true;
        }
        (e.due, e.sample) = (Some(now + RETRY * 2u32.pow(e.tries - 1)), Some(s));
        false
    }
}

/// What the musician is told (the `media-reload` event).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaEvent {
    /// `reloaded`, `failed` (kept the audio it had) or `gone` (playing it from memory).
    pub kind: &'static str,
    pub path: String,
    /// The first source's name.
    pub name: String,
    pub source_ids: Vec<Id>,
    /// The file's length now, seconds (reloaded only).
    pub seconds: Option<f64>,
    pub length_differs: bool,
    pub was_missing: bool,
    pub error: Option<String>,
}

enum Msg {
    Files(Vec<PathBuf>),
    Fs(notify::Result<notify::Event>),
    Stop,
}

/// The running watcher. Dropping it stops the thread.
pub struct MediaWatch {
    tx: mpsc::Sender<Msg>,
    last: Mutex<Vec<PathBuf>>,
}

impl MediaWatch {
    /// Starts watching the engine's source files; `report` hears what happened to them.
    pub fn start(engine: Arc<Engine>, report: impl Fn(MediaEvent) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let fs_tx = tx.clone();
        let files = engine.source_files();
        let me = MediaWatch { tx: tx.clone(), last: Mutex::new(files.clone()) };
        let _ = tx.send(Msg::Files(files));
        let spawned = std::thread::Builder::new().name("wave-media-watch".into()).spawn(move || {
            let watcher = notify::recommended_watcher(move |ev| {
                let _ = fs_tx.send(Msg::Fs(ev));
            });
            let watcher = match watcher {
                Ok(w) => Some(w),
                Err(e) => {
                    log::warn!("audio files are not watched for changes: {e}");
                    None
                }
            };
            run(rx, watcher, &engine, &report);
        });
        if let Err(e) = spawned {
            log::warn!("audio files are not watched for changes: {e}");
        }
        me
    }

    /// The project's source files now (after any change of the project; cheap when they are
    /// the same).
    pub fn set_files(&self, files: Vec<PathBuf>) {
        let mut last = self.last.lock();
        if *last != files {
            *last = files.clone();
            let _ = self.tx.send(Msg::Files(files));
        }
    }
}

impl Drop for MediaWatch {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

fn relevant(kind: &notify::EventKind) -> bool {
    use notify::event::{AccessKind, AccessMode};
    use notify::EventKind;
    match kind {
        // reading a file (our own decode included) is not a change; a writer closing it is
        EventKind::Access(a) => matches!(a, AccessKind::Close(AccessMode::Write)),
        _ => true,
    }
}

fn run(rx: mpsc::Receiver<Msg>, mut watcher: Option<notify::RecommendedWatcher>, engine: &Engine, report: &dyn Fn(MediaEvent)) {
    use notify::Watcher;
    let mut tracker = Tracker::default();
    let mut watched: BTreeSet<PathBuf> = BTreeSet::new();
    loop {
        let wait = tracker.next_due().map_or(Duration::from_secs(3600), |d| d.saturating_duration_since(Instant::now()));
        match rx.recv_timeout(wait) {
            Ok(Msg::Files(files)) => {
                tracker.set_files(&files, &stamp);
                if let Some(w) = watcher.as_mut() {
                    let want: BTreeSet<PathBuf> = tracker.dirs().into_iter().take(MAX_DIRS).collect();
                    for d in watched.difference(&want) {
                        let _ = w.unwatch(d);
                    }
                    for d in want.difference(&watched) {
                        // a folder that is not there cannot be watched: its files stay missing
                        if let Err(e) = w.watch(d, notify::RecursiveMode::NonRecursive) {
                            log::info!("not watching {}: {e}", d.display());
                        }
                    }
                    watched = want;
                }
            }
            Ok(Msg::Fs(Ok(ev))) => {
                let now = Instant::now();
                if ev.need_rescan() {
                    tracker.touch_all(now);
                } else if relevant(&ev.kind) {
                    for p in &ev.paths {
                        tracker.touched(p, now);
                    }
                }
            }
            Ok(Msg::Fs(Err(e))) => {
                log::info!("file watch: {e}");
                tracker.touch_all(Instant::now());
            }
            Ok(Msg::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        for due in tracker.poll(Instant::now(), &stamp) {
            if let Some(ev) = act(&mut tracker, engine, due) {
                report(ev);
            }
        }
    }
}

/// Does what [`Tracker::poll`] found due; what to tell the musician, if anything.
fn act(tracker: &mut Tracker, engine: &Engine, due: Due) -> Option<MediaEvent> {
    match due {
        Due::Reload(path, s) => match engine.reload_source_file(&path) {
            Ok(Some(r)) => {
                tracker.loaded(&path, s);
                log::info!("reloaded {} (changed on disk)", path.display());
                Some(MediaEvent {
                    kind: "reloaded",
                    path: r.path,
                    name: r.name,
                    source_ids: r.source_ids,
                    seconds: Some(r.seconds),
                    length_differs: r.length_differs,
                    was_missing: r.was_missing,
                    error: None,
                })
            }
            Ok(None) => {
                tracker.loaded(&path, s);
                None
            }
            Err(e) => {
                let e = e.to_string();
                if !tracker.failed(&path, s, Instant::now()) {
                    return None;
                }
                log::warn!("could not reload {}: {e}", path.display());
                Some(about(engine, "failed", &path, Some(e)))
            }
        },
        Due::Gone(path) => Some(about(engine, "gone", &path, None)),
    }
}

fn about(engine: &Engine, kind: &'static str, path: &Path, error: Option<String>) -> MediaEvent {
    let p = engine.project();
    let sources: Vec<_> = p.sources.iter().filter(|s| wave_engine::reload::same_file(&s.path, path)).collect();
    MediaEvent {
        kind,
        path: path.display().to_string(),
        name: sources.first().map_or_else(|| path.file_name().unwrap_or_default().to_string_lossy().into_owned(), |s| s.name.clone()),
        source_ids: sources.iter().map(|s| s.id).collect(),
        seconds: None,
        length_differs: false,
        was_missing: false,
        error,
    }
}

#[cfg(test)]
#[path = "media_watch_tests.rs"]
mod tests;
