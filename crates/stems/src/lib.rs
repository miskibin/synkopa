//! Stem separation: a stereo mix in, one stereo track per source out (drums, bass, other and
//! vocals for HTDemucs; also guitar and piano for the 6-source model).
//!
//! The network is Demucs v4 (HTDemucs, Rouard, Massa and Défossez 2023, MIT), exported by
//! `scripts/export-htdemucs-onnx.py` with the STFT and iSTFT left out of the graph, because ONNX
//! has no complex tensors. This crate does the rest exactly as Demucs does:
//!
//! * `stft`/`istft` mirror `HTDemucs._spec`/`_ispec` (n_fft 4096, hop 1024, periodic Hann,
//!   `normalized=True`, reflect padding, the Nyquist bin dropped and two frames trimmed each side);
//! * [`Separator::separate`] mirrors `demucs.apply.apply_model` with `shifts=0, overlap=0.25`:
//!   7.8 s segments (the length the transformer was trained on) every 5.85 s, cross-faded with
//!   Demucs' triangular weights, the last one centred and zero padded; and `demucs.separate`'s
//!   normalisation of the whole mix by the mean and standard deviation of its mono sum.
//!
//! Memory stays bounded by one segment per source whatever the length of the input: each finished
//! stretch is handed to the caller's sink as soon as no later segment can touch it.

use std::path::Path;

use parking_lot::Mutex;
use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

/// The rate every HTDemucs model runs at. Other rates are resampled with [`resample`].
pub const SR: u32 = 44100;

/// The model is not shipped with the installer: the app downloads it the first time stems are
/// extracted. It is `scripts/export-htdemucs-onnx.py htdemucs` (Demucs 4.0.1) with its weights
/// stored as fp16 (`--fp16-weights`; the graph still computes in fp32), published as a
/// pre-release asset under its own tag so the app updater never sees it.
pub const MODEL_ID: &str = "htdemucs";
pub const MODEL_NAME: &str = "HTDemucs stem separation model";
pub const MODEL_URL: &str = "https://github.com/miskibin/synkopa/releases/download/models-htdemucs-v1/htdemucs.onnx";
pub const MODEL_SHA256: &str = "4d199bd7f79b5c4a509871a05ec82e2b32a28148db31b811b7453896ce808660";
pub const MODEL_BYTES: u64 = 90_358_774;
pub const LICENSE_URL: &str = "https://github.com/facebookresearch/demucs/blob/main/LICENSE";
pub const LICENSE_NOTE: &str = "Demucs v4 (HTDemucs) by Meta AI Research: code and pretrained weights under the MIT licence.";
/// The stems [`Separator::sources`] lists for this model, in its order.
pub const STEMS: [&str; 4] = ["drums", "bass", "other", "vocals"];

/// Where the downloaded model lives (`WAVE_STEMS_MODEL` overrides it): `<data>/Models/htdemucs.onnx`.
pub fn model_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("WAVE_STEMS_MODEL") {
        return p.into();
    }
    wave_rmvpe::models_dir().join("htdemucs.onnx")
}

/// ONNX Runtime threads for a separation: leave two logical cores for the audio engine and the
/// UI (the app keeps playing while it separates), at most 8 (more gained nothing measurable).
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get().saturating_sub(2).clamp(2, 8)).unwrap_or(4)
}
const NFFT: usize = 4096;
const HOP: usize = NFFT / 4;
const BINS: usize = NFFT / 2;
/// `HTDemucs._spec`'s extra padding, which keeps `frames == ceil(len / HOP)`.
const PAD: usize = HOP / 2 * 3;
const OVERLAP: f64 = 0.25;

