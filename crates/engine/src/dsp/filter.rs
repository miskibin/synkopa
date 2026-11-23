//! Filter: a resonant filter whose cutoff moves — by hand (automation), with an LFO, or with the
//! level of the sound (envelope follower, optionally from a sidechain).
//!
//! - **Types**: LP / HP / BP / Notch, 12 or 24 dB/oct, from a zero-delay-feedback state-variable
//!   filter (trapezoidal integration, A. Simper / V. Zavalishin): it stays stable and click-free
//!   while the cutoff moves every sample. At 24 dB the first stage is Butterworth and only the
//!   second resonates, so Resonance feels the same at both slopes. **Ladder**: a 4-pole 24 dB
//!   low-pass with a saturating feedback path (tanh, linearised around the previous sample, the
//!   "cheap" nonlinear ZDF ladder): it growls when pushed and self-oscillates at the top of the
//!   Resonance range without running away; the input is raised with the feedback (half of it) so
//!   the low end does not vanish as resonance rises. **DJ**: one knob, left = low-pass, right =
//!   high-pass, the middle (±1 %) passes the sound untouched, fading in over the next 4 %.
//! - **Cutoff** moves in octaves: the LFO (±5 oct) and the envelope (±6 oct) add octaves, so a
//!   sweep sounds even across the range. The envelope reads the level in dB, −40 dBFS → 0 and
//!   0 dBFS → 1, so a quiet guitar and a loud one open the filter alike.
//! - **Drive** (0–24 dB) saturates the input before the filter, which then shapes the harmonics;
//!   **Soft clip** limits the output to 0 dBFS smoothly (linear up to −3 dB), catching resonant
//!   peaks.
//!
//! Every continuous parameter is smoothed per sample (~20 ms, cutoff in octaves), the LFO's
//! jumps (saw, square, S&H) are rounded over 1.5 ms, nothing allocates, and there is no latency.

use std::f32::consts::{PI, TAU};

use super::flush;
use super::modulation::{phaser_lfo, Lfo, Smooth, DIVS};
use crate::processor::{ParamBlock, ProcessCtx, Processor, METERS};

/// Parameter order (= catalog.rs FILTER).
pub mod idx {
    pub const TYPE: usize = 0;
    pub const CUTOFF: usize = 1;
    pub const RES: usize = 2;
    pub const SLOPE: usize = 3;
    pub const DRIVE: usize = 4;
    pub const DJ: usize = 5;
    pub const SYNC: usize = 6;
    pub const RATE: usize = 7;
    pub const DIV: usize = 8;
    pub const SHAPE: usize = 9;
    pub const LFO: usize = 10;
    pub const STEREO: usize = 11;
    pub const ENV: usize = 12;
    pub const EATT: usize = 13;
    pub const EREL: usize = 14;
    pub const CLIP: usize = 15;
}

pub const TYPES: [&str; 6] = ["LP", "Ladder", "HP", "BP", "Notch", "DJ"];
pub const SLOPES: [&str; 2] = ["12 dB", "24 dB"];
pub const SHAPES: [&str; 6] = ["Sine", "Triangle", "Saw up", "Saw down", "Square", "S&H"];
pub const LP: usize = 0;
pub const LADDER: usize = 1;
pub const HP: usize = 2;
pub const BP: usize = 3;
pub const NOTCH: usize = 4;
pub const DJ: usize = 5;
/// Ladder feedback at 100 % resonance (4 = the edge of self-oscillation).
pub const LADDER_K_MAX: f32 = 4.15;
/// Envelope level range: −40 dBFS reads 0, 0 dBFS reads 1.
pub const ENV_FLOOR_DB: f32 = -40.0;
/// DJ: the dead zone around the centre, and the fade-in after it.
pub const DJ_DEAD: f32 = 0.01;
pub const DJ_FADE: f32 = 0.04;
/// Soft clip: linear up to this level, then towards 1.0.
const CLIP_KNEE: f32 = 0.7;

/// SVF resonance: Q from 0.5 (no peak) at 0 % to 64 at 100 %, even steps per octave of Q.
#[inline]
pub fn svf_q(res: f32) -> f32 {
    0.5 * (7.0 * res.clamp(0.0, 1.0)).exp2()
}

/// DJ knob (−1..1) → the base cutoff (Hz), the filter it uses and how much of it is heard.
#[inline]
pub fn dj_map(pos: f32) -> (f32, usize, f32) {
    let m = pos.abs();
    let t = ((m - DJ_DEAD) / (1.0 - DJ_DEAD)).clamp(0.0, 1.0);
    let w = ((m - DJ_DEAD) / DJ_FADE).clamp(0.0, 1.0);
    let w = w * w * (3.0 - 2.0 * w);
    // ten octaves: 20 kHz down to 20 Hz (low-pass), 20 Hz up to 20 kHz (high-pass)
    if pos < 0.0 {
        (20_000.0 * (-9.966 * t).exp2(), LP, w)
    } else {
        (20.0 * (9.966 * t).exp2(), HP, w)
    }
}

