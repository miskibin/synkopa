use super::*;
use crate::dsp::saturator::pink_noise;
use crate::mastering::measure;

const SR: f32 = 48_000.0;

fn block(kv: &[(usize, f32)]) -> ParamBlock {
    let mut b = ParamBlock { values: Master::default().v, ..Default::default() };
    for &(i, v) in kv {
        b.values[i] = v;
    }
    b
}

fn run(kv: &[(usize, f32)], l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>, Master) {
    let mut m = Master::default();
    m.set_params(&block(kv));
    m.prepare(SR, 512);
    m.set_params(&block(kv));
    let ctx = ProcessCtx { sample_rate: SR, bpm: 120.0, beat: 0.0, playing: true };
    let (mut ol, mut or) = (l.to_vec(), r.to_vec());
    for (a, b) in ol.chunks_mut(512).zip(or.chunks_mut(512)) {
        m.process(a, b, &ctx);
    }
    (ol, or, m)
}

fn sine(hz: f32, db: f32, secs: f32) -> Vec<f32> {
    let a = db_to_gain(db);
    (0..(secs * SR) as usize).map(|i| a * (2.0 * std::f32::consts::PI * hz * i as f32 / SR).sin()).collect()
}

fn rms_db(x: &[f32]) -> f32 {
    10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).log10()
}

#[test]
fn idle_master_passes_every_frequency_at_unity() {
    // default params: no tone, no glue, no clip, drive 0 under the ceiling: an all-pass
    for hz in [40.0, 120.0, 1000.0, 2800.0, 9000.0, 16000.0] {
        let x = sine(hz, -20.0, 1.0);
        let (ol, _, _) = run(&[], &x, &x);
        let d = rms_db(&ol[24000..]) - rms_db(&x[24000..]);
        assert!(d.abs() < 0.05, "{hz} Hz: {d:.3} dB");
    }
}

#[test]
fn ceiling_holds_on_loud_noise_with_and_without_the_clipper() {
    let x = pink_noise(5 * SR as usize, -14.0, 1);
    for clip in [0.0, 50.0, 100.0] {
        let (ol, or, m) = run(&[(idx::DRIVE, 12.0), (idx::CEIL, -1.0), (idx::CLIP, clip)], &x, &x);
        let (_, tp, _) = measure(&ol, &or, SR).unwrap();
        assert!(tp <= -0.9, "clip {clip}: true peak {tp:.2} dBTP");
        assert_eq!(m.limiter.clamped, 0, "clip {clip}: the safety clamp had to act");
    }
}

#[test]
fn the_clipper_takes_work_off_the_limiter() {
    let x = pink_noise(4 * SR as usize, -14.0, 2);
    let gr = |clip: f32| {
        let mut m = Master::default();
        let b = block(&[(idx::DRIVE, 10.0), (idx::CLIP, clip)]);
        m.set_params(&b);
        m.prepare(SR, 512);
        m.set_params(&b);
        let ctx = ProcessCtx { sample_rate: SR, bpm: 120.0, beat: 0.0, playing: true };
        let (mut l, mut r) = (x.clone(), x.clone());
        let mut worst = 0f32;
        for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
            m.process(a, b, &ctx);
            worst = worst.min(m.limiter.gain_reduction_db());
        }
        worst
    };
    let (off, on) = (gr(0.0), gr(80.0));
    assert!(on > off + 2.0, "limiter peak reduction: {off:.1} dB without clipper, {on:.1} dB with");
}

#[test]
fn clipper_aliasing_stays_low() {
    // a 1.7 kHz sine (mid band) driven 6 dB into the clipper: its harmonics above Nyquist must
    // not fold back below it
    let x = sine(1700.0, -3.0, 1.0);
    let (ol, _, _) = run(&[(idx::DRIVE, 6.0), (idx::CLIP, 100.0), (idx::CEIL, 0.0)], &x, &x);
    let y = &ol[8192..8192 + 16384];
    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(y.len());
    let mut buf: Vec<f32> =
        y.iter().enumerate().map(|(i, v)| v * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / y.len() as f32).cos())).collect();
    let mut spec = fft.make_output_vec();
    fft.process(&mut buf, &mut spec).unwrap();
    let hz = SR / y.len() as f32;
    let p: Vec<f32> = spec.iter().map(|c| c.norm_sqr()).collect();
    let peak = p.iter().copied().fold(0.0, f32::max);
    // everything under 6 kHz is aliasing or noise (the harmonics are 21, 35 kHz …)
    let worst =
        p.iter().enumerate().filter(|(b, _)| (*b as f32 * hz) > 100.0 && (*b as f32 * hz) < 1500.0).map(|(_, v)| *v).fold(0.0, f32::max);
    let rel = 10.0 * (worst / peak).log10();
    let top: Vec<(usize, f32)> = {
        let mut v: Vec<(usize, f32)> = p.iter().copied().enumerate().collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v.truncate(4);
        v
    };
    assert!(
        rel < -70.0,
        "worst alias {rel:.1} dB under the tone; top bins {:?}",
        top.iter().map(|(b, v)| (*b as f32 * hz, 10.0 * (v / peak).log10())).collect::<Vec<_>>()
    );
}

