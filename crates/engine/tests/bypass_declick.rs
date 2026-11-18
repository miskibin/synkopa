//! Bypassing an effect with a tail must declick: the wet contribution ramps out over ~5 ms
//! (and back in on re-enable) — it must not cut a ringing reverb in one sample.

use std::sync::Arc;

use wave_engine::catalog::new_fx;
use wave_engine::*;

const SR: u32 = 48000;
const BLOCK: usize = 256;

/// Loudest jump between neighbouring samples.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

/// A reverb on track 1, a steady tone playing through it.
fn setup(tag: &str) -> (Arc<Engine>, Rt) {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    let ins = p.tracks[1].insert;
    p.inserts.iter_mut().find(|m| m.id == ins).unwrap().fx.push(new_fx(7000, FxKind::Reverb, p.bpm));
    let e = Engine::new(p, Arc::new(NoPlugins));
    let dir = std::env::temp_dir().join(format!("wave-declick-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let x: Vec<f32> = (0..SR as usize * 2 * 12).map(|i| 0.2 * (i as f32 * 0.05).sin() + 0.25).collect();
    wave_engine::sources::write_wav(&wav, &x, SR).unwrap();
    e.import_audio(std::slice::from_ref(&wav), Some((1, 0.0))).unwrap();
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.play(0.0);
    run(&mut rt, SR as usize);
    (e, rt)
}

fn run(rt: &mut Rt, n: usize) -> Vec<f32> {
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n / BLOCK {
        rt.process(&mut l, &mut r, None);
        out.extend_from_slice(&l);
    }
    out
}

fn put(e: &Engine, track: usize, fx: Fx) {
    let ins = e.project().tracks[track].insert;
    e.apply(&[Op::PutFx { insert_id: ins, fx, index: None }], false, None).unwrap();
}

#[test]
fn a_bypass_flip_never_cuts_the_wet_in_one_sample() {
    let (e, mut rt) = setup("bypass");
    let wet = run(&mut rt, SR as usize);
    let peak = wet.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
    assert!(peak > 0.15, "the reverb must be audible for the test to mean anything (peak {peak})");

    // bypass mid-ring: the wet must ramp out, not step to zero
    put(&e, 1, Fx { bypass: true, ..new_fx(7000, FxKind::Reverb, e.project().bpm) });
    let mut around = Vec::new();
    around.push(*wet.last().unwrap());
    around.extend(run(&mut rt, BLOCK * 2));
    let down = max_step(&around);
    eprintln!("bypass on: loudest neighbouring-sample jump {down:.4} (wet peak {peak:.4})");
    assert!(down < 0.08, "bypassing cut the wet in one sample: jump {down}");

    // dry again (bypassed); switching the effect back on must ramp in the same way
    let dry = run(&mut rt, SR as usize / 2);
    put(&e, 1, new_fx(7000, FxKind::Reverb, e.project().bpm));
    let mut around = Vec::new();
    around.push(*dry.last().unwrap());
    around.extend(run(&mut rt, BLOCK * 2));
    let up = max_step(&around);
    eprintln!("bypass off: loudest neighbouring-sample jump {up:.4}");
    assert!(up < 0.08, "re-enabling stepped the wet in: jump {up}");
}
