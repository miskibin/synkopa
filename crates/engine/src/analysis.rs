//! Spectra and scopes for open effect windows. Runs on the housekeeping thread,
//! only for watched effects, ~30 times a second.
//!
//! The input tap is mono; the output tap is stereo (interleaved L, R), so effects that show the
//! stereo field (Stereo, Analyzer) get the last output samples as X/Y points for a vectorscope.
//! The Analyzer also gets its bass bands from a 4× longer FFT: at 48 kHz a 2048-point FFT has
//! 23 Hz bins, too coarse to tell a 41 Hz kick from a 49 Hz 808.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

use crate::model::{FxKind, Id};
use crate::rt::Shared;

pub const BANDS: usize = 128;
pub const SCOPE: usize = 256;
/// X/Y points sent for a vectorscope (the newest output samples, L and R).
pub const XY: usize = 1024;
const FFT: usize = 2048;
/// Long FFT for the bass bands of analysers.
const FFT_LONG: usize = 8192;
/// Bands below this use the long FFT (analysers only).
const LONG_BELOW_HZ: f32 = 300.0;

pub struct FxAnalysis {
    pub id: Id,
    pub in_peak: f32,
    pub out_peak: f32,
    pub gr: f32,
    pub meters: [f32; crate::processor::METERS],
    pub spec_in: [f32; BANDS],
    pub spec_out: [f32; BANDS],
    pub scope_in: [f32; SCOPE],
    pub scope_out: [f32; SCOPE],
    /// Interleaved L, R of the newest output samples, oldest first; empty for effects that do
    /// not draw the stereo field.
    pub xy: Vec<f32>,
}

/// What an effect's window draws, beyond the common spectra and scopes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Detail {
    /// send X/Y points
    pub xy: bool,
    /// bass bands from the long FFT
    pub long_fft: bool,
}

impl Detail {
    pub fn of(kind: Option<FxKind>) -> Self {
        match kind {
            Some(FxKind::Analyzer) => Self { xy: true, long_fft: true },
            Some(FxKind::Stereo) => Self { xy: true, long_fft: false },
            _ => Self::default(),
        }
    }
}

struct History {
    /// mono (L+R)/2
    buf: Vec<f32>,
    w: usize,
    /// stereo output only: interleaved L, R
    xy: Vec<f32>,
    xw: usize,
    peak: f32,
    /// smoothed spectrum (dB)
    smooth: [f32; BANDS],
}

impl History {
    fn new() -> Self {
        Self { buf: vec![0.0; FFT_LONG], w: 0, xy: vec![0.0; 2 * XY], xw: 0, peak: 0.0, smooth: [-120.0; BANDS] }
    }
    fn push(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w = (self.w + 1) % FFT_LONG;
        self.peak = self.peak.max(x.abs());
    }
    fn feed_mono(&mut self, c: &mut rtrb::Consumer<f32>) {
        let n = c.slots();
        if n == 0 {
            return;
        }
        if let Ok(chunk) = c.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            for &x in a.iter().chain(b.iter()) {
                self.push(x);
            }
            chunk.commit_all();
        }
    }
    /// Interleaved stereo: the peak is the larger channel, the history the mid.
    fn feed_stereo(&mut self, c: &mut rtrb::Consumer<f32>) {
        let n = c.slots() & !1;
        if n == 0 {
            return;
        }
        if let Ok(chunk) = c.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            let mut it = a.iter().chain(b.iter());
            while let (Some(&l), Some(&r)) = (it.next(), it.next()) {
                self.buf[self.w] = 0.5 * (l + r);
                self.w = (self.w + 1) % FFT_LONG;
                self.peak = self.peak.max(l.abs()).max(r.abs());
                self.xy[2 * self.xw] = l;
                self.xy[2 * self.xw + 1] = r;
                self.xw = (self.xw + 1) % XY;
            }
            chunk.commit_all();
        }
    }
    fn last(&self, out: &mut [f32]) {
        let n = out.len();
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.buf[(self.w + FFT_LONG - n + i) % FFT_LONG];
        }
    }
    fn xy_points(&self) -> Vec<f32> {
        let mut v = Vec::with_capacity(2 * XY);
        for i in 0..XY {
            let j = (self.xw + i) % XY;
            v.push(self.xy[2 * j]);
            v.push(self.xy[2 * j + 1]);
        }
        v
    }
}