/// Smooth limiter towards ±1: unity below `CLIP_KNEE`, continuous slope at the knee.
#[inline(always)]
fn clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= CLIP_KNEE {
        x
    } else {
        let r = 1.0 - CLIP_KNEE;
        (CLIP_KNEE + r * ((a - CLIP_KNEE) / r).tanh()).copysign(x)
    }
}

/// Trapezoidal state-variable filter (Simper): `g` = tan(π f / sr), `k` = 1/Q.
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// Low-pass, band-pass and high-pass outputs.
    #[inline(always)]
    fn tick(&mut self, v0: f32, g: f32, k: f32) -> (f32, f32, f32) {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = v0 - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        (v2, v1, v0 - k * v1 - v2)
    }
    /// One mode's output. Band-pass is scaled by √k: its peak grows with Q, but gently.
    #[inline(always)]
    fn run(&mut self, x: f32, g: f32, k: f32, mode: usize) -> f32 {
        let (lp, bp, hp) = self.tick(x, g, k);
        match mode {
            HP => hp,
            BP => bp * k.sqrt(),
            NOTCH => x - k * bp,
            _ => lp,
        }
    }
}

/// Four one-pole stages with saturating feedback (the transistor-ladder low-pass).
#[derive(Clone, Copy, Default)]
struct Ladder {
    s: [f32; 4],
    /// Last output, for the feedback's operating point.
    y: f32,
}

impl Ladder {
    #[inline(always)]
    fn tick(&mut self, x: f32, g: f32, k: f32) -> f32 {
        let g1 = g / (1.0 + g);
        let inv = 1.0 / (1.0 + g);
        let (g2, g3) = (g1 * g1, g1 * g1 * g1);
        let g4 = g2 * g2;
        // the stages' contribution with zero input
        let s = (g3 * self.s[0] + g2 * self.s[1] + g1 * self.s[2] + self.s[3]) * inv;
        // tanh(y) ≈ t·y around the previous output: solve the delay-free loop with that slope
        let t = if self.y.abs() < 1e-4 { 1.0 } else { self.y.tanh() / self.y };
        let x = x * (1.0 + 0.5 * k);
        let y_est = (g4 * x + s) / (1.0 + k * t * g4);
        let mut u = x - k * y_est.tanh();
        for st in &mut self.s {
            let v = g1 * (u - *st);
            let y = v + *st;
            *st = flush(y + v);
            u = y;
        }
        self.y = u;
        u
    }
}

/// Deterministic noise for S&H: a value in −1..1 for each LFO cycle.
#[inline]
fn hash01(n: u64) -> f32 {
    let mut z = n.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
}

/// LFO value (−1..1) for `shape` at phase `p` in cycle `cycle`.
#[inline]
pub fn lfo_value(shape: usize, p: f32, cycle: u64) -> f32 {
    let p = p - p.floor();
    match shape {
        1 => phaser_lfo(p + 0.25),
        2 => 2.0 * p - 1.0,
        3 => 1.0 - 2.0 * p,
        4 => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        5 => hash01(cycle),
        _ => (TAU * p).sin(),
    }
}

pub struct Filter {
    sr: f32,
    k: f32,
    k_slew: f32,
    ftype: usize,
    slope24: bool,
    sync: bool,
    rate: f32,
    div: usize,
    shape: usize,
    clip: bool,
    a_att: f32,
    a_rel: f32,
    /// Cutoff in octaves above 1 Hz (log2 Hz), smoothed.
    oct: Smooth,
    res: Smooth,
    drive: Smooth,
    dj: Smooth,
    lfo_amt: Smooth,
    stereo: Smooth,
    env_amt: Smooth,
    primed: bool,
    svf: [[Svf; 2]; 2],
    ladder: [Ladder; 2],
    lfo: Lfo,
    /// Completed LFO cycles (S&H picks a value per cycle).
    cycle: u64,
    lfo_y: [f32; 2],
    env: f32,
    // meters
    fc: [f32; 2],
    env_m: f32,
    dj_w: f32,
    clipped: bool,
    keyed: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            sr: 48_000.0,
            k: 0.0,
            k_slew: 0.0,
            ftype: LADDER,
            slope24: true,
            sync: true,
            rate: 1.0,
            div: 2,
            shape: 0,
            clip: true,
            a_att: 0.0,
            a_rel: 0.0,
            oct: Smooth { v: 1600f32.log2(), target: 1600f32.log2() },
            res: Smooth { v: 0.25, target: 0.25 },
            drive: Smooth::default(),
            dj: Smooth::default(),
            lfo_amt: Smooth::default(),
            stereo: Smooth::default(),
            env_amt: Smooth::default(),
            primed: false,
            svf: [[Svf::default(); 2]; 2],
            ladder: [Ladder::default(); 2],
            lfo: Lfo::default(),
            cycle: 0,
            lfo_y: [0.0; 2],
            env: 0.0,
            fc: [1600.0; 2],
            env_m: 0.0,
            dj_w: 1.0,
            clipped: false,
            keyed: false,
        }
    }
}

