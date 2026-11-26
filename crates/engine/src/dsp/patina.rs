//! Patina: a lo-fi character effect. Five modules in a fixed order, each with one Amount knob
//! and a few details, under one Magnitude that scales them all, plus a master tone section.
//!
//! ```text
//!            ┌──────────────── noise generator (vinyl, shellac, tape, hum) ─────────────┐
//!            │ pre (processed by the modules)                       post (added clean) │
//!  in ─ + ───┴─ Wobble ─ Drive (band) ─ Digital ─ Wear ─ + ──────────────────────────────┴─ low/high cut ─ tilt ─ width ─ out
//! ```
//!
//! - **Noise**: procedural, never a loop. Hiss is pink + white noise shaped per medium (a
//!   cassette's band stops near 11 kHz, a reel's near 17 kHz) with a slow level drift. Vinyl
//!   crackle is a Poisson stream of short band-passed noise bursts: every click gets its own
//!   centre frequency, Q, length, level (a cubed uniform: many faint ticks, few loud ones) and
//!   pan, so no two clicks sound alike; pops are rarer, lower and longer; shellac adds the
//!   scratch of its 78 rpm revolution. Hum is 50 Hz with its odd and even harmonics. Follow
//!   lets the noise ride the input level, Duck pulls it under loud passages.
//! - **Wobble**: wow and flutter from a delay line read with 4-point Hermite interpolation. The
//!   delay moves by a sine at Rate blended with a band-limited random curve (Wander sets the
//!   blend), plus a faster flutter; depth is set as a pitch deviation, so the amplitude of the
//!   delay swing scales with 1/rate and the depth sounds the same at any rate. The delay is a
//!   bounded sum of zero-mean curves: the pitch centre never drifts.
//! - **Drive**: the Saturator's core (five voicings, oversampled, level-matched). Band limits
//!   split the input with Linkwitz-Riley crossovers; only the band between them is driven and
//!   the rest passes through an identical but clean core (same latency, same linear filtering),
//!   so the bands sum flat.
//! - **Digital**: sample-rate reduction by a fractional-rate hold (the held value is latched at
//!   the exact crossing instant, so the rate sweeps without beating) and bit reduction with a
//!   continuous bit depth, then a 4-pole Smooth filter.
//! - **Wear**: playback head losses (a low-pass that falls with wear and tape speed, a head bump
//!   in the lows), soft compression of peaks, slow level wear and random dropouts (a dip in level
//!   and in the high end, as when the tape lifts off the head), mono or per channel.
//!
//! Magnitude scales each module on its own curve (noise and dropouts come in last, so a little
//! Magnitude is colour without hiss). Wander moves every module a little at random.
//!
//! Real time: everything is allocated in `prepare`; `process` neither allocates nor locks.

use std::f32::consts::{PI, TAU};

use super::saturator::{self, Core, Settings as SatSettings, STYLES};
use super::xover::{Ap2, Lr4};
use super::{db_to_gain, flush, gain_to_db, Biquad, FilterType, Smooth};
use crate::processor::{ParamBlock, ProcessCtx, Processor, METERS};

/// Parameter order (= catalog `PATINA`).
pub mod idx {
    pub const AMOUNT: usize = 0;
    pub const NOISE: usize = 1;
    pub const NTYPE: usize = 2;
    pub const NTONE: usize = 3;
    pub const NFOLLOW: usize = 4;
    pub const NROUTE: usize = 5;
    pub const WOBBLE: usize = 6;
    pub const WRATE: usize = 7;
    pub const FLUTTER: usize = 8;
    pub const WSTEREO: usize = 9;
    pub const DRIVE: usize = 10;
    pub const DSTYLE: usize = 11;
    pub const DLOW: usize = 12;
    pub const DHIGH: usize = 13;
    pub const DIGITAL: usize = 14;
    pub const SRATE: usize = 15;
    pub const BITS: usize = 16;
    pub const DSMOOTH: usize = 17;
    pub const WEAR: usize = 18;
    pub const TAPE: usize = 19;
    pub const DROPS: usize = 20;
    pub const DSTEREO: usize = 21;
    pub const WANDER: usize = 22;
    pub const LOWCUT: usize = 23;
    pub const HIGHCUT: usize = 24;
    pub const TONE: usize = 25;
    pub const WIDTH: usize = 26;
    pub const OUT: usize = 27;
}

/// Noise types, in parameter order.
pub const NOISE_TYPES: [&str; 6] = ["Winyl", "Winyl — kurz", "Szelak 78", "Kaseta", "Taśma szpulowa", "Brum"];
/// Where the noise joins the signal.
pub const ROUTES: [&str; 2] = ["Po", "Przed"];
/// Tape speeds of the Wear module.
pub const TAPES: [&str; 3] = ["Kaseta", "Taśma 19 cm/s", "Taśma 38 cm/s"];
/// The Drive voicings (the Saturator's).
pub const DRIVE_STYLES: [&str; 5] = STYLES;

/// Fewest samples the wobble line delays by (the Hermite read needs one newer sample).
const WOB_MIN: f32 = 2.0;
/// Longest wobble swing either side of the centre, seconds.
const WOB_MAX_S: f32 = 0.025;
/// Deepest wow, as a fraction of the speed (±2.5 % ≈ ±43 cents).
const WOW_MAX: f32 = 0.025;
/// Samples per control block (targets, crossovers, filter coefficients).
const BLOCK: usize = 32;

/// Delay the effect adds, in samples: the Drive's oversampling plus the wobble line's minimum.
pub fn latency_samples() -> usize {
    saturator::latency_samples(false) + WOB_MIN as usize
}

/// Meter slots (see docs/architecture.md).
pub mod meter {
    /// Noise level heard (after Follow/Duck and the level), peak, linear.
    pub const NOISE: usize = 0;
    /// Pitch deviation of the wobble, cents, left / right.
    pub const CENTS_L: usize = 1;
    pub const CENTS_R: usize = 2;
    /// Peak into the driven band, linear.
    pub const DRIVE_IN: usize = 3;
    /// Effective sample rate (Hz) and bit depth of the Digital module.
    pub const SRATE: usize = 4;
    pub const BITS: usize = 5;
    /// Dropout depth 0..1, left / right.
    pub const DROP_L: usize = 6;
    pub const DROP_R: usize = 7;
    /// Wear: the head-loss low-pass (Hz) and the peak compression (dB, ≤ 0).
    pub const WEAR_HZ: usize = 8;
    pub const WEAR_GR: usize = 9;
    /// Effective amounts 0..1 after Magnitude and Wander: noise, wobble, drive, digital, wear.
    pub const EFF: usize = 10;
    /// Magnitude 0..1.
    pub const MAG: usize = 15;
    /// Clicks and pops started since the effect was prepared (wraps at 2^24).
    pub const CLICKS: usize = 16;
}

// ------------------------------------------------------------------ small parts

/// xorshift32: cheap, deterministic, no allocation.
#[derive(Clone, Copy)]
struct Rng(u32);

