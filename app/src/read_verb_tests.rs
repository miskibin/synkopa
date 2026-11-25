//! The read verbs that need the pitch model (`pitch_analyze`, `pitch_target`, the Auto vocal
//! plan and the Auto vocal action) ask the host to ensure it — once, on the shared action the
//! GUI command and the session command both call. A probe host counts the asks, so the guard
//! cannot silently disappear from one front door again.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use wave_engine::autovocal::Style;
use wave_engine::Project;

use crate::actions::{auto_vocal, control, music_analysis, pitch_analysis, pitch_target, vocal_plan, Host, ALL_COMMANDS, R};
use crate::headless::Headless;
use crate::plugins::Plugins;

/// Forwards everything to a [`Headless`] and counts the guard asks.
struct Probed<'a>(&'a Headless, AtomicUsize, AtomicUsize);

impl Host for Probed<'_> {
    fn engine(&self) -> &Arc<wave_engine::Engine> {
        self.0.engine()
    }
    fn plugins(&self) -> &Arc<Plugins> {
        self.0.plugins()
    }
    fn live(&self) -> bool {
        self.0.live()
    }
    fn audio_json(&self) -> Value {
        self.0.audio_json()
    }
    fn cli_json(&self) -> Value {
        self.0.cli_json()
    }
    fn model_error(&self) -> Option<String> {
        self.0.model_error()
    }
    fn wait_keys(&self) -> R<()> {
        self.0.wait_keys()
    }
    fn ensure_pitch_model(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
    fn ensure_live_plugins(&self) {
        self.2.fetch_add(1, Ordering::SeqCst);
    }
}

fn asked(p: &Probed) -> usize {
    p.1.load(Ordering::SeqCst)
}

fn live_asked(p: &Probed) -> usize {
    p.2.load(Ordering::SeqCst)
}

#[test]
fn the_read_verbs_ask_the_host_for_the_pitch_model() {
    let h = Headless::new(Project::empty());
    let p = Probed(&h, AtomicUsize::new(0), AtomicUsize::new(0));

    // a missing clip refuses, but only after the guard ran — that is what is pinned here
    let _ = pitch_analysis(&p, 4242);
    assert_eq!(asked(&p), 1, "pitch_analysis ensures the model");
    let _ = pitch_target(&p, 4242, None);
    assert_eq!(asked(&p), 2, "pitch_target ensures the model");
    let _ = vocal_plan(&p, 4242, None, Style::Natural, false);
    assert_eq!(asked(&p), 3, "vocal_plan ensures the model");
    let _ = auto_vocal(&p, 4242, None, Style::Natural, false, None, None, &|_| {});
    assert_eq!(asked(&p), 4, "auto_vocal ensures the model before its plan");
    let _ = music_analysis(&p, None, None);
    assert_eq!(asked(&p), 4, "music_analysis needs no model");

    // the session command for the same verb goes through the shared action, which asks
    let p2 = Probed(&h, AtomicUsize::new(0), AtomicUsize::new(0));
    let _ = control(&p2, "pitch_analyze", &json!({ "clipId": 4242 }), None, None);
    assert!(asked(&p2) >= 1, "the session's pitch_analyze ensures the model");
}

#[test]
fn the_plugin_parameter_verbs_ask_the_host_for_live_plugins() {
    let h = Headless::new(Project::empty());
    let p = Probed(&h, AtomicUsize::new(0), AtomicUsize::new(0));

    let _ = crate::actions::plugin_scan(&p, false);
    assert_eq!(live_asked(&p), 1, "plugin_scan ensures live plugins");
    let _ = crate::actions::plugin_params(&p, 4242);
    assert_eq!(live_asked(&p), 2, "plugin_params ensures live plugins");
    let _ = crate::actions::plugin_set_param(&p, 4242, 0, 0.5);
    assert_eq!(live_asked(&p), 3, "plugin_set_param ensures live plugins");
    // the pitch guards stay out of the plugin verbs
    assert_eq!(asked(&p), 0);

    let p2 = Probed(&h, AtomicUsize::new(0), AtomicUsize::new(0));
    let _ = control(&p2, "plugin_params", &json!({ "fxId": 4242 }), None, None);
    assert!(live_asked(&p2) >= 1, "the session's plugin_params ensures live plugins");
}

#[test]
fn the_session_plugin_scan_reply_keeps_the_documented_shape() {
    let h = Headless::new(Project::empty());
    let r = control(&h, "plugin_scan", &json!({}), None, None).unwrap();
    assert!(r.get("plugins").is_some(), "the ScanResult fields sit at the top level: {r}");
    assert!(r.get("Err").is_none() && r.get("Ok").is_none(), "a Result wrapper would break every documented caller: {r}");
}

/// The `catalog` reply's verb list is what scripts are told to consult: every name in
/// [`ALL_COMMANDS`] must reach [`control`] — refusals (live-only, missing args) are fine,
/// `UNKNOWN_COMMAND` is not. A typo'd entry fails this test; the other direction (a verb
/// missing from the catalog) is what [`ALL_COMMANDS`]'s own doc warns about.
#[test]
fn every_cataloged_command_reaches_control() {
    let h = Headless::new(Project::sketch());
    let p = Probed(&h, AtomicUsize::new(0), AtomicUsize::new(0));
    for cmd in ALL_COMMANDS {
        // the installers with empty args would download the real models on a machine (or CI
        // runner) that has none; their names are what this walk is about, and their behavior
        // has tests of its own
        if *cmd == "f0_model_install" || *cmd == "note_model_install" || *cmd == "stems_model_install" || *cmd == "chords_model_install" {
            continue;
        }
        match control(&p, cmd, &json!({}), None, None) {
            Ok(_) => {}
            Err(e) => assert!(!e.contains("UNKNOWN_COMMAND"), "{cmd} is not a verb: {e}"),
        }
    }
}
