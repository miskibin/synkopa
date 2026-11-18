//! Synkopa Keys: finds the piano's recordings (shipped inside the app, or downloaded into the
//! sample library) and loads them the first time a project has a Keys channel.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{Emitter, Manager};
use wave_engine::keys::{KeysBank, KIT_ID};

use crate::actions::R;
use crate::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// No Keys channel has needed the piano yet.
    Idle,
    /// The recordings are not on this computer (a build without them: download the kit).
    Missing,
    Loading,
    Ready,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysStatus {
    pub state: Phase,
    pub error: Option<String>,
    /// Library kit that holds the piano (for the download button).
    pub kit: &'static str,
    /// The recordings are on disk (bundled or downloaded).
    pub installed: bool,
}

pub struct Keys {
    st: Mutex<(Phase, Option<String>)>,
}

impl Default for Keys {
    fn default() -> Self {
        Self { st: Mutex::new((Phase::Idle, None)) }
    }
}

/// Where the recordings are: inside the app first, then the sample library.
fn dir(app: &App) -> Option<PathBuf> {
    let bundled = app.handle.get().and_then(|h| h.path().resource_dir().ok()).map(|d| d.join("keys"));
    find_dir(bundled)
}

/// `bundled` (the app's resource folder, if known) first, then the sample library.
pub fn find_dir(bundled: Option<PathBuf>) -> Option<PathBuf> {
    bundled.into_iter().chain([crate::library::library_dir().join(KIT_ID)]).find(|d| KeysBank::present(d))
}

pub fn status(app: &App) -> KeysStatus {
    let (state, error) = app.keys.st.lock().clone();
    KeysStatus { state, error, kit: KIT_ID, installed: state == Phase::Ready || dir(app).is_some() }
}

fn set(app: &App, phase: Phase, error: Option<String>) {
    *app.keys.st.lock() = (phase, error);
    if let Some(h) = app.handle.get() {
        let _ = h.emit("keys-status", status(app));
    }
}

/// Claims the load if nobody has: true when the caller must load the piano now.
fn claim(app: &App, needed: bool) -> bool {
    let mut st = app.keys.st.lock();
    if !needed || matches!(st.0, Phase::Loading | Phase::Ready) {
        return false;
    }
    st.0 = Phase::Loading;
    true
}

fn load_now(app: &App) {
    let Some(d) = dir(app) else { return set(app, Phase::Missing, None) };
    set(app, Phase::Loading, None);
    match KeysBank::load(&d) {
        Ok(b) => {
            app.engine.set_keys_bank(Some(Arc::new(b)));
            set(app, Phase::Ready, None);
        }
        Err(e) => {
            log::warn!("Synkopa Keys: {e}");
            set(app, Phase::Error, Some(e));
        }
    }
}

/// Loads the piano in the background if `needed` and it is not loaded (or loading) yet.
/// Cheap when there is nothing to do, so it runs on every project change.
pub fn ensure(app: &Arc<App>, needed: bool) {
    if claim(app, needed) {
        let a = app.clone();
        std::thread::spawn(move || {
            let _landing = Landing(&a);
            load_now(&a);
        });
    }
}

/// The bounce-refusal wording, shared with the headless host so both front doors say the same thing.
pub fn missing_msg() -> String {
    "KEYS_MISSING: the project plays Synkopa Keys, but the piano recordings are not on this computer \
     (download the kit in the app: Library → Synkopa Keys); the bounce would be silent"
        .into()
}

pub fn error_msg(e: &str) -> String {
    format!("KEYS_ERROR: Synkopa Keys: {e}")
}

/// The bounce verdict for a final loader state (wait reads it once `Loading` is over).
fn verdict(state: Phase, error: Option<&str>) -> R<()> {
    match state {
        Phase::Ready => Ok(()),
        Phase::Missing => Err(missing_msg()),
        Phase::Error => Err(error_msg(error.unwrap_or("the piano did not load"))),
        // unreachable without a loader that died on the way (see `Landing`) — but a bounce
        // that cannot know must be refused loudly, never started half-blind
        Phase::Idle | Phase::Loading => Err(error_msg("the loader did not finish")),
    }
}

/// True when the loader never reached a final phase: flips `Loading` to `Error` (see `Landing`).
fn land_stuck(st: &Mutex<(Phase, Option<String>)>) -> bool {
    let mut st = st.lock();
    if st.0 != Phase::Loading {
        return false;
    }
    *st = (Phase::Error, Some("the Keys loader did not finish".into()));
    true
}

/// A panic (or an early exit) between `claim` and the next `set` would leave the phase
/// `Loading` for ever: every bounce would spin, the UI would never learn. Lands on `Error`.
struct Landing<'a>(&'a App);

impl Drop for Landing<'_> {
    fn drop(&mut self) {
        if land_stuck(&self.0.keys.st) {
            set(self.0, Phase::Error, Some("the Keys loader did not finish".into()));
        }
    }
}

/// For an offline bounce: waits until the piano a project needs has finished loading.
/// Returns `Err` when the bounce would start without the piano (nothing installed, the
/// load failed, the loader died): a silently absent piano is worse than a refused bounce.
pub fn wait(app: &App) -> R<()> {
    if !app.engine.uses_keys() {
        return Ok(());
    }
    if claim(app, true) {
        let _landing = Landing(app);
        load_now(app);
    } else {
        while app.keys.st.lock().0 == Phase::Loading {
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }
    let (state, err) = app.keys.st.lock().clone();
    verdict(state, err.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_bank_refuses_the_bounce_with_the_kit_hint() {
        let e = verdict(Phase::Missing, None).unwrap_err();
        assert!(e.starts_with("KEYS_MISSING"), "{e}");
        assert!(e.contains("would be silent"), "{e}");
    }

    #[test]
    fn a_failed_load_reports_its_cause() {
        assert_eq!(verdict(Phase::Error, Some("disk full")).unwrap_err(), "KEYS_ERROR: Synkopa Keys: disk full");
    }

    #[test]
    fn a_ready_bank_does_not_block_the_bounce() {
        assert!(verdict(Phase::Ready, None).is_ok());
    }

    #[test]
    fn an_unfinished_loader_is_refused_not_half_blind() {
        assert!(verdict(Phase::Loading, None).is_err());
        assert!(verdict(Phase::Idle, None).is_err());
    }

    #[test]
    fn a_panicked_loader_lands_on_error_not_loading_for_ever() {
        let k = Keys::default();
        assert!(!land_stuck(&k.st), "Idle is final: nothing to land");
        *k.st.lock() = (Phase::Loading, None);
        assert!(land_stuck(&k.st));
        let (state, err) = k.st.lock().clone();
        assert_eq!(state, Phase::Error);
        assert!(err.unwrap().contains("did not finish"));
    }
}
