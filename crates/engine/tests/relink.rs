//! Relinking missing audio: a project whose files moved opens with its sources reported
//! missing (silent), and pointing them at the files again — one by one or by searching a
//! folder — makes them play, as one undo step that undo and redo follow exactly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wave_engine::*;

const SR: u32 = 48000;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn write(path: &Path, seconds: f64, level: f32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let n = (seconds * SR as f64) as usize;
    let data: Vec<f32> = (0..n).flat_map(|_| [level, level]).collect();
    wave_engine::sources::write_wav(path, &data, SR).unwrap();
}

fn dry(mut p: Project) -> Project {
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    p
}

/// A saved project playing `kick.wav` (1 s, level 0.3) at beat 0, whose folder of samples was
/// then moved to `<root>/moved/Drums`. Returns (engine with it open, root, kick source id).
fn moved_project(tag: &str) -> (Arc<Engine>, tempfile::TempDir, Id) {
    let root = tempfile::Builder::new().prefix(&format!("wave-relink-{tag}")).tempdir().unwrap();
    let old = root.path().join("samples/Drums/kick.wav");
    write(&old, 1.0, 0.3);
    let e = Engine::new(dry(Project::empty()), Arc::new(NoPlugins));
    let (srcs, _) = e.import_audio(std::slice::from_ref(&old), Some((0, 0.0))).unwrap();
    let proj = root.path().join("songs/song.wave");
    std::fs::create_dir_all(proj.parent().unwrap()).unwrap();
    e.save(&proj).unwrap();
    std::fs::create_dir_all(root.path().join("moved")).unwrap();
    std::fs::rename(root.path().join("samples/Drums"), root.path().join("moved/Drums")).unwrap();
    let e = Engine::new(dry(Project::empty()), Arc::new(NoPlugins));
    let warnings = e.open(&proj).unwrap();
    assert_eq!(warnings.len(), 1);
    (e, root, srcs[0])
}

fn level(e: &Engine) -> f32 {
    let spb = 60.0 / e.project().bpm;
    let out = e.render(0.0, 0.5 / spb, 0.0, SR).unwrap();
    rms(&out)
}

#[test]
fn a_moved_file_opens_missing_and_silent_with_the_reason() {
    let (e, _root, src) = moved_project("open");
    let st = e.state();
    assert_eq!(st.missing.len(), 1);
    assert!(st.missing[&src].contains("kick.wav"), "{}", st.missing[&src]);
    assert!(level(&e) < 1e-6);
}

#[test]
fn relinking_plays_the_file_and_undo_redo_follow_it() {
    let (e, root, src) = moved_project("one");
    let new = root.path().join("moved/Drums/kick.wav");
    let r = e.relink_sources(&[Relink { source_id: src, path: new.display().to_string() }]).unwrap();
    assert_eq!(r.len(), 1);
    assert!(r[0].error.is_none() && !r[0].length_differs);
    let st = e.state();
    assert!(st.missing.is_empty());
    assert!(st.dirty && st.can_undo);
    let s = st.project.sources.iter().find(|s| s.id == src).unwrap();
    assert_eq!(PathBuf::from(&s.path), new);
    assert!((level(&e) - 0.3).abs() < 0.02, "{}", level(&e));

    assert!(e.undo());
    assert_eq!(e.state().missing.len(), 1, "undo brings the old, missing path back");
    assert!(level(&e) < 1e-6);
    assert!(e.redo());
    assert!(e.state().missing.is_empty());
    assert!((level(&e) - 0.3).abs() < 0.02);
}

#[test]
fn a_relinked_project_saves_the_new_path() {
    let (e, root, src) = moved_project("save");
    let new = root.path().join("moved/Drums/kick.wav");
    e.relink_sources(&[Relink { source_id: src, path: new.display().to_string() }]).unwrap();
    let proj = root.path().join("songs/song.wave");
    e.save(&proj).unwrap();
    let again = Engine::new(Project::empty(), Arc::new(NoPlugins));
    assert!(again.open(&proj).unwrap().is_empty());
    assert!(again.state().missing.is_empty());
}

