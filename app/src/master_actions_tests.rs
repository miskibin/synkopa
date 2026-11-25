//! The Master's whole-song listen through the same action the app and `synkopa master-listen`
//! answer: a project file with a song on the master, the effect added, listened to and set —
//! in one step, with its report — plus the refusals.

use std::path::PathBuf;

use serde_json::{json, Value};
use wave_cli::Files as _;
use wave_engine::sources::write_wav;

use crate::headless::Files;

const SR: f32 = 44100.0;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-master-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Eight seconds of a beat-ish signal: a decaying 55 Hz kick every half second over a quiet tone.
fn beat_wav(path: &std::path::Path) {
    let n = (8.0 * SR) as usize;
    let mut st = Vec::with_capacity(2 * n);
    for i in 0..n {
        let t = i as f32 / SR;
        let kick = (-(t * 2.0).fract() * 25.0).exp() * (std::f32::consts::TAU * 55.0 * t).sin() * 0.6;
        let tone = (std::f32::consts::TAU * 440.0 * t).sin() * 0.05;
        st.push(kick + tone);
        st.push(kick + tone * 0.8);
    }
    write_wav(path, &st, SR as u32).unwrap();
}

fn master_fx(doc: &dyn wave_cli::Doc) -> Vec<Value> {
    let ctx = doc.call("context", &json!({ "full": true }), None).unwrap();
    let p = ctx.get("project").cloned().unwrap_or(ctx);
    let m = p["inserts"].as_array().unwrap().iter().find(|m| m["id"] == 0).cloned().unwrap();
    m["fx"].as_array().cloned().unwrap()
}

#[test]
fn master_listen_sets_the_effect_and_stores_its_report_in_one_step() {
    let d = tmp("listen");
    beat_wav(&d.join("beat.wav"));
    let doc = Files.create("empty").unwrap();
    doc.call("import", &json!({ "paths": [d.join("beat.wav").display().to_string()], "track": 0, "beat": 0 }), None).unwrap();
    let added = doc.call("add_fx", &json!({ "insertId": 0, "type": "master" }), None).unwrap();
    let fx = added["fxId"].as_u64().unwrap();
    let r = doc.call("master_listen", &json!({ "fxId": fx, "target": 1 }), None).unwrap();
    let rep = &r["listen"];
    assert!((rep["outLufs"].as_f64().unwrap() + 14.0).abs() <= 0.3, "{rep}");
    assert!(rep["outTp"].as_f64().unwrap() <= -0.9, "{rep}");
    assert!(rep["seconds"].as_f64().unwrap() >= 7.0, "{rep}");
    let f = master_fx(&*doc).into_iter().find(|f| f["id"] == fx).unwrap();
    assert_eq!(f["params"]["target"], 1.0);
    assert!(f["listen"].is_object(), "the report is stored on the effect");
    assert!(f["params"]["drive"].as_f64().unwrap() > 0.0);
    // one step: the listen moved the project exactly one revision past the effect's addition
    assert_eq!(r["revision"].as_u64().unwrap(), added["revision"].as_u64().unwrap() + 1);
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn master_listen_refuses_what_it_cannot_serve() {
    let doc = Files.create("empty").unwrap();
    let added = doc.call("add_fx", &json!({ "insertId": 0, "type": "master" }), None).unwrap();
    let fx = added["fxId"].as_u64().unwrap();
    // an empty song
    let e = doc.call("master_listen", &json!({ "fxId": fx }), None).unwrap_err().to_string();
    assert!(e.contains("empty") || e.contains("silent"), "{e}");
    // not a Master, no such effect, a target out of range
    let lim = doc.call("add_fx", &json!({ "insertId": 0, "type": "limit" }), None).unwrap()["fxId"].as_u64().unwrap();
    let e = doc.call("master_listen", &json!({ "fxId": lim }), None).unwrap_err().to_string();
    assert!(e.contains("not a Master"), "{e}");
    let e = doc.call("master_listen", &json!({ "fxId": 999_999 }), None).unwrap_err().to_string();
    assert!(e.contains("no effect"), "{e}");
    let e = doc.call("master_listen", &json!({ "fxId": fx, "target": 9 }), None).unwrap_err().to_string();
    assert!(e.contains("target"), "{e}");
}
