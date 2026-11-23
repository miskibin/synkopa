//! Filter with deterministic signals: block-size independence, slopes and notch where the theory
//! puts them, the same cutoff at every sample rate, a bounded self-oscillating ladder, stability
//! under audio-rate modulation, the soft clip, a transparent DJ centre, the envelope follower,
//! tempo sync, click-free parameter jumps, aliasing with drive, the presets, and the cost.

use std::f32::consts::TAU;

use super::*;
use crate::catalog::{default_params, load_preset, new_fx, param_block, presets};
use crate::model::{Fx, FxKind};

fn ctx(sr: f32) -> ProcessCtx {
    ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: false }
}

fn filt(kv: &[(&str, f64)]) -> Fx {
    let mut f = new_fx(1, FxKind::Filter, 120.0);
    f.params = default_params(FxKind::Filter);
    // a plain, clean filter unless asked otherwise
    for (k, v) in [("drive", 0.0), ("clip", 0.0), ("res", 0.0), ("lfo", 0.0), ("env", 0.0)] {
        f.params.insert(k.into(), v);
    }
    for (k, v) in kv {
        assert!(f.params.contains_key(*k), "filter has no `{k}`");
        f.params.insert((*k).to_string(), *v);
    }
    f
}

fn make(f: &Fx, sr: f32) -> Filter {
    let mut p = Filter::default();
    p.prepare(sr, 512);
    p.set_params(&param_block(f));
    p
}

fn sine(n: usize, hz: f32, amp: f32, sr: f32) -> Vec<f32> {
    (0..n).map(|i| (TAU * hz * i as f32 / sr).sin() * amp).collect()
}

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

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

fn run(p: &mut Filter, l: &[f32], r: &[f32], key: Option<&[f32]>, block: usize, c: ProcessCtx) -> (Vec<f32>, Vec<f32>) {
    let (mut ol, mut or) = (l.to_vec(), r.to_vec());
    let mut at = 0;
    while at < ol.len() {
        let n = block.min(ol.len() - at);
        let beat = c.beat + at as f64 / f64::from(c.sample_rate) * c.bpm / 60.0;
        let cx = ProcessCtx { beat, ..c };
        match key {
            Some(k) => p.process_keyed(&mut ol[at..at + n], &mut or[at..at + n], &k[at..at + n], &cx),
            None => p.process(&mut ol[at..at + n], &mut or[at..at + n], &cx),
        }
        at += n;
    }
    (ol, or)
}

/// Gain (dB) of a steady sine at `hz` through `f`, after it settles.
fn gain_at(f: &Fx, hz: f32, sr: f32) -> f32 {
    let x = sine((0.5 * sr) as usize, hz, 0.25, sr);
    let (y, _) = run(&mut make(f, sr), &x, &x, None, 512, ctx(sr));
    let tail = (0.25 * sr) as usize;
    db(rms(&y[tail..]) / rms(&x[tail..]))
}

#[test]
fn the_output_does_not_depend_on_the_block_size() {
    let sr = 48_000.0;
    let x = noise(sr as usize, 0.5, 4);
    let k = sine(sr as usize, 3.0, 0.5, sr);
    for f in [
        filt(&[("type", 1.0), ("res", 80.0), ("drive", 12.0), ("lfo", 2.0), ("sync", 0.0), ("rate", 3.0), ("shape", 4.0)]),
        filt(&[("type", 3.0), ("res", 60.0), ("env", 4.0), ("stereo", 90.0), ("lfo", 1.0), ("shape", 5.0)]),
        filt(&[("type", 5.0), ("dj", -40.0), ("clip", 1.0)]),
    ] {
        let (a, _) = run(&mut make(&f, sr), &x, &x, Some(&k), 512, ctx(sr));
        for block in [1, 7, 64, 333] {
            let (b, _) = run(&mut make(&f, sr), &x, &x, Some(&k), block, ctx(sr));
            assert_eq!(a, b, "block {block}");
        }
    }
}

