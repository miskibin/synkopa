//! Stereo, Multiband and Analyzer as mixer effects: on an insert of a playing project they do
//! what their parameters say, their windows get their readings (and the X/Y points), they
//! survive a save and load, and a bounce is repeatable.

use std::sync::Arc;

use wave_engine::catalog::{load_preset, new_fx, presets};
use wave_engine::*;

const SR: u32 = 48000;
const BLOCK: usize = 256;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// A stereo tone (different in L and R) on track 2, playing from 0.
fn setup(tag: &str) -> (Arc<Engine>, Rt, u64) {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    let e = Engine::new(p, Arc::new(NoPlugins));
    let dir = std::env::temp_dir().join(format!("wave-smb-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let x: Vec<f32> = (0..SR as usize * 10)
        .flat_map(|i| {
            let t = i as f32 / SR as f32;
            let m = 0.25 * (std::f32::consts::TAU * 220.0 * t).sin();
            let s = 0.12 * (std::f32::consts::TAU * 1330.0 * t).sin();
            [m + s, m - s]
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

fn fx_of(kind: FxKind, id: u64, preset: &str) -> Fx {
    let mut f = new_fx(id, kind, 120.0);
    let i = presets(kind).iter().position(|p| p.name == preset).unwrap_or_else(|| panic!("no preset {preset}"));
    assert!(load_preset(&mut f, i, 120.0));
    f
}

fn side(l: &[f32], r: &[f32]) -> f32 {
    rms(&l.iter().zip(r).map(|(a, b)| 0.5 * (a - b)).collect::<Vec<_>>())
}

#[test]
fn stereo_on_an_insert_narrows_and_widens() {
    let (e, mut rt, ins) = setup("stereo");
    let (dl, dr) = run(&mut rt, SR as usize / 2);
    let s0 = side(&dl, &dr);
    assert!(s0 > 0.01);
    let id = e.project().next_id;
    e.apply(&[Op::PutFx { insert_id: ins, fx: fx_of(FxKind::Stereo, id, "Mono"), index: None }], false, None).unwrap();
    run(&mut rt, SR as usize / 4);
    let (l, r) = run(&mut rt, SR as usize / 2);
    assert!(side(&l, &r) < 1e-3 * s0, "mono: side {}", side(&l, &r));
    let mut f = e.project().find_fx(id).unwrap().1.clone();
    f.params.insert("width".into(), 200.0);
    e.apply(&[Op::PutFx { insert_id: ins, fx: f, index: None }], false, None).unwrap();
    run(&mut rt, SR as usize / 4);
    let (l, r) = run(&mut rt, SR as usize / 2);
    let ratio = side(&l, &r) / s0;
    assert!((ratio - 2.0).abs() < 0.1, "200 % doubles the side: {ratio}");
}

#[test]
fn multiband_and_analyzer_report_to_their_windows() {
    let (e, mut rt, ins) = setup("meters");
    let id = e.project().next_id;
    e.apply(
        &[
            Op::PutFx { insert_id: ins, fx: fx_of(FxKind::Multiband, id, "Klasyczny OTT (100 %)"), index: None },
            Op::PutFx { insert_id: ins, fx: new_fx(id + 1, FxKind::Analyzer, 120.0), index: None },
        ],
        false,
        None,
    )
    .unwrap();
    e.watch_fx(&[id, id + 1]);
    let (l, r) = run(&mut rt, SR as usize * 4);
    assert!(l.iter().chain(&r).all(|v| v.is_finite()) && rms(&l) > 0.01);
    let shared = rt_shared(&e);
    // the multiband's mid band (220 Hz and 1.33 kHz) is well above its downward threshold
    let m = shared.fx_meters(0);
    assert!(m[4] < -3.0, "mid band pulled down: {} dB", m[4]);
    // the analyzer measured the song so far
    let a = shared.fx_meters(1);
    assert!(a[2] > -40.0 && a[2] < 0.0, "integrated {}", a[2]);
    assert!(a[8] > 3.0, "seconds measured {}", a[8]);
}

/// The engine's shared state, as the analysis thread reads it.
fn rt_shared(e: &Engine) -> Arc<wave_engine::rt::Shared> {
    e.shared()
}

#[test]
fn the_three_survive_a_save_and_load_and_bounce_repeatably() {
    let (e, _rt, ins) = setup("save");
    let id = e.project().next_id;
    e.apply(
        &[
            Op::PutFx { insert_id: ins, fx: fx_of(FxKind::Stereo, id, "Mono → stereo (spread)"), index: None },
            Op::PutFx { insert_id: ins, fx: fx_of(FxKind::Multiband, id + 1, "Hi-haty — chrupko"), index: None },
            Op::PutFx { insert_id: ins, fx: new_fx(id + 2, FxKind::Analyzer, 120.0), index: None },
        ],
        false,
        None,
    )
    .unwrap();
    let p = e.project();
    let json = serde_json::to_string(&p).unwrap();
    for t in ["stereo", "multiband", "analyzer"] {
        assert!(json.contains(&format!("\"type\":\"{t}\"")), "{t}");
    }
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.insert(ins).unwrap().fx, p.insert(ins).unwrap().fx);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let a = e.render_export(None, 0.0, 8.0, RenderTail::Seconds(0.5), SR, &cancel).unwrap();
    assert!(a.iter().all(|v| v.is_finite()) && rms(&a) > 0.01);
    let b = e.render_export(None, 0.0, 8.0, RenderTail::Seconds(0.5), SR, &cancel).unwrap();
    assert_eq!(a, b);
}
