//! Downloaded models. RMVPE (pitch) is optional: downloaded only when the user asks (after the
//! licence note), verified by SHA-256, loaded into the engine as its F0 estimator; without it
//! the engine analyses with YIN. HTDemucs (stem separation, `stems_actions.rs`) is downloaded
//! the same way the first time stems are extracted.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};
use wave_engine::Engine;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub bytes: u64,
    pub installed: bool,
    /// The detector the engine analyses with right now: "rmvpe" or "yin".
    pub backend: String,
    pub license_note: &'static str,
    pub license_url: &'static str,
    pub path: String,
    pub error: Option<String>,
}

pub fn status(engine: &Engine, error: Option<String>) -> ModelStatus {
    let path = wave_rmvpe::model_path();
    ModelStatus {
        id: "rmvpe",
        name: "RMVPE vocal pitch model",
        bytes: wave_rmvpe::MODEL_BYTES,
        installed: path.is_file(),
        backend: engine.f0_backend().to_string(),
        license_note: wave_rmvpe::LICENSE_NOTE,
        license_url: wave_rmvpe::LICENSE_URL,
        path: path.display().to_string(),
        error,
    }
}

/// Loads the installed model into the engine. `Ok(false)` when nothing is installed.
pub fn activate(engine: &Engine) -> Result<bool, String> {
    let path = wave_rmvpe::model_path();
    if !path.is_file() {
        return Ok(false);
    }
    let m = wave_rmvpe::Rmvpe::load(&path)?;
    engine.set_f0_estimator(Some(Arc::new(m)));
    Ok(true)
}

/// One downloadable model file: where it comes from, what it must be, where it goes.
pub struct Download {
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub dst: PathBuf,
}

/// The RMVPE download.
pub fn rmvpe() -> Download {
    Download { url: wave_rmvpe::MODEL_URL, sha256: wave_rmvpe::MODEL_SHA256, bytes: wave_rmvpe::MODEL_BYTES, dst: wave_rmvpe::model_path() }
}

/// The RMVPE model (see [`fetch`]).
pub fn install(progress: impl Fn(u64, u64)) -> Result<(), String> {
    fetch(&rmvpe(), None, progress)
}

/// What [`fetch`] returns when its `cancel` flag was raised (nothing is left behind).
pub const DOWNLOAD_CANCELLED: &str = "download cancelled";

/// Streams a model to `<file>.part`, hashing as it goes, and renames it into place only when
/// the size and checksum match. Blocking. `cancel` stops it between chunks
/// ([`DOWNLOAD_CANCELLED`]); an existing file is kept as it is.
pub fn fetch(d: &Download, cancel: Option<&AtomicBool>, progress: impl Fn(u64, u64)) -> Result<(), String> {
    // one install at a time: the app's buttons and `synkopa f0-model --install` /
    // `synkopa stems --install` stream to the same `.part` files. A second caller waits, then
    // finds the model in place.
    static INSTALLING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = INSTALLING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dst = &d.dst;
    if dst.is_file() {
        return Ok(());
    }
    let dir = dst.parent().ok_or("no folder for the model")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let total = d.bytes;
    let mut resp = ureq::get(d.url).call().map_err(|e| format!("download failed: {e}"))?;
    let mut body = resp.body_mut().with_config().limit(total + 1).reader();
    let tmp = dst.with_extension("part");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let fail = |file: std::fs::File, e: String| {
        drop(file);
        let _ = std::fs::remove_file(&tmp);
        Err(e)
    };
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut told) = (0u64, 0u64);
    progress(0, total);
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return fail(file, DOWNLOAD_CANCELLED.into());
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return fail(file, format!("download failed: {e}")),
        };
        hasher.update(&buf[..n]);
        if let Err(e) = file.write_all(&buf[..n]) {
            return fail(file, e.to_string());
        }
        done += n as u64;
        if done - told >= 1 << 20 {
            told = done;
            progress(done, total);
        }
    }
    drop(file);
    let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != total || !got.eq_ignore_ascii_case(d.sha256) {
        let _ = std::fs::remove_file(&tmp);
        return Err("checksum mismatch — the download was altered or damaged; nothing was saved".into());
    }
    std::fs::rename(&tmp, dst).map_err(|e| e.to_string())?;
    progress(total, total);
    Ok(())
}
