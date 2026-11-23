//! Gate / expander with deterministic signals: block-size independence, hysteresis against
//! chatter, click-free edges, floor, expander slope, ducking, look-ahead and its latency,
//! side-chain listen, sample-rate independence, the presets, and the cost.

use std::f32::consts::TAU;

use super::*;
use crate::catalog::{default_params, latency, load_preset, new_fx, param_block, presets};
use crate::model::{Fx, FxKind};

fn ctx(sr: f32) -> ProcessCtx {
    ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: false }
}

fn gate_fx(kv: &[(&str, f64)]) -> Fx {
    let mut f = new_fx(1, FxKind::Gate, 120.0);
    f.params = default_params(FxKind::Gate);
    for (k, v) in kv {
        assert!(f.params.contains_key(*k), "gate has no `{k}`");
        f.params.insert((*k).to_string(), *v);
    }
    f
}

fn make(f: &Fx, sr: f32) -> Gate {
    let mut g = Gate::default();
    g.prepare(sr, 512);
    g.set_params(&param_block(f));
    g
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

/// Runs the gate in blocks, with an optional key; returns the output and the state after each block.
fn run(g: &mut Gate, x: &[f32], key: Option<&[f32]>, block: usize) -> (Vec<f32>, Vec<State>) {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    let mut states = vec![];
    let mut at = 0;
    while at < l.len() {
        let n = block.min(l.len() - at);
        match key {
            Some(k) => g.process_keyed(&mut l[at..at + n], &mut r[at..at + n], &k[at..at + n], &ctx(g.sr)),
            None => g.process(&mut l[at..at + n], &mut r[at..at + n], &ctx(g.sr)),
        }
        states.push(g.state);
        at += n;
    }
    assert_eq!(l, r, "a mono input stays mono");
    (l, states)
}

/// A tone that is loud for `on` samples, then quiet for `off`, repeated.
fn bursts(n: usize, sr: f32, on: usize, off: usize, loud: f32, quiet: f32) -> Vec<f32> {
    sine(n, 220.0, 1.0, sr).iter().enumerate().map(|(i, v)| v * if i % (on + off) < on { loud } else { quiet }).collect()
}

#[test]
fn the_output_does_not_depend_on_the_block_size() {
    let sr = 48_000.0;
    let x = bursts(sr as usize, sr, 4000, 6000, 0.5, 0.003);
    for f in [
        gate_fx(&[]),
        gate_fx(&[("ratio", 3.0), ("floor", -20.0), ("look", 3.0), ("det", 1.0)]),
        gate_fx(&[("mode", 1.0), ("schp", 200.0), ("sclp", 3000.0), ("hold", 0.0)]),
    ] {
        let (a, _) = run(&mut make(&f, sr), &x, None, 512);
        for block in [1, 7, 64, 333] {
            let (b, _) = run(&mut make(&f, sr), &x, None, block);
            assert_eq!(a, b, "block {block}");
        }
    }
}

#[test]
fn hysteresis_and_hold_stop_a_decaying_note_from_chattering() {
    // a 200 Hz tone decaying from −6 to −45 dBFS over 600 ms, over steady noise at −26 dBFS
    // RMS (band-limited to the low mids so it sits near the threshold), through a −20 dB gate
    let sr = 48_000.0;
    let n = (0.8 * sr) as usize;
    let nz = noise(n, 0.05 * 3f32.sqrt(), 5);
    let mut lp = 0.0f32;
    let x: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let env = 10f32.powf((-6.0 - 39.0 * (t / 0.6).min(1.0)) / 20.0);
            lp += (nz[i] - lp) * 0.2;
            env * (TAU * 200.0 * t).sin() + lp
        })
        .collect();
    let opens = |kv: &[(&str, f64)]| {
        let mut g = make(&gate_fx(kv), sr);
        let (_, st) = run(&mut g, &x, None, 1);
        st.windows(2).filter(|w| w[1] == State::Open && w[0] != State::Open && w[0] != State::Hold).count()
    };
    let calm = opens(&[("thr", -20.0), ("hyst", 6.0), ("hold", 40.0), ("att", 1.0), ("rel", 50.0)]);
    let raw = opens(&[("thr", -20.0), ("hyst", 0.0), ("hold", 0.0), ("att", 1.0), ("rel", 50.0)]);
    assert_eq!(calm, 1, "with hysteresis and hold the gate opens once, stays open through the decay, then closes");
    assert!(raw >= 4, "without them it chatters ({raw} re-openings): the test signal is a real stress");
}

