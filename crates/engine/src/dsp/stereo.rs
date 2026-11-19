//! Stereo: width, mono-safe spread, bass to mono, gain, balance, polarity and channel swap.
//!
//! **Width** scales the side signal (S = (L − R) / 2) against the mid (M = (L + R) / 2): 0 % is
//! mono, 100 % leaves the image alone, 200 % doubles the side (+6 dB, the most that stays useful;
//! Ozone Imager stops at the same point).
//!
//! **Spread** widens what width cannot: a mono source has no side to scale. A copy of the mid
//! (high-passed at 250 Hz, low-passed at 9 kHz) runs through a short FIR of 48 ±1 impulses over 0.8–12 ms with a 5 ms decay
//! (velvet noise: decorrelated from the input, with no audible echo or comb of its own) and is
//! added to the side only. L = M + d and R = M − d, so L + R = 2M: summed to mono the spread
//! cancels exactly, which a Haas delay or an L/R all-pass widener cannot promise.
//!
//! **Bass mono** splits each channel with a Linkwitz-Riley crossover (`dsp::xover`) and sums the
//! band below the frequency to mono, leaving the top as it was. A high-pass on the side alone
//! would turn the phase of the side against the mid around the cutoff; the crossover keeps both
//! bands in phase. The crossover path is faded in over 10 ms when bass mono is switched on and
//! out when it is off, so with bass mono off the effect is an exact pass-through at its defaults.
//!
//! **Balance** (pan) turns down only the opposite channel on a −3 dB sine law, so a centred stereo
//! signal passes at unity. Every control is smoothed per sample; polarity and swap cross-fade over
//! 5 ms, so none of them clicks. Nothing allocates after `prepare`.

use std::f32::consts::FRAC_PI_2;

use super::xover::Lr4;
use super::{flush, Biquad, FilterType};
use crate::processor::{ParamBlock, ProcessCtx, Processor, METERS};

/// Parameter order (= catalog.rs STEREO).
pub mod idx {
    pub const WIDTH: usize = 0;
    pub const SPREAD: usize = 1;
    pub const BASS: usize = 2;
    pub const BASS_HZ: usize = 3;
    pub const GAIN: usize = 4;
    pub const PAN: usize = 5;
    pub const INV_L: usize = 6;
    pub const INV_R: usize = 7;
    pub const SWAP: usize = 8;
}

/// Taps of the spread's decorrelation FIR: dense and short, so it reads as width, not as a small
/// room (a sparse 16-tap kernel over 18 ms was heard as a metallic, flammy halo on drums).
const TAPS: usize = 48;
/// Its span, seconds, and the decay of the taps' level.
const SPAN_LO: f32 = 0.0008;
const SPAN_HI: f32 = 0.012;
const SPAN_DECAY: f32 = 0.005;
/// The spread leaves the low end alone below this, and darkens above `SPREAD_LP` (a bright
/// decorrelated copy sounds tinny on hats and snare wires).
const SPREAD_HP: f32 = 250.0;
const SPREAD_LP: f32 = 9000.0;
/// Correlation and level meters integrate over about this long.
const METER_S: f32 = 0.3;

#[derive(Clone, Copy, Default)]
struct Smooth {
    v: f32,
    target: f32,
}

impl Smooth {
    #[inline(always)]
    fn tick(&mut self, k: f32) -> f32 {
        self.v += (self.target - self.v) * k;
        if (self.target - self.v).abs() < 1e-6 {
            self.v = self.target;
        }
        self.v
    }
    fn snap(&mut self) {
        self.v = self.target;
    }
}

/// Running sums for correlation, balance and side/mid ratio over ~300 ms.
#[derive(Clone, Copy, Default)]
pub struct StereoStats {
    ll: f32,
    rr: f32,
    lr: f32,
}

