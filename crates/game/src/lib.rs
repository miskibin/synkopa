//! GAME (Generative Adaptive MIDI Extractor, openvpi, 2026): a singing note transcriber that
//! predicts note boundaries and per-note pitch from the waveform, run through ONNX Runtime as
//! Synkopa's optional note model. It hears what a pitch curve cannot: a new syllable on the same
//! pitch is a new note, and a vibrato or scoop is not.
//!
//! Inference follows the reference ONNX workflow (`ONNX.md`, `inference/me_infer.py`): 44.1 kHz
//! mono, cut at silences like the reference slicer (`inference/slicer2.py`: −40 dB, ≥ 1 s
//! chunks, ≥ 200 ms gaps, ≤ 100 ms silence kept); per chunk the encoder, **one** segmenter step
//! at t = 0 (boundary threshold 0.2, radius 2 frames), the estimator (presence threshold 0.2).
//! The segmenter samples D3PM noise; at t = 0 every boundary is removed before it predicts, so
//! the single step is deterministic (more steps re-noise at random). On the benchmark one step
//! was as good as eight (vocadito repeated notes 65/88 vs 59/88, COnP within 0.01) and 5× faster;
//! see `crates/tune-bench/RESULTS.md`.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use wave_engine::pitch::{ModelNote, NoteEstimator};

pub const SR: f32 = 44100.0;
/// Seconds per model frame.
const TIMESTEP: f32 = 0.01;
const BOUNDARY_THRESHOLD: f32 = 0.2;
const BOUNDARY_RADIUS: i64 = 2;
const PRESENCE_THRESHOLD: f32 = 0.2;
/// Longest chunk given to the model at once, seconds: a take without a 200 ms silence is cut
/// at its quietest moment between `MAX_CHUNK / 2` and `MAX_CHUNK` (the encoder's attention grows
/// with the square of the length).
const MAX_CHUNK: f32 = 30.0;

/// The model is not shipped: the user downloads it on request after reading the licence note.
pub const MODEL_ID: &str = "game";
pub const MODEL_NAME: &str = "GAME singing note model";
pub const MODEL_URL: &str = "https://github.com/openvpi/GAME/releases/download/v1.0.3/GAME-1.0.3-small-onnx.zip";
pub const MODEL_SHA256: &str = "00ba0c64115b6b874d9ea4afd3e6cf822abda2a04e52569233b0a044fd40e4e8";
pub const MODEL_BYTES: u64 = 45_676_364;
pub const LICENSE_URL: &str = "https://github.com/openvpi/GAME/releases/tag/v1.0.0";
pub const LICENSE_NOTE: &str = "GAME by Team OpenVPI: the code is MIT, the model weights are licensed CC BY-NC-SA 4.0 \
(non-commercial). Downloading is your decision; without the model Synkopa finds notes with its built-in note tracker.";
/// The archive's folder, and the files the model needs from it.
const ARCHIVE_DIR: &str = "GAME-1.0.3-small-onnx";
const FILES: [&str; 6] = ["config.json", "encoder.onnx", "segmenter.onnx", "estimator.onnx", "dur2bd.onnx", "bd2dur.onnx"];

/// Where the unpacked model lives (`WAVE_GAME_MODEL` overrides it): `<data>/Models/game-small`.
pub fn model_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("WAVE_GAME_MODEL") {
        return PathBuf::from(p);
    }
    wave_rmvpe::models_dir().join("game-small")
}

/// Where the downloaded archive is kept while it is unpacked.
pub fn archive_path() -> PathBuf {
    wave_rmvpe::models_dir().join("GAME-1.0.3-small-onnx.zip")
}

/// Whether every model file is in `dir`.
pub fn installed(dir: &Path) -> bool {
    FILES.iter().all(|f| dir.join(f).is_file())
}

