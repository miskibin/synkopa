//! The agent's Auto Vocal journey through the same actions the app answers: a project file, a
//! sung take, the rider baked into the audio and the calibrated chain on its insert — plus the
//! RMVPE install surfacing a bad model file instead of downloading it.

use std::path::{Path, PathBuf};
use std::sync::Once;

use serde_json::{json, Value};
use wave_cli::Files as _;
use wave_engine::singer::{sing, song, Voice};
use wave_engine::sources::write_wav;

use crate::headless::Files;

const SR: f32 = 44100.0;

/// The one model path the tests of this crate share, holding junk bytes: activation fails and
/// the detector stays YIN — the same outcome as the missing model `pitch_actions_tests`
/// arranges (it points this very path at nothing) — and `f0_model_install` finds a file, so no
/// test here ever downloads the real model.
fn no_model() {
    static ONCE: Once = Once::new();
    let p = std::env::temp_dir().join("wave-no-such-model.onnx");
    ONCE.call_once(|| {
        std::fs::write(&p, b"junk bytes, not a model").expect("write the junk model fixture");
        std::env::set_var("WAVE_RMVPE_MODEL", &p);
    });
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-autovocal-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn wav(path: &Path, mono: &[f32]) {
    let st: Vec<f32> = mono.iter().flat_map(|&x| [x, x]).collect();
    write_wav(path, &st, SR as u32).unwrap();
}

/// A project with a sung take and a second voice as the beat's key source, both on the
/// master-routed lanes of an empty project: (doc, take clip, beat clip).
fn project(d: &Path) -> (Box<dyn wave_cli::Doc>, u64, u64) {
    no_model();
    let s = song(41, 10);
    wav(&d.join("take.wav"), &sing(&s, &Voice::default(), &[], 0.0, SR).audio);
    let beat = Voice { seed: 5, transpose: -12.0, formant: 0.9, ..Voice::default() };
    wav(&d.join("beat.wav"), &sing(&s, &beat, &[], 0.0, SR).audio);
    let doc = Files.create("empty").unwrap();
    let paths = [d.join("take.wav").display().to_string(), d.join("beat.wav").display().to_string()];
    let r = doc.call("import", &json!({ "paths": paths, "track": 0, "beat": 0 }), None).unwrap();
    let clip = r["clipIds"][0].as_u64().unwrap();
    let beat_clip = r["clipIds"][1].as_u64().unwrap();
    (doc, clip, beat_clip)
}

fn clip_json(doc: &dyn wave_cli::Doc, id: u64) -> Value {
    let ctx = doc.call("context", &json!({ "full": true }), None).unwrap();
    let clips = ctx["clips"].as_array().or_else(|| ctx["project"]["clips"].as_array()).expect("context --full lists clips");
    clips.iter().find(|c| c["id"] == id).cloned().unwrap()
}

fn project_json(doc: &dyn wave_cli::Doc) -> Value {
    let ctx = doc.call("context", &json!({ "full": true }), None).unwrap();
    ctx.get("project").cloned().unwrap_or(ctx)
}

/// The audio clip's own insert is what Auto Vocal builds on (a track of an empty project plays
/// into the master, which Auto vocal refuses, as the app does until the user assigns one).
fn route_to_insert(doc: &dyn wave_cli::Doc, clip: u64, insert: u64) {
    let mut c = clip_json(doc, clip);
    c["insert"] = json!(insert);
    doc.call("apply", &json!({ "ops": [{ "op": "putClips", "clips": [c] }] }), None).unwrap();
}

fn insert_fx(doc: &dyn wave_cli::Doc, insert: u64) -> Vec<Value> {
    let p = project_json(doc);
    let inserts = p["inserts"].as_array().expect("context --full lists inserts");
    let m = inserts.iter().find(|m| m["id"] == insert).expect("the insert exists");
    m["fx"].as_array().cloned().unwrap()
}

fn sources(doc: &dyn wave_cli::Doc) -> Vec<Value> {
    project_json(doc)["sources"].as_array().cloned().unwrap()
}

fn baked_sources(doc: &dyn wave_cli::Doc) -> Vec<Value> {
    sources(doc).into_iter().filter(|s| s["name"].as_str().unwrap_or("").ends_with(" (rider)")).collect()
}

fn types(fx: &[Value]) -> Vec<&str> {
    fx.iter().map(|f| f["type"].as_str().unwrap()).collect()
}

#[test]
fn auto_vocal_bakes_the_rider_and_puts_the_chain_on_the_insert() {
    let d = tmp("journey");
    let (doc, clip, beat) = project(&d);
    let doc = doc.as_ref();
    route_to_insert(doc, clip, 1);
    doc.call("add_fx", &json!({ "insertId": 1, "type": "comp" }), None).unwrap();
    doc.call("add_fx", &json!({ "insertId": 1, "type": "eq" }), None).unwrap();

    let plan = doc.call("auto_vocal_plan", &json!({ "clipId": clip, "beatClipId": beat, "bakeRider": true }), None).unwrap();
    let chain: Vec<&str> = plan["chain"].as_array().unwrap().iter().map(|p| p["type"].as_str().unwrap()).collect();
    assert!(!chain.contains(&"rider"), "baked: the rider leaves the chain, not {chain:?}");
    assert_eq!(plan["riderBake"]["type"], "rider");
    assert!(plan["trackGainDb"].as_f64().is_some(), "with a beat clip the plan suggests a level");

    let rev0 = doc.revision();
    let r = doc.call("auto_vocal", &json!({ "clipId": clip, "beatClipId": beat }), Some(rev0)).unwrap();
    assert!(r["revision"].as_u64().unwrap() >= rev0 + 2, "the bake and the chain are two undo steps: {r}");
    assert_eq!(r["riderBaked"]["clipId"], clip);
    assert_eq!(r["fxIds"].as_array().unwrap().len(), chain.len());
    assert_eq!(r["warnings"].as_array().unwrap(), plan["warnings"].as_array().unwrap());
    assert_eq!(r["insertId"], 1);
    assert_eq!(r["style"], "natural");

    // the clip plays the baked render now, at 0 dB gain, and the original source stays
    let c = clip_json(doc, clip);
    assert_eq!(c["gain"], 0.0);
    let baked_name = sources(doc).iter().find(|s| s["id"] == c["ref"]).unwrap()["name"].clone();
    let baked_name = baked_name.as_str().unwrap().to_string();
    assert!(baked_name.ends_with(" (rider)"), "{baked_name}");
    assert!(sources(doc).iter().any(|s| s["id"] != c["ref"]), "the original source stays in the project");

    // the insert: the eq stays in front, the same-kind comp is replaced, the chain follows in order
    let fx = insert_fx(doc, 1);
    let mut want = vec!["eq"];
    want.extend(chain.iter().copied());
    assert_eq!(types(&fx), want, "eq kept, comp replaced by the chain");
    let limit = fx.last().unwrap();
    let gain = limit["params"]["gain"].as_f64().unwrap();
    assert!((0.0..=24.0).contains(&gain), "the level suggestion is folded into the limiter: {gain}");
    assert_eq!(limit["preset"], Value::Null, "calibrated values, not a preset");

    // a second call does not bake a second render, and only the chain step moves the revision
    let n = baked_sources(doc).len();
    let rev = doc.revision();
    let r2 = doc.call("auto_vocal", &json!({ "clipId": clip, "beatClipId": beat }), None).unwrap();
    assert!(r2["riderBaked"].is_null(), "{r2}");
    assert_eq!(baked_sources(doc).len(), n, "one baked render");
    assert_eq!(doc.revision(), rev + 1);
    assert_eq!(r2["fxIds"].as_array().unwrap().len(), chain.len(), "the chain is rebuilt (fresh stages)");
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn auto_vocal_without_bake_keeps_the_rider_live_and_the_audio_untouched() {
    let d = tmp("nobake");
    let (doc, clip, beat) = project(&d);
    let doc = doc.as_ref();
    route_to_insert(doc, clip, 1);
    let source0 = clip_json(doc, clip)["ref"].clone();

    let r = doc.call("auto_vocal", &json!({ "clipId": clip, "beatClipId": beat, "noBake": true }), None).unwrap();
    assert!(r["riderBaked"].is_null(), "{r}");
    let fx = insert_fx(doc, 1);
    let t = types(&fx);
    assert!(t.contains(&"rider"), "the rider runs live on the insert: {t:?}");
    assert_eq!(clip_json(doc, clip)["ref"], source0, "the clip still plays the original audio");
    assert!(baked_sources(doc).is_empty());
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn auto_vocal_refuses_clips_it_cannot_serve() {
    let d = tmp("refusals");
    let (doc, clip, _) = project(&d);
    let doc = doc.as_ref();
    // a lane of the empty project plays into the master: the app asks for an insert first
    let e = doc.call("auto_vocal", &json!({ "clipId": clip }), None).unwrap_err();
    assert!(e.contains("master insert"), "{e}");
    let e = doc.call("auto_vocal", &json!({ "clipId": clip + 999 }), None).unwrap_err();
    assert!(e.contains("no such clip"), "{e}");
    // a stale guard refuses the whole job, and nothing changed
    route_to_insert(doc, clip, 1);
    let rev = doc.revision();
    let e = doc.call("auto_vocal", &json!({ "clipId": clip }), Some(rev - 1)).unwrap_err();
    assert!(e.contains("REVISION_CONFLICT"), "{e}");
    assert!(insert_fx(doc, 1).is_empty(), "the refusal left the insert alone");
    assert_eq!(doc.revision(), rev);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn f0_model_install_surfaces_a_bad_model_file_instead_of_downloading() {
    no_model();
    let doc = Files.create("empty").unwrap();
    // the path `no_model` arranges holds junk bytes, so `install` finds a file (no download)
    // and `activate` refuses to load it: the error comes back and the detector stays YIN
    let e = doc.call("f0_model_install", &json!({}), None).unwrap_err();
    assert!(!e.is_empty(), "the load error is the reply, not a success");
    let st = doc.call("f0_model", &json!({}), None).unwrap();
    assert_eq!(st["backend"], "yin", "{st}");
}
