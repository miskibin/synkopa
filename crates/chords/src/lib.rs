//! Chord recognition: ChordMini (Phan et al., 2026; the "2E1D" ChordNet, ~2.3 M parameters,
//! MIT) through ONNX Runtime, as the engine's [`ChordModel`].
//!
//! The network reads log constant-Q frames and gives logits over 170 chords (12 roots × 14
//! qualities, "X" and "N"). Everything around it follows the musetric export it ships as
//! (`musetric/chordmini-onnx`), whose features must equal `librosa.cqt` to agree with the
//! reference:
//!
//! * mono at 22 050 Hz (the arithmetic mean of the channels: the model is gain-sensitive);
//! * librosa 0.11's recursive CQT (hop 2048, C1, 144 bins at 24 per octave, `scale=True`): one
//!   early 2:1 downsample, then per octave a 512-point FFT of each hop, times a sparse basis,
//!   halving the rate between octaves with a 255-tap Kaiser low-pass. The basis and the filter
//!   are the export's own `cqt-plan.bin` (MIT, baked from librosa; built into this crate), so
//!   the features are librosa's: 100 % frame agreement on a real song (see the tests);
//! * features `ln(|CQT| + 1e-6)` (the normalisation is inside the graph), 16 windows of 108
//!   frames per run (the graph is static at that shape), the last group zero-padded;
//! * a uniform 9-frame moving average of the logits, as the reference does before its argmax,
//!   then a softmax, folded into the engine's vocabulary ([`fold`]).

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use realfft::RealFftPlanner;
use wave_engine::chords::{class, ChordFrames, ChordModel, Quality, CLASSES, NO_CHORD};

/// The model is not shipped: the user downloads it on first use (see `app/src/chords_actions.rs`).
/// The URL names a fixed revision of the repository, so the file can never change under the hash.
pub const MODEL_ID: &str = "chordmini";
pub const MODEL_NAME: &str = "ChordMini chord recognition model";
pub const MODEL_URL: &str = "https://huggingface.co/musetric/chordmini-onnx/resolve/8ab85fc0f61439900cd07ac21257a43eac57075c/chordnet.onnx";
pub const MODEL_SHA256: &str = "1faee8e1cf168300afe0f47517e76836ac200c3841b0fb430fad7dfea5ad6394";
pub const MODEL_BYTES: u64 = 17_080_550;
pub const LICENSE_URL: &str = "https://github.com/ptnghia-j/ChordMini/blob/main/LICENSE";
pub const LICENSE_NOTE: &str = "ChordMini by Nghia Phan et al.: code and pretrained weights under the MIT licence \
(ONNX export by the musetric project, also MIT).";

/// Where the downloaded model lives (`WAVE_CHORDS_MODEL` overrides it): `<data>/Models/chordmini.onnx`.
pub fn model_path() -> PathBuf {
    if let Some(p) = std::env::var_os("WAVE_CHORDS_MODEL") {
        return p.into();
    }
    wave_rmvpe::models_dir().join("chordmini.onnx")
}

pub const SR: u32 = 22050;
pub const HOP: usize = 2048;
pub const BINS: usize = 144;
/// Frames per window and windows per run: the graph's static input shape.
const SEQ: usize = 108;
const BATCH: usize = 16;
const MODEL_CLASSES: usize = 170;
const SMOOTH: usize = 9;
/// ChordMini's qualities, in its vocabulary's order per root ("" is major), and the engine's
/// quality each one is heard as.
const MODEL_QUALITIES: [Quality; 14] = [
    Quality::Min,  // min
    Quality::Maj,  // maj
    Quality::Dim,  // dim
    Quality::Maj,  // aug: the major third and the root carry it
    Quality::Min,  // min6
    Quality::Maj,  // maj6
    Quality::Min7, // min7
    Quality::Min,  // minmaj7
    Quality::Maj7, // maj7
    Quality::Dom7, // 7
    Quality::Dim,  // dim7
    Quality::Dim,  // hdim7: the diminished triad (a m7 would put a perfect fifth against its b5)
    Quality::Sus2, // sus2
    Quality::Sus4, // sus4
];