#[test]
fn slopes_and_notch_sit_where_the_theory_puts_them() {
    let sr = 48_000.0;
    // low-pass at 1 kHz: −3 dB at the cutoff (Butterworth-like at 0 % resonance for 12 dB:
    // Q 0.5 is −6 dB), 12 or 24 dB per octave above it
    let lp12 = filt(&[("type", 0.0), ("slope", 0.0), ("cutoff", 1000.0), ("res", 7.143)]);
    assert!((gain_at(&lp12, 1000.0, sr) + 3.0).abs() < 0.3, "12 dB LP at its cutoff");
    let oct = gain_at(&lp12, 4000.0, sr) - gain_at(&lp12, 8000.0, sr);
    assert!((oct - 12.0).abs() < 1.5, "12 dB LP: {oct:.1} dB per octave");
    let lp24 = filt(&[("type", 0.0), ("slope", 1.0), ("cutoff", 1000.0), ("res", 7.143)]);
    let oct = gain_at(&lp24, 4000.0, sr) - gain_at(&lp24, 8000.0, sr);
    assert!((oct - 24.0).abs() < 2.5, "24 dB LP: {oct:.1} dB per octave");
    assert!(gain_at(&lp24, 100.0, sr).abs() < 0.2, "the pass band is flat");
    let hp24 = filt(&[("type", 2.0), ("slope", 1.0), ("cutoff", 1000.0), ("res", 7.143)]);
    let oct = gain_at(&hp24, 250.0, sr) - gain_at(&hp24, 125.0, sr);
    assert!((oct - 24.0).abs() < 2.5, "24 dB HP: {oct:.1} dB per octave");
    assert!(gain_at(&hp24, 10_000.0, sr).abs() < 0.2);
    let notch = filt(&[("type", 4.0), ("slope", 0.0), ("cutoff", 1000.0), ("res", 30.0)]);
    assert!(gain_at(&notch, 1000.0, sr) < -40.0, "the notch is deep at its frequency");
    assert!(gain_at(&notch, 100.0, sr).abs() < 0.5 && gain_at(&notch, 10_000.0, sr).abs() < 0.5);
    let bp = filt(&[("type", 3.0), ("slope", 0.0), ("cutoff", 1000.0), ("res", 50.0)]);
    let peak = gain_at(&bp, 1000.0, sr);
    assert!(peak > gain_at(&bp, 250.0, sr) + 15.0 && peak > gain_at(&bp, 4000.0, sr) + 15.0, "band-pass peaks at its cutoff");
    // the ladder: 24 dB per octave too, and resonance lifts the cutoff
    let lad = filt(&[("type", 1.0), ("cutoff", 1000.0), ("res", 0.0)]);
    let oct = gain_at(&lad, 4000.0, sr) - gain_at(&lad, 8000.0, sr);
    assert!((oct - 24.0).abs() < 3.0, "ladder: {oct:.1} dB per octave");
    let res = filt(&[("type", 1.0), ("cutoff", 1000.0), ("res", 85.0)]);
    assert!(gain_at(&res, 1000.0, sr) > gain_at(&res, 200.0, sr) + 6.0, "a resonant ladder peaks at its cutoff");
    // passband compensation: 85 % resonance loses at most ~6 dB of low end, not the 11 dB of
    // a plain ladder
    assert!(gain_at(&res, 100.0, sr) > -6.5, "low end at 85 %: {:.1} dB", gain_at(&res, 100.0, sr));
}

#[test]
fn the_cutoff_is_the_same_at_every_sample_rate() {
    for (ty, hz) in [(0.0, 2000.0), (1.0, 2000.0), (2.0, 300.0), (0.0, 12_000.0)] {
        let f = filt(&[("type", ty), ("cutoff", hz as f64), ("res", 40.0)]);
        let base = gain_at(&f, hz, 48_000.0);
        for sr in [44_100.0, 96_000.0, 192_000.0] {
            let g = gain_at(&f, hz, sr);
            assert!((g - base).abs() < 0.6, "type {ty} at {hz} Hz: {g:.2} dB at {sr} Hz vs {base:.2} at 48 kHz");
        }
    }
}

