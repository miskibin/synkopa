//! The review-round fixes: a cancel that arrives with the last progress frame still cancels,
//! an open drag never leaks into a replaced project, stem context shrinks to the reopen
//! limit, and a saved scratch take never replaces audio that is already there.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use wave_engine::*;

/// 0.5 s of 440 Hz tone as a wav file, imported onto a fresh engine: one audio clip.
fn engine_with_tone(name: &str) -> (Arc<Engine>, Id, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("wave-fixes-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let tone: Vec<f32> = (0..22050)
        .flat_map(|i| {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 0.4;
            [v, v]
        })
        .collect();
    wave_engine::sources::write_wav(&wav, &tone, 44100).unwrap();
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let (_, clips) = e.import_audio(std::slice::from_ref(&wav), Some((0, 4.0))).unwrap();
    (e, clips[0], dir)
}

#[test]
fn a_cancel_at_the_final_progress_frame_still_cancels() {
    let (e, _clip, dir) = engine_with_tone("cancel");
    let cancel = AtomicBool::new(false);
    // a plain tail runs the loop to the very last frame, where the final progress callback
    // fires: cancelling from it must not hand back the finished audio as a success
    let r = e.render_export_with(None, 0.0, 8.0, RenderTail::Seconds(0.0), 48_000, &cancel, &mut |p| {
        if let RenderProgress::Rendering { done, total, .. } = p {
            if done >= total {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    });
    assert!(matches!(r, Err(EngineError::Cancelled)), "a cancelled render must not succeed: {r:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_open_drag_never_leaks_into_a_replaced_project() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    e.begin_gesture();
    let drag = Clip {
        id: 9100,
        track: 1,
        start: 20.0,
        len: 4.0,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: 1,
        muted: false,
        gain: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        pan: 0.0,
        norm: false,
        rev: false,
        tune: None,
        stretch: 1.0,
        resample: false,
        pitch: 0.0,
        insert: None,
        root: 60,
    };
    e.apply(&[Op::PutClips { clips: vec![drag] }], true, None).unwrap();
    // replacing the project must forget the drag's snapshot (as undo/redo do)
    e.new_project(Project::sketch()).unwrap();
    e.end_gesture();
    assert!(!e.undo(), "the replaced project must not undo into the old one");
    assert!(!e.project().clips.iter().any(|c| c.id == 9100), "the dragged clip stayed behind");
}

#[test]
fn stem_context_shrinks_so_the_region_fits_the_reopen_limit() {
    let (e, clip, dir) = engine_with_tone("stems");
    // a context far past the limit must not grow the written region past it: the file is
    // what Audio::load decodes again when the project reopens (it stops at STEMS_MAX_S)
    let region = e.stem_region(clip, 100_000.0).unwrap();
    let secs = region.data.len() as f64 / 2.0 / f64::from(region.sr);
    assert!(secs <= STEMS_MAX_S, "the region is {secs:.0} s, past the {STEMS_MAX_S:.0} s reopen limit");
    assert!(secs > 0.3, "the clip's own span must survive the trim ({secs})");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_saved_scratch_take_never_replaces_audio_that_is_already_there() {
    let scratch = wave_engine::engine::scratch_dir();
    std::fs::create_dir_all(&scratch).unwrap();
    let take = scratch.join("fixes-take.wav");
    let tone: Vec<f32> = (0..22050).flat_map(|i| [(i as f32 * 0.01).sin(), -(i as f32 * 0.01).sin()]).collect();
    wave_engine::sources::write_wav(&take, &tone, 44100).unwrap();
    let dir_a = std::env::temp_dir().join(format!("wave-fixes-save-a-{}", std::process::id()));
    let dir_b = std::env::temp_dir().join(format!("wave-fixes-save-b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
    std::fs::create_dir_all(dir_b.join("Song Audio")).unwrap();
    // the audio project B already references: saving B must not overwrite it
    let occupied = dir_b.join("Song Audio").join("fixes-take.wav");
    let other: Vec<f32> = vec![0.9, -0.9];
    wave_engine::sources::write_wav(&occupied, &other, 44100).unwrap();

    let e_a = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    e_a.import_audio(std::slice::from_ref(&take), None).unwrap();
    e_a.save(&dir_a.join("Song.synkopa")).unwrap();

    e_a.new_project(Project::sketch()).unwrap();
    e_a.import_audio(std::slice::from_ref(&take), None).unwrap();
    e_a.save(&dir_b.join("Song.synkopa")).unwrap();

    // B's existing audio survived, the take landed beside it under a fresh name
    let kept = wave_engine::sources::Audio::load(&occupied).unwrap();
    assert_eq!(kept.frames, 1, "the existing file was overwritten");
    let audio = dir_b.join("Song Audio").join("fixes-take (2).wav");
    assert!(audio.is_file(), "the moved take went to {} instead", audio.display());
    let _ = std::fs::remove_dir_all(dir_a);
    let _ = std::fs::remove_dir_all(dir_b);
    let _ = std::fs::remove_file(&take);
}

#[test]
fn overlapping_copies_of_the_same_stem_each_publish_their_own_file() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let dir = std::env::temp_dir().join(format!("wave-fixes-copy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("Song.synkopa");
    let b = dir.join("Song.wave");
    let t1 = e.save_copy(&a);
    let t2 = e.save_copy(&b);
    assert!(t1.is_ok() && t2.is_ok(), "{t1:?} / {t2:?}");
    assert!(a.is_file() && b.is_file());
    let _ = std::fs::remove_dir_all(dir);
}