/// Engine class of model class `c` (12 × 14 chords, then "X" = unknown and "N" = none).
pub fn fold(c: usize) -> usize {
    if c >= 12 * MODEL_QUALITIES.len() {
        return NO_CHORD;
    }
    class((c / MODEL_QUALITIES.len()) as u8, MODEL_QUALITIES[c % MODEL_QUALITIES.len()])
}

// ---------------------------------------------------------------- CQT

/// `cqt-plan.bin` from the musetric export (MIT): librosa 0.11's CQT basis and resampling
/// filter for exactly this configuration.
static PLAN_BYTES: &[u8] = include_bytes!("cqt-plan.bin");

struct Octave {
    hop: usize,
    fft: usize,
    bin_start: usize,
    bin_count: usize,
}

/// The CQT, parsed from the plan: per bin a sparse row of (FFT bin, complex weight).
pub struct Cqt {
    octaves: Vec<Octave>,
    rows: Vec<Vec<(usize, f32, f32)>>,
    /// The symmetric low-pass, full length.
    fir: Vec<f32>,
    gain: f32,
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, String> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "truncated CQT plan".to_string())
}
fn f32_at(b: &[u8], o: usize) -> Result<f32, String> {
    u32_at(b, o).map(f32::from_bits)
}
fn f64_at(b: &[u8], o: usize) -> Result<f64, String> {
    b.get(o..o + 8).map(|s| f64::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| "truncated CQT plan".to_string())
}

impl Cqt {
    /// The built-in plan.
    pub fn new() -> Self {
        Self::parse(PLAN_BYTES).expect("the built-in CQT plan parses")
    }

    /// Format 1 (little-endian): a 128-byte header (magic "MCQT", version, header size, early
    /// downsamples, sample rate f64 @16, hop, bins, bins per octave, output, fmin f64 @40,
    /// octaves, taps, coefficients, payload size, generator length, then the offsets of the
    /// generator, the octave table, the row pointers, the columns, the complex values, the
    /// filter lengths and the half filter) and the payload. Each octave record is 32 bytes:
    /// index, sample rate f64, hop, FFT size, first bin, bin count, reserved.
    fn parse(b: &[u8]) -> Result<Self, String> {
        if b.get(0..4) != Some(b"MCQT") || u32_at(b, 4)? != 1 {
            return Err("not a version 1 CQT plan".into());
        }
        let (sr, hop, bins, bpo) = (f64_at(b, 16)?, u32_at(b, 24)? as usize, u32_at(b, 28)? as usize, u32_at(b, 32)?);
        if sr != SR as f64 || hop != HOP || bins != BINS || bpo != 24 || u32_at(b, 12)? != 1 {
            return Err("the CQT plan is for another configuration".into());
        }
        let (n_oct, taps, nnz) = (u32_at(b, 48)? as usize, u32_at(b, 52)? as usize, u32_at(b, 56)? as usize);
        let off = |k: usize| u32_at(b, 0x44 + 4 * k).map(|x| x as usize);
        let (oct_at, rows_at, cols_at, vals_at, half_at) = (off(1)?, off(2)?, off(3)?, off(4)?, off(6)?);
        let mut octaves = vec![];
        for k in 0..n_oct {
            let o = oct_at + 32 * k;
            octaves.push(Octave {
                hop: u32_at(b, o + 12)? as usize,
                fft: u32_at(b, o + 16)? as usize,
                bin_start: u32_at(b, o + 20)? as usize,
                bin_count: u32_at(b, o + 24)? as usize,
            });
        }
        let mut rows = Vec::with_capacity(bins);
        for r in 0..bins {
            let (a, z) = (u32_at(b, rows_at + 4 * r)? as usize, u32_at(b, rows_at + 4 * (r + 1))? as usize);
            if a > z || z > nnz {
                return Err("bad CQT plan rows".into());
            }
            let mut row = vec![];
            for i in a..z {
                row.push((u32_at(b, cols_at + 4 * i)? as usize, f32_at(b, vals_at + 8 * i)?, f32_at(b, vals_at + 8 * i + 4)?));
            }
            rows.push(row);
        }
        let half_len = taps.div_ceil(2);
        let half: Vec<f32> = (0..half_len).map(|i| f32_at(b, half_at + 4 * i)).collect::<Result<_, _>>()?;
        let fir: Vec<f32> = half[1..].iter().rev().chain(half.iter()).copied().collect();
        Ok(Self { octaves, rows, fir, gain: std::f32::consts::SQRT_2 })
    }