/// Unpacks the model files from the downloaded archive into `dir` (each written to a `.part`
/// file and renamed, so an interrupted unpack never looks installed).
pub fn unpack(archive: &Path, dir: &Path) -> Result<(), String> {
    let f = std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", archive.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for name in FILES {
        let mut src = z.by_name(&format!("{ARCHIVE_DIR}/{name}")).map_err(|e| format!("{name} is missing from the archive: {e}"))?;
        let part = dir.join(format!("{name}.part"));
        let mut out = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        std::io::copy(&mut src, &mut out).map_err(|e| format!("{name}: {e}"))?;
        drop(out);
        std::fs::rename(&part, dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

struct Sessions {
    encoder: ort::session::Session,
    segmenter: ort::session::Session,
    estimator: ort::session::Session,
    dur2bd: ort::session::Session,
    bd2dur: ort::session::Session,
}

pub struct Game {
    s: Mutex<Sessions>,
    _env: ort::environment::Environment,
}

type Bools = (Vec<i64>, Vec<bool>);

impl Game {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let fail = |e: String| format!("GAME note model could not be loaded: {e}");
        let cfg: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).map_err(|e| fail(e.to_string()))?)
                .map_err(|e| fail(e.to_string()))?;
        if cfg["samplerate"].as_f64() != Some(SR as f64)
            || cfg["timestep"].as_f64().map(|t| (t as f32 - TIMESTEP).abs() > 1e-6) != Some(false)
        {
            return Err(fail(format!("unexpected config {cfg}")));
        }
        let env = wave_rmvpe::ort_env().map_err(fail)?;
        let threads = std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4);
        let open = |name: &str| -> Result<ort::session::Session, String> {
            ort::session::Session::builder(&env)
                .map_err(|e| fail(e.to_string()))?
                .with_intra_threads(threads)
                .map_err(|e| fail(e.to_string()))?
                .commit_from_file(dir.join(name))
                .map_err(|e| fail(format!("{name}: {e}")))
        };
        let s = Sessions {
            encoder: open("encoder.onnx")?,
            segmenter: open("segmenter.onnx")?,
            estimator: open("estimator.onnx")?,
            dur2bd: open("dur2bd.onnx")?,
            bd2dur: open("bd2dur.onnx")?,
        };
        Ok(Self { s: Mutex::new(s), _env: env })
    }

    /// Notes of one chunk of 44.1 kHz audio, seconds from its start.
    fn chunk_notes(&self, w: &[f32]) -> Result<Vec<ModelNote>, String> {
        use ort::value::Tensor;
        let e = |x: ort::Error| format!("GAME inference failed: {x}");
        let dur = w.len() as f32 / SR;
        let mut s = self.s.lock();
        let r = s
            .encoder
            .run(ort::inputs![
                "waveform" => Tensor::from_array(([1usize, w.len()], w.to_vec())).map_err(e)?,
                "duration" => Tensor::from_array(([1usize], vec![dur])).map_err(e)?,
            ])
            .map_err(e)?;
        let f32s = |i: usize| -> Result<(Vec<i64>, Vec<f32>), String> {
            let (sh, d) = r[i].try_extract_tensor::<f32>().map_err(e)?;
            Ok((sh.to_vec(), d.to_vec()))
        };
        let bools = |v: &ort::value::DynValue| -> Result<Bools, String> {
            let (sh, d) = v.try_extract_tensor::<bool>().map_err(e)?;
            Ok((sh.to_vec(), d.to_vec()))
        };
        let (seg_sh, x_seg) = f32s(0)?;
        let (est_sh, x_est) = f32s(1)?;
        let mask_t = bools(&r[2])?;
        drop(r);
        let t3 = |sh: &[i64], d: Vec<f32>| Tensor::from_array(([sh[0] as usize, sh[1] as usize, sh[2] as usize], d)).map_err(e);
        let t2b = |b: &Bools| Tensor::from_array(([b.0[0] as usize, b.0[1] as usize], b.1.clone())).map_err(e);
        let known = {
            let r = s
                .dur2bd
                .run(ort::inputs![
                    "durations" => Tensor::from_array(([1usize, 1], vec![dur])).map_err(e)?,
                    "maskT" => t2b(&mask_t)?,
                ])
                .map_err(e)?;
            bools(&r[0])?
        };
        let bounds = {
            let r = s
                .segmenter
                .run(ort::inputs![
                    "x_seg" => t3(&seg_sh, x_seg)?,
                    "language" => Tensor::from_array(([1usize], vec![0i64])).map_err(e)?,
                    "known_boundaries" => t2b(&known)?,
                    "prev_boundaries" => t2b(&known)?,
                    "t" => Tensor::from_array(([1usize], vec![0f32])).map_err(e)?,
                    "maskT" => t2b(&mask_t)?,
                    "threshold" => Tensor::from_array(([0usize; 0], vec![BOUNDARY_THRESHOLD])).map_err(e)?,
                    "radius" => Tensor::from_array(([0usize; 0], vec![BOUNDARY_RADIUS])).map_err(e)?,
                ])
                .map_err(e)?;
            bools(&r[0])?
        };
        let (durs, mask_n) = {
            let r = s.bd2dur.run(ort::inputs!["boundaries" => t2b(&bounds)?, "maskT" => t2b(&mask_t)?]).map_err(e)?;
            let (_, d) = r[0].try_extract_tensor::<f32>().map_err(e)?;
            (d.to_vec(), bools(&r[1])?)
        };
        let (presence, scores) = {
            let r = s
                .estimator
                .run(ort::inputs![
                    "x_est" => t3(&est_sh, x_est)?,
                    "boundaries" => t2b(&bounds)?,
                    "maskT" => t2b(&mask_t)?,
                    "maskN" => t2b(&mask_n)?,
                    "threshold" => Tensor::from_array(([0usize; 0], vec![PRESENCE_THRESHOLD])).map_err(e)?,
                ])
                .map_err(e)?;
            let p = bools(&r[0])?.1;
            let (_, sc) = r[1].try_extract_tensor::<f32>().map_err(e)?;
            (p, sc.to_vec())
        };
        let mut on = 0f64;
        let mut out = vec![];
        for (k, &d) in durs.iter().enumerate() {
            if !mask_n.1.get(k).copied().unwrap_or(false) {
                break;
            }
            let d = d as f64;
            if presence.get(k).copied().unwrap_or(false) && d > 0.0 {
                out.push(ModelNote { start: on, end: on + d, pitch: scores[k] });
            }
            on += d;
        }
        Ok(out)
    }
}