impl StereoStats {
    #[inline(always)]
    pub fn feed(&mut self, l: f32, r: f32, k: f32) {
        self.ll += (l * l - self.ll) * k;
        self.rr += (r * r - self.rr) * k;
        self.lr += (l * r - self.lr) * k;
    }
    /// Pearson correlation of L and R, +1 mono, 0 unrelated, −1 out of phase (1 when silent).
    pub fn correlation(&self) -> f32 {
        let d = (self.ll * self.rr).sqrt();
        if d < 1e-12 {
            1.0
        } else {
            (self.lr / d).clamp(-1.0, 1.0)
        }
    }
    /// −1 all left … +1 all right (by energy).
    pub fn balance(&self) -> f32 {
        let s = self.ll + self.rr;
        if s < 1e-12 {
            0.0
        } else {
            (self.rr - self.ll) / s
        }
    }
    /// Side energy against mid energy, dB (−60 for mono).
    pub fn side_to_mid_db(&self) -> f32 {
        // M² = (ll + rr + 2lr)/4, S² = (ll + rr − 2lr)/4
        let m = (self.ll + self.rr + 2.0 * self.lr).max(0.0);
        let s = (self.ll + self.rr - 2.0 * self.lr).max(0.0);
        if m + s < 1e-12 {
            -60.0
        } else {
            (10.0 * ((s + 1e-12) / (m + 1e-12)).log10()).clamp(-60.0, 30.0)
        }
    }
    pub fn level(&self) -> f32 {
        self.ll + self.rr
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

pub struct Stereo {
    sr: f32,
    /// smoother coefficient, 10 ms
    k: f32,
    /// 5 ms, for polarity and swap
    k_fast: f32,
    /// meters
    k_meter: f32,
    width: Smooth,
    spread: Smooth,
    /// 1 = the band below `bass_hz` is mono
    bass: Smooth,
    /// 1 = the crossover path is heard (bass mono on, or fading)
    xo: Smooth,
    gain: Smooth,
    pan: Smooth,
    pol_l: Smooth,
    pol_r: Smooth,
    swap: Smooth,
    bass_hz: f32,
    /// the frequency the crossovers are set to (glides towards `bass_hz` per block)
    xo_hz: f32,
    split: [Lr4; 2],
    // spread
    hp: Biquad,
    lp: Biquad,
    line: Vec<f32>,
    mask: usize,
    w: usize,
    taps: [usize; TAPS],
    tap_g: [f32; TAPS],
    // meters
    stats_in: StereoStats,
    stats: StereoStats,
    lows: StereoStats,
    low_lp: [Biquad; 2],
    primed: bool,
}

impl Default for Stereo {
    fn default() -> Self {
        let one = Smooth { v: 1.0, target: 1.0 };
        Self {
            sr: 48_000.0,
            k: 0.002,
            k_fast: 0.004,
            k_meter: 0.0001,
            width: one,
            spread: Smooth::default(),
            bass: Smooth::default(),
            xo: Smooth::default(),
            gain: one,
            pan: Smooth::default(),
            pol_l: one,
            pol_r: one,
            swap: Smooth::default(),
            bass_hz: 120.0,
            xo_hz: 120.0,
            split: [Lr4::default(); 2],
            hp: Biquad::default(),
            lp: Biquad::default(),
            line: vec![],
            mask: 0,
            w: 0,
            taps: [0; TAPS],
            tap_g: [0.0; TAPS],
            stats_in: StereoStats::default(),
            stats: StereoStats::default(),
            lows: StereoStats::default(),
            low_lp: [Biquad::default(); 2],
            primed: false,
        }
    }
}

/// Positions (log-spaced with a fixed jitter) and signs of the decorrelation taps, normalised so
/// the FIR passes white noise at unity power. Deterministic: the same session sounds the same.
fn velvet(sr: f32) -> ([usize; TAPS], [f32; TAPS]) {
    // a fixed pseudo-random sequence (same every session): signs and a jitter of each tap's time
    let mut seed = 0x2545_f491u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let mut pos = [0usize; TAPS];
    let mut g = [0f32; TAPS];
    let ratio = SPAN_HI / SPAN_LO;
    let mut e = 0.0;
    for i in 0..TAPS {
        let u = (i as f32 + 0.5 + 0.8 * (rnd() - 0.5)) / TAPS as f32;
        let t = SPAN_LO * ratio.powf(u.clamp(0.0, 1.0));
        pos[i] = ((t * sr) as usize).max(1);
        // later taps quieter: a fast decay sounds like width, a flat burst like a room
        let amp = (-(t - SPAN_LO) / SPAN_DECAY).exp();
        g[i] = if rnd() < 0.5 { amp } else { -amp };
        e += amp * amp;
    }
    let norm = 1.0 / e.sqrt();
    for x in &mut g {
        *x *= norm;
    }
    (pos, g)
}

impl Processor for Stereo {
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sr = sample_rate.max(8000.0);
        let k = |s: f32| 1.0 - (-1.0 / (s * self.sr)).exp();
        self.k = k(0.01);
        self.k_fast = k(0.005);
        self.k_meter = k(METER_S);
        let need = (SPAN_HI * self.sr) as usize + 4;
        let len = need.next_power_of_two();
        self.line = vec![0.0; len];
        self.mask = len - 1;
        let (p, g) = velvet(self.sr);
        self.taps = p;
        self.tap_g = g;
        self.hp.set(FilterType::Highpass, self.sr, SPREAD_HP, 0.707, 0.0);
        self.lp.set(FilterType::Lowpass, self.sr, SPREAD_LP, 0.707, 0.0);
        for f in &mut self.low_lp {
            f.set(FilterType::Lowpass, self.sr, 120.0, 0.707, 0.0);
        }
        self.xo_hz = self.bass_hz;
        for s in &mut self.split {
            s.set(f64::from(self.xo_hz), f64::from(self.sr));
        }
        self.reset();
    }

    fn set_params(&mut self, p: &ParamBlock) {
        let v = &p.values;
        self.width.target = (v[idx::WIDTH] / 100.0).clamp(0.0, 2.0);
        // squared: the first half of the knob is subtle (70 % was heard as far too much)
        let sp = (v[idx::SPREAD] / 100.0).clamp(0.0, 1.0);
        self.spread.target = sp * sp;
        let bass_on = v[idx::BASS] >= 0.5;
        self.bass.target = if bass_on { 1.0 } else { 0.0 };
        self.xo.target = self.bass.target;
        self.bass_hz = v[idx::BASS_HZ].clamp(20.0, 1000.0);
        let g = v[idx::GAIN];
        self.gain.target = if g <= -59.5 { 0.0 } else { 10f32.powf(g.clamp(-60.0, 24.0) / 20.0) };
        self.pan.target = (v[idx::PAN] / 100.0).clamp(-1.0, 1.0);
        self.pol_l.target = if v[idx::INV_L] >= 0.5 { -1.0 } else { 1.0 };
        self.pol_r.target = if v[idx::INV_R] >= 0.5 { -1.0 } else { 1.0 };
        self.swap.target = if v[idx::SWAP] >= 0.5 { 1.0 } else { 0.0 };
        if !self.primed {
            self.primed = true;
            for s in [
                &mut self.width,
                &mut self.spread,
                &mut self.bass,
                &mut self.xo,
                &mut self.gain,
                &mut self.pan,
                &mut self.pol_l,
                &mut self.pol_r,
                &mut self.swap,
            ] {
                s.snap();
            }
            self.xo_hz = self.bass_hz;
            for s in &mut self.split {
                s.set(f64::from(self.xo_hz), f64::from(self.sr));
            }
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessCtx) {
        let n = l.len().min(r.len());
        let (k, kf, km) = (self.k, self.k_fast, self.k_meter);
        // the crossover glides to a new frequency over a few blocks (a jump would click)
        if (self.xo_hz - self.bass_hz).abs() > 0.01 {
            let blk = n as f32 / self.sr;
            let a = 1.0 - (-blk / 0.03).exp();
            self.xo_hz = (self.xo_hz.ln() + (self.bass_hz.ln() - self.xo_hz.ln()) * a).exp();
            if (self.xo_hz - self.bass_hz).abs() < 0.05 {
                self.xo_hz = self.bass_hz;
            }
            for s in &mut self.split {
                s.set(f64::from(self.xo_hz), f64::from(self.sr));
            }
        }
        let mut pan_last = f32::NAN;
        let (mut gl, mut gr) = (1.0, 1.0);
        for i in 0..n {
            let (x_l, x_r) = (l[i], r[i]);
            self.stats_in.feed(x_l, x_r, km);
            let a = x_l * self.pol_l.tick(kf);
            let b = x_r * self.pol_r.tick(kf);
            let sw = self.swap.tick(kf);
            let (a, b) = (a + (b - a) * sw, b + (a - b) * sw);
            let w = self.width.tick(k);
            let sp = self.spread.tick(k);
            let bm = self.bass.tick(k);
            let xo = self.xo.tick(k);

            let m = 0.5 * (a + b);
            let s = 0.5 * (a - b);
            // spread: a decorrelated copy of the mid, into the side only
            let mh = self.lp.tick(self.hp.tick(m));
            self.line[self.w] = mh;
            let mut d = 0.0;
            for t in 0..TAPS {
                d += self.line[(self.w.wrapping_sub(self.taps[t])) & self.mask] * self.tap_g[t];
            }
            self.w = (self.w + 1) & self.mask;
            let side = s * w + sp * d;
            let (mut yl, mut yr) = (m + side, m - side);
            // the crossovers keep running so the path fades in from a settled state
            let (lo_a, hi_a) = self.split[0].split(f64::from(a));
            let (lo_b, hi_b) = self.split[1].split(f64::from(b));
            if xo > 0.0 {
                let (lo_a, hi_a, lo_b, hi_b) = (lo_a as f32, hi_a as f32, lo_b as f32, hi_b as f32);
                let m_lo = 0.5 * (lo_a + lo_b);
                let s_lo = 0.5 * (lo_a - lo_b) * w * (1.0 - bm);
                let m_hi = 0.5 * (hi_a + hi_b);
                let s_hi = 0.5 * (hi_a - hi_b) * w + sp * d;
                let (cl, cr) = (m_lo + s_lo + m_hi + s_hi, m_lo - s_lo + m_hi - s_hi);
                yl += (cl - yl) * xo;
                yr += (cr - yr) * xo;
            }
            let p = self.pan.tick(k);
            if p != pan_last {
                pan_last = p;
                gl = if p > 0.0 { (p * FRAC_PI_2).cos() } else { 1.0 };
                gr = if p < 0.0 { (-p * FRAC_PI_2).cos() } else { 1.0 };
            }
            let g = self.gain.tick(k);
            let (ol, or) = (flush(yl * gl * g), flush(yr * gr * g));
            l[i] = ol;
            r[i] = or;
            self.stats.feed(ol, or, km);
            let (ll, lr) = (self.low_lp[0].tick(ol), self.low_lp[1].tick(or));
            self.lows.feed(ll, lr, km);
        }
    }

    fn reset(&mut self) {
        self.line.fill(0.0);
        self.w = 0;
        for s in &mut self.split {
            s.clear();
        }
        self.hp.reset();
        self.lp.reset();
        for f in &mut self.low_lp {
            f.reset();
        }
        self.stats_in.clear();
        self.stats.clear();
        self.lows.clear();
    }

    fn tail_samples(&self) -> Option<usize> {
        // the spread's line and the meters' memory
        Some((0.05 * self.sr) as usize)
    }

    fn park(&mut self) {
        self.reset();
        for s in [
            &mut self.width,
            &mut self.spread,
            &mut self.bass,
            &mut self.xo,
            &mut self.gain,
            &mut self.pan,
            &mut self.pol_l,
            &mut self.pol_r,
            &mut self.swap,
        ] {
            s.snap();
        }
    }

    fn meters(&self, out: &mut [f32; METERS]) {
        out[0] = self.stats.correlation();
        out[1] = self.stats.balance();
        out[2] = self.stats.side_to_mid_db();
        out[3] = self.lows.correlation();
        out[4] = self.stats_in.correlation();
        out[5] = self.stats_in.side_to_mid_db();
        out[6] = self.xo_hz;
        // 1 while the low band carries enough to judge its correlation
        out[7] = if self.lows.level() > 1e-7 { 1.0 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::MAX_PARAMS;

    fn block(vals: &[(usize, f32)]) -> ParamBlock {
        let mut p = ParamBlock { values: [0.0; MAX_PARAMS], ..Default::default() };
        // defaults (catalog.rs)
        p.values[idx::WIDTH] = 100.0;
        p.values[idx::BASS_HZ] = 120.0;
        for &(i, v) in vals {
            p.values[i] = v;
        }
        p
    }

    fn noise(n: usize, seed: &mut u32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (*seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect()
    }

    fn run(s: &mut Stereo, l: &mut [f32], r: &mut [f32]) {
        let ctx = ProcessCtx { sample_rate: 48_000.0, bpm: 120.0, beat: 0.0, playing: true };
        for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            s.process(a, b, &ctx);
        }
    }

    #[test]
    fn defaults_pass_through_exactly() {
        let mut s = Stereo::default();
        s.prepare(48_000.0, 512);
        s.set_params(&block(&[]));
        let mut seed = 7;
        let (l0, r0) = (noise(4800, &mut seed), noise(4800, &mut seed));
        let (mut l, mut r) = (l0.clone(), r0.clone());
        run(&mut s, &mut l, &mut r);
        for i in 0..l.len() {
            assert!((l[i] - l0[i]).abs() < 1e-6 && (r[i] - r0[i]).abs() < 1e-6, "sample {i}");
        }
    }

    #[test]
    fn spread_widens_a_mono_source_and_cancels_in_mono() {
        let mut s = Stereo::default();
        s.prepare(48_000.0, 512);
        s.set_params(&block(&[(idx::SPREAD, 100.0)]));
        let mut seed = 3;
        let m0 = noise(48_000, &mut seed);
        let (mut l, mut r) = (m0.clone(), m0.clone());
        run(&mut s, &mut l, &mut r);
        // the mono sum is the input, untouched
        for i in 0..l.len() {
            assert!((0.5 * (l[i] + r[i]) - m0[i]).abs() < 1e-5, "sample {i}");
        }
        // and the two sides are no longer the same signal
        assert!(s.stats.correlation() < 0.6, "correlation {}", s.stats.correlation());
    }

    #[test]
    fn width_zero_is_mono_and_two_hundred_doubles_the_side() {
        let mut seed = 11;
        let (l0, r0) = (noise(9600, &mut seed), noise(9600, &mut seed));
        for (w, want) in [(0.0f32, 0.0f32), (200.0, 2.0)] {
            let mut s = Stereo::default();
            s.prepare(48_000.0, 512);
            s.set_params(&block(&[(idx::WIDTH, w)]));
            let (mut l, mut r) = (l0.clone(), r0.clone());
            run(&mut s, &mut l, &mut r);
            for i in 0..l.len() {
                let side = 0.5 * (l[i] - r[i]);
                assert!((side - want * 0.5 * (l0[i] - r0[i])).abs() < 1e-5);
                assert!((0.5 * (l[i] + r[i]) - 0.5 * (l0[i] + r0[i])).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn bass_mono_removes_the_side_below_the_crossover_only() {
        let sr = 48_000.0;
        let tone = |hz: f32, i: usize, ph: f32| (std::f32::consts::TAU * hz * i as f32 / sr + ph).sin() * 0.5;
        for (hz, low) in [(50.0, true), (2_000.0, false)] {
            let mut s = Stereo::default();
            s.prepare(sr, 512);
            s.set_params(&block(&[(idx::BASS, 1.0), (idx::BASS_HZ, 150.0)]));
            // the same tone, out of phase: pure side
            let mut l: Vec<f32> = (0..48_000).map(|i| tone(hz, i, 0.0)).collect();
            let mut r: Vec<f32> = l.iter().map(|x| -x).collect();
            run(&mut s, &mut l, &mut r);
            let side: f32 = (24_000..48_000).map(|i| (0.5 * (l[i] - r[i])).powi(2)).sum::<f32>() / 24_000.0;
            if low {
                assert!(side < 0.125 * 0.002, "{hz} Hz side power {side}");
            } else {
                assert!((side / 0.125 - 1.0).abs() < 0.05, "{hz} Hz side power {side}");
            }
        }
    }

    #[test]
    fn switching_bass_mono_does_not_click() {
        let sr = 48_000.0;
        let mut s = Stereo::default();
        s.prepare(sr, 512);
        s.set_params(&block(&[]));
        let ctx = ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: true };
        let mut prev = 0.0f32;
        let mut worst = 0.0f32;
        for b in 0..200 {
            if b == 100 {
                s.set_params(&block(&[(idx::BASS, 1.0), (idx::INV_R, 1.0), (idx::SWAP, 1.0)]));
            }
            let mut l: Vec<f32> = (0..64).map(|i| (std::f32::consts::TAU * 440.0 * (b * 64 + i) as f32 / sr).sin() * 0.5).collect();
            let mut r = l.clone();
            s.process(&mut l, &mut r, &ctx);
            for &x in &l {
                worst = worst.max((x - prev).abs());
                prev = x;
            }
        }
        // a 440 Hz sine at 0.5 moves at most ~0.029 per sample
        assert!(worst < 0.04, "largest step {worst}");
    }

    #[test]
    fn balance_keeps_the_centre_at_unity() {
        let mut s = Stereo::default();
        s.prepare(48_000.0, 512);
        s.set_params(&block(&[(idx::PAN, 50.0)]));
        let (mut l, mut r) = (vec![0.5f32; 4800], vec![0.5f32; 4800]);
        run(&mut s, &mut l, &mut r);
        assert!((r[4799] - 0.5).abs() < 1e-5);
        assert!((l[4799] - 0.5 * (std::f32::consts::FRAC_PI_4).cos()).abs() < 1e-4);
    }
}