impl Filter {
    fn smoothers(&mut self) -> [&mut Smooth; 7] {
        [&mut self.oct, &mut self.res, &mut self.drive, &mut self.dj, &mut self.lfo_amt, &mut self.stereo, &mut self.env_amt]
    }

    fn run(&mut self, l: &mut [f32], r: &mut [f32], key: Option<&[f32]>, ctx: &ProcessCtx) {
        let n = l.len().min(r.len());
        let (sr, k) = (self.sr, self.k);
        let inc = self.lfo.block(self.sync, self.rate, self.div, ctx);
        if self.sync && ctx.playing {
            // S&H follows the song: the cycle index is the song position in LFO periods
            let beats = super::modulation::DIV_BEATS[self.div.min(DIVS.len() - 1)];
            self.cycle = (ctx.beat / beats).floor().max(0.0) as u64;
        }
        let fmax = (0.45 * sr).min(21_000.0);
        let (ft, slope24, shape, use_clip) = (self.ftype, self.slope24, self.shape, self.clip);
        let mut peak = 0f32;
        for s in 0..n {
            let oct = self.oct.tick(k);
            let res = self.res.tick(k);
            let drive = self.drive.tick(k);
            let dj = self.dj.tick(k);
            let lfo_amt = self.lfo_amt.tick(k);
            let stereo = self.stereo.tick(k);
            let env_amt = self.env_amt.tick(k);
            let (xl, xr) = (l[s], r[s]);

            // envelope follower on the level in dB
            let det = match key {
                Some(kk) => kk[s].abs(),
                None => xl.abs().max(xr.abs()),
            };
            let a = if det > self.env { self.a_att } else { self.a_rel };
            self.env = flush(det + (self.env - det) * a);
            let env_m = ((20.0 * self.env.max(1e-5).log10() - ENV_FLOOR_DB) / -ENV_FLOOR_DB).clamp(0.0, 1.0);

            // DJ: the knob picks the base cutoff and the side; elsewhere the Cutoff knob
            let (base_oct, mode, wet) = if ft == DJ {
                let (f, m, w) = dj_map(dj);
                (f.log2(), m, w)
            } else {
                (oct, ft, 1.0)
            };
            let ph = self.lfo.phase as f32;
            let gain = (drive * (1.0 / 20.0) * std::f32::consts::LOG2_10).exp2();
            let drive_w = (drive / 1.0).clamp(0.0, 1.0);
            let trim = 1.0 / (1.0 + 0.35 * (gain - 1.0));
            for c in 0..2 {
                let p = ph + if c == 1 { stereo } else { 0.0 };
                let raw = lfo_value(shape, p, self.cycle + p.floor().max(0.0) as u64);
                self.lfo_y[c] += (raw - self.lfo_y[c]) * self.k_slew;
                let f = (base_oct + lfo_amt * self.lfo_y[c] + env_amt * env_m).exp2().clamp(16.0, fmax);
                if s + 1 == n {
                    self.fc[c] = f;
                }
                let g = (PI * f / sr).tan();
                let x = if c == 0 { xl } else { xr };
                // drive: a smooth blend in over the first dB, so 0 dB is exactly clean
                let xd = if drive_w > 0.0 { x + drive_w * ((x * gain).tanh() * trim - x) } else { x };
                let y = if mode == LADDER {
                    self.ladder[c].tick(xd, g, LADDER_K_MAX * res)
                } else {
                    let kq = 1.0 / svf_q(res);
                    if slope24 {
                        let st1 = self.svf[c][0].run(xd, g, std::f32::consts::SQRT_2, mode);
                        self.svf[c][1].run(st1, g, kq, mode)
                    } else {
                        self.svf[c][0].run(xd, g, kq, mode)
                    }
                };
                let y = x + wet * (y - x);
                let y = if use_clip { clip(y) } else { y };
                peak = peak.max(y.abs());
                if c == 0 {
                    l[s] = y;
                } else {
                    r[s] = y;
                }
            }
            self.env_m = env_m;
            self.dj_w = wet;
            let was = self.lfo.phase;
            self.lfo.advance(inc);
            if self.lfo.phase < was {
                self.cycle = self.cycle.wrapping_add(1);
            }
        }
        self.clipped = use_clip && peak > CLIP_KNEE;
        self.keyed = key.is_some();
    }
}

