//! Synkopa Keys: finds each instrument's recordings (the grand piano ships inside the app,
//! the others are downloaded into the sample library) and loads an instrument the first time
//! a project's Keys channel plays it. An instrument no channel plays any more is dropped, so
//! browsing instruments does not pile them up in memory.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{Emitter, Manager};
use wave_engine::keys::{Articulation, Category, InstrumentInfo, KeysBank, CATALOG};

use crate::actions::R;
use crate::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// No Keys channel has needed the instrument yet (or none needs it any more).
    Idle,
    /// The recordings are not on this computer (download the kit).
    Missing,
    Loading,
    Ready,
    Error,
}

/// One instrument of the catalog, as the Keys window shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub description: &'static str,
    /// Library kit that holds the recordings (for the download button).
    pub kit: &'static str,
    pub bundled: bool,
    pub credit: &'static str,
    pub license: &'static str,
    pub range: (u8, u8),
    pub articulations: &'static [Articulation],
    /// Download size of the kit, bytes.
    pub bytes: u64,
    /// The recordings are on disk (bundled or downloaded).
    pub installed: bool,
    pub state: Phase,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysStatus {
    pub instruments: Vec<InstrumentStatus>,
}

#[derive(Clone, Debug)]
struct KeysSt {
    phase: Phase,
    error: Option<String>,
    /// Identity of the claim that set `Loading`: a stale guard may not land on a newer
    /// claim's state (see `Landing`).
    claim: u64,
}

impl Default for KeysSt {
    fn default() -> Self {
        Self { phase: Phase::Idle, error: None, claim: 0 }
    }
}

#[derive(Default)]
pub struct Keys {
    st: Mutex<HashMap<&'static str, KeysSt>>,
    next_claim: Mutex<u64>,
}

impl Keys {
    fn get(&self, id: &str) -> KeysSt {
        self.st.lock().get(id).cloned().unwrap_or_default()
    }
}

/// Where an instrument's recordings are: inside the app first (the bundled piano), then the
/// sample library.
fn dir(app: &App, info: &InstrumentInfo) -> Option<PathBuf> {
    let bundled = app.handle.get().and_then(|h| h.path().resource_dir().ok()).map(|d| d.join("keys"));
    find_dir(bundled, info)
}

/// `bundled` (the app's resource folder, if known; only the bundled instrument looks there)
/// first, then the sample library.
pub fn find_dir(bundled: Option<PathBuf>, info: &InstrumentInfo) -> Option<PathBuf> {
    bundled.filter(|_| info.bundled).into_iter().chain([crate::library::library_dir().join(info.kit)]).find(|d| KeysBank::present(info, d))
}

pub fn status(app: &App) -> KeysStatus {
    let sizes = crate::library::kit_sizes();
    let st = app.keys.st.lock().clone();
    KeysStatus {
        instruments: CATALOG
            .iter()
            .map(|i| {
                let s = st.get(i.id).cloned().unwrap_or_default();
                InstrumentStatus {
                    id: i.id,
                    name: i.name,
                    category: i.category,
                    description: i.description,
                    kit: i.kit,
                    bundled: i.bundled,
                    credit: i.credit,
                    license: i.license,
                    range: i.range,
                    articulations: wave_engine::keys::articulations(i.id),
                    bytes: sizes.get(i.kit).copied().unwrap_or(0),
                    installed: s.phase == Phase::Ready || dir(app, i).is_some(),
                    state: s.phase,
                    error: s.error,
                }
            })
            .collect(),
    }
}

/// True when the grand piano is on this computer (a MIDI import gives piano parts to it).
pub fn piano_installed(app: &App) -> bool {
    wave_engine::keys::instrument(wave_engine::keys::DEFAULT_INSTRUMENT).is_some_and(|i| dir(app, i).is_some())
}

/// Emits the current status without writing: the write already happened under a lock that
/// may since have been superseded (a retry's `Ready` must survive the notification).
fn notify(app: &App) {
    if let Some(h) = app.handle.get() {
        let _ = h.emit("keys-status", status(app));
    }
}

fn set(app: &App, id: &'static str, phase: Phase, error: Option<String>) {
    {
        let mut st = app.keys.st.lock();
        let s = st.entry(id).or_default();
        s.phase = phase;
        s.error = error;
    }
    notify(app);
}