struct Fft {
    n: usize,
    plan: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    scratch: Vec<f32>,
    spec: Vec<realfft::num_complex::Complex<f32>>,
    /// FFT bin range per band.
    bands: Vec<(usize, usize)>,
    bin_hz: f32,
}

impl Fft {
    fn new(planner: &mut RealFftPlanner<f32>, n: usize, sr: f32) -> Self {
        let plan = planner.plan_fft_forward(n);
        let window = (0..n).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()).collect();
        let spec = plan.make_output_vec();
        let bin_hz = sr / n as f32;
        let bands = (0..BANDS)
            .map(|k| {
                let f0 = 20.0 * 1000f32.powf(k as f32 / BANDS as f32);
                let f1 = 20.0 * 1000f32.powf((k + 1) as f32 / BANDS as f32);
                let b0 = ((f0 / bin_hz).floor() as usize).clamp(1, n / 2);
                let b1 = ((f1 / bin_hz).ceil() as usize).clamp(b0, n / 2);
                (b0, b1)
            })
            .collect();
        Self { n, plan, window, scratch: vec![0.0; n], spec, bands, bin_hz }
    }

    /// Peak magnitude per band in dBFS (a full-scale sine reads 0).
    fn run(&mut self, h: &History, out: &mut [f32; BANDS], only_below: Option<f32>) {
        let start = (h.w + FFT_LONG - self.n) % FFT_LONG;
        for i in 0..self.n {
            self.scratch[i] = h.buf[(start + i) % FFT_LONG] * self.window[i];
        }
        let _ = self.plan.process(&mut self.scratch, &mut self.spec);
        let norm = 2.0 / (self.n as f32 * 0.5);
        for (k, &(b0, b1)) in self.bands.iter().enumerate() {
            if let Some(f) = only_below {
                if 20.0 * 1000f32.powf((k + 1) as f32 / BANDS as f32) > f {
                    break;
                }
            }
            let m = if b1 - b0 <= 1 {
                // a band narrower than a bin: read the spectrum at the band's centre, or every
                // band within a bin of a peak would show that peak at full height
                let fc = 20.0 * 1000f32.powf((k as f32 + 0.5) / BANDS as f32) / self.bin_hz;
                let i = (fc.floor() as usize).clamp(1, self.n / 2 - 1);
                let u = (fc - i as f32).clamp(0.0, 1.0);
                self.spec[i].norm() * (1.0 - u) + self.spec[i + 1].norm() * u
            } else {
                let mut m = 0f32;
                for bin in self.spec.iter().take(b1 + 1).skip(b0) {
                    m = m.max(bin.norm());
                }
                m
            };
            out[k] = 20.0 * (m * norm).max(1e-7).log10();
        }
    }
}

pub struct Analyzer {
    inputs: Vec<rtrb::Consumer<f32>>,
    outputs: Vec<rtrb::Consumer<f32>>,
    hin: Vec<History>,
    hout: Vec<History>,
    ids: Vec<Id>,
    short: Fft,
    long: Fft,
}

impl Analyzer {
    pub fn new(inputs: Vec<rtrb::Consumer<f32>>, outputs: Vec<rtrb::Consumer<f32>>, sr: f32) -> Self {
        let n = inputs.len();
        let mut planner = RealFftPlanner::<f32>::new();
        Self {
            inputs,
            outputs,
            hin: (0..n).map(|_| History::new()).collect(),
            hout: (0..n).map(|_| History::new()).collect(),
            ids: vec![],
            short: Fft::new(&mut planner, FFT, sr),
            long: Fft::new(&mut planner, FFT_LONG, sr),
        }
    }

    fn spectrum(&mut self, h: &History, smooth: &mut [f32; BANDS], long: bool) -> [f32; BANDS] {
        let mut raw = [-140f32; BANDS];
        self.short.run(h, &mut raw, None);
        if long {
            self.long.run(h, &mut raw, Some(LONG_BELOW_HZ));
        }
        let mut out = [0f32; BANDS];
        for k in 0..BANDS {
            let db = raw[k];
            // fast attack, slow-ish fall, like a real analyser
            smooth[k] = if db > smooth[k] { db } else { smooth[k] * 0.7 + db * 0.3 };
            out[k] = smooth[k];
        }
        out
    }

