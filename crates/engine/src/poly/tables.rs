//! Band-limited wavetables for the synth oscillators, generated once (never on the audio thread).
//!
//! The first eight waves have two frames (Shape morphs between them); the wavetables after them
//! ([`BASIC`] on) have [`WT_FRAMES`] frames each and Shape scans through them (the wavetable
//! position), crossfading neighbouring frames. Every frame is stored as a set of
//! mip levels: level `j` keeps harmonics `1..=harmonics(j)`, where the count falls by a third
//! of an octave per level. At playback the oscillator picks the richest level whose top
//! harmonic is still below Nyquist and crossfades it with the next poorer one, so no
//! harmonic is ever above Nyquist (alias-free by construction) and brightness changes
//! smoothly with pitch.
//!
//! Third-octave spacing instead of the usual one level per octave: with whole octaves the
//! safe level loses up to an octave of top harmonics (a saw at the bottom of a level's range
//! stops at fs/4 = 12 kHz). With thirds the loss is at most a third of an octave (content
//! above ~0.79·fs/2). Memory: (8 × 2 + 6 × 8) frames × 25 levels × 2049 floats ≈ 13 MB.
//!
//! The wavetables are defined by their spectra, so effects that alias when done live (hard sync,
//! phase modulation, a formant sweep) are alias-free here: the [`SYNC`] table *is* a hard-synced
//! saw at eight sync ratios, band-limited like every other frame.
//!
//! Harmonics are capped at [`MAX_H`] = 256 per 2048-sample table, so the table is at least
//! 4× oversampled relative to its content and linear interpolation's images stay below
//! about −65 dB (measured in `tests::alias_snr`).

use std::sync::OnceLock;

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

pub const SIZE: usize = 2048;
const STRIDE: usize = SIZE + 1;
pub const MAX_H: usize = 256;
/// Mip levels per octave.
pub const LPO: f32 = 3.0;
pub const N_WAVES: usize = 14;
/// Frames of a classic wave (Sine … Metal).
pub const FRAMES: usize = 2;
/// Frames of a wavetable (Basic … Comb).
pub const WT_FRAMES: usize = 8;

pub const SINE: u8 = 0;
pub const TRI: u8 = 1;
pub const SAW: u8 = 2;
pub const SQUARE: u8 = 3;
pub const HOLLOW: u8 = 4;
pub const AIR: u8 = 5;
pub const DIGITAL: u8 = 6;
pub const METAL: u8 = 7;
/// Sine → triangle → saw → square → narrow pulse.
pub const BASIC: u8 = 8;
/// Vowels A → E → I → O → U (formants on the harmonic series).
pub const FORMANT: u8 = 9;
/// Additive: a sine gaining harmonics up to a full saw.
pub const HARMONICS: u8 = 10;
/// A saw hard-synced to the fundamental, slave ratio 1 → 8.
pub const SYNC: u8 = 11;
/// Phase modulation of a sine by its own octave, index rising: hollow → snarling.
pub const GROWL: u8 = 12;
/// A saw through a moving comb (notches sweep the spectrum: a phaser that is part of the wave).
pub const COMB: u8 = 13;

/// Frames of `wave`.
pub const fn frames(wave: u8) -> usize {
    if wave >= BASIC {
        WT_FRAMES
    } else {
        FRAMES
    }
}

/// Halfband FIR for the 2× oversampled waveshapers: the odd taps h[±1], h[±3], … h[±15]
/// (h[0] = 0.5, even taps 0). Kaiser-windowed sinc, β = 8, normalised for unity DC gain.
pub const HB_TAPS: usize = 8;

pub struct Bank {
    levels: usize,
    /// First frame of every wave (frames are stored wave after wave).
    first: [usize; N_WAVES],
    harm: Vec<usize>,
    data: Vec<f32>,
    pub hb: [f32; HB_TAPS],
}

static BANK: OnceLock<Bank> = OnceLock::new();

