//! Patina as a mixer effect: on an insert of a playing project it colours the sound, its noise
//! keeps sounding when the song is silent (the chain is not parked under it), its window gets its
//! readings, its latency is compensated, and it survives a save and load.

use std::sync::Arc;

use wave_engine::catalog::{latency, load_preset, new_fx, presets};
use wave_engine::*;

const SR: u32 = 48000;
const BLOCK: usize = 256;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// A tone on track 2 (2 s, then silence), playing from 0.
fn setup(tag: &str) -> (Arc<Engine>, Rt, u64) {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    let e = Engine::new(p, Arc::new(NoPlugins));
    let dir = std::env::temp_dir().join(format!("wave-patina-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let x: Vec<f32> = (0..SR as usize * 2)
        .flat_map(|i| {
            let s = 0.3 * (std::f32::consts::TAU * 330.0 * i as f32 / SR as f32).sin();
            [s, s]
        })
        .collect();
    sources::write_wav(&wav, &x, SR).unwrap();
    e.import_audio(std::slice::from_ref(&wav), Some((2, 0.0))).unwrap();
    let rt = e.create_rt(SR as f32, BLOCK);
    e.play(0.0);
    let ins = e.project().tracks[2].insert;
    (e, rt, ins)
}

fn run(rt: &mut Rt, n: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let (mut ol, mut or) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for _ in 0..n / BLOCK {
        rt.process(&mut l, &mut r, None);
        ol.extend_from_slice(&l);
        or.extend_from_slice(&r);
    }
    (ol, or)
}

fn patina(id: u64, preset: &str) -> Fx {
    let mut f = new_fx(id, FxKind::Patina, 120.0);
    let i = presets(FxKind::Patina).iter().position(|p| p.name == preset).unwrap_or_else(|| panic!("no preset {preset}"));
    assert!(load_preset(&mut f, i, 120.0));
    f
}

#[test]
fn a_new_patina_starts_on_its_first_preset_and_reports_its_latency() {
    let f = new_fx(1, FxKind::Patina, 120.0);
    assert_eq!(f.preset, Some(0));
    assert!(f.params["noise"] > 0.0);
    let lat = latency(&f, SR as f32);
    assert!((1..16).contains(&lat), "latency {lat}");
    assert_eq!(lat, wave_engine::dsp::patina::latency_samples());
}

#[test]
fn its_noise_plays_on_through_silence_and_its_window_gets_readings() {
    let (e, mut rt, ins) = setup("noise");
    let id = e.project().next_id;
    let mut f = patina(id, "Tylko szum winylu");
    f.params.insert("nfollow".into(), 0.0);
    e.apply(&[Op::PutFx { insert_id: ins, fx: f, index: None }], false, None).unwrap();
    e.watch_fx(&[id]);
    let (l, r) = run(&mut rt, SR as usize * 4);
    assert!(l.iter().chain(&r).all(|v| v.is_finite()));
    // a second after the tone (the song loops back at 3.5 s), the record keeps crackling
    let tail = &l[SR as usize * 3..SR as usize * 34 / 10];
    assert!(rms(tail) > 1e-4, "noise after the song: {}", rms(tail));
    let m = e.shared().fx_meters(0);
    assert!(m[wave_engine::dsp::patina::meter::CLICKS] > 10.0, "clicks counted: {}", m[16]);
    assert!(m[wave_engine::dsp::patina::meter::EFF] > 0.0, "noise amount reported");
}

#[test]
fn follow_lets_the_noise_go_when_the_song_stops() {
    let (e, mut rt, ins) = setup("follow");
    let id = e.project().next_id;
    let mut f = patina(id, "Tylko szum winylu");
    f.params.insert("nfollow".into(), 100.0);
    e.apply(&[Op::PutFx { insert_id: ins, fx: f, index: None }], false, None).unwrap();
    let (l, _) = run(&mut rt, SR as usize * 4);
    assert!(rms(&l[SR as usize / 2..SR as usize * 2]) > 0.05, "the tone plays");
    // a second after the tone (the song loops back at 3.5 s)
    let quiet = &l[SR as usize * 3..SR as usize * 34 / 10];
    assert!(rms(quiet) < 1e-5, "silence after it: {}", rms(quiet));
}

#[test]
fn every_preset_plays_cleanly_and_survives_save_and_load() {
    let (e, mut rt, ins) = setup("presets");
    let id = e.project().next_id;
    for (i, p) in presets(FxKind::Patina).iter().enumerate() {
        let f = patina(id, p.name);
        e.apply(&[Op::PutFx { insert_id: ins, fx: f.clone(), index: None }], false, None).unwrap();
        let (l, r) = run(&mut rt, SR as usize / 2);
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 4.0), "preset {i} {}", p.name);
        let json = serde_json::to_string(&e.project()).unwrap();
        assert!(json.contains("\"type\":\"patina\""));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.insert(ins).unwrap().fx, e.project().insert(ins).unwrap().fx, "{}", p.name);
    }
}
