//! Master: a one-window mastering chain — tone EQ → three-band glue compression with stereo
//! width and bass mono → oversampled soft clipper → true-peak look-ahead limiter → optional
//! level-matched monitoring. FL Studio's Maximus is the starting point (three bands, a master
//! limiter, crossovers you drag); what it lacks for mastering is filled in: a true-peak
//! ceiling, a clipper that shaves transients before the limiter, per-band width and bass mono,
//! and a loudness-matched "is it actually better?" comparison. `mastering::listen` measures a
//! whole song and sets every control (the "assistant"); this file only plays them.
//!
//! **Tone.** Five fixed bands (low shelf 70 Hz, bells at 200 Hz / 800 Hz / 3.5 kHz, high
//! shelf 10 kHz), broad on purpose: mastering EQ corrects balance, not single notes. *Tone*
//! scales all five gains at once (0 % = flat), so the assistant's correction can be dialled
//! back with one control.
//!
//! **Glue.** The signal is split by two Linkwitz-Riley crossovers (`dsp::xover`, flat sum).
//! Each band has a stereo-linked 10 ms RMS detector, a 6 dB soft knee and the shared ratio;
//! attack / release are slower in the lows than in the highs (25/300, 12/200, 6/120 ms) so
//! the bass does not modulate. A threshold of 0 dB leaves the band alone. Each band has its own
//! stereo width (side gain, 0–150 %), Maximus-style: the lows are usually kept narrow or mono
//! (0 % = bass mono: tight on club systems and phones), the highs may open up. Each band also
//! has its own level after the glue (±12 dB), Maximus's band gain.
//!
//! **Clipper.** Before the limiter, on the low and mid bands only: the peaks a limiter struggles
//! with are the kick and the bass, and a full-band clipper shaving them also distorts the hi-hats
//! riding on them (intermodulation: measured −14.5 dB under the hats on a trap mix). The high
//! band goes around it through the same 2× filters, so the bands still sum flat. At 2× (the
//! saturator's polyphase IIR half-bands): linear
//! up to −3 dB of its threshold, then a tanh knee to the threshold. Only the distortion part
//! (curve − input) gets first-order antiderivative antialiasing, so below the knee the clipper
//! is an exact wire. *Clip* sets how close to the limiter's threshold it starts: 0 % = off,
//! 100 % = right at it (the limiter then only catches inter-sample peaks); in between it
//! shaves the tallest transients and leaves the rest to the limiter, which is how dense
//! masters get loud without the limiter pumping.
//!
//! **Limiter.** `dsp::limiter_v2` (true peak, 3 ms look-ahead, program-dependent release) with
//! *Loudness* as its input gain and *Ceiling* in dBTP.
//!
//! **Level match.** K-weighted power of the input and the output, averaged over ~3 s (the
//! short-term loudness window); with it on, the output is turned down by their difference,
//! so toggling the effect's bypass compares the master with the mix at equal loudness —
//! louder always sounds "better", and that is what the comparison must rule out.
//!
//! Nothing allocates after `prepare`.

// per-channel DSP loops index several parallel arrays at once
#![allow(clippy::needless_range_loop)]

use super::limiter_v2::{self, LimiterV2};
use super::loudness::{k_weighting, Bq};
use super::saturator::{coefs, lncosh, Hb};
use super::xover::{Ap2, Lr4};
use super::{db_to_gain, Biquad, FilterType};
use crate::processor::{ParamBlock, ProcessCtx, Processor, MAX_PARAMS, METERS};

/// Parameter order (= catalog `MASTER`).
pub mod idx {
    pub const TARGET: usize = 0;
    pub const DRIVE: usize = 1;
    pub const CEIL: usize = 2;
    pub const TONE: usize = 3;
    /// Five tone gains: sub shelf, low, mid, presence, air shelf.
    pub const EQ: usize = 4;
    pub const X_LO: usize = 9;
    pub const X_HI: usize = 10;
    /// Three band thresholds: low, mid, high.
    pub const THR: usize = 11;
    pub const RATIO: usize = 14;
    /// Three band widths (side gain, %): low, mid, high. Low 0 % = bass mono.
    pub const WIDTH: usize = 15;
    pub const CLIP: usize = 18;
    pub const MATCH: usize = 19;
    /// Three band levels (dB, after the glue): low, mid, high.
    pub const BAND_GAIN: usize = 20;
}

