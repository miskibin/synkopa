//! Analysis caching in `engine/analysis.rs`: pitch analysis of a source variant is computed
//! once and shared (same `Arc` back), a copied source ("Make unique") analyzes on its own,
//! only audio clips have pitch, and the curve is the real pitch of the tone.

use std::sync::Arc;

use wave_engine::*;

/// 1 s of 440 Hz tone (MIDI 69) as a wav file, imported onto a fresh engine: one audio clip.
fn engine_with_tone(name: &str) -> (Arc<Engine>, Id, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("wave-analysis-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let tone: Vec<f32> = (0..44100)
        .flat_map(|i| {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 0.4;
            [v, v]
        })
        .collect();
    wave_engine::sources::write_wav(&wav, &tone, 44100).unwrap();
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let (_, clips, _) = e.import_audio(std::slice::from_ref(&wav), Some((0, 4.0))).unwrap();
    (e, clips[0], dir)
}

#[test]
fn pitch_analysis_is_computed_once_and_shared() {
    let (e, clip, dir) = engine_with_tone("cache");
    let a = e.pitch_analysis(clip).unwrap();
    let b = e.pitch_analysis(clip).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "the second call must hit the cache, not re-analyze");
    assert!(!a.curve.is_empty() && a.rate > 0.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn make_unique_moves_the_clip_to_its_own_analysis() {
    let (e, clip, dir) = engine_with_tone("unique");
    // a second clip that plays the SAME source, placed by hand so the source is shared
    let src = e.project().clips.iter().find(|c| c.id == clip).unwrap().source;
    let twin = Clip { id: 9003, track: 1, start: 8.0, kind: ClipKind::Audio, source: src, ..pattern_like() };
    e.apply(&[Op::PutClips { clips: vec![twin] }], false, None).unwrap();
    let before = e.pitch_analysis(clip).unwrap();
    e.make_unique_audio(&[clip]).unwrap();
    let for_copy = e.pitch_analysis(clip).unwrap();
    assert!(!Arc::ptr_eq(&before, &for_copy), "the made-unique clip analyzes its own new source");
    let untouched = e.pitch_analysis(9003).unwrap();
    assert!(Arc::ptr_eq(&before, &untouched), "the source the copy came from keeps its analysis");
    let _ = std::fs::remove_dir_all(dir);
}

/// A default-shaped clip to build audio/pattern variants from in these tests.
fn pattern_like() -> Clip {
    Clip {
        id: 0,
        track: 0,
        start: 0.0,
        kind: ClipKind::Audio,
        source: 0,
        len: 1.07,
        offset: 0.0,
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
    }
}

#[test]
fn only_audio_clips_have_pitch() {
    let (e, _clip, dir) = engine_with_tone("kinds");
    assert!(e.pitch_analysis(999_999).is_err(), "an unknown clip is an error");
    let pat = Clip {
        id: 9002,
        track: 1,
        start: 0.0,
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
    e.apply(&[Op::PutClips { clips: vec![pat] }], false, None).unwrap();
    assert!(e.pitch_analysis(9002).is_err(), "a pattern clip has no pitch");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_curve_follows_the_tone() {
    let (e, clip, dir) = engine_with_tone("pitch");
    let a = e.pitch_analysis(clip).unwrap();
    let voiced: Vec<f32> = a.curve.iter().copied().filter(|p| p.is_finite()).collect();
    assert!(voiced.len() > a.curve.len() / 2, "a steady tone must be mostly voiced");
    let mid = voiced[voiced.len() / 2];
    assert!((mid - 69.0).abs() < 1.0, "440 Hz is MIDI 69, got {mid}");
    assert!(!a.notes.is_empty(), "the tone must segment into notes");
    let _ = std::fs::remove_dir_all(dir);
}