impl Rng {
    #[inline]
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// Uniform in [0, 1).
    #[inline]
    fn uni(&mut self) -> f32 {
        (self.next() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
    /// Uniform in [−1, 1).
    #[inline]
    fn bip(&mut self) -> f32 {
        self.uni() * 2.0 - 1.0
    }
}

/// A band-limited random curve in [−1, 1]: a new random target every period, reached along a
/// half cosine. Its rate and amplitude do not depend on the sample rate.
#[derive(Clone, Copy, Default)]
struct Drift {
    a: f32,
    b: f32,
    ph: f32,
}

impl Drift {
    #[inline]
    fn tick(&mut self, inc: f32, rng: &mut Rng) -> f32 {
        self.ph += inc;
        if self.ph >= 1.0 {
            self.ph -= self.ph.floor();
            self.a = self.b;
            self.b = rng.bip();
        }
        let t = 0.5 - 0.5 * (PI * self.ph).cos();
        self.a + (self.b - self.a) * t
    }
}

/// Zero-delay-feedback state-variable filter (A. Simper): stable while its cutoff moves.
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Svf {
    fn set(&mut self, f: f32, q: f32, sr: f32) {
        let g = (PI * f.clamp(5.0, sr * 0.49) / sr).tan();
        self.k = 1.0 / q.max(0.05);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    /// (low-pass, band-pass normalised to unity at the centre, high-pass).
    #[inline]
    fn tick(&mut self, x: f32) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        (v2, self.k * v1, x - self.k * v1 - v2)
    }
    fn clear(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// `x` towards `t` in log frequency, `w` 0..1.
#[inline]
fn log_lerp(x: f32, t: f32, w: f32) -> f32 {
    (x.ln() + (t.ln() - x.ln()) * w.clamp(0.0, 1.0)).exp()
}

// ------------------------------------------------------------------ noise

/// How one noise type is built, at full level.
#[derive(Clone, Copy)]
struct NoiseType {
    /// Hiss level (RMS-ish, linear) and its band.
    hiss: f32,
    hiss_lp: f32,
    hiss_hp: f32,
    /// A band of noise on top of the hiss: a record's groove swish (it breathes with the
    /// revolution), a cassette's 3–6 kHz sizzle. Level, centre, Q.
    surf: f32,
    surf_f: f32,
    surf_q: f32,
    /// Grit: a dense stream of tiny impulses (shellac's abrasive roar), per second and level.
    grit: f32,
    grit_amp: f32,
    /// Crackle in three tiers (heavy-tailed levels): ticks (tiny, very short), clicks (short
    /// band-passed bursts), pops (rare, low and long). Per second and level of each.
    ticks: f32,
    tick_amp: f32,
    clicks: f32,
    click_amp: f32,
    pops: f32,
    pop_amp: f32,
    /// Scratches pressed into the record: they come back every revolution. How many, level.
    marks: usize,
    mark_amp: f32,
    /// Low rumble under the record or tape.
    rumble: f32,
    /// Mains hum (50 Hz, its harmonics and buzz).
    hum: f32,
    /// Revolutions per minute (swish, scratches); 0 = not a record.
    rpm: f32,
    /// Band of the whole noise (the medium's response); `steep` = 24 dB/oct at the top.
    band_lo: f32,
    band_hi: f32,
    steep: bool,
    /// Stereo width of the crackle, 0 (mono, a lateral-cut 78) .. 1.
    width: f32,
    /// Slow drift of the hiss level, ± fraction.
    drift: f32,
    /// Share of white (vs pink) noise in the hiss: finer, airier grain.
    white: f32,
    /// Faster wander of the whole noise level (transport, contact): rate Hz, ± fraction.
    wob_hz: f32,
    wob: f32,
}

#[rustfmt::skip]
const TYPES: [NoiseType; 6] = [
    // vinyl: quiet surface, sparse ticks and clicks, a pop now and then, three scratches that come round every 1.8 s
    NoiseType { hiss: 0.007, hiss_lp: 12000.0, hiss_hp: 40.0, surf: 0.010, surf_f: 3000.0, surf_q: 0.6, grit: 0.0, grit_amp: 0.0,
        ticks: 60.0, tick_amp: 0.10, clicks: 12.0, click_amp: 0.35, pops: 0.5, pop_amp: 0.30, marks: 3, mark_amp: 0.25,
        rumble: 0.05, hum: 0.0, rpm: 33.33, band_lo: 25.0, band_hi: 14000.0, steep: false, width: 0.8, drift: 0.25,
        white: 0.35, wob_hz: 0.0, wob: 0.0 },
    // dusty vinyl: a dense, uneven bed of ticks and clicks, thumps, more scratches
    NoiseType { hiss: 0.006, hiss_lp: 11000.0, hiss_hp: 40.0, surf: 0.012, surf_f: 2800.0, surf_q: 0.6, grit: 400.0, grit_amp: 0.015,
        ticks: 160.0, tick_amp: 0.12, clicks: 30.0, click_amp: 0.38, pops: 1.8, pop_amp: 0.32, marks: 5, mark_amp: 0.28,
        rumble: 0.06, hum: 0.0, rpm: 33.33, band_lo: 25.0, band_hi: 13000.0, steep: false, width: 0.8, drift: 0.25,
        white: 0.35, wob_hz: 0.0, wob: 0.0 },
    // shellac 78: a choked, nasal, mid-heavy roar in a narrow steep band, nearly mono, grinding with the revolution
    NoiseType { hiss: 0.028, hiss_lp: 5000.0, hiss_hp: 150.0, surf: 0.055, surf_f: 1600.0, surf_q: 0.9, grit: 2500.0, grit_amp: 0.05,
        ticks: 120.0, tick_amp: 0.14, clicks: 14.0, click_amp: 0.38, pops: 3.0, pop_amp: 0.35, marks: 4, mark_amp: 0.35,
        rumble: 0.03, hum: 0.0, rpm: 78.0, band_lo: 200.0, band_hi: 4200.0, steep: true, width: 0.12, drift: 0.4,
        white: 0.3, wob_hz: 0.0, wob: 0.0 },
    // cassette (Type I): hiss with its 4–7 kHz sizzle, a 12 dB/oct roll-off past 12 kHz, breathing with the transport
    NoiseType { hiss: 0.034, hiss_lp: 12500.0, hiss_hp: 90.0, surf: 0.016, surf_f: 5500.0, surf_q: 0.7, grit: 0.0, grit_amp: 0.0,
        ticks: 0.0, tick_amp: 0.0, clicks: 0.0, click_amp: 0.0, pops: 0.0, pop_amp: 0.0, marks: 0, mark_amp: 0.0,
        rumble: 0.008, hum: 0.0015, rpm: 0.0, band_lo: 30.0, band_hi: 13000.0, steep: false, width: 1.0, drift: 0.15,
        white: 0.35, wob_hz: 5.0, wob: 0.07 },
    // reel-to-reel at 15 ips: quieter, finer and airier than a cassette, barely moving
    NoiseType { hiss: 0.018, hiss_lp: 16500.0, hiss_hp: 40.0, surf: 0.0, surf_f: 3000.0, surf_q: 0.6, grit: 0.0, grit_amp: 0.0,
        ticks: 0.0, tick_amp: 0.0, clicks: 0.0, click_amp: 0.0, pops: 0.0, pop_amp: 0.0, marks: 0, mark_amp: 0.0,
        rumble: 0.003, hum: 0.0008, rpm: 0.0, band_lo: 30.0, band_hi: 18000.0, steep: false, width: 1.0, drift: 0.08,
        white: 0.65, wob_hz: 1.0, wob: 0.06 },
    // hum: a ground loop (50 Hz and its first harmonics, a little rectifier buzz) over a preamp's hiss
    NoiseType { hiss: 0.006, hiss_lp: 10000.0, hiss_hp: 120.0, surf: 0.0, surf_f: 3000.0, surf_q: 0.6, grit: 0.0, grit_amp: 0.0,
        ticks: 0.0, tick_amp: 0.0, clicks: 0.0, click_amp: 0.0, pops: 0.0, pop_amp: 0.0, marks: 0, mark_amp: 0.0,
        rumble: 0.015, hum: 0.05, rpm: 0.0, band_lo: 30.0, band_hi: 12000.0, steep: false, width: 1.0, drift: 0.05,
        white: 0.35, wob_hz: 0.7, wob: 0.25 },
];

const VOICES: usize = 12;
const MAX_MARKS: usize = 6;

#[derive(Clone, Copy, Default, PartialEq)]
enum Tier {
    #[default]
    Tick,
    Click,
    Pop,
}

/// One tick, click or pop: an impulse and a short noise burst through its own filter.
#[derive(Clone, Copy, Default)]
struct Click {
    svf: Svf,
    env: f32,
    dec: f32,
    amp: f32,
    /// the opening impulse, then its opposite (a biphasic edge), then nothing
    kick: [f32; 2],
    lowpass: bool,
    gl: f32,
    gr: f32,
    on: bool,
}

/// A scratch pressed into the record: where on the revolution, its pitch and level.
#[derive(Clone, Copy, Default)]
struct Mark {
    ph: f32,
    f: f32,
    amp: f32,
}

/// The noise of one type, before its level.
struct NoiseGen {
    rng: Rng,
    pink: [[f32; 7]; 2],
    hiss_lp: [Svf; 2],
    hiss_hp: [Svf; 2],
    surf: [Svf; 2],
    rumble: Svf,
    band_hp: [Svf; 2],
    band_lp: [[Svf; 2]; 2],
    clicks: [Click; VOICES],
    marks: [Mark; MAX_MARKS],
    hum_ph: f32,
    buzz_lp: f32,
    buzz_k: f32,
    /// the mains frequency wanders a little, and the buzz flickers
    hum_drift: Drift,
    wob: Drift,
    rev_ph: f32,
    ty: usize,
    sr: f32,
    /// white noise scale that keeps the noise density, so the level, independent of the rate
    norm: f32,
    started: u32,
}

impl Default for NoiseGen {
    fn default() -> Self {
        Self {
            rng: Rng(0x9E37_79B9),
            pink: [[0.0; 7]; 2],
            hiss_lp: [Svf::default(); 2],
            hiss_hp: [Svf::default(); 2],
            surf: [Svf::default(); 2],
            rumble: Svf::default(),
            band_hp: [Svf::default(); 2],
            band_lp: [[Svf::default(); 2]; 2],
            clicks: [Click::default(); VOICES],
            marks: [Mark::default(); MAX_MARKS],
            hum_ph: 0.0,
            buzz_lp: 0.0,
            buzz_k: 0.2,
            hum_drift: Drift::default(),
            wob: Drift::default(),
            rev_ph: 0.0,
            ty: usize::MAX,
            sr: 48000.0,
            norm: 1.0,
            started: 0,
        }
    }
}

impl NoiseGen {
    fn prepare(&mut self, sr: f32) {
        self.sr = sr;
        self.norm = (sr / 48000.0).sqrt();
        self.buzz_k = 1.0 - (-TAU * 2000.0 / sr).exp();
        self.ty = usize::MAX;
        self.reset();
    }

    fn reset(&mut self) {
        self.pink = [[0.0; 7]; 2];
        for f in self.hiss_lp.iter_mut().chain(&mut self.hiss_hp).chain(&mut self.surf).chain(&mut self.band_hp) {
            f.clear();
        }
        for f in self.band_lp.iter_mut().flatten() {
            f.clear();
        }
        self.rumble.clear();
        self.buzz_lp = 0.0;
        for c in &mut self.clicks {
            c.on = false;
            c.svf.clear();
        }
    }

    fn set_type(&mut self, ty: usize) {
        if ty == self.ty {
            return;
        }
        self.ty = ty;
        let t = &TYPES[ty];
        let sr = self.sr;
        for c in 0..2 {
            self.hiss_lp[c].set(t.hiss_lp, 0.6, sr);
            self.hiss_hp[c].set(t.hiss_hp, 0.7, sr);
            self.surf[c].set(t.surf_f, t.surf_q, sr);
            self.band_hp[c].set(t.band_lo, 0.707, sr);
            // two stages in Butterworth pairs (Q 0.54, 1.31) make 24 dB/oct; one stage 12 dB/oct
            if t.steep {
                self.band_lp[c][0].set(t.band_hi, 0.541, sr);
                self.band_lp[c][1].set(t.band_hi, 1.307, sr);
            } else {
                self.band_lp[c][0].set(t.band_hi, 0.707, sr);
            }
        }
        self.rumble.set(30.0, 0.8, sr);
        // the scratches of this record: where they sit on the revolution, their pitch and level
        for m in &mut self.marks {
            let r = &mut self.rng;
            *m = Mark { ph: r.uni(), f: 700.0 * 5f32.powf(r.uni()), amp: (0.4 + 0.6 * r.uni()) * t.mark_amp };
        }
    }

    /// Kellet's pink filter on white noise `w`, about unit RMS for unit-RMS input.
    #[inline]
    fn pink(b: &mut [f32; 7], w: f32) -> f32 {
        b[0] = 0.99886 * b[0] + w * 0.055_517_9;
        b[1] = 0.99332 * b[1] + w * 0.075_075_9;
        b[2] = 0.969 * b[2] + w * 0.153_852;
        b[3] = 0.8665 * b[3] + w * 0.310_485_6;
        b[4] = 0.55 * b[4] + w * 0.532_952_2;
        b[5] = -0.7616 * b[5] - w * 0.016_898;
        let p = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115_926;
        for s in b.iter_mut() {
            *s = flush(*s);
        }
        p * 0.11
    }

    /// Start a tick, click or pop; `f` fixes its pitch (a scratch that comes round again).
    fn trigger(&mut self, tier: Tier, amp: f32, f: Option<f32>, width: f32) {
        // a free voice, or the quietest one
        let mut best = 0;
        let mut lo = f32::MAX;
        for (i, c) in self.clicks.iter().enumerate() {
            let e = if c.on { c.env * c.amp.abs() } else { -1.0 };
            if e < lo {
                lo = e;
                best = i;
            }
        }
        let sr = self.sr;
        let rng = &mut self.rng;
        let (fc, q, ms, lowpass) = match tier {
            // a dust grain: a tiny edge, its top end rounded by the stylus
            Tier::Tick => (3000.0 * 2.3f32.powf(rng.uni()), 0.5 + 0.3 * rng.uni(), 0.03 + 0.12 * rng.uni(), true),
            // a click: a short burst in the 0.7–4.5 kHz band where real crackle lives
            Tier::Click => (700.0 * 6.4f32.powf(rng.uni()), 0.7 + 0.9 * rng.uni(), 0.2 + 0.7 * rng.uni(), false),
            // a pop: a dull thump from a deep gouge
            Tier::Pop => (150.0 * 4f32.powf(rng.uni()), 0.6 + 0.4 * rng.uni(), 3.0 + 5.0 * rng.uni(), false),
        };
        let fc = f.unwrap_or(fc);
        let pan = 0.5 + (rng.uni() - 0.5) * width;
        let sign = if rng.uni() < 0.5 { -1.0 } else { 1.0 };
        let c = &mut self.clicks[best];
        c.svf.set(fc.min(sr * 0.45), q, sr);
        c.env = 1.0;
        c.dec = (-1.0 / (ms * 0.001 * sr)).exp();
        c.amp = amp * sign;
        c.kick = [amp * sign * 2.0, -amp * sign * 1.3];
        c.lowpass = lowpass;
        c.gl = (1.0 - pan) * 2.0f32.sqrt();
        c.gr = pan * 2.0f32.sqrt();
        c.on = true;
        self.started = (self.started + 1) & 0x00FF_FFFF;
    }

    /// One stereo sample of the current type at full level; `hiss_mod` scales the hiss.
    #[inline]
    fn tick(&mut self, hiss_mod: f32) -> (f32, f32) {
        let t = TYPES[self.ty.min(TYPES.len() - 1)];
        let sr = self.sr;
        let mut out = [0.0f32; 2];
        if t.hiss > 0.0 {
            for (c, o) in out.iter_mut().enumerate() {
                let w = self.rng.bip() * 1.732 * self.norm;
                let p = Self::pink(&mut self.pink[c], w);
                let h = (1.0 - t.white) * p + t.white * w * 0.5;
                let (lp, _, _) = self.hiss_lp[c].tick(h);
                let (_, _, hp) = self.hiss_hp[c].tick(lp);
                *o += hp * t.hiss * hiss_mod;
            }
        }
        // a record swells and fades about ±2 dB once per revolution; tape drifts and breathes
        let mut breath = hiss_mod;
        let wob = if t.wob > 0.0 { 1.0 + t.wob * self.wob.tick(t.wob_hz / sr, &mut self.rng) } else { 1.0 };
        if t.rpm > 0.0 {
            let prev = self.rev_ph;
            self.rev_ph += t.rpm / 60.0 / sr;
            let wrapped = self.rev_ph >= 1.0;
            if wrapped {
                self.rev_ph -= 1.0;
            }
            // a 78 grinds harder with its revolution than a quiet LP
            let depth = if t.rpm > 50.0 { 0.45 } else { 0.25 };
            breath = 1.0 + depth * (TAU * self.rev_ph).sin();
            for k in 0..t.marks.min(MAX_MARKS) {
                let m = self.marks[k];
                let hit = if wrapped { m.ph > prev || m.ph <= self.rev_ph } else { m.ph > prev && m.ph <= self.rev_ph };
                if hit {
                    let a = m.amp * (0.8 + 0.4 * self.rng.uni());
                    self.trigger(Tier::Click, a, Some(m.f), t.width);
                }
            }
        }
        if t.surf > 0.0 {
            for (c, o) in out.iter_mut().enumerate() {
                let (_, bp, _) = self.surf[c].tick(self.rng.bip() * self.norm);
                *o += bp * t.surf * breath;
            }
        }
        if t.grit > 0.0 && self.rng.uni() < t.grit / sr {
            let a = t.grit_amp * self.rng.bip() * breath;
            let pan = 0.5 + (self.rng.uni() - 0.5) * t.width;
            out[0] += a * (1.0 - pan) * 2.0;
            out[1] += a * pan * 2.0;
        }
        if t.ticks > 0.0 && self.rng.uni() < t.ticks / sr {
            let u = self.rng.uni();
            self.trigger(Tier::Tick, t.tick_amp * u * u * u * u, None, t.width);
        }
        if t.clicks > 0.0 && self.rng.uni() < t.clicks / sr {
            let u = self.rng.uni();
            self.trigger(Tier::Click, t.click_amp * u * u * u, None, t.width);
        }
        if t.pops > 0.0 && self.rng.uni() < t.pops / sr {
            let u = self.rng.uni();
            self.trigger(Tier::Pop, t.pop_amp * (0.4 + 0.6 * u), None, t.width);
        }
        for i in 0..VOICES {
            if !self.clicks[i].on {
                continue;
            }
            let n = self.rng.bip() * self.norm;
            let c = &mut self.clicks[i];
            let e = c.amp * c.env * n + c.kick[0];
            c.kick = [c.kick[1], 0.0];
            c.env *= c.dec;
            let (lp, bp, _) = c.svf.tick(e * 2.0);
            let y = if c.lowpass { lp } else { bp };
            out[0] += y * c.gl;
            out[1] += y * c.gr;
            if c.env < 1e-4 && c.svf.ic1.abs() < 1e-6 && c.svf.ic2.abs() < 1e-6 {
                c.on = false;
            }
        }
        if t.rumble > 0.0 {
            let (lp, _, _) = self.rumble.tick(self.rng.bip() * self.norm);
            // a mono rumble: the stylus moves both walls of the groove the same way at the bottom
            out[0] += lp * t.rumble * 6.0;
            out[1] += lp * t.rumble * 6.0;
        }
        if t.hum > 0.0 {
            let dv = self.hum_drift.tick(0.08 / sr, &mut self.rng);
            self.hum_ph += (50.0 + 0.08 * dv) / sr;
            if self.hum_ph >= 1.0 {
                self.hum_ph -= 1.0;
            }
            let p = TAU * self.hum_ph;
            let s1 = p.sin();
            // the rectifier's full-wave ripple (even harmonics) and the transformer's buzz (odd,
            // up to ~2 kHz), over the fundamental and its first harmonics
            let ripple = s1.abs() - std::f32::consts::FRAC_2_PI;
            let buzz = (6.0 * s1).tanh() - s1;
            self.buzz_lp += (buzz - self.buzz_lp) * self.buzz_k;
            let flicker = wob;
            let h = t.hum * (0.75 * s1 + 0.45 * (2.0 * p).sin() + 0.28 * (3.0 * p).sin() + (0.2 * ripple + 0.14 * self.buzz_lp) * flicker);
            out[0] += h;
            out[1] += h;
        }
        // the medium's stereo width (a lateral-cut 78 is mono), and its level wander
        let mid = 0.5 * (out[0] + out[1]);
        let side = 0.5 * (out[0] - out[1]) * t.width;
        let g = if t.hum > 0.0 && t.hiss < t.hum { 1.0 } else { wob };
        out = [(mid + side) * g, (mid - side) * g];
        for (c, o) in out.iter_mut().enumerate() {
            let (_, _, hp) = self.band_hp[c].tick(*o);
            let (mut y, _, _) = self.band_lp[c][0].tick(hp);
            if t.steep {
                y = self.band_lp[c][1].tick(y).0;
            }
            *o = y;
        }
        (out[0], out[1])
    }
}

// ------------------------------------------------------------------ dropouts

#[derive(Clone, Copy, Default, PartialEq)]
enum DropState {
    #[default]
    Idle,
    Attack,
    Hold,
    Release,
}

/// Random dropouts: a Poisson stream of dips with a random depth, attack, hold and release.
#[derive(Clone, Copy)]
struct Dropouts {
    rng: Rng,
    state: DropState,
    env: f32,
    depth: f32,
    step_a: f32,
    step_r: f32,
    hold: u32,
}

impl Dropouts {
    fn new(seed: u32) -> Self {
        Self { rng: Rng(seed), state: DropState::Idle, env: 0.0, depth: 0.0, step_a: 0.0, step_r: 0.0, hold: 0 }
    }
    /// Advance `n` samples at amount `a` (0..1); returns the depth 0..1.
    fn advance(&mut self, a: f32, n: usize, sr: f32) -> f32 {
        let n32 = n as f32;
        match self.state {
            DropState::Idle => {
                self.env = 0.0;
                let rate = if a > 0.001 { 0.15 + 3.4 * a * a } else { 0.0 };
                if self.rng.uni() < rate * n32 / sr {
                    let r = &mut self.rng;
                    self.depth = (0.3 + 0.7 * r.uni()) * a.sqrt().min(1.0);
                    // the tape lifts off and settles back over tens of milliseconds, never in a step
                    let att = 0.008 + 0.022 * r.uni();
                    let hold = 0.005 + 0.06 * r.uni() * r.uni();
                    let rel = 0.03 + 0.13 * r.uni();
                    self.step_a = self.depth / (att * sr);
                    self.step_r = self.depth / (rel * sr);
                    self.hold = (hold * sr) as u32;
                    self.state = DropState::Attack;
                }
            }
            DropState::Attack => {
                self.env += self.step_a * n32;
                if self.env >= self.depth {
                    self.env = self.depth;
                    self.state = DropState::Hold;
                }
            }
            DropState::Hold => {
                self.hold = self.hold.saturating_sub(n as u32);
                if self.hold == 0 {
                    self.state = DropState::Release;
                }
            }
            DropState::Release => {
                self.env -= self.step_r * n32;
                if self.env <= 0.0 {
                    self.env = 0.0;
                    self.state = DropState::Idle;
                }
            }
        }
        self.env
    }
}

// ------------------------------------------------------------------ settings

/// Parameters in natural units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub magnitude: f32,
    pub noise: f32,
    pub ntype: usize,
    pub ntone: f32,
    pub follow: f32,
    pub pre: bool,
    pub wobble: f32,
    pub wrate: f32,
    pub flutter: f32,
    pub wstereo: f32,
    pub drive: f32,
    pub dstyle: usize,
    pub dlow: f32,
    pub dhigh: f32,
    pub digital: f32,
    pub srate: f32,
    pub bits: f32,
    pub smooth: f32,
    pub wear: f32,
    pub tape: usize,
    pub drops: f32,
    pub drop_stereo: bool,
    pub wander: f32,
    pub lowcut: f32,
    pub highcut: f32,
    pub tone: f32,
    pub width: f32,
    pub out_db: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_values(&[0.0; crate::processor::MAX_PARAMS])
    }
}

impl Settings {
    pub fn from_values(v: &[f32; crate::processor::MAX_PARAMS]) -> Self {
        let pct = |i: usize| (v[i] / 100.0).clamp(0.0, 1.0);
        let bip = |i: usize| (v[i] / 100.0).clamp(-1.0, 1.0);
        let ch = |i: usize, n: usize| (v[i].round().max(0.0) as usize).min(n - 1);
        Self {
            magnitude: pct(idx::AMOUNT),
            noise: pct(idx::NOISE),
            ntype: ch(idx::NTYPE, NOISE_TYPES.len()),
            ntone: bip(idx::NTONE),
            follow: bip(idx::NFOLLOW),
            pre: v[idx::NROUTE] >= 0.5,
            wobble: pct(idx::WOBBLE),
            wrate: v[idx::WRATE].clamp(0.1, 8.0),
            flutter: pct(idx::FLUTTER),
            wstereo: pct(idx::WSTEREO),
            drive: pct(idx::DRIVE),
            dstyle: ch(idx::DSTYLE, STYLES.len()),
            dlow: v[idx::DLOW].clamp(20.0, 2000.0),
            dhigh: v[idx::DHIGH].clamp(500.0, 20000.0),
            digital: pct(idx::DIGITAL),
            srate: v[idx::SRATE].clamp(1000.0, 48000.0),
            bits: v[idx::BITS].clamp(2.0, 16.0),
            smooth: pct(idx::DSMOOTH),
            wear: pct(idx::WEAR),
            tape: ch(idx::TAPE, TAPES.len()),
            drops: pct(idx::DROPS),
            drop_stereo: v[idx::DSTEREO] >= 0.5,
            wander: pct(idx::WANDER),
            lowcut: v[idx::LOWCUT].clamp(20.0, 1000.0),
            highcut: v[idx::HIGHCUT].clamp(1000.0, 20000.0),
            tone: bip(idx::TONE),
            width: (v[idx::WIDTH] / 100.0).clamp(0.0, 2.0),
            out_db: v[idx::OUT].clamp(-24.0, 12.0),
        }
    }
}

/// Module amounts after Magnitude (before Wander), each on its own curve: drive and wear come
/// in first, noise and dropouts last.
pub fn scaled(s: &Settings) -> [f32; 6] {
    let m = s.magnitude;
    [s.noise * m.powf(1.5), s.wobble * m.powf(1.3), s.drive * m, s.digital * m.powf(1.3), s.wear * m, s.drops * m * m]
}

/// Head-loss low-pass at full wear by tape speed, Hz.
const TAPE_HZ: [f32; 3] = [11000.0, 15000.0, 19000.0];
/// Head bump by tape speed, Hz.
const BUMP_HZ: [f32; 3] = [60.0, 80.0, 110.0];

/// Wear's head-loss cutoff, Hz, at wear `w` (0..1) on `tape`.
pub fn wear_cutoff(w: f32, tape: usize, sr: f32) -> f32 {
    let base = TAPE_HZ[tape.min(2)] * 2f32.powf(-2.5 * w);
    log_lerp((sr * 0.45).min(22000.0), base, w * 6.0)
}

/// Wobble depth (fraction of the speed) at amount `a`.
pub fn wow_depth(a: f32) -> f32 {
    WOW_MAX * a.clamp(0.0, 1.0).powf(1.5)
}

// ------------------------------------------------------------------ the effect

pub struct Patina {
    sr: f32,
    want: Settings,
    /// samples left in the current control block (blocks are global, not per call)
    left: usize,
    rng: Rng,
    wander: [Drift; 5],
    // noise
    noise: NoiseGen,
    nbuf: [Vec<f32>; 2],
    s_ngain: Smooth,
    s_pre: Smooth,
    n_env: f32,
    n_att: f32,
    n_rel: f32,
    tone_lp: [f32; 2],
    tone_k: f32,
    n_gl: f32,
    n_gh: f32,
    hiss_drift: Drift,
    // wobble
    buf: [Vec<f32>; 2],
    mask: usize,
    w: usize,
    wow_ph: f32,
    flut_ph: f32,
    wow_rnd: [Drift; 2],
    flut_rnd: [Drift; 2],
    s_aw: Smooth,
    s_af: Smooth,
    t_aw: f32,
    t_af: f32,
    wow_hz: f32,
    rnd_share: f32,
    wst: f32,
    prev_d: [f32; 2],
    cents: [f32; 2],
    // drive
    wet: Core,
    dry: Core,
    xlo: [Lr4; 2],
    xhi: [Lr4; 2],
    ap: [Ap2; 2],
    band: [Vec<f32>; 2],
    rest: [Vec<f32>; 2],
    xo: (f32, f32),
    /// 1 while the band is narrower than the full range (the split is in), 0 when not
    s_split: Smooth,
    split: f32,
    drive_peak: f32,
    // digital
    dph: f32,
    held: [f32; 2],
    prev_x: [f32; 2],
    smooth: [[Svf; 2]; 2],
    s_dmix: Smooth,
    d_ratio: f32,
    d_bits: f32,
    d_smooth_on: bool,
    // wear
    loss: [Svf; 2],
    bump: [Biquad; 2],
    bump_db: f32,
    glue_env: f32,
    glue_att: f32,
    glue_rel: f32,
    glue_gr: f32,
    drops: [Dropouts; 2],
    wear_am: Drift,
    wear_fl: Drift,
    s_wear: Smooth,
    wear_hz: [f32; 2],
    drop_env: [f32; 2],
    // master
    hp: [Biquad; 2],
    lp: [Biquad; 2],
    lo_sh: [Biquad; 2],
    hi_sh: [Biquad; 2],
    m_set: (f32, f32, f32),
    s_width: Smooth,
    s_out: Smooth,
    // meters
    eff: [f32; 6],
    noise_peak: f32,
    fresh: bool,
}

impl Default for Patina {
    fn default() -> Self {
        Self {
            sr: 48000.0,
            want: Settings::default(),
            left: 0,
            rng: Rng(0x2545_F491),
            wander: [Drift::default(); 5],
            noise: NoiseGen::default(),
            nbuf: [Vec::new(), Vec::new()],
            s_ngain: Smooth::default(),
            s_pre: Smooth::default(),
            n_env: 0.0,
            n_att: 0.0,
            n_rel: 0.0,
            tone_lp: [0.0; 2],
            tone_k: 0.1,
            n_gl: 1.0,
            n_gh: 1.0,
            hiss_drift: Drift::default(),
            buf: [Vec::new(), Vec::new()],
            mask: 0,
            w: 0,
            wow_ph: 0.0,
            flut_ph: 0.0,
            wow_rnd: [Drift::default(); 2],
            flut_rnd: [Drift::default(); 2],
            s_aw: Smooth::default(),
            s_af: Smooth::default(),
            t_aw: 0.0,
            t_af: 0.0,
            wow_hz: 0.6,
            rnd_share: 0.3,
            wst: 0.0,
            prev_d: [WOB_MIN; 2],
            cents: [0.0; 2],
            wet: Core::default(),
            dry: Core::default(),
            xlo: [Lr4::default(); 2],
            xhi: [Lr4::default(); 2],
            ap: [Ap2::default(); 2],
            band: [Vec::new(), Vec::new()],
            rest: [Vec::new(), Vec::new()],
            xo: (0.0, 0.0),
            s_split: Smooth::default(),
            split: 0.0,
            drive_peak: 0.0,
            dph: 0.0,
            held: [0.0; 2],
            prev_x: [0.0; 2],
            smooth: [[Svf::default(); 2]; 2],
            s_dmix: Smooth::default(),
            d_ratio: 1.0,
            d_bits: 16.0,
            d_smooth_on: false,
            loss: [Svf::default(); 2],
            bump: [Biquad::default(); 2],
            bump_db: f32::NAN,
            glue_env: 0.0,
            glue_att: 0.0,
            glue_rel: 0.0,
            glue_gr: 0.0,
            drops: [Dropouts::new(0x1234_5679), Dropouts::new(0x7F4A_7C15)],
            wear_am: Drift::default(),
            wear_fl: Drift::default(),
            s_wear: Smooth::default(),
            wear_hz: [20000.0; 2],
            drop_env: [0.0; 2],
            hp: [Biquad::default(); 2],
            lp: [Biquad::default(); 2],
            lo_sh: [Biquad::default(); 2],
            hi_sh: [Biquad::default(); 2],
            m_set: (f32::NAN, f32::NAN, f32::NAN),
            s_width: Smooth::default(),
            s_out: Smooth::default(),
            eff: [0.0; 6],
            noise_peak: 0.0,
            fresh: true,
        }
    }
}

impl Patina {
    /// Recompute the block targets (once per [`BLOCK`] samples).
    fn block_update(&mut self) {
        let s = self.want;
        let sr = self.sr;
        let inc = BLOCK as f32 / sr;
        let wd = s.wander;
        let mut wv = [0.0f32; 5];
        for (i, d) in self.wander.iter_mut().enumerate() {
            wv[i] = d.tick(inc * (0.25 + 0.12 * i as f32), &mut self.rng) * wd;
        }
        let k = scaled(&s);
        let noise = (k[0] * (1.0 + 0.35 * wv[0])).clamp(0.0, 1.0);
        let wobble = k[1];
        let drive = (k[2] * (1.0 + 0.25 * wv[1])).clamp(0.0, 1.0);
        let digital = k[3];
        let wear = (k[4] * (1.0 + 0.3 * wv[3])).clamp(0.0, 1.0);
        self.eff = [noise, wobble, drive, digital, wear, k[5]];

        // noise
        self.noise.set_type(s.ntype);
        let t = s.ntone;
        self.n_gl = db_to_gain(-9.0 * t);
        self.n_gh = db_to_gain(9.0 * t);

        // wobble: pitch depth → delay swing (samples), capped by the line
        let depth = wow_depth(wobble);
        let fl = s.flutter;
        self.wow_hz = s.wrate;
        let wow = depth * (1.0 - 0.7 * fl);
        let flut = depth * fl * 0.35;
        let flut_hz = 9.0;
        self.t_aw = (wow / (TAU * self.wow_hz) * sr).min(WOB_MAX_S * sr);
        self.t_af = (flut / (TAU * flut_hz) * sr).min(0.002 * sr);
        self.rnd_share = 0.25 + 0.6 * wd;
        self.wst = s.wstereo;

        // drive
        let style = s.dstyle;
        let ws = SatSettings { style, drive, auto: true, mix: (drive * 8.0).min(1.0), ..SatSettings::default() };
        self.wet.set(&ws);
        let ds = SatSettings { style, drive: 0.0, auto: false, mix: 0.0, ..SatSettings::default() };
        self.dry.set(&ds);
        let lo = s.dlow;
        let hi = s.dhigh.max(lo * 1.25).min(sr * 0.45);
        // a full-range band needs no split, whose all-passes would turn the phase of the bass
        self.split = if lo > 20.5 || s.dhigh < 19900.0 { 1.0 } else { 0.0 };
        if (lo, hi) != self.xo {
            self.xo = (lo, hi);
            for c in 0..2 {
                self.xlo[c].set(lo as f64, sr as f64);
                self.xhi[c].set(hi as f64, sr as f64);
                self.ap[c].set(hi as f64, sr as f64);
            }
        }

        // digital
        let rate = (s.srate * 2f32.powf(0.25 * wv[2])).clamp(500.0, sr);
        self.d_ratio = (rate / sr).min(1.0);
        self.d_bits = s.bits;
        let fc = log_lerp(sr * 0.49, (rate * 0.3).max(200.0), s.smooth);
        self.d_smooth_on = s.smooth > 0.005;
        for st in &mut self.smooth {
            for f in st.iter_mut() {
                f.set(fc, 0.707, sr);
            }
        }

        // wear
        let tape = s.tape;
        let drop = k[5];
        for c in 0..2 {
            let src = if s.drop_stereo || c == 0 { c } else { 0 };
            if src == c {
                self.drop_env[c] = self.drops[c].advance(drop, BLOCK, sr);
            } else {
                self.drop_env[c] = self.drop_env[0];
            }
            let fc = wear_cutoff(wear, tape, sr) * (1.0 - 0.8 * self.drop_env[c]);
            self.wear_hz[c] = fc;
            self.loss[c].set(fc, 0.6, sr);
        }
        let bump = 3.5 * wear;
        if (bump - self.bump_db).abs() > 0.01 || self.bump_db.is_nan() {
            self.bump_db = bump;
            for b in &mut self.bump {
                b.set(FilterType::Peak, sr, BUMP_HZ[tape.min(2)], 0.9, bump);
            }
        }

        // master: neutral at Magnitude 0
        let m = s.magnitude;
        let lc = log_lerp(20.0, s.lowcut, m);
        let hc = log_lerp(20000.0, s.highcut, m);
        let tl = s.tone * m;
        if (lc, hc, tl) != self.m_set {
            self.m_set = (lc, hc, tl);
            for c in 0..2 {
                self.hp[c].set(FilterType::Highpass, sr, lc, 0.707, 0.0);
                self.lp[c].set(FilterType::Lowpass, sr, hc.min(sr * 0.45), 0.707, 0.0);
                self.lo_sh[c].set(FilterType::LowShelf, sr, 900.0, 0.6, -3.0 * tl);
                self.hi_sh[c].set(FilterType::HighShelf, sr, 900.0, 0.6, 3.0 * tl);
            }
        }
    }

    fn noise_stage(&mut self, l: &[f32], r: &[f32]) {
        let n = l.len();
        let s = self.want;
        let g = self.eff[0];
        let target = g * g.sqrt();
        let follow = s.follow;
        let hiss_mod = 1.0 + TYPES[s.ntype].drift * self.hiss_drift.tick(n as f32 * 0.4 / self.sr, &mut self.rng);
        let mut peak = 0.0f32;
        for i in 0..n {
            let a = l[i].abs().max(r[i].abs());
            let k = if a > self.n_env { self.n_att } else { self.n_rel };
            self.n_env = flush(self.n_env + (a - self.n_env) * k);
            let gain = self.s_ngain.next(target);
            if gain < 1e-6 {
                self.nbuf[0][i] = 0.0;
                self.nbuf[1][i] = 0.0;
                continue;
            }
            let (x0, x1) = self.noise.tick(hiss_mod);
            let e = (self.n_env * 10.0).min(2.0);
            let dynamic = if follow >= 0.0 { (1.0 - follow) + follow * e } else { 1.0 / (1.0 - 6.0 * follow * e) };
            let gg = gain * dynamic;
            for (c, x) in [x0, x1].into_iter().enumerate() {
                self.tone_lp[c] = flush(self.tone_lp[c] + (x - self.tone_lp[c]) * self.tone_k);
                let y = (self.tone_lp[c] * self.n_gl + (x - self.tone_lp[c]) * self.n_gh) * gg;
                self.nbuf[c][i] = y;
                peak = peak.max(y.abs());
            }
        }
        self.noise_peak = peak.max(self.noise_peak * 0.9);
    }

    fn add_noise(&mut self, l: &mut [f32], r: &mut [f32], pre: bool) {
        let target = if self.want.pre { 1.0 } else { 0.0 };
        for i in 0..l.len() {
            // the route glides, so switching it never clicks
            let w = if pre { self.s_pre.next(target) } else { 1.0 - self.s_pre.v };
            l[i] += self.nbuf[0][i] * w;
            r[i] += self.nbuf[1][i] * w;
        }
    }

    #[inline]
    fn read(buf: &[f32], mask: usize, w: usize, d: f32) -> f32 {
        let di = d as usize;
        let f = d - di as f32;
        let y0 = buf[(w + 1).wrapping_sub(di) & mask];
        let y1 = buf[w.wrapping_sub(di) & mask];
        let y2 = buf[w.wrapping_sub(di + 1) & mask];
        let y3 = buf[w.wrapping_sub(di + 2) & mask];
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * f + c2) * f + c1) * f + y1
    }

