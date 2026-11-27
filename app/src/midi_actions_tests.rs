//! MIDI files through the shared actions, as the app and `synkopa --project FILE midi …` use them:
//! inspect, import (new pattern, drums as library samplers, tempo, into a channel), export of
//! the song and of a pattern, and an import → export round trip that keeps every note.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use wave_cli::Files as _;
use wave_engine::midi_file::{self, OutTrack, SmfNote, DRUM_CHANNEL};
use wave_engine::{Clip, Note, Pattern};

use super::placeholder_clip;

use crate::headless::Files;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-midi-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn n(key: u8, start: f64, len: f64, vel: u8) -> SmfNote {
    SmfNote { key, vel, start, len }
}

/// A 2-bar file at 97 BPM: a fingered bass, a piano with a chord, and GM drums.
fn song_file(d: &Path) -> (PathBuf, Vec<OutTrack>) {
    let tracks = vec![
        OutTrack {
            name: "Bass".into(),
            channel: 1,
            program: Some(33),
            notes: vec![n(36, 0.0, 1.0, 100), n(36, 1.5, 0.5, 80), n(43, 4.0, 3.75, 127)],
        },
        OutTrack {
            name: "Piano".into(),
            channel: 0,
            program: Some(0),
            notes: vec![n(60, 0.0, 2.0, 70), n(64, 0.0, 2.0, 70), n(67, 0.0, 2.0, 70), n(72, 6.25, 0.25, 1)],
        },
        OutTrack {
            name: "Drums".into(),
            channel: DRUM_CHANNEL,
            program: None,
            notes: vec![n(36, 0.0, 0.25, 110), n(42, 0.5, 0.25, 60), n(38, 1.0, 0.25, 100), n(42, 1.5, 0.25, 60), n(36, 4.0, 0.25, 110)],
        },
    ];
    let path = d.join("groove.mid");
    std::fs::write(&path, midi_file::write(&[(0.0, 97.0)], "Groove", &tracks, &[]).unwrap()).unwrap();
    (path, tracks)
}

/// A file whose tempo changes: 120 at tick 0, 90 at beat 4, 150 at beat 8 (written with our
/// own writer — the raw-event edge cases are `midi_file`'s unit tests; this is the import).
fn mapped_file(d: &Path) -> PathBuf {
    let tracks = vec![OutTrack { name: "P".into(), channel: 0, program: Some(0), notes: vec![n(60, 0.0, 1.0, 100)] }];
    let path = d.join("mapped.mid");
    std::fs::write(&path, midi_file::write(&[(0.0, 120.0), (4.0, 90.0), (8.0, 150.0)], "M", &tracks, &[]).unwrap()).unwrap();
    path
}

