use super::*;
use std::cell::RefCell;
use std::sync::Arc;
use wave_engine::{NoPlugins, Project};

const SR: u32 = 48000;

fn st(len: u64) -> Option<Stamp> {
    Some(Stamp { len, modified: None })
}

/// A fake disk: path → stamp.
struct Disk(RefCell<HashMap<PathBuf, Stamp>>);
impl Disk {
    fn new(files: &[(&str, u64)]) -> Self {
        Disk(RefCell::new(files.iter().map(|(p, l)| (PathBuf::from(p), st(*l).unwrap())).collect()))
    }
    fn set(&self, p: &str, len: Option<u64>) {
        match len {
            Some(l) => self.0.borrow_mut().insert(p.into(), st(l).unwrap()),
            None => self.0.borrow_mut().remove(Path::new(p)),
        };
    }
    fn stat(&self) -> impl Fn(&Path) -> Option<Stamp> + '_ {
        |p| self.0.borrow().get(p).copied()
    }
}

fn tracker(disk: &Disk, files: &[&str]) -> Tracker {
    let mut t = Tracker::default();
    t.set_files(&files.iter().map(PathBuf::from).collect::<Vec<_>>(), &disk.stat());
    t
}

#[test]
fn a_file_is_reloaded_once_it_holds_still() {
    let disk = Disk::new(&[("/a/pad.wav", 100)]);
    let mut t = tracker(&disk, &["/a/pad.wav"]);
    let t0 = Instant::now();
    disk.set("/a/pad.wav", Some(50));
    assert!(t.touched(Path::new("/a/pad.wav"), t0));
    // more writes keep pushing the look back
    assert!(t.poll(t0 + QUIET / 2, &disk.stat()).is_empty());
    disk.set("/a/pad.wav", Some(80));
    t.touched(Path::new("/a/pad.wav"), t0 + QUIET / 2);
    assert!(t.poll(t0 + QUIET, &disk.stat()).is_empty(), "the second write restarted the wait");
    let t1 = t0 + QUIET / 2 + QUIET;
    assert!(t.poll(t1, &disk.stat()).is_empty(), "first look: a sample, not a reload");
    // still growing at the second look: wait again
    disk.set("/a/pad.wav", Some(200));
    assert!(t.poll(t1 + SETTLE, &disk.stat()).is_empty());
    assert_eq!(t.poll(t1 + SETTLE * 2, &disk.stat()), vec![Due::Reload("/a/pad.wav".into(), st(200).unwrap())]);
    t.loaded(Path::new("/a/pad.wav"), st(200).unwrap());
    assert_eq!(t.next_due(), None);
}

#[test]
fn a_touch_without_a_change_does_nothing() {
    let disk = Disk::new(&[("/a/pad.wav", 100)]);
    let mut t = tracker(&disk, &["/a/pad.wav"]);
    let t0 = Instant::now();
    t.touched(Path::new("/a/pad.wav"), t0);
    assert!(t.poll(t0 + QUIET, &disk.stat()).is_empty());
    assert_eq!(t.next_due(), None);
}

#[test]
fn files_not_in_the_project_are_ignored() {
    let disk = Disk::new(&[("/a/pad.wav", 100), ("/a/other.wav", 1)]);
    let mut t = tracker(&disk, &["/a/pad.wav"]);
    assert!(!t.touched(Path::new("/a/other.wav"), Instant::now()));
    assert_eq!(t.next_due(), None);
}

#[cfg(windows)]
#[test]
fn an_event_in_another_spelling_is_the_same_file() {
    let disk = Disk::new(&[(r"C:\Music\Pad.wav", 100)]);
    let mut t = tracker(&disk, &[r"C:\Music\Pad.wav"]);
    assert!(t.touched(Path::new("c:/music/pad.WAV"), Instant::now()));
}

#[test]
fn a_deleted_file_is_reported_once_and_reloaded_when_it_comes_back() {
    let disk = Disk::new(&[("/a/pad.wav", 100)]);
    let mut t = tracker(&disk, &["/a/pad.wav"]);
    let t0 = Instant::now();
    disk.set("/a/pad.wav", None);
    t.touched(Path::new("/a/pad.wav"), t0);
    assert_eq!(t.poll(t0 + QUIET, &disk.stat()), vec![Due::Gone("/a/pad.wav".into())]);
    t.touched(Path::new("/a/pad.wav"), t0 + QUIET);
    assert!(t.poll(t0 + QUIET * 2, &disk.stat()).is_empty(), "told once");
    // back (the same bytes, say from the recycle bin): the engine still decodes it
    disk.set("/a/pad.wav", Some(100));
    let t1 = t0 + QUIET * 3;
    t.touched(Path::new("/a/pad.wav"), t1);
    assert!(t.poll(t1 + QUIET, &disk.stat()).is_empty());
    assert!(matches!(t.poll(t1 + QUIET + SETTLE, &disk.stat())[..], [Due::Reload(..)]));
}

#[test]
fn an_atomic_save_is_one_reload_not_a_delete() {
    // editors write a temporary file and rename it over the old one: remove + create events
    let disk = Disk::new(&[("/a/pad.wav", 100)]);
    let mut t = tracker(&disk, &["/a/pad.wav"]);
    let t0 = Instant::now();
    disk.set("/a/pad.wav", None);
    t.touched(Path::new("/a/pad.wav"), t0);
    disk.set("/a/pad.wav", Some(120));
    t.touched(Path::new("/a/pad.wav"), t0 + Duration::from_millis(5));
    let t1 = t0 + Duration::from_millis(5) + QUIET;
    assert!(t.poll(t1, &disk.stat()).is_empty());
    assert!(matches!(t.poll(t1 + SETTLE, &disk.stat())[..], [Due::Reload(..)]));
}

