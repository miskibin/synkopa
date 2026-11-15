//! Chorus and Phaser: the two modulation effects of the mixer.
//!
//! **Chorus** mixes the input with copies of itself read from a delay line whose length an LFO
//! sweeps. *Chorus* uses two copies per side, *Ensemble* three spread evenly around the LFO cycle
//! with a fast, shallow second LFO on top (the shimmer of string ensembles), *Vibrato* one copy
//! and no dry signal, so only the pitch wobbles. The right side reads the LFO a quarter cycle
//! (Ensemble: a sixth) later than the left, which turns a mono source into a wide stereo one
//! that still sums to mono without cancelling. Reads use 4-point Hermite interpolation, so slow
//! sweeps stay clean; a high-pass before the line keeps the bass out of the modulation ("safe
//! bass") and a low-pass on the copies darkens them like analogue bucket-brigade chips.
//!
//! **Phaser** runs the input through 2–12 first-order all-pass filters whose break frequency an
//! LFO sweeps exponentially (each octave takes the same time) around `center`. Mixed 50/50 with
//! the dry signal, every two stages cut one notch into the spectrum; feedback from the last
//! stage to the first sharpens the notches into resonant peaks (negative feedback moves them).
//!
//! Both: every continuous parameter is smoothed per sample (~20 ms), feedback goes through a
//! soft clipper so it stays bounded at the extremes, the LFO can follow the song position
//! (`sync`), and nothing allocates after `prepare`.

use std::f32::consts::{PI, TAU};

use super::{flush, Biquad, FilterType};
use crate::processor::{ParamBlock, ProcessCtx, Processor, METERS};

/// LFO periods offered when `sync` is on, in beats.
pub const DIVS: [&str; 8] = ["1/16", "1/8", "1/4", "1/2", "1 takt", "2 takty", "4 takty", "8 taktów"];
const DIV_BEATS: [f64; 8] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0];
pub const SYNCS: [&str; 2] = ["Hz", "Tempo"];
pub const CHORUS_MODES: [&str; 3] = ["Chorus", "Ensemble", "Vibrato"];
pub const STAGES: [&str; 5] = ["2", "4", "6", "8", "12"];
const STAGE_N: [usize; 5] = [2, 4, 6, 8, 12];
const MAX_STAGES: usize = 12;

/// Parameter order of the chorus (= catalog.rs CHORUS).
pub mod chorus_idx {
    pub const MODE: usize = 0;
    pub const SYNC: usize = 1;
    pub const RATE: usize = 2;
    pub const DIV: usize = 3;
    pub const DEPTH: usize = 4;
    pub const DELAY: usize = 5;
    pub const FB: usize = 6;
    pub const WIDTH: usize = 7;
    pub const HP: usize = 8;
    pub const TONE: usize = 9;
    pub const MIX: usize = 10;
}

/// Parameter order of the phaser (= catalog.rs PHASER).
pub mod phaser_idx {
    pub const STAGES: usize = 0;
    pub const SYNC: usize = 1;
    pub const RATE: usize = 2;
    pub const DIV: usize = 3;
    pub const CENTER: usize = 4;
    pub const DEPTH: usize = 5;
    pub const FB: usize = 6;
    pub const STEREO: usize = 7;
    pub const MIX: usize = 8;
}

/// Longest base delay (ms) and the most the LFOs add on top of it.
const MAX_DELAY_MS: f32 = 40.0;
const MAX_SWING_MS: f32 = 12.0;
/// Range the phaser's sweep covers at 100 % depth, in octaves each way.
pub const PHASER_OCTAVES: f32 = 3.0;

/// One-pole smoother towards a target (about 20 ms to settle).
#[derive(Clone, Copy, Default)]
struct Smooth {
    v: f32,
    target: f32,
}

impl Smooth {
    #[inline(always)]
    fn tick(&mut self, k: f32) -> f32 {
        self.v += (self.target - self.v) * k;
        self.v
    }
    fn snap(&mut self) {
        self.v = self.target;
    }
}