    fn wobble_stage(&mut self, l: &mut [f32], r: &mut [f32]) {
        let sr = self.sr;
        let inc_w = self.wow_hz / sr;
        let inc_f = 9.0 / sr;
        let share = self.rnd_share;
        let wst = self.wst;
        let mask = self.mask;
        for i in 0..l.len() {
            let aw = self.s_aw.next(self.t_aw);
            let af = self.s_af.next(self.t_af);
            self.w = (self.w + 1) & mask;
            self.buf[0][self.w] = l[i];
            self.buf[1][self.w] = r[i];
            self.wow_ph += inc_w;
            if self.wow_ph >= 1.0 {
                self.wow_ph -= 1.0;
            }
            self.flut_ph += inc_f;
            if self.flut_ph >= 1.0 {
                self.flut_ph -= 1.0;
            }
            if aw + af < 1e-4 {
                // still: read the line at its fixed minimum, the same latency as when moving
                self.cents = [0.0; 2];
                self.prev_d = [WOB_MIN; 2];
                l[i] = self.buf[0][self.w.wrapping_sub(WOB_MIN as usize) & mask];
                r[i] = self.buf[1][self.w.wrapping_sub(WOB_MIN as usize) & mask];
                continue;
            }
            let rw0 = self.wow_rnd[0].tick(inc_w * 1.3, &mut self.rng);
            let rw1 = self.wow_rnd[1].tick(inc_w * 1.3, &mut self.rng);
            let rf0 = self.flut_rnd[0].tick(inc_f, &mut self.rng);
            let rf1 = self.flut_rnd[1].tick(inc_f, &mut self.rng);
            let sl = (TAU * self.wow_ph).sin();
            let sr_ = (TAU * (self.wow_ph + 0.25 * wst)).sin();
            let fl = (TAU * self.flut_ph).sin();
            let fr = (TAU * (self.flut_ph + 0.5 * wst)).sin();
            let ml = (1.0 - share) * sl + share * rw0;
            let mr = (1.0 - share) * sr_ + share * (rw0 + (rw1 - rw0) * wst);
            let fml = 0.5 * fl + 0.5 * rf0;
            let fmr = 0.5 * fr + 0.5 * (rf0 + (rf1 - rf0) * wst);
            let dl = WOB_MIN + aw * (1.0 + ml) + af * (1.0 + fml);
            let dr = WOB_MIN + aw * (1.0 + mr) + af * (1.0 + fmr);
            self.cents[0] = (self.prev_d[0] - dl) * 1731.234;
            self.cents[1] = (self.prev_d[1] - dr) * 1731.234;
            self.prev_d = [dl, dr];
            l[i] = Self::read(&self.buf[0], mask, self.w, dl);
            r[i] = Self::read(&self.buf[1], mask, self.w, dr);
        }
    }

