//! The Master's assistant: listen to the whole song as it reaches the effect and set every
//! control of `dsp::master` from what it hears. Offline only, never on the audio thread.
//!
//! What "AI mastering" products do is, under the marketing, measurement plus rules: Ozone's
//! Master Assistant classifies the genre with a small network only to pick a target tonal
//! curve, then fits its EQ to that curve, uses the crest factor to decide whether dynamics
//! help, and drives the maximizer to a loudness target (iZotope's own documentation). LANDR
//! and the like do not publish theirs. This assistant does the measurable part, deterministic
//! and explainable, over the *whole* song instead of a 30 s excerpt:
//!
//! 1. **Measure** the input: integrated loudness, loudness range and true peak (BS.1770, the
//!    engine's own meter), and its long-term spectrum in third octaves, over the loud parts
//!    only (frames more than 20 dB under the song's loud level are left out, so an intro or
//!    a breakdown does not tilt the balance).
//! 2. **Tone.** The spectrum is compared with a reference balance: third-octave level falling
//!    1.5 dB per octave (the −4.5 dB/oct per-Hz slope analysers tilt by so that typical
//!    commercial mixes read flat), rolling off under 40 Hz and over 12.5 kHz. The level of
//!    the reference is fitted to the mix between 100 Hz and 5 kHz. Each tone band corrects
//!    half of the deviation beyond a dead zone, at most 3 dB: a master fixes what is clearly
//!    off and leaves a genre's character (a trap mix's 808, a dark ballad) mostly alone.
//! 3. **Glue.** With the tone applied, every band's detector is traced over the song. Each
//!    threshold is the one at which that band's louder half (of its active part: within 20 dB of
//!    its loud level) gets 1.5 dB of 2:1 on average — the same amount in every band, so the glue
//!    holds the song together without tilting it — but never more than 10 dB under the band's
//!    loud level, so a band of short hits is not squashed on its peaks.
//!
//!    **Width per band** comes from each band's side-to-mid energy ([`band_widths`]): wide lows
//!    go mono, phasey mids narrow a little, narrow highs open up (≤ 130 %).
//! 4. **Clipper and loudness.** From the song's peak-to-loudness ratio and the target's, the
//!    peak reduction the master must do is known; when it is more than 2 dB the clipper takes the
//!    tallest transients of it (FL- and Ableton-style "clip, then limit"), so the limiter does not
//!    pump. The limiter's input gain is then searched (secant steps over real
//!    renders of the effect) until the output's integrated loudness is within 0.1 LU of the
//!    target, or the limiter's 24 dB are used up.
//! 5. **Keep the balance.** Heavy limiting ducks whatever plays with the kick and the bass, which
//!    tilts the long-term spectrum (measured: ±1.5–3 dB on a trap mix arriving 9 dB over the
//!    ceiling). The output's balance is compared with the input's plus the tone EQ; what moved
//!    beyond 0.5 dB is taken back with the tone bands (at most 2 dB each), and the loudness is
//!    solved again.
//!
//! The ceiling, the crossovers, the width and level match stay what the user set.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use crate::autovocal::render_stereo_with;
use crate::dsp::loudness::Loudness;
use crate::dsp::master::{self, Master, TARGETS, TONE_BANDS};
use crate::model::{FxKind, MasterListen};
use crate::processor::{ParamBlock, ProcessCtx, Processor, MAX_PARAMS, METERS};

/// Third-octave centres the balance is reported at, Hz.
pub const THIRDS: [f64; 31] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0,
    2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
];

/// Reference third-octave balance, dB relative (see the module docs).
pub fn reference_db(f: f64) -> f64 {
    let mut v = -1.5 * (f / 1000.0).log2();
    if f < 40.0 {
        v -= 12.0 * (40.0 / f).log2();
    }
    if f > 12_500.0 {
        v -= 12.0 * (f / 12_500.0).log2();
    }
    v
}