#[test]
fn glue_compresses_a_band_by_its_ratio() {
    // 60 Hz at −10 dBFS RMS-ish into a low threshold of −30 dB at 2:1: about −10 dB
    let x = sine(60.0, -7.0, 2.0);
    let (_, _, m) = run(&[(idx::THR, -30.0), (idx::RATIO, 2.0)], &x, &x);
    let mut out = [0f32; METERS];
    m.meters(&mut out);
    assert!((out[0] + 10.0).abs() < 1.0, "low band gain {:.2} dB", out[0]);
    assert!(out[1] > -0.1 && out[2] > -0.1, "the other bands stay idle: {:.2} {:.2}", out[1], out[2]);
}

#[test]
fn bass_mono_folds_the_low_band_only() {
    let lo = sine(50.0, -12.0, 1.0);
    let hi = sine(5000.0, -12.0, 1.0);
    let l: Vec<f32> = lo.iter().zip(&hi).map(|(a, b)| a + b).collect();
    let r: Vec<f32> = hi.clone();
    let (ol, or, _) = run(&[(idx::WIDTH, 0.0)], &l, &r);
    let diff: Vec<f32> = ol.iter().zip(&or).map(|(a, b)| a - b).collect();
    // a mono low band leaves no difference between the channels (the 5 kHz part is identical in
    // both); what is left is the 50 Hz that leaks through the 24 dB/oct crossover's upper side
    assert!(rms_db(&diff[24000..]) < rms_db(&lo) - 25.0, "L−R {:.1} dB", rms_db(&diff[24000..]));
}

#[test]
fn level_match_brings_the_output_back_to_the_input_loudness() {
    let x = pink_noise(12 * SR as usize, -20.0, 3);
    let (ol, or, m) = run(&[(idx::DRIVE, 10.0), (idx::MATCH, 1.0)], &x, &x);
    let tail = 8 * SR as usize;
    let (li, _, _) = measure(&x[tail..], &x[tail..], SR).unwrap();
    let (lo, _, _) = measure(&ol[tail..], &or[tail..], SR).unwrap();
    assert!((lo - li).abs() < 0.5, "input {li:.2} LUFS, matched output {lo:.2} LUFS");
    let mut out = [0f32; METERS];
    m.meters(&mut out);
    assert!(out[7] < -5.0, "match gain {:.1} dB", out[7]);
}

#[test]
fn reported_latency_is_where_an_impulse_comes_out() {
    let mut x = vec![0f32; 4096];
    x[100] = 0.25;
    let (ol, _, m) = run(&[], &x, &x);
    let at = ol.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
    let lat = Processor::latency_samples(&m);
    assert!((at as i64 - 100 - lat as i64).abs() <= 3, "impulse at {at}, latency {lat}");
}

#[test]
fn silence_parks_to_silence() {
    let x = vec![0f32; 48_000];
    let (ol, or, _) = run(&[(idx::DRIVE, 12.0), (idx::CLIP, 60.0), (idx::MATCH, 1.0)], &x, &x);
    assert!(ol.iter().chain(&or).all(|v| *v == 0.0));
}

#[test]
fn the_clipper_leaves_the_highs_alone() {
    // a loud 60 Hz "808" clipped hard with a quiet 9 kHz "hat" on top: a full-band clipper puts
    // sidebands on the hat (9 kHz ± 60, ± 120 Hz); the band-split clipper does not
    // (drive 0, ceiling 0: the clipper rounds the 808's peaks under the ceiling and the limiter has nothing left
    // to do, so any sideband would be the clipper's)
    let lo = sine(60.0, 1.0, 1.0);
    let hi = sine(9000.0, -50.0, 1.0);
    let x: Vec<f32> = lo.iter().zip(&hi).map(|(a, b)| a + b).collect();
    let (ol, _, m) = run(&[(idx::CLIP, 100.0), (idx::CEIL, 0.0)], &x, &x);
    assert!(m.clip_db < -1.0, "the clipper must be working: {:.2} dB", m.clip_db);
    let y = &ol[8192..8192 + 32768];
    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(y.len());
    let mut buf: Vec<f32> =
        y.iter().enumerate().map(|(i, v)| v * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / y.len() as f32).cos())).collect();
    let mut spec = fft.make_output_vec();
    fft.process(&mut buf, &mut spec).unwrap();
    let hz = SR / y.len() as f32;
    let at = |f: f32| -> f32 {
        let k = (f / hz).round() as usize;
        (k - 2..=k + 2).map(|j| spec[j].norm_sqr()).fold(0.0, f32::max)
    };
    let tone = at(9000.0);
    for side in [8880.0, 8940.0, 9060.0, 9120.0] {
        let rel = 10.0 * (at(side) / tone).log10();
        // what is left is the hat's −40 dB copy the 24 dB/oct crossover leaves in the mid band
        assert!(rel < -50.0, "sideband at {side} Hz: {rel:.1} dB under the hat");
    }
}

