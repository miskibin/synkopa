//! MIDI files through the shared actions, as the app and `synkopa --project FILE midi …` use them:
//! inspect, import (new pattern, drums as library samplers, tempo, into a channel), export of
//! the song and of a pattern, and an import → export round trip that keeps every note.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use wave_cli::Files as _;
use wave_engine::midi_file::{self, OutTrack, SmfNote, DRUM_CHANNEL};

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
    std::fs::write(&path, midi_file::write(97.0, "Groove", &tracks).unwrap()).unwrap();
    (path, tracks)
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
    assert_eq!(parts[0]["becomes"], json!({ "kind": "synth", "preset": "Bas Ciepły" }));
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
    // programs survive: the bass synth and the piano (synth piano or Synkopa Keys)
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