    fn drive_stage(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let mut pk = 0.0f32;
        // a full-range band (and the glide back to it over) skips the split and the clean core:
        // the second core is fed nothing then, so it is left holding nothing
        let splitting = self.split > 0.0 || self.s_split.v > 1e-6;
        if !splitting {
            self.s_split.v = 0.0;
            for i in 0..n {
                self.band[0][i] = l[i];
                self.band[1][i] = r[i];
                pk = pk.max(l[i].abs()).max(r[i].abs());
            }
        } else {
            for i in 0..n {
                let w = self.s_split.next(self.split);
                for (c, x) in [l[i], r[i]].into_iter().enumerate() {
                    let (lo, rest) = self.xlo[c].split(x as f64);
                    let (mid, hi) = self.xhi[c].split(rest);
                    let lo = self.ap[c].run(lo);
                    let b = x + (mid as f32 - x) * w;
                    self.band[c][i] = b;
                    self.rest[c][i] = (lo + hi) as f32 * w;
                    pk = pk.max(b.abs());
                }
            }
        }
        self.drive_peak = pk.max(self.drive_peak * 0.9);
        let [b0, b1] = &mut self.band;
        self.wet.process(&mut b0[..n], Some(&mut b1[..n]));
        if splitting {
            let [r0, r1] = &mut self.rest;
            self.dry.process(&mut r0[..n], Some(&mut r1[..n]));
            for i in 0..n {
                l[i] = self.band[0][i] + self.rest[0][i];
                r[i] = self.band[1][i] + self.rest[1][i];
            }
        } else {
            l.copy_from_slice(&self.band[0][..n]);
            r.copy_from_slice(&self.band[1][..n]);
        }
    }