    /// `detail[i]` belongs to `watch[i]` (missing = the default).
    pub fn analyze(&mut self, watch: &[Id], detail: &[Detail], shared: &Shared) -> Vec<FxAnalysis> {
        if self.ids != watch {
            // slots were reassigned: forget history so windows never show another effect's audio
            for h in self.hin.iter_mut().chain(self.hout.iter_mut()) {
                *h = History::new();
            }
            self.ids = watch.to_vec();
        }
        let mut res = vec![];
        for (slot, &id) in watch.iter().enumerate().take(self.inputs.len()) {
            let d = detail.get(slot).copied().unwrap_or_default();
            self.hin[slot].feed_mono(&mut self.inputs[slot]);
            self.hout[slot].feed_stereo(&mut self.outputs[slot]);
            let hin = std::mem::replace(&mut self.hin[slot], History::new());
            let hout = std::mem::replace(&mut self.hout[slot], History::new());
            let mut si = hin.smooth;
            let mut so = hout.smooth;
            let spec_in = self.spectrum(&hin, &mut si, d.long_fft);
            let spec_out = self.spectrum(&hout, &mut so, d.long_fft);
            let mut scope_in = [0f32; SCOPE];
            let mut scope_out = [0f32; SCOPE];
            hin.last(&mut scope_in);
            hout.last(&mut scope_out);
            res.push(FxAnalysis {
                id,
                in_peak: hin.peak,
                out_peak: hout.peak,
                gr: shared.gr(slot),
                meters: shared.fx_meters(slot),
                spec_in,
                spec_out,
                scope_in,
                scope_out,
                xy: if d.xy { hout.xy_points() } else { Vec::new() },
            });
            self.hin[slot] = History { peak: 0.0, smooth: si, ..hin };
            self.hout[slot] = History { peak: 0.0, smooth: so, ..hout };
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_history(sr: f32, hz: f32, amp: f32) -> History {
        let mut h = History::new();
        for i in 0..FFT_LONG {
            h.push(amp * (std::f32::consts::TAU * hz * i as f32 / sr).sin());
        }
        h
    }

    #[test]
    fn long_fft_resolves_bass_notes_the_short_one_smears() {
        // a kick at 41 Hz against an 808 at 49 Hz (three semitones): the long FFT tells them
        // apart, the short one shows one smeared bump
        let sr = 48_000.0;
        let mut planner = RealFftPlanner::<f32>::new();
        let (mut short, mut long) = (Fft::new(&mut planner, FFT, sr), Fft::new(&mut planner, FFT_LONG, sr));
        let h = tone_history(sr, 41.2, 0.5);
        let band = |hz: f32| ((hz / 20.0).ln() / 1000f32.ln() * BANDS as f32) as usize;
        let (on, off) = (band(41.2), band(49.0));
        let mut a = [-140f32; BANDS];
        short.run(&h, &mut a, None);
        let mut b = a;
        long.run(&h, &mut b, Some(LONG_BELOW_HZ));
        assert!(b[on] - b[off] > 8.0, "long: {} vs {}", b[on], b[off]);
        assert!(a[on] - a[off] < 3.0, "short: {} vs {}", a[on], a[off]);
        // a full-scale-ish sine reads its level
        assert!((b[on] - 20.0 * 0.5f32.log10()).abs() < 1.5, "{}", b[on]);
    }

    #[test]
    fn stereo_feed_keeps_xy_in_order() {
        let (mut p, mut c) = rtrb::RingBuffer::<f32>::new(64);
        for i in 0..8 {
            p.push(i as f32).unwrap();
            p.push(-(i as f32)).unwrap();
        }
        let mut h = History::new();
        h.feed_stereo(&mut c);
        let xy = h.xy_points();
        assert_eq!(&xy[2 * XY - 4..], &[6.0, -6.0, 7.0, -7.0]);
        assert_eq!(h.peak, 7.0);
    }
}