#[derive(Debug)]
pub enum Error {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Cancelled => f.write_str("stem separation cancelled"),
            Error::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Error {}

fn fail(e: impl std::fmt::Display) -> Error {
    Error::Failed(e.to_string())
}

pub struct Separator {
    session: Mutex<ort::session::Session>,
    _env: ort::environment::Environment,
    sources: Vec<String>,
    segment: usize,
}

impl Separator {
    /// Load an export of `scripts/export-htdemucs-onnx.py`. `threads` = 0 picks one per core (≤ 8).
    pub fn load(model: &Path, threads: usize) -> Result<Self, String> {
        let fail = |e: String| format!("stem model could not be loaded: {e}");
        let env = wave_rmvpe::ort_init().map_err(fail)?.build().map_err(|e| fail(e.to_string()))?;
        let threads = match threads {
            0 => std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4),
            n => n,
        };
        let session = ort::session::Session::builder(&env)
            .map_err(|e| fail(e.to_string()))?
            .with_intra_threads(threads)
            .map_err(|e| fail(e.to_string()))?
            // The memory pattern pre-plans one buffer per activation of the whole graph: on a
            // 4-minute song the peak working set was 5.0 GB with it and 2.7 GB without, at the
            // same speed. (Disabling the CPU arena as well saved little more and cost ~18 %.)
            .with_memory_pattern(false)
            .map_err(|e| fail(e.to_string()))?
            .commit_from_file(model)
            .map_err(|e| fail(e.to_string()))?;
        let meta = session.metadata().map_err(|e| fail(e.to_string()))?;
        let get = |k: &str| {
            meta.custom(k)
                .ok_or_else(|| fail(format!("the model has no `{k}` metadata (not an export of scripts/export-htdemucs-onnx.py?)")))
        };
        let sources: Vec<String> = get("sources")?.split(',').map(str::to_owned).collect();
        let segment: usize = get("segment")?.parse().map_err(|_| fail("bad `segment` metadata".into()))?;
        if get("samplerate")? != SR.to_string() || get("nfft")? != NFFT.to_string() || get("hop")? != HOP.to_string() {
            return Err(fail("the model's sample rate or STFT differ from what this build supports".into()));
        }
        drop(meta);
        Ok(Self { session: Mutex::new(session), _env: env, sources, segment })
    }

    /// The stems this model writes, in the order [`Separator::separate`] hands them to the sink.
    pub fn sources(&self) -> &[String] {
        &self.sources
    }