    fn digital_stage(&mut self, l: &mut [f32], r: &mut [f32]) {
        let target = self.eff[3];
        let ratio = self.d_ratio;
        let levels = 2f32.powf(self.d_bits - 1.0);
        let inv = 1.0 / levels;
        for i in 0..l.len() {
            let m = self.s_dmix.next(target);
            let x = [l[i], r[i]];
            self.dph += ratio;
            if self.dph >= 1.0 {
                self.dph -= 1.0;
                // the crossing fell `dph / ratio` samples ago: latch the input as it was then
                let f = (self.dph / ratio).clamp(0.0, 1.0);
                for c in 0..2 {
                    let v = x[c] + (self.prev_x[c] - x[c]) * f;
                    self.held[c] = (v * levels).round() * inv;
                }
            }
            self.prev_x = x;
            if m < 1e-5 {
                continue;
            }
            let mut y = self.held;
            if self.d_smooth_on {
                for (c, yc) in y.iter_mut().enumerate() {
                    let (a, _, _) = self.smooth[c][0].tick(*yc);
                    let (b, _, _) = self.smooth[c][1].tick(a);
                    *yc = b;
                }
            }
            l[i] = x[0] + (y[0] - x[0]) * m;
            r[i] = x[1] + (y[1] - x[1]) * m;
        }
    }