    /// 2:1 decimation: the low-pass centred on each kept sample (zeros outside), times √2
    /// (librosa resamples with `scale=True`). Output length `ceil(n / 2)`.
    fn halve(&self, y: &[f32]) -> Vec<f32> {
        let d = (self.fir.len() / 2) as isize;
        let n = y.len() as isize;
        (0..y.len().div_ceil(2))
            .map(|j| {
                let c = 2 * j as isize;
                let (lo, hi) = ((c - d).max(0), (c + d).min(n - 1));
                let mut acc = 0f32;
                for i in lo..=hi {
                    acc += self.fir[(i - c + d) as usize] * y[i as usize];
                }
                acc * self.gain
            })
            .collect()
    }

    /// `ln(|CQT| + 1e-6)` of mono audio at [`SR`]: one row of [`BINS`] per hop (`1 + n / HOP` frames).
    pub fn log_frames(&self, x: &[f32]) -> Vec<[f32; BINS]> {
        let frames = 1 + x.len() / HOP;
        let mut out = vec![[0f32; BINS]; frames];
        let mut planner = RealFftPlanner::<f32>::new();
        let mut y = self.halve(x);
        for (k, oct) in self.octaves.iter().enumerate() {
            let fft = planner.plan_fft_forward(oct.fft);
            let (mut buf, mut spec, mut scratch) = (fft.make_input_vec(), fft.make_output_vec(), fft.make_scratch_vec());
            let half = oct.fft / 2;
            for (f, row) in out.iter_mut().enumerate() {
                // centred frames, zero padded
                let s = (f * oct.hop) as isize - half as isize;
                for (i, v) in buf.iter_mut().enumerate() {
                    let j = s + i as isize;
                    *v = if j >= 0 && (j as usize) < y.len() { y[j as usize] } else { 0.0 };
                }
                let _ = fft.process_with_scratch(&mut buf, &mut spec, &mut scratch);
                for r in 0..oct.bin_count {
                    let bin = oct.bin_start + r;
                    let (mut re, mut im) = (0f32, 0f32);
                    for &(c, wr, wi) in &self.rows[bin] {
                        let z = spec[c];
                        re += wr * z.re - wi * z.im;
                        im += wr * z.im + wi * z.re;
                    }
                    row[bin] = ((re * re + im * im).sqrt() + 1e-6).ln();
                }
            }
            if k + 1 < self.octaves.len() {
                y = self.halve(&y);
            }
        }
        out
    }
}

impl Default for Cqt {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------- model

pub struct ChordMini {
    session: Mutex<ort::session::Session>,
    _env: ort::environment::Environment,
    cqt: Cqt,
}

impl ChordMini {
    pub fn load(model: &Path) -> Result<Self, String> {
        let fail = |e: String| format!("The chord model could not be loaded: {e}");
        let env = wave_rmvpe::ort_env().map_err(fail)?;
        let threads = std::thread::available_parallelism().map(|n| n.get().saturating_sub(2).clamp(1, 4)).unwrap_or(2);
        let mut b = ort::session::Session::builder(&env)
            .map_err(|e| fail(e.to_string()))?
            .with_intra_threads(threads)
            .map_err(|e| fail(e.to_string()))?;
        let session = b.commit_from_file(model).map_err(|e| fail(e.to_string()))?;
        Ok(Self { session: Mutex::new(session), _env: env, cqt: Cqt::new() })
    }