/// Soft clipper for feedback paths: unity for small signals, saturating smoothly towards ±1.5.
#[inline(always)]
fn soft(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// The LFO's phase (0..1): free-running at `rate` Hz, or locked to the song position with a
/// period of `div` beats while the transport plays (free-running at that tempo when stopped).
#[derive(Clone, Copy, Default)]
struct Lfo {
    phase: f64,
}

impl Lfo {
    /// Phase at the start of the block and the per-sample increment.
    fn block(&mut self, sync: bool, rate: f32, div: usize, ctx: &ProcessCtx) -> f64 {
        let sr = f64::from(ctx.sample_rate.max(1.0));
        if sync {
            let beats = DIV_BEATS[div.min(DIV_BEATS.len() - 1)];
            if ctx.playing {
                self.phase = (ctx.beat / beats).rem_euclid(1.0);
            }
            ctx.bpm.max(1.0) / 60.0 / beats / sr
        } else {
            f64::from(rate.max(0.0)) / sr
        }
    }
    #[inline(always)]
    fn advance(&mut self, inc: f64) {
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
    }
}

/// Repeats until a feedback loop with gain `fb` falls 140 dB.
fn repeats(fb: f32) -> f32 {
    let g = fb.abs().clamp(1e-3, 0.999);
    (140.0 / (-20.0 * g.log10())).max(1.0)
}

// ---------------------------------------------------------------- delay line

/// Power-of-two ring with 4-point Hermite reads.
#[derive(Default)]
struct Line {
    buf: Vec<f32>,
    mask: usize,
    w: usize,
}

impl Line {
    fn alloc(&mut self, min_len: usize) {
        let n = (min_len + 4).next_power_of_two();
        self.buf = vec![0.0; n];
        self.mask = n - 1;
        self.w = 0;
    }
    #[inline(always)]
    fn push(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w = (self.w + 1) & self.mask;
    }
    /// `d` samples behind the newest sample (`d` ≥ 2).
    #[inline(always)]
    fn read(&self, d: f32) -> f32 {
        let pos = self.w as f32 - 1.0 - d;
        let i = pos.floor();
        let t = pos - i;
        let i = i as isize as usize;
        let m = self.mask;
        let y0 = self.buf[i.wrapping_sub(1) & m];
        let y1 = self.buf[i & m];
        let y2 = self.buf[i.wrapping_add(1) & m];
        let y3 = self.buf[i.wrapping_add(2) & m];
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }
    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

// ---------------------------------------------------------------- chorus

pub struct Chorus {
    sr: f32,
    k: f32,
    lines: [Line; 2],
    hp: [Biquad; 2],
    lp: [Biquad; 2],
    fb_state: [f32; 2],
    lfo: Lfo,
    shimmer: f64,
    mode: usize,
    sync: bool,
    rate: f32,
    div: usize,
    hp_hz: f32,
    tone_hz: f32,
    depth: Smooth,
    delay: Smooth,
    fb: Smooth,
    width: Smooth,
    mix: Smooth,
    primed: bool,
    /// Last read positions in ms per voice, left then right (meters).
    taps: [f32; 6],
}

impl Default for Chorus {
    fn default() -> Self {
        Self {
            sr: 48_000.0,
            k: 0.0,
            lines: Default::default(),
            hp: [Biquad::default(); 2],
            lp: [Biquad::default(); 2],
            fb_state: [0.0; 2],
            lfo: Lfo::default(),
            shimmer: 0.0,
            mode: 0,
            sync: false,
            rate: 0.6,
            div: 4,
            hp_hz: 20.0,
            tone_hz: 20_000.0,
            depth: Smooth { v: 0.4, target: 0.4 },
            delay: Smooth { v: 12.0, target: 12.0 },
            fb: Smooth::default(),
            width: Smooth { v: 1.0, target: 1.0 },
            mix: Smooth { v: 0.5, target: 0.5 },
            primed: false,
            taps: [0.0; 6],
        }
    }
}

impl Chorus {
    fn voices(&self) -> usize {
        match self.mode {
            1 => 3,
            2 => 1,
            _ => 2,
        }
    }

    fn update_filters(&mut self) {
        let nyq = 0.45 * self.sr;
        for h in &mut self.hp {
            if self.hp_hz <= 20.5 {
                h.bypass();
            } else {
                h.set(FilterType::Highpass, self.sr, self.hp_hz.min(nyq), 0.707, 0.0);
            }
        }
        for l in &mut self.lp {
            if self.tone_hz >= 19_500.0 || self.tone_hz >= nyq {
                l.bypass();
            } else {
                l.set(FilterType::Lowpass, self.sr, self.tone_hz, 0.707, 0.0);
            }
        }
    }
}

impl Processor for Chorus {
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sr = sample_rate.max(8000.0);
        self.k = 1.0 - (-1.0 / (0.02 * self.sr)).exp();
        let n = ((MAX_DELAY_MS + MAX_SWING_MS + 2.0) / 1000.0 * self.sr).ceil() as usize + 8;
        for l in &mut self.lines {
            l.alloc(n);
        }
        self.update_filters();
        self.reset();
    }

    fn set_params(&mut self, p: &ParamBlock) {
        use chorus_idx as i;
        let v = &p.values;
        self.mode = (v[i::MODE].round().max(0.0) as usize).min(2);
        self.sync = v[i::SYNC] >= 0.5;
        self.rate = v[i::RATE].clamp(0.01, 20.0);
        self.div = (v[i::DIV].round().max(0.0) as usize).min(DIVS.len() - 1);
        self.depth.target = (v[i::DEPTH] / 100.0).clamp(0.0, 1.0);
        self.delay.target = v[i::DELAY].clamp(1.0, MAX_DELAY_MS);
        self.fb.target = (v[i::FB] / 100.0).clamp(-0.95, 0.95);
        self.width.target = (v[i::WIDTH] / 100.0).clamp(0.0, 2.0);
        self.mix.target = (v[i::MIX] / 100.0).clamp(0.0, 1.0);
        let (hp, tone) = (v[i::HP].clamp(20.0, 2000.0), v[i::TONE].clamp(500.0, 20_000.0));
        if hp != self.hp_hz || tone != self.tone_hz {
            (self.hp_hz, self.tone_hz) = (hp, tone);
            self.update_filters();
        }
        if !self.primed {
            self.primed = true;
            for s in [&mut self.depth, &mut self.delay, &mut self.fb, &mut self.width, &mut self.mix] {
                s.snap();
            }
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        let n = l.len().min(r.len());
        if self.lines[0].buf.is_empty() {
            return;
        }
        let sr = self.sr;
        let ms = sr / 1000.0;
        let k = self.k;
        let inc = self.lfo.block(self.sync, self.rate, self.div, ctx);
        let rate_hz = (inc * f64::from(sr)) as f32;
        // a fast, shallow second LFO for Ensemble's shimmer
        let shimmer_inc = (rate_hz * 6.7).clamp(3.0, 9.0) as f64 / f64::from(sr);
        let voices = self.voices();
        let (vib, ens) = (self.mode == 2, self.mode == 1);
        // the right side reads the cycle later: a quarter (two voices), a sixth (three)
        let r_off = if vib {
            0.0
        } else if ens {
            1.0 / 6.0
        } else {
            0.25
        };
        let max_d = (self.lines[0].buf.len() - 6) as f32;
        for s in 0..n {
            let depth = self.depth.tick(k);
            let base_ms = self.delay.tick(k);
            let fb = self.fb.tick(k);
            let width = self.width.tick(k);
            let mix = self.mix.tick(k);
            // how far the LFO swings the read head, in ms
            let swing = if vib {
                // ±6 % pitch at full depth: Δpitch ≈ 2π·f·A
                (0.06 * depth / (TAU * rate_hz.max(0.05))).min(MAX_SWING_MS / 1000.0) * 1000.0
            } else {
                (depth * 0.25 * base_ms).min(6.0)
            };
            let centre = if vib { swing + 1.0 } else { base_ms };
            let ph = self.lfo.phase as f32;
            let sh = (TAU * self.shimmer as f32).sin() * 0.12 * swing;
            let (xl, xr) = (l[s], r[s]);
            let mut wet = [0.0f32; 2];
            // what the high-pass keeps out of the line (the lows), for Vibrato's dry bass
            let mut lows = [0.0f32; 2];
            for (c, x) in [xl, xr].into_iter().enumerate() {
                let hp_in = self.hp[c].tick(x);
                lows[c] = x - hp_in;
                let fed = hp_in + fb * soft(self.fb_state[c]);
                self.lines[c].push(flush(fed));
                let off = if c == 1 { r_off } else { 0.0 };
                let mut sum = 0.0;
                for v in 0..voices {
                    let vp = ph + off + v as f32 / voices as f32;
                    let lfo = (TAU * vp).sin();
                    // Chorus spreads its two voices' centres apart so they never line up
                    let c_ms = if voices == 2 && v == 1 { centre * 1.37 } else { centre };
                    let d_ms = c_ms + swing * lfo + if ens { sh } else { 0.0 };
                    let d = (d_ms * ms).clamp(2.0, max_d);
                    if s + 1 == n {
                        self.taps[c * 3 + v] = d / ms;
                    }
                    sum += self.lines[c].read(d);
                }
                let y = self.lp[c].tick(sum / voices as f32);
                self.fb_state[c] = flush(y);
                wet[c] = y;
            }
            // stereo width of the copies only (mid/side), so the dry image is untouched
            let (m, sd) = (0.5 * (wet[0] + wet[1]), 0.5 * (wet[0] - wet[1]) * width);
            let (wl, wr) = (m + sd, m - sd);
            if vib {
                // all wet, except the bass under "safe bass", which passes unmodulated
                l[s] = xl * (1.0 - mix) + (wl + lows[0]) * mix;
                r[s] = xr * (1.0 - mix) + (wr + lows[1]) * mix;
            } else {
                l[s] = xl * (1.0 - mix) + wl * mix;
                r[s] = xr * (1.0 - mix) + wr * mix;
            }
            self.lfo.advance(inc);
            self.shimmer = (self.shimmer + shimmer_inc).fract();
        }
    }

    fn reset(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
        for f in self.hp.iter_mut().chain(self.lp.iter_mut()) {
            f.reset();
        }
        self.fb_state = [0.0; 2];
        for s in [&mut self.depth, &mut self.delay, &mut self.fb, &mut self.width, &mut self.mix] {
            s.snap();
        }
    }

    fn tail_samples(&self) -> Option<usize> {
        let longest = (MAX_DELAY_MS * 1.37 + MAX_SWING_MS) / 1000.0 * self.sr;
        Some((longest * repeats(self.fb.target.max(self.fb.v)) + 0.05 * self.sr) as usize)
    }

    /// `[0]` LFO phase 0..1, `[1..4]` left voices' delay (ms), `[4..7]` right voices', `[7]` voices,
    /// `[8]` the LFO rate in Hz.
    fn meters(&self, out: &mut [f32; METERS]) {
        out[0] = self.lfo.phase as f32;
        out[1..7].copy_from_slice(&self.taps);
        out[7] = self.voices() as f32;
        out[8] = self.rate;
    }
}

// ---------------------------------------------------------------- phaser

pub struct Phaser {
    sr: f32,
    k: f32,
    /// All-pass state per side and stage: previous input and output.
    x1: [[f32; MAX_STAGES]; 2],
    y1: [[f32; MAX_STAGES]; 2],
    last: [f32; 2],
    lfo: Lfo,
    stages: usize,
    sync: bool,
    rate: f32,
    div: usize,
    center: Smooth,
    depth: Smooth,
    fb: Smooth,
    stereo: Smooth,
    mix: Smooth,
    primed: bool,
    /// Break frequency of the stages just used, per side (meters).
    freq: [f32; 2],
}

impl Default for Phaser {
    fn default() -> Self {
        Self {
            sr: 48_000.0,
            k: 0.0,
            x1: [[0.0; MAX_STAGES]; 2],
            y1: [[0.0; MAX_STAGES]; 2],
            last: [0.0; 2],
            lfo: Lfo::default(),
            stages: 4,
            sync: false,
            rate: 0.4,
            div: 4,
            center: Smooth { v: 800.0, target: 800.0 },
            depth: Smooth { v: 0.7, target: 0.7 },
            fb: Smooth::default(),
            stereo: Smooth { v: 0.25, target: 0.25 },
            mix: Smooth { v: 0.5, target: 0.5 },
            primed: false,
            freq: [800.0; 2],
        }
    }
}

/// The phaser's LFO: a triangle, which with the exponential sweep spends equal time per octave.
#[inline(always)]
pub fn phaser_lfo(phase: f32) -> f32 {
    let p = phase - phase.floor();
    if p < 0.5 {
        4.0 * p - 1.0
    } else {
        3.0 - 4.0 * p
    }
}

/// First-order all-pass coefficient for break frequency `f` (phase −90° there).
#[inline(always)]
pub fn allpass_coef(f: f32, sr: f32) -> f32 {
    let t = (PI * f / sr).tan();
    (t - 1.0) / (t + 1.0)
}

impl Processor for Phaser {
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sr = sample_rate.max(8000.0);
        self.k = 1.0 - (-1.0 / (0.02 * self.sr)).exp();
        self.reset();
    }

    fn set_params(&mut self, p: &ParamBlock) {
        use phaser_idx as i;
        let v = &p.values;
        self.stages = STAGE_N[(v[i::STAGES].round().max(0.0) as usize).min(STAGE_N.len() - 1)];
        self.sync = v[i::SYNC] >= 0.5;
        self.rate = v[i::RATE].clamp(0.01, 20.0);
        self.div = (v[i::DIV].round().max(0.0) as usize).min(DIVS.len() - 1);
        self.center.target = v[i::CENTER].clamp(50.0, 12_000.0);
        self.depth.target = (v[i::DEPTH] / 100.0).clamp(0.0, 1.0);
        self.fb.target = (v[i::FB] / 100.0).clamp(-0.95, 0.95);
        self.stereo.target = (v[i::STEREO] / 360.0).clamp(0.0, 0.5);
        self.mix.target = (v[i::MIX] / 100.0).clamp(0.0, 1.0);
        if !self.primed {
            self.primed = true;
            for s in [&mut self.center, &mut self.depth, &mut self.fb, &mut self.stereo, &mut self.mix] {
                s.snap();
            }
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        let n = l.len().min(r.len());
        let (sr, k) = (self.sr, self.k);
        let inc = self.lfo.block(self.sync, self.rate, self.div, ctx);
        let fmax = 0.45 * sr;
        for s in 0..n {
            let center = self.center.tick(k);
            let depth = self.depth.tick(k);
            let fb = self.fb.tick(k);
            let stereo = self.stereo.tick(k);
            let mix = self.mix.tick(k);
            let ph = self.lfo.phase as f32;
            for c in 0..2 {
                let lfo = phaser_lfo(ph + if c == 1 { stereo } else { 0.0 });
                let f = (center * (PHASER_OCTAVES * depth * lfo).exp2()).clamp(20.0, fmax);
                self.freq[c] = f;
                let a = allpass_coef(f, sr);
                let x = if c == 0 { l[s] } else { r[s] };
                let mut u = x + fb * soft(self.last[c]);
                let (xs, ys) = (&mut self.x1[c], &mut self.y1[c]);
                for st in 0..self.stages {
                    // y[n] = a·x[n] + x[n−1] − a·y[n−1]
                    let y = flush(a * u + xs[st] - a * ys[st]);
                    xs[st] = u;
                    ys[st] = y;
                    u = y;
                }
                self.last[c] = u;
                let out = x * (1.0 - mix) + u * mix;
                if c == 0 {
                    l[s] = out;
                } else {
                    r[s] = out;
                }
            }
            self.lfo.advance(inc);
        }
    }

    fn reset(&mut self) {
        self.x1 = [[0.0; MAX_STAGES]; 2];
        self.y1 = [[0.0; MAX_STAGES]; 2];
        self.last = [0.0; 2];
        for s in [&mut self.center, &mut self.depth, &mut self.fb, &mut self.stereo, &mut self.mix] {
            s.snap();
        }
    }

    fn tail_samples(&self) -> Option<usize> {
        // the stages ring for about 1/(2π·f) each at the lowest sweep frequency; feedback repeats that
        let ring = self.stages as f32 / (TAU * 20.0) * self.sr;
        Some((ring * repeats(self.fb.target.max(self.fb.v)).min(400.0) + 0.05 * self.sr) as usize)
    }

    /// `[0]`/`[1]` the stages' break frequency left/right (Hz), `[2]` stages, `[3]` LFO phase.
    fn meters(&self, out: &mut [f32; METERS]) {
        out[0] = self.freq[0];
        out[1] = self.freq[1];
        out[2] = self.stages as f32;
        out[3] = self.lfo.phase as f32;
    }
}

#[cfg(test)]
#[path = "modulation_tests.rs"]
mod tests;