#[test]
fn opening_and_closing_do_not_click() {
    // a steady low tone, gated by a key that switches on and off: every high-frequency
    // component of the output comes from the gate's edges
    let sr = 48_000.0;
    let n = sr as usize;
    let x = sine(n, 100.0, 0.5, sr);
    let key: Vec<f32> = (0..n).map(|i| if (i / 9600) % 2 == 0 { 0.5 } else { 0.0 }).collect();
    for (att, limit) in [(2.0, -65.0), (0.5, -50.0)] {
        let f = gate_fx(&[("thr", -20.0), ("hyst", 3.0), ("att", att), ("hold", 0.0), ("rel", 20.0), ("floor", -80.0)]);
        let (y, _) = run(&mut make(&f, sr), &x, Some(&key), 256);
        // 4th-order high-pass at 4 kHz: the 100 Hz tone is gone, the edges' splatter is not
        let mut hp = [Biquad::default(); 2];
        for (j, q) in [0.541, 1.307].into_iter().enumerate() {
            hp[j].set(FilterType::Highpass, sr, 4000.0, q, 0.0);
        }
        let resid = y
            .iter()
            .map(|&v| {
                let a = hp[0].tick(v);
                hp[1].tick(a).abs()
            })
            .skip(2000)
            .fold(0.0f32, f32::max);
        assert!(db(resid) < limit, "attack {att} ms: edge splatter at {:.1} dBFS", db(resid));
    }
}

#[test]
fn a_closed_gate_rests_at_the_floor_and_an_expander_follows_its_ratio() {
    let sr = 48_000.0;
    let x = sine(sr as usize, 440.0, 0.1, sr); // −20 dBFS peak, under the threshold
    let tail = |kv: &[(&str, f64)]| {
        let (y, _) = run(&mut make(&gate_fx(kv), sr), &x, None, 512);
        db(rms(&y[24_000..]) / rms(&x[24_000..]))
    };
    let floor = tail(&[("thr", -10.0), ("floor", -14.0)]);
    assert!((floor + 14.0).abs() < 0.2, "a hard gate holds at its floor: {floor:.2} dB");
    assert!(tail(&[("thr", -10.0), ("floor", -80.0)]) < -100.0, "the bottom of the floor is silence");
    // 2:1 expander, close threshold at −10 dB: the peak detector sits at −20 dB, 10 dB under,
    // so the gain is (2 − 1) × −10 = −10 dB
    let exp = tail(&[("thr", -6.0), ("hyst", 4.0), ("ratio", 2.0), ("floor", -40.0)]);
    assert!((exp + 10.0).abs() < 0.6, "2:1 expansion 10 dB under: {exp:.2} dB");
    // a loud tone passes untouched once open
    let open = tail(&[("thr", -30.0)]);
    assert!(open.abs() < 0.01, "open: {open:.3} dB");
}

#[test]
fn duck_mode_turns_the_sound_down_while_the_key_plays() {
    let sr = 48_000.0;
    let n = 2 * sr as usize;
    let x = sine(n, 330.0, 0.3, sr);
    let key: Vec<f32> = (0..n).map(|i| if i < n / 2 { 0.5 } else { 0.0 }).collect();
    let f = gate_fx(&[("mode", 1.0), ("thr", -20.0), ("floor", -12.0), ("att", 1.0), ("hold", 0.0), ("rel", 50.0)]);
    let (y, _) = run(&mut make(&f, sr), &x, Some(&key), 256);
    let ducked = db(rms(&y[4800..n / 2]) / rms(&x[4800..n / 2]));
    let back = db(rms(&y[n / 2 + 24_000..]) / rms(&x[n / 2 + 24_000..]));
    assert!((ducked + 12.0).abs() < 0.3, "key playing: {ducked:.2} dB");
    assert!(back.abs() < 0.05, "key quiet: {back:.3} dB");
}