/// Loudness targets the assistant aims at (integrated LUFS), in `TARGET` order.
pub const TARGETS: [f32; 5] = [-16.0, -14.0, -12.0, -10.0, -8.0];
pub const TARGET_NAMES: [&str; 5] = ["Dynamic −16", "Streaming −14", "Balanced −12", "Loud −10", "Club −8"];

/// Tone bands: (kind, Hz, Q).
pub const TONE_BANDS: [(FilterType, f32, f32); 5] = [
    (FilterType::LowShelf, 70.0, 0.7),
    (FilterType::Peak, 200.0, 0.8),
    (FilterType::Peak, 800.0, 0.7),
    (FilterType::Peak, 3500.0, 0.8),
    (FilterType::HighShelf, 10_000.0, 0.7),
];
pub const TONE_MAX_DB: f32 = 6.0;

const BANDS: usize = 3;
const ATTACK_MS: [f32; BANDS] = [25.0, 12.0, 6.0];
const RELEASE_MS: [f32; BANDS] = [300.0, 200.0, 120.0];
const KNEE: f32 = 6.0;
const RMS_MS: f32 = 10.0;
/// The limiter's fixed look-ahead.
pub const LOOK_MS: f32 = 3.0;
/// Clipper: linear up to this fraction of its threshold (−3 dB), tanh knee above.
const CLIP_KNEE: f64 = 0.708;
/// At *Clip* 1 % the clipper starts this far above the limiter's threshold.
const CLIP_RANGE_DB: f32 = 6.0;
/// Level match: averaging time of both loudness detectors.
const MATCH_S: f32 = 3.0;

/// Gain of the band compressor at level `x` dB: ratio `r` above `t`, soft knee.
#[inline]
pub fn band_gain_db(x: f32, t: f32, r: f32) -> f32 {
    if t >= -0.01 || r <= 1.0 {
        return 0.0;
    }
    let s = 1.0 / r - 1.0;
    let o = x - t;
    if 2.0 * o <= -KNEE {
        0.0
    } else if 2.0 * o < KNEE {
        let u = o + KNEE / 2.0;
        s * u * u / (2.0 * KNEE)
    } else {
        s * o
    }
}

/// The clipper's curve at threshold `t` (`a` = start of the knee) and its antiderivative.
#[inline]
fn clip_f(x: f64, t: f64) -> f64 {
    let a = CLIP_KNEE * t;
    let w = t - a;
    let ax = x.abs();
    if ax <= a {
        x
    } else {
        x.signum() * (a + w * ((ax - a) / w).tanh())
    }
}

#[inline]
fn clip_big_f(x: f64, t: f64) -> f64 {
    let a = CLIP_KNEE * t;
    let w = t - a;
    let ax = x.abs();
    if ax <= a {
        0.5 * x * x
    } else {
        0.5 * a * a + a * (ax - a) + w * w * lncosh((ax - a) / w)
    }
}

/// Distortion part d(x) = f(x) − x and its antiderivative D(x) = F(x) − x²/2.
#[inline]
fn dist(x: f64, t: f64) -> f64 {
    clip_f(x, t) - x
}
#[inline]
fn dist_big(x: f64, t: f64) -> f64 {
    clip_big_f(x, t) - 0.5 * x * x
}

#[derive(Clone, Copy, Default)]
struct Clipper {
    up: Hb,
    down: Hb,
    x1: f64,
    /// the high band's own 2× round trip (no curve): the same filters, so the bands still sum flat
    hi_up: Hb,
    hi_down: Hb,
}

impl Clipper {
    /// One high-rate sample through the curve at threshold `t` (ADAA on the distortion only).
    #[inline]
    fn shape(&mut self, x: f32, t: f64) -> f32 {
        let x = f64::from(x);
        let x0 = self.x1;
        self.x1 = x;
        let a = CLIP_KNEE * t;
        if x.abs() <= a && x0.abs() <= a {
            return x as f32;
        }
        let dx = x - x0;
        let d = if dx.abs() > 1e-7 { (dist_big(x, t) - dist_big(x0, t)) / dx } else { dist(0.5 * (x + x0), t) };
        (x + d) as f32
    }
}