#[test]
fn a_file_that_does_not_decode_is_tried_again_then_given_up() {
    let disk = Disk::new(&[("/a/pad.wav", 100)]);
    let mut t = tracker(&disk, &["/a/pad.wav"]);
    let p = Path::new("/a/pad.wav");
    let s = st(100).unwrap();
    let mut now = Instant::now();
    for k in 1..TRIES {
        assert!(!t.failed(p, s, now), "try {k} is not the last");
        let wait = RETRY * 2u32.pow(k - 1);
        assert_eq!(t.next_due(), Some(now + wait), "the pause doubles");
        assert!(t.poll(now + wait / 2, &disk.stat()).is_empty());
        now += wait;
        // the same bytes are decoded again (a writer let go of its lock)
        assert_eq!(t.poll(now, &disk.stat()), vec![Due::Reload(p.into(), s)]);
    }
    assert!(t.failed(p, s, now), "given up after {TRIES}");
    assert_eq!(t.next_due(), None);
    // only a new change brings it back
    t.touched(p, now);
    assert!(t.poll(now + QUIET, &disk.stat()).is_empty());
    assert_eq!(t.next_due(), None);
}

#[test]
fn a_change_during_the_retries_waits_for_the_new_version() {
    let disk = Disk::new(&[("/a/pad.wav", 100)]);
    let mut t = tracker(&disk, &["/a/pad.wav"]);
    let p = Path::new("/a/pad.wav");
    let now = Instant::now();
    t.failed(p, st(100).unwrap(), now);
    disk.set("/a/pad.wav", Some(300));
    assert!(t.poll(now + RETRY, &disk.stat()).is_empty(), "it grew: wait for it to hold still");
    assert_eq!(t.poll(now + RETRY + SETTLE, &disk.stat()), vec![Due::Reload(p.into(), st(300).unwrap())]);
}

#[test]
fn files_kept_across_project_changes_keep_their_state_and_dirs_follow() {
    let disk = Disk::new(&[("/a/pad.wav", 100), ("/b/kick.wav", 5)]);
    let mut t = tracker(&disk, &["/a/pad.wav"]);
    let now = Instant::now();
    disk.set("/a/pad.wav", Some(7));
    t.touched(Path::new("/a/pad.wav"), now);
    t.set_files(&["/a/pad.wav".into(), "/b/kick.wav".into()], &disk.stat());
    assert_eq!(t.len(), 2);
    assert_eq!(t.next_due(), Some(now + QUIET), "the pending look survives");
    assert_eq!(t.dirs(), [PathBuf::from("/a"), PathBuf::from("/b")].into_iter().collect());
    t.set_files(&["/b/kick.wav".into()], &disk.stat());
    assert_eq!(t.next_due(), None);
    assert_eq!(t.dirs().len(), 1);
}

// ---- the real thing: a project, a file on disk and the OS watch ----

fn write(path: &Path, seconds: f64, level: f32) {
    let n = (seconds * SR as f64) as usize;
    let data: Vec<f32> = (0..n).flat_map(|_| [level, level]).collect();
    wave_engine::sources::write_wav(path, &data, SR).unwrap();
}

fn level(e: &Engine) -> f32 {
    let spb = 60.0 / e.project().bpm;
    let x = e.render(0.0, 0.5 / spb, 0.0, SR).unwrap();
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn wait_for(rx: &mpsc::Receiver<MediaEvent>, kind: &str) -> MediaEvent {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let left = until.saturating_duration_since(Instant::now());
        let ev = rx.recv_timeout(left).unwrap_or_else(|_| panic!("no `{kind}` event in 20 s"));
        if ev.kind == kind {
            return ev;
        }
    }
}

#[test]
fn a_rebounced_file_plays_in_the_open_project_and_a_deleted_one_keeps_playing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pad.wav");
    write(&file, 1.0, 0.3);
    let mut p = Project::empty();
    p.inserts.iter_mut().for_each(|m| m.fx.clear());
    let e = Engine::new(p, Arc::new(NoPlugins));
    let (srcs, _) = e.import_audio(std::slice::from_ref(&file), Some((0, 0.0))).unwrap();
    let (tx, rx) = mpsc::channel();
    let w = MediaWatch::start(e.clone(), move |ev| {
        let _ = tx.send(ev);
    });
    w.set_files(e.source_files());
    // the watch is set up on its thread: give it a moment before the "other app" writes
    std::thread::sleep(Duration::from_millis(300));

    // the other app writes in place (truncate, then the data)
    write(&file, 1.0, 0.1);
    let ev = wait_for(&rx, "reloaded");
    assert_eq!(ev.source_ids, srcs);
    assert_eq!(ev.name, "pad");
    assert!(!ev.length_differs);
    assert!((level(&e) - 0.1).abs() < 0.02, "{}", level(&e));

    // an atomic save: a new file renamed over it, of another length
    let tmp = dir.path().join("pad.tmp");
    write(&tmp, 2.0, 0.2);
    std::fs::rename(&tmp, &file).unwrap();
    let ev = wait_for(&rx, "reloaded");
    assert!(ev.length_differs);
    assert!((level(&e) - 0.2).abs() < 0.02, "{}", level(&e));

    std::fs::remove_file(&file).unwrap();
    let ev = wait_for(&rx, "gone");
    assert_eq!(ev.source_ids, srcs);
    assert!((level(&e) - 0.2).abs() < 0.02, "the audio in memory keeps playing");
    assert!(e.state().missing.is_empty());
    drop(w);
}
