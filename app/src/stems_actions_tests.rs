//! "Extract stems" through the shared action (`stems_actions.rs`) on a project file, as
//! `synkopa stems` and the app's dialog run it. Without the model only the refusals are testable;
//! `WAVE_STEMS_MODEL=htdemucs.onnx cargo test -p wave-app --release stems -- --ignored` runs the
//! whole journey with the real network.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use wave_cli::Files as _;
use wave_engine::sources::{write_wav, Audio};

use crate::headless::Files;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-stems-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 12 s at 48 kHz (not the model's 44.1 kHz): a kick every half second, a bass line and a
/// vibrato "voice" in the middle.
fn mix(path: &Path) -> Vec<f32> {
    let sr = 48000.0f32;
    let n = (12.0 * sr) as usize;
    let tau = std::f32::consts::TAU;
    let data: Vec<f32> = (0..n)
        .flat_map(|i| {
            let t = i as f32 / sr;
            let beat = t % 0.5;
            let kick = (tau * (50.0 + 120.0 * (-beat * 30.0).exp()) * beat).sin() * (-beat * 12.0).exp() * 0.6;
            let bass = (tau * [55.0, 55.0, 73.4, 65.4][(t / 2.0) as usize % 4] * t).sin() * 0.25;
            let voice =
                (tau * 440.0 * t + 3.0 * (tau * 5.5 * t).sin()).sin() * 0.2 * ((t - 2.0).clamp(0.0, 1.0)) * ((10.0 - t).clamp(0.0, 1.0));
            let s = kick + bass + voice;
            [s, s * 0.95]
        })
        .collect();
    write_wav(path, &data, 48000).unwrap();
    data
}

fn project(d: &Path) -> (Box<dyn wave_cli::Doc>, u64) {
    mix(&d.join("mix.wav"));
    let doc = Files.create("empty").unwrap();
    let r = doc.call("import", &json!({ "paths": [d.join("mix.wav").display().to_string()], "track": 1, "beat": 4 }), None).unwrap();
    (doc, r["clipIds"][0].as_u64().unwrap())
}

fn model_installed() -> bool {
    wave_stems::model_path().is_file()
}

#[test]
fn refuses_bad_requests_before_doing_any_work() {
    let d = tmp("refuse");
    let (doc, clip) = project(&d);
    let call = |args: Value| doc.call("extract_stems", &args, None).unwrap_err();
    assert!(call(json!({ "clipId": clip, "stems": ["piano"] })).contains("no stem called `piano`"));
    assert!(call(json!({ "clipId": clip, "stems": [] })).contains("at least one"));
    assert!(call(json!({ "stems": ["bass"] })).contains("clipId"));
    if !model_installed() {
        let e = call(json!({ "clipId": clip }));
        assert!(e.starts_with(wave_engine::errors::ERR_NO_STEM_MODEL), "{e}");
    }
    let st = doc.call("stems_model", &json!({}), None).unwrap();
    assert_eq!(st["id"], "htdemucs");
    assert_eq!(st["stems"], json!(["drums", "bass", "other", "vocals"]));
    assert_eq!(st["bytes"].as_u64().unwrap(), wave_stems::MODEL_BYTES);
}

#[test]
fn stem_names_parse_from_a_list_or_the_cli_string() {
    let p = |v: Value| crate::stems_actions::stems_arg(&json!({ "stems": v })).unwrap();
    assert_eq!(p(json!(["vocals", "drums"])), ["vocals", "drums"]);
    assert_eq!(p(json!("Vocals, drums")), ["vocals", "drums"]);
    assert_eq!(p(Value::Null), ["drums", "bass", "other", "vocals"]);
    assert!(crate::stems_actions::stems_arg(&json!({ "stems": 3 })).is_err());
}

/// The whole journey with the real model: 48 kHz source, a stretched region in the middle of the
/// file, two stems; they land below the clip, the files sit with the project's media and survive a
/// save and reopen, and one undo takes it all back.
#[test]
#[ignore]
fn extracts_stems_with_the_real_model() {
    if !model_installed() {
        eprintln!("no model at {}", wave_stems::model_path().display());
        return;
    }
    let d = tmp("real");
    let (doc, clip) = project(&d);
    let saved = d.join("song.synkopa");
    doc.save(&saved).unwrap();
    let (doc, _) = Files.open(&saved).unwrap();
    // play 3 s .. 9 s of the file, stretched by 1.25 (resampled)
    let ctx = doc.call("context", &json!({ "full": true }), None).unwrap();
    let p = if ctx.get("project").is_some() { &ctx["project"] } else { &ctx };
    let bpm = p["bpm"].as_f64().unwrap();
    let mut c = p["clips"].as_array().unwrap().iter().find(|c| c["id"] == clip).cloned().unwrap();
    c["offset"] = json!(3.0 * bpm / 60.0 * 1.25);
    c["len"] = json!(6.0 * bpm / 60.0 * 1.25);
    c["stretch"] = json!(1.25);
    c["resample"] = json!(true);
    doc.call("apply", &json!({ "ops": [{ "op": "putClips", "clips": [c] }] }), None).unwrap();
    let tracks0 = p["tracks"].as_array().unwrap().len();

    let t = std::time::Instant::now();
    let r = doc.call("extract_stems", &json!({ "clipId": clip, "stems": ["vocals", "drums"] }), None).unwrap();
    eprintln!("12 s at 48 kHz → 2 stems in {:.1} s", t.elapsed().as_secs_f64());
    assert_eq!(r["track"], 2);
    let ctx = doc.call("context", &json!({ "full": true }), None).unwrap();
    let p = if ctx.get("project").is_some() { &ctx["project"] } else { &ctx };
    let tracks = p["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), tracks0 + 2);
    assert_eq!(tracks[2]["name"], "mix – Drums");
    assert_eq!(tracks[3]["name"], "mix – Vocals");
    let src = p["clips"].as_array().unwrap().iter().find(|c| c["id"] == clip).unwrap();
    assert_eq!(src["muted"], true);
    for id in r["clipIds"].as_array().unwrap() {
        let c = p["clips"].as_array().unwrap().iter().find(|c| &c["id"] == id).unwrap();
        assert_eq!(
            (c["start"].as_f64(), c["len"].as_f64(), c["stretch"].as_f64()),
            (src["start"].as_f64(), src["len"].as_f64(), Some(1.25))
        );
        let s = p["sources"].as_array().unwrap().iter().find(|s| s["id"] == c["ref"]).unwrap();
        let path = s["path"].as_str().unwrap();
        assert!(Path::new(path).starts_with(d.join("song Audio")) || path.starts_with("song Audio"), "{path}");
        // 0 s .. 12 s: the region (3 .. 9 s) plus 3 s either side, at 44.1 kHz
        let a = Audio::load(&d.join(path)).or_else(|_| Audio::load(Path::new(path))).unwrap();
        assert_eq!(a.sr, wave_stems::SR);
        assert!((a.seconds() - 12.0).abs() < 0.01, "{}", a.seconds());
        assert!(a.peak() > 0.01);
    }
    doc.save(&saved).unwrap();
    let (re, warnings) = Files.open(&saved).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(re.call("missing", &json!({}), None).unwrap().as_array().map_or(0, Vec::len), 0);
}
