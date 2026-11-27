use super::*;
use crate::processor::MAX_PARAMS;

/// Parameter values: the catalog defaults with `kv` on top.
fn values(kv: &[(&str, f32)]) -> [f32; MAX_PARAMS] {
    let mut v = [0.0f32; MAX_PARAMS];
    for (i, d) in crate::catalog::params(crate::model::FxKind::Patina).iter().enumerate() {
        v[i] = d.def as f32;
    }
    for (k, x) in kv {
        let i =
            crate::catalog::params(crate::model::FxKind::Patina).iter().position(|d| d.id == *k).unwrap_or_else(|| panic!("no param {k}"));
        v[i] = *x;
    }
    v
}

/// Run `x` (left = right unless `xr`) through Patina; returns (left, right).
fn run_lr(kv: &[(&str, f32)], xl: &[f32], xr: Option<&[f32]>, sr: f32, block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut p = Patina::default();
    p.prepare(sr, block);
    p.set_params(&ParamBlock { values: values(kv), ..Default::default() });
    let mut l = xl.to_vec();
    let mut r = xr.unwrap_or(xl).to_vec();
    let ctx = ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: true };
    for (a, b) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        p.process(a, b, &ctx);
    }
    (l, r)
}

fn run(kv: &[(&str, f32)], x: &[f32], sr: f32) -> Vec<f32> {
    run_lr(kv, x, None, sr, 256).0
}

fn sine(f: f32, amp: f32, sr: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| (TAU * f * i as f32 / sr).sin() * amp).collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len().max(1) as f64).sqrt() as f32
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

#[test]
fn modules_at_zero_leave_the_level_alone() {
    let sr = 48000.0;
    for f in [60.0, 440.0, 3000.0, 12000.0] {
        let x = sine(f, 0.5, sr, 24000);
        let y = run(&[], &x, sr);
        let d = db(rms(&y[4000..])) - db(rms(&x[4000..]));
        assert!(d.abs() < 0.3, "{f} Hz: {d:.2} dB");
    }
}

#[test]
fn magnitude_zero_is_clean_even_with_every_module_up() {
    let sr = 48000.0;
    let all = [
        ("amount", 0.0),
        ("noise", 100.0),
        ("wobble", 100.0),
        ("drive", 100.0),
        ("digital", 100.0),
        ("bits", 4.0),
        ("wear", 100.0),
        ("drops", 100.0),
        ("lowcut", 800.0),
        ("highcut", 2000.0),
        ("tone", 100.0),
    ];
    let x = sine(440.0, 0.5, sr, 48000);
    let y = run(&all, &x, sr);
    let lat = latency_samples();
    // the output is the input, late by the reported latency (the Drive's oversampler is a
    // near-linear-phase filter, so this holds to a small error)
    let err: Vec<f32> = (lat + 2000..x.len()).map(|i| y[i] - x[i - lat]).collect();
    assert!(db(rms(&err)) - db(rms(&x)) < -30.0, "residual {:.1} dB", db(rms(&err)) - db(rms(&x)));
    assert!(y.iter().all(|v| v.is_finite()));
}

#[test]
fn every_noise_type_has_a_sane_level_at_any_sample_rate() {
    for (ty, name) in NOISE_TYPES.iter().enumerate() {
        let mut lv = [0.0f32; 2];
        for (k, sr) in [44100.0f32, 96000.0].into_iter().enumerate() {
            let x = vec![0.0; (sr * 3.0) as usize];
            let y = run(&[("noise", 100.0), ("ntype", ty as f32)], &x, sr);
            let y = &y[(sr * 0.5) as usize..];
            assert!(y.iter().all(|v| v.is_finite()));
            lv[k] = db(rms(y));
            let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak < 1.0, "{name}: peak {peak}");
        }
        assert!((-50.0..-18.0).contains(&lv[0]), "{name}: {:.1} dBFS", lv[0]);
        assert!((lv[0] - lv[1]).abs() < 2.0, "{name}: 44.1k {:.1} vs 96k {:.1} dB", lv[0], lv[1]);
    }
}

#[test]
fn half_noise_is_well_below_full() {
    let sr = 48000.0;
    let x = vec![0.0; 96000];
    let full = rms(&run(&[("noise", 100.0), ("ntype", 3.0)], &x, sr)[24000..]);
    let half = rms(&run(&[("noise", 50.0), ("ntype", 3.0)], &x, sr)[24000..]);
    let d = db(full) - db(half);
    assert!((6.0..12.0).contains(&d), "{d:.1} dB");
}

