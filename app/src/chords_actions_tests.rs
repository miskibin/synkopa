//! Chords through the shared actions (`chords_actions.rs`) on a project file, as `synkopa chords`
//! and the tempo and key dialog run them. Without the model only the refusals are testable;
//! `WAVE_CHORDS_MODEL=chordnet.onnx cargo test -p wave-app --release chords -- --ignored` runs
//! the whole journey with the real network.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use wave_cli::Files as _;
use wave_engine::sources::write_wav;

use crate::headless::Files;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-chords-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A progression played by a plucked, harmonic-rich "keyboard" re-struck every beat at 120 BPM,
/// each chord one bar, the root an octave lower in the bass; `lead` seconds of silence first.
fn progression(path: &Path, chords: &[(u8, &[u8])], lead: f32) {
    let sr = 44100.0f32;
    let bar = 2.0f32;
    let n = ((lead + bar * chords.len() as f32 + 0.5) * sr) as usize;
    let tau = std::f32::consts::TAU;
    let hz = |k: f32| 440.0 * 2f32.powf((k - 69.0) / 12.0);
    let mut data = vec![0f32; n * 2];
    for (ci, (root, tones)) in chords.iter().enumerate() {
        let mut keys: Vec<f32> = tones.iter().map(|t| 60.0 + *root as f32 + *t as f32).collect();
        keys.push(36.0 + *root as f32 + if *root < 4 { 12.0 } else { 0.0 });
        for beat in 0..4 {
            let t0 = lead + ci as f32 * bar + beat as f32 * 0.5;
            let i0 = (t0 * sr) as usize;
            for i in 0..(0.5 * sr) as usize {
                let t = i as f32 / sr;
                let env = (-t * 4.0).exp() * (t * 400.0).min(1.0);
                let mut s = 0.0;
                for &k in &keys {
                    for h in 1..8 {
                        s += (tau * hz(k) * h as f32 * t).sin() / (h * h) as f32;
                    }
                }
                let j = i0 + i;
                if j < n {
                    data[2 * j] += s * env * 0.08;
                    data[2 * j + 1] += s * env * 0.08;
                }
            }
        }
    }
    write_wav(path, &data, 44100).unwrap();
}

const MAJ: &[u8] = &[0, 4, 7];
const MIN: &[u8] = &[0, 3, 7];

fn project(d: &Path, lead: f32) -> (Box<dyn wave_cli::Doc>, u64) {
    // C – Am – F – G, twice
    let prog: Vec<(u8, &[u8])> = [(0, MAJ), (9, MIN), (5, MAJ), (7, MAJ)].iter().cycle().take(8).copied().collect();
    progression(&d.join("keys.wav"), &prog, lead);
    let doc = Files.create("empty").unwrap();
    doc.call("apply", &json!({ "ops": [{ "op": "setBpm", "bpm": 120.0 }] }), None).unwrap();
    let r = doc.call("import", &json!({ "paths": [d.join("keys.wav").display().to_string()], "track": 0, "beat": 4 }), None).unwrap();
    (doc, r["clipIds"][0].as_u64().unwrap())
}

fn model_installed() -> bool {
    wave_chords::model_path().is_file()
}

#[test]
fn refuses_bad_requests_and_reports_the_model() {
    let d = tmp("refuse");
    let (doc, clip) = project(&d, 0.0);
    let st = doc.call("chords_model", &json!({}), None).unwrap();
    assert_eq!(st["id"], "chordmini");
    assert_eq!(st["bytes"].as_u64().unwrap(), wave_chords::MODEL_BYTES);
    assert!(doc.call("chords_analyze", &json!({}), None).unwrap_err().contains("clipId"));
    assert!(doc.call("chords_analyze", &json!({ "clipId": clip, "detail": "jazz" }), None).unwrap_err().contains("full or simple"));
    if !model_installed() {
        let e = doc.call("chords_analyze", &json!({ "clipId": clip }), None).unwrap_err();
        assert!(e.starts_with(wave_engine::errors::ERR_NO_CHORD_MODEL), "{e}");
        let e = doc.call("chords_to_pattern", &json!({ "clipId": clip }), None).unwrap_err();
        assert!(e.starts_with(wave_engine::errors::ERR_NO_CHORD_MODEL), "{e}");
    }
}

