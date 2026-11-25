//! Convert to MIDI through the shared action (`audio_midi_actions.rs`) on a project file, as
//! `synkopa audio-to-midi` and the Playlist's dialog run it: a sung-like melody becomes a pattern
//! under the clip, the preview matches what is written, and bad requests are refused.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use wave_cli::Files as _;
use wave_engine::sources::write_wav;

use crate::headless::Files;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-a2m-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A voice-like line at 120 BPM (a beat = 0.5 s): each key held `beats` beats, steady
/// with a 60 ms breath before the next, `lead` seconds of silence first.
fn melody(path: &Path, keys: &[(f32, f32)], lead: f32) {
    let sr = 44100.0f32;
    let total: f32 = lead + keys.iter().map(|k| k.1 * 0.5).sum::<f32>() + 0.5;
    let n = (total * sr) as usize;
    let mut data = vec![0f32; n * 2];
    let mut t0 = lead;
    let mut ph = 0f32;
    for &(k, beats) in keys {
        let dur = beats * 0.5 - 0.06;
        let i0 = (t0 * sr) as usize;
        for i in 0..(dur * sr) as usize {
            let t = i as f32 / sr;
            let m = k;
            ph += std::f32::consts::TAU * 440.0 * 2f32.powf((m - 69.0) / 12.0) / sr;
            let env = (t * 30.0).min(1.0) * ((dur - t) * 30.0).clamp(0.0, 1.0);
            let s = (ph.sin() + 0.5 * (2.0 * ph).sin() + 0.3 * (3.0 * ph).sin() + 0.15 * (4.0 * ph).sin()) * 0.2 * env;
            let j = i0 + i;
            if j < n {
                data[2 * j] = s;
                data[2 * j + 1] = s;
            }
        }
        t0 += beats * 0.5;
    }
    write_wav(path, &data, 44100).unwrap();
}

fn project(d: &Path) -> (Box<dyn wave_cli::Doc>, u64) {
    // C4 E4 G4 C5, a beat each, then G4 for two beats
    melody(&d.join("sung.wav"), &[(60.0, 1.0), (64.0, 1.0), (67.0, 1.0), (72.0, 1.0), (67.0, 2.0)], 0.0);
    let doc = Files.create("empty").unwrap();
    doc.call("apply", &json!({ "ops": [{ "op": "setBpm", "bpm": 120.0 }] }), None).unwrap();
    let r = doc.call("import", &json!({ "paths": [d.join("sung.wav").display().to_string()], "track": 0, "beat": 4 }), None).unwrap();
    (doc, r["clipIds"][0].as_u64().unwrap())
}

fn keys(v: &Value) -> Vec<u64> {
    v["notes"].as_array().unwrap().iter().map(|n| n["p"].as_u64().unwrap()).collect()
}

