//! Warp markers: pinning a source moment to a stretched-audio position, through the
//! render, playback, ops, undo and files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wave_engine::*;

const SR: u32 = 48000;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-warp-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A project with no effects anywhere, at 120 BPM (a beat is 0.5 s).
fn dry_project() -> Project {
    let mut p = Project::empty();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    p.bpm = 120.0;
    p
}

/// A sine at `f1` for `t1` seconds, then `f2` until `secs`.
fn write_tones(path: &Path, f1: f64, t1: f64, f2: f64, secs: f64) {
    let mut ph = 0.0f64;
    let data: Vec<f32> = (0..(secs * SR as f64) as usize)
        .flat_map(|i| {
            let f = if (i as f64) < t1 * SR as f64 { f1 } else { f2 };
            ph += 2.0 * std::f64::consts::PI * f / SR as f64;
            let v = (0.5 * ph.sin()) as f32;
            [v, v]
        })
        .collect();
    wave_engine::sources::write_wav(path, &data, SR).unwrap();
}

/// An engine playing one audio clip of a two-tone file (440 Hz for 1 s, then 880 Hz).
fn engine_with_clip(tag: &str) -> (Arc<Engine>, Clip) {
    let dir = tmp(tag);
    let path = dir.join("tones.wav");
    write_tones(&path, 440.0, 1.0, 880.0, 2.0);
    let e = Engine::new(dry_project(), Arc::new(NoPlugins));
    let (_, clips, _) = e.import_audio(&[path], Some((0, 0.0))).unwrap();
    let clip = e.project().clips.into_iter().find(|c| c.id == clips[0]).unwrap();
    (e, clip)
}

fn left(x: &[f32]) -> Vec<f32> {
    x.chunks(2).map(|f| f[0]).collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Frequency from upward zero crossings of `x[a..b]`.
fn freq(x: &[f32], a: usize, b: usize) -> f64 {
    let mut cross = vec![];
    for i in a.max(1)..b {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            cross.push((i - 1) as f64 + (-x[i - 1] / (x[i] - x[i - 1])) as f64);
        }
    }
    assert!(cross.len() > 4, "no tone in {a}..{b}");
    (cross.len() - 1) as f64 / ((cross[cross.len() - 1] - cross[0]) / SR as f64)
}

fn sec(t: f64) -> usize {
    (t * SR as f64) as usize
}

fn put(e: &Engine, c: Clip) -> Result<u64, EngineError> {
    e.apply(&[Op::PutClips { clips: vec![c] }], false, None)
}

fn marker(at: f64, to: f64) -> WarpPoint {
    WarpPoint { at, to }
}

/// An exported `.wav` is int16; keep the analysis tone loud enough for it.
#[test]
fn a_marker_retimes_the_material_around_it() {
    let (e, clip) = engine_with_clip("retime");
    // the tone change at 1.0 s plays at 1.4 s instead: the 440 half stretches to cover
    // 0..1.4 s, the 880 half squeezes into 1.4..2 s, and the clip stays 2 s long
    let c = Clip { warp: vec![marker(1.0, 1.4)], ..clip.clone() };
    put(&e, c.clone()).unwrap();
    let out = left(&e.render(0.0, 8.0, 0.0, SR).unwrap()); // 8 beats = 4 s
    let f = |a: f64, b: f64| freq(&out, sec(a), sec(b));
    assert!((f(0.2, 1.2) - 440.0).abs() < 5.0, "the 440 half before the marker: {}", f(0.2, 1.2));
    assert!((f(1.5, 1.9) - 880.0).abs() < 9.0, "the 880 half after it: {}", f(1.5, 1.9));
    // the boundary moved: 440 until ~1.35, 880 from ~1.45 (it was at 1.0)
    assert!((f(1.15, 1.33) - 440.0).abs() < 6.0, "still 440 at 1.15..1.33: {}", f(1.15, 1.33));
    assert!((f(1.47, 1.62) - 880.0).abs() < 12.0, "already 880 at 1.47..1.62: {}", f(1.47, 1.62));
    assert!(rms(&out[sec(2.05)..sec(2.9)]) < 1e-3, "the clip is 2 s still");
    // and the map round-trips through the model
    assert_eq!(c.warped_to(1.0, 2.0), 1.4);
    assert!((c.warped_from(1.4, 2.0) - 1.0).abs() < 1e-9);
    assert!((c.warped_to(0.0, 2.0) - 0.0).abs() < 1e-9);
    assert!((c.warped_to(2.0, 2.0) - 2.0).abs() < 1e-9, "the ends stay put");
}