/// Tone bands: (lowest, highest) third-octave centre averaged, dead zone below / above (dB),
/// most cut, most boost.
const TONE_FIT: [(f64, f64, f64, f64, f64, f64); 5] = [
    (31.5, 80.0, 4.0, 5.0, 3.0, 2.0),
    (125.0, 315.0, 2.5, 2.5, 3.0, 3.0),
    (500.0, 1250.0, 2.0, 2.0, 3.0, 3.0),
    (2500.0, 5000.0, 2.0, 2.0, 3.0, 3.0),
    (8000.0, 12500.0, 3.0, 3.0, 3.0, 3.0),
];
const TONE_SHARE: f64 = 0.5;
/// The band widths (%) for the stereo the song arrives with (side against mid energy per band,
/// dB). Mastering practice: the lows go narrow or mono when they carry real side energy
/// (phase trouble on mono systems, a wandering kick), the mids stay, unless they are so wide
/// they are phasey, and narrow highs open up a little — never past 130 %, so the mono fold-down
/// loses at most a fraction of a dB of the highs' level.
pub fn band_widths(side_db: [f32; 3]) -> [f64; 3] {
    let lo = if side_db[0] > -15.0 {
        0.0
    } else if side_db[0] > -25.0 {
        50.0
    } else {
        100.0
    };
    let mid = if side_db[1] > -3.0 { 85.0 } else { 100.0 };
    let hi = if side_db[2] < -18.0 {
        130.0
    } else if side_db[2] < -12.0 {
        120.0
    } else if side_db[2] < -8.0 {
        110.0
    } else {
        100.0
    };
    [lo, mid, hi]
}
const RATIO: f64 = 2.0;
/// Most limiter input gain (= the parameter's range).
const DRIVE_MAX: f64 = 24.0;
const TOLERANCE_LU: f64 = 0.1;
/// Most peak reduction (clipper + limiter, dB) the assistant asks for. Past it a master is
/// audibly squashed (listening tests on a trap mix: 6.7 dB clean, 8.7 dB crunchy and pumping),
/// so a louder target stops here and says so; the Loudness knob still goes further by hand.
pub const MAX_CLEAN_CUT_DB: f64 = 7.5;
/// The clipper takes this share of the peak reduction beyond its first 2 dB, at most
/// [`MAX_CLIP`] % (67 % crunched the 808; 30 % relieved the limiter without grit).
const CLIP_PER_DB: f64 = 6.0;
const MAX_CLIP: f64 = 35.0;
const MAX_RENDERS: usize = 7;

/// Integrated loudness, true peak, loudness range of a stereo signal. `None` = silent.
pub fn measure(l: &[f32], r: &[f32], sr: f32) -> Option<(f64, f64, f64)> {
    let mut m = Loudness::default();
    m.prepare(sr, 4096);
    let ctx = ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: true };
    let (mut bl, mut br) = (vec![0f32; 4096], vec![0f32; 4096]);
    let n = l.len().min(r.len());
    let mut i = 0;
    while i < n {
        let k = 4096.min(n - i);
        bl[..k].copy_from_slice(&l[i..i + k]);
        br[..k].copy_from_slice(&r[i..i + k]);
        m.process(&mut bl[..k], &mut br[..k], &ctx);
        i += k;
    }
    let mut out = [0f32; METERS];
    m.meters(&mut out);
    if out[2] <= crate::dsp::loudness::FLOOR {
        return None;
    }
    let tp = true_peak(l).max(true_peak(r));
    let tp_db = if tp > 1e-10 { 20.0 * f64::from(tp).log10() } else { f64::from(crate::dsp::loudness::FLOOR) };
    Some((f64::from(out[2]), tp_db, f64::from(out[3].max(0.0))))
}

/// True peak, 16× oversampled (Kaiser β 12 windowed sinc, ±32 taps): exact enough to check a
/// limiter's ceiling to a hundredth of a dB (the live meter's 4× interpolator reads a few
/// tenths high on content near Nyquist). Only samples within 4.5 dB of the largest one are
/// interpolated around: an inter-sample peak is at most ~3 dB over its neighbours.
pub fn true_peak(x: &[f32]) -> f32 {
    const OS: usize = 16;
    const HALF: usize = 32;
    use std::sync::OnceLock;
    static H: OnceLock<Vec<[f32; 2 * HALF]>> = OnceLock::new();
    let h = H.get_or_init(|| {
        let i0 = |x: f64| {
            let (mut s, mut t) = (1.0, 1.0);
            for k in 1..40 {
                t *= (x / 2.0) / k as f64;
                s += t * t;
            }
            s
        };
        (0..OS)
            .map(|k| {
                let mut row = [0f32; 2 * HALF];
                for (j, v) in row.iter_mut().enumerate() {
                    let t = (j as f64 - HALF as f64) + k as f64 / OS as f64;
                    let r = t / (HALF as f64 + 1.0);
                    let sinc = if t == 0.0 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
                    *v = (sinc * i0(12.0 * (1.0 - r * r).max(0.0).sqrt()) / i0(12.0)) as f32;
                }
                row
            })
            .collect()
    });
    let sample_peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
    let screen = sample_peak * 0.6;
    let mut pk = sample_peak;
    for i in HALF..x.len().saturating_sub(HALF) {
        if x[i].abs() < screen && x[i + 1].abs() < screen {
            continue;
        }
        for row in h.iter() {
            let v: f32 = row.iter().enumerate().map(|(j, c)| c * x[i + HALF - j]).sum();
            pk = pk.max(v.abs());
        }
    }
    pk
}