    /// Separate a stereo mix at [`SR`] (`left` and `right` of equal length).
    ///
    /// `progress` gets the fraction done before each segment and 1.0 at the end; returning `false`
    /// cancels. `sink` gets, in order and without gaps, the next finished frames of every source
    /// (`stems[s]` interleaved stereo, all the same length); an error from it stops the job.
    pub fn separate(
        &self,
        left: &[f32],
        right: &[f32],
        mut progress: impl FnMut(f32) -> bool,
        mut sink: impl FnMut(&[Vec<f32>]) -> Result<(), String>,
    ) -> Result<(), Error> {
        assert_eq!(left.len(), right.len());
        let n = left.len();
        let seg = self.segment;
        let ns = self.sources.len();
        if n == 0 {
            return Ok(());
        }
        // demucs.separate: normalise by the mono mix's mean and (unbiased) standard deviation.
        let mono = |i: usize| 0.5 * (left[i] as f64 + right[i] as f64);
        let mean = (0..n).map(mono).sum::<f64>() / n as f64;
        let var = (0..n).map(|i| (mono(i) - mean).powi(2)).sum::<f64>() / (n.max(2) - 1) as f64;
        let std = var.sqrt().max(1e-8);
        let norm = |x: &[f32], i: isize| -> f32 {
            if i < 0 || i as usize >= n {
                0.0
            } else {
                ((x[i as usize] as f64 - mean) / std) as f32
            }
        };

        let stride = ((1.0 - OVERLAP) * seg as f64) as usize;
        let half = seg / 2;
        let weight: Vec<f32> = (0..seg).map(|i| if i < half { i + 1 } else { seg - i } as f32 / half as f32).collect();
        let chunks = n.div_ceil(stride);

        let t = seg.div_ceil(HOP);
        let mut stft = Stft::new();
        let mut mix = vec![0f32; 2 * seg];
        let mut spec = vec![0f32; 4 * BINS * t];
        let mut wave = vec![0f32; seg];
        // Overlap-add accumulators, one segment long, starting at absolute frame `offset`.
        let mut acc = vec![vec![0f32; 2 * seg]; ns];
        let mut wsum = vec![0f32; seg];
        let mut out: Vec<Vec<f32>> = vec![Vec::with_capacity(2 * stride); ns];

        for (k, offset) in (0..n).step_by(stride).enumerate() {
            if !progress(k as f32 / chunks as f32) {
                return Err(Error::Cancelled);
            }
            // demucs.apply.TensorChunk.padded: a short last chunk is centred in the segment.
            let len = seg.min(n - offset);
            let lead = (seg - len) / 2;
            let start = offset as isize - lead as isize;
            for i in 0..seg {
                mix[i] = norm(left, start + i as isize);
                mix[seg + i] = norm(right, start + i as isize);
            }
            for c in 0..2 {
                let (re, im) = spec[c * 2 * BINS * t..(c + 1) * 2 * BINS * t].split_at_mut(BINS * t);
                stft.forward(&mix[c * seg..(c + 1) * seg], re, im);
            }
            let input = ort::value::Tensor::from_array(([1usize, 2, seg], mix.clone())).map_err(fail)?;
            let spec_in = ort::value::Tensor::from_array(([1usize, 4, BINS, t], spec.clone())).map_err(fail)?;
            let mut sess = self.session.lock();
            let res = sess.run(ort::inputs![input, spec_in]).map_err(|e| fail(format!("stem model failed: {e}")))?;
            let (_, zs) = res[0].try_extract_tensor::<f32>().map_err(fail)?;
            let (_, xt) = res[1].try_extract_tensor::<f32>().map_err(fail)?;
            for (s, acc) in acc.iter_mut().enumerate() {
                for c in 0..2 {
                    let z = &zs[(s * 4 + c * 2) * BINS * t..(s * 4 + c * 2 + 2) * BINS * t];
                    let (re, im) = z.split_at(BINS * t);
                    stft.inverse(re, im, &mut wave);
                    let xt = &xt[(s * 2 + c) * seg..(s * 2 + c + 1) * seg];
                    for i in 0..len {
                        let j = lead + i;
                        acc[2 * i + c] += weight[i] * (wave[j] + xt[j]);
                    }
                }
            }
            drop(res);
            drop(sess);
            for (w, wt) in wsum[..len].iter_mut().zip(&weight) {
                *w += wt;
            }
            // Frames before the next chunk's offset are final.
            let done = if offset + stride < n { stride } else { len };
            for (s, acc) in acc.iter_mut().enumerate() {
                out[s].clear();
                out[s].extend((0..2 * done).map(|i| (acc[i] / wsum[i / 2]) as f64 * std + mean).map(|v| v as f32));
                acc.copy_within(2 * done.., 0);
                acc[2 * (seg - done)..].fill(0.0);
            }
            wsum.copy_within(done.., 0);
            wsum[seg - done..].fill(0.0);
            sink(&out).map_err(Error::Failed)?;
        }
        progress(1.0);
        Ok(())
    }
}

/// `HTDemucs._spec`/`_ispec` for one channel of one segment.
struct Stft {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    window: Vec<f32>,
    frame: Vec<f32>,
    bins: Vec<Complex32>,
    padded: Vec<f32>,
    ola: Vec<f32>,
    env: Vec<f32>,
}

/// `x[i]` with numpy/torch "reflect" padding (the edge sample is not repeated).
fn reflect(x: &[f32], i: isize) -> f32 {
    let n = x.len() as isize;
    let mut i = i;
    if i < 0 {
        i = -i;
    }
    if i >= n {
        i = 2 * (n - 1) - i;
    }
    x[i.clamp(0, n - 1) as usize]
}

