use super::*;

const SR: f32 = 48_000.0;

/// Deterministic noise shaped to `tilt_db(f)` (dB, third-octave band level) by summing
/// sines on a log grid: every third-octave gets the same count, so the band level follows
/// the per-sine amplitude.
fn shaped(secs: f32, tilt: impl Fn(f64) -> f64, seed: u32) -> Vec<f32> {
    let n = (secs * SR) as usize;
    let mut out = vec![0f32; n];
    let mut s = seed.wrapping_mul(2_654_435_761).max(1);
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s as f32 / u32::MAX as f32
    };
    let per_third = 12;
    for k in 0..(31 * per_third) {
        let f = 20.0 * 2f64.powf(k as f64 / (3.0 * per_third as f64));
        if f > 20_000.0 {
            break;
        }
        let a = 10f64.powf(tilt(f) / 20.0) as f32 * 0.01;
        let ph = rnd() * std::f32::consts::TAU;
        let w = std::f32::consts::TAU * f as f32 / SR;
        for (i, o) in out.iter_mut().enumerate() {
            *o += a * (w * i as f32 + ph).sin();
        }
    }
    out
}

#[test]
fn a_mix_on_the_reference_balance_gets_no_tone_correction() {
    let x = shaped(4.0, reference_db, 1);
    let bal = balance(&spectrum(&x, &x, SR));
    let mid: Vec<f64> = bal.iter().zip(THIRDS).filter(|(_, f)| (63.0..=12500.0).contains(f)).map(|(b, _)| *b).collect();
    assert!(mid.iter().all(|b| b.abs() < 1.5), "balance {mid:?}");
    assert_eq!(tone_gains(&bal), [0.0; 5]);
}

#[test]
fn a_bright_boomy_mix_is_corrected_the_right_way_and_only_partly() {
    let x = shaped(4.0, |f| reference_db(f) + if f > 7000.0 { 8.0 } else { 0.0 } + if (120.0..=320.0).contains(&f) { 6.0 } else { 0.0 }, 2);
    let g = tone_gains(&balance(&spectrum(&x, &x, SR)));
    assert!(g[4] < -1.0 && g[4] >= -3.0, "air {g:?}");
    assert!(g[1] < -0.5 && g[1] >= -3.0, "low {g:?}");
    assert!(g[2].abs() <= 0.5 && g[3].abs() <= 0.5, "mid and presence stay: {g:?}");
}

#[test]
fn a_genre_s_big_sub_is_mostly_left_alone() {
    // +4 dB of sub over the reference (an 808-heavy mix) is inside the dead zone
    let x = shaped(4.0, |f| reference_db(f) + if f < 90.0 { 4.0 } else { 0.0 }, 3);
    let g = tone_gains(&balance(&spectrum(&x, &x, SR)));
    assert!(g[0].abs() < 0.3, "sub {g:?}");
}

/// A short "song": a quiet intro, then drums (noise bursts) over a bass line and a pad.
fn song() -> (Vec<f32>, Vec<f32>) {
    let n = (16.0 * SR) as usize;
    let mut l = vec![0f32; n];
    let mut r = vec![0f32; n];
    let mut s = 12345u32;
    for i in 0..n {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        let noise = s as f32 / u32::MAX as f32 * 2.0 - 1.0;
        let t = i as f32 / SR;
        let beat = (t * 2.0).fract();
        let intro = if t < 2.0 { 0.15 } else { 1.0 };
        let kick = (-beat * 30.0).exp() * (std::f32::consts::TAU * 55.0 * t).sin() * 0.5;
        let hat = if (t * 4.0).fract() < 0.05 { noise * 0.15 } else { 0.0 };
        let bass = (std::f32::consts::TAU * 82.4 * t).sin() * 0.12;
        let pad = (std::f32::consts::TAU * 440.0 * t).sin() * 0.04 + (std::f32::consts::TAU * 660.0 * t).sin() * 0.03;
        let wide = (std::f32::consts::TAU * 1320.0 * t).sin() * 0.02;
        l[i] = intro * (kick + hat + bass + pad + wide);
        r[i] = intro * (kick + hat * 0.7 + bass + pad - wide);
    }
    (l, r)
}