fn call(doc: &dyn wave_cli::Doc, cmd: &str, a: Value) -> Value {
    doc.call(cmd, &a, None).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

#[test]
fn inspect_lists_parts_with_what_each_becomes() {
    let d = tmp("inspect");
    let (path, _) = song_file(&d);
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_inspect", json!({ "path": path }));
    assert_eq!(r["bpm"], 97.0);
    let parts = r["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0]["name"], "Bass");
    assert_eq!(parts[0]["gmName"], "Fingered Bass");
    assert_eq!(parts[0]["becomes"], json!({ "kind": "synth", "preset": "Warm Bass" }));
    assert_eq!(parts[2]["drums"], true);
    assert_eq!(parts[2]["channel"], 10);
    let drums: Vec<&str> = parts[2]["becomes"]["drums"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(drums, ["Bass Drum", "Snare", "Closed Hi-hat"]);
    assert!(!doc.dirty(), "inspecting is not an edit");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn import_makes_one_pattern_with_drum_samplers_and_round_trips_through_export() {
    let d = tmp("roundtrip");
    let (path, tracks) = song_file(&d);
    let doc = Files.create("empty").unwrap();
    let before = call(&*doc, "context", json!({ "full": true }));
    let r = call(&*doc, "midi_import", json!({ "path": path, "setTempo": true, "beat": 8 }));
    assert_eq!(r["tempoSet"], true);
    assert_eq!(r["len"], 8.0, "two bars");
    assert_eq!(r["beat"], 8.0);
    // bass + piano + 3 drums (kick, snare, hat)
    assert_eq!(r["channelIds"].as_array().unwrap().len(), 5);
    let ctx = call(&*doc, "context", json!({ "full": true }));
    assert_eq!(ctx["bpm"].as_f64().or(ctx["project"]["bpm"].as_f64()).unwrap_or(97.0), 97.0);
    assert_ne!(before, ctx);

    // the pattern exported alone gives back every note (keys, times, lengths, velocities)
    let pid = r["patternId"].as_u64().unwrap();
    let out = d.join("back.mid");
    let x = call(&*doc, "midi_export", json!({ "out": out, "pattern": pid }));
    assert_eq!(x["notes"], 12);
    let back = midi_file::parse(&std::fs::read(&out).unwrap()).unwrap();
    assert!((back.bpm.unwrap() - 97.0).abs() < 0.01);
    let mut want: Vec<(u8, SmfNote)> =
        tracks.iter().flat_map(|t| t.notes.iter().map(|n| (t.channel == DRUM_CHANNEL, *n))).map(|(dr, n)| (dr as u8, n)).collect();
    let mut got: Vec<(u8, SmfNote)> =
        back.parts.iter().flat_map(|p| p.notes.iter().map(|n| ((p.channel == DRUM_CHANNEL) as u8, *n))).collect();
    let key = |x: &(u8, SmfNote)| (x.0, (x.1.start * 1e4) as i64, x.1.key);
    want.sort_by_key(key);
    got.sort_by_key(key);
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert_eq!(g.0, w.0);
        assert_eq!(g.1.key, w.1.key);
        assert_eq!(g.1.vel, w.1.vel);
        assert!((g.1.start - w.1.start).abs() < 1e-3 && (g.1.len - w.1.len).abs() < 1e-3, "{g:?} vs {w:?}");
    }
    // programs survive: the bass synth and the piano (synth piano or Synkopa Instruments)
    let programs: Vec<Option<u8>> = back.parts.iter().filter(|p| p.channel != DRUM_CHANNEL).map(|p| p.program).collect();
    assert!(programs.contains(&Some(38)), "{programs:?}");

    // the song: the clip at beat 8 plays the pattern there
    let song = d.join("song.mid");
    call(&*doc, "midi_export", json!({ "out": song }));
    let s = midi_file::parse(&std::fs::read(&song).unwrap()).unwrap();
    let first = s.parts.iter().flat_map(|p| &p.notes).map(|n| n.start).fold(f64::MAX, f64::min);
    assert_eq!(first, 8.0);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_part_goes_into_a_channel_of_a_pattern_adding_or_replacing() {
    let d = tmp("into");
    let (path, _) = song_file(&d);
    let doc = Files.create("sketch").unwrap();
    let chans = call(&*doc, "channels", json!({}));
    let first = &chans.as_array().unwrap()[0];
    let (ch, name) = (first["id"].as_u64().unwrap(), first["name"].as_str().unwrap().to_string());
    let notes_of = |doc: &dyn wave_cli::Doc| -> Vec<u8> {
        let out = d.join("p.mid");
        call(doc, "midi_export", json!({ "out": out, "pattern": 1 }));
        let f = midi_file::parse(&std::fs::read(&out).unwrap()).unwrap();
        f.parts.iter().find(|p| p.name == name).map(|p| p.notes.iter().map(|n| n.key).collect()).unwrap_or_default()
    };
    let had = notes_of(&*doc).len();
    assert!(had > 0);
    let r = call(&*doc, "midi_import", json!({ "path": path, "parts": [0], "into": { "pattern": 1, "ch": ch, "replace": true } }));
    assert_eq!(r["notes"], 3);
    let keys = notes_of(&*doc);
    assert_eq!(keys.len(), 3, "replaced: {keys:?}");
    call(&*doc, "midi_import", json!({ "path": path, "parts": [0], "into": { "pattern": 1, "ch": ch } }));
    assert_eq!(notes_of(&*doc).len(), 6, "added");
    assert!(doc
        .call("midi_import", &json!({ "path": path, "parts": [0, 1], "into": { "pattern": 1, "ch": ch } }), None)
        .unwrap_err()
        .contains("exactly one part"));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_mapped_file_imports_its_tempo_map_with_set_tempo() {
    let d = tmp("mapped");
    let path = mapped_file(&d);
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_import", json!({ "path": path, "setTempo": true }));
    assert_eq!(r["tempoSet"], true);
    let ctx = call(&*doc, "context", json!({ "full": true }));
    let proj = ctx["project"].as_object().map(|p| p.to_owned()).unwrap_or_else(|| ctx.as_object().cloned().unwrap());
    assert_eq!(proj["bpm"], 120.0, "the file's tick-0 tempo is the base");
    let pts = proj["tempoPoints"].as_array().expect("the map came with the import");
    let got: Vec<(f64, f64)> = pts.iter().map(|p| (p["beat"].as_f64().unwrap(), p["bpm"].as_f64().unwrap())).collect();
    assert_eq!(got, vec![(4.0, 90.0), (8.0, 150.0)]);
    // without the opt-in the project keeps its own tempo entirely
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_import", json!({ "path": path }));
    assert_eq!(r["tempoSet"], false);
    let ctx = call(&*doc, "context", json!({ "full": true }));
    let proj = ctx["project"].as_object().map(|p| p.to_owned()).unwrap_or_else(|| ctx.as_object().cloned().unwrap());
    assert!(proj.get("tempoPoints").is_none() || proj["tempoPoints"].is_null(), "no map without the opt-in");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_mapped_project_exports_a_ramp_whose_file_elapsed_time_is_the_engines() {
    let d = tmp("exportmap");
    let doc = Files.create("sketch").unwrap();
    let ch = call(&*doc, "channels", json!({})).as_array().unwrap()[0]["id"].as_u64().unwrap();
    call(
        &*doc,
        "apply",
        json!({ "ops": [
            { "op": "setBpm", "bpm": 90.0 },
            { "op": "setTempoPoints", "points": [
                { "beat": 0.0, "bpm": 90.0, "ramp": true },
                { "beat": 8.0, "bpm": 140.0, "ramp": false },
            ] },
            { "op": "putPattern", "pattern": { "id": 1, "name": "P", "color": "#fff", "len": 4.0, "minLen": 4.0, "notes": [
                { "ch": ch, "step": 0.0, "len": 4.0, "p": 36 }
            ], "swing": 0.0, "channelLoops": {} } }],
        }),
    );
    // the map is the song's: the song export carries it (a pattern alone plays the base tempo)
    let clip = serde_json::to_value(Clip { id: 1, len: 4.0, ..placeholder_clip(1) }).unwrap();
    call(&*doc, "apply", json!({ "ops": [{ "op": "putClips", "clips": [clip] }] }));
    let out = d.join("ramp.mid");
    call(&*doc, "midi_export", json!({ "out": out }));
    let f = midi_file::parse(&std::fs::read(&out).unwrap()).unwrap();
    // the ramp 90→140 over 8 beats steps once per beat — each event at its interval's
    // effective BPM (the tick-0 value is beat 0's interval average, so it is not 90 itself)
    assert_eq!(f.tempo_points.len(), 8, "one point per beat of the glide plus its end: {:?}", f.tempo_points);
    let evs: Vec<(f64, f64)> = std::iter::once((0.0, f.bpm.unwrap())).chain(f.tempo_points.iter().map(|t| (t.beat, t.bpm))).collect();
    let mut rising = true;
    for w in evs.windows(2) {
        rising &= w[0].1 < w[1].1;
    }
    assert!(rising, "the glide rises: {evs:?}");
    // the file's own elapsed time (its µs tempos, summed per event interval) reaches beat 8
    // in the engine's integral time — 4.2417 s, not the 4.3629 s the stepped-start events of
    // the old export played (#322: test the elapsed time, not just the BPM values)
    let mut t = 0.0;
    for w in evs.windows(2) {
        t += (w[1].0 - w[0].0) * 60.0 / w[0].1;
    }
    let want = 60.0 * 8.0 * (140.0f64 / 90.0).ln() / 50.0;
    assert!((want - 4.2417).abs() < 1e-3, "the integral is the drift benchmark: {want}");
    assert!((t - want).abs() < 1e-3, "the file plays the glide in {t} s, the engine in {want} s");
    // beyond the glide the hold at 140 is exact
    assert_eq!(f.tempo_points[7], midi_file::SmfTempo { beat: 8.0, bpm: 140.0 });
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn imported_tempo_points_land_at_the_insertion_beat() {
    // the notes respect the playlist insertion position, and the map travels with them: the
    // file's beat 0 is where the clip starts, so its beat-4 change lands at 16 + 4
    let d = tmp("mapoffset");
    let path = mapped_file(&d);
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_import", json!({ "path": path, "setTempo": true, "beat": 16.0 }));
    assert_eq!(r["beat"], 16.0);
    let ctx = call(&*doc, "context", json!({ "full": true }));
    let proj = ctx["project"].as_object().map(|p| p.to_owned()).unwrap_or_else(|| ctx.as_object().cloned().unwrap());
    let pts = proj["tempoPoints"].as_array().expect("the map came with the import");
    let got: Vec<(f64, f64)> = pts.iter().map(|p| (p["beat"].as_f64().unwrap(), p["bpm"].as_f64().unwrap())).collect();
    assert_eq!(got, vec![(20.0, 90.0), (24.0, 150.0)]);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn inspect_exposes_the_tempo_map_even_without_a_tick_0_tempo() {
    // MidiInfo requires `tempoPoints`, and the dialog reads its length whenever the tick-0
    // tempo is null — which includes files with no tempo events at all: the response must
    // always carry the array (a TypeError here broke the dialog for every such file)
    let d = tmp("inspectmap");
    let tracks = vec![OutTrack { name: "P".into(), channel: 0, program: Some(0), notes: vec![n(60, 0.0, 1.0, 100)] }];
    let path = d.join("late.mid");
    std::fs::write(&path, midi_file::write(&[(2.0, 140.0)], "L", &tracks, &[]).unwrap()).unwrap();
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_inspect", json!({ "path": path }));
    assert_eq!(r["bpm"], Value::Null, "no event at tick 0: the default governs");
    let pts = r["tempoPoints"].as_array().expect("tempoPoints is always an array");
    assert_eq!(pts.len(), 1);
    assert_eq!(pts[0]["beat"], 2.0);
    assert_eq!(pts[0]["bpm"], 140.0);
    // imported with its tempo, beats 0–2 play MIDI's implied 120 as the file does, not the
    // project's old tempo
    call(&*doc, "apply", json!({ "ops": [{ "op": "setBpm", "bpm": 100.0 }] }));
    call(&*doc, "midi_import", json!({ "path": path, "setTempo": true, "beat": 0 }));
    let p = &call(&*doc, "context", json!({ "full": true }))["project"];
    assert_eq!(p["bpm"], 120.0);
    assert_eq!(p["tempoPoints"].as_array().map(|a| a.len()), Some(1));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn importing_a_point_above_the_limit_discloses_the_clamp() {
    // SetTempoPoints clamps values into 20–300; the import says so instead of normalizing
    // silently (a 400-BPM change arriving as 300 without a word would corrupt the user's
    // reading of the file)
    let d = tmp("clamped");
    let tracks = vec![OutTrack { name: "P".into(), channel: 0, program: Some(0), notes: vec![n(60, 0.0, 1.0, 100)] }];
    let path = d.join("fast.mid");
    std::fs::write(&path, midi_file::write(&[(0.0, 120.0), (4.0, 400.0)], "F", &tracks, &[]).unwrap()).unwrap();
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_import", json!({ "path": path, "setTempo": true }));
    assert_eq!(r["tempoSet"], true);
    let warns: Vec<String> = r["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect();
    assert!(warns.iter().any(|w| w.contains("outside Synkopa's 20–300")), "{warns:?}");
    let ctx = call(&*doc, "context", json!({ "full": true }));
    let proj = ctx["project"].as_object().map(|p| p.to_owned()).unwrap_or_else(|| ctx.as_object().cloned().unwrap());
    assert_eq!(proj["tempoPoints"][0]["bpm"], 300.0, "clamped to the engine's limit");
    // the disclosure names where the point lands (the insertion beat included), so the user
    // finds the clamped value in their project
    let r = call(&*doc, "midi_import", json!({ "path": path, "setTempo": true, "beat": 16.0 }));
    let warns: Vec<String> = r["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect();
    assert!(warns.iter().any(|w| w.contains("landing at beat 20")), "{warns:?}");
    // the opt-out imports nothing: no warning about an imported tempo may fire either
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_import", json!({ "path": path }));
    assert_eq!(r["tempoSet"], false);
    assert_eq!(r["warnings"].as_array().unwrap().len(), 0, "{:?}", r["warnings"]);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn bad_files_and_empty_choices_are_errors_and_change_nothing() {
    let d = tmp("bad");
    let junk = d.join("x.mid");
    std::fs::write(&junk, b"MThd nope").unwrap();
    let doc = Files.create("empty").unwrap();
    assert!(doc.call("midi_inspect", &json!({ "path": junk }), None).unwrap_err().contains("not a MIDI file"));
    let (path, _) = song_file(&d);
    assert!(doc.call("midi_import", &json!({ "path": path, "parts": [9] }), None).unwrap_err().contains("no part 9"));
    assert!(doc.call("midi_import", &json!({ "path": path, "parts": [] }), None).unwrap_err().contains("no notes"));
    assert!(!doc.dirty());
    assert!(doc.call("midi_export", &json!({ "out": d.join("e.mid") }), None).unwrap_err().contains("no notes"));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn slide_notes_are_left_out_of_a_midi_file() {
    let held = Note::new(1, 0.0, 8.0, Some(36));
    let slide = Note { slide: true, ..Note::new(1, 4.0, 2.0, Some(48)) };
    let pat = Pattern {
        id: 1,
        name: "P".into(),
        color: "#fff".into(),
        len: 4.0,
        min_len: 4.0,
        bar: 4.0,
        notes: vec![held, slide],
        swing: 0.0,
        channel_loops: std::collections::BTreeMap::new(),
    };
    let mut out = vec![];
    super::clip_notes(&wave_engine::Clip { len: 4.0, ..super::placeholder_clip(1) }, &pat, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].2, Some(36));
}

#[test]
fn the_cli_writes_and_lists_slide_notes_and_refuses_a_sliding_trigger() {
    let d = tmp("slidecli");
    let doc = Files.create("sketch").unwrap();
    let chans = call(&*doc, "channels", json!({}));
    let ch = chans.as_array().unwrap()[0]["id"].as_u64().unwrap();
    let r = call(
        &*doc,
        "pattern",
        json!({ "id": 1, "ch": ch, "notes": [{ "step": 0, "len": 8, "p": 36 }, { "step": 4, "len": 2, "p": 48, "slide": true }] }),
    );
    // the reply lists the row's notes, the slide marked
    let text = serde_json::to_string(&r).unwrap();
    assert!(text.contains("\"slide\":true"), "{text}");
    let e = doc
        .call("pattern", &json!({ "id": 1, "ch": ch, "notes": [{ "step": 0, "len": 1, "p": 36, "trig": true, "slide": true }] }), None)
        .unwrap_err();
    assert!(e.contains("cannot be a trigger"), "{e}");
    // the exported file has the held note only
    let out = d.join("s.mid");
    call(&*doc, "midi_export", json!({ "out": out, "pattern": 1 }));
    let f = midi_file::parse(&std::fs::read(&out).unwrap()).unwrap();
    assert!(f.parts.iter().flat_map(|p| &p.notes).all(|n| n.key != 48));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_channel_joins_and_leaves_a_filter_group_from_the_cli() {
    let doc = Files.create("sketch").unwrap();
    let chans = call(&*doc, "channels", json!({}));
    let ch = chans.as_array().unwrap()[0]["id"].as_u64().unwrap();
    let r = call(&*doc, "channels", json!({ "ch": ch, "group": " Drums " }));
    let row = r["channels"].as_array().unwrap().iter().find(|c| c["id"] == ch).unwrap().clone();
    assert_eq!(row["group"], "Drums");
    let r = call(&*doc, "channels", json!({ "ch": ch, "group": "" }));
    let row = r["channels"].as_array().unwrap().iter().find(|c| c["id"] == ch).unwrap().clone();
    assert_eq!(row["group"], Value::Null);
    assert!(doc.call("channels", &json!({ "ch": ch, "group": "x".repeat(41) }), None).unwrap_err().contains("40 characters"));
}

#[test]
fn midi_export_includes_channel_loop_repeats() {
    // the exported file matches what plays: a looped channel writes its cell's repeats,
    // and (the write path trims it) nothing past the loop exists to export
    let d = tmp("loops");
    let doc = Files.create("sketch").unwrap();
    let r = call(&*doc, "pattern", json!({ "new": true }));
    let pid = r["id"].as_u64().unwrap();
    call(&*doc, "pattern", json!({ "id": pid, "ch": "Hat", "steps": "x.x", "loop": 3 }));
    let out = d.join("loops.mid");
    let x = call(&*doc, "midi_export", json!({ "out": out, "pattern": pid }));
    // the 2-step cell repeats every 3 steps over 16: hits at 0,2,3,5,6,8,9,11,12,14,15
    assert_eq!(x["notes"], 11, "{x}");
    let back = midi_file::parse(&std::fs::read(&out).unwrap()).unwrap();
    let starts: Vec<i64> = back.parts.iter().flat_map(|p| &p.notes).map(|n| (n.start * 4.0 * 1e4).round() as i64).collect();
    let want: Vec<i64> = [0.0, 2.0, 3.0, 5.0, 6.0, 8.0, 9.0, 11.0, 12.0, 14.0, 15.0].iter().map(|s| (s * 1e4) as i64).collect();
    let mut got = starts.clone();
    got.sort();
    assert_eq!(got, want, "steps in the file: {:?}", starts.iter().map(|s| *s as f64 / 1e4).collect::<Vec<_>>());
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_mapped_file_imports_its_meter_shifted_to_where_it_lands() {
    let d = tmp("sigimport");
    let path = d.join("mapped.mid");
    let tracks =
        vec![OutTrack { name: "Bass".into(), channel: 0, program: Some(33), notes: vec![n(36, 0.0, 1.0, 100), n(43, 8.5, 0.5, 90)] }];
    // 7/8 opener + a 3/4 change at beat 7
    std::fs::write(&path, midi_file::write(&[(0.0, 97.0)], "Mapped", &tracks, &[(0.0, 7, 8), (7.0, 3, 4)]).unwrap()).unwrap();
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_inspect", json!({ "path": path }));
    assert_eq!(
        r["sigPoints"],
        json!([{ "beat": 0.0, "num": 7, "den": 8 }, { "beat": 7.0, "num": 3, "den": 4 }]),
        "the inspect shows the map: the 7/8 opener is a point (a project cannot imply a meter)"
    );
    let r = call(&*doc, "midi_import", json!({ "path": path, "setTempo": true, "beat": 0 }));
    assert_eq!(r["tempoSet"], true);
    assert_eq!(r["len"], 10.5, "9 beats of notes end inside the second 3/4 bar: 3.5 + 7 = 10.5");
    let ctx = call(&*doc, "context", json!({ "full": true }));
    let sigs = &ctx["project"]["sigPoints"];
    assert_eq!(sigs.as_array().map(|a| a.len()), Some(2), "{sigs}");
    assert_eq!(sigs[0]["num"], 7);
    assert_eq!(sigs[0]["den"], 8);
    assert_eq!(sigs[1]["beat"], 7.0);
    // the pattern itself sits on the 7/8 grid (the refit reads the map through its clip)
    let pat = ctx["project"]["patterns"].as_array().unwrap().iter().find(|p| p["id"] == r["patternId"]).unwrap();
    assert_eq!(pat["len"], 10.5);
    // a mapped project exports its map: one signature meta per point
    let out = d.join("mapped-out.mid");
    call(&*doc, "midi_export", json!({ "out": out }));
    let back = midi_file::parse(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(back.sig_points, vec![midi_file::SmfSig { beat: 0.0, num: 7, den: 8 }, midi_file::SmfSig { beat: 7.0, num: 3, den: 4 }]);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn the_meter_travels_only_with_the_tempo_opt_in() {
    let d = tmp("sigsig");
    let path = d.join("mapped.mid");
    let tracks = vec![OutTrack { name: "Bass".into(), channel: 0, program: Some(33), notes: vec![n(36, 0.0, 1.0, 100)] }];
    std::fs::write(&path, midi_file::write(&[(0.0, 97.0)], "Mapped", &tracks, &[(4.0, 3, 4)]).unwrap()).unwrap();
    let doc = Files.create("empty").unwrap();
    let r = call(&*doc, "midi_import", json!({ "path": path, "beat": 0 }));
    assert_eq!(r["tempoSet"], false, "no opt-in: the file's meter does not touch the project");
    let ctx = call(&*doc, "context", json!({ "full": true }));
    assert!(ctx["project"]["sigPoints"].as_array().is_none_or(|a| a.is_empty()));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_pattern_exports_at_the_base_tempo_in_the_meter_where_it_plays() {
    // a pattern plays at the base tempo (the tempo map is the song's), and its file starts at
    // the pattern's own beat 0: the song's tempo glide and meter changes would misplace it
    let d = tmp("patmaps");
    let doc = Files.create("sketch").unwrap();
    let ch = call(&*doc, "channels", json!({})).as_array().unwrap()[0]["id"].as_u64().unwrap();
    let clip = serde_json::to_value(Clip { id: 1, start: 16.0, len: 3.0, ..placeholder_clip(1) }).unwrap();
    call(
        &*doc,
        "apply",
        json!({ "ops": [
            { "op": "setBpm", "bpm": 100.0 },
            { "op": "setTempoPoints", "points": [
                { "beat": 0.0, "bpm": 90.0, "ramp": true },
                { "beat": 8.0, "bpm": 140.0, "ramp": false },
            ] },
            { "op": "setSigPoints", "points": [{ "beat": 4.0, "num": 7, "den": 8 }, { "beat": 16.0, "num": 3, "den": 4 }] },
            { "op": "putPattern", "pattern": { "id": 1, "name": "P", "color": "#fff", "len": 3.0, "minLen": 3.0, "notes": [
                { "ch": ch, "step": 0.0, "len": 4.0, "p": 36 }
            ], "swing": 0.0, "channelLoops": {} } },
            { "op": "putClips", "clips": [clip] }],
        }),
    );
    let base = call(&*doc, "context", json!({ "full": true }))["project"]["bpm"].as_f64().unwrap();
    let out = d.join("pat.mid");
    call(&*doc, "midi_export", json!({ "out": out, "pattern": 1 }));
    let f = midi_file::parse(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(f.bpm, Some(base), "one tempo: the base the pattern plays at");
    assert!(f.tempo_points.is_empty(), "no song glide in a pattern file: {:?}", f.tempo_points);
    assert_eq!(f.sig_points, vec![midi_file::SmfSig { beat: 0.0, num: 3, den: 4 }], "the meter where the pattern plays");
    // the song export still carries both maps
    let song = d.join("song.mid");
    call(&*doc, "midi_export", json!({ "out": song }));
    let f = midi_file::parse(&std::fs::read(&song).unwrap()).unwrap();
    assert!(!f.tempo_points.is_empty());
    // (the 4/4 opener is the implied default and reads back as no point)
    assert_eq!(f.sig_points.len(), 2, "both changes: {:?}", f.sig_points);
    std::fs::remove_dir_all(&d).ok();
}