    fn wear_stage(&mut self, l: &mut [f32], r: &mut [f32]) {
        let target = self.eff[4];
        let n = l.len();
        let inc = n as f32 / self.sr;
        let am = self.wear_am.tick(inc * 0.8, &mut self.rng);
        let fl = self.wear_fl.tick(inc * 14.0, &mut self.rng);
        let mut gr_min = 0.0f32;
        for i in 0..n {
            let k = self.s_wear.next(target);
            let dropping = self.drop_env[0] > 0.0 || self.drop_env[1] > 0.0;
            if k < 1e-5 && !dropping {
                // keep the filters warm (no click when the knob comes up)
                self.loss[0].tick(l[i]);
                self.loss[1].tick(r[i]);
                continue;
            }
            let det = 0.5 * (l[i].abs() + r[i].abs());
            let c = if det > self.glue_env { self.glue_att } else { self.glue_rel };
            self.glue_env = flush(self.glue_env + (det - self.glue_env) * c);
            let over = (self.glue_env / 0.25).max(1.0);
            let glue = over.powf(-0.45 * k);
            gr_min = gr_min.min(gain_to_db(glue));
            let level = glue * (1.0 + k * (0.08 * am + 0.025 * fl));
            let x = [l[i], r[i]];
            let mut y = [0.0f32; 2];
            for ch in 0..2 {
                let b = self.bump[ch].tick(x[ch]);
                let (lp, _, _) = self.loss[ch].tick(b);
                // a dropout darkens the top end too, also at zero wear
                let w = (k * 20.0).max(3.0 * self.drop_env[ch]).min(1.0);
                let worn = lp * level * (1.0 - 0.85 * self.drop_env[ch]);
                // a dropout also lands at zero wear (Dropouts is its own knob)
                let dry_drop = x[ch] * (1.0 - 0.85 * self.drop_env[ch]);
                y[ch] = dry_drop + (worn - dry_drop) * w;
            }
            l[i] = y[0];
            r[i] = y[1];
        }
        self.glue_gr = gr_min;
    }