#[test]
fn a_sung_melody_becomes_a_pattern_under_the_clip_in_one_step() {
    let d = tmp("journey");
    let (doc, clip) = project(&d);
    let pv = doc.call("audio_to_midi", &json!({ "clipId": clip, "preview": true }), None).unwrap();
    assert_eq!(keys(&pv), [60, 64, 67, 72, 67], "{pv:#}");
    // from the clip's bar (beat 4), each note on its beat as sung
    assert_eq!(pv["start"], 4.0);
    let steps: Vec<f64> = pv["notes"].as_array().unwrap().iter().map(|n| n["step"].as_f64().unwrap()).collect();
    for (k, s) in steps.iter().enumerate() {
        assert!((s - 4.0 * k as f64).abs() < 0.5, "note {k} at step {s}");
    }
    assert_eq!(pv["takeRange"], json!([60, 72]));
    // a preview changes nothing
    let rev0 = doc.revision();
    assert_eq!(doc.revision(), rev0);

    let r = doc.call("audio_to_midi", &json!({ "clipId": clip, "quantize": 4 }), None).unwrap();
    assert_eq!(doc.revision(), rev0 + 1, "channel, pattern and clip in one edit");
    assert_eq!(r["newChannel"], true);
    assert_eq!(r["count"], 5);
    let p = doc.call("context", &json!({ "full": true }), None).unwrap();
    let proj = &p["project"];
    let pat = proj["patterns"].as_array().unwrap().iter().find(|x| x["id"] == r["patternId"]).unwrap();
    assert_eq!(pat["name"], "Melody – sung");
    let notes = pat["notes"].as_array().unwrap();
    // quantized to beats: on 0, 4, 8, 12, 16
    let got: Vec<(f64, u64)> = notes.iter().map(|n| (n["step"].as_f64().unwrap(), n["p"].as_u64().unwrap())).collect();
    assert_eq!(got, [(0.0, 60), (4.0, 64), (8.0, 67), (12.0, 72), (16.0, 67)]);
    assert!(notes.iter().all(|n| n["ch"] == r["channelId"]));
    assert_eq!(r["clipId"], clip);
    let pc = proj["clips"].as_array().unwrap().iter().find(|c| c["id"] == r["patternClipId"]).unwrap();
    assert_eq!(pc["start"], 4.0);
    assert_ne!(pc["track"], 0, "not on top of the audio");

    // onto the same channel, fixed velocity and a range that leaves the top C out
    let ch = r["channelId"].as_u64();
    let r2 = doc.call("audio_to_midi", &json!({ "clipId": clip, "channel": ch, "velocity": 0.7, "high": 71 }), None).unwrap();
    assert_eq!(r2["newChannel"], false);
    assert_eq!(keys(&r2), [60, 64, 67, 67]);
    assert!(r2["notes"].as_array().unwrap().iter().all(|n| (n["v"].as_f64().unwrap() - 0.7).abs() < 1e-6));
}

#[test]
fn refuses_what_it_cannot_convert() {
    let d = tmp("refuse");
    let (doc, clip) = project(&d);
    assert!(doc.call("audio_to_midi", &json!({}), None).unwrap_err().contains("clipId"));
    assert!(doc.call("audio_to_midi", &json!({ "clipId": clip, "mode": "drums" }), None).unwrap_err().contains("only `melody`"));
    assert!(doc.call("audio_to_midi", &json!({ "clipId": clip, "low": 80, "high": 60 }), None).unwrap_err().contains("above"));
    assert!(doc.call("audio_to_midi", &json!({ "clipId": clip, "low": 300 }), None).unwrap_err().contains("0–127"));
    assert!(doc.call("audio_to_midi", &json!({ "clipId": clip, "channel": 999_999 }), None).unwrap_err().contains("no channel"));
    // a range with nothing in it: the preview says so, Convert refuses and changes nothing
    let pv = doc.call("audio_to_midi", &json!({ "clipId": clip, "low": 100, "preview": true }), None).unwrap();
    assert_eq!(pv["count"], 0);
    let rev = doc.revision();
    assert!(doc.call("audio_to_midi", &json!({ "clipId": clip, "low": 100 }), None).unwrap_err().contains("no notes"));
    assert_eq!(doc.revision(), rev);
}

#[test]
fn a_transposed_clip_converts_to_the_keys_the_playlist_plays() {
    let d = tmp("transposed");
    let (doc, clip) = project(&d);
    let p = doc.call("context", &json!({ "full": true }), None).unwrap();
    let mut c = p["project"]["clips"].as_array().unwrap().iter().find(|x| x["id"] == clip).unwrap().clone();
    c["pitch"] = json!(3.0);
    doc.call("apply", &json!({ "ops": [{ "op": "putClips", "clips": [c] }] }), None).unwrap();
    let pv = doc.call("audio_to_midi", &json!({ "clipId": clip, "preview": true }), None).unwrap();
    assert_eq!(keys(&pv), [63, 67, 70, 75, 70], "{pv:#}");
    assert_eq!(pv["takeRange"], json!([63, 75]));
    let _ = std::fs::remove_dir_all(&d);
}