impl NoteEstimator for Game {
    fn name(&self) -> &'static str {
        MODEL_ID
    }

    fn notes(&self, mono: &[f32], sr: f32) -> Result<Vec<ModelNote>, String> {
        let x = if (sr - SR).abs() < 0.5 { mono.to_vec() } else { wave_rmvpe::resample(mono, sr, SR) };
        let mut out = vec![];
        for (a, b) in chunks(&x) {
            let off = a as f64 / SR as f64;
            out.extend(self.chunk_notes(&x[a..b])?.into_iter().map(|n| ModelNote { start: n.start + off, end: n.end + off, ..n }));
        }
        Ok(out)
    }
}

/// Hop of the slicer's level, samples (10 ms).
const HOP: usize = 441;

/// RMS per hop over a `win`-sample window centred on it (librosa `feature.rms`, zero padded).
fn rms(x: &[f32], win: usize) -> Vec<f32> {
    let n = x.len().div_ceil(HOP);
    let mut pre = vec![0f64; x.len() + 1];
    for (i, v) in x.iter().enumerate() {
        pre[i + 1] = pre[i] + (*v as f64) * (*v as f64);
    }
    (0..n)
        .map(|f| {
            let c = (f * HOP) as i64;
            let (a, b) = ((c - win as i64 / 2).max(0) as usize, ((c + win as i64 / 2) as usize).min(x.len()));
            ((pre[b] - pre[a]) / win as f64).sqrt() as f32
        })
        .collect()
}

fn argmin(v: &[f32]) -> usize {
    v.iter().enumerate().fold((0, f32::INFINITY), |m, (i, &x)| if x < m.1 { (i, x) } else { m }).0
}