/// Long-term third-octave spectrum (dB, one per [`THIRDS`]) of the loud parts of `l`/`r`.
pub fn spectrum(l: &[f32], r: &[f32], sr: f32) -> Vec<f64> {
    const N: usize = 8192;
    let n = l.len().min(r.len());
    let mut planner = realfft::RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    let win: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos()).collect();
    let mut buf = fft.make_input_vec();
    let mut spec = fft.make_output_vec();
    let mut frames: Vec<(f64, Vec<f32>)> = vec![];
    let mut i = 0;
    while i + N <= n.max(N) {
        let mut e = 0f64;
        for k in 0..N {
            let x = if i + k < n { 0.5 * (l[i + k] + r[i + k]) } else { 0.0 };
            e += f64::from(x * x);
            buf[k] = x * win[k];
        }
        if fft.process(&mut buf, &mut spec).is_ok() {
            frames.push((e / N as f64, spec.iter().map(|c| c.norm_sqr()).collect()));
        }
        i += N / 2;
    }
    let mut energies: Vec<f64> = frames.iter().map(|f| f.0).filter(|e| *e > 1e-12).collect();
    energies.sort_by(f64::total_cmp);
    let gate = energies.get(energies.len().saturating_sub(1) * 95 / 100).map_or(f64::INFINITY, |e| e * 0.01);
    let bins = N / 2 + 1;
    let mut acc = vec![0f64; bins];
    for (e, p) in &frames {
        if *e >= gate {
            for (a, v) in acc.iter_mut().zip(p) {
                *a += f64::from(*v);
            }
        }
    }
    let hz = f64::from(sr) / N as f64;
    THIRDS
        .iter()
        .map(|&fc| {
            let (lo, hi) = (fc / 2f64.powf(1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
            let mut p = 0.0;
            for (b, a) in acc.iter().enumerate() {
                let f = b as f64 * hz;
                if f >= lo && f < hi {
                    p += a;
                }
            }
            // a band with no bin in it (very low centres at short FFTs): nearest bin
            if p <= 0.0 {
                p = acc.get((fc / hz).round() as usize).copied().unwrap_or(0.0);
            }
            10.0 * p.max(1e-30).log10()
        })
        .collect()
}

/// The song's recurring peak level, dBFS: the largest sample peak of 50 ms blocks once the top
/// 0.5 % of blocks are set aside.
pub fn robust_peak_db(l: &[f32], r: &[f32], sr: f32) -> f64 {
    let hop = ((sr * 0.05) as usize).max(1);
    let n = l.len().min(r.len());
    let mut pk: Vec<f32> = (0..n)
        .step_by(hop)
        .map(|i| l[i..(i + hop).min(n)].iter().chain(&r[i..(i + hop).min(n)]).fold(0f32, |m, v| m.max(v.abs())))
        .filter(|p| *p > 1e-6)
        .collect();
    if pk.is_empty() {
        return f64::from(crate::dsp::loudness::FLOOR);
    }
    pk.sort_by(f32::total_cmp);
    let p = pk[((pk.len() - 1) as f64 * 0.995) as usize];
    20.0 * f64::from(p).log10()
}

/// `spectrum` minus the reference balance, levelled over 100 Hz–5 kHz.
pub fn balance(spec: &[f64]) -> Vec<f64> {
    let dev: Vec<f64> = spec.iter().zip(THIRDS).map(|(s, f)| s - reference_db(f)).collect();
    let fit: Vec<f64> = dev.iter().zip(THIRDS).filter(|(_, f)| (100.0..=5000.0).contains(f)).map(|(d, _)| *d).collect();
    let mean = fit.iter().sum::<f64>() / fit.len().max(1) as f64;
    dev.iter().map(|d| d - mean).collect()
}

/// The five tone gains (dB) for a balance.
pub fn tone_gains(bal: &[f64]) -> [f64; 5] {
    let mut g = [0.0; 5];
    for (b, &(lo, hi, dead_lo, dead_hi, cut, boost)) in TONE_FIT.iter().enumerate() {
        let vals: Vec<f64> = bal.iter().zip(THIRDS).filter(|(_, f)| *f >= lo && *f <= hi).map(|(d, _)| *d).collect();
        let d = vals.iter().sum::<f64>() / vals.len().max(1) as f64;
        let over = if d > dead_hi {
            d - dead_hi
        } else if d < -dead_lo {
            d + dead_lo
        } else {
            0.0
        };
        let v = (-over * TONE_SHARE).clamp(-cut, boost);
        g[b] = (v * 10.0).round() / 10.0;
    }
    g
}

/// Mean gain reduction the glue aims at in every band, over the band's active (louder) half, dB.
const GLUE_DB: f64 = 1.5;

/// Band threshold from a detector trace: the one at which this band's louder half gets
/// [`GLUE_DB`] of reduction on average (static curve, `ratio`). Every band gets the same amount,
/// so the glue holds the song together without tilting its balance (a threshold set from each
/// band's own spread squeezed the transient-rich top much harder than the lows: the master
/// came out darker than the mix).
fn threshold(levels: &[f32], ratio: f64) -> f64 {
    let mut v: Vec<f32> = levels.iter().copied().filter(|x| *x > -90.0).collect();
    if v.len() < 10 {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    // the band's active part: within 20 dB of its loud level (a band of short hits, all hi-hat,
    // is mostly decay and silence: those frames must not drag the threshold down)
    let p95 = f64::from(v[(v.len() - 1) * 95 / 100]);
    let from = v.partition_point(|x| f64::from(*x) < p95 - 20.0);
    let active = &v[from..];
    let loud = &active[active.len() / 2..];
    // never more than ~5 dB on the band's loudest moments at 2:1
    let floor = p95 - 10.0;
    let mean_gr = |t: f64| -> f64 {
        loud.iter().map(|&l| -f64::from(master::band_gain_db(l, t as f32, ratio as f32))).sum::<f64>() / loud.len() as f64
    };
    // the reduction grows as the threshold falls: bisect
    let (mut hi, mut lo) = (-0.1f64, floor.clamp(-40.0, -0.1));
    if mean_gr(hi) >= GLUE_DB {
        return hi;
    }
    if mean_gr(lo) < GLUE_DB {
        return lo;
    }
    for _ in 0..40 {
        let mid = 0.5 * (hi + lo);
        if mean_gr(mid) < GLUE_DB {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    ((0.5 * (hi + lo) * 10.0).round() / 10.0).clamp(-40.0, -0.1)
}

/// The master may move the balance this much beyond its tone EQ before it is taken back, dB.
const DRIFT_MIN_DB: f64 = 0.5;
const DRIFT_MAX_DB: f64 = 2.0;

/// The five tone gains in `p`.
fn tone_now(p: &BTreeMap<String, f64>) -> [f64; 5] {
    ["sub", "low", "mid", "pres", "air"].map(|k| p.get(k).copied().unwrap_or(0.0))
}

/// Per tone band (its fit range): how far the output balance moved beyond the tone EQ, dB.
fn tone_drift(before: &[f64], after: &[f64], tone: &[f64; 5], sr: f32) -> [f64; 5] {
    // the EQ curve levelled like a balance (over 100 Hz–5 kHz), so only its shape counts
    let eq: Vec<f64> = THIRDS.iter().map(|&f| f64::from(tone_response(tone, f as f32, sr))).collect();
    let lev: Vec<f64> = eq.iter().zip(THIRDS).filter(|(_, f)| (100.0..=5000.0).contains(f)).map(|(v, _)| *v).collect();
    let eq_mean = lev.iter().sum::<f64>() / lev.len().max(1) as f64;
    let mut d = [0.0; 5];
    for (b, &(lo, hi, ..)) in TONE_FIT.iter().enumerate() {
        let v: Vec<f64> =
            (0..THIRDS.len()).filter(|&i| THIRDS[i] >= lo && THIRDS[i] <= hi).map(|i| after[i] - before[i] - (eq[i] - eq_mean)).collect();
        d[b] = v.iter().sum::<f64>() / v.len().max(1) as f64;
    }
    d
}

fn block_of(params: &BTreeMap<String, f64>) -> [f32; MAX_PARAMS] {
    let mut fx = crate::catalog::new_fx(1, FxKind::Master, 120.0);
    for (k, v) in params {
        fx.params.insert(k.clone(), *v);
    }
    let ParamBlock { values, .. } = crate::catalog::param_block(&fx);
    values
}

#[derive(Debug)]
pub struct Listened {
    pub params: BTreeMap<String, f64>,
    pub report: MasterListen,
}

/// The whole assistant (see the module docs) on the song `l`/`r` as it reaches the effect.
/// `current` are the effect's parameters now (ceiling, crossovers, width, level match are
/// kept); `target` indexes [`TARGETS`]. `progress` gets 0..1.
pub fn listen(
    l: &[f32],
    r: &[f32],
    sr: f32,
    current: &BTreeMap<String, f64>,
    target: usize,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Listened, String> {
    let target = target.min(TARGETS.len() - 1);
    let goal = f64::from(TARGETS[target]);
    let check = |p: f32, progress: &mut dyn FnMut(f32)| {
        progress(p);
        if cancel.load(Relaxed) {
            Err("cancelled".to_string())
        } else {
            Ok(())
        }
    };
    let (in_lufs, in_tp, in_lra) = measure(l, r, sr).ok_or("the song is silent where the Master is: nothing to listen to")?;
    check(0.05, progress)?;
    let bal = balance(&spectrum(l, r, sr));
    let tone = tone_gains(&bal);
    let mut p = crate::catalog::default_params(FxKind::Master);
    // the user's own choices the listen builds on: the ceiling, where the bands split, the band
    // levels they set and how they compare
    for k in ["ceil", "xlo", "xhi", "match", "gl", "gm", "gh"] {
        if let Some(v) = current.get(k) {
            p.insert(k.into(), *v);
        }
    }
    p.insert("target".into(), target as f64);
    p.insert("tone".into(), 100.0);
    for (k, g) in ["sub", "low", "mid", "pres", "air"].iter().zip(tone) {
        p.insert((*k).into(), g);
    }
    check(0.12, progress)?;
    // glue thresholds and band widths from the toned song
    let trace = master::band_trace(l, r, sr, &block_of(&p));
    for (k, lv) in ["tl", "tm", "th"].iter().zip(&trace.levels) {
        p.insert((*k).into(), threshold(lv, RATIO));
    }
    p.insert("ratio".into(), RATIO);
    for (k, w) in ["wl", "wm", "wh"].iter().zip(band_widths(trace.side_db)) {
        p.insert((*k).into(), w);
    }
    check(0.25, progress)?;
    let ceil = p.get("ceil").copied().unwrap_or(-1.0);
    let render = |p: &BTreeMap<String, f64>| {
        let (ol, or) = render_stereo_with(&mut Master::default(), FxKind::Master, p, l, r, sr);
        let m = measure(&ol, &or, sr);
        (ol, or, m)
    };
    // the clipper's share from the peak-to-loudness ratio the song arrives with (the limiter is
    // in every render, so the reduction it will do is read from the input, before it)
    // the peaks that recur (a stray spike must not decide how loud the song can go)
    let peak = robust_peak_db(l, r, sr).min(in_tp);
    let need = (peak - in_lufs) - (ceil - goal);
    // a target that needs more than stays clean is lowered to the loudest clean level
    let guarded = need > MAX_CLEAN_CUT_DB;
    let goal = if guarded { ceil - (peak - in_lufs - MAX_CLEAN_CUT_DB) } else { goal };
    let reduction = need.min(MAX_CLEAN_CUT_DB);
    let clip = if reduction > 2.0 { ((reduction - 2.0) * CLIP_PER_DB).clamp(0.0, MAX_CLIP).round() } else { 0.0 };
    p.insert("clip".into(), clip);
    p.insert("drive".into(), 0.0);
    let (_, _, m0) = render(&p);
    let (l0, _, _) = m0.ok_or("the song is silent after the tone and glue stages")?;
    check(0.3, progress)?;
    // drive: secant search on integrated loudness, from `pts` (drive, loudness) known so far
    type Out = (f64, f64, Option<(Vec<f32>, Vec<f32>)>);
    let solve = |p: &mut BTreeMap<String, f64>,
                 mut pts: Vec<(f64, f64)>,
                 mut d: f64,
                 p0: f32,
                 p1: f32,
                 progress: &mut dyn FnMut(f32)|
     -> Result<Out, String> {
        let mut best: Out = (pts[0].0, pts[0].1, None);
        for step in 0..MAX_RENDERS {
            p.insert("drive".into(), (d * 100.0).round() / 100.0);
            let (ol, or, m) = render(p);
            let lufs = m.map_or(f64::NEG_INFINITY, |m| m.0);
            if (lufs - goal).abs() < (best.1 - goal).abs() || best.2.is_none() {
                best = (d, lufs, Some((ol, or)));
            }
            check(p0 + (p1 - p0) * (step + 1) as f32 / MAX_RENDERS as f32, progress)?;
            if (lufs - goal).abs() < TOLERANCE_LU || (d >= DRIVE_MAX && lufs < goal) || (d <= 0.0 && lufs > goal) {
                break;
            }
            pts.push((d, lufs));
            let (a, b) = (pts[pts.len() - 2], pts[pts.len() - 1]);
            let slope = if (b.1 - a.1).abs() > 1e-6 && (b.0 - a.0).abs() > 1e-6 { (b.1 - a.1) / (b.0 - a.0) } else { 1.0 };
            // a limiter gives less than a dB per dB
            d = (b.0 + (goal - b.1) / slope.clamp(0.05, 1.5)).clamp(0.0, DRIVE_MAX);
        }
        Ok(best)
    };
    let (drive, lufs, out) = solve(&mut p, vec![(0.0, l0)], (goal - l0).clamp(0.0, DRIVE_MAX), 0.3, 0.65, progress)?;
    let (mut ol, mut or) = out.ok_or("could not render the master")?;
    p.insert("drive".into(), (drive * 100.0).round() / 100.0);
    // keep the mix's balance: heavy limiting ducks whatever plays with the kick and the bass, which
    // tilts the long-term spectrum. What the master did beyond the tone EQ it meant is measured
    // and taken back with the same five bands (at most 2 dB each), then the loudness re-solved.
    let mut bal_out = balance(&spectrum(&ol, &or, sr));
    let drift = tone_drift(&bal, &bal_out, &tone_now(&p), sr);
    if drift.iter().any(|d| d.abs() >= DRIFT_MIN_DB) {
        for (k, d) in ["sub", "low", "mid", "pres", "air"].iter().zip(drift) {
            if d.abs() >= DRIFT_MIN_DB {
                let g = p[*k] - d.clamp(-DRIFT_MAX_DB, DRIFT_MAX_DB);
                p.insert((*k).into(), (g.clamp(-6.0, 6.0) * 10.0).round() / 10.0);
            }
        }
        let (drive2, _, out2) = solve(&mut p, vec![(drive - 1.0, lufs - 0.7), (drive, lufs)], drive, 0.65, 0.98, progress)?;
        if let Some((a, b)) = out2 {
            p.insert("drive".into(), (drive2 * 100.0).round() / 100.0);
            ol = a;
            or = b;
            bal_out = balance(&spectrum(&ol, &or, sr));
        }
    }
    let (out_lufs, out_tp, out_lra) = measure(&ol, &or, sr).ok_or("the master is silent")?;
    progress(1.0);
    let round = |v: f64| (v * 10.0).round() / 10.0;
    Ok(Listened {
        report: MasterListen {
            seconds: round(l.len() as f64 / f64::from(sr)),
            target: target as u32,
            in_lufs: round(in_lufs),
            in_tp: round(in_tp),
            in_lra: round(in_lra),
            out_lufs: round(out_lufs),
            out_tp: round(out_tp),
            out_lra: round(out_lra),
            balance: bal.iter().map(|v| round(*v)).collect(),
            balance_out: bal_out.iter().map(|v| round(*v)).collect(),
            short: out_lufs < goal - 0.5,
            peak_cut: round(need.max(0.0)),
            guarded,
            aim: round(goal),
        },
        params: p,
    })
}

/// The tone curve the five gains make, dB at `f` (for the UI's display and tests).
pub fn tone_response(gains: &[f64; 5], f: f32, sr: f32) -> f32 {
    let mut db = 0.0;
    for (b, &(kind, fc, q)) in TONE_BANDS.iter().enumerate() {
        if gains[b].abs() > 1e-6 {
            let mut bq = crate::dsp::Biquad::default();
            bq.set(kind, sr, fc, q, gains[b] as f32);
            db += bq.response_db(f, sr);
        }
    }
    db
}

#[cfg(test)]
#[path = "mastering_tests.rs"]
mod tests;