    fn master_stage(&mut self, l: &mut [f32], r: &mut [f32]) {
        let s = self.want;
        let m = s.magnitude;
        let width = 1.0 + (s.width - 1.0) * m;
        let out = db_to_gain(s.out_db);
        let (lc, hc, tl) = self.m_set;
        let use_hp = lc > 20.5;
        let use_lp = hc < 19900.0;
        let use_tilt = tl.abs() > 1e-3;
        for i in 0..l.len() {
            let mut x = [l[i], r[i]];
            for (c, v) in x.iter_mut().enumerate() {
                let a = self.hp[c].tick(*v);
                let b = self.lp[c].tick(if use_hp { a } else { *v });
                let mut y = if use_lp {
                    b
                } else if use_hp {
                    a
                } else {
                    *v
                };
                let s1 = self.lo_sh[c].tick(y);
                let s2 = self.hi_sh[c].tick(s1);
                if use_tilt {
                    y = s2;
                }
                *v = y;
            }
            let wd = self.s_width.next(width);
            let o = self.s_out.next(out);
            let mid = 0.5 * (x[0] + x[1]);
            let side = 0.5 * (x[0] - x[1]) * wd;
            l[i] = (mid + side) * o;
            r[i] = (mid - side) * o;
            if !l[i].is_finite() || !r[i].is_finite() {
                l[i] = 0.0;
                r[i] = 0.0;
            }
        }
    }
}