/// Sample ranges to transcribe one at a time: the reference slicer's cuts at silences (−40 dB,
/// chunks ≥ 1 s, gaps ≥ 200 ms, ≤ 100 ms of silence kept), then any chunk longer than
/// [`MAX_CHUNK`] cut at its quietest frame in the second half of the allowed length.
pub fn chunks(x: &[f32]) -> Vec<(usize, usize)> {
    let total = x.len().div_ceil(HOP);
    let (min_length, min_interval, max_sil) = (100usize, 20usize, 10usize);
    let thr = 10f32.powf(-40.0 / 20.0);
    let lv = rms(x, 4 * HOP);
    let mut tags: Vec<(usize, usize)> = vec![];
    if total > min_length {
        let (mut sil, mut clip) = (None::<usize>, 0usize);
        for (i, &r) in lv.iter().enumerate() {
            if r < thr {
                sil.get_or_insert(i);
                continue;
            }
            let Some(s) = sil else { continue };
            let leading = s == 0 && i > max_sil;
            let middle = i - s >= min_interval && i - clip >= min_length;
            if !leading && !middle {
                sil = None;
                continue;
            }
            if i - s <= max_sil {
                let p = argmin(&lv[s..=i]) + s;
                tags.push(if s == 0 { (0, p) } else { (p, p) });
                clip = p;
            } else if i - s <= max_sil * 2 {
                let p = argmin(&lv[i - max_sil..=(s + max_sil).min(lv.len() - 1)]) + i - max_sil;
                let pl = argmin(&lv[s..=s + max_sil]) + s;
                let pr = argmin(&lv[i - max_sil..=i]) + i - max_sil;
                if s == 0 {
                    tags.push((0, pr));
                    clip = pr;
                } else {
                    tags.push((pl.min(p), pr.max(p)));
                    clip = pr.max(p);
                }
            } else {
                let pl = argmin(&lv[s..=s + max_sil]) + s;
                let pr = argmin(&lv[i - max_sil..=i]) + i - max_sil;
                tags.push(if s == 0 { (0, pr) } else { (pl, pr) });
                clip = pr;
            }
            sil = None;
        }
        if let Some(s) = sil {
            if total - s >= min_interval {
                let p = argmin(&lv[s..(s + max_sil + 1).min(lv.len())]) + s;
                tags.push((p, total + 1));
            }
        }
    }
    let mut frames = vec![];
    if tags.is_empty() {
        frames.push((0, total));
    } else {
        if tags[0].0 > 0 {
            frames.push((0, tags[0].0));
        }
        for w in tags.windows(2) {
            frames.push((w[0].1, w[1].0));
        }
        if tags[tags.len() - 1].1 < total {
            frames.push((tags[tags.len() - 1].1, total));
        }
    }
    // long chunks: cut at the quietest frame between half and the whole allowed length
    let max = (MAX_CHUNK / TIMESTEP) as usize;
    let mut out = vec![];
    for (mut a, b) in frames {
        while b - a > max {
            let c = a + max / 2 + argmin(&lv[a + max / 2..a + max]);
            out.push((a, c));
            a = c;
        }
        out.push((a, b));
    }
    out.into_iter().map(|(a, b)| (a * HOP, (b * HOP).min(x.len()))).filter(|(a, b)| b > a).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * SR) as usize).map(|i| amp * (i as f32 * 0.05).sin()).collect()
    }

    #[test]
    fn chunks_cut_at_silences_and_cover_the_singing() {
        // 2 s sung, 0.5 s silence, 2 s sung
        let mut x = tone(2.0, 0.3);
        x.extend(vec![0.0; (0.5 * SR) as usize]);
        x.extend(tone(2.0, 0.3));
        let c = chunks(&x);
        assert_eq!(c.len(), 2, "{c:?}");
        let s = |n: usize| n as f32 / SR;
        assert!(s(c[0].0) < 0.01 && (s(c[0].1) - 2.0).abs() < 0.15, "{c:?}");
        assert!((s(c[1].0) - 2.5).abs() < 0.15 && (s(c[1].1) - 4.5).abs() < 0.01, "{c:?}");
        // a short take is one chunk
        assert_eq!(chunks(&tone(0.8, 0.3)), vec![(0, (0.8 * SR) as usize)]);
        assert!(chunks(&[]).is_empty());
    }

    #[test]
    fn a_long_take_without_pauses_is_cut_into_bounded_chunks() {
        let x = tone(75.0, 0.3);
        let c = chunks(&x);
        assert!(c.len() >= 3);
        assert_eq!(c[0].0, 0);
        assert_eq!(c[c.len() - 1].1, x.len());
        for w in c.windows(2) {
            assert_eq!(w[0].1, w[1].0, "contiguous");
        }
        assert!(c.iter().all(|(a, b)| (b - a) as f32 / SR <= MAX_CHUNK + 0.011));
    }

    /// With the model installed (`WAVE_GAME_MODEL` or the default folder): the same notes every
    /// run, the boundary at the pitch step, and the first note's key. (The model hears a plain
    /// harmonic tone as barely sung: it calls the second, brighter half unvoiced, as the
    /// reference Python inference does on this signal.)
    #[test]
    #[ignore]
    fn model_transcribes_a_two_note_phrase() {
        let dir = model_dir();
        let g = Game::load(&dir).expect("model");
        let mut x = vec![];
        for (k, midi) in [(0, 60.0f32), (1, 64.0)] {
            let hz = 440.0 * 2f32.powf((midi - 69.0) / 12.0);
            for i in 0..(0.6 * SR) as usize {
                let t = i as f32 / SR + k as f32 * 0.6;
                let ph = std::f32::consts::TAU * hz * t;
                x.push(0.3 * (ph.sin() + 0.5 * (2.0 * ph).sin() + 0.25 * (3.0 * ph).sin()));
            }
        }
        let a = g.notes(&x, SR).unwrap();
        let b = g.notes(&x, SR).unwrap();
        assert_eq!(a, b, "deterministic");
        assert_eq!(a[0].pitch.round(), 60.0, "{a:?}");
        assert!((a[0].end - 0.6).abs() < 0.03, "boundary at the step: {a:?}");
    }
}