#[test]
fn markerless_and_empty_maps_map_at_the_baseline() {
    let clip = Clip {
        id: 1,
        track: 0,
        start: 0.0,
        len: 4.0,
        offset: 0.0,
        kind: ClipKind::Audio,
        source: 1,
        muted: false,
        gain: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        pan: 0.0,
        norm: false,
        rev: false,
        tune: None,
        stretch: 2.0,
        resample: false,
        pitch: 0.0,
        insert: None,
        root: CLIP_ROOT,
        warp: vec![],
    };
    assert!((clip.warped_to(1.5, 4.0) - 3.0).abs() < 1e-12, "baseline: s·stretch");
    assert!((clip.warped_from(3.0, 4.0) - 1.5).abs() < 1e-12);
    assert!(!clip.warp_active());
    assert!(clip.needs_stretch(), "stretch ≠ 1 renders (the plain path)");
    let plain = Clip { stretch: 1.0, ..clip.clone() };
    assert!(!plain.needs_stretch(), "a markerless clip at plain speed needs no render");
}

#[test]
fn putting_markers_is_checked() {
    let (e, clip) = engine_with_clip("checked");
    // unsorted input is stored sorted
    put(&e, Clip { warp: vec![marker(1.5, 1.6), marker(0.5, 0.5)], ..clip.clone() }).unwrap();
    let got = e.project().clips.into_iter().find(|c| c.id == clip.id).unwrap();
    let ats: Vec<f64> = got.warp.iter().map(|w| w.at).collect();
    assert_eq!(ats, vec![0.5, 1.5], "sorted by `at`");

    let pat = Clip { id: 78_001, kind: ClipKind::Pat, source: 1, warp: vec![marker(1.0, 1.0)], ..clip.clone() };
    assert!(put(&e, pat).is_err(), "only audio clips carry markers");
    let rs = Clip { resample: true, warp: vec![marker(1.0, 1.0)], ..clip.clone() };
    assert!(put(&e, rs).is_err(), "resampling maps linearly, markers cannot apply");
    let past = Clip { warp: vec![marker(2.5, 2.5)], ..clip.clone() };
    assert!(put(&e, past).is_err(), "past the source's 2 s");
    let steep = Clip { warp: vec![marker(0.1, 1.0)], ..clip.clone() };
    assert!(put(&e, steep).is_err(), "the head would stretch by 10");
    let tail = Clip { warp: vec![marker(1.9, 0.2)], ..clip.clone() };
    assert!(put(&e, tail).is_err(), "the tail would stretch by 18");
    let close = Clip { warp: vec![marker(1.0, 1.0), marker(1.002, 1.002)], ..clip.clone() };
    assert!(put(&e, close).is_err(), "two markers 2 ms apart");
    assert!(put(&e, Clip { warp: vec![marker(f64::NAN, 1.0)], ..clip.clone() }).is_err());
    assert!(put(&e, Clip { warp: vec![marker(-0.5, 1.0)], ..clip.clone() }).is_err());
    // a rejected op leaves the project unchanged
    assert_eq!(e.project().clips.iter().find(|c| c.id == clip.id).unwrap().warp.len(), 2);
}

#[test]
fn a_1025_marker_map_is_refused() {
    let dir = tmp("count");
    let path = dir.join("long.wav");
    write_tones(&path, 220.0, 5.0, 330.0, 10.0);
    let e = Engine::new(dry_project(), Arc::new(NoPlugins));
    let (_, clips, _) = e.import_audio(&[path], Some((0, 0.0))).unwrap();
    let clip = e.project().clips.into_iter().find(|c| c.id == clips[0]).unwrap();
    let many: Vec<WarpPoint> = (0..1025).map(|i| marker(0.05 + i as f64 * 0.0097, 0.05 + i as f64 * 0.0097)).collect();
    assert!(put(&e, Clip { warp: many, ..clip }).is_err(), "too many warp markers");
}