/// K-weighted power, one-pole averaged.
#[derive(Clone, Copy, Default)]
struct Kmeter {
    kw: [(Bq, Bq); 2],
    p: f64,
}

impl Kmeter {
    #[inline]
    fn feed(&mut self, l: f32, r: f32, k: f64) {
        let mut s = 0.0;
        for (c, v) in [l, r].into_iter().enumerate() {
            let (a, b) = &mut self.kw[c];
            let y = b.run(a.run(f64::from(v)));
            s += y * y;
        }
        self.p += (s - self.p) * k;
        if self.p < 1e-30 {
            self.p = 0.0;
        }
    }
    fn lufs(&self) -> f32 {
        if self.p > 1e-20 {
            (-0.691 + 10.0 * self.p.log10()) as f32
        } else {
            -200.0
        }
    }
}

pub struct Master {
    sr: f32,
    v: [f32; MAX_PARAMS],
    /// tone gains the filters are set to (dB, after Tone scaling)
    eq_set: [f32; 5],
    eq: [[Biquad; 5]; 2],
    x_lo: f32,
    x_hi: f32,
    xover_set: bool,
    split_lo: [Lr4; 2],
    split_hi: [Lr4; 2],
    low_ap: [Ap2; 2],
    rms: [f32; BANDS],
    gain: [f32; BANDS],
    k_rms: f32,
    att: [f32; BANDS],
    rel: [f32; BANDS],
    k_param: f32,
    /// band side gains the audio is at (smoothed towards the params)
    width: [f32; BANDS],
    /// band levels (linear), smoothed the same way
    band_g: [f32; BANDS],
    clip: [Clipper; 2],
    limiter: LimiterV2,
    lim_block: ParamBlock,
    k_match: f64,
    m_in: Kmeter,
    m_out: Kmeter,
    match_db: f32,
    // meters
    gr_band: [f32; BANDS],
    lvl_band: [f32; BANDS],
    clip_db: f32,
}

impl Default for Master {
    fn default() -> Self {
        let mut v = [0f32; MAX_PARAMS];
        v[idx::TARGET] = 2.0;
        v[idx::CEIL] = -1.0;
        v[idx::TONE] = 100.0;
        v[idx::X_LO] = 120.0;
        v[idx::X_HI] = 2800.0;
        v[idx::RATIO] = 2.0;
        v[idx::WIDTH] = 100.0;
        v[idx::WIDTH + 1] = 100.0;
        v[idx::WIDTH + 2] = 100.0;
        Self {
            sr: 48_000.0,
            v,
            eq_set: [f32::NAN; 5],
            eq: [[Biquad::default(); 5]; 2],
            x_lo: 120.0,
            x_hi: 2800.0,
            xover_set: false,
            split_lo: [Lr4::default(); 2],
            split_hi: [Lr4::default(); 2],
            low_ap: [Ap2::default(); 2],
            rms: [0.0; BANDS],
            gain: [0.0; BANDS],
            k_rms: 0.002,
            att: [0.0; BANDS],
            rel: [0.0; BANDS],
            k_param: 0.002,
            width: [1.0; BANDS],
            band_g: [1.0; BANDS],
            clip: [Clipper::default(); 2],
            limiter: LimiterV2::default(),
            lim_block: ParamBlock::default(),
            k_match: 1e-5,
            m_in: Kmeter::default(),
            m_out: Kmeter::default(),
            match_db: 0.0,
            gr_band: [0.0; BANDS],
            lvl_band: [-120.0; BANDS],
            clip_db: 0.0,
        }
    }
}

/// Delay the Master adds at `sr` (limiter look-ahead + the clipper's oversampling), samples.
pub fn latency_samples(sr: f32) -> usize {
    limiter_v2::latency_samples(LOOK_MS, sr) + clipper_latency().round() as usize
}

/// Group delay of the 2× up + down half-bands near DC, base-rate samples.
fn clipper_latency() -> f64 {
    super::saturator::hb_delay(&coefs().0)
}

