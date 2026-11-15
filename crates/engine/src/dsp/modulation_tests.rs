//! Chorus and Phaser with deterministic signals: block-size independence, exact delays and
//! notches, stability at full feedback, smooth parameter changes, mono compatibility, tempo
//! sync, the presets, and the cost per second of audio.

use super::*;
use crate::catalog::{new_fx, param_block, presets};
use crate::model::{Fx, FxKind};

fn ctx(sr: f32) -> ProcessCtx {
    ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: false }
}

fn fx_with(kind: FxKind, kv: &[(&str, f64)]) -> Fx {
    let mut f = new_fx(1, kind, 120.0);
    f.params = crate::catalog::default_params(kind);
    for (k, v) in kv {
        assert!(f.params.contains_key(*k), "{kind:?} has no `{k}`");
        f.params.insert((*k).to_string(), *v);
    }
    f
}

fn make(f: &Fx, sr: f32) -> Box<dyn Processor> {
    let mut p = crate::dsp::create(f.kind).unwrap();
    p.prepare(sr, 512);
    p.set_params(&param_block(f));
    p
}

/// Deterministic white noise in [-amp, amp].
fn noise(n: usize, amp: f32, seed: u32) -> Vec<f32> {
    let mut s = seed.max(1);
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
        })
        .collect()
}