#[test]
fn replacing_a_loaded_file_and_undoing_brings_the_old_audio_back() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a.wav");
    let b = root.path().join("b.wav");
    write(&a, 1.0, 0.3);
    write(&b, 2.0, 0.1);
    let e = Engine::new(dry(Project::empty()), Arc::new(NoPlugins));
    let (srcs, _) = e.import_audio(&[a], Some((0, 0.0))).unwrap();
    let r = e.relink_sources(&[Relink { source_id: srcs[0], path: b.display().to_string() }]).unwrap();
    assert!(r[0].length_differs, "a 2 s file for a 1 s source is reported");
    assert!((level(&e) - 0.1).abs() < 0.02);
    let s = e.project().sources.into_iter().find(|s| s.id == srcs[0]).unwrap();
    assert!((s.dur - 2.0).abs() < 1e-3);
    e.undo();
    assert!((level(&e) - 0.3).abs() < 0.02, "{}", level(&e));
    assert!((e.project().sources.into_iter().find(|s| s.id == srcs[0]).unwrap().dur - 1.0).abs() < 1e-3);
    e.redo();
    assert!((level(&e) - 0.1).abs() < 0.02);
}

#[test]
fn a_file_that_does_not_decode_changes_nothing() {
    let (e, root, src) = moved_project("bad");
    let junk = root.path().join("junk.wav");
    std::fs::write(&junk, b"not audio").unwrap();
    let rev = e.revision();
    assert!(e.relink_sources(&[Relink { source_id: src, path: junk.display().to_string() }]).is_err());
    assert!(e.relink_sources(&[Relink { source_id: src, path: "relative.wav".into() }]).is_err());
    assert!(e.relink_sources(&[Relink { source_id: 999_999, path: junk.display().to_string() }]).is_err());
    assert_eq!(e.revision(), rev);
    assert_eq!(e.state().missing.len(), 1);
    assert!(!e.state().can_undo);
}

#[test]
fn searching_a_folder_relinks_the_unambiguous_file_in_one_undo_step() {
    let (e, root, src) = moved_project("search");
    let r = e.relink_search(root.path()).unwrap();
    assert_eq!(r.relinked.len(), 1, "{r:?}");
    assert!(r.relinked[0].path.ends_with("kick.wav"));
    assert!(r.candidates.is_empty() && r.not_found.is_empty() && !r.truncated);
    assert!(e.state().missing.is_empty());
    assert!((level(&e) - 0.3).abs() < 0.02);
    e.undo();
    assert!(e.state().missing.contains_key(&src));
}

#[test]
fn ambiguous_or_different_files_are_left_for_the_user() {
    let (e, root, src) = moved_project("ambiguous");
    // two equally good copies
    write(&root.path().join("copy/kick.wav"), 1.0, 0.3);
    std::fs::create_dir_all(root.path().join("again")).unwrap();
    std::fs::copy(root.path().join("copy/kick.wav"), root.path().join("again/kick.wav")).unwrap();
    std::fs::remove_dir_all(root.path().join("moved")).unwrap();
    let r = e.relink_search(root.path()).unwrap();
    assert!(r.relinked.is_empty());
    assert_eq!(r.candidates[&src].len(), 2);
    assert!(!e.state().can_undo);

    // one file of that name, but a different take (longer): not taken without asking
    std::fs::remove_dir_all(root.path().join("again")).unwrap();
    write(&root.path().join("copy/kick.wav"), 3.0, 0.3);
    let r = e.relink_search(root.path()).unwrap();
    assert!(r.relinked.is_empty());
    assert_eq!(r.candidates[&src].len(), 1);

    std::fs::remove_dir_all(root.path().join("copy")).unwrap();
    let r = e.relink_search(root.path()).unwrap();
    assert_eq!(r.not_found, vec![src]);
    assert!(e.relink_search(&root.path().join("nope")).is_err());
}

#[test]
fn a_sampler_channel_of_a_relinked_source_plays_again() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old/snare.wav");
    write(&old, 0.5, 0.4);
    let e = Engine::new(dry(Project::empty()), Arc::new(NoPlugins));
    let (srcs, _) = e.import_audio(std::slice::from_ref(&old), None).unwrap();
    let proj = root.path().join("p.wave");
    e.save(&proj).unwrap();
    let new = root.path().join("new/snare.wav");
    std::fs::create_dir_all(new.parent().unwrap()).unwrap();
    std::fs::rename(&old, &new).unwrap();
    e.open(&proj).unwrap();
    assert!(e.state().missing.contains_key(&srcs[0]));
    e.relink_sources(&[Relink { source_id: srcs[0], path: new.display().to_string() }]).unwrap();
    assert!(e.state().missing.is_empty());
    assert!(e.source_peaks(srcs[0], false, false).is_some_and(|p| !p.is_empty()));
}