/// The shared tables (built on first use; call it off the audio thread first).
pub fn bank() -> &'static Bank {
    BANK.get_or_init(Bank::build)
}

/// Harmonic `n` (1-based) of a frame as (amplitude, phase) of `a·sin(2πnt + φ)`.
type Spec = Vec<(f64, f64)>;

fn hash(n: usize, salt: u32) -> f64 {
    let mut x = (n as u32).wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x as f64) / (u32::MAX as f64)
}

fn spec_fn(f: impl Fn(usize) -> (f64, f64)) -> Spec {
    (1..=MAX_H).map(f).collect()
}

/// Spectrum of a periodic time-domain shape (one cycle sampled at `m` points).
fn spec_of(m: usize, shape: impl Fn(f64) -> f64) -> Spec {
    let mut fft = RealFftPlanner::<f64>::new();
    let f = fft.plan_fft_forward(m);
    let mut x: Vec<f64> = (0..m).map(|i| shape(i as f64 / m as f64)).collect();
    let mut y = f.make_output_vec();
    f.process(&mut x, &mut y).unwrap();
    // X_n = (m·a/2)·e^{i(φ − π/2)}
    (1..=MAX_H)
        .map(|n| {
            let c = y[n] / (m as f64 / 2.0);
            (c.norm(), c.arg() + std::f64::consts::FRAC_PI_2)
        })
        .collect()
}

fn gauss(n: f64, c: f64, w: f64) -> f64 {
    (-((n - c) / w).powi(2) * 0.5).exp()
}

/// The frames of every wave.
fn spectra(wave: u8) -> Vec<Spec> {
    if wave >= BASIC {
        return (0..WT_FRAMES).map(|f| wt_frame(wave, f as f64 / (WT_FRAMES - 1) as f64)).collect();
    }
    use std::f64::consts::PI;
    let saw = |n: usize| (2.0 / PI / n as f64, PI); // falling edge at the cycle start: rising ramp
    let two: [Spec; FRAMES] = match wave {
        SINE => [
            spec_fn(|n| (if n == 1 { 1.0 } else { 0.0 }, 0.0)),
            // a warm, slightly saturated sine (2nd–4th harmonics)
            spec_fn(|n| {
                (
                    match n {
                        1 => 1.0,
                        2 => 0.32,
                        3 => 0.16,
                        4 => 0.06,
                        _ => 0.0,
                    },
                    0.0,
                )
            }),
        ],
        TRI => [
            spec_fn(|n| if n % 2 == 1 { (8.0 / (PI * PI) / (n * n) as f64, if (n / 2) % 2 == 0 { 0.0 } else { PI }) } else { (0.0, 0.0) }),
            // between a triangle and a square: odd harmonics at 1/n^1.5
            spec_fn(|n| if n % 2 == 1 { (1.0 / (n as f64).powf(1.5), 0.0) } else { (0.0, 0.0) }),
        ],
        SAW | SQUARE => [
            spec_fn(saw),
            // saw with its octave: brighter, more "double"
            spec_fn(|n| {
                let a = 2.0 / PI / n as f64 + if n % 2 == 0 { 0.7 * 2.0 / PI / (n / 2) as f64 } else { 0.0 };
                (a, PI)
            }),
        ],
        HOLLOW => [
            // odd harmonics with every fourth odd one missing: a hollow, pipe-like tone
            spec_fn(|n| if n % 2 == 1 && n % 8 != 7 { (1.0 / (n as f64).powf(1.15), 0.0) } else { (0.0, 0.0) }),
            // fundamental and octaves/even harmonics: organ-flute hollow
            spec_fn(|n| if n == 1 || (n % 2 == 0 && n % 6 != 0) { (1.0 / (n as f64).powf(1.05), 0.0) } else { (0.0, 0.0) }),
        ],
        AIR => [
            // formant bumps (open vowel) over a soft 1/sqrt(n) floor, scattered phases
            spec_fn(|n| {
                let x = n as f64;
                let a = (gauss(x, 3.0, 1.2) + 0.7 * gauss(x, 9.0, 2.5) + 0.4 * gauss(x, 24.0, 6.0) + 0.05) / x.sqrt();
                (a, 2.0 * PI * hash(n, 1))
            }),
            spec_fn(|n| {
                let x = n as f64;
                let a = (0.8 * gauss(x, 2.0, 0.8) + gauss(x, 6.0, 1.6) + 0.6 * gauss(x, 16.0, 4.0) + 0.35 * gauss(x, 40.0, 10.0) + 0.05)
                    / x.sqrt();
                (a, 2.0 * PI * hash(n, 2))
            }),
        ],
        DIGITAL => [
            // a 3-bit sine: stepped, "digital"
            spec_of(8192, |t| ((2.0 * PI * t).sin() * 3.5).round() / 3.5),
            // a 6-step staircase saw
            spec_of(8192, |t| ((t * 6.0).floor() / 5.0) * 2.0 - 1.0),
        ],
        _ => [
            // metallic: sparse, wide-spaced harmonics with slow decay
            spec_fn(|n| {
                const SET: [usize; 10] = [1, 5, 9, 14, 21, 30, 41, 53, 67, 83];
                (if SET.contains(&n) { 1.0 / (n as f64).powf(0.45) } else { 0.0 }, 2.0 * PI * hash(n, 3))
            }),
            spec_fn(|n| {
                const SET: [usize; 10] = [1, 3, 7, 11, 17, 26, 37, 49, 62, 78];
                (if SET.contains(&n) { 1.0 / (n as f64).powf(0.4) } else { 0.0 }, 2.0 * PI * hash(n, 4))
            }),
        ],
    };
    two.into()
}