/// Runs `p` over (l, r) in blocks of `block` frames, with the transport position advancing.
fn run(p: &mut dyn Processor, l: &[f32], r: &[f32], block: usize, c: ProcessCtx) -> (Vec<f32>, Vec<f32>) {
    let (mut ol, mut or) = (l.to_vec(), r.to_vec());
    let mut at = 0;
    while at < ol.len() {
        let n = block.min(ol.len() - at);
        let beat = c.beat + at as f64 / f64::from(c.sample_rate) * c.bpm / 60.0;
        p.process(&mut ol[at..at + n], &mut or[at..at + n], &ProcessCtx { beat, ..c });
        at += n;
    }
    (ol, or)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn all_kinds() -> Vec<Fx> {
    vec![
        fx_with(FxKind::Chorus, &[("mode", 0.0), ("fb", 40.0), ("hp", 150.0), ("tone", 8000.0)]),
        fx_with(FxKind::Chorus, &[("mode", 1.0), ("width", 180.0)]),
        fx_with(FxKind::Chorus, &[("mode", 2.0), ("rate", 5.0), ("hp", 200.0)]),
        fx_with(FxKind::Phaser, &[("stages", 4.0), ("fb", 70.0)]),
        fx_with(FxKind::Phaser, &[("stages", 0.0), ("fb", -50.0), ("sync", 1.0), ("div", 2.0)]),
    ]
}

#[test]
fn the_output_does_not_depend_on_the_block_size() {
    let sr = 48_000.0;
    let (l, r) = (noise(24_000, 0.5, 7), noise(24_000, 0.5, 11));
    for f in all_kinds() {
        let c = ProcessCtx { playing: true, ..ctx(sr) };
        let reference = run(&mut *make(&f, sr), &l, &r, 512, c);
        for block in [1, 37, 64] {
            let got = run(&mut *make(&f, sr), &l, &r, block, c);
            let err = reference.0.iter().zip(&got.0).chain(reference.1.iter().zip(&got.1)).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(err < 1e-4, "{:?} {:?}: blocks of {block} differ by {err}", f.kind, f.params);
        }
    }
}

#[test]
fn a_still_chorus_is_two_plain_delays() {
    // depth 0: the voices sit at `delay` and 1.37 × `delay`, half level each
    let sr = 48_000.0;
    let f = fx_with(FxKind::Chorus, &[("depth", 0.0), ("delay", 10.0), ("mix", 100.0), ("width", 100.0)]);
    let mut p = make(&f, sr);
    let mut l = vec![0.0; 2048];
    l[0] = 1.0;
    let r = l.clone();
    let (out, out_r) = run(&mut *p, &l, &r, 256, ctx(sr));
    let at = |ms: f32| (ms * sr / 1000.0).round() as usize;
    // a fractional delay spreads the impulse over the interpolator's four taps: sum them
    let peak = |c: usize| out[c - 3..=c + 3].iter().sum::<f32>();
    assert!((peak(at(10.0)) - 0.5).abs() < 0.02, "first voice {}", peak(at(10.0)));
    assert!((peak(at(13.7)) - 0.5).abs() < 0.02, "second voice {}", peak(at(13.7)));
    let elsewhere: f32 = out
        .iter()
        .enumerate()
        .filter(|(i, _)| (*i as isize - at(10.0) as isize).abs() > 3 && (*i as isize - at(13.7) as isize).abs() > 3)
        .map(|(_, v)| v.abs())
        .fold(0.0, f32::max);
    assert!(elsewhere < 0.01, "stray energy {elsewhere}");
    assert_eq!(out, out_r, "a still chorus is the same on both sides");
}

#[test]
fn a_still_phaser_cuts_its_notches_where_the_theory_puts_them() {
    for sr in [44_100.0f32, 48_000.0, 96_000.0] {
        for (stages_ix, n) in [(0.0, 2usize), (1.0, 4), (3.0, 8)] {
            let center = 1000.0f32;
            let f =
                fx_with(FxKind::Phaser, &[("stages", stages_ix), ("depth", 0.0), ("fb", 0.0), ("mix", 50.0), ("center", center as f64)]);
            // N first-order all-passes reach −180° where tan(πf/sr) = tan(πfc/sr)·tan(π/(2N))
            let first = (sr / PI) * ((PI * center / sr).tan() * (PI / (2.0 * n as f32)).tan()).atan();
            let gain = |freq: f32| {
                let mut p = make(&f, sr);
                let len = (sr * 0.5) as usize;
                let x: Vec<f32> = (0..len).map(|i| (TAU * freq * i as f32 / sr).sin() * 0.5).collect();
                let (y, _) = run(&mut *p, &x, &x, 256, ctx(sr));
                rms(&y[len / 2..]) / rms(&x[len / 2..])
            };
            let notch = gain(first);
            assert!(notch < 0.01, "{sr} Hz, {n} stages: {notch} at the notch {first} Hz");
            // far below every notch the all-passes are in phase with the dry signal
            assert!(gain(20.0) > 0.97, "{sr} Hz, {n} stages: low end {}", gain(20.0));
        }
    }
}

#[test]
fn full_feedback_stays_bounded_and_finite() {
    let sr = 48_000.0;
    let (l, r) = (noise(sr as usize * 6, 1.0, 3), noise(sr as usize * 6, 1.0, 5));
    for f in [
        fx_with(FxKind::Chorus, &[("fb", 90.0), ("depth", 100.0), ("delay", 2.0), ("mix", 100.0)]),
        fx_with(FxKind::Chorus, &[("fb", -90.0), ("mode", 1.0), ("mix", 100.0), ("width", 200.0)]),
        fx_with(FxKind::Phaser, &[("fb", 95.0), ("stages", 4.0), ("mix", 100.0), ("center", 8000.0)]),
        fx_with(FxKind::Phaser, &[("fb", -95.0), ("stages", 0.0), ("mix", 100.0), ("center", 100.0)]),
    ] {
        let (ol, or) = run(&mut *make(&f, sr), &l, &r, 512, ctx(sr));
        let peak = ol.iter().chain(&or).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(ol.iter().chain(&or).all(|v| v.is_finite()) && peak < 8.0, "{:?} {:?}: peak {peak}", f.kind, f.params);
        // and after the input stops, it dies away within the tail it reports
        let mut p = make(&f, sr);
        run(&mut *p, &l, &r, 512, ctx(sr));
        let tail = p.tail_samples().unwrap();
        let silence = vec![0.0; tail + 4800];
        let (sl, _) = run(&mut *p, &silence, &silence, 512, ctx(sr));
        let end = rms(&sl[tail..]);
        assert!(end < 1e-5, "{:?}: still {end} after the reported tail of {tail} samples", f.kind);
    }
}

#[test]
fn parameter_jumps_do_not_click() {
    // a sine through the effect; halfway, every continuous parameter jumps to the other end
    let sr = 48_000.0;
    let len = sr as usize;
    let x: Vec<f32> = (0..len).map(|i| (TAU * 220.0 * i as f32 / sr).sin() * 0.5).collect();
    for (kind, a, b) in [
        (
            FxKind::Chorus,
            vec![("delay", 3.0), ("depth", 0.0), ("width", 0.0), ("mix", 0.0)],
            vec![("delay", 40.0), ("depth", 100.0), ("width", 200.0), ("mix", 100.0)],
        ),
        (
            FxKind::Phaser,
            vec![("center", 100.0), ("depth", 0.0), ("mix", 0.0), ("fb", 0.0)],
            vec![("center", 8000.0), ("depth", 100.0), ("mix", 100.0), ("fb", 80.0)],
        ),
    ] {
        let (fa, fb) = (fx_with(kind, &a), fx_with(kind, &b));
        let mut p = make(&fa, sr);
        let (mut l, mut r) = (x.clone(), x.clone());
        let half = len / 2;
        p.process(&mut l[..half], &mut r[..half], &ctx(sr));
        p.set_params(&param_block(&fb));
        p.process(&mut l[half..], &mut r[half..], &ctx(sr));
        let step = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        // the sine's own largest step is 2π·220/48000·0.5 ≈ 0.0144
        let jump = step(&l[half - 2..half + 2400]);
        assert!(jump < 0.05, "{kind:?}: a step of {jump} right after the change");
    }
}

#[test]
fn the_chorus_widens_without_cancelling_in_mono() {
    let sr = 48_000.0;
    let x = noise(sr as usize * 2, 0.5, 9);
    for (i, pre) in presets(FxKind::Chorus).iter().enumerate() {
        let mut f = new_fx(1, FxKind::Chorus, 120.0);
        crate::catalog::load_preset(&mut f, i, 120.0);
        let (l, r) = run(&mut *make(&f, sr), &x, &x, 512, ctx(sr));
        let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
        let side: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a - b)).collect();
        // what summing to mono loses against either side on its own
        let ratio = rms(&mono[4800..]) / rms(&l[4800..]).max(rms(&r[4800..]));
        assert!(ratio > 0.85, "{}: the mono sum keeps only {ratio} of the level", pre.name);
        if pre.params.get("width").copied().unwrap_or(100.0) >= 100.0 && pre.params.get("mode") != Some(&2.0) {
            assert!(rms(&side[4800..]) > 0.02 * rms(&x[4800..]), "{}: no stereo from a mono input", pre.name);
        }
    }
}