/// Claims the load of `id` if nobody has: `Some(claim)` when the caller must load it now.
fn claim(app: &App, id: &'static str) -> Option<u64> {
    let mut st = app.keys.st.lock();
    let s = st.entry(id).or_default();
    if matches!(s.phase, Phase::Loading | Phase::Ready) {
        return None;
    }
    let mut n = app.keys.next_claim.lock();
    *n = n.wrapping_add(1);
    s.claim = *n;
    s.phase = Phase::Loading;
    s.error = None;
    Some(s.claim)
}

fn load_now(app: &App, info: &'static InstrumentInfo) {
    let Some(d) = dir(app, info) else { return set(app, info.id, Phase::Missing, None) };
    set(app, info.id, Phase::Loading, None);
    match KeysBank::load_instrument(info, &d) {
        Ok(b) => {
            app.engine.set_keys_bank(info.id, Some(Arc::new(b)));
            set(app, info.id, Phase::Ready, None);
        }
        Err(e) => {
            log::warn!("Synkopa Keys {}: {e}", info.id);
            set(app, info.id, Phase::Error, Some(e));
        }
    }
}

/// Loads, in the background, every instrument the project's Keys channels play that is not
/// loaded (or loading) yet, and drops the ones no channel plays any more. Cheap when there is
/// nothing to do, so it runs on every project change.
pub fn ensure(app: &Arc<App>) {
    let needed = app.engine.keys_instruments();
    // drop what nothing plays: a loaded instrument holds tens to hundreds of MB
    for id in app.engine.keys_banks() {
        if !needed.contains(&id) {
            app.engine.set_keys_bank(&id, None);
            if let Some(info) = wave_engine::keys::instrument(&id) {
                set(app, info.id, Phase::Idle, None);
            }
        }
    }
    for id in needed {
        let Some(info) = wave_engine::keys::instrument(&id) else { continue };
        let Some(claim) = claim(app, info.id) else { continue };
        let a = app.clone();
        // the OS refusing the thread must not panic mid-claim (`Loading` for ever, exports
        // waiting on a worker that does not exist): Builder reports it, and the claim lands
        // on a retryable error instead — the next `ensure` claims anew
        let spawned = std::thread::Builder::new().name(format!("keys-load-{}", info.id)).spawn(move || {
            let _landing = Landing { app: &a, id: info.id, claim };
            load_now(&a, info);
            // the channel may have switched instrument while it loaded
            if !a.engine.keys_instruments().iter().any(|n| n == info.id) {
                a.engine.set_keys_bank(info.id, None);
                set(&a, info.id, Phase::Idle, None);
            }
        });
        if spawned.is_err() {
            set(app, info.id, Phase::Error, Some("the Keys loader could not start".into()));
        }
    }
}

/// The bounce-refusal wording, shared with the headless host so both front doors say the same thing.
pub fn missing_msg(info: &InstrumentInfo) -> String {
    format!(
        "KEYS_MISSING: the project plays Synkopa Keys {}, but its recordings are not on this computer \
         (download it in the app: the Keys window, or Library → {}); the bounce would be silent",
        info.name, info.kit
    )
}

pub fn error_msg(info: &InstrumentInfo, e: &str) -> String {
    format!("KEYS_ERROR: Synkopa Keys {}: {e}", info.name)
}

/// The bounce verdict for an instrument's final loader state (wait reads it once `Loading` is over).
fn verdict(info: &InstrumentInfo, state: Phase, error: Option<&str>) -> R<()> {
    match state {
        Phase::Ready => Ok(()),
        Phase::Missing => Err(missing_msg(info)),
        Phase::Error => Err(error_msg(info, error.unwrap_or("the instrument did not load"))),
        // unreachable without a loader that died on the way (see `Landing`) — but a bounce
        // that cannot know must be refused loudly, never started half-blind
        Phase::Idle | Phase::Loading => Err(error_msg(info, "the loader did not finish")),
    }
}

/// True when the loader never reached a final phase *and* its own claim still owns
/// `Loading`: flips that to `Error` under the same lock it checked. A newer claim's state
/// is left alone — a stale guard must not turn a fresh load into a false error.
fn land_stuck(st: &Mutex<HashMap<&'static str, KeysSt>>, id: &'static str, claim: u64) -> bool {
    let mut st = st.lock();
    let s = st.entry(id).or_default();
    if s.phase != Phase::Loading || s.claim != claim {
        return false;
    }
    *s = KeysSt { phase: Phase::Error, error: Some("the Keys loader did not finish".into()), claim };
    true
}