#[test]
fn editing_a_marker_re_renders_but_the_tempo_does_not() {
    let (e, clip) = engine_with_clip("recache");
    put(&e, Clip { warp: vec![marker(1.0, 1.3)], ..clip.clone() }).unwrap();
    let t = std::time::Instant::now();
    while e.state().stretching.contains(&clip.id) {
        assert!(t.elapsed().as_secs() < 20, "the warp never finished");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(e.state().stretch_errors.is_empty());
    // a tempo change re-uses the same render (the map is in seconds, not beats)
    e.apply(&[Op::SetBpm { bpm: 90.0 }], false, None).unwrap();
    assert!(!e.state().stretching.contains(&clip.id), "no re-render after a tempo change");
    // moving the marker re-renders (a short file may finish before we look, so wait and
    // then prove the new map by ear: the tone boundary moves from 1.3 s to 1.6 s)
    put(&e, Clip { warp: vec![marker(1.0, 1.6)], ..clip.clone() }).unwrap();
    let t = std::time::Instant::now();
    while e.state().stretching.contains(&clip.id) {
        assert!(t.elapsed().as_secs() < 20, "the warp never finished");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let out = left(&e.render(0.0, 8.0, 0.0, SR).unwrap());
    let f = |a: f64, b: f64| freq(&out, sec(a), sec(b));
    assert!((f(1.3, 1.5) - 440.0).abs() < 6.0, "still 440 at 1.3..1.5: {}", f(1.3, 1.5));
    assert!((f(1.65, 1.8) - 880.0).abs() < 12.0, "already 880 at 1.65..1.8: {}", f(1.65, 1.8));
}

#[test]
fn undo_restores_the_markerless_clip() {
    let (e, clip) = engine_with_clip("undo");
    put(&e, Clip { warp: vec![marker(1.0, 1.4)], ..clip.clone() }).unwrap();
    e.undo();
    let got = e.project().clips.into_iter().find(|c| c.id == clip.id).unwrap();
    assert_eq!(got, clip, "markers gone, everything else identical");
}

#[test]
fn markers_are_saved_and_a_file_without_them_loads() {
    let (e, clip) = engine_with_clip("serde");
    let plain = serde_json::to_value(&clip).unwrap();
    assert!(plain.get("warp").is_none(), "markerless clips are written as before");
    let back: Clip = serde_json::from_value(plain).unwrap();
    assert!(back.warp.is_empty());

    let c = Clip { warp: vec![marker(0.5, 0.75), marker(1.5, 1.25)], ..clip.clone() };
    put(&e, c.clone()).unwrap();
    let json = serde_json::to_value(e.project()).unwrap();
    assert_eq!(json["version"], 5);
    let saved = json["clips"].as_array().unwrap().iter().find(|c| c["id"] == clip.id).unwrap();
    let warp = saved["warp"].as_array().unwrap();
    assert_eq!(warp.len(), 2);
    assert_eq!((&warp[0]["at"], &warp[0]["to"]), (&serde_json::json!(0.5), &serde_json::json!(0.75)));
    let path = tmp("serde").join("p.wave");
    e.save(&path).unwrap();
    let f = Engine::new(Project::empty(), Arc::new(NoPlugins));
    f.open(&path).unwrap();
    let got = f.project().clips.into_iter().find(|x| x.id == c.id).unwrap();
    assert_eq!(got.warp, vec![marker(0.5, 0.75), marker(1.5, 1.25)]);
    assert_eq!(got.warped_to(1.5, 2.0), 1.25);
}

#[test]
fn the_map_takes_the_stretch_into_account() {
    // with a ×2 stretch the baseline end anchor is the source doubled
    let clip = Clip {
        id: 1,
        track: 0,
        start: 0.0,
        len: 8.0,
        offset: 0.0,
        kind: ClipKind::Audio,
        source: 1,
        muted: false,
        gain: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        pan: 0.0,
        norm: false,
        rev: false,
        tune: None,
        stretch: 2.0,
        resample: false,
        pitch: 0.0,
        insert: None,
        root: CLIP_ROOT,
        warp: vec![marker(1.0, 3.0)],
    };
    assert!(clip.warp_active());
    assert!(clip.needs_stretch());
    let anchors = clip.warp_anchors(96000, SR); // 2 s of source at 48 kHz
    let want = [(0.0, 0.0), (48000.0, 144000.0), (96000.0, 192000.0)];
    for ((a, b), (c, d)) in anchors.iter().zip(want) {
        assert!((a - c).abs() < 1e-9 && (b - d).abs() < 1e-9);
    }
    assert_eq!(anchors.len(), 3);
    // past the marker the map walks toward the doubled end
    assert!((clip.warped_to(2.0, 2.0) - 4.0).abs() < 1e-9);
    assert!((clip.warped_to(1.5, 2.0) - 3.5).abs() < 1e-9);
}