#[test]
fn look_ahead_delays_by_its_reported_latency_and_keeps_the_transient() {
    for sr in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
        for look in 0..LOOKS.len() {
            let f = gate_fx(&[("look", look as f64), ("thr", -60.0)]);
            let d = latency(&f, sr);
            assert_eq!(d, latency_samples(look as f64, sr));
            let mut g = make(&f, sr);
            // open the gate first, then an impulse
            let mut x = vec![0.0f32; (0.05 * sr) as usize];
            for v in x.iter_mut().take((0.02 * sr) as usize) {
                *v = 0.01;
            }
            let at = (0.03 * sr) as usize;
            x[at] = 1.0;
            let (y, _) = run(&mut g, &x, None, 64);
            let peak = y.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
            assert_eq!(peak, at + d, "{sr} Hz, look {look}: the impulse comes out {d} samples late");
        }
    }
    // a drum-like hit from silence: 10 ms of look-ahead lets a 5 ms attack open before it lands
    let sr = 48_000.0;
    let mut x = vec![0.0f32; 9600];
    for (i, v) in x.iter_mut().enumerate().skip(4800).take(480) {
        *v = (-((i - 4800) as f32) / 120.0).exp() * (TAU * 80.0 * (i - 4800) as f32 / sr).sin() * 0.8;
    }
    let first_ms = |look: f64| {
        let f = gate_fx(&[("look", look), ("att", 5.0), ("thr", -30.0)]);
        let d = latency(&f, sr);
        let (y, _) = run(&mut make(&f, sr), &x, None, 64);
        rms(&y[4800 + d..4800 + d + 240]) / rms(&x[4800..5040])
    };
    assert!(first_ms(4.0) > 0.98, "with 10 ms look-ahead the hit's first 5 ms come through whole");
    assert!(first_ms(0.0) < 0.8, "without it the attack eats the hit's start");
}

#[test]
fn the_sidechain_filters_and_listen_hear_only_the_band() {
    // the kick (60 Hz) is loud, the snare band (2 kHz) quiet: with the side-chain high-pass at
    // 500 Hz only the 2 kHz part decides, so a threshold between the two keeps the gate shut
    let sr = 48_000.0;
    let n = sr as usize / 2;
    let x: Vec<f32> = sine(n, 60.0, 0.5, sr).iter().zip(sine(n, 2000.0, 0.01, sr)).map(|(a, b)| a + b).collect();
    let f = gate_fx(&[("thr", -25.0), ("schp", 500.0), ("floor", -40.0)]);
    let (y, _) = run(&mut make(&f, sr), &x, None, 256);
    assert!(db(rms(&y[12_000..]) / rms(&x[12_000..])) < -39.0, "the kick does not open a gate that listens above 500 Hz");
    let open = gate_fx(&[("thr", -25.0), ("floor", -40.0)]);
    let (y, _) = run(&mut make(&open, sr), &x, None, 256);
    assert!(db(rms(&y[12_000..]) / rms(&x[12_000..])).abs() < 0.1, "without the filter it does");
    // listen: the output is what the detector hears, so the kick is gone
    let listen = gate_fx(&[("thr", -25.0), ("schp", 500.0), ("listen", 1.0)]);
    let (y, _) = run(&mut make(&listen, sr), &x, None, 256);
    assert!((db(rms(&y[12_000..])) - db(0.01 / 2f32.sqrt())).abs() < 1.0, "listen plays the 2 kHz band");
}

#[test]
fn times_mean_the_same_at_every_sample_rate() {
    // release 100 ms: how long the gain takes to fall from 0 to −40 dB after the input stops
    let fall_ms = |sr: f32| {
        let f = gate_fx(&[("thr", -30.0), ("hold", 0.0), ("rel", 100.0), ("floor", -60.0)]);
        let mut g = make(&f, sr);
        let x = sine((0.1 * sr) as usize, 440.0, 0.5, sr);
        run(&mut g, &x, None, 64);
        let mut m = [0.0f32; METERS];
        let silent = vec![0.0f32; 16];
        let mut t = 0usize;
        loop {
            let (mut l, mut r) = (silent.clone(), silent.clone());
            g.process(&mut l, &mut r, &ctx(sr));
            t += 16;
            g.meters(&mut m);
            if m[1] < -40.0 || t > sr as usize {
                break;
            }
        }
        t as f32 / sr * 1000.0
    };
    let base = fall_ms(48_000.0);
    for sr in [44_100.0, 96_000.0, 192_000.0] {
        let ms = fall_ms(sr);
        assert!((ms - base).abs() < 2.0, "{sr} Hz: {ms:.1} ms against {base:.1} ms at 48 kHz");
    }
}