#[test]
fn a_ladder_at_full_resonance_sings_but_stays_bounded() {
    let sr = 48_000.0;
    let f = filt(&[("type", 1.0), ("cutoff", 800.0), ("res", 100.0)]);
    let mut p = make(&f, sr);
    let mut x = vec![0.0f32; sr as usize * 2];
    x[0] = 0.5;
    let (y, _) = run(&mut p, &x, &x, None, 256, ctx(sr));
    assert!(y.iter().all(|v| v.is_finite()));
    let late = rms(&y[sr as usize..]);
    let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(late > 0.01, "it keeps oscillating on its own ({late})");
    assert!(peak < 1.5, "but the saturating feedback bounds it ({peak})");
    assert_eq!(p.tail_samples(), None, "a self-oscillating filter never lets the chain sleep");
    // below the edge it rings out
    let f = filt(&[("type", 1.0), ("cutoff", 800.0), ("res", 90.0)]);
    let mut p = make(&f, sr);
    let (y, _) = run(&mut p, &x, &x, None, 256, ctx(sr));
    assert!(rms(&y[sr as usize..]) < 1e-5);
    assert!(p.tail_samples().unwrap() < sr as usize * 2);
}

#[test]
fn audio_rate_modulation_at_full_resonance_stays_stable() {
    let x: Vec<f32> = noise(96_000, 0.5, 8)
        .iter()
        .enumerate()
        .map(|(i, v)| v + if i > 30_000 { 0.4 } else { 0.0 } + 0.3 * (i as f32 * 0.13).sin())
        .collect();
    for sr in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        for ty in 0..TYPES.len() {
            for clip_on in [0.0, 1.0] {
                // a 20 Hz square sweeping ±5 octaves around 640 Hz: 20 Hz to 20 kHz
                let f = filt(&[
                    ("type", ty as f64),
                    ("cutoff", 640.0),
                    ("res", 100.0),
                    ("drive", 24.0),
                    ("sync", 0.0),
                    ("rate", 20.0),
                    ("shape", 4.0),
                    ("lfo", 5.0),
                    ("dj", -60.0),
                    ("clip", clip_on),
                ]);
                let (l, r) = run(&mut make(&f, sr), &x, &x, None, 128, ctx(sr));
                let peak = l.iter().chain(&r).fold(0.0f32, |m, v| if v.is_finite() { m.max(v.abs()) } else { f32::INFINITY });
                if clip_on > 0.5 {
                    assert!(peak <= 1.0, "type {ty} at {sr} Hz, soft clip on: peak {peak}");
                } else {
                    assert!(peak < 40.0, "type {ty} at {sr} Hz: peak {peak}");
                }
            }
        }
    }
}

#[test]
fn the_dj_centre_is_untouched_and_its_sides_filter() {
    let sr = 48_000.0;
    let x = noise(sr as usize, 0.5, 11);
    let centre = filt(&[("type", 5.0), ("dj", 0.0), ("res", 80.0)]);
    let (y, _) = run(&mut make(&centre, sr), &x, &x, None, 256, ctx(sr));
    assert_eq!(y, x, "the centre passes the sound bit for bit");
    let lp = filt(&[("type", 5.0), ("dj", -60.0)]);
    let hp = filt(&[("type", 5.0), ("dj", 60.0)]);
    assert!(gain_at(&lp, 10_000.0, sr) < -30.0 && gain_at(&lp, 100.0, sr) > -1.0, "left: a low-pass");
    assert!(gain_at(&hp, 100.0, sr) < -30.0 && gain_at(&hp, 10_000.0, sr) > -1.0, "right: a high-pass");
}

