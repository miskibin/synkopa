//! Export in place through the shared action: the bounce asks the host to wait for the
//! Synkopa Keys piano first (a silent bounce is worse than a refused one — the app's
//! command makes the same wait), and a piano-free project still bounces.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use wave_engine::sources::write_wav;
use wave_engine::{Engine, Project};

use crate::actions::{bake_rider, control, export_in_place, make_unique, Host, R};
use crate::headless::Headless;
use crate::plugins::Plugins;

const SR: f32 = 44100.0;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-export-in-place-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn wav(path: &std::path::Path) {
    let n = (SR * 2.0) as usize;
    let mono: Vec<f32> = (0..n).map(|i| (i as f64 * 220.0 * std::f64::consts::TAU / SR as f64).sin() as f32 * 0.4).collect();
    let st: Vec<f32> = mono.iter().flat_map(|&x| [x, x]).collect();
    write_wav(path, &st, SR as u32).unwrap();
}

/// A [`Host`] that forwards everything to a [`Headless`] except `wait_keys`, which always
/// refuses: the action layer must ask the host and let the answer stop the bounce, whatever
/// the machine happens to have installed.
struct NoPiano<'a>(&'a Headless);

impl Host for NoPiano<'_> {
    fn engine(&self) -> &Arc<Engine> {
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
        Err("KEYS_MISSING: probe".into())
    }
}

/// An empty project with one imported audio clip: (host, clip id).
fn with_a_clip(d: &std::path::Path) -> (Headless, u64) {
    let h = Headless::new(Project::empty());
    let f = d.join("take.wav");
    wav(&f);
    let r = control(&h, "import", &json!({ "paths": [f.display().to_string()], "track": 0, "beat": 0 }), None, None).unwrap();
    (h, r["clipIds"][0].as_u64().unwrap())
}

#[test]
fn the_bounce_asks_the_host_to_wait_for_the_piano() {
    let d = tmp("probe");
    let (h, clip) = with_a_clip(&d);
    let h = NoPiano(&h);
    let rev = h.engine().revision();
    let sources = h.engine().project().sources.len();

    let e = control(&h, "export_in_place", &json!({ "clipIds": [clip] }), None, None).unwrap_err();
    assert!(e.starts_with("KEYS_MISSING"), "{e}");
    assert_eq!(h.engine().revision(), rev, "the refusal is not an edit");
    assert_eq!(h.engine().project().sources.len(), sources, "nothing was rendered");
}

#[test]
fn a_project_that_plays_the_piano_goes_through_keys_wait() {
    let d = tmp("keys");
    let (h, clip) = with_a_clip(&d);
    // the project now plays Synkopa Keys: the front-door wait decides
    let ctx = control(&h, "context", &json!({}), None, None).unwrap();
    let ch = ctx["nextId"].as_u64().unwrap();
    control(
        &h,
        "apply",
        &json!({ "ops": [{ "op": "putChannel", "channel": { "id": ch, "name": "Piano", "kind": "keys", "color": "#4f8ef7", "vol": 0.7937 } }] }),
        None,
        None,
    )
    .unwrap();
    assert!(h.engine().uses_keys());

    // this checkout ships no piano recordings (resources/keys is a stub), so the real
    // front door refuses; where they are installed the bounce legitimately proceeds
    if let Err(e) = control(&h, "export_in_place", &json!({ "clipIds": [clip] }), None, None) {
        assert!(e.starts_with("KEYS_MISSING"), "{e}");
        assert!(e.contains("would be silent"), "{e}");
    }
}

