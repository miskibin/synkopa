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

#[derive(Clone, Debug)]
struct KeysSt {
    phase: Phase,
    error: Option<String>,
    /// Identity of the claim that set `Loading`: a stale guard may not land on a newer
    /// claim's state (see `Landing`).
    claim: u64,
}

pub struct Keys {
    st: Mutex<KeysSt>,
}

impl Default for Keys {
    fn default() -> Self {
        Self { st: Mutex::new(KeysSt { phase: Phase::Idle, error: None, claim: 0 }) }
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
    let st = app.keys.st.lock().clone();
    KeysStatus { state: st.phase, error: st.error, kit: KIT_ID, installed: st.phase == Phase::Ready || dir(app).is_some() }
}

/// Emits the current status without writing: the write already happened under a lock that
/// may since have been superseded (a retry's `Ready` must survive the notification).
fn notify(app: &App) {
    if let Some(h) = app.handle.get() {
        let _ = h.emit("keys-status", status(app));
    }
}

fn set(app: &App, phase: Phase, error: Option<String>) {
    {
        let mut st = app.keys.st.lock();
        st.phase = phase;
        st.error = error;
    }
    notify(app);
}

/// Claims the load if nobody has: `Some(id)` when the caller must load the piano now.
fn claim(app: &App, needed: bool) -> Option<u64> {
    let mut st = app.keys.st.lock();
    if !needed || matches!(st.phase, Phase::Loading | Phase::Ready) {
        return None;
    }
    st.claim = st.claim.wrapping_add(1);
    st.phase = Phase::Loading;
    st.error = None;
    Some(st.claim)
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
    if let Some(claim) = claim(app, needed) {
        let a = app.clone();
        // the OS refusing the thread must not panic mid-claim (`Loading` for ever, exports
        // waiting on a worker that does not exist): Builder reports it, and the claim lands
        // on a retryable error instead — the next `ensure` claims anew
        let spawned = std::thread::Builder::new().name("keys-load".into()).spawn(move || {
            let _landing = Landing { app: &a, claim };
            load_now(&a);
        });
        if spawned.is_err() {
            set(app, Phase::Error, Some("the Keys loader could not start".into()));
        }
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

/// True when the loader never reached a final phase *and* its own claim still owns
/// `Loading`: flips that to `Error` under the same lock it checked. A newer claim's state
/// is left alone — a stale guard must not turn a fresh load into a false error.
fn land_stuck(st: &Mutex<KeysSt>, claim: u64) -> bool {
    let mut st = st.lock();
    if st.phase != Phase::Loading || st.claim != claim {
        return false;
    }
    *st = KeysSt { phase: Phase::Error, error: Some("the Keys loader did not finish".into()), claim };
    true
}

/// A panic (or an early exit) between `claim` and the next `set` would leave the phase
/// `Loading` for ever: every bounce would spin, the UI would never learn. Lands on `Error`.
struct Landing<'a> {
    app: &'a App,
    claim: u64,
}

impl Drop for Landing<'_> {
    fn drop(&mut self) {
        // the transition is already written under the lock: notify without writing again —
        // a retry that installed the bank in the meantime keeps its `Ready`
        if land_stuck(&self.app.keys.st, self.claim) {
            notify(self.app);
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
    if let Some(claim) = claim(app, true) {
        let _landing = Landing { app, claim };
        load_now(app);
    }
    // phase and error are read as one snapshot: a retry claimed between the sleep and a
    // second read must not turn into a false "the loader did not finish" — while the
    // snapshot says `Loading`, the wait simply continues
    let mut snap = app.keys.st.lock().clone();
    while snap.phase == Phase::Loading {
        drop(snap);
        std::thread::sleep(std::time::Duration::from_millis(30));
        snap = app.keys.st.lock().clone();
    }
    verdict(snap.phase, snap.error.as_deref())
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
        assert!(!land_stuck(&k.st, 1), "Idle is final: nothing to land");
        *k.st.lock() = KeysSt { phase: Phase::Loading, error: None, claim: 1 };
        assert!(land_stuck(&k.st, 1));
        let st = k.st.lock().clone();
        assert_eq!(st.phase, Phase::Error);
        assert!(st.error.unwrap().contains("did not finish"));
    }

    #[test]
    fn a_stale_guard_does_not_land_on_a_newer_claim() {
        // the first loader published `Missing` and is still dropping its guard; a newer
        // claim has already started again: the stale drop must not turn it into an error
        let k = Keys::default();
        *k.st.lock() = KeysSt { phase: Phase::Loading, error: None, claim: 2 };
        assert!(!land_stuck(&k.st, 1), "claim 1 no longer owns the state");
        let st = k.st.lock().clone();
        assert_eq!(st.phase, Phase::Loading, "the newer claim keeps loading");
        assert!(land_stuck(&k.st, 2), "the owner still lands");
        assert_eq!(k.st.lock().phase, Phase::Error);
    }
}