impl Stft {
    fn new() -> Self {
        let mut p = RealFftPlanner::<f32>::new();
        let fwd = p.plan_fft_forward(NFFT);
        let inv = p.plan_fft_inverse(NFFT);
        // torch.hann_window (periodic)
        let window = (0..NFFT).map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / NFFT as f64).cos()) as f32).collect();
        Self { bins: fwd.make_output_vec(), frame: vec![0.0; NFFT], fwd, inv, window, padded: vec![], ola: vec![], env: vec![] }
    }

    /// `x` (one channel) to `re`/`im`, each `[BINS][frames]` with `frames = ceil(len / HOP)`.
    fn forward(&mut self, x: &[f32], re: &mut [f32], im: &mut [f32]) {
        let t = x.len().div_ceil(HOP);
        // _spec's reflect pad (PAD left, PAD + rounding right), then torch.stft's centre pad.
        let p1_len = t * HOP + 2 * PAD;
        let p1: Vec<f32> = (0..p1_len).map(|i| reflect(x, i as isize - PAD as isize)).collect();
        let c = NFFT / 2;
        self.padded.clear();
        self.padded.extend((0..p1_len + 2 * c).map(|i| reflect(&p1, i as isize - c as isize)));
        let scale = 1.0 / (NFFT as f32).sqrt();
        for k in 0..t {
            // torch.stft frame k + 2: _spec drops the first two.
            let s = (k + 2) * HOP;
            for (f, (x, w)) in self.frame.iter_mut().zip(self.padded[s..s + NFFT].iter().zip(&self.window)) {
                *f = x * w;
            }
            self.fwd.process(&mut self.frame, &mut self.bins).expect("fft sizes");
            for b in 0..BINS {
                re[b * t + k] = self.bins[b].re * scale;
                im[b * t + k] = self.bins[b].im * scale;
            }
        }
    }

    /// `re`/`im` (`[BINS][frames]`) back to `out.len()` samples.
    fn inverse(&mut self, re: &[f32], im: &[f32], out: &mut [f32]) {
        let t = re.len() / BINS;
        let frames = t + 4; // _ispec pads two empty frames each side
        let len = NFFT + (frames - 1) * HOP;
        self.ola.clear();
        self.ola.resize(len, 0.0);
        self.env.clear();
        self.env.resize(len, 0.0);
        // torch.istft(normalized=True) multiplies by sqrt(n_fft); realfft's inverse is unscaled.
        let scale = (NFFT as f32).sqrt() / NFFT as f32;
        for k in 0..frames {
            let s = k * HOP;
            for (e, w) in self.env[s..s + NFFT].iter_mut().zip(&self.window) {
                *e += w * w;
            }
            if k < 2 || k >= t + 2 {
                continue;
            }
            for b in 0..BINS {
                self.bins[b] = Complex32::new(re[b * t + k - 2], im[b * t + k - 2]);
            }
            self.bins[0].im = 0.0;
            self.bins[BINS] = Complex32::new(0.0, 0.0);
            self.inv.process(&mut self.bins, &mut self.frame).expect("fft sizes");
            for (i, (y, w)) in self.frame.iter().zip(&self.window).enumerate() {
                self.ola[s + i] += y * scale * w;
            }
        }
        // torch.istft trims n_fft/2 (centre); _ispec then drops PAD.
        let o = NFFT / 2 + PAD;
        for (i, v) in out.iter_mut().enumerate() {
            let e = self.env[o + i];
            *v = if e > 1e-11 { self.ola[o + i] / e } else { 0.0 };
        }
    }
}