impl Master {
    fn set_eq(&mut self) {
        let tone = (self.v[idx::TONE] / 100.0).clamp(0.0, 1.0);
        for b in 0..5 {
            let g = self.v[idx::EQ + b].clamp(-TONE_MAX_DB, TONE_MAX_DB) * tone;
            if (g - self.eq_set[b]).abs() > 1e-4 || self.eq_set[b].is_nan() {
                self.eq_set[b] = g;
                let (kind, f, q) = TONE_BANDS[b];
                for c in 0..2 {
                    if g.abs() < 1e-4 {
                        self.eq[c][b].bypass();
                    } else {
                        self.eq[c][b].set(kind, self.sr, f, q, g);
                    }
                }
            }
        }
    }

    fn set_xover(&mut self) {
        let lo = self.v[idx::X_LO].clamp(40.0, 500.0);
        let hi = self.v[idx::X_HI].clamp((lo * 4.0).max(1000.0), 8000.0);
        if (lo - self.x_lo).abs() > 1e-3 || (hi - self.x_hi).abs() > 1e-3 || !self.xover_set {
            self.xover_set = true;
            self.x_lo = lo;
            self.x_hi = hi;
            let sr = f64::from(self.sr);
            for c in 0..2 {
                self.split_lo[c].set(f64::from(lo), sr);
                self.split_hi[c].set(f64::from(hi), sr);
                self.low_ap[c].set(f64::from(hi), sr);
            }
        }
    }

    fn set_limiter(&mut self) {
        let v = &mut self.lim_block.values;
        v[limiter_v2::idx::GAIN] = self.v[idx::DRIVE].clamp(0.0, 24.0);
        v[limiter_v2::idx::CEIL] = self.v[idx::CEIL].clamp(-6.0, 0.0);
        v[limiter_v2::idx::REL] = 80.0;
        v[limiter_v2::idx::AUTO] = 1.0;
        v[limiter_v2::idx::LOOK] = LOOK_MS;
        v[limiter_v2::idx::LINK] = 100.0;
        v[limiter_v2::idx::OUT] = 0.0;
        self.limiter.set_params(&self.lim_block);
    }
}

impl Master {
    /// Tone EQ and the three-band split of one stereo sample: `[channel][band]`.
    #[inline]
    fn front(&mut self, l: f32, r: f32) -> [[f64; BANDS]; 2] {
        let mut x = [l, r];
        let mut bands = [[0f64; BANDS]; 2];
        for c in 0..2 {
            for b in 0..5 {
                x[c] = self.eq[c][b].tick(x[c]);
            }
            let (lo, rest) = self.split_lo[c].split(f64::from(x[c]));
            let (mid, hi) = self.split_hi[c].split(rest);
            bands[c] = [self.low_ap[c].run(lo), mid, hi];
        }
        bands
    }

    /// The band's stereo-linked RMS detector, dB.
    #[inline]
    fn detect(&mut self, b: usize, bl: f32, br: f32) -> f32 {
        let p = 0.5 * (bl * bl + br * br);
        self.rms[b] += (p - self.rms[b]) * self.k_rms;
        if self.rms[b] < 1e-30 {
            self.rms[b] = 0.0;
        }
        10.0 * (self.rms[b] + 1e-20).log10()
    }
}

/// What the band detectors see over a whole song, for the assistant.
pub struct BandTrace {
    /// Each band's detector level, dB, every 10 ms.
    pub levels: [Vec<f32>; BANDS],
    /// Side energy against mid energy in each band, dB.
    pub side_db: [f32; BANDS],
}

/// Runs the tone EQ, the split and the detectors of a Master set to `values` over `l`/`r`.
/// Offline only (allocates).
pub fn band_trace(l: &[f32], r: &[f32], sr: f32, values: &[f32; MAX_PARAMS]) -> BandTrace {
    let mut m = Master { v: *values, ..Master::default() };
    m.prepare(sr, 1024);
    let hop = ((sr * 0.01) as usize).max(1);
    let n = l.len().min(r.len());
    let mut levels: [Vec<f32>; BANDS] = std::array::from_fn(|_| Vec::with_capacity(n / hop + 1));
    let (mut side, mut mid) = ([0f64; BANDS], [0f64; BANDS]);
    for i in 0..n {
        let bands = m.front(l[i], r[i]);
        for b in 0..BANDS {
            let lvl = m.detect(b, bands[0][b] as f32, bands[1][b] as f32);
            if i % hop == hop - 1 {
                levels[b].push(lvl);
            }
        }
        for b in 0..BANDS {
            let (a, c) = (bands[0][b], bands[1][b]);
            mid[b] += 0.25 * (a + c) * (a + c);
            side[b] += 0.25 * (a - c) * (a - c);
        }
    }
    let side_db = std::array::from_fn(|b| if mid[b] > 1e-12 { (10.0 * (side[b].max(1e-30) / mid[b]).log10()) as f32 } else { -120.0 });
    BandTrace { levels, side_db }
}

