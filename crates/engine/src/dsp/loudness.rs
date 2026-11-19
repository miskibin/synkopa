//! Analyzer: loudness to ITU-R BS.1770 / EBU R128, true peak, correlation. It never changes the
//! sound; the spectrum and the vectorscope come from the analysis taps every effect window has.
//!
//! **K-weighting.** Two biquads per channel, a high shelf (+4 dB above ~1.7 kHz, the head) and a
//! high-pass (~38 Hz), computed from their analog prototypes for the session's sample rate (the
//! coefficients printed in BS.1770 are for 48 kHz only). The constants are the ones every
//! conforming meter uses (libebur128 and its ports).
//!
//! **Loudness.** The weighted power of both channels is summed in 100 ms steps. Momentary is the
//! mean of the last 4 steps (400 ms), short-term of the last 30 (3 s), `L = −0.691 + 10·log10`.
//! Integrated loudness gates the 400 ms blocks (one every 100 ms, 75 % overlap) at −70 LUFS, then
//! at 10 LU below the loudness of what is left; the loudness range (EBU Tech 3342) is the spread
//! between the 10th and 95th percentile of the 3 s blocks above −70 LUFS and 20 LU under their
//! loudness. Both keep their blocks in 0.01 LU histograms, so memory and work per step are fixed
//! however long the song is. They count while the song plays and start over when it starts, as
//! a loudness meter on the master should (Youlean's "auto reset").
//!
//! **True peak.** Each channel is up-sampled 4× by a 48-tap windowed-sinc interpolator (12 taps a
//! phase, BS.1770 Annex 2 describes the same structure) and the largest absolute value is kept:
//! the peak a converter or a lossy encoder will meet between the samples.

use crate::processor::{ParamBlock, ProcessCtx, Processor, METERS};

use super::stereo::StereoStats;

/// −0.691 dB: makes a 997 Hz sine at 0 dBFS read −3.01 LUFS per channel.
const OFFSET: f64 = -0.691;
/// Absolute gate, LUFS.
const ABS_GATE: f64 = -70.0;
/// Histogram: 0.01 LU bins from −70 to +10 LUFS.
const BIN_LU: f64 = 0.01;
const BINS: usize = 8000;
/// 100 ms steps kept (short-term = 30).
const STEPS: usize = 30;
const OS: usize = 4;
const TP_TAPS: usize = 12;
/// "Silence" reads as this many LUFS / dBTP in the meters.
pub const FLOOR: f32 = -200.0;

#[derive(Clone, Copy, Default)]
struct Bq {
    b: [f64; 3],
    a: [f64; 3],
    z: [f64; 2],
}

impl Bq {
    #[inline(always)]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[1] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[2] * y;
        y
    }
}