#[test]
fn the_envelope_follower_opens_the_filter_with_the_level() {
    let sr = 48_000.0;
    let n = sr as usize;
    let quiet_loud: Vec<f32> = sine(n, 200.0, 1.0, sr).iter().enumerate().map(|(i, v)| v * if i < n / 2 { 0.005 } else { 0.5 }).collect();
    let f = filt(&[("type", 0.0), ("cutoff", 300.0), ("env", 4.0), ("eatt", 5.0), ("erel", 100.0)]);
    let mut p = make(&f, sr);
    let mut m = [0.0f32; METERS];
    run(&mut p, &quiet_loud[..n / 2], &quiet_loud[..n / 2], None, 256, ctx(sr));
    p.meters(&mut m);
    assert!(m[0] < 300.0 * 2f32.powf(0.6), "quiet (−46 dBFS): almost closed, {} Hz", m[0]);
    run(&mut p, &quiet_loud[n / 2..], &quiet_loud[n / 2..], None, 256, ctx(sr));
    p.meters(&mut m);
    // −6 dBFS reads (−6 + 40) / 40 = 0.85 → 3.4 octaves up
    assert!((m[0] / 300.0).log2() > 3.0, "loud: opened to {} Hz", m[0]);
    // a sidechain drives it instead of the input
    let silent = vec![0.0f32; n / 2];
    let key = vec![0.5f32; n / 2];
    let mut p = make(&f, sr);
    run(&mut p, &silent, &silent, Some(&key), 256, ctx(sr));
    p.meters(&mut m);
    assert!((m[0] / 300.0).log2() > 3.0 && m[6] > 0.5, "keyed: opened to {} Hz", m[0]);
}

#[test]
fn a_synced_lfo_and_its_s_and_h_follow_the_song() {
    let sr = 48_000.0;
    let f = filt(&[("type", 0.0), ("cutoff", 1000.0), ("lfo", 2.0), ("sync", 1.0), ("div", 2.0), ("shape", 5.0)]);
    let cut_at = |beat: f64| {
        let mut p = make(&f, sr);
        let x = vec![0.0f32; 4800];
        run(&mut p, &x, &x, None, 480, ProcessCtx { beat, playing: true, ..ctx(sr) });
        let mut m = [0.0f32; METERS];
        p.meters(&mut m);
        (m[0], m[3])
    };
    // the same song position gives the same step, wherever playback started
    let (a, ph) = cut_at(3.2);
    assert!((ph - 0.4).abs() < 1e-3, "quarter-note LFO at beat 3.4: phase {ph}");
    let (b, _) = cut_at(3.2);
    assert_eq!(a, b);
    let steps: Vec<f32> = (0..6).map(|i| cut_at(f64::from(i) + 0.3).0).collect();
    let distinct = steps.windows(2).filter(|w| (w[0] - w[1]).abs() > 1.0).count();
    assert!(distinct >= 4, "S&H picks a new step every beat: {steps:?}");
    for s in &steps {
        let o = (s / 1000.0).log2();
        assert!(o.abs() <= 2.0 + 1e-3, "within ±2 octaves: {s}");
    }
}

#[test]
fn parameter_jumps_do_not_click() {
    let sr = 48_000.0;
    let len = sr as usize;
    let x = sine(len, 220.0, 0.5, sr);
    for ty in [0.0, 1.0, 2.0, 3.0] {
        let a = filt(&[("type", ty), ("cutoff", 200.0), ("res", 0.0), ("drive", 0.0)]);
        let b = filt(&[("type", ty), ("cutoff", 12_000.0), ("res", 60.0), ("drive", 12.0)]);
        let mut p = make(&a, sr);
        let (mut l, mut r) = (x.clone(), x.clone());
        let half = len / 2;
        p.process(&mut l[..half], &mut r[..half], &ctx(sr));
        p.set_params(&param_block(&b));
        p.process(&mut l[half..], &mut r[half..], &ctx(sr));
        let step = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        // the sine's own largest step is 2π·220/48000·0.5 ≈ 0.0144; resonance adds some
        let jump = step(&l[half - 2..half + 4800]);
        assert!(jump < 0.06, "type {ty}: a step of {jump} right after the change");
    }
}

