//! Hot reload of audio files that changed on disk (re-bounced or edited in another app): the
//! sources that play the file play its new audio, without an undo step, and a file that does
//! not decode (half written, not audio) changes nothing.

use std::path::Path;
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

fn level(e: &Engine) -> f32 {
    let spb = 60.0 / e.project().bpm;
    rms(&e.render(0.0, 0.5 / spb, 0.0, SR).unwrap())
}

fn source(e: &Engine, id: Id) -> Source {
    e.project().sources.into_iter().find(|s| s.id == id).unwrap()
}

/// A saved project playing `pad.wav` (1 s at 0.3) from beat 0. (engine, folder, file, source)
fn project(tag: &str) -> (Arc<Engine>, tempfile::TempDir, std::path::PathBuf, Id) {
    let root = tempfile::Builder::new().prefix(&format!("wave-reload-{tag}")).tempdir().unwrap();
    let file = root.path().join("Bounces/pad.wav");
    write(&file, 1.0, 0.3);
    let e = Engine::new(dry(Project::empty()), Arc::new(NoPlugins));
    let (srcs, _) = e.import_audio(std::slice::from_ref(&file), Some((0, 0.0))).unwrap();
    e.save(&root.path().join("song.wave")).unwrap();
    (e, root, file, srcs[0])
}

#[test]
fn a_changed_file_plays_its_new_audio_without_an_undo_step() {
    let (e, _root, file, src) = project("swap");
    assert!((level(&e) - 0.3).abs() < 0.02);
    let before = e.state();
    assert!(!before.dirty);
    let gen = before.audio_gen[&src];
    let peaks = e.source_peaks(src, false, false).unwrap();

    write(&file, 1.0, 0.1);
    let r = e.reload_source_file(&file).unwrap().expect("the project plays the file");
    assert_eq!(r.source_ids, vec![src]);
    assert_eq!(r.name, "pad");
    assert!(!r.length_differs && !r.was_missing);

    assert!((level(&e) - 0.1).abs() < 0.02, "{}", level(&e));
    let after = e.state();
    assert!(after.revision > before.revision, "listeners hear about it");
    assert!(!after.dirty, "a saved project stays saved: the file changed, not the project");
    assert_eq!(after.can_undo, before.can_undo, "no undo step");
    assert_ne!(after.audio_gen[&src], gen, "waveforms of the old audio are stale");
    let new_peaks = e.source_peaks(src, false, false).unwrap();
    assert!(new_peaks.iter().copied().fold(0f32, f32::max) < peaks.iter().copied().fold(0f32, f32::max));
}

#[test]
fn a_different_length_is_reported_and_clips_keep_theirs() {
    let (e, _root, file, src) = project("length");
    let clips = e.project().clips;
    // an edit before the reload: undo must not bring back the old length
    e.apply(&[Op::SetBpm { bpm: 100.0 }], false, None).unwrap();
    write(&file, 2.5, 0.2);
    let r = e.reload_source_file(&file).unwrap().unwrap();
    assert!(r.length_differs);
    assert!((r.seconds - 2.5).abs() < 1e-3);
    assert!((source(&e, src).dur - 2.5).abs() < 1e-3);
    assert_eq!(e.project().clips, clips);
    assert!(e.state().dirty, "unsaved edits stay unsaved");
    assert!(e.undo());
    assert!((source(&e, src).dur - 2.5).abs() < 1e-3, "the undo history knows the file's length too");
    assert!((level(&e) - 0.2).abs() < 0.02, "undo plays the file as it is now");
}

#[test]
fn a_file_that_does_not_decode_changes_nothing() {
    let (e, _root, file, src) = project("bad");
    let rev = e.revision();
    // half written, or not audio any more
    std::fs::write(&file, b"RIFF\x24\0\0\0WAVEfmt ").unwrap();
    assert!(e.reload_source_file(&file).is_err());
    assert_eq!(e.revision(), rev);
    assert!(e.state().missing.is_empty(), "the source keeps the audio it had");
    assert!((level(&e) - 0.3).abs() < 0.02);
    assert!((source(&e, src).dur - 1.0).abs() < 1e-3);

    std::fs::remove_file(&file).unwrap();
    assert!(e.reload_source_file(&file).is_err(), "a deleted file is an error too");
    assert!((level(&e) - 0.3).abs() < 0.02, "and the audio in memory keeps playing");
}

#[test]
fn a_file_no_source_plays_is_not_decoded() {
    let (e, root, _file, _src) = project("other");
    let other = root.path().join("other.wav");
    std::fs::write(&other, b"not even audio").unwrap();
    let rev = e.revision();
    assert_eq!(e.reload_source_file(&other).unwrap(), None);
    assert_eq!(e.revision(), rev);
}

#[test]
fn a_missing_file_that_comes_back_plays() {
    let (e, root, file, src) = project("back");
    let moved = root.path().join("pad-elsewhere.wav");
    std::fs::rename(&file, &moved).unwrap();
    e.open(&root.path().join("song.wave")).unwrap();
    assert!(e.state().missing.contains_key(&src));
    assert!(!e.state().audio_gen.contains_key(&src));
    std::fs::rename(&moved, &file).unwrap();
    let r = e.reload_source_file(&file).unwrap().unwrap();
    assert!(r.was_missing);
    assert!(e.state().missing.is_empty());
    assert!((level(&e) - 0.3).abs() < 0.02);
}

#[test]
fn every_source_of_the_file_reloads() {
    let (e, _root, file, src) = project("unique");
    let clip = e.project().clips.iter().find(|c| c.source == src).unwrap().id;
    let made = e.make_unique_audio(&[clip]).unwrap();
    assert_ne!(made.source_id, src);
    assert_eq!(e.source_files().len(), 1, "one file, watched once");
    write(&file, 1.0, 0.05);
    let r = e.reload_source_file(&file).unwrap().unwrap();
    let mut ids = r.source_ids.clone();
    ids.sort();
    assert_eq!(ids, vec![src, made.source_id]);
    assert!((level(&e) - 0.05).abs() < 0.02, "{}", level(&e));
}

#[test]
fn a_stretched_clip_plays_the_new_audio_stretched() {
    let (e, _root, file, src) = project("stretch");
    let mut c = e.project().clips.into_iter().find(|c| c.source == src).unwrap();
    c.stretch = 1.5;
    c.len *= 1.5;
    e.apply(&[Op::PutClips { clips: vec![c] }], false, None).unwrap();
    // the stretcher does not keep a constant level exactly: compare before and after
    let before = level(&e);
    assert!(before > 0.1, "{before}");
    write(&file, 1.0, 0.1);
    e.reload_source_file(&file).unwrap().unwrap();
    let ratio = level(&e) / before;
    assert!((ratio - 1.0 / 3.0).abs() < 0.03, "the render of the old audio is not played: {ratio}");
}

#[test]
fn library_samples_are_not_files_to_watch() {
    let e = Engine::new(wave_engine::demo::sketch_project(), Arc::new(NoPlugins));
    assert!(e.project().sources.iter().any(|s| s.path.starts_with("lib:")));
    assert!(e.source_files().iter().all(|f| !f.to_string_lossy().starts_with("lib:")));
}