#[test]
fn follow_silences_the_noise_in_silence_and_duck_lowers_it_under_music() {
    let sr = 48000.0;
    let x = vec![0.0; 48000];
    let y = run(&[("noise", 100.0), ("ntype", 3.0), ("nfollow", 100.0)], &x, sr);
    assert!(rms(&y[12000..]) < 1e-6);
    let loud = sine(200.0, 0.5, sr, 96000);
    let plain = run(&[("noise", 100.0), ("ntype", 3.0)], &loud, sr);
    let ducked = run(&[("noise", 100.0), ("ntype", 3.0), ("nfollow", -100.0)], &loud, sr);
    // the noise is what is left after taking the tone away: compare above the tone
    let hi = |y: &[f32]| {
        let mut s = Svf::default();
        s.set(4000.0, 0.7, sr);
        let h: Vec<f32> = y.iter().map(|v| s.tick(*v).2).collect();
        rms(&h[24000..])
    };
    assert!(db(hi(&plain)) - db(hi(&ducked)) > 12.0, "duck {:.1} dB", db(hi(&plain)) - db(hi(&ducked)));
}

#[test]
fn crackle_is_made_of_distinct_clicks() {
    let sr = 48000.0;
    let mut p = Patina::default();
    p.prepare(sr, 256);
    p.set_params(&ParamBlock { values: values(&[("noise", 100.0), ("ntype", 1.0)]), ..Default::default() });
    let ctx = ProcessCtx { sample_rate: sr, ..Default::default() };
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    for _ in 0..(sr as usize * 2 / 256) {
        l.fill(0.0);
        r.fill(0.0);
        p.process(&mut l, &mut r, &ctx);
    }
    let mut m = [0.0; METERS];
    p.meters(&mut m);
    // dusty vinyl: 160 ticks, 30 clicks, 1.8 pops a second and 5 scratches every revolution (≈ 195/s)
    let n = m[meter::CLICKS];
    assert!((300.0..500.0).contains(&n), "{n} clicks in 2 s");
}

#[test]
fn wobble_swings_the_pitch_by_the_set_depth_and_keeps_its_centre() {
    let sr = 48000.0;
    let n = (sr * 8.0) as usize;
    let x = sine(3150.0, 0.5, sr, n);
    let amount = 0.6f32;
    let y = run(&[("wobble", amount * 100.0), ("flutter", 0.0), ("wander", 0.0), ("wrate", 1.0)], &x, sr);
    // frequency from the spacing of rising zero crossings
    let mut cross = Vec::new();
    for i in 1..n {
        if y[i - 1] < 0.0 && y[i] >= 0.0 {
            cross.push((i - 1) as f64 + (-y[i - 1] / (y[i] - y[i - 1])) as f64);
        }
    }
    let freqs: Vec<f64> = cross.windows(2).skip(200).map(|w| sr as f64 / (w[1] - w[0])).collect();
    let mean = freqs.iter().sum::<f64>() / freqs.len() as f64;
    let dev = freqs.iter().fold(0.0f64, |m, f| m.max((f / mean - 1.0).abs())) as f32;
    let want = wow_depth(amount);
    assert!(((mean / 3150.0) - 1.0).abs() < 2e-4, "centre moved: {mean:.2} Hz");
    assert!(dev > want * 0.5 && dev < want * 1.4, "deviation {:.3} % for {:.3} %", dev * 100.0, want * 100.0);
}

#[test]
fn digital_quantises_to_the_bit_depth() {
    let sr = 48000.0;
    let x = sine(220.0, 0.8, sr, 24000);
    let y = run(&[("digital", 100.0), ("bits", 4.0), ("srate", 48000.0), ("dsmooth", 0.0), ("wander", 0.0)], &x, sr);
    for v in &y[4000..] {
        let q = v * 8.0;
        assert!((q - q.round()).abs() < 2e-3, "{v} is not a 4-bit level");
    }
}

#[test]
fn digital_rate_folds_highs_down() {
    let sr = 48000.0;
    // 7 kHz held at 8 kHz: an image appears at 1 kHz
    let x = sine(7000.0, 0.5, sr, 48000);
    let y = run(&[("digital", 100.0), ("bits", 16.0), ("srate", 8000.0), ("dsmooth", 0.0), ("wander", 0.0)], &x, sr);
    let bin = |y: &[f32], f: f32| {
        let (mut re, mut im) = (0f64, 0f64);
        for (i, &s) in y.iter().enumerate() {
            let ph = std::f64::consts::TAU * f as f64 * i as f64 / sr as f64;
            re += s as f64 * ph.cos();
            im += s as f64 * ph.sin();
        }
        ((re * re + im * im).sqrt() * 2.0 / y.len() as f64) as f32
    };
    assert!(bin(&y[8000..], 1000.0) > 0.05, "no alias at 1 kHz");
}

#[test]
fn wear_rolls_off_the_highs_and_keeps_the_lows() {
    let sr = 48000.0;
    for (f, lo, hi) in [(100.0, -1.0, 4.0), (10000.0, -60.0, -12.0)] {
        let x = sine(f, 0.1, sr, 24000);
        let y = run(&[("wear", 100.0), ("wander", 0.0)], &x, sr);
        let d = db(rms(&y[6000..])) - db(rms(&x[6000..]));
        assert!(d > lo && d < hi, "{f} Hz: {d:.1} dB");
    }
}