impl Processor for Master {
    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        self.sr = sample_rate.max(8000.0);
        let sr = self.sr;
        let k = |ms: f32| 1.0 - (-1.0 / (ms / 1000.0 * sr)).exp();
        self.k_rms = k(RMS_MS);
        self.k_param = k(20.0);
        for b in 0..BANDS {
            self.att[b] = k(ATTACK_MS[b]);
            self.rel[b] = k(RELEASE_MS[b]);
        }
        self.k_match = 1.0 - (-1.0 / (f64::from(MATCH_S) * f64::from(sr))).exp();
        let kw = k_weighting(f64::from(sr));
        self.m_in.kw = [kw; 2];
        self.m_out.kw = [kw; 2];
        self.eq_set = [f32::NAN; 5];
        self.set_eq();
        self.xover_set = false;
        self.set_xover();
        self.limiter.prepare(sr, max_block);
        self.set_limiter();
        self.width = std::array::from_fn(|b| (self.v[idx::WIDTH + b] / 100.0).clamp(0.0, 1.5));
        self.band_g = std::array::from_fn(|b| db_to_gain(self.v[idx::BAND_GAIN + b].clamp(-12.0, 12.0)));
        self.reset();
    }

    fn set_params(&mut self, p: &ParamBlock) {
        self.v = p.values;
        self.set_eq();
        self.set_xover();
        self.set_limiter();
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        let n = l.len().min(r.len());
        let (c1, _) = coefs();
        let ratio = self.v[idx::RATIO].clamp(1.0, 4.0);
        let thr = [self.v[idx::THR].clamp(-40.0, 0.0), self.v[idx::THR + 1].clamp(-40.0, 0.0), self.v[idx::THR + 2].clamp(-40.0, 0.0)];
        let width_t: [f32; BANDS] = std::array::from_fn(|b| (self.v[idx::WIDTH + b] / 100.0).clamp(0.0, 1.5));
        let gain_t: [f32; BANDS] = std::array::from_fn(|b| db_to_gain(self.v[idx::BAND_GAIN + b].clamp(-12.0, 12.0)));
        let clip = self.v[idx::CLIP].clamp(0.0, 100.0);
        // the limiter's threshold in this (pre-drive) domain; the clipper sits up to 6 dB above it
        let lim_thr = db_to_gain(self.v[idx::CEIL].clamp(-6.0, 0.0) - self.v[idx::DRIVE].clamp(0.0, 24.0));
        let clip_t = if clip > 0.0 { f64::from(lim_thr * db_to_gain(CLIP_RANGE_DB * (1.0 - clip / 100.0))) } else { f64::INFINITY };
        let match_on = self.v[idx::MATCH] >= 0.5;
        let kp = self.k_param;
        let mut gr_min = [0f32; BANDS];
        let mut lvl_max = [-120f32; BANDS];
        let (mut pk_in, mut pk_out) = (0f32, 0f32);
        for i in 0..n {
            self.m_in.feed(l[i], r[i], self.k_match);
            for b in 0..BANDS {
                self.width[b] += (width_t[b] - self.width[b]) * kp;
                self.band_g[b] += (gain_t[b] - self.band_g[b]) * kp;
            }
            let bands = self.front(l[i], r[i]);
            // glue, then mid/side per band; the low and mid bands go to the clipper, the high
            // band around it
            let (mut ol, mut or) = (0f32, 0f32);
            let (mut hl, mut hr) = (0f32, 0f32);
            for b in 0..BANDS {
                let (bl, br) = (bands[0][b] as f32, bands[1][b] as f32);
                let lvl = self.detect(b, bl, br);
                let target = band_gain_db(lvl, thr[b], ratio);
                let k = if target < self.gain[b] { self.att[b] } else { self.rel[b] };
                self.gain[b] += (target - self.gain[b]) * k;
                if self.gain[b].abs() < 1e-6 {
                    self.gain[b] = 0.0;
                }
                let g = db_to_gain(self.gain[b]) * self.band_g[b];
                let side_g = self.width[b];
                let (m, s) = (0.5 * (bl + br) * g, 0.5 * (bl - br) * g * side_g);
                if b == BANDS - 1 {
                    hl = m + s;
                    hr = m - s;
                } else {
                    ol += m + s;
                    or += m - s;
                }
                gr_min[b] = gr_min[b].min(self.gain[b]);
                lvl_max[b] = lvl_max[b].max(lvl);
            }
            // clipper at 2× (always run: constant latency, and the half-bands stay warm)
            let mut y = [ol, or];
            let hi = [hl, hr];
            for c in 0..2 {
                let cl = &mut self.clip[c];
                let (he, ho) = cl.hi_up.up(c1, hi[c]);
                let h = cl.hi_down.down(c1, he, ho);
                let (e, o) = cl.up.up(c1, y[c]);
                let (e, o) = if clip_t.is_finite() {
                    (cl.shape(e, clip_t), cl.shape(o, clip_t))
                } else {
                    cl.x1 = f64::from(o);
                    (e, o)
                };
                let d = cl.down.down(c1, e, o);
                pk_in = pk_in.max(y[c].abs());
                pk_out = pk_out.max(d.abs());
                y[c] = d + h;
            }
            l[i] = y[0];
            r[i] = y[1];
        }
        self.limiter.process(l, r, ctx);
        // output loudness and level match
        for i in 0..n {
            self.m_out.feed(l[i], r[i], self.k_match);
            if match_on {
                let (pi, po) = (self.m_in.p, self.m_out.p);
                let target = if pi > 1e-12 && po > 1e-12 { (10.0 * (pi / po).log10()).clamp(-30.0, 6.0) as f32 } else { self.match_db };
                self.match_db += (target - self.match_db) * kp;
            } else {
                self.match_db += (0.0 - self.match_db) * kp;
            }
            if self.match_db.abs() > 1e-4 {
                let g = db_to_gain(self.match_db);
                l[i] *= g;
                r[i] *= g;
            }
        }
        self.gr_band = gr_min;
        self.lvl_band = lvl_max;
        self.clip_db = if pk_in > 1e-9 && pk_out < pk_in { 20.0 * (pk_out / pk_in).log10() } else { 0.0 };
    }

    fn reset(&mut self) {
        for c in 0..2 {
            for b in 0..5 {
                self.eq[c][b].reset();
            }
            self.split_lo[c].clear();
            self.split_hi[c].clear();
            self.low_ap[c].clear();
            self.clip[c] = Clipper::default();
        }
        self.rms = [0.0; BANDS];
        self.gain = [0.0; BANDS];
        self.limiter.reset();
        for m in [&mut self.m_in, &mut self.m_out] {
            for k in &mut m.kw {
                k.0.z = [0.0; 2];
                k.1.z = [0.0; 2];
            }
            m.p = 0.0;
        }
        self.match_db = 0.0;
        self.gr_band = [0.0; BANDS];
        self.clip_db = 0.0;
    }

    fn latency_samples(&self) -> usize {
        latency_samples(self.sr)
    }

    fn tail_samples(&self) -> Option<usize> {
        // release of the slowest band + the limiter's window; the level-match average is
        // deliberately not waited for (it holds its gain through silence)
        Some((self.sr * 1.5) as usize + latency_samples(self.sr))
    }

    fn gain_reduction_db(&self) -> f32 {
        self.limiter.gain_reduction_db() + self.clip_db + self.gr_band.iter().copied().fold(0.0, f32::min)
    }

    fn meters(&self, out: &mut [f32; METERS]) {
        out[0] = self.gr_band[0];
        out[1] = self.gr_band[1];
        out[2] = self.gr_band[2];
        out[3] = self.clip_db;
        out[4] = self.limiter.gain_reduction_db();
        out[5] = self.m_in.lufs();
        out[6] = self.m_out.lufs();
        out[7] = self.match_db;
        out[8] = self.lvl_band[0];
        out[9] = self.lvl_band[1];
        out[10] = self.lvl_band[2];
    }
}

#[cfg(test)]
#[path = "master_tests.rs"]
mod tests;
