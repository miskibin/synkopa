//! "Extract stems" (FL Studio's *Extract stems from sample*): an audio clip split into drums,
//! bass, other and vocals by HTDemucs (`crates/stems`), each stem a new track right below the
//! clip, the clip muted, one undo step. The same function answers the app's dialog (with
//! progress and cancel), the session socket and `synkopa --project FILE stems`.
//!
//! The separation runs off the engine lock: the region is copied out first
//! ([`wave_engine::Engine::stem_region`]), the stems stream to WAV files in the project's media
//! folder as the separator finishes them, and only then are they placed
//! ([`wave_engine::Engine::place_stems`]), which checks that the clip is still there.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use serde_json::{json, Value};
use wave_engine::errors::ERR_NO_STEM_MODEL;
use wave_engine::sources::Audio;
use wave_engine::{Engine, PlacedStems, StemFile};

use crate::actions::{arg, need_u64, Host, R};
use crate::models;

/// Seconds of the source either side of the clip's region the separator also hears, so the
/// first and last notes are separated with what leads into them (not cut from silence).
pub const CONTEXT_S: f64 = 3.0;

/// The stems, in the model's order: id, the name new tracks get, their colour.
pub const STEMS: [(&str, &str, &str); 4] =
    [("drums", "Drums", "#e8a33d"), ("bass", "Bass", "#8f6ee8"), ("other", "Other", "#47b8a8"), ("vocals", "Vocals", "#e05a8a")];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StemModelStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub bytes: u64,
    pub installed: bool,
    pub license_note: &'static str,
    pub license_url: &'static str,
    pub path: String,
    pub stems: Vec<&'static str>,
}

pub fn status() -> StemModelStatus {
    let path = wave_stems::model_path();
    StemModelStatus {
        id: wave_stems::MODEL_ID,
        name: wave_stems::MODEL_NAME,
        bytes: wave_stems::MODEL_BYTES,
        installed: path.is_file(),
        license_note: wave_stems::LICENSE_NOTE,
        license_url: wave_stems::LICENSE_URL,
        path: path.display().to_string(),
        stems: STEMS.iter().map(|s| s.0).collect(),
    }
}

pub fn download() -> models::Download {
    models::Download {
        url: wave_stems::MODEL_URL,
        sha256: wave_stems::MODEL_SHA256,
        bytes: wave_stems::MODEL_BYTES,
        dst: wave_stems::model_path(),
    }
}

pub fn control(app: &dyn Host, cmd: &str, a: &Value, _if_revision: Option<u64>) -> Option<R<Value>> {
    Some(match cmd {
        "stems_model" => Ok(json!(status())),
        // the download (~90 MB) is checked inside `fetch`; session requests run on their own thread
        "stems_model_install" => models::fetch(&download(), None, |_, _| {}).map(|_| json!(status())),
        "extract_stems" => (|| {
            let clip = need_u64(a, "clipId")?;
            let which = stems_arg(a)?;
            let e = app.engine();
            match extract(e, clip, &which, &AtomicBool::new(false), &|_, _| {})? {
                Some(p) => Ok(placed_json(&p, e.revision())),
                None => Ok(json!({ "cancelled": true })),
            }
        })(),
        _ => return None,
    })
}

/// `stems`: the ids to extract (default: all four), in any order.
pub fn stems_arg(a: &Value) -> R<Vec<String>> {
    match arg(a, "stems") {
        None | Some(Value::Null) => Ok(STEMS.iter().map(|s| s.0.to_string()).collect()),
        Some(Value::Array(v)) => {
            v.iter().map(|x| x.as_str().map(str::to_owned).ok_or("`stems`: a list of stem names".to_string())).collect()
        }
        Some(Value::String(s)) => Ok(s.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()),
        Some(_) => Err("`stems`: a list of stem names (drums, bass, other, vocals)".into()),
    }
}

pub fn placed_json(p: &PlacedStems, revision: u64) -> Value {
    json!({ "sourceIds": p.source_ids, "clipIds": p.clip_ids, "track": p.track, "mutedClip": p.muted_clip, "revision": revision })
}