    /// Raw logits (170 per frame) of log-CQT frames.
    pub fn logits(&self, feats: &[[f32; BINS]]) -> Result<Vec<f32>, String> {
        let n = feats.len();
        let mut out = vec![0f32; n * MODEL_CLASSES];
        let per_run = SEQ * BATCH;
        let mut s = 0;
        while s < n {
            let e = (s + per_run).min(n);
            let mut data = vec![0f32; per_run * BINS];
            for (i, f) in feats[s..e].iter().enumerate() {
                data[i * BINS..(i + 1) * BINS].copy_from_slice(f);
            }
            let input = ort::value::Tensor::from_array(([BATCH, SEQ, BINS], data)).map_err(|e| e.to_string())?;
            let mut sess = self.session.lock();
            let res = sess.run(ort::inputs!["features" => input]).map_err(|e| format!("chord inference failed: {e}"))?;
            let (_, l) = res[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            out[s * MODEL_CLASSES..e * MODEL_CLASSES].copy_from_slice(&l[..(e - s) * MODEL_CLASSES]);
            s = e;
        }
        Ok(out)
    }
}

/// The reference's smoothing (`np.convolve(x, ones(9) / 9, 'same')` per class), softmax, and the
/// fold to the engine's classes.
pub fn frames_from_logits(logits: &[f32]) -> Vec<[f32; CLASSES]> {
    let n = logits.len() / MODEL_CLASSES;
    let h = SMOOTH / 2;
    let mut out = Vec::with_capacity(n);
    let mut sm = [0f32; MODEL_CLASSES];
    for t in 0..n {
        sm.iter_mut().for_each(|v| *v = 0.0);
        for k in t.saturating_sub(h)..(t + h + 1).min(n) {
            for (c, v) in sm.iter_mut().enumerate() {
                *v += logits[k * MODEL_CLASSES + c];
            }
        }
        let mx = sm.iter().fold(f32::MIN, |m, v| m.max(*v / SMOOTH as f32));
        let mut p = [0f32; CLASSES];
        let mut z = 0f32;
        for (c, v) in sm.iter().enumerate() {
            let e = (v / SMOOTH as f32 - mx).exp();
            p[fold(c)] += e;
            z += e;
        }
        p.iter_mut().for_each(|v| *v /= z);
        out.push(p);
    }
    out
}

impl ChordModel for ChordMini {
    fn name(&self) -> &'static str {
        "chordmini"
    }

    fn analyze(&self, mono: &[f32], sr: u32) -> Result<ChordFrames, String> {
        let x = wave_stems::resample(mono, sr as f64, SR as f64);
        let feats = self.cqt.log_frames(&x);
        let logits = self.logits(&feats)?;
        Ok(ChordFrames { rate: SR as f64 / HOP as f64, probs: frames_from_logits(&logits) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn the_plan_is_the_published_one() {
        let h = Sha256::digest(PLAN_BYTES);
        let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "c31f0a6fd2d582d753be6628b5daecdee58acba53cba93b2bc2b5c75dee2ba48");
        let c = Cqt::new();
        assert_eq!(c.octaves.len(), 6);
        assert_eq!(c.fir.len(), 255);
        assert_eq!(c.rows.iter().map(Vec::len).sum::<usize>(), 1818);
        // a unity-gain low-pass
        let dc: f32 = c.fir.iter().sum();
        assert!((dc - 1.0).abs() < 1e-4, "{dc}");
    }

    #[test]
    fn a_tone_lights_its_own_bin() {
        let c = Cqt::new();
        // A4 = 440 Hz is 45 semitones above C1 → bin 90
        let x: Vec<f32> = (0..SR as usize * 3).map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / SR as f32).sin() * 0.5).collect();
        let f = c.log_frames(&x);
        assert_eq!(f.len(), 1 + x.len() / HOP);
        let mid = &f[f.len() / 2];
        let top = (0..BINS).max_by(|&a, &b| mid[a].total_cmp(&mid[b])).unwrap();
        assert_eq!(top, 90);
        // and an octave lower is far quieter
        assert!(mid[90] - mid[66] > 3.0, "{} vs {}", mid[90], mid[66]);
    }