#[test]
fn dropouts_dip_the_level_now_and_then() {
    let sr = 48000.0;
    let x = sine(1000.0, 0.3, sr, (sr * 10.0) as usize);
    let y = run(&[("drops", 100.0), ("wander", 0.0)], &x, sr);
    let win: Vec<f32> = y.chunks(480).skip(10).map(rms).collect();
    let mut s = win.clone();
    s.sort_by(f32::total_cmp);
    let median = s[s.len() / 2];
    let lowest = s[0];
    assert!(db(median) - db(lowest) > 6.0, "deepest dip {:.1} dB", db(median) - db(lowest));
    // most of the time the tape plays normally
    assert!(db(median) - db(rms(&x)) > -2.0);
}

#[test]
fn band_split_sums_flat_with_a_clean_drive() {
    let sr = 48000.0;
    for f in [50.0, 300.0, 1000.0, 3000.0, 9000.0] {
        let x = sine(f, 0.3, sr, 24000);
        let y = run(&[("dlow", 300.0), ("dhigh", 3000.0)], &x, sr);
        let d = db(rms(&y[6000..])) - db(rms(&x[6000..]));
        assert!(d.abs() < 0.3, "{f} Hz: {d:.2} dB");
    }
}

#[test]
fn drive_only_touches_its_band() {
    let sr = 48000.0;
    // a hot 80 Hz tone below a 400 Hz – 6 kHz band: no harmonics are added to it
    let x = sine(80.0, 0.9, sr, 48000);
    let y = run(&[("drive", 100.0), ("dstyle", 4.0), ("dlow", 400.0), ("dhigh", 6000.0), ("wander", 0.0)], &x, sr);
    let full = run(&[("drive", 100.0), ("dstyle", 4.0), ("wander", 0.0)], &x, sr);
    // 48 dB/oct from 1 kHz: what is left is what the drive added
    let hp = |y: &[f32]| {
        let mut s = [Svf::default(); 4];
        for f in &mut s {
            f.set(1000.0, 0.7, sr);
        }
        let h: Vec<f32> = y.iter().map(|v| s.iter_mut().fold(*v, |a, f| f.tick(a).2)).collect();
        rms(&h[12000..])
    };
    assert!(db(hp(&full)) - db(hp(&y)) > 20.0, "band-limited drive still adds {:.1} dB of harmonics", db(hp(&y)));
}

#[test]
fn extremes_stay_finite_at_any_block_size() {
    let sr = 44100.0;
    let x: Vec<f32> = sine(97.0, 1.5, sr, 30000).iter().zip(sine(5333.0, 0.8, sr, 30000)).map(|(a, b)| a + b).collect();
    let kv = [
        ("noise", 100.0),
        ("ntype", 2.0),
        ("nroute", 1.0),
        ("nfollow", -100.0),
        ("wobble", 100.0),
        ("wrate", 0.1),
        ("flutter", 100.0),
        ("wstereo", 100.0),
        ("drive", 100.0),
        ("dlow", 2000.0),
        ("dhigh", 500.0),
        ("digital", 100.0),
        ("srate", 1000.0),
        ("bits", 2.0),
        ("dsmooth", 100.0),
        ("wear", 100.0),
        ("drops", 100.0),
        ("dstereo", 1.0),
        ("wander", 100.0),
        ("lowcut", 1000.0),
        ("highcut", 1000.0),
        ("tone", -100.0),
        ("width", 200.0),
        ("out", 12.0),
    ];
    for block in [1, 17, 64, 512] {
        let (l, r) = run_lr(&kv, &x, None, sr, block);
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 50.0), "block {block}");
    }
}

#[test]
fn no_parameter_is_left_unread() {
    // catalog order and dsp::patina::idx agree
    let defs = crate::catalog::params(crate::model::FxKind::Patina);
    assert!(defs.len() <= MAX_PARAMS);
    let ids = [
        (idx::AMOUNT, "amount"),
        (idx::NOISE, "noise"),
        (idx::NTYPE, "ntype"),
        (idx::NFOLLOW, "nfollow"),
        (idx::NROUTE, "nroute"),
        (idx::WOBBLE, "wobble"),
        (idx::WSTEREO, "wstereo"),
        (idx::DRIVE, "drive"),
        (idx::DHIGH, "dhigh"),
        (idx::DIGITAL, "digital"),
        (idx::DSMOOTH, "dsmooth"),
        (idx::WEAR, "wear"),
        (idx::DSTEREO, "dstereo"),
        (idx::WANDER, "wander"),
        (idx::OUT, "out"),
    ];
    for (i, id) in ids {
        assert_eq!(defs[i].id, id);
    }
    assert_eq!(defs.len(), idx::OUT + 1);
}