#[test]
fn silence_and_extremes_stay_finite() {
    let sr = 44_100.0;
    let mut x = noise(sr as usize, 1.0, 3);
    x.extend(std::iter::repeat_n(0.0, sr as usize * 2));
    for f in [
        gate_fx(&[("att", 0.1), ("rel", 5.0), ("hold", 0.0), ("hyst", 0.0), ("thr", 0.0)]),
        gate_fx(&[("ratio", 1.0), ("floor", 0.0)]),
        gate_fx(&[("mode", 1.0), ("floor", -80.0), ("look", 4.0), ("schp", 4000.0), ("sclp", 200.0)]),
    ] {
        let mut g = make(&f, sr);
        let (y, _) = run(&mut g, &x, None, 128);
        assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 1.0 + 1e-6));
        assert!(y[y.len() - 1000..].iter().all(|v| *v == 0.0), "digital silence in, digital silence out");
        assert!(g.tail_samples().is_some());
    }
}

#[test]
fn every_preset_is_in_range_and_works() {
    let sr = 48_000.0;
    let x = bursts(sr as usize, sr, 6000, 6000, 0.5, 0.002);
    let defs = crate::catalog::params(FxKind::Gate);
    let list = presets(FxKind::Gate);
    assert!(list.len() >= 8);
    let mut names: Vec<_> = list.iter().map(|p| p.name).collect();
    names.dedup();
    assert_eq!(names.len(), list.len(), "preset names are unique");
    for (i, pre) in list.iter().enumerate() {
        for (k, v) in &pre.params {
            let d = defs.iter().find(|d| d.id == *k).unwrap_or_else(|| panic!("{}: unknown `{k}`", pre.name));
            assert!((d.min..=d.max).contains(v), "{}: {k} = {v} outside {}..{}", pre.name, d.min, d.max);
        }
        let mut f = new_fx(1, FxKind::Gate, 120.0);
        assert!(load_preset(&mut f, i, 120.0));
        let (y, _) = run(&mut make(&f, sr), &x, None, 512);
        assert!(y.iter().all(|v| v.is_finite()), "{}", pre.name);
        // the loud parts pass (gates) or are turned down (duckers), the quiet ones the other way
        let duck = f.params["mode"] >= 0.5;
        let d = latency(&f, sr);
        let loud = db(rms(&y[12_000 + 1000 + d..12_000 + 5000 + d]) / rms(&x[13_000..17_000]));
        if duck {
            assert!(loud < -6.0, "{}: ducks the loud part ({loud:.1} dB)", pre.name);
        } else {
            assert!(loud > -1.0, "{}: passes the loud part ({loud:.1} dB)", pre.name);
        }
    }
}

#[test]
fn the_gate_runs_far_faster_than_real_time() {
    // the heaviest settings: side-chain filters on both sides, RMS detection, look-ahead
    let sr = 48_000.0;
    let secs = 10.0;
    let x = bursts((sr * secs) as usize, sr, 5000, 7000, 0.5, 0.01);
    let f = gate_fx(&[("schp", 100.0), ("sclp", 8000.0), ("det", 1.0), ("look", 4.0), ("ratio", 3.0)]);
    let speed = (0..3)
        .map(|_| {
            let mut g = make(&f, sr);
            let (mut l, mut r) = (x.clone(), x.clone());
            let t = std::time::Instant::now();
            for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                g.process(a, b, &ctx(sr));
            }
            secs / t.elapsed().as_secs_f32()
        })
        .fold(0.0f32, f32::max);
    // a debug build is ~20× slower than release; this only catches something pathological
    assert!(speed > 20.0, "gate: {speed:.0}× real time");
    eprintln!("gate: {speed:.0}× real time");
}
