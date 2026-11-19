//! One-shot previews and pattern loops (`engine/preview.rs`): a preview or a pattern loop
//! plays through the live output without ever editing the project, pattern and song are
//! exclusive (FL's PAT / SONG), and bad input is an error rather than a panic.

use std::sync::Arc;

use wave_engine::*;

const SR: f32 = 48000.0;
const BLOCK: usize = 256;

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |m, v| m.max(v.abs()))
}

/// 0.5 s of quiet 440 Hz tone as a wav file, imported onto a fresh engine: one audio clip.
fn engine_with_tone(name: &str) -> (Arc<Engine>, Id, std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("wave-preview-{name}-{}", std::process::id()));
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
    let (_, clips, _) = e.import_audio(std::slice::from_ref(&wav), Some((0, 4.0))).unwrap();
    (e, clips[0], wav, dir)
}

/// A pattern clip on track 1 over the sketch's "Beat" pattern.
fn pattern_clip(id: Id) -> Clip {
    Clip {
        id,
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
    }
}

#[test]
fn a_clip_preview_sounds_and_is_not_an_edit() {
    let (e, clip, _wav, dir) = engine_with_tone("clip");
    let rev = e.revision();
    let mut rt = e.create_rt(SR, BLOCK);
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    e.preview_clip(clip, None).unwrap();
    let mut heard = 0f32;
    for _ in 0..60 {
        rt.process(&mut l, &mut r, None);
        heard = heard.max(peak(&l));
    }
    assert!(heard > 1e-3, "the preview must sound ({heard})");
    assert_eq!(e.revision(), rev, "a preview is not an edit");
    e.preview_stop();
    let mut after = 0f32;
    for _ in 0..60 {
        rt.process(&mut l, &mut r, None);
        after = after.max(peak(&l));
    }
    assert!(after < 1e-6, "preview_stop must end the preview ({after})");
    assert!(e.preview_clip(999_999, None).is_err(), "an unknown clip is an error");
    assert!(e.preview_clip(clip, Some(99.0)).is_err(), "past the clip's region there is nothing to play");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_sample_keyboard_plays_audio_clips_only() {
    let (e, clip, _wav, dir) = engine_with_tone("key");
    e.apply(&[Op::PutClips { clips: vec![pattern_clip(9001)] }], false, None).unwrap();
    assert!(e.preview_clip_key(9001, 60, 1.0).is_err(), "a pattern clip is not a sample");
    let mut rt = e.create_rt(SR, BLOCK);
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    e.preview_clip_key(clip, 60, 1.0).unwrap();
    let mut heard = 0f32;
    for _ in 0..60 {
        rt.process(&mut l, &mut r, None);
        heard = heard.max(peak(&l));
    }
    assert!(heard > 1e-3, "the sample must sound at its root key ({heard})");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn preview_file_plays_a_file_and_reports_missing_ones() {
    let (e, _clip, wav, dir) = engine_with_tone("file");
    let rev = e.revision();
    let mut rt = e.create_rt(SR, BLOCK);
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    e.preview_file(&wav).unwrap();
    let mut heard = 0f32;
    for _ in 0..60 {
        rt.process(&mut l, &mut r, None);
        heard = heard.max(peak(&l));
    }
    assert!(heard > 1e-3, "the file preview must sound ({heard})");
    assert_eq!(e.revision(), rev, "a preview is not an edit");
    assert!(e.preview_file(&dir.join("missing.wav")).is_err(), "a missing file is an error");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pattern_play_stops_the_song_and_the_song_stops_the_pattern() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let mut rt = e.create_rt(SR, BLOCK);
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    e.play(0.0);
    assert!(e.state().transport.playing);
    e.pattern_play(Some(1), 0.0, None).unwrap();
    assert!(!e.state().transport.playing, "starting a pattern stops the song");
    let mut heard = 0f32;
    for _ in 0..80 {
        rt.process(&mut l, &mut r, None);
        heard = heard.max(peak(&l));
    }
    assert!(heard > 1e-3, "the pattern must sound on its own ({heard})");
    assert!(e.pattern_play(Some(999_999), 0.0, None).is_err(), "an unknown pattern is an error");
    assert!(e.pattern_play(Some(1), f64::NAN, None).is_err(), "a not-a-number position is an error");

    // the other direction: starting the song ends the pattern loop at once. The sketch's
    // own playlist clips keep sounding, so the published pattern playhead is the check.
    e.play(0.0);
    assert!(e.state().transport.playing, "the song is playing again");
    rt.process(&mut l, &mut r, None); // the Play command takes effect on the next block
    assert!(e.shared().pat_beat() < 0.0, "starting the song must stop the pattern loop");
    for _ in 0..80 {
        rt.process(&mut l, &mut r, None);
        assert!(e.shared().pat_beat() < 0.0, "the pattern loop came back under the song");
    }

    // and an explicit stop still ends a running loop
    e.pattern_play(Some(1), 0.0, None).unwrap();
    let mut heard = 0f32;
    for _ in 0..40 {
        rt.process(&mut l, &mut r, None);
        heard = heard.max(peak(&l));
    }
    assert!(heard > 1e-3, "the restarted pattern must sound ({heard})");
    e.pattern_play(None, 0.0, None).unwrap();
    rt.process(&mut l, &mut r, None); // the stop takes effect on the next block
    assert!(e.shared().pat_beat() < 0.0, "pattern_play(None) must stop the loop");
    // the loop no longer schedules: no new kicks start, the last ones just ring out
    let mut last = 0f32;
    for _ in 0..120 {
        rt.process(&mut l, &mut r, None);
        last = peak(&l);
        assert!(e.shared().pat_beat() < 0.0, "the pattern loop came back after the stop");
    }
    assert!(last < 1e-4, "nothing was still being scheduled ({last})");
}

#[test]
fn pattern_loop_range_validates_and_needs_a_loop() {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    // the device comes first: pattern commands reach the running audio through it
    let mut rt = e.create_rt(SR, BLOCK);
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let rev = e.revision();
    e.pattern_loop(Some((0.0, 2.0))).unwrap();
    assert_eq!(e.revision(), rev, "a range change with no loop running changes nothing");
    assert!(e.pattern_loop(Some((0.0, f64::NAN))).is_err(), "a not-a-number range is an error");
    e.pattern_play(Some(1), 0.0, None).unwrap();
    e.pattern_loop(Some((0.0, 2.0))).unwrap();
    let mut heard = 0f32;
    for _ in 0..40 {
        rt.process(&mut l, &mut r, None);
        heard = heard.max(peak(&l));
    }
    assert!(heard > 1e-3, "the loop keeps playing inside its range ({heard})");
    // a range that excludes the playhead moves the playhead into the range
    e.pattern_loop(Some((2.0, 4.0))).unwrap();
    for _ in 0..10 {
        rt.process(&mut l, &mut r, None);
    }
    let beat = e.shared().pat_beat();
    assert!((2.0..4.0).contains(&beat), "the playhead must follow the new range, got {beat}");
    e.pattern_play(None, 0.0, None).unwrap();
}