    #[test]
    fn the_vocabulary_folds_as_documented() {
        // index 0 = C:min, 1 = C, 23 = C#:7 (root 1, quality 9), 168 = X, 169 = N
        assert_eq!(fold(0), class(0, Quality::Min));
        assert_eq!(fold(1), class(0, Quality::Maj));
        assert_eq!(fold(14 + 9), class(1, Quality::Dom7));
        assert_eq!(fold(11 * 14 + 13), class(11, Quality::Sus4));
        assert_eq!(fold(168), NO_CHORD);
        assert_eq!(fold(169), NO_CHORD);
    }

    #[test]
    fn smoothing_and_softmax_give_distributions() {
        let mut l = vec![0f32; 20 * MODEL_CLASSES];
        for t in 0..20 {
            l[t * MODEL_CLASSES + 1] = 20.0; // C major everywhere
        }
        let p = frames_from_logits(&l);
        assert_eq!(p.len(), 20);
        for f in &p {
            assert!((f.iter().sum::<f32>() - 1.0).abs() < 1e-4);
            assert!(f[class(0, Quality::Maj)] > 0.9);
        }
    }

    /// With the model at `CHORDS_MODEL` and a reference dump at `CHORDS_REF` (written by
    /// `scripts/chords-reference.py`): the features and the chords match librosa + ONNX Runtime
    /// in Python on the same audio.
    #[test]
    fn matches_the_python_reference() {
        let (Ok(model), Ok(dir)) = (std::env::var("CHORDS_MODEL"), std::env::var("CHORDS_REF")) else {
            return;
        };
        let dir = PathBuf::from(dir);
        let read = |n: &str| -> Vec<f32> {
            std::fs::read(dir.join(n)).unwrap().as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
        };
        let audio = read("audio22050.f32");
        let want_feats = read("feats.f32");
        let want_idx: Vec<usize> =
            std::fs::read(dir.join("argmax.u16")).unwrap().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c) as usize).collect();
        let m = ChordMini::load(Path::new(&model)).unwrap();
        let feats = m.cqt.log_frames(&audio);
        assert_eq!(feats.len() * BINS, want_feats.len());
        let mut worst = 0f32;
        let mut total = 0f64;
        for (f, w) in feats.iter().flatten().zip(&want_feats) {
            worst = worst.max((f - w).abs());
            total += (f - w).abs() as f64;
        }
        let mean = total / want_feats.len() as f64;
        eprintln!("features: max |Δ| {worst}, mean {mean}");
        assert!(mean < 1e-3, "mean feature error {mean}");
        let logits = m.logits(&feats).unwrap();
        // the reference's own decision: smoothed logits, argmax over the 170 classes
        let n = feats.len();
        let mut same = 0;
        for (t, want) in want_idx.iter().enumerate().take(n) {
            let mut best = (0, f32::MIN);
            for c in 0..MODEL_CLASSES {
                let s: f32 = (t.saturating_sub(4)..(t + 5).min(n)).map(|k| logits[k * MODEL_CLASSES + c]).sum();
                if s > best.1 {
                    best = (c, s);
                }
            }
            same += (best.0 == *want) as usize;
        }
        let agree = same as f64 / n as f64;
        eprintln!("frame agreement {agree}");
        assert!(agree > 0.99, "frame agreement {agree}");
    }
}
