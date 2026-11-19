//! One synth voice and its building blocks: envelope, state-variable filter, 2× oversampled
//! waveshaper. Everything here runs on the audio thread: no allocation, no locks.

use std::f32::consts::{FRAC_PI_4, PI, SQRT_2};

use super::params::{ix, Patch, DEST_SCALE, SLOTS};
use super::tables::{self, Bank, HB_TAPS, SAW, SINE, SQUARE};

pub const MAX_UNI: usize = 7;
/// Matrix source / destination indices (see `params::SOURCES` / `params::DESTS`).
pub mod src {
    pub const MOD_ENV: u8 = 1;
    pub const LFO1: u8 = 2;
    pub const LFO2: u8 = 3;
    pub const VEL: u8 = 4;
    pub const WHEEL: u8 = 5;
    pub const RANDOM: u8 = 6;
    pub const KEY: u8 = 7;
    pub const LFO1_WHEEL: u8 = 8;
    /// Macro 1..4 are 9..12.
    pub const MACRO1: u8 = 9;
    pub const N: usize = 13;
}
pub mod dst {
    pub const PITCH: usize = 0;
    pub const P1: usize = 1;
    pub const P2: usize = 2;
    pub const SHAPE1: usize = 3;
    pub const SHAPE2: usize = 4;
    pub const LVL1: usize = 5;
    pub const LVL2: usize = 6;
    pub const CUTOFF: usize = 7;
    pub const RES: usize = 8;
    pub const PAN: usize = 9;
    pub const DETUNE: usize = 10;
    pub const DRIVE: usize = 11;
    pub const AMP: usize = 12;
    pub const NOISE: usize = 13;
    pub const WARP1: usize = 14;
    pub const WARP2: usize = 15;
    pub const N: usize = 16;
}

// ---------------------------------------------------------------- envelope

/// Coefficients of an ADSR in Nigel Redmon's iterative form: each stage is a one-pole step
/// towards a target past its end point (attack aims at 1.3, decay/release 80 dB below), so
/// the curve is exponential and still reaches its end in the set time.
#[derive(Clone, Copy, Default)]
pub struct EnvCoef {
    ac: f32,
    ab: f32,
    dc: f32,
    db: f32,
    rc: f32,
    rb: f32,
    pub sus: f32,
}

const RATIO_A: f32 = 0.3;
const RATIO_DR: f32 = 1e-4;

fn coef(samples: f32, ratio: f32) -> f32 {
    if samples <= 1.0 {
        0.0
    } else {
        (-((1.0 + ratio) / ratio).ln() / samples).exp()
    }
}

impl EnvCoef {
    pub fn new(a_ms: f32, d_ms: f32, sus: f32, r_ms: f32, sr: f32) -> Self {
        let ms = sr / 1000.0;
        let ac = coef(a_ms * ms, RATIO_A);
        let dc = coef(d_ms * ms, RATIO_DR);
        let rc = coef(r_ms * ms, RATIO_DR);
        Self { ac, ab: (1.0 + RATIO_A) * (1.0 - ac), dc, db: (sus - RATIO_DR) * (1.0 - dc), rc, rb: -RATIO_DR * (1.0 - rc), sus }
    }
}

pub const IDLE: u8 = 0;
pub const ATTACK: u8 = 1;
pub const DECAY: u8 = 2;
pub const SUSTAIN: u8 = 3;
pub const RELEASE: u8 = 4;

#[derive(Clone, Copy, Default)]
pub struct Env {
    pub v: f32,
    pub st: u8,
}

impl Env {
    /// (Re)starts the attack from the current level (no jump on retrigger).
    pub fn gate(&mut self) {
        self.st = ATTACK;
    }
    pub fn release(&mut self) {
        if self.st != IDLE {
            self.st = RELEASE;
        }
    }
    #[inline(always)]
    pub fn tick(&mut self, c: &EnvCoef) -> f32 {
        match self.st {
            ATTACK => {
                self.v = c.ab + self.v * c.ac;
                if self.v >= 1.0 {
                    self.v = 1.0;
                    self.st = DECAY;
                }
            }
            DECAY => {
                self.v = c.db + self.v * c.dc;
                if self.v <= c.sus {
                    self.v = c.sus;
                    self.st = SUSTAIN;
                }
            }
            SUSTAIN => self.v = c.sus,
            RELEASE => {
                self.v = c.rb + self.v * c.rc;
                if self.v <= 0.0 {
                    self.v = 0.0;
                    self.st = IDLE;
                }
            }
            _ => self.v = 0.0,
        }
        self.v
    }
}