impl Processor for Filter {
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sr = sample_rate.max(8000.0);
        self.k = 1.0 - (-1.0 / (0.02 * self.sr)).exp();
        self.k_slew = 1.0 - (-1.0 / (0.0015 * self.sr)).exp();
        self.reset();
    }

    fn set_params(&mut self, p: &ParamBlock) {
        let v = &p.values;
        let ftype = (v[idx::TYPE].round().max(0.0) as usize).min(TYPES.len() - 1);
        let slope24 = v[idx::SLOPE] >= 0.5;
        if ftype != self.ftype || slope24 != self.slope24 {
            // the state of one topology means nothing to another: start the new one empty
            self.svf = [[Svf::default(); 2]; 2];
            self.ladder = [Ladder::default(); 2];
            self.ftype = ftype;
            self.slope24 = slope24;
        }
        self.oct.target = v[idx::CUTOFF].clamp(16.0, 22_000.0).log2();
        self.res.target = (v[idx::RES] / 100.0).clamp(0.0, 1.0);
        self.drive.target = v[idx::DRIVE].clamp(0.0, 24.0);
        self.dj.target = (v[idx::DJ] / 100.0).clamp(-1.0, 1.0);
        self.sync = v[idx::SYNC] >= 0.5;
        self.rate = v[idx::RATE].clamp(0.01, 40.0);
        self.div = (v[idx::DIV].round().max(0.0) as usize).min(DIVS.len() - 1);
        self.shape = (v[idx::SHAPE].round().max(0.0) as usize).min(SHAPES.len() - 1);
        self.lfo_amt.target = v[idx::LFO].clamp(-5.0, 5.0);
        self.stereo.target = (v[idx::STEREO] / 360.0).clamp(0.0, 0.5);
        self.env_amt.target = v[idx::ENV].clamp(-6.0, 6.0);
        self.a_att = (-1.0 / (v[idx::EATT].clamp(0.1, 1000.0) / 1000.0 * self.sr)).exp();
        self.a_rel = (-1.0 / (v[idx::EREL].clamp(1.0, 5000.0) / 1000.0 * self.sr)).exp();
        self.clip = v[idx::CLIP] >= 0.5;
        if !self.primed {
            self.primed = true;
            for s in self.smoothers() {
                s.snap();
            }
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        self.run(l, r, None, ctx);
    }

    /// Sidechain: the envelope follower hears `key` (another insert's sound), so a kick can
    /// pump the cutoff of a pad.
    fn process_keyed(&mut self, l: &mut [f32], r: &mut [f32], key: &[f32], ctx: &ProcessCtx) {
        self.run(l, r, Some(key), ctx);
    }

    fn reset(&mut self) {
        self.svf = [[Svf::default(); 2]; 2];
        self.ladder = [Ladder::default(); 2];
        self.env = 0.0;
        self.lfo_y = [0.0; 2];
        for s in self.smoothers() {
            s.snap();
        }
    }

    fn tail_samples(&self) -> Option<usize> {
        let res = self.res.target.max(self.res.v);
        if self.ftype == LADDER && LADDER_K_MAX * res >= 4.0 {
            return None; // self-oscillating: it may sing on its own
        }
        // a resonance at f with quality Q falls 140 dB in 16.1·Q/(π f) s; the lowest cutoff
        // the modulation can reach is the slowest to ring out
        let reach = self.lfo_amt.target.abs().max(self.lfo_amt.v.abs()) + self.env_amt.target.abs().max(self.env_amt.v.abs());
        let base = if self.ftype == DJ { 20.0 } else { self.oct.target.min(self.oct.v).exp2() };
        let f = (base * (-reach).exp2()).max(16.0);
        let q = if self.ftype == LADDER { 0.7 + 8.0 * res } else { svf_q(res) + 0.7 };
        let secs = 16.1 * q / (PI * f) * if self.slope24 || self.ftype == LADDER { 2.0 } else { 1.0 };
        Some(((secs + 0.05) * self.sr) as usize)
    }

    /// `[0]`/`[1]` cutoff left/right now (Hz, with the LFO and envelope), `[2]` envelope 0..1,
    /// `[3]` LFO phase 0..1, `[4]` DJ: how much of the filter is heard (0 at the centre), `[5]` 1
    /// when the soft clip is working, `[6]` 1 when the envelope hears a sidechain.
    fn meters(&self, out: &mut [f32; METERS]) {
        out[0] = self.fc[0];
        out[1] = self.fc[1];
        out[2] = self.env_m;
        out[3] = self.lfo.phase as f32;
        out[4] = self.dj_w;
        out[5] = if self.clipped { 1.0 } else { 0.0 };
        out[6] = if self.keyed { 1.0 } else { 0.0 };
    }
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