#[test]
fn drive_aliasing_stays_low() {
    // 4.1 kHz at −6 dBFS into +12 dB of drive and a bright ladder: the odd harmonics above
    // Nyquist fold back to 19.3, 11.1 and 2.9 kHz. Measured with a Hann-windowed DFT.
    let sr = 48_000.0;
    let n = 1 << 14;
    let x = sine(n + 4800, 4100.0, 0.5, sr);
    let f = filt(&[("type", 1.0), ("cutoff", 12_000.0), ("res", 60.0), ("drive", 12.0)]);
    let (y, _) = run(&mut make(&f, sr), &x, &x, None, 512, ctx(sr));
    let y = &y[4800..];
    let bin = |hz: f32| {
        let (mut re, mut im, mut wsum) = (0.0f64, 0.0f64, 0.0f64);
        for (i, v) in y.iter().enumerate() {
            let w = 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos();
            let ph = f64::from(TAU * hz * i as f32 / sr);
            re += f64::from(v * w) * ph.cos();
            im += f64::from(v * w) * ph.sin();
            wsum += f64::from(w);
        }
        (2.0 * (re * re + im * im).sqrt() / wsum) as f32
    };
    let fund = db(bin(4100.0));
    let worst = [19_300.0, 11_100.0, 2_900.0].iter().map(|&h| db(bin(h)) - fund).fold(f32::NEG_INFINITY, f32::max);
    eprintln!("drive aliasing: worst alias {worst:.1} dB under the fundamental");
    assert!(worst < -40.0, "aliases {worst:.1} dB under the fundamental");
}

