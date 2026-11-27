//! Time signatures: the bar map's conversions (bar↔beat, off-grid boundaries, fragments,
//! backwards bars), the op's bounds, pattern lengths in whole bars of where they play,
//! the song end, and the metronome's accents heard through a real engine.

use std::sync::Arc;

use wave_engine::*;

fn pts(list: &[(f64, u8, u8)]) -> Vec<SigPoint> {
    list.iter().map(|&(beat, num, den)| SigPoint { beat, num, den }).collect()
}

fn map(list: &[(f64, u8, u8)]) -> SigMap {
    SigMap::compile(&pts(list))
}

fn clip(id: Id, track: u32, start: f64, len: f64, pat: Id) -> Clip {
    Clip {
        id,
        track,
        start,
        len,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: pat,
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
fn the_constant_map_is_the_old_bar_math() {
    let m = SigMap::constant();
    assert_eq!(m.bar_beats_at(123.0), 4.0);
    assert_eq!(m.bar_at(7.9), 1.0);
    assert_eq!(m.bar_at(8.0), 2.0);
    assert_eq!(m.beat_at_bar(2.0), 8.0);
    assert!(m.is_bar_start(4.0));
    assert!(!m.is_bar_start(5.0));
    assert_eq!(map(&[]).bar_beats_at(99.0), 4.0, "no points = 4/4 everywhere");
}

#[test]
fn a_change_reflows_the_bars_from_its_beat() {
    // 3/4 from beat 8: bars 0-4-8, then 8-11-14
    let m = map(&[(8.0, 3, 4)]);
    assert_eq!(m.bar_at(7.999), 1.0);
    assert_eq!(m.bar_at(8.0), 2.0);
    assert_eq!(m.bar_at(13.5), 3.0);
    assert_eq!(m.beat_at_bar(2.0), 8.0);
    assert_eq!(m.beat_at_bar(3.0), 11.0);
    assert_eq!(m.bar_beats_at(7.0), 4.0);
    assert_eq!(m.bar_beats_at(8.0), 3.0);
    assert!(m.is_bar_start(11.0));
    assert!(!m.is_bar_start(10.0));
}

#[test]
fn a_mid_bar_change_truncates_the_bar_before_it() {
    // 3/4 lands mid-bar (beat 10 of a 4/4 grid): [8,10) stays as a fragment and counts,
    // the new grid starts at 10
    let m = map(&[(10.0, 3, 4)]);
    assert_eq!(m.bar_at(9.0), 2.0, "the fragment is bar 2");
    assert_eq!(m.bar_at(10.0), 3.0);
    assert_eq!(m.beat_at_bar(2.0), 8.0);
    assert_eq!(m.beat_at_bar(3.0), 10.0);
    assert_eq!(m.beat_at_bar(4.0), 13.0);
    assert!(m.is_bar_start(8.0));
    assert!(m.is_bar_start(10.0));
    assert!(!m.is_bar_start(9.0));
}

#[test]
fn off_grid_bars_start_between_beats() {
    // 7/8 is 3.5 beats: its boundaries land on the half-beat, exactly (dyadic bar lengths
    // keep floating point exact)
    let m = map(&[(0.0, 7, 8)]);
    assert_eq!(m.bar_beats_at(4.0), 3.5);
    assert_eq!(m.beat_at_bar(1.0), 3.5);
    assert_eq!(m.beat_at_bar(2.0), 7.0);
    assert_eq!(m.bar_at(3.5), 1.0);
    assert_eq!(m.bar_at(3.6), 1.0);
    assert!(m.is_bar_start(3.5));
    assert!(!m.is_bar_start(3.0), "a whole beat is not a bar start");
    assert!(!m.is_bar_start(4.0));
}

#[test]
fn bad_fields_clamp_and_snap() {
    // num clamps to 1..=32, a den off the 1/2/4/8/16 ladder reads as 4
    let m = map(&[(0.0, 99, 4)]);
    assert_eq!(m.bar_beats_at(1.0), 32.0);
    let m = map(&[(0.0, 0, 7)]);
    assert_eq!(m.bar_beats_at(1.0), 1.0, "num 0 -> 1, den 7 -> 4 (1/4)");
    let m = map(&[(0.0, 7, 16)]);
    assert_eq!(m.bar_beats_at(1.0), 1.75, "7/16 keeps its odd quarters");
    let m = map(&[(0.0, 1, 1)]);
    assert_eq!(m.bar_beats_at(1.0), 4.0, "1/1 is a whole-note bar");
}

#[test]
fn two_points_at_one_beat_the_last_governs() {
    let m = map(&[(4.0, 3, 4), (4.0, 7, 8)]);
    assert_eq!(m.bar_beats_at(4.0), 3.5);
    assert_eq!(m.bar_beats_at(3.9), 4.0);
    let m = map(&[(0.0, 3, 4), (0.0, 6, 8)]);
    assert_eq!(m.bar_beats_at(0.0), 3.0, "at beat 0 too, the last point wins over 4/4");
}

#[test]
fn bars_before_zero_continue_backwards() {
    // a count-in plays negative beats: bars extend backwards on the opening grid
    let m = map(&[]);
    assert_eq!(m.bar_at(-4.0), -1.0);
    assert_eq!(m.beat_at_bar(-1.0), -4.0);
    assert!(m.is_bar_start(-4.0));
    let m = map(&[(0.0, 7, 8)]);
    assert_eq!(m.bar_at(-3.5), -1.0);
    assert_eq!(m.beat_at_bar(-1.0), -3.5);
}

#[test]
fn bar_numbers_round_trip_across_changes() {
    let m = map(&[(0.0, 5, 4), (12.0, 7, 8), (30.0, 3, 4)]);
    for bar in -3..40 {
        let b = m.beat_at_bar(f64::from(bar));
        assert!((m.bar_at(b + 1e-9) - f64::from(bar)).abs() < 1e-9, "bar {bar} -> beat {b}");
    }
}

#[test]
fn the_song_ends_on_the_maps_next_bar_line() {
    let mut p = Project::routed();
    p.clips.push(clip(900, 0, 0.0, 10.2, 1));
    assert!((p.song_end() - 12.0).abs() < 1e-9, "4/4: the clip ending at 10.2 ends bar 3");
    p.sig_points = pts(&[(8.0, 3, 4)]);
    assert!((p.song_end() - 11.0).abs() < 1e-9, "3/4 from beat 8: 10.2 ends the bar that spans 8..11");
}

#[test]
fn set_sig_points_sorts_clamps_and_caps() {
    let mut p = Project::routed();
    let mut list = pts(&[(16.0, 5, 4), (0.0, 250, 3), (8.0, 7, 9)]);
    list.push(SigPoint { beat: 4096.0, num: 1, den: 4 });
    ops::apply(&mut p, &Op::SetSigPoints { points: list }).unwrap();
    let got: Vec<(f64, u8, u8)> = p.sig_points.iter().map(|s| (s.beat, s.num, s.den)).collect();
    assert_eq!(got, [(0.0, MAX_SIG_NUM, 4), (8.0, 7, 4), (16.0, 5, 4), (TOTAL_BEATS, 1, 4)], "sorted, clamped, den-snapped, beat-capped");
    let many: Vec<SigPoint> = (0..1100).map(|i| SigPoint { beat: f64::from(i), num: 4, den: 4 }).collect();
    assert!(ops::apply(&mut p, &Op::SetSigPoints { points: many }).is_err(), "above the cap the op refuses");
    ops::apply(&mut p, &Op::SetSigPoints { points: vec![] }).unwrap();
    assert!(p.sig_points.is_empty());
}

#[test]
fn a_pattern_fits_whole_bars_of_its_own_grid() {
    // a pattern keeps the bar length it was created with (miskibin's policy: FL and Live
    // keep patterns independent of the arrangement); its fit rounds to that grid
    let mut p = Project::routed();
    p.patterns[0].bar = 3.5;
    p.patterns[0].notes.push(Note::new(1, 12.0, 4.0, None)); // content ends at 4 beats
    p.patterns[0].fit_len();
    assert!((p.patterns[0].len - 7.0).abs() < 1e-9, "4 beats round up to two 3.5-beat bars");
    assert!((p.patterns[0].min_len - 7.0).abs() < 1e-9);
    let mut q = Project::routed();
    q.patterns[0].notes.push(Note::new(1, 12.0, 4.0, None)); // ends at 4 beats: exactly one bar
    q.patterns[0].fit_len();
    assert!((q.patterns[0].len - 4.0).abs() < 1e-9, "the default grid is 4/4");
    q.patterns[0].notes.push(Note::new(1, 13.0, 4.0, None)); // ends at 4.25
    q.patterns[0].fit_len();
    assert!((q.patterns[0].len - 8.0).abs() < 1e-9);
}

#[test]
fn migrate_refits_to_the_stored_bar_and_stays_idempotent() {
    // the stored bar (the signature where the pattern was created) drives the reload fit:
    // whatever the arrangement looks like, migrate keeps the stored grid and is idempotent
    let mut p = Project::routed();
    p.clips.push(clip(902, 0, 8.0, 7.0, 1));
    p.sig_points = pts(&[(8.0, 7, 8)]);
    p.patterns[0].bar = 3.5;
    p.patterns[0].min_len = 7.0;
    p.patterns[0].len = 7.0;
    p.migrate();
    assert!((p.patterns[0].len - 7.0).abs() < 1e-9 && (p.patterns[0].min_len - 7.0).abs() < 1e-9);
    let before = serde_json::to_string(&p).unwrap();
    p.migrate();
    assert_eq!(serde_json::to_string(&p).unwrap(), before, "migrate twice changes nothing");
    let v = serde_json::from_str::<serde_json::Value>(&before).unwrap()["version"].as_u64().unwrap();
    assert_eq!(v, u64::from(PROJECT_VERSION));
}

// ---------------------------------------------------------------- metronome

const SR: u32 = 48000;
const BLOCK: usize = 256;

fn frames(beats: f64, bpm: f64) -> usize {
    (beats * 60.0 / bpm * SR as f64).round() as usize
}

/// A quiet project (clips but no notes, no fx) so only the metronome sounds.
fn quiet() -> Project {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    for pt in p.patterns.iter_mut() {
        pt.notes.clear();
    }
    ops::apply_all(&mut p, &[Op::SetBpm { bpm: 120.0 }, Op::PutClips { clips: vec![clip(950, 0, 0.0, 32.0, 1)] }]).unwrap();
    p
}

/// Plays `beats` beats with the metronome on (no count-in), collecting the left channel.
fn play_with_metro(p: Project, beats: f64) -> Vec<f32> {
    let e = Engine::new(p, Arc::new(NoPlugins));
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.set_transport(Some(true), Some(false), None, None);
    e.play(0.0);
    let mut out: Vec<f32> = vec![];
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    for _ in 0..frames(beats, 120.0) / BLOCK {
        rt.process(&mut l, &mut r, None);
        out.extend_from_slice(&l);
    }
    let _ = e.stop();
    let rms = (out.iter().map(|x| *x as f64).map(|x| x * x).sum::<f64>() / out.len().max(1) as f64).sqrt();
    eprintln!("probe: {n_blocks} blocks, {outlen} frames, rms {rms}", n_blocks = frames(beats, 120.0) / BLOCK, outlen = out.len());
    out
}

/// Energy in the window ±0.15 beat around `beat`, and its zero-crossing rate (the
/// accent click is 1760 Hz against the plain click's 1320).
fn burst(out: &[f32], beat: f64) -> Option<(f32, usize)> {
    let c = frames(beat, 120.0);
    let w = frames(0.15, 120.0);
    let win = &out[c.saturating_sub(w)..(c + w).min(out.len())];
    let rms = (win.iter().map(|x| *x as f64).map(|x| x * x).sum::<f64>() / win.len() as f64).sqrt() as f32;
    if rms < 0.01 {
        return None;
    }
    let zc = win.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    Some((rms, zc))
}

#[test]
fn metronome_accents_follow_the_map() {
    // 3/4 from beat 0, 7/8 from beat 9 (a bar line of the 3/4 grid: no fragment)
    let mut p = quiet();
    p.sig_points = pts(&[(0.0, 3, 4), (9.0, 7, 8)]);
    let out = play_with_metro(p, 18.0);
    // every beat clicks; accents land on 0, 3, 6, 9, then the 7/8 grid 12.5, 16 — and
    // 12.5 is a click between beats, which 4/4's rem_euclid accent could never make
    for b in 0..18 {
        assert!(burst(&out, f64::from(b)).is_some(), "beat {b} clicks");
    }
    let (_, zc_plain) = burst(&out, 1.0).unwrap();
    {
        let (rms, zc) = burst(&out, 12.5).expect("a fractional bar start still gets its accent");
        assert!(rms > 0.015);
        assert!(zc > zc_plain + 20, "and it is the accented click, not a spill of beat 12 or 13");
    }
    for b in [3.5, 8.5, 11.5, 13.5] {
        assert!(burst(&out, b).is_none(), "no click at {b}: halves are not bar starts here");
    }
    // the accents ring higher than the plain clicks (1760 vs 1320 Hz): the zc count
    // separates them
    for b in [0.0, 3.0, 6.0, 9.0, 16.0] {
        let (_, zc) = burst(&out, b).unwrap();
        assert!(zc > zc_plain + 20, "beat {b} is the accented click (zc {zc} > plain {zc_plain})");
    }
}

#[test]
fn the_count_in_is_one_bar_of_where_you_record_and_accents_its_bar_start() {
    // recording from beat 12 under 3/4 from beat 8: one bar of count-in is three beats
    // (9, 10, 11 — not four), and beat 11 is a bar start of the map, so it accents
    let mut p = quiet();
    p.sig_points = pts(&[(8.0, 3, 4)]);
    for t in p.tracks.iter_mut() {
        t.arm = true;
    }
    let dir = std::env::temp_dir().join(format!("wave-sig-countin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let e = Engine::new(p, Arc::new(NoPlugins));
    e.save(&dir.join("song.wave")).unwrap();
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.set_transport(Some(false), Some(true), Some(true), None);
    e.play(12.0);
    let mut out: Vec<f32> = vec![];
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    for _ in 0..frames(5.0, 120.0) / BLOCK {
        rt.process(&mut l, &mut r, None);
        out.extend_from_slice(&l);
    }
    let _ = e.stop();
    let _ = std::fs::remove_dir_all(&dir);
    // the stream starts when the count-in does (beat 9): out beat 0 is song beat 9
    for b in [0.0, 1.0, 2.0] {
        assert!(burst(&out, b).is_some(), "count-in beat {} clicks", 9.0 + b);
    }
    assert!(burst(&out, 3.0).is_none(), "three count beats, not four (and metro is off once recording starts)");
    assert!(burst(&out, 4.0).is_none());
    let (_, zc_plain) = burst(&out, 0.0).unwrap();
    let (_, zc_bar) = burst(&out, 2.0).unwrap();
    assert!(zc_bar > zc_plain + 20, "beat 11 opens a bar of the map: it accents");
}

#[test]
fn a_crafted_file_that_overflows_the_bar_count_opens_without_panicking() {
    // only putClips validates ranges: a shared file can carry a 1e308-long clip and a 1/16
    // change, and song_end runs from the timeline build inside the open — the bar count
    // overflows to infinity and beat_at_bar used to hit an unreachable!() there
    let v = serde_json::json!({
        "version": PROJECT_VERSION, "bpm": 120.0,
        "inserts": [{ "id": 0, "name": "Master", "color": "#d5dae2", "vol": 0.7937, "pan": 0.0, "width": 1.0, "muted": false, "solo": false, "fx": [], "sends": [], "master": true }],
        "tracks": [{ "id": 1, "name": "T", "color": "#4f8ef7", "muted": false, "solo": false, "arm": false, "vol": 0.7937, "h": 56, "insert": 0 }],
        "sources": [],
        "patterns": [{ "id": 1, "name": "P", "color": "#4f8ef7", "len": 4.0, "minLen": 4.0, "notes": [] }],
        "clips": [{ "id": 10, "track": 0, "start": 0.0, "len": 1e308, "offset": 0.0, "kind": "pat", "ref": 1, "muted": false, "gain": 0.0, "pan": 0.0, "fade_in": 0.0, "fade_out": 0.0, "norm": false, "rev": false, "stretch": 1.0, "resample": false, "pitch": 0.0, "root": 60 }],
        "sigPoints": [{ "beat": 0.0, "num": 1, "den": 16 }],
        "loop": null, "nextId": 100,
    });
    let mut p = Project::from_json(v).expect("the file parses");
    p.migrate();
    let end = p.song_end();
    assert!(end.is_finite(), "the song end stays finite: {end}");
    assert!(end <= TOTAL_BEATS, "the song end clamps into the song: {end}");
}

#[test]
fn moving_a_clip_never_resizes_the_pattern() {
    // miskibin's policy: moving a clip cannot resize a pattern it does not edit — the
    // pattern keeps the bar grid it was created with, wherever its clips sit
    let mut p = Project::routed();
    p.sig_points = pts(&[(8.0, 7, 8)]);
    p.clips.push(clip(910, 0, 0.0, 8.0, 1));
    p.patterns[0].min_len = 8.0;
    p.patterns[0].len = 8.0;
    ops::apply(&mut p, &Op::PutClips { clips: vec![clip(910, 0, 8.0, 8.0, 1)] }).unwrap();
    assert!((p.patterns[0].len - 8.0).abs() < 1e-9, "the length stays: {}", p.patterns[0].len);
    assert!((p.patterns[0].min_len - 8.0).abs() < 1e-9, "{}", p.patterns[0].min_len);
    assert!((p.clips[0].len - 8.0).abs() < 1e-9);
    ops::apply(&mut p, &Op::RemoveClips { ids: vec![910] }).unwrap();
    assert!((p.patterns[0].len - 8.0).abs() < 1e-9, "removing the clip changes nothing either");
}

#[test]
fn setting_the_map_never_resizes_patterns() {
    // the map reflows where the bars fall (the ruler, the accents, the snap); a pattern
    // keeps the bar grid it was created with and its length with it
    let mut p = Project::routed();
    p.clips.push(clip(911, 0, 0.0, 4.0, 1));
    ops::apply(&mut p, &Op::SetSigPoints { points: pts(&[(0.0, 3, 4)]) }).unwrap();
    assert!((p.patterns[0].len - 4.0).abs() < 1e-9, "{}", p.patterns[0].len);
    assert!((p.patterns[0].min_len - 4.0).abs() < 1e-9, "{}", p.patterns[0].min_len);
    assert!((p.clips[0].len - 4.0).abs() < 1e-9);
}

#[test]
fn a_pattern_keeps_its_grid_across_migrate_and_map_edits() {
    // the stored bar survives a reload byte-for-byte, and editing the song's signature map
    // never touches it — the two drift sources the position-fit policy had
    let mut p = Project::routed();
    p.patterns[0].bar = 3.5;
    p.patterns[0].min_len = 7.0;
    p.patterns[0].len = 7.0;
    let before =
        serde_json::json!({ "patterns": serde_json::to_value(&p.patterns).unwrap(), "clips": serde_json::to_value(&p.clips).unwrap() });
    ops::apply(&mut p, &Op::SetSigPoints { points: pts(&[(0.0, 3, 4)]) }).unwrap();
    let after =
        serde_json::json!({ "patterns": serde_json::to_value(&p.patterns).unwrap(), "clips": serde_json::to_value(&p.clips).unwrap() });
    assert_eq!(after, before, "the map edit changes no pattern or clip");
    p.migrate();
    let reloaded =
        serde_json::json!({ "patterns": serde_json::to_value(&p.patterns).unwrap(), "clips": serde_json::to_value(&p.clips).unwrap() });
    assert_eq!(reloaded, before, "the reload changes no pattern or clip");
}

#[test]
fn a_bad_bar_refuses_the_op_and_leaves_it_untouched() {
    // putPattern validates the pattern's own bar (a hand-built op cannot smuggle in a
    // zero or negative grid); apply promises "on error the project is unchanged"
    let mut p = Project::routed();
    let mut pt = p.patterns[0].clone();
    pt.id = 40;
    pt.bar = 0.0;
    let before = serde_json::to_string(&p).unwrap();
    let err = ops::apply(&mut p, &Op::PutPattern { pattern: pt }).unwrap_err();
    assert!(format!("{err}").contains("pattern bar"), "{err}");
    assert_eq!(serde_json::to_string(&p).unwrap(), before, "the refused op changed nothing");
}

#[test]
fn reassigning_a_clip_never_resizes_either_pattern() {
    // a clip moving from pattern A to pattern B changes neither: each keeps the bar grid
    // it was created with, whatever the arrangement does
    let mut p = Project::routed();
    p.sig_points = pts(&[(8.0, 7, 8)]);
    p.patterns.push(Pattern {
        id: 2,
        name: "B".into(),
        color: "#4f8ef7".into(),
        len: 8.0,
        min_len: 8.0,
        bar: 4.0,
        notes: vec![],
        swing: 0.0,
        channel_loops: std::collections::BTreeMap::new(),
    });
    p.patterns[0].min_len = 5.0;
    p.patterns[0].len = 8.0;
    p.clips.push(clip(912, 0, 0.0, 8.0, 1));
    ops::apply(&mut p, &Op::PutClips { clips: vec![clip(912, 0, 8.0, 8.0, 2)] }).unwrap();
    assert!((p.patterns[0].len - 8.0).abs() < 1e-9 && (p.patterns[0].min_len - 5.0).abs() < 1e-9, "A keeps its grid");
    assert!((p.patterns[1].len - 8.0).abs() < 1e-9, "B keeps its grid");
}
