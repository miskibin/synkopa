//! `wave watch` end to end: a script file drives a project held by a session server (as the
//! running app serves the CLI) or a project file, through the real `wave` command line.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use wave_engine::Project;

use crate::actions::Host;
use crate::headless::{Files, Headless};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-watch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn wave(args: &[&str]) -> Result<Value, String> {
    wave_cli::execute(args.iter().map(|s| s.to_string()).collect(), Some(&Files))
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

/// A session server answering like the app (`lib.rs`), over a project held in this process.
fn serve(project: Project) -> (Arc<Headless>, wave_session::Server) {
    let host = Arc::new(Headless::new(project));
    let h = host.clone();
    let handler: Arc<wave_session::Handler> = Arc::new(move |req: &wave_session::Request| {
        let res = crate::actions::control(h.as_ref(), &req.command, &req.args, req.if_revision);
        let rev = h.engine().revision();
        res.map(|d| (d, rev)).map_err(|m| {
            let code = m
                .split(':')
                .next()
                .filter(|c| c.len() > 2 && c.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_'))
                .unwrap_or("REFUSED");
            wave_session::ErrorBody::new(code, &m)
        })
    });
    let server = wave_session::Server::start(handler).expect("session server");
    (host, server)
}

fn bpm(h: &Headless) -> f64 {
    h.engine().project().bpm
}

#[test]
fn once_on_a_file_runs_the_script_on_a_copy_and_renders_it() {
    let d = tmp("file");
    let song = d.join("song.wave");
    wave(&["new", &s(&song), "--template", "sketch"]).unwrap();
    let before = std::fs::read_to_string(&song).unwrap();
    let script = d.join("song.txt");
    std::fs::write(&script, "# the beat\nbpm 141\npattern --id 1 --ch kick --steps \"x... x... x... x...\"\n").unwrap();
    let out = d.join("preview.wav");
    let ev = wave(&["watch", &s(&script), "--project", &s(&song), "--once", "--render", &s(&out)]).unwrap();
    assert_eq!(ev["event"], "applied");
    assert_eq!(ev["lines"], 2);
    assert_eq!(ev["render"]["stats"]["silent"], false, "{ev}");
    assert!(out.is_file());
    assert_eq!(std::fs::read_to_string(&song).unwrap(), before, "the project file is never saved");

    std::fs::write(&script, "bpm 141\nadd-fx --insert 999 --type eq\n").unwrap();
    let e = wave(&["watch", &s(&script), "--project", &s(&song), "--once"]).unwrap_err();
    assert!(e.contains("line 2") && e.contains("nothing was applied"), "{e}");
    assert!(wave(&["watch", &s(&d.join("nope.txt")), "--project", &s(&song), "--once"]).is_err());
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn once_against_the_app_replaces_the_song_in_one_undo_step() {
    let d = tmp("once");
    let (h, server) = serve(Project::sketch());
    let id = server.endpoint.session_id.clone();
    let base_bpm = bpm(&h);
    let script = d.join("song.txt");
    std::fs::write(&script, "bpm 150\nadd-fx --insert 1 --type reverb\n").unwrap();
    let ev = wave(&["watch", &s(&script), "--session", &id, "--once"]).unwrap();
    assert_eq!(ev["changed"], true, "{ev}");
    assert_eq!(bpm(&h), 150.0);
    let fx = |h: &Headless| h.engine().project().inserts.iter().find(|m| m.id == 1).map_or(0, |m| m.fx.len());
    let with_reverb = fx(&h);
    assert!(h.engine().undo(), "one undo step");
    assert_eq!(bpm(&h), base_bpm);
    assert_eq!(fx(&h), with_reverb - 1, "the whole run is undone at once");
    assert!(!h.engine().state().can_undo, "and it was only one step");
    std::fs::remove_dir_all(&d).ok();
}

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn every_save_replaces_the_song_and_an_edit_in_the_app_is_not_overwritten() {
    let d = tmp("live");
    let (h, server) = serve(Project::sketch());
    let id = server.endpoint.session_id.clone();
    let base = bpm(&h);
    let script = d.join("song.txt");
    std::fs::write(&script, "bpm 133\n").unwrap();
    let file = s(&script);
    // runs until the test process ends (as `wave watch` runs until Ctrl+C)
    std::thread::spawn(move || {
        let _ = wave(&["watch", &file, "--session", &id]);
    });
    until("the first run", || bpm(&h) == 133.0);

    // a save that drops the line: the song is the file, so the tempo goes back
    std::thread::sleep(Duration::from_millis(50));
    std::fs::write(&script, "# no tempo change\n").unwrap();
    until("the line to be gone", || bpm(&h) == base);

    // a save with a broken line changes nothing
    std::fs::write(&script, "bpm 120\nset-param --fx 424242 --param mix --value 1\n").unwrap();
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(bpm(&h), base);

    // the musician moves something in the app: the next save does not replace it
    h.engine().apply(&[wave_engine::Op::SetBpm { bpm: 97.0 }], false, None).unwrap();
    std::fs::write(&script, "bpm 128\n").unwrap();
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(bpm(&h), 97.0, "REVISION_CONFLICT: the app's edit is kept");
    std::fs::remove_dir_all(&d).ok();
}