#[test]
fn every_preset_is_in_range_and_sounds_different() {
    let sr = 48_000.0;
    let x = noise(sr as usize, 0.4, 21);
    let defs = crate::catalog::params(FxKind::Filter);
    let list = presets(FxKind::Filter);
    assert!(list.len() >= 8);
    let mut renders: Vec<(&str, Vec<f32>)> = vec![];
    for (i, pre) in list.iter().enumerate() {
        for (k, v) in &pre.params {
            let d = defs.iter().find(|d| d.id == *k).unwrap_or_else(|| panic!("{}: unknown `{k}`", pre.name));
            assert!((d.min..=d.max).contains(v), "{}: {k} = {v} outside {}..{}", pre.name, d.min, d.max);
        }
        let mut f = new_fx(1, FxKind::Filter, 120.0);
        assert!(load_preset(&mut f, i, 120.0));
        let (l, _) = run(&mut make(&f, sr), &x, &x, None, 512, ProcessCtx { playing: true, ..ctx(sr) });
        assert!(l.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{}", pre.name);
        renders.push((pre.name, l));
    }
    for (i, (a, ya)) in renders.iter().enumerate() {
        for (b, yb) in &renders[i + 1..] {
            let diff: Vec<f32> = ya.iter().zip(yb).map(|(p, q)| p - q).collect();
            assert!(rms(&diff) > 0.01 * rms(ya), "{a} and {b} sound the same");
        }
    }
}

#[test]
fn the_filter_runs_far_faster_than_real_time() {
    // the heaviest settings: 24 dB SVF and the ladder, both modulated every sample
    let sr = 48_000.0;
    let secs = 10.0;
    let x = noise((sr * secs) as usize, 0.5, 1);
    for f in [
        filt(&[("type", 0.0), ("slope", 1.0), ("res", 60.0), ("drive", 6.0), ("lfo", 2.0), ("stereo", 90.0), ("env", 2.0), ("clip", 1.0)]),
        filt(&[("type", 1.0), ("res", 80.0), ("drive", 12.0), ("lfo", 2.0), ("stereo", 90.0), ("env", 2.0), ("clip", 1.0)]),
    ] {
        let speed = (0..3)
            .map(|_| {
                let mut p = make(&f, sr);
                let t = std::time::Instant::now();
                run(&mut p, &x, &x, None, 256, ctx(sr));
                secs / t.elapsed().as_secs_f32()
            })
            .fold(0.0f32, f32::max);
        eprintln!("filter type {}: {speed:.0}× real time", f.params["type"]);
        assert!(speed > 20.0, "filter: {speed:.0}× real time");
    }
}

#[test]
fn the_dj_centre_leaves_loud_audio_alone_with_the_clip_on() {
    let sr = 48_000.0;
    let x = noise(sr as usize / 4, 1.0, 5);
    let centre = filt(&[("type", 5.0), ("dj", 0.0), ("clip", 1.0)]);
    let (y, _) = run(&mut make(&centre, sr), &x, &x, None, 256, ctx(sr));
    assert_eq!(y, x, "full-scale samples pass the centre unclipped");
}

#[test]
fn switching_the_type_or_slope_on_a_sustained_note_does_not_click() {
    let sr = 48_000.0;
    let len = sr as usize;
    let x = sine(len, 220.0, 0.5, sr);
    let step = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    type Kv<'a> = &'a [(&'a str, f64)];
    let cases: [(Kv, Kv); 6] = [
        (&[("type", 0.0)], &[("type", 2.0)]),
        (&[("type", 2.0)], &[("type", 1.0)]),
        (&[("type", 1.0)], &[("type", 0.0)]),
        (&[("type", 3.0)], &[("type", 4.0)]),
        (&[("type", 0.0), ("slope", 0.0)], &[("type", 0.0), ("slope", 1.0)]),
        (&[("type", 5.0), ("dj", -40.0)], &[("type", 1.0)]),
    ];
    for (a, b) in cases {
        let with = |kv: &[(&str, f64)]| filt(&[kv, &[("cutoff", 2000.0), ("res", 40.0)]].concat());
        let mut p = make(&with(a), sr);
        let (mut l, mut r) = (x.clone(), x.clone());
        let half = len / 2;
        p.process(&mut l[..half], &mut r[..half], &ctx(sr));
        p.set_params(&param_block(&with(b)));
        p.process(&mut l[half..], &mut r[half..], &ctx(sr));
        let jump = step(&l[half - 2..half + 4800]);
        assert!(jump < 0.03, "{a:?} → {b:?}: a step of {jump} at the switch");
        assert!(l.iter().all(|v| v.is_finite()));
    }
    // a burst of switches (automation stepping the type) still never jumps
    let mut p = make(&filt(&[("type", 0.0), ("cutoff", 2000.0)]), sr);
    let (mut l, mut r) = (x.clone(), x.clone());
    for (i, ch) in l.chunks_mut(64).zip(r.chunks_mut(64)).enumerate() {
        p.set_params(&param_block(&filt(&[("type", (i % 5) as f64), ("cutoff", 2000.0)])));
        p.process(ch.0, ch.1, &ctx(sr));
    }
    let jump = step(&l);
    assert!(jump < 0.06, "switching every 64 samples: a step of {jump}");
}

#[test]
fn the_ladder_has_rung_out_when_its_reported_tail_ends() {
    let sr = 48_000.0;
    for res in [80.0, 92.0, 96.0] {
        let f = filt(&[("type", 1.0), ("cutoff", 800.0), ("res", res)]);
        let mut p = make(&f, sr);
        let mut x = vec![0.0f32; 64];
        x[0] = 0.5;
        let (mut l, mut r) = (x.clone(), x.clone());
        p.process(&mut l, &mut r, &ctx(sr));
        let tail = p.tail_samples().expect("below self-oscillation the ladder reports a tail");
        let mut silent = vec![0.0f32; tail];
        let mut s2 = silent.clone();
        p.process(&mut silent, &mut s2, &ctx(sr));
        let mut after = vec![0.0f32; 4800];
        let mut a2 = after.clone();
        p.process(&mut after, &mut a2, &ctx(sr));
        let left = after.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(db(left) < -100.0, "res {res}: {:.1} dBFS still ringing after the {:.2} s tail", db(left), tail as f32 / sr);
    }
}