#[test]
fn a_synced_lfo_follows_the_song_position() {
    let sr = 48_000.0;
    let f = fx_with(FxKind::Phaser, &[("sync", 1.0), ("div", 4.0)]); // one bar = 4 beats
    let mut p = make(&f, sr);
    let mut m = [0.0f32; METERS];
    for beat in [0.0, 1.0, 2.5, 7.0] {
        let (mut l, mut r) = (vec![0.0; 480], vec![0.0; 480]);
        p.process(&mut l, &mut r, &ProcessCtx { sample_rate: sr, bpm: 120.0, beat, playing: true });
        p.meters(&mut m);
        // 480 samples at 120 BPM are 0.02 beats
        let want = ((beat + 0.02) / 4.0).fract() as f32;
        assert!((m[3] - want).abs() < 1e-3, "beat {beat}: phase {} not {want}", m[3]);
    }
}

#[test]
fn every_preset_is_in_range_and_sounds_different() {
    let sr = 48_000.0;
    let x = noise(sr as usize, 0.4, 21);
    for kind in [FxKind::Chorus, FxKind::Phaser] {
        let defs = crate::catalog::params(kind);
        let list = presets(kind);
        assert!(list.len() >= 6, "{kind:?} has {} presets", list.len());
        let mut renders: Vec<(&str, Vec<f32>)> = vec![];
        for (i, pre) in list.iter().enumerate() {
            for (k, v) in &pre.params {
                let d = defs.iter().find(|d| d.id == *k).unwrap_or_else(|| panic!("{}: unknown `{k}`", pre.name));
                assert!((d.min..=d.max).contains(v), "{}: {k} = {v} outside {}..{}", pre.name, d.min, d.max);
            }
            let mut f = new_fx(1, kind, 120.0);
            assert!(crate::catalog::load_preset(&mut f, i, 120.0));
            let (l, _) = run(&mut *make(&f, sr), &x, &x, 512, ProcessCtx { playing: true, ..ctx(sr) });
            assert!(l.iter().all(|v| v.is_finite()));
            renders.push((pre.name, l));
        }
        for (i, (a, ya)) in renders.iter().enumerate() {
            for (b, yb) in &renders[i + 1..] {
                let diff: Vec<f32> = ya.iter().zip(yb).map(|(p, q)| p - q).collect();
                assert!(rms(&diff) > 0.01 * rms(ya), "{a} and {b} sound the same");
            }
        }
    }
}

#[test]
fn both_effects_run_far_faster_than_real_time() {
    // the heaviest settings: Ensemble (3 voices a side) and a 12-stage phaser, 10 s at 48 kHz
    let sr = 48_000.0;
    let secs = 10.0;
    let (l, r) = (noise((sr * secs) as usize, 0.5, 1), noise((sr * secs) as usize, 0.5, 2));
    for f in [
        fx_with(FxKind::Chorus, &[("mode", 1.0), ("fb", 30.0), ("hp", 120.0), ("tone", 9000.0)]),
        fx_with(FxKind::Phaser, &[("stages", 4.0)]),
    ] {
        let mut p = make(&f, sr);
        let t = std::time::Instant::now();
        run(&mut *p, &l, &r, 256, ctx(sr));
        let speed = secs / t.elapsed().as_secs_f32();
        eprintln!("{:?}: {speed:.0}× real time", f.kind);
        // generous for debug builds and loaded CI machines; release is far faster
        assert!(speed > 40.0, "{:?} runs only {speed:.0}× real time", f.kind);
    }
}