/// A panic (or an early exit) between `claim` and the next `set` would leave the phase
/// `Loading` for ever: every bounce would spin, the UI would never learn. Lands on `Error`.
struct Landing<'a> {
    app: &'a App,
    id: &'static str,
    claim: u64,
}

impl Drop for Landing<'_> {
    fn drop(&mut self) {
        // the transition is already written under the lock: notify without writing again —
        // a retry that installed the bank in the meantime keeps its `Ready`
        if land_stuck(&self.app.keys.st, self.id, self.claim) {
            notify(self.app);
        }
    }
}

/// For an offline bounce: waits until every instrument the project plays has finished
/// loading. Returns `Err` when the bounce would start without one (not installed, the load
/// failed, the loader died): a silently absent instrument is worse than a refused bounce.
pub fn wait(app: &App) -> R<()> {
    for id in app.engine.keys_instruments() {
        let Some(info) = wave_engine::keys::instrument(&id) else { continue };
        if let Some(claim) = claim(app, info.id) {
            let _landing = Landing { app, id: info.id, claim };
            load_now(app, info);
        }
        // phase and error are read as one snapshot: a retry claimed between the sleep and a
        // second read must not turn into a false "the loader did not finish" — while the
        // snapshot says `Loading`, the wait simply continues
        let mut snap = app.keys.get(info.id);
        while snap.phase == Phase::Loading {
            std::thread::sleep(std::time::Duration::from_millis(30));
            snap = app.keys.get(info.id);
        }
        verdict(info, snap.phase, snap.error.as_deref())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piano() -> &'static InstrumentInfo {
        wave_engine::keys::instrument(wave_engine::keys::DEFAULT_INSTRUMENT).unwrap()
    }

    #[test]
    fn a_missing_bank_refuses_the_bounce_with_the_kit_hint() {
        let e = verdict(piano(), Phase::Missing, None).unwrap_err();
        assert!(e.starts_with("KEYS_MISSING"), "{e}");
        assert!(e.contains("Grand Piano") && e.contains("would be silent"), "{e}");
    }

    #[test]
    fn a_failed_load_reports_its_cause() {
        assert_eq!(verdict(piano(), Phase::Error, Some("disk full")).unwrap_err(), "KEYS_ERROR: Synkopa Keys Grand Piano: disk full");
    }

    #[test]
    fn a_ready_bank_does_not_block_the_bounce() {
        assert!(verdict(piano(), Phase::Ready, None).is_ok());
    }

    #[test]
    fn an_unfinished_loader_is_refused_not_half_blind() {
        assert!(verdict(piano(), Phase::Loading, None).is_err());
        assert!(verdict(piano(), Phase::Idle, None).is_err());
    }

    #[test]
    fn a_panicked_loader_lands_on_error_not_loading_for_ever() {
        let k = Keys::default();
        assert!(!land_stuck(&k.st, "grand-piano", 1), "Idle is final: nothing to land");
        k.st.lock().insert("grand-piano", KeysSt { phase: Phase::Loading, error: None, claim: 1 });
        assert!(land_stuck(&k.st, "grand-piano", 1));
        let st = k.get("grand-piano");
        assert_eq!(st.phase, Phase::Error);
        assert!(st.error.unwrap().contains("did not finish"));
    }

    #[test]
    fn a_stale_guard_does_not_land_on_a_newer_claim() {
        // the first loader published `Missing` and is still dropping its guard; a newer
        // claim has already started again: the stale drop must not turn it into an error
        let k = Keys::default();
        k.st.lock().insert("grand-piano", KeysSt { phase: Phase::Loading, error: None, claim: 2 });
        assert!(!land_stuck(&k.st, "grand-piano", 1), "claim 1 no longer owns the state");
        assert_eq!(k.get("grand-piano").phase, Phase::Loading, "the newer claim keeps loading");
        assert!(land_stuck(&k.st, "grand-piano", 2), "the owner still lands");
        assert_eq!(k.get("grand-piano").phase, Phase::Error);
    }

    #[test]
    fn instruments_are_independent() {
        let k = Keys::default();
        k.st.lock().insert("wurlitzer", KeysSt { phase: Phase::Loading, error: None, claim: 3 });
        assert!(!land_stuck(&k.st, "grand-piano", 3), "another instrument's claim");
        assert_eq!(k.get("wurlitzer").phase, Phase::Loading);
    }
}