#[test]
fn listen_reaches_the_target_under_the_ceiling() {
    let (l, r) = song();
    let cur = crate::catalog::default_params(FxKind::Master);
    for target in [1usize, 3] {
        let got = listen(&l, &r, SR, &cur, target, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let rep = &got.report;
        let goal = f64::from(TARGETS[target]);
        assert!((rep.out_lufs - goal).abs() <= 0.3, "target {goal}: got {} LUFS", rep.out_lufs);
        assert!(rep.out_tp <= -0.9, "true peak {}", rep.out_tp);
        assert!(!rep.short);
        assert!(rep.in_lufs < rep.out_lufs);
        assert_eq!(rep.balance.len(), THIRDS.len());
        // the result is what the effect does with the returned params
        let (ol, or) = render_stereo_with(&mut Master::default(), FxKind::Master, &got.params, &l, &r, SR);
        let (lufs, _, _) = measure(&ol, &or, SR).unwrap();
        assert!((lufs - rep.out_lufs).abs() < 0.1);
    }
}

#[test]
fn listen_keeps_the_user_s_ceiling_and_sets_glue() {
    let (l, r) = song();
    let mut cur = crate::catalog::default_params(FxKind::Master);
    cur.insert("ceil".into(), -2.0);
    let got = listen(&l, &r, SR, &cur, 2, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(got.params["ceil"], -2.0);
    assert!(got.report.out_tp <= -1.9, "{}", got.report.out_tp);
    for k in ["tl", "tm", "th"] {
        assert!(got.params[k] < 0.0 && got.params[k] > -40.0, "{k} = {}", got.params[k]);
    }
}

#[test]
fn listen_refuses_silence_and_stops_when_cancelled() {
    let z = vec![0f32; 48_000];
    let cur = crate::catalog::default_params(FxKind::Master);
    assert!(listen(&z, &z, SR, &cur, 2, &AtomicBool::new(false), &mut |_| {}).unwrap_err().contains("silent"));
    let (l, r) = song();
    let e = listen(&l, &r, SR, &cur, 2, &AtomicBool::new(true), &mut |_| {}).unwrap_err();
    assert_eq!(e, "cancelled");
}

#[test]
fn an_impossible_target_is_reported_short() {
    // a sine already at the ceiling cannot get 30 LU louder
    let x: Vec<f32> = (0..(4.0 * SR) as usize).map(|i| 0.01 * (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()).collect();
    let cur = crate::catalog::default_params(FxKind::Master);
    let got = listen(&x, &x, SR, &cur, 4, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert!(got.report.short, "{:?}", got.report.out_lufs);
    assert!(!got.report.guarded, "a quiet sine has no peaks to cut: it is short, not guarded");
    assert_eq!(got.params["drive"], 24.0);
}

#[test]
fn the_master_keeps_the_mix_balance_apart_from_its_tone_eq() {
    // a mix arriving hot (peaks far over the ceiling) is limited hard; the output's balance must
    // still be the input's plus the tone EQ the assistant chose, within a dB per tone band
    let (l, r) = song();
    let (l, r): (Vec<f32>, Vec<f32>) = (l.iter().map(|v| v * 4.0).collect(), r.iter().map(|v| v * 4.0).collect());
    let cur = crate::catalog::default_params(FxKind::Master);
    let got = listen(&l, &r, SR, &cur, 3, &AtomicBool::new(false), &mut |_| {}).unwrap();
    let rep = &got.report;
    let tone = tone_now(&got.params);
    let drift = tone_drift(&rep.balance, &rep.balance_out, &tone, SR);
    for (b, d) in drift.iter().enumerate().skip(1) {
        assert!(d.abs() < 1.0, "tone band {b} drifted {d:.2} dB (drift {drift:?}, tone {tone:?})");
    }
    assert!((rep.out_lufs - rep.aim).abs() <= 0.3, "{} vs aim {}", rep.out_lufs, rep.aim);
}

#[test]
fn a_target_that_would_squash_the_song_stops_at_the_loudest_clean_level() {
    // a peaky mix asked for Club −8: the peaks would have to come down far more than stays clean
    let (l, r) = song();
    let cur = crate::catalog::default_params(FxKind::Master);
    let got = listen(&l, &r, SR, &cur, 4, &AtomicBool::new(false), &mut |_| {}).unwrap();
    let rep = &got.report;
    assert!(rep.guarded, "{rep:?}");
    assert!(rep.peak_cut > MAX_CLEAN_CUT_DB, "{}", rep.peak_cut);
    assert!(rep.aim < f64::from(TARGETS[4]) - 0.5, "aim {}", rep.aim);
    assert!((rep.out_lufs - rep.aim).abs() <= 0.3, "{} vs aim {}", rep.out_lufs, rep.aim);
    assert!(!rep.short, "a guarded listen is not a failed one");
    assert!(got.params["clip"] <= 35.0, "clip {}", got.params["clip"]);
    // what it aimed at is what stays clean: the peak cut it does is the clean maximum
    let peak = robust_peak_db(&l, &r, SR);
    let did = (peak - rep.in_lufs) - (-1.0 - rep.aim);
    assert!((did - MAX_CLEAN_CUT_DB).abs() < 0.2, "{did}");
}

#[test]
fn widths_follow_how_wide_each_band_arrives() {
    // a wide low end goes mono, normal mids stay, narrow highs open up a little
    assert_eq!(band_widths([-8.0, -12.0, -20.0]), [0.0, 100.0, 130.0]);
    // a mono-safe low end and already wide highs are left alone; phasey mids narrow
    assert_eq!(band_widths([-40.0, -2.0, -6.0]), [100.0, 85.0, 100.0]);
}