#[test]
fn a_piano_free_project_still_bounces() {
    let d = tmp("bounce");
    let (h, clip) = with_a_clip(&d);
    let rev = h.engine().revision();

    let r = control(&h, "export_in_place", &json!({ "clipIds": [clip] }), None, None).unwrap();
    let source = r["sourceId"].as_u64().expect("the render becomes a new source");
    let bounce = r["clipId"].as_u64().unwrap();
    assert_ne!(bounce, clip, "the reply names the new bounce clip");
    assert_eq!(r["track"], json!(1), "the render lands on a new track below");
    assert!(r["revision"].as_u64().unwrap() > rev);

    let p = h.engine().project();
    let original = p.clips.iter().find(|c| c.id == clip).unwrap();
    assert!(original.muted, "the original clip is muted");
    let bounce_clip = p.clips.iter().find(|c| c.id == bounce).unwrap();
    assert!(!bounce_clip.muted, "the bounce clip plays");
    assert_eq!(bounce_clip.source, source, "the bounce clip plays the render");
    assert_eq!(bounce_clip.track, 1);
    let baked = p.sources.iter().find(|s| s.id == source).unwrap();
    assert_ne!(baked.id, original.source, "the render is its own source");
}

#[test]
fn a_bounce_without_clips_still_names_the_problem() {
    let d = tmp("empty-ids");
    let (h, _) = with_a_clip(&d);
    let e = control(&h, "export_in_place", &json!({}), None, None).unwrap_err();
    assert!(e.contains("clipIds"), "{e}");
    let e = control(&h, "export_in_place", &json!({ "clipIds": [4242] }), None, None).unwrap_err();
    assert!(!e.is_empty(), "an unknown clip is refused, not silently ignored");
}

#[test]
fn the_session_command_and_the_shared_actions_answer_alike() {
    // two identical projects: one asked through the session commands, one through the shared
    // actions the app's Tauri commands call — one action per verb, one reply
    let d = tmp("parity-a");
    let (h, clip) = with_a_clip(&d);
    let params = std::collections::BTreeMap::new();
    let a1 = control(&h, "make_unique", &json!({ "clipIds": [clip] }), None, None).unwrap();
    let a2 = control(&h, "bake_rider", &json!({ "clipId": clip, "params": params }), None, None).unwrap();
    let a3 = control(&h, "export_in_place", &json!({ "clipIds": [clip] }), None, None).unwrap();

    let d2 = tmp("parity-b");
    let (h2, clip2) = with_a_clip(&d2);
    let b1 = make_unique(&h2, &[clip2]).unwrap();
    let b2 = bake_rider(&h2, clip2, &params, None).unwrap();
    let b3 = export_in_place(&h2, &[clip2], None, &std::sync::atomic::AtomicBool::new(false), &mut |_| {}).unwrap();

    assert_eq!(a1, b1, "make_unique: one action, one reply");
    assert_eq!(a2, b2, "bake_rider: one action, one reply");
    assert_eq!(a3, b3, "export_in_place: one action, one reply");
}

#[test]
fn a_cancelled_bounce_comes_back_as_the_cancelled_error() {
    let d = tmp("cancel");
    let (h, clip) = with_a_clip(&d);
    let rev = h.engine().revision();
    // the app's command maps exactly this string to `{cancelled: true}` (lib.rs matches
    // `wave_engine::errors::ERR_CANCELLED`), so the action must surface the engine's
    // `Cancelled` unchanged — not wrapped, not reworded
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let e = export_in_place(&h, &[clip], None, &cancelled, &mut |_| {}).unwrap_err();
    assert_eq!(e, wave_engine::errors::ERR_CANCELLED, "exactly the shared constant");
    assert_eq!(h.engine().revision(), rev, "a cancelled bounce is not an edit");
}

#[test]
fn frombeat_and_tobeat_go_together() {
    let d = tmp("range-pair");
    let (h, clip) = with_a_clip(&d);
    // the pairing rule lives in the session parser, before any keys or model work
    let e = control(&h, "export_in_place", &json!({ "clipIds": [clip], "fromBeat": 0.0 }), None, None).unwrap_err();
    assert!(e.contains("go together"), "{e}");
    let e = control(&h, "export_in_place", &json!({ "clipIds": [clip], "toBeat": 2.0 }), None, None).unwrap_err();
    assert!(e.contains("go together"), "{e}");
}