/// Windowed-sinc resampling of one channel (offline; for inputs not at [`SR`]).
///
/// The kernel (sinc × Blackman over ±32 input samples) is tabulated at 1/512 of a sample and
/// read with linear interpolation instead of evaluating a sine and two cosines per tap.
pub fn resample(x: &[f32], from: f64, to: f64) -> Vec<f32> {
    if (from - to).abs() < 1e-9 || x.is_empty() {
        return x.to_vec();
    }
    const HALF: isize = 32;
    const STEPS: usize = 512;
    let ratio = to / from;
    let cutoff = ratio.min(1.0) * 0.97;
    let kernel = |d: f64| {
        let arg = std::f64::consts::PI * d * cutoff;
        let sinc = if arg.abs() < 1e-12 { 1.0 } else { arg.sin() / arg };
        let u = (d / HALF as f64 + 1.0) * 0.5;
        if (0.0..=1.0).contains(&u) {
            sinc * (0.42 - 0.5 * (2.0 * std::f64::consts::PI * u).cos() + 0.08 * (4.0 * std::f64::consts::PI * u).cos())
        } else {
            0.0
        }
    };
    // table[k] = kernel(k / STEPS - HALF), k = 0 ..= 2·HALF·STEPS (+1 guard for the interpolation)
    let len = 2 * HALF as usize * STEPS + 2;
    let table: Vec<f64> = (0..len).map(|k| kernel(k as f64 / STEPS as f64 - HALF as f64)).collect();
    let n_out = (x.len() as f64 * ratio).round() as usize;
    let mut y = Vec::with_capacity(n_out);
    for j in 0..n_out {
        let pos = j as f64 / ratio;
        let c = pos.floor() as isize;
        let (mut acc, mut wsum) = (0.0f64, 0.0f64);
        for i in c - HALF + 1..=c + HALF {
            let t = (pos - i as f64 + HALF as f64) * STEPS as f64;
            let k0 = t as usize;
            let f = t - k0 as f64;
            let k = table[k0] + (table[k0 + 1] - table[k0]) * f;
            wsum += k;
            if i >= 0 && (i as usize) < x.len() {
                acc += k * x[i as usize] as f64;
            }
        }
        y.push((acc / wsum.max(1e-12)) as f32);
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stft_round_trip_is_identity() {
        let n: usize = 343980;
        let x: Vec<f32> = (0..n).map(|i| ((i as f32 * 0.0123).sin() + (i as f32 * 0.31).cos() * 0.3) * 0.5).collect();
        let t = n.div_ceil(HOP);
        let (mut re, mut im) = (vec![0.0; BINS * t], vec![0.0; BINS * t]);
        let mut s = Stft::new();
        s.forward(&x, &mut re, &mut im);
        let mut y = vec![0.0; n];
        s.inverse(&re, &im, &mut y);
        // Demucs drops the first and last two STFT frames, so the outer PAD samples do not come
        // back (the network learnt that); everything between must.
        let err = x[PAD..n - PAD].iter().zip(&y[PAD..n - PAD]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-4, "max error {err}");
    }

    /// Needs an export: `WAVE_STEMS_MODEL=htdemucs.onnx cargo test -p wave-stems --release -- --ignored`.
    #[test]
    #[ignore]
    fn streams_every_frame_once_and_cancels() {
        let Some(model) = std::env::var_os("WAVE_STEMS_MODEL") else { return };
        let sep = Separator::load(Path::new(&model), 0).unwrap();
        // 20 s: three segments, so the overlap-add and the short centred last chunk both run.
        let n = 20 * SR as usize;
        let l: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let r = l.clone();
        let mut got = vec![0usize; sep.sources().len()];
        let mut calls = 0;
        sep.separate(
            &l,
            &r,
            |_| true,
            |s| {
                calls += 1;
                for (g, s) in got.iter_mut().zip(s) {
                    assert!(s.iter().all(|v| v.is_finite()));
                    *g += s.len() / 2;
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(calls, 4);
        assert!(got.iter().all(|&g| g == n), "{got:?}");

        let mut seen = 0;
        let res = sep.separate(
            &l,
            &r,
            |f| {
                seen += 1;
                f == 0.0
            },
            |_| Ok(()),
        );
        assert!(matches!(res, Err(Error::Cancelled)));
        assert_eq!(seen, 2);
    }

    #[test]
    fn the_model_download_is_pinned() {
        const { assert!(MODEL_SHA256.len() == 64 && MODEL_BYTES > 50_000_000 && MODEL_BYTES < 200_000_000) };
        assert!(MODEL_SHA256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        // a pre-release tag of its own: never `releases/latest`, which the app updater reads
        assert!(MODEL_URL.starts_with("https://github.com/miskibin/synkopa/releases/download/models-htdemucs-v"), "{MODEL_URL}");
        assert!((2..=8).contains(&default_threads()));
    }

    #[test]
    fn reflect_matches_numpy() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let got: Vec<f32> = (-3..7).map(|i| reflect(&x, i)).collect();
        assert_eq!(got, [4.0, 3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0]);
    }

    #[test]
    fn resample_keeps_a_tone() {
        let x: Vec<f32> = (0..48000).map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin()).collect();
        let y = resample(&x, 48000.0, 44100.0);
        assert_eq!(y.len(), 44100);
        let err =
            (1000..43000).map(|i| (y[i] - (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 44100.0).sin()).abs()).fold(0.0, f32::max);
        assert!(err < 2e-3, "max error {err}");
    }
}