/// One frame of a wavetable at position `x` ∈ [0, 1].
fn wt_frame(wave: u8, x: f64) -> Spec {
    use std::f64::consts::PI;
    let tau = 2.0 * PI;
    match wave {
        BASIC => {
            // sine, triangle, saw, square at x = 0, 2/7, 4/7, 6/7 (crossfaded in time), then
            // the pulse narrows
            let shape = |k: usize, t: f64| match k {
                0 => (tau * t).sin(),
                1 => 1.0 - 4.0 * ((t + 0.25).fract() - 0.5).abs(),
                2 => 2.0 * (t + 0.5).fract() - 1.0,
                _ => {
                    if t < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
            };
            let y = x * 7.0;
            if y > 6.0 {
                let pw = 0.5 - 0.38 * (y - 6.0);
                return spec_of(8192, move |t| if t < pw { 1.0 } else { -1.0 });
            }
            let (k, f) = ((y / 2.0).floor() as usize, (y / 2.0).fract());
            spec_of(8192, move |t| shape(k, t) * (1.0 - f) + shape((k + 1).min(3), t) * f)
        }
        FORMANT => {
            // formant pairs (F1, F2) in harmonics of a ~130 Hz voice: A E I O U
            const V: [(f64, f64); 5] = [(6.0, 9.5), (3.3, 13.5), (2.2, 18.0), (3.5, 6.5), (2.6, 5.5)];
            let y = x * 4.0;
            let i = (y.floor() as usize).min(3);
            let f = y - i as f64;
            let (a, b) = (V[i], V[i + 1]);
            let (f1, f2) = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
            spec_fn(|n| {
                let m = n as f64;
                let amp = (gauss(m, f1, 1.1) + 0.7 * gauss(m, f2, 1.8) + 0.25 * gauss(m, 2.0 * f2 + 4.0, 3.0) + 0.04) / m.powf(0.6);
                (amp, if n % 2 == 0 { PI } else { 0.0 })
            })
        }
        HARMONICS => {
            // harmonics fade in one after another: x = 0 a sine, x = 1 a saw up to harmonic 64
            let top = 64f64.powf(x);
            spec_fn(|n| {
                let m = n as f64;
                let g = if m > 64.0 { 0.0 } else { (top - m + 1.0).clamp(0.0, 1.0) };
                (g * 2.0 / PI / m, PI)
            })
        }
        SYNC => {
            let r = 1.0 + 7.0 * x;
            // a slave saw reset every master cycle, over a quarter of the master's own ramp so
            // the fundamental stays strong at high ratios
            spec_of(8192, move |t| 0.75 * (2.0 * (t * r).fract() - 1.0) + 0.25 * (2.0 * t - 1.0))
        }
        GROWL => {
            let idx = 0.3 + 5.5 * x * x;
            spec_of(8192, move |t| (tau * t + idx * (tau * 2.0 * t).sin() + 0.3 * idx * (tau * 3.0 * t).sin()).sin())
        }
        _ => {
            // COMB: a saw spectrum under an |cos| comb whose notch spacing shrinks along the table
            let d = 2.0 + 14.0 * x;
            spec_fn(|n| {
                let m = n as f64;
                (2.0 / PI / m * (0.15 + 0.85 * (PI * m / d).cos().abs()), PI)
            })
        }
    }
}

/// Harmonics kept at mip level `j`.
fn harmonics(j: usize) -> usize {
    ((MAX_H as f64) * 2f64.powf(-(j as f64) / LPO as f64)).floor().max(1.0) as usize
}

fn kaiser_halfband() -> [f32; HB_TAPS] {
    fn i0(x: f64) -> f64 {
        let (mut s, mut t) = (1.0, 1.0);
        for k in 1..40 {
            t *= (x / 2.0) / k as f64;
            s += t * t;
        }
        s
    }
    let m = (2 * HB_TAPS - 1) as f64; // half length in taps (15)
    let beta = 8.0;
    let mut h = [0f64; HB_TAPS];
    for (j, v) in h.iter_mut().enumerate() {
        let k = (2 * j + 1) as f64;
        let sinc = (std::f64::consts::PI * k / 2.0).sin() / (std::f64::consts::PI * k / 2.0);
        let w = i0(beta * (1.0 - (k / (m + 1.0)).powi(2)).max(0.0).sqrt()) / i0(beta);
        *v = 0.5 * sinc * w;
    }
    let s: f64 = h.iter().sum();
    let mut out = [0f32; HB_TAPS];
    for j in 0..HB_TAPS {
        out[j] = (h[j] * 0.25 / s) as f32;
    }
    out
}

impl Bank {
    fn build() -> Bank {
        let mut levels = 1;
        while harmonics(levels - 1) > 1 {
            levels += 1;
        }
        let harm: Vec<usize> = (0..levels).map(harmonics).collect();
        let mut first = [0usize; N_WAVES];
        let mut total = 0;
        for (w, f) in first.iter_mut().enumerate() {
            *f = total;
            total += frames(w as u8);
        }
        let mut data = vec![0f32; total * levels * STRIDE];
        let mut planner = RealFftPlanner::<f64>::new();
        let inv = planner.plan_fft_inverse(SIZE);
        let mut spec = inv.make_input_vec();
        let mut out = inv.make_output_vec();
        for w in 0..N_WAVES as u8 {
            for (f, sp) in spectra(w).iter().enumerate() {
                let mut gain = 1.0;
                for (j, &nh) in harm.iter().enumerate() {
                    spec.iter_mut().for_each(|c| *c = Complex::new(0.0, 0.0));
                    for (n, &(a, ph)) in sp.iter().enumerate().take(nh) {
                        // a·sin(θ+φ) = 2·Re(X·e^{iθ}) with X = (a/2)·e^{i(φ−π/2)}
                        spec[n + 1] = Complex::from_polar(a / 2.0, ph - std::f64::consts::FRAC_PI_2);
                    }
                    inv.process(&mut spec, &mut out).unwrap();
                    if j == 0 {
                        // normalise every level by the richest one's peak (no level jumps)
                        let pk = out.iter().fold(0f64, |m, v| m.max(v.abs()));
                        gain = if pk > 0.0 { 1.0 / pk } else { 1.0 };
                    }
                    let base = ((first[w as usize] + f) * levels + j) * STRIDE;
                    for i in 0..SIZE {
                        data[base + i] = (out[i] * gain) as f32;
                    }
                    data[base + SIZE] = data[base];
                }
            }
        }
        Bank { levels, first, harm, data, hb: kaiser_halfband() }
    }

    pub fn levels(&self) -> usize {
        self.levels
    }

    pub fn harmonics(&self, level: usize) -> usize {
        self.harm[level]
    }

    /// One cycle of every frame of every wave at `points` points (the richest level, peak 1):
    /// what the synth panel draws.
    pub fn display(&self, points: usize) -> Vec<Vec<Vec<f32>>> {
        (0..N_WAVES as u8)
            .map(|w| {
                (0..frames(w))
                    .map(|f| {
                        let t = self.table(w, f, 0);
                        (0..points).map(|i| (read(t, i as f32 / points as f32) * 1000.0).round() / 1000.0).collect()
                    })
                    .collect()
            })
            .collect()
    }

    /// The table of `wave`/`frame` at mip `level` (`SIZE + 1` samples, the last = the first).
    #[inline]
    pub fn table(&self, wave: u8, frame: usize, level: usize) -> &[f32] {
        let w = (wave as usize).min(N_WAVES - 1);
        let o = ((self.first[w] + frame.min(frames(w as u8) - 1)) * self.levels + level) * STRIDE;
        &self.data[o..o + STRIDE]
    }

    /// Mip levels for a phase increment `dt` (cycles per sample): (rich level, poor level,
    /// weight of the rich one). Both levels have every harmonic below Nyquist.
    #[inline]
    pub fn select(&self, dt: f32) -> (usize, usize, f32) {
        let dt = dt.abs().max(1e-9);
        // level j is safe while harmonics(j)·dt ≤ 0.5
        let jr = LPO * (2.0 * MAX_H as f32 * dt).log2();
        if jr <= 0.0 {
            return (0, 1.min(self.levels - 1), 1.0);
        }
        let js = jr.ceil();
        let w = js - jr;
        let j = (js as usize).min(self.levels - 1);
        (j, (j + 1).min(self.levels - 1), if j == self.levels - 1 { 1.0 } else { w })
    }
}

/// Linear-interpolated read of a table at phase `ph` ∈ [0, 1).
#[inline(always)]
pub fn read(t: &[f32], ph: f32) -> f32 {
    let x = ph * SIZE as f32;
    let i = (x as usize).min(SIZE - 1);
    let fr = x - i as f32;
    let a = t[i];
    a + (t[i + 1] - a) * fr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_below_nyquist() {
        let b = bank();
        // every selected level keeps all harmonics below Nyquist
        for k in 0..2000 {
            let dt = 0.5 * (k as f32 + 0.5) / 2000.0;
            let (j0, j1, _) = b.select(dt);
            for j in [j0, j1] {
                if b.harmonics(j) > 1 {
                    assert!(b.harmonics(j) as f32 * dt <= 0.5 + 1e-6, "dt {dt} level {j}");
                }
            }
        }
    }

    #[test]
    fn saw_table_is_a_ramp() {
        let b = bank();
        let t = b.table(SAW, 0, 0);
        // rising ramp: mid-cycle value near 0, increasing
        assert!(t[SIZE / 4] < t[SIZE / 2] && t[SIZE / 2] < t[3 * SIZE / 4], "{} {} {}", t[SIZE / 4], t[SIZE / 2], t[3 * SIZE / 4]);
        let pk = t.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((pk - 1.0).abs() < 1e-3);
    }
}