impl Processor for Patina {
    fn prepare(&mut self, sr: f32, max_block: usize) {
        self.sr = sr;
        let len = ((2.0 * WOB_MAX_S + 0.004) * sr) as usize + 8;
        let size = len.next_power_of_two();
        self.mask = size - 1;
        self.buf = [vec![0.0; size], vec![0.0; size]];
        let mb = max_block.max(BLOCK);
        self.nbuf = [vec![0.0; mb], vec![0.0; mb]];
        self.band = [vec![0.0; mb], vec![0.0; mb]];
        self.rest = [vec![0.0; mb], vec![0.0; mb]];
        self.noise.prepare(sr);
        self.s_ngain.setup(sr, 30.0);
        self.s_pre.setup(sr, 20.0);
        self.s_aw.setup(sr, 80.0);
        self.s_af.setup(sr, 80.0);
        self.s_dmix.setup(sr, 20.0);
        self.s_split.setup(sr, 20.0);
        self.s_wear.setup(sr, 30.0);
        self.s_width.setup(sr, 20.0);
        self.s_out.setup(sr, 20.0);
        self.n_att = 1.0 - (-1.0 / (0.005 * sr)).exp();
        self.n_rel = 1.0 - (-1.0 / (0.15 * sr)).exp();
        self.tone_k = 1.0 - (-TAU * 1000.0 / sr).exp();
        self.glue_att = 1.0 - (-1.0 / (0.002 * sr)).exp();
        self.glue_rel = 1.0 - (-1.0 / (0.085 * sr)).exp();
        self.wet.prepare(sr, 2);
        self.dry.prepare(sr, 2);
        self.xo = (0.0, 0.0);
        self.m_set = (f32::NAN, f32::NAN, f32::NAN);
        self.bump_db = f32::NAN;
        self.fresh = true;
        self.reset();
    }

    fn set_params(&mut self, p: &ParamBlock) {
        self.want = Settings::from_values(&p.values);
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessCtx) {
        let n = l.len().min(r.len());
        let mut i0 = 0;
        while i0 < n {
            if self.left == 0 {
                self.block_update();
                self.left = BLOCK;
                if self.fresh {
                    // the first block starts at its targets: no fade-in from silence
                    self.fresh = false;
                    self.s_ngain.v = self.eff[0] * self.eff[0].sqrt();
                    self.s_pre.v = if self.want.pre { 1.0 } else { 0.0 };
                    self.s_aw.v = self.t_aw;
                    self.s_af.v = self.t_af;
                    self.s_dmix.v = self.eff[3];
                    self.s_split.v = self.split;
                    self.s_wear.v = self.eff[4];
                    let m = self.want.magnitude;
                    self.s_width.v = 1.0 + (self.want.width - 1.0) * m;
                    self.s_out.v = db_to_gain(self.want.out_db);
                }
            }
            let i1 = (i0 + self.left).min(n);
            self.left -= i1 - i0;
            let (bl, br) = (&mut l[i0..i1], &mut r[i0..i1]);
            self.noise_stage(bl, br);
            self.add_noise(bl, br, true);
            self.wobble_stage(bl, br);
            self.drive_stage(bl, br);
            self.digital_stage(bl, br);
            self.wear_stage(bl, br);
            self.add_noise(bl, br, false);
            self.master_stage(bl, br);
            i0 = i1;
        }
    }

    fn reset(&mut self) {
        for b in &mut self.buf {
            b.iter_mut().for_each(|x| *x = 0.0);
        }
        self.noise.reset();
        self.n_env = 0.0;
        self.tone_lp = [0.0; 2];
        self.prev_d = [WOB_MIN; 2];
        self.cents = [0.0; 2];
        self.wet.reset();
        self.dry.reset();
        for c in 0..2 {
            self.xlo[c].clear();
            self.xhi[c].clear();
            self.ap[c].clear();
            self.loss[c].clear();
            self.bump[c].reset();
            self.hp[c].reset();
            self.lp[c].reset();
            self.lo_sh[c].reset();
            self.hi_sh[c].reset();
            for f in &mut self.smooth[c] {
                f.clear();
            }
        }
        self.held = [0.0; 2];
        self.prev_x = [0.0; 2];
        self.glue_env = 0.0;
        self.drop_env = [0.0; 2];
        for d in &mut self.drops {
            d.state = DropState::Idle;
            d.env = 0.0;
        }
        self.noise_peak = 0.0;
        self.drive_peak = 0.0;
    }

    fn park(&mut self) {
        self.reset();
        self.wet.park();
        self.dry.park();
    }

    fn latency_samples(&self) -> usize {
        latency_samples()
    }

    fn tail_samples(&self) -> Option<usize> {
        // a noise floor is a sound of its own: the chain never sleeps while it is up
        if scaled(&self.want)[0] > 0.0 {
            None
        } else {
            Some((self.sr.max(8000.0) * 0.4) as usize)
        }
    }

    fn meters(&self, out: &mut [f32; METERS]) {
        out[meter::NOISE] = self.noise_peak;
        out[meter::CENTS_L] = self.cents[0];
        out[meter::CENTS_R] = self.cents[1];
        out[meter::DRIVE_IN] = self.drive_peak;
        out[meter::SRATE] = self.d_ratio * self.sr;
        out[meter::BITS] = self.d_bits;
        out[meter::DROP_L] = self.drop_env[0];
        out[meter::DROP_R] = self.drop_env[1];
        out[meter::WEAR_HZ] = 0.5 * (self.wear_hz[0] + self.wear_hz[1]);
        out[meter::WEAR_GR] = self.glue_gr;
        out[meter::EFF..meter::EFF + 5].copy_from_slice(&self.eff[..5]);
        out[meter::MAG] = self.want.magnitude;
        out[meter::CLICKS] = self.noise.started as f32;
    }
}

#[cfg(test)]
#[path = "patina_tests.rs"]
mod tests;
