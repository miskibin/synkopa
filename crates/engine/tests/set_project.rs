//! Replacing the whole project in one go (`wave watch`): one undo step, the transport keeps
//! going, loaded audio is kept, new files are decoded and a bad one goes missing.

use std::path::Path;
use std::sync::Arc;

use wave_engine::*;

const SR: u32 = 48000;

fn write(path: &Path, seconds: f64, level: f32) {
    let n = (seconds * SR as f64) as usize;
    let data: Vec<f32> = (0..n).flat_map(|_| [level, level]).collect();
    wave_engine::sources::write_wav(path, &data, SR).unwrap();
}

fn dry() -> Project {
    let mut p = Project::empty();
    p.inserts.iter_mut().for_each(|m| m.fx.clear());
    p
}

fn level(e: &Engine) -> f32 {
    let spb = 60.0 / e.project().bpm;
    let x = e.render(0.0, 0.5 / spb, 0.0, SR).unwrap();
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn a_new_project_is_one_undo_step_and_the_transport_keeps_playing() {
    let e = Engine::new(dry(), Arc::new(NoPlugins));
    e.play(0.0);
    let mut p = e.project();
    p.bpm = 99.0;
    let rev = e.revision();
    let r = e.set_project(p.clone(), Some(rev)).unwrap();
    assert!(r > rev);
    assert_eq!(e.project().bpm, 99.0);
    assert!(e.state().transport.playing, "not stopped");
    assert_eq!(e.set_project(p, None).unwrap(), r, "the same project changes nothing");
    assert!(e.undo());
    assert!(!e.state().can_undo, "one step");
    assert!(e.state().transport.playing);
}

#[test]
fn audio_is_kept_new_files_are_decoded_and_a_bad_one_goes_missing() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a.wav");
    write(&a, 1.0, 0.3);
    let e = Engine::new(dry(), Arc::new(NoPlugins));
    let (srcs, _) = e.import_audio(std::slice::from_ref(&a), Some((0, 0.0))).unwrap();

    // a copy of the project, made elsewhere, with one more source of a file not loaded here
    let b = d.path().join("b.wav");
    write(&b, 1.0, 0.2);
    let other = Engine::new(e.project(), Arc::new(NoPlugins));
    let (more, _) = other.import_audio(std::slice::from_ref(&b), Some((1, 0.0))).unwrap();
    // the file of `a` is gone meanwhile: what is loaded for it is kept, not decoded again
    std::fs::remove_file(&a).unwrap();
    e.set_project(other.project(), None).unwrap();
    let st = e.state();
    assert!(st.missing.is_empty(), "{:?}", st.missing);
    assert!(e.source_peaks(srcs[0], false, false).is_some());
    assert!(e.source_peaks(more[0], false, false).is_some());
    assert!(level(&e) > 0.3, "both play: {}", level(&e));

    let mut bad = e.project();
    let c = d.path().join("c.wav");
    std::fs::write(&c, b"not audio").unwrap();
    let mut s = bad.sources[0].clone();
    s.id = bad.next_id;
    s.path = c.display().to_string();
    bad.next_id += 1;
    bad.sources.push(s.clone());
    e.set_project(bad, None).unwrap();
    assert!(e.state().missing.contains_key(&s.id));
}

#[test]
fn refused_without_a_master_or_on_a_stale_revision() {
    let e = Engine::new(dry(), Arc::new(NoPlugins));
    let mut p = e.project();
    p.inserts.clear();
    assert!(e.set_project(p, None).is_err());
    let mut p = e.project();
    p.bpm = 77.0;
    let err = e.set_project(p, Some(e.revision() + 5)).unwrap_err().to_string();
    assert!(err.starts_with("REVISION_CONFLICT"), "{err}");
    assert_ne!(e.project().bpm, 77.0);
}