/// One extraction at a time: a second would double the memory (each holds the model, ~2.5 GB
/// while running) and halve the speed of both.
static RUNNING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Separates clip `clip_id` into the stems `which` and places them (see the module docs).
/// `progress(stage, frac)` reports `"prepare"`, `"load"` (the model), `"separate"` (0..1) and
/// `"place"`. `Ok(None)` when `cancel` was raised: nothing is left behind.
pub fn extract(
    engine: &Engine,
    clip_id: u64,
    which: &[String],
    cancel: &AtomicBool,
    progress: &dyn Fn(&'static str, f64),
) -> R<Option<PlacedStems>> {
    let picked: Vec<usize> = (0..STEMS.len()).filter(|&i| which.iter().any(|w| w.eq_ignore_ascii_case(STEMS[i].0))).collect();
    if let Some(bad) = which.iter().find(|w| !STEMS.iter().any(|s| s.0.eq_ignore_ascii_case(w))) {
        return Err(format!("no stem called `{bad}` (drums, bass, other, vocals)"));
    }
    if picked.is_empty() {
        return Err("pick at least one stem".into());
    }
    let model = wave_stems::model_path();
    if !model.is_file() {
        return Err(format!(
            "{ERR_NO_STEM_MODEL} the stem separation model is not installed ({} MB download; `synkopa stems --install` or the Extract stems dialog)",
            (wave_stems::MODEL_BYTES as f64 / 1e6).round()
        ));
    }
    let Ok(_one) = RUNNING.try_lock() else {
        return Err("stems are already being extracted from another clip; wait for it or cancel it".into());
    };
    progress("prepare", 0.0);
    let region = engine.stem_region(clip_id, CONTEXT_S).map_err(|e| e.to_string())?;
    let (mut l, mut r): (Vec<f32>, Vec<f32>) = region.data.as_chunks::<2>().0.iter().map(|f| (f[0], f[1])).unzip();
    if region.sr != wave_stems::SR {
        // HTDemucs runs at 44.1 kHz; the stems stay there (any rate plays, and resampling them
        // back would only cost time and a second filter)
        let (from, to) = (f64::from(region.sr), f64::from(wave_stems::SR));
        (l, r) = std::thread::scope(|s| {
            let a = s.spawn(|| wave_stems::resample(&l, from, to));
            let b = wave_stems::resample(&r, from, to);
            (a.join().unwrap_or_default(), b)
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    progress("load", 0.0);
    let sep = wave_stems::Separator::load(&model, wave_stems::default_threads())?;
    let idx: Vec<usize> = picked
        .iter()
        .map(|&i| sep.sources().iter().position(|s| s == STEMS[i].0).ok_or(format!("the installed model has no `{}` stem", STEMS[i].0)))
        .collect::<R<_>>()?;
    std::fs::create_dir_all(&region.dir).map_err(|e| format!("{}: {e}", region.dir.display()))?;
    let paths: Vec<PathBuf> =
        picked.iter().map(|&i| fresh_path(&region.dir, &format!("{} - {}", sanitize(&region.name), STEMS[i].1))).collect();
    let cleanup = |paths: &[PathBuf]| {
        for p in paths {
            let _ = std::fs::remove_file(p);
        }
    };
    let spec = hound::WavSpec { channels: 2, sample_rate: wave_stems::SR, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut writers = Vec::with_capacity(paths.len());
    for p in &paths {
        match hound::WavWriter::create(p, spec) {
            Ok(w) => writers.push(w),
            Err(e) => {
                drop(writers);
                cleanup(&paths);
                return Err(format!("{}: {e}", p.display()));
            }
        }
    }
    let res = sep.separate(
        &l,
        &r,
        |f| {
            progress("separate", f64::from(f));
            !cancel.load(Ordering::Relaxed)
        },
        |stems| {
            for (w, &i) in writers.iter_mut().zip(&idx) {
                for v in &stems[i] {
                    w.write_sample(*v).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        },
    );
    // the model's memory goes before the stems are decoded
    drop(sep);
    drop((l, r));
    let finished: Result<(), String> = writers.into_iter().try_for_each(|w| w.finalize().map_err(|e| e.to_string()));
    match res {
        Err(wave_stems::Error::Cancelled) => {
            cleanup(&paths);
            return Ok(None);
        }
        Err(e) => {
            cleanup(&paths);
            return Err(e.to_string());
        }
        Ok(()) => {}
    }
    if let Err(e) = finished {
        cleanup(&paths);
        return Err(format!("writing the stems: {e}"));
    }
    if cancel.load(Ordering::Relaxed) {
        cleanup(&paths);
        return Ok(None);
    }
    progress("place", 1.0);
    let mut files = Vec::with_capacity(paths.len());
    for (p, &i) in paths.iter().zip(&picked) {
        match Audio::load(p) {
            Ok(audio) => files.push(StemFile { label: STEMS[i].1.into(), color: STEMS[i].2.into(), path: p.clone(), audio }),
            Err(e) => {
                cleanup(&paths);
                return Err(format!("{}: {e}", p.display()));
            }
        }
    }
    match engine.place_stems(&region, files) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            cleanup(&paths);
            Err(e.to_string())
        }
    }
}

/// Like the engine's media names: letters, digits, spaces, `-` and `_`.
fn sanitize(s: &str) -> String {
    // short enough that the folder, the stem name and a counter stay far below Windows' MAX_PATH
    s.chars().take(80).map(|ch| if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' { ch } else { '_' }).collect()
}

/// `dir/stem.wav`, or `dir/stem (2).wav` … when taken (never overwrite media another project uses).
fn fresh_path(dir: &Path, stem: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.wav"));
    let mut k = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({k}).wav"));
        k += 1;
    }
    p
}
