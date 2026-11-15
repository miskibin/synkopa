//! Chorus and Phaser as mixer effects: they run on an insert of a playing project, change the
//! sound as their parameters say, survive a project save and load, and bounce offline.

use std::sync::Arc;

use wave_engine::catalog::{load_preset, new_fx, presets};
use wave_engine::*;

const SR: u32 = 48000;
const BLOCK: usize = 256;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// A steady tone on track 2 of a project without effects, playing from 0.
fn setup(tag: &str) -> (Arc<Engine>, Rt, u64) {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    let e = Engine::new(p, Arc::new(NoPlugins));
    let dir = std::env::temp_dir().join(format!("wave-mod-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let x: Vec<f32> = (0..SR as usize * 2 * 10).map(|i| 0.3 * ((i / 2) as f32 * 0.03).sin()).collect();
    sources::write_wav(&wav, &x, SR).unwrap();
    e.import_audio(std::slice::from_ref(&wav), Some((2, 0.0))).unwrap();
    let rt = e.create_rt(SR as f32, BLOCK);
    e.play(0.0);
    let ins = e.project().tracks[2].insert;
    (e, rt, ins)
}

/// `n` frames of the master, left and right.
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

#[test]
fn a_chorus_on_an_insert_turns_a_mono_tone_stereo() {
    let (e, mut rt, ins) = setup("chorus");
    let (dl, dr) = run(&mut rt, SR as usize / 2);
    assert!(rms(&dl) > 0.01);
    let side = |l: &[f32], r: &[f32]| rms(&l.iter().zip(r).map(|(a, b)| a - b).collect::<Vec<_>>());
    assert!(side(&dl, &dr) < 1e-4, "the dry tone is mono");
    let id = e.project().next_id;
    e.apply(&[Op::PutFx { insert_id: ins, fx: fx_of(FxKind::Chorus, id, "Szeroki syntezator"), index: None }], false, None).unwrap();
    run(&mut rt, SR as usize / 4);
    let (l, r) = run(&mut rt, SR as usize);
    assert!(l.iter().chain(&r).all(|v| v.is_finite()));
    assert!(side(&l, &r) > 0.05 * rms(&l), "the chorus makes it stereo: side {} vs {}", side(&l, &r), rms(&l));
    // a parameter edit reaches the running effect: no depth and no width = mono again
    let mut f = e.project().find_fx(id).unwrap().1.clone();
    f.params.insert("width".into(), 0.0);
    e.apply(&[Op::PutFx { insert_id: ins, fx: f, index: None }], false, None).unwrap();
    run(&mut rt, SR as usize / 4);
    let (l, r) = run(&mut rt, SR as usize / 2);
    assert!(side(&l, &r) < 1e-3 * rms(&l).max(1e-6) + 1e-5, "width 0 sums the copies to mono: side {}", side(&l, &r));
}

#[test]
fn both_effects_survive_a_save_and_load_and_bounce_offline() {
    let (e, _rt, ins) = setup("save");
    let id = e.project().next_id;
    e.apply(
        &[
            Op::PutFx { insert_id: ins, fx: fx_of(FxKind::Chorus, id, "Vintage — dwa BBD"), index: None },
            Op::PutFx { insert_id: ins, fx: fx_of(FxKind::Phaser, id + 1, "Funk — gitara (1/8)"), index: None },
        ],
        false,
        None,
    )
    .unwrap();
    let p = e.project();
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"type\":\"chorus\"") && json.contains("\"type\":\"phaser\""));
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.insert(ins).unwrap().fx, p.insert(ins).unwrap().fx);

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let wet = e.render_export(None, 0.0, 8.0, RenderTail::Seconds(1.0), SR, &cancel).unwrap();
    assert!(wet.iter().all(|v| v.is_finite()) && rms(&wet) > 0.01);
    // and a second bounce is the same: nothing random, no state carried over
    let again = e.render_export(None, 0.0, 8.0, RenderTail::Seconds(1.0), SR, &cancel).unwrap();
    assert_eq!(wet, again);
}