/// The two K-weighting stages for `sr`.
fn k_weighting(sr: f64) -> (Bq, Bq) {
    use std::f64::consts::PI;
    // stage 1: high shelf
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (PI * f0 / sr).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Bq {
        b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        a: [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    // stage 2: high-pass (numerator 1, −2, 1 as printed: the −0.691 offset is calibrated to it)
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (PI * f0 / sr).tan();
    let d = 1.0 + k / q + k * k;
    let hp = Bq { b: [1.0, -2.0, 1.0], a: [1.0, 2.0 * (k * k - 1.0) / d, (1.0 - k / q + k * k) / d], z: [0.0; 2] };
    (shelf, hp)
}

/// 4× interpolator, phase-major: `TP[p][k]` multiplies the sample `k` steps back for phase `p`.
fn tp_kernel() -> [[f32; TP_TAPS]; OS] {
    let n = OS * TP_TAPS;
    let centre = (n as f64 - 1.0) / 2.0;
    // Kaiser window, β = 7
    let bessel = |x: f64| {
        let (mut s, mut t) = (1.0, 1.0);
        for k in 1..30 {
            t *= (x / 2.0) / k as f64;
            s += t * t;
        }
        s
    };
    let beta = 7.0;
    let mut h = vec![0f64; n];
    for (i, v) in h.iter_mut().enumerate() {
        let x = (i as f64 - centre) / OS as f64;
        let sinc = if x.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
        let r = (i as f64 - centre) / centre;
        let w = bessel(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel(beta);
        *v = sinc * w;
    }
    let mut out = [[0f32; TP_TAPS]; OS];
    for (p, row) in out.iter_mut().enumerate() {
        // each phase passes DC at unity
        let s: f64 = (0..TP_TAPS).map(|k| h[p + OS * k]).sum();
        for (k, c) in row.iter_mut().enumerate() {
            *c = (h[p + OS * k] / s) as f32;
        }
    }
    out
}

fn lufs(power: f64) -> f64 {
    if power <= 1e-20 {
        f64::NEG_INFINITY
    } else {
        OFFSET + 10.0 * power.log10()
    }
}

fn bin_of(l: f64) -> Option<usize> {
    // NaN and −∞ fall below the gate too
    if l.is_nan() || l <= ABS_GATE {
        return None;
    }
    Some((((l - ABS_GATE) / BIN_LU) as usize).min(BINS - 1))
}

/// Gated loudness histogram (integrated loudness or loudness range).
struct Hist {
    count: Vec<u32>,
    total: u64,
}

impl Hist {
    fn new() -> Self {
        Self { count: vec![0; BINS], total: 0 }
    }
    fn add(&mut self, l: f64) {
        if let Some(b) = bin_of(l) {
            self.count[b] = self.count[b].saturating_add(1);
            self.total += 1;
        }
    }
    fn clear(&mut self) {
        self.count.fill(0);
        self.total = 0;
    }
    /// Mean power of the blocks from bin `from` up, as loudness.
    fn mean_from(&self, from: usize, power: &[f64]) -> Option<f64> {
        let (mut s, mut c) = (0.0, 0u64);
        for (&n, &pw) in self.count[from..].iter().zip(&power[from..]) {
            if n > 0 {
                s += pw * f64::from(n);
                c += u64::from(n);
            }
        }
        (c > 0).then(|| lufs(s / c as f64))
    }
    /// BS.1770 integrated loudness.
    fn integrated(&self, power: &[f64]) -> Option<f64> {
        let ungated = self.mean_from(0, power)?;
        let rel = bin_of(ungated - 10.0).unwrap_or(0);
        self.mean_from(rel, power)
    }
    /// EBU Tech 3342 loudness range.
    fn range(&self, power: &[f64]) -> Option<f64> {
        let ungated = self.mean_from(0, power)?;
        let from = bin_of(ungated - 20.0).unwrap_or(0);
        let n: u64 = self.count[from..].iter().map(|&c| u64::from(c)).sum();
        if n < 2 {
            return None;
        }
        let pct = |p: f64| {
            let want = ((n - 1) as f64 * p).round() as u64;
            let mut acc = 0u64;
            for (b, &c) in self.count.iter().enumerate().skip(from) {
                acc += u64::from(c);
                if acc > want {
                    return ABS_GATE + (b as f64 + 0.5) * BIN_LU;
                }
            }
            ABS_GATE + BINS as f64 * BIN_LU
        };
        Some(pct(0.95) - pct(0.10))
    }
}

pub struct Loudness {
    sr: f64,
    kw: [(Bq, Bq); 2],
    /// samples per 100 ms step
    step_len: usize,
    step_n: usize,
    step_acc: f64,
    /// mean power of the last steps (ring), and how many are valid
    steps: [f64; STEPS],
    head: usize,
    filled: usize,
    momentary: f64,
    short: f64,
    integrated: Option<f64>,
    range: Option<f64>,
    hist_i: Hist,
    hist_s: Hist,
    /// power of each histogram bin's centre
    bin_power: Vec<f64>,
    /// seconds measured since the song started
    measured: f64,
    // true peak
    kernel: [[f32; TP_TAPS]; OS],
    /// last TP_TAPS samples per channel, written twice so a window is one contiguous slice
    hist: [[f32; 2 * TP_TAPS]; 2],
    hpos: usize,
    tp_max: f32,
    /// largest true peak of the current and previous step (a short "now" reading)
    tp_step: f32,
    tp_now: f32,
    stats: StereoStats,
    k_meter: f32,
    was_playing: bool,
}

impl Default for Loudness {
    fn default() -> Self {
        Self {
            sr: 48_000.0,
            kw: [k_weighting(48_000.0); 2],
            step_len: 4800,
            step_n: 0,
            step_acc: 0.0,
            steps: [0.0; STEPS],
            head: 0,
            filled: 0,
            momentary: f64::NEG_INFINITY,
            short: f64::NEG_INFINITY,
            integrated: None,
            range: None,
            hist_i: Hist { count: vec![], total: 0 },
            hist_s: Hist { count: vec![], total: 0 },
            bin_power: vec![],
            measured: 0.0,
            kernel: [[0.0; TP_TAPS]; OS],
            hist: [[0.0; 2 * TP_TAPS]; 2],
            hpos: 0,
            tp_max: 0.0,
            tp_step: 0.0,
            tp_now: 0.0,
            stats: StereoStats::default(),
            k_meter: 0.0001,
            was_playing: false,
        }
    }
}

impl Loudness {
    /// Forget the song's measurement (integrated, range, peak hold).
    fn restart(&mut self) {
        self.hist_i.clear();
        self.hist_s.clear();
        self.integrated = None;
        self.range = None;
        self.measured = 0.0;
        self.tp_max = 0.0;
    }

    fn end_step(&mut self, counting: bool) {
        let p = self.step_acc / self.step_len as f64;
        self.step_acc = 0.0;
        self.step_n = 0;
        self.steps[self.head] = p;
        self.head = (self.head + 1) % STEPS;
        self.filled = (self.filled + 1).min(STEPS);
        let mean = |n: usize| -> f64 {
            let n = n.min(self.filled).max(1);
            (0..n).map(|i| self.steps[(self.head + STEPS - 1 - i) % STEPS]).sum::<f64>() / n as f64
        };
        self.momentary = if self.filled >= 4 { lufs(mean(4)) } else { f64::NEG_INFINITY };
        self.short = if self.filled >= STEPS { lufs(mean(STEPS)) } else { f64::NEG_INFINITY };
        self.tp_now = self.tp_step;
        self.tp_step = 0.0;
        if counting {
            self.measured += 0.1;
            if self.filled >= 4 {
                self.hist_i.add(self.momentary);
                self.integrated = self.hist_i.integrated(&self.bin_power);
            }
            if self.filled >= STEPS {
                self.hist_s.add(self.short);
                self.range = self.hist_s.range(&self.bin_power);
            }
        }
    }
}

impl Processor for Loudness {
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sr = f64::from(sample_rate.max(8000.0));
        self.kw = [k_weighting(self.sr); 2];
        self.step_len = (self.sr * 0.1).round() as usize;
        self.hist_i = Hist::new();
        self.hist_s = Hist::new();
        self.bin_power = (0..BINS).map(|b| 10f64.powf((ABS_GATE + (b as f64 + 0.5) * BIN_LU - OFFSET) / 10.0)).collect();
        self.kernel = tp_kernel();
        self.k_meter = 1.0 - (-1.0 / (0.3 * sample_rate.max(8000.0))).exp();
        self.reset();
        self.restart();
    }

    fn set_params(&mut self, _p: &ParamBlock) {}

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        if ctx.playing && !self.was_playing {
            self.restart();
        }
        self.was_playing = ctx.playing;
        let n = l.len().min(r.len());
        for i in 0..n {
            let x = [l[i], r[i]];
            let mut p = 0.0;
            for (c, &v) in x.iter().enumerate() {
                let (a, b) = &mut self.kw[c];
                let y = b.run(a.run(f64::from(v)));
                p += y * y;
            }
            self.step_acc += p;
            // true peak
            self.hpos = (self.hpos + 1) % TP_TAPS;
            for (c, &v) in x.iter().enumerate() {
                self.hist[c][self.hpos] = v;
                self.hist[c][self.hpos + TP_TAPS] = v;
                // newest first: k samples back is w[TP_TAPS - 1 - k]
                let w = &self.hist[c][self.hpos + 1..=self.hpos + TP_TAPS];
                for ph in &self.kernel {
                    let mut y = 0.0;
                    for (k, &cf) in ph.iter().enumerate() {
                        y += cf * w[TP_TAPS - 1 - k];
                    }
                    let a = y.abs();
                    if a > self.tp_step {
                        self.tp_step = a;
                    }
                }
            }
            self.stats.feed(x[0], x[1], self.k_meter);
            self.step_n += 1;
            if self.step_n >= self.step_len {
                if self.tp_step > self.tp_max && ctx.playing {
                    self.tp_max = self.tp_step;
                }
                self.end_step(ctx.playing);
            }
        }
        for k in &mut self.kw {
            for z in k.0.z.iter_mut().chain(k.1.z.iter_mut()) {
                if z.abs() < 1e-30 {
                    *z = 0.0;
                }
            }
        }
    }

    /// A transport jump or bypass forgets the filters and the current step, not the song's
    /// measurement (that restarts when playback starts).
    fn reset(&mut self) {
        for k in &mut self.kw {
            k.0.z = [0.0; 2];
            k.1.z = [0.0; 2];
        }
        self.step_n = 0;
        self.step_acc = 0.0;
        self.steps = [0.0; STEPS];
        self.head = 0;
        self.filled = 0;
        self.momentary = f64::NEG_INFINITY;
        self.short = f64::NEG_INFINITY;
        self.hist = [[0.0; 2 * TP_TAPS]; 2];
        self.tp_step = 0.0;
        self.tp_now = 0.0;
        self.stats.clear();
    }

    fn tail_samples(&self) -> Option<usize> {
        Some(0)
    }

    fn meters(&self, out: &mut [f32; METERS]) {
        let f = |v: f64| if v.is_finite() { v as f32 } else { FLOOR };
        let db = |v: f32| if v > 1e-10 { 20.0 * v.log10() } else { FLOOR };
        out[0] = f(self.momentary);
        out[1] = f(self.short);
        out[2] = self.integrated.map_or(FLOOR, |v| v as f32);
        out[3] = self.range.map_or(-1.0, |v| v as f32);
        out[4] = db(self.tp_max);
        out[5] = db(self.tp_now);
        out[6] = self.stats.correlation();
        out[7] = self.stats.balance();
        out[8] = self.measured as f32;
        out[9] = self.stats.side_to_mid_db();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(m: &mut Loudness, sr: f32, secs: f32, mut gen: impl FnMut(usize) -> (f32, f32)) {
        let ctx = ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: true };
        let n = (sr * secs) as usize;
        let mut i = 0;
        while i < n {
            let len = 256.min(n - i);
            let (mut l, mut r) = (vec![0f32; len], vec![0f32; len]);
            for j in 0..len {
                let (a, b) = gen(i + j);
                l[j] = a;
                r[j] = b;
            }
            m.process(&mut l, &mut r, &ctx);
            i += len;
        }
    }

    fn sine(sr: f32, hz: f32, amp: f32) -> impl FnMut(usize) -> f32 {
        move |i| amp * (std::f32::consts::TAU * hz * i as f32 / sr).sin()
    }

    fn meters(m: &Loudness) -> [f32; METERS] {
        let mut o = [0.0; METERS];
        m.meters(&mut o);
        o
    }

    #[test]
    fn calibration_997hz_reads_minus_3_per_channel_at_every_rate() {
        for sr in [44_100.0, 48_000.0, 96_000.0] {
            let mut m = Loudness::default();
            m.prepare(sr, 512);
            let mut s = sine(sr, 997.0, 1.0);
            // one channel only: −3.01 LUFS
            play(&mut m, sr, 5.0, |i| (s(i), 0.0));
            let o = meters(&m);
            assert!((o[2] + 3.01).abs() < 0.05, "{sr}: integrated {}", o[2]);
            assert!((o[1] + 3.01).abs() < 0.05, "{sr}: short-term {}", o[1]);
        }
    }

    #[test]
    fn ebu_3341_case_1_and_2() {
        // stereo 1 kHz sine at −23 dBFS (and −33): −23 (−33) LUFS ±0.1
        let sr = 48_000.0;
        for (dbfs, want) in [(-23.0f32, -23.0f32), (-33.0, -33.0)] {
            let mut m = Loudness::default();
            m.prepare(sr, 512);
            let amp = 10f32.powf(dbfs / 20.0);
            let mut s = sine(sr, 1000.0, amp);
            play(&mut m, sr, 20.0, |i| {
                let v = s(i);
                (v, v)
            });
            let o = meters(&m);
            assert!((o[2] - want).abs() < 0.1, "integrated {} want {want}", o[2]);
        }
    }

    #[test]
    fn gating_ignores_silence_and_the_quiet_part() {
        // EBU 3341 case 3-like: −36 / −23 / −36 dBFS stretches read −23 LUFS integrated
        let sr = 48_000.0;
        let mut m = Loudness::default();
        m.prepare(sr, 512);
        let lo = 10f32.powf(-36.0 / 20.0);
        let hi = 10f32.powf(-23.0 / 20.0);
        let mut s = sine(sr, 1000.0, 1.0);
        let n10 = (sr * 10.0) as usize;
        let n60 = (sr * 60.0) as usize;
        play(&mut m, sr, 80.0, |i| {
            let a = if i < n10 {
                lo
            } else if i < n10 + n60 {
                hi
            } else {
                lo
            };
            let v = s(i) * a;
            (v, v)
        });
        let o = meters(&m);
        assert!((o[2] + 23.0).abs() < 0.1, "integrated {}", o[2]);
    }

    #[test]
    fn loudness_range_of_two_levels() {
        // EBU 3342 case 1: 20 s at −20 dBFS then 20 s at −30 dBFS → LRA 10 LU ±1
        let sr = 48_000.0;
        let mut m = Loudness::default();
        m.prepare(sr, 512);
        let mut s = sine(sr, 1000.0, 1.0);
        let n20 = (sr * 20.0) as usize;
        play(&mut m, sr, 40.0, |i| {
            let a = if i < n20 { 10f32.powf(-1.0) } else { 10f32.powf(-1.5) };
            let v = s(i) * a;
            (v, v)
        });
        let o = meters(&m);
        assert!((o[3] - 10.0).abs() < 1.0, "LRA {}", o[3]);
    }

    #[test]
    fn true_peak_finds_the_peak_between_samples() {
        // fs/4 sine at 45° phase: every sample is at 0.707, the true peak is 1.0 (0 dBTP)
        let sr = 48_000.0;
        let mut m = Loudness::default();
        m.prepare(sr, 512);
        play(&mut m, sr, 1.0, |i| {
            let v = (std::f32::consts::FRAC_PI_2 * i as f32 + std::f32::consts::FRAC_PI_4).sin();
            (v, v)
        });
        let o = meters(&m);
        assert!(o[4] > -0.4 && o[4] < 0.2, "true peak {} dBTP", o[4]);
    }

    #[test]
    fn restarts_when_playback_starts_and_keeps_through_a_reset() {
        let sr = 48_000.0;
        let mut m = Loudness::default();
        m.prepare(sr, 512);
        let mut s = sine(sr, 1000.0, 0.1);
        play(&mut m, sr, 4.0, |i| {
            let v = s(i);
            (v, v)
        });
        let before = meters(&m)[2];
        assert!(before > -40.0);
        m.reset();
        assert_eq!(meters(&m)[2], before);
        let stop = ProcessCtx { sample_rate: sr, bpm: 120.0, beat: 0.0, playing: false };
        m.process(&mut [0.0; 64], &mut [0.0; 64], &stop);
        play(&mut m, sr, 0.2, |_| (0.0, 0.0));
        assert_eq!(meters(&m)[2], FLOOR);
    }
}