// ---------------------------------------------------------------- filter

/// Andrew Simper's (Cytomic) trapezoidal state-variable filter, one channel.
#[derive(Clone, Copy, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

#[derive(Clone, Copy)]
pub struct SvfCoef {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl SvfCoef {
    #[inline(always)]
    pub fn new(g: f32, k: f32) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { k, a1, a2, a3: g * a2 }
    }
}

pub const LP12: u8 = 0;
pub const LP24: u8 = 1;
pub const BP: u8 = 2;
pub const HP: u8 = 3;
pub const NOTCH: u8 = 4;

impl Svf {
    #[inline(always)]
    pub fn tick(&mut self, v0: f32, c: &SvfCoef, mode: u8) -> f32 {
        let v3 = v0 - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match mode {
            BP => v1,
            HP => v0 - c.k * v1 - v2,
            NOTCH => v0 - c.k * v1,
            _ => v2,
        }
    }
    pub fn flush(&mut self) {
        if self.ic1.abs() < 1e-20 {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < 1e-20 {
            self.ic2 = 0.0;
        }
    }
}

// ---------------------------------------------------------------- 2× oversampled shaper

/// Rational tanh approximation (exact at 0, saturates at ±3).
#[inline(always)]
pub fn tanh(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// A waveshaper run at twice the sample rate through a halfband FIR (31 taps) up and down.
/// Latency: 15 samples. One channel.
#[derive(Clone, Copy)]
pub struct Os2 {
    /// input history, newest last (doubled ring: `x[p..p+16]` is contiguous)
    x: [f32; 32],
    xp: usize,
    /// upsampled history: even-phase samples (8) and odd-phase samples (16)
    e: [f32; 16],
    ep: usize,
    o: [f32; 32],
    op: usize,
}

impl Default for Os2 {
    fn default() -> Self {
        Self { x: [0.0; 32], xp: 0, e: [0.0; 16], ep: 0, o: [0.0; 32], op: 0 }
    }
}

impl Os2 {
    #[inline(always)]
    fn push(buf: &mut [f32], p: &mut usize, half: usize, v: f32) {
        buf[*p] = v;
        buf[*p + half] = v;
        *p += 1;
        if *p == half {
            *p = 0;
        }
    }

    /// One input sample through `f` at 2× rate; returns one output sample.
    #[inline(always)]
    pub fn process(&mut self, x: f32, hb: &[f32; HB_TAPS], f: impl Fn(f32) -> f32) -> f32 {
        Self::push(&mut self.x, &mut self.xp, 16, x);
        let h = &self.x[self.xp..self.xp + 16]; // h[15] = x[n], h[0] = x[n-15]
        let even = h[7];
        let mut odd = 0.0;
        for j in 0..HB_TAPS {
            odd += hb[j] * (h[7 - j] + h[8 + j]);
        }
        let (a, b) = (f(even), f(2.0 * odd));
        Self::push(&mut self.e, &mut self.ep, 8, a);
        Self::push(&mut self.o, &mut self.op, 16, b);
        let e = &self.e[self.ep..self.ep + 8]; // e[7] = newest
        let o = &self.o[self.op..self.op + 16]; // o[15] = newest
        let mut y = 0.5 * e[0];
        for j in 0..HB_TAPS {
            y += hb[j] * (o[7 - j] + o[8 + j]);
        }
        y
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ---------------------------------------------------------------- oscillator warp

/// Warp modes (see `params::WARPS1`).
pub mod warp {
    pub const OFF: u8 = 0;
    pub const SYNC: u8 = 1;
    pub const BEND: u8 = 2;
    pub const ASYM: u8 = 3;
    pub const FM: u8 = 4;
}

/// Width of the window that softens the sync reset (a fraction of the master cycle).
const SYNC_WIN: f32 = 0.035;
/// Asym at 100 %: depth of each of its two sine phase-distortion stages.
const ASYM_MAX: f32 = 0.95;
/// Phase deviation of FM at 100 %, in cycles (peak index 2π).
const FM_DEPTH: f32 = 1.0;

/// How much faster than the oscillator's own pitch a warp reads its table at most (the mip
/// level is chosen for that rate, so a warp never plays a harmonic above Nyquist from the
/// table itself). `fm_ratio`: modulator / carrier frequency.
#[inline(always)]
fn warp_speed(mode: u8, a: f32, fm_ratio: f32) -> f32 {
    match mode {
        warp::SYNC => 1.0 + 7.0 * a,
        warp::BEND => 1.0 + 2.0 * a,
        warp::ASYM => {
            let b = ASYM_MAX * a;
            (1.0 + b) * (1.0 + b)
        }
        warp::FM => 1.0 + std::f32::consts::TAU * FM_DEPTH * a * fm_ratio,
        _ => 1.0,
    }
}

/// The table phase a warp reads at oscillator phase `ph` ∈ [0, 1), and the gain of that sample
/// (the sync window). `m`: the FM modulator's value (−1..1); `dt`: the oscillator's phase step;
/// `sine`: the sine table.
#[inline(always)]
fn warp_phase(mode: u8, a: f32, ph: f32, m: f32, dt: f32, sine: &[f32]) -> (f32, f32) {
    match mode {
        warp::SYNC => {
            // hard sync to the oscillator's own cycle (slave ratio 1..8); the reset is softened
            // by a short raised-cosine dip that deepens with the amount (a step at the reset
            // would alias)
            let q = ph * (1.0 + 7.0 * a);
            let e = ph.min(1.0 - ph);
            // at least eight samples wide: a narrower dip is itself a near-step and aliases
            let win = SYNC_WIN.max(8.0 * dt);
            let g = if e < win {
                let w = tables::read(sine, 0.25 * e / win);
                1.0 - (a * 6.0).min(1.0) * (1.0 - w * w)
            } else {
                1.0
            };
            (q - q.floor(), g)
        }
        warp::BEND => {
            // cubic bend around the cycle's centre: content moves out to the cycle's ends
            let x = 2.0 * ph - 1.0;
            (0.5 + 0.5 * ((1.0 - a) * x + a * x * x * x), 1.0)
        }
        warp::ASYM => {
            // phase distortion, twice: q = p + b·sin(2πp)/2π speeds the start of the cycle up
            // (1 + b) and slows its end (1 − b), with a smooth phase curve (a kinked one, like
            // the classic two-segment PD, aliases)
            let b = ASYM_MAX * a * (1.0 / std::f32::consts::TAU);
            let q1 = ph + b * tables::read(sine, ph);
            let q1 = q1 - q1.floor();
            let q = q1 + b * tables::read(sine, q1);
            (q - q.floor(), 1.0)
        }
        warp::FM => {
            let q = ph + a * FM_DEPTH * m;
            (q - q.floor(), 1.0)
        }
        _ => (ph, 1.0),
    }
}

// ---------------------------------------------------------------- the voice

/// Detune positions of the unison voices (symmetric pairs, irregular so the beating does not
/// fall into a pattern) and their stereo positions (neighbours alternate sides).
const UNI_OFF: [[f32; MAX_UNI]; 4] = [
    [0.0; MAX_UNI],
    [-1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    [-1.0, -0.45, 0.0, 0.45, 1.0, 0.0, 0.0],
    [-1.0, -0.6, -0.27, 0.0, 0.27, 0.6, 1.0],
];
const UNI_PAN: [[f32; MAX_UNI]; 4] = [
    [0.0; MAX_UNI],
    [-1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    [-1.0, 0.5, 0.0, -0.5, 1.0, 0.0, 0.0],
    [-1.0, 0.67, -0.33, 0.0, 0.33, -0.67, 1.0],
];
/// Outer unison voices at 100 % detune: ± half a semitone (x² knob curve).
const MAX_SPREAD_ST: f32 = 0.5;

/// Everything a voice reads that is shared by all voices for one control chunk.
pub struct Ctx<'a> {
    pub bank: &'static Bank,
    pub sr: f32,
    pub p: &'a Patch,
    pub lfo1: &'a [f32],
    pub lfo2: &'a [f32],
    pub wheel: f32,
    /// Pitch bend of live notes, semitones.
    pub bend: f32,
    pub amp: EnvCoef,
    pub menv: EnvCoef,
    /// Macro 1..4, 0..1.
    pub macros: [f32; 4],
    /// Active matrix slots: (source, destination, amount in destination units).
    pub slots: [(u8, usize, f32); SLOTS],
    pub nslots: usize,
}

#[derive(Clone)]
pub struct Voice {
    pub on: bool,
    /// The note's own pan (-1..1), added to the patch's pan.
    pub npan: f32,
    pub note: u32,
    pub key: u8,
    pub vel: f32,
    pub live: bool,
    pub age: u64,
    /// Pitch now and its glide target, in semitones (MIDI key units).
    pub st: f32,
    st_target: f32,
    glide_step: f32,
    glide_left: u32,
    /// Samples since the note started (the LFO rise; a legato slide keeps counting).
    t: u32,
    // discrete settings captured when the note starts (changing them cannot click a ringing note)
    n_uni: usize,
    uni_row: usize,
    wave1: u8,
    wave2: u8,
    warp1: u8,
    warp2: u8,
    ftype: u8,
    sub_div: f32,
    drive_on: bool,
    ph1: [f32; MAX_UNI],
    ph2: [f32; MAX_UNI],
    /// Last output of each unison copy of each oscillator (the FM modulator).
    last1: [f32; MAX_UNI],
    last2: [f32; MAX_UNI],
    ph_sub: f32,
    rng: u32,
    rnd: f32,
    noise_lp: f32,
    svf: [[Svf; 2]; 2],
    os: [Os2; 2],
    pub amp: Env,
    pub menv: Env,
    /// Steal fade: gain and its per-sample step (0 = not fading).
    pub fade: f32,
    pub fade_step: f32,
    /// Fade-in (a restart that takes over from a sounding voice): gain and per-sample step.
    fin: f32,
    fin_step: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            on: false,
            npan: 0.0,
            note: 0,
            key: 0,
            vel: 0.0,
            live: false,
            age: 0,
            st: 60.0,
            st_target: 60.0,
            glide_step: 0.0,
            glide_left: 0,
            t: 0,
            n_uni: 1,
            uni_row: 0,
            wave1: SAW,
            wave2: SAW,
            warp1: warp::OFF,
            warp2: warp::OFF,
            ftype: LP12,
            sub_div: 0.5,
            drive_on: false,
            ph1: [0.0; MAX_UNI],
            ph2: [0.0; MAX_UNI],
            last1: [0.0; MAX_UNI],
            last2: [0.0; MAX_UNI],
            ph_sub: 0.0,
            rng: 1,
            rnd: 0.0,
            noise_lp: 0.0,
            svf: [[Svf::default(); 2]; 2],
            os: [Os2::default(); 2],
            amp: Env::default(),
            menv: Env::default(),
            fade: 1.0,
            fade_step: 0.0,
            fin: 1.0,
            fin_step: 0.0,
        }
    }
}

/// Smoothstep: the gain curve of the steal and take-over fades (a linear fade has a corner at
/// each end, which a loud low note turns into an audible tick).
#[inline(always)]
fn smooth(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

#[inline(always)]
fn xorshift(s: &mut u32) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// One oscillator's tables for one sample: two mip levels × up to two frames, or a pulse
/// made of two saws.
struct Osc<'a> {
    t00: &'a [f32],
    t01: &'a [f32],
    t10: &'a [f32],
    t11: &'a [f32],
    w: f32,
    fm: f32,
    morph: bool,
    pw: f32,
    pulse: bool,
}

impl<'a> Osc<'a> {
    #[inline(always)]
    fn new(b: &'a Bank, wave: u8, shape: f32, dt: f32) -> Self {
        let (j0, j1, w) = b.select(dt);
        if wave == SQUARE {
            let t0 = b.table(SAW, 0, j0);
            let t1 = b.table(SAW, 0, j1);
            return Self { t00: t0, t01: t1, t10: t0, t11: t1, w, fm: 0.0, morph: false, pw: 0.5 - 0.45 * shape, pulse: true };
        }
        // Shape is the position between the wave's frames: two frames morph, a wavetable scans
        let pos = shape * (tables::frames(wave) - 1) as f32;
        let f0 = (pos as usize).min(tables::frames(wave) - 1);
        let fm = pos - f0 as f32;
        let morph = fm > 1e-4;
        Self {
            t00: b.table(wave, f0, j0),
            t01: b.table(wave, f0, j1),
            t10: b.table(wave, f0 + 1, j0),
            t11: b.table(wave, f0 + 1, j1),
            w,
            fm,
            morph,
            pw: 0.5,
            pulse: false,
        }
    }
    #[inline(always)]
    fn lv(&self, a: &[f32], b: &[f32], ph: f32) -> f32 {
        let x = tables::read(a, ph);
        if self.w >= 1.0 {
            x
        } else {
            let y = tables::read(b, ph);
            y + (x - y) * self.w
        }
    }
    #[inline(always)]
    fn sample(&self, ph: f32) -> f32 {
        let a = self.lv(self.t00, self.t01, ph);
        if self.pulse {
            let mut q = ph + self.pw;
            if q >= 1.0 {
                q -= 1.0;
            }
            return a - self.lv(self.t00, self.t01, q);
        }
        if self.morph {
            a + (self.lv(self.t10, self.t11, ph) - a) * self.fm
        } else {
            a
        }
    }
}

impl Voice {
    /// Starts a note in this (free) slot. `from`: pitch to glide from (`None` = start on the note).
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        p: &Patch,
        note: u32,
        key: u8,
        vel: f32,
        live: bool,
        age: u64,
        seed: u32,
        from: Option<f32>,
        glide_samples: f32,
    ) {
        let mut rng = seed | 1;
        let row = (p[ix::UNI_VOICES] as usize).min(3);
        let n_uni = [1, 3, 5, 7][row];
        let drive_mod = (0..SLOTS).any(|s| p[ix::M1_SRC + 3 * s] > 0.5 && p[ix::M1_DST + 3 * s] as usize == dst::DRIVE);
        *self = Voice {
            on: true,
            note,
            key,
            vel,
            live,
            age,
            n_uni,
            uni_row: row,
            wave1: p[ix::O1_WAVE] as u8,
            wave2: p[ix::O2_WAVE] as u8,
            warp1: p[ix::O1_WARP] as u8,
            warp2: p[ix::O2_WARP] as u8,
            ftype: p[ix::FLT_TYPE] as u8,
            sub_div: if p[ix::SUB_OCT] > 0.5 { 0.25 } else { 0.5 },
            drive_on: p[ix::FLT_DRIVE] > 0.0 || drive_mod,
            ..Voice::default()
        };
        if n_uni > 1 {
            for k in 0..n_uni {
                self.ph1[k] = xorshift(&mut rng) * 0.5 + 0.5;
                self.ph2[k] = xorshift(&mut rng) * 0.5 + 0.5;
            }
        } else if self.wave2 != SINE && self.wave2 != tables::TRI && self.warp1 != warp::FM {
            // a single osc 1 starts at phase 0 (a clean, repeatable attack, 808s); osc 2 at a
            // random phase so two bright waves do not stack their edges note after note
            self.ph2[0] = xorshift(&mut rng) * 0.5 + 0.5;
        }
        self.rnd = xorshift(&mut rng);
        self.rng = rng;
        self.st_target = key as f32;
        self.st = from.unwrap_or(key as f32);
        self.set_glide(glide_samples);
        self.amp.gate();
        self.menv.gate();
    }

    /// After [`Self::start`]: take over from a voice that is still sounding (mono retrigger).
    /// The envelopes continue from its levels and this voice fades in over `samples` while the
    /// old one fades out, so the restart (oscillators from phase 0) has no jump.
    pub fn take_over(&mut self, amp: f32, menv: f32, samples: f32) {
        self.amp.v = amp;
        self.menv.v = menv;
        self.fin = 0.0;
        self.fin_step = 1.0 / samples.max(1.0);
    }

    /// Moves the pitch target to `key` (legato / mono / slide).
    pub fn retarget(&mut self, key: u8, glide_samples: f32) {
        self.key = key;
        self.st_target = key as f32;
        self.set_glide(glide_samples);
    }

    fn set_glide(&mut self, samples: f32) {
        if samples >= 1.0 && (self.st_target - self.st).abs() > 1e-4 {
            self.glide_left = samples as u32;
            self.glide_step = (self.st_target - self.st) / samples;
        } else {
            self.st = self.st_target;
            self.glide_left = 0;
        }
    }

    pub fn release(&mut self) {
        self.amp.release();
        self.menv.release();
    }

    pub fn released(&self) -> bool {
        self.amp.st == RELEASE || self.amp.st == IDLE
    }

    /// Renders and adds this voice into `l`/`r`. Clears `on` when the voice has finished.
    pub fn render(&mut self, c: &Ctx, l: &mut [f32], r: &mut [f32]) {
        let p = c.p;
        let b = c.bank;
        let inv_sr = 1.0 / c.sr;
        let dt_a4 = 440.0 * inv_sr;
        let lvl1 = p[ix::O1_LEVEL] * 0.01;
        let lvl2 = p[ix::O2_LEVEL] * 0.01;
        let sh1 = p[ix::O1_SHAPE] * 0.01;
        let sh2 = p[ix::O2_SHAPE] * 0.01;
        let c1 = p[ix::O1_COARSE];
        let c2 = p[ix::O2_COARSE] + p[ix::O2_FINE] * 0.01;
        let sub = p[ix::SUB_LEVEL] * 0.01;
        let noise = p[ix::NOISE_LEVEL] * 0.01;
        let ncol = p[ix::NOISE_COLOR] * 0.01;
        let cut_oct = p[ix::FLT_CUTOFF].log2();
        let res0 = p[ix::FLT_RES] * 0.01;
        let fdrive0 = p[ix::FLT_DRIVE] * 0.01;
        let ktrack = p[ix::FLT_KEY] * 0.01;
        let envamt = p[ix::FLT_ENV];
        let max_fc = 0.45 * c.sr;
        let velg = (0.2 + 0.8 * self.vel).powi(2);
        let n = self.n_uni;
        let bend = if self.live { c.bend } else { 0.0 };
        let nlp_a = 1.0 - (-2.0 * PI * 1200.0 * inv_sr).exp();
        let wamt1 = p[ix::O1_WAMT] * 0.01;
        let wamt2 = p[ix::O2_WAMT] * 0.01;
        let wmod1 = c.slots[..c.nslots].iter().any(|s| s.1 == dst::WARP1);
        let wmod2 = c.slots[..c.nslots].iter().any(|s| s.1 == dst::WARP2);
        let warp1 = if wamt1 > 0.0 || wmod1 { self.warp1 } else { warp::OFF };
        let warp2 = if wamt2 > 0.0 || wmod2 { self.warp2 } else { warp::OFF };
        // osc 2 runs when it is heard or when it is osc 1's FM modulator
        let do2 = lvl2 > 0.0 || c.slots[..c.nslots].iter().any(|s| s.1 == dst::LVL2 && s.2 > 0.0) || warp1 == warp::FM;
        // filter damping and drive curve: per sample only when the matrix moves them
        let res_mod = c.slots[..c.nslots].iter().any(|s| s.1 == dst::RES);
        let drive_mod = c.slots[..c.nslots].iter().any(|s| s.1 == dst::DRIVE);
        let damp = |res: f32, lp24: bool| if lp24 { 1.0 / (1.3066 * 15.3f32.powf(res)) } else { 2.0 * 40f32.powf(-res) };
        let lp24 = self.ftype == LP24;
        let k_chunk = damp(res0, lp24);
        let shaper = |fd: f32| {
            let g = 1.0 + 24.0 * fd * fd;
            (g, g.powf(-0.25), (fd * 20.0).min(1.0))
        };
        let drive_chunk = shaper(fdrive0);

        // unison: detune ratios and stereo gains for this chunk (detune modulation sampled once)
        let mut srcv = [0f32; src::N];
        let mut dmod = [0f32; dst::N];
        self.sources(c, 0, &mut srcv);
        Self::matrix(c, &srcv, &mut dmod);
        let det = (p[ix::UNI_DETUNE] * 0.01 + dmod[dst::DETUNE]).clamp(0.0, 1.0);
        let spread = MAX_SPREAD_ST * det * det;
        let width = p[ix::UNI_WIDTH] * 0.01;
        let norm = 1.0 / (n as f32).sqrt();
        let mut ratio = [1f32; MAX_UNI];
        let mut gl = [0f32; MAX_UNI];
        let mut gr = [0f32; MAX_UNI];
        let mut rmax = 1f32;
        for k in 0..n {
            ratio[k] = (UNI_OFF[self.uni_row][k] * spread / 12.0).exp2();
            rmax = rmax.max(ratio[k]);
            let th = (UNI_PAN[self.uni_row][k] * width + 1.0) * FRAC_PI_4;
            gl[k] = th.cos() * SQRT_2 * norm;
            gr[k] = th.sin() * SQRT_2 * norm;
        }
        let sine = b.table(SINE, 0, 0);

        // LFO rise: each LFO's depth fades in over its rise time from the note's start
        let rise = [p[ix::L1_RISE] * 0.001 * c.sr, p[ix::L2_RISE] * 0.001 * c.sr];
        for i in 0..l.len() {
            self.t = self.t.saturating_add(1);
            let me = self.menv.tick(&c.menv);
            let ae = self.amp.tick(&c.amp);
            self.sources(c, i, &mut srcv);
            srcv[src::MOD_ENV as usize] = me;
            for (j, r) in rise.iter().enumerate() {
                if *r > 1.0 && (self.t as f32) < *r {
                    let g = self.t as f32 / r;
                    srcv[src::LFO1 as usize + j] *= g;
                    if j == 0 {
                        srcv[src::LFO1_WHEEL as usize] *= g;
                    }
                }
            }
            let mut d = [0f32; dst::N];
            Self::matrix(c, &srcv, &mut d);
            if self.glide_left > 0 {
                self.st += self.glide_step;
                self.glide_left -= 1;
                if self.glide_left == 0 {
                    self.st = self.st_target;
                }
            }
            let note = self.st + bend + d[dst::PITCH];
            let dt_note = dt_a4 * ((note - 69.0) / 12.0).exp2();

            // oscillators: osc 2 first (it is osc 1's FM modulator, same sample); osc 2's own FM
            // reads osc 1 one sample late
            let dt1 = dt_note * ((c1 + d[dst::P1]) / 12.0).exp2();
            let dt2 = dt_note * ((c2 + d[dst::P2]) / 12.0).exp2();
            let (mut xl, mut xr) = (0.0, 0.0);
            if do2 {
                let wa = if wmod2 { (wamt2 + d[dst::WARP2]).clamp(0.0, 1.0) } else { wamt2 };
                let o2 = Osc::new(b, self.wave2, (sh2 + d[dst::SHAPE2]).clamp(0.0, 1.0), dt2 * rmax * warp_speed(warp2, wa, dt1 / dt2));
                let (mut a2l, mut a2r) = (0.0, 0.0);
                for k in 0..n {
                    let ph = self.ph2[k];
                    let (q, wg) = warp_phase(warp2, wa, ph, self.last1[k], dt2, sine);
                    let s = o2.sample(q) * wg;
                    self.last2[k] = s;
                    a2l += s * gl[k];
                    a2r += s * gr[k];
                    let mut ph = ph + dt2 * ratio[k];
                    if ph >= 1.0 {
                        ph -= 1.0;
                    }
                    self.ph2[k] = ph;
                }
                let g2 = (lvl2 + d[dst::LVL2]).clamp(0.0, 1.0);
                xl += a2l * g2;
                xr += a2r * g2;
            }
            let wa = if wmod1 { (wamt1 + d[dst::WARP1]).clamp(0.0, 1.0) } else { wamt1 };
            let o1 = Osc::new(b, self.wave1, (sh1 + d[dst::SHAPE1]).clamp(0.0, 1.0), dt1 * rmax * warp_speed(warp1, wa, dt2 / dt1));
            let (mut a1l, mut a1r) = (0.0, 0.0);
            for k in 0..n {
                let ph = self.ph1[k];
                let (q, wg) = warp_phase(warp1, wa, ph, self.last2[k], dt1, sine);
                let s = o1.sample(q) * wg;
                self.last1[k] = s;
                a1l += s * gl[k];
                a1r += s * gr[k];
                let mut ph = ph + dt1 * ratio[k];
                if ph >= 1.0 {
                    ph -= 1.0;
                }
                self.ph1[k] = ph;
            }
            let g1 = (lvl1 + d[dst::LVL1]).clamp(0.0, 1.0);
            xl += a1l * g1;
            xr += a1r * g1;
            if sub > 0.0 {
                let s = tables::read(sine, self.ph_sub) * sub;
                self.ph_sub += dt_note * self.sub_div;
                if self.ph_sub >= 1.0 {
                    self.ph_sub -= 1.0;
                }
                xl += s;
                xr += s;
            }
            let nl = (noise + d[dst::NOISE]).clamp(0.0, 1.0);
            if nl > 0.0 {
                let w = xorshift(&mut self.rng);
                self.noise_lp += (w - self.noise_lp) * nlp_a;
                let s = if ncol < 0.0 { w + (self.noise_lp - w) * -ncol } else { w - self.noise_lp * ncol } * 0.5 * nl;
                xl += s;
                xr += s;
            }

            // drive into the filter
            if self.drive_on {
                let (g, comp, w) = if drive_mod { shaper((fdrive0 + d[dst::DRIVE]).clamp(0.0, 1.0)) } else { drive_chunk };
                let f = |x: f32| x + (tanh(g * x) * comp - x) * w;
                xl = self.os[0].process(xl, &b.hb, f);
                xr = self.os[1].process(xr, &b.hb, f);
            }

            // filter: cutoff in octaves, coefficients every sample
            let oct = cut_oct + ktrack * (note - 60.0) / 12.0 + envamt * me + d[dst::CUTOFF];
            let fc = oct.exp2().clamp(16.0, max_fc);
            let g = (PI * fc * inv_sr).tan();
            let k = if res_mod { damp((res0 + d[dst::RES]).clamp(0.0, 1.0), lp24) } else { k_chunk };
            if self.ftype == LP24 {
                let ca = SvfCoef::new(g, 1.0 / 0.5412);
                let cb = SvfCoef::new(g, k);
                let [s0, s1] = &mut self.svf;
                xl = s1[0].tick(s0[0].tick(xl, &ca, LP12), &cb, LP12);
                xr = s1[1].tick(s0[1].tick(xr, &ca, LP12), &cb, LP12);
            } else {
                let cf = SvfCoef::new(g, k);
                xl = self.svf[0][0].tick(xl, &cf, self.ftype);
                xr = self.svf[0][1].tick(xr, &cf, self.ftype);
            }

            // amp, pan
            let mut a = ae * velg * (1.0 + d[dst::AMP]).max(0.0);
            if self.fin_step > 0.0 {
                self.fin += self.fin_step;
                if self.fin >= 1.0 {
                    self.fin = 1.0;
                    self.fin_step = 0.0;
                }
                a *= smooth(self.fin);
            }
            if self.fade_step > 0.0 {
                self.fade -= self.fade_step;
                if self.fade <= 0.0 {
                    self.on = false;
                    break;
                }
                a *= smooth(self.fade);
            }
            let pan = (d[dst::PAN] + self.npan).clamp(-1.0, 1.0);
            let (pl, pr) = (if pan > 0.0 { 1.0 - pan } else { 1.0 }, if pan < 0.0 { 1.0 + pan } else { 1.0 });
            l[i] += xl * a * pl;
            r[i] += xr * a * pr;
        }
        for s in self.svf.iter_mut().flatten() {
            s.flush();
        }
        if self.noise_lp.abs() < 1e-20 {
            self.noise_lp = 0.0;
        }
        let silent_sustain = self.amp.st == SUSTAIN && c.amp.sus < 1e-4 && self.amp.v < 1e-4;
        if self.amp.st == IDLE || silent_sustain {
            self.on = false;
        }
    }

    #[inline(always)]
    fn sources(&self, c: &Ctx, i: usize, s: &mut [f32; src::N]) {
        s[src::MOD_ENV as usize] = self.menv.v;
        s[src::LFO1 as usize] = c.lfo1[i];
        s[src::LFO2 as usize] = c.lfo2[i];
        s[src::VEL as usize] = self.vel;
        s[src::WHEEL as usize] = c.wheel;
        s[src::RANDOM as usize] = self.rnd;
        s[src::KEY as usize] = (self.key as f32 - 60.0) / 60.0;
        s[src::LFO1_WHEEL as usize] = c.lfo1[i] * c.wheel;
        s[src::MACRO1 as usize..src::MACRO1 as usize + 4].copy_from_slice(&c.macros);
    }

    #[inline(always)]
    fn matrix(c: &Ctx, s: &[f32; src::N], d: &mut [f32; dst::N]) {
        for &(sr, ds, amt) in &c.slots[..c.nslots] {
            d[ds] += s[sr as usize] * amt;
        }
    }
}

/// Active matrix slots from (source, destination, amount %), amounts in destination units.
pub fn slots(m: &[(u8, usize, f32); SLOTS]) -> ([(u8, usize, f32); SLOTS], usize) {
    let mut out = [(0u8, 0usize, 0f32); SLOTS];
    let mut n = 0;
    for &(srcv, ds, amt) in m {
        let ds = ds.min(dst::N - 1);
        let amt = amt * 0.01 * DEST_SCALE[ds];
        if srcv > 0 && (srcv as usize) < src::N && amt != 0.0 {
            out[n] = (srcv, ds, amt);
            n += 1;
        }
    }
    (out, n)
}

/// One oscillator alone (the exact table/mip path a voice uses), for measurements: `n`
/// samples of `wave` with `shape` 0..1 at `freq` Hz.
pub fn render_osc(wave: u8, shape: f32, freq: f32, sr: f32, n: usize) -> Vec<f32> {
    render_warped(wave, shape, warp::OFF, 0.0, freq, sr, n)
}

/// [`render_osc`] through a warp (FM: by a sine at the oscillator's own pitch).
pub fn render_warped(wave: u8, shape: f32, mode: u8, amt: f32, freq: f32, sr: f32, n: usize) -> Vec<f32> {
    let b = tables::bank();
    let dt = freq / sr;
    let o = Osc::new(b, wave, shape, dt * warp_speed(mode, amt, 1.0));
    let mut ph = 0f32;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (q, g) = warp_phase(mode, amt, ph, (std::f32::consts::TAU * ph).sin(), dt, b.table(SINE, 0, 0));
        out.push(o.sample(q) * g);
        ph += dt;
        if ph >= 1.0 {
            ph -= 1.0;
        }
    }
    out
}