fn names(v: &Value) -> Vec<String> {
    v["segments"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap().to_string()).collect()
}

#[test]
#[ignore = "needs the chord model (WAVE_CHORDS_MODEL)"]
fn detects_a_progression_and_writes_it_to_a_pattern() {
    assert!(model_installed(), "set WAVE_CHORDS_MODEL to chordnet.onnx");
    let d = tmp("journey");
    let (doc, clip) = project(&d, 0.0);
    let an = doc.call("chords_analyze", &json!({ "clipId": clip }), None).unwrap();
    eprintln!("{an:#}");
    let got = names(&an);
    assert_eq!(got, ["C", "Am", "F", "G", "C", "Am", "F", "G"], "{got:?}");
    // each chord a bar, from the clip's start at beat 4
    for (i, s) in an["segments"].as_array().unwrap().iter().enumerate() {
        assert_eq!(s["start"].as_f64().unwrap(), 4.0 + 4.0 * i as f64, "{s}");
        assert!(s["confidence"].as_f64().unwrap() > 0.3, "{s}");
    }
    assert_eq!(an["flats"], false); // C major

    // bars 2..4 of the clip (Am F G) to a new pattern on a new piano channel
    let rev0 = doc.revision();
    let r = doc.call("chords_to_pattern", &json!({ "clipId": clip, "from": 8.0, "to": 20.0 }), None).unwrap();
    assert_eq!(r["chords"], 3);
    assert_eq!(r["newChannel"], true);
    assert_eq!(r["start"], 8.0);
    let p = doc.call("context", &json!({ "full": true }), None).unwrap();
    let proj = &p["project"];
    let pat = proj["patterns"].as_array().unwrap().iter().find(|x| x["id"] == r["patternId"]).unwrap();
    assert_eq!(pat["len"], 12.0);
    let notes = pat["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 12);
    let ch = proj["channels"].as_array().unwrap().iter().find(|c| c["id"] == r["channelId"]).unwrap();
    assert_eq!(ch["kind"], "keys");
    let pc = proj["clips"].as_array().unwrap().iter().find(|c| c["id"] == r["clipId"]).unwrap();
    assert_eq!(pc["start"], 8.0);
    assert_ne!(pc["track"], 0, "not on top of the audio");
    // channel, pattern and clip in one edit (one undo step in the app)
    assert_eq!(doc.revision(), rev0 + 1);

    // onto an existing channel, simplified, in a second pattern with its own name
    let ch = r["channelId"].as_u64();
    let r2 = doc.call("chords_to_pattern", &json!({ "clipId": clip, "channel": ch, "detail": "simple" }), None).unwrap();
    assert_eq!(r2["newChannel"], false);
    assert_eq!(r2["chords"], 8);
    let p = doc.call("context", &json!({ "full": true }), None).unwrap();
    let names: Vec<&str> = p["project"]["patterns"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Chords – keys") && names.contains(&"Chords – keys 2"), "{names:?}");
}

#[test]
#[ignore = "needs the chord model (WAVE_CHORDS_MODEL)"]
fn a_transposed_clip_reads_in_its_new_key() {
    assert!(model_installed(), "set WAVE_CHORDS_MODEL to chordnet.onnx");
    let d = tmp("transpose");
    let (doc, clip) = project(&d, 0.0);
    let p = doc.call("context", &json!({ "full": true }), None).unwrap();
    let mut c = p["project"]["clips"].as_array().unwrap().iter().find(|c| c["id"] == clip).unwrap().clone();
    c["pitch"] = json!(2.0);
    doc.call("apply", &json!({ "ops": [{ "op": "putClips", "clips": [c] }] }), None).unwrap();
    let an = doc.call("chords_analyze", &json!({ "clipId": clip }), None).unwrap();
    assert_eq!(an["transpose"], 2);
    assert_eq!(&names(&an)[..4], ["D", "Bm", "G", "A"]);
}
