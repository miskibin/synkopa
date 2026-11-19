//! Linkwitz-Riley crossovers, shared by the Stereo effect (bass mono) and the Multiband compressor.
//!
//! An LR4 split is a 2nd-order Butterworth low-pass run twice and the matching high-pass run twice
//! (RBJ biquads, Q = 1/√2, the same bilinear warp at the crossover frequency). The two outputs add
//! up to a 2nd-order all-pass at that frequency: flat magnitude, only the phase turns. That holds
//! exactly in the digital domain too, because every filter is the same analog prototype through
//! the same bilinear transform. A three-band split runs the low band through the all-pass of the
//! upper crossover, so all three bands share the phase of the upper bands and sum flat for any
//! spacing (Linkwitz; Rane note 160). A dry path that must mix with the bands (parallel blends)
//! runs through both all-passes, so blending never cuts a notch at a crossover.
//!
//! The filters run in f64: at 40 Hz and 96 kHz the poles sit so close to the unit circle that
//! f32 coefficients and state would add audible noise and summing error.

use std::f64::consts::PI;

/// Transposed direct form II biquad in f64.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bq64 {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

/// Butterworth Q.
const Q_BW: f64 = std::f64::consts::FRAC_1_SQRT_2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Lowpass,
    Highpass,
    Allpass,
}

impl Bq64 {
    /// New coefficients for `shape` at `f` Hz (the state is kept, so it can move while running).
    pub fn set(&mut self, shape: Shape, f: f64, sr: f64) {
        let f = f.clamp(5.0, sr * 0.45);
        let w = 2.0 * PI * f / sr;
        let (sw, cw) = w.sin_cos();
        let alpha = sw / (2.0 * Q_BW);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = match shape {
            Shape::Lowpass => ((1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0),
            Shape::Highpass => ((1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0),
            Shape::Allpass => (1.0 - alpha, -2.0 * cw, 1.0 + alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = -2.0 * cw / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline(always)]
    pub fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        // denormal guard: the state of a filter fed silence decays through the subnormal range
        if self.z1.abs() < 1e-30 {
            self.z1 = 0.0;
        }
        if self.z2.abs() < 1e-30 {
            self.z2 = 0.0;
        }
        y
    }

    pub fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// One LR4 (24 dB/oct) split of one channel.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lr4 {
    lp: [Bq64; 2],
    hp: [Bq64; 2],
}

impl Lr4 {
    pub fn set(&mut self, f: f64, sr: f64) {
        for b in &mut self.lp {
            b.set(Shape::Lowpass, f, sr);
        }
        for b in &mut self.hp {
            b.set(Shape::Highpass, f, sr);
        }
    }

    /// (low, high). `low + high` is `x` through a 2nd-order all-pass at the crossover.
    #[inline(always)]
    pub fn split(&mut self, x: f64) -> (f64, f64) {
        let lo = self.lp[0].run(x);
        let lo = self.lp[1].run(lo);
        let hi = self.hp[0].run(x);
        let hi = self.hp[1].run(hi);
        (lo, hi)
    }

    pub fn clear(&mut self) {
        for b in self.lp.iter_mut().chain(self.hp.iter_mut()) {
            b.clear();
        }
    }
}

/// The all-pass an [`Lr4`] at the same frequency turns its sum into.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ap2(Bq64);

impl Ap2 {
    pub fn set(&mut self, f: f64, sr: f64) {
        self.0.set(Shape::Allpass, f, sr);
    }
    #[inline(always)]
    pub fn run(&mut self, x: f64) -> f64 {
        self.0.run(x)
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Magnitude of a sine through `f` after settling, in dB relative to the input.
    fn gain_db(sr: f64, hz: f64, mut f: impl FnMut(f64) -> f64) -> f64 {
        let n = (sr * 0.5) as usize;
        let (mut e_in, mut e_out) = (0.0, 0.0);
        for i in 0..n {
            let x = (2.0 * PI * hz * i as f64 / sr).sin();
            let y = f(x);
            if i > n / 2 {
                e_in += x * x;
                e_out += y * y;
            }
        }
        10.0 * (e_out / e_in).log10()
    }

    #[test]
    fn lr4_sums_flat() {
        let sr = 48_000.0;
        for hz in [30.0, 88.0, 120.0, 500.0, 2_500.0, 9_000.0] {
            let mut x = Lr4::default();
            x.set(120.0, sr);
            let g = gain_db(sr, hz, |s| {
                let (a, b) = x.split(s);
                a + b
            });
            assert!(g.abs() < 0.001, "{hz} Hz: {g} dB");
        }
    }

    #[test]
    fn three_bands_sum_flat_even_close_together() {
        // an octave apart: without the all-pass on the low band this is several dB off
        let sr = 48_000.0;
        for hz in [50.0, 200.0, 283.0, 400.0, 1_000.0, 8_000.0] {
            let (mut a, mut b, mut ap) = (Lr4::default(), Lr4::default(), Ap2::default());
            a.set(200.0, sr);
            b.set(400.0, sr);
            ap.set(400.0, sr);
            let g = gain_db(sr, hz, |s| {
                let (lo, rest) = a.split(s);
                let (mid, hi) = b.split(rest);
                ap.run(lo) + mid + hi
            });
            assert!(g.abs() < 0.001, "{hz} Hz: {g} dB");
        }
    }

    #[test]
    fn dry_through_both_allpasses_matches_the_band_sum() {
        // a blend of dry and bands then has no notch: the two are the same signal
        let sr = 44_100.0;
        let (mut a, mut b, mut ap) = (Lr4::default(), Lr4::default(), Ap2::default());
        let (mut d1, mut d2) = (Ap2::default(), Ap2::default());
        a.set(88.0, sr);
        b.set(2_500.0, sr);
        ap.set(2_500.0, sr);
        d1.set(88.0, sr);
        d2.set(2_500.0, sr);
        let mut worst = 0f64;
        let mut seed = 1u32;
        for _ in 0..20_000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let x = (seed >> 8) as f64 / (1u32 << 24) as f64 - 0.5;
            let (lo, rest) = a.split(x);
            let (mid, hi) = b.split(rest);
            let bands = ap.run(lo) + mid + hi;
            let dry = d2.run(d1.run(x));
            worst = worst.max((bands - dry).abs());
        }
        assert!(worst < 1e-9, "{worst}");
    }
}