#[test]
fn band_levels_move_their_band_only() {
    for (hz, b) in [(50.0, 0usize), (800.0, 1), (9000.0, 2)] {
        let x = sine(hz, -24.0, 1.0);
        let (o0, _, _) = run(&[], &x, &x);
        let (o1, _, _) = run(&[(idx::BAND_GAIN + b, 6.0)], &x, &x);
        let (o2, _, _) = run(&[(idx::BAND_GAIN + (b + 1) % 3, 6.0)], &x, &x);
        let d = rms_db(&o1[24000..]) - rms_db(&o0[24000..]);
        let other = rms_db(&o2[24000..]) - rms_db(&o0[24000..]);
        assert!((d - 6.0).abs() < 0.3, "{hz} Hz: own band +{d:.2} dB");
        assert!(other.abs() < 0.3, "{hz} Hz: another band's level moved it {other:.2} dB");
    }
}

#[test]
fn each_band_has_its_own_width() {
    // a wide signal in every band (opposite-polarity sines in L and R): widening only the highs
    // and narrowing only the lows changes the side energy of those bands alone
    let side_in = |hz: f32| -> (Vec<f32>, Vec<f32>) {
        let x = sine(hz, -18.0, 1.0);
        (x.clone(), x.iter().map(|v| -v * 0.5).collect())
    };
    let side_db = |l: &[f32], r: &[f32]| {
        let s: Vec<f32> = l.iter().zip(r).map(|(a, b)| 0.5 * (a - b)).collect();
        rms_db(&s[24000..])
    };
    for (hz, band, want) in [(50.0, 0usize, 0.0f32), (800.0, 1, 100.0), (9000.0, 2, 150.0)] {
        let (l, r) = side_in(hz);
        let (ol0, or0, _) = run(&[], &l, &r);
        let w = [(idx::WIDTH, 0.0), (idx::WIDTH + 1, 100.0), (idx::WIDTH + 2, 150.0)];
        let (ol, or, _) = run(&w, &l, &r);
        let d = side_db(&ol, &or) - side_db(&ol0, &or0);
        let expect = if want <= 0.0 { f32::NEG_INFINITY } else { 20.0 * (want / 100.0).log10() };
        if expect.is_finite() {
            assert!((d - expect).abs() < 0.3, "band {band} at {hz} Hz: side {d:.2} dB, want {expect:.2}");
        } else {
            assert!(d < -25.0, "band {band} at {hz} Hz: side only {d:.2} dB down at 0 %");
        }
    }
}

#[test]
fn glue_time_scales_how_fast_the_bands_clamp_down() {
    // a burst into a low threshold: 15 ms in, a slow glue has reduced less than a fast one
    let mut x = vec![0f32; 24_000];
    x.extend(sine(800.0, -6.0, 0.5));
    let gr_after = |time: f32| {
        let mut m = Master::default();
        let b = block(&[(idx::THR + 1, -30.0), (idx::RATIO, 2.0), (idx::TIME, time)]);
        m.set_params(&b);
        m.prepare(SR, 64);
        m.set_params(&b);
        let ctx = ProcessCtx { sample_rate: SR, bpm: 120.0, beat: 0.0, playing: true };
        let (mut l, mut r) = (x.clone(), x.clone());
        let stop = 24_000 + (0.015 * SR) as usize;
        for (a, b) in l[..stop].chunks_mut(64).zip(r[..stop].chunks_mut(64)) {
            m.process(a, b, &ctx);
        }
        m.gr_band[1]
    };
    let (fast, slow) = (gr_after(25.0), gr_after(400.0));
    assert!(fast < slow - 2.0, "15 ms into the burst: fast glue {fast:.2} dB, slow glue {slow:.2} dB");
}
