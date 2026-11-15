//! FX chain presets as `fxchain …` and the mixer use them: save an insert's effects, load them
//! onto another insert or into another project (replace or append, one undo step), rename,
//! delete, and the ways a preset can be wrong.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use wave_engine::catalog::new_fx;
use wave_engine::{Engine, Fx, FxKind, NoPlugins, Op, PluginRef, Project};

use super::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-fxchain-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// The sketch with a four-effect chain on its first non-master insert: an EQ with its bands,
/// a compressor at half wet and bypassed, a delay with an edited parameter, and a VST3 plugin
/// with saved state (which `NoPlugins` cannot load). Returns the engine and that insert's id.
fn with_chain() -> (Arc<Engine>, u64) {
    let e = Engine::new(Project::sketch(), Arc::new(NoPlugins));
    let p = e.project();
    let insert = p.inserts.iter().find(|m| m.id != 0).unwrap().id;
    let mut ops: Vec<Op> = p.insert(insert).unwrap().fx.iter().map(|f| Op::RemoveFx { fx_id: f.id }).collect();
    let mut id = p.next_id;
    let mut next = || {
        id += 1;
        id
    };
    let eq = new_fx(next(), FxKind::Eq, p.bpm);
    assert!(eq.bands.as_ref().is_some_and(|b| !b.is_empty()));
    let mut comp = new_fx(next(), FxKind::Comp, p.bpm);
    comp.bypass = true;
    comp.mix = 0.5;
    let mut delay = new_fx(next(), FxKind::Delay, p.bpm);
    delay.params.insert("fb".into(), 37.0);
    delay.preset = None;
    let vst = Fx {
        id: next(),
        kind: FxKind::Plugin,
        bypass: false,
        mix: 0.8,
        preset: None,
        params: Default::default(),
        bands: None,
        plugin: Some(PluginRef {
            format: "vst3".into(),
            path: "C:\\Program Files\\Common Files\\VST3\\Nowhere.vst3".into(),
            cid: "00112233445566778899AABBCCDDEEFF".into(),
            name: "Nowhere Verb".into(),
            vendor: "Nobody".into(),
            state: Some("c3RhdGU=".into()),
        }),
        maps: None,
    };
    ops.extend([eq, comp, delay, vst].into_iter().map(|fx| Op::PutFx { insert_id: insert, fx, index: None }));
    e.apply(&ops, false, None).unwrap();
    (e, insert)
}

/// An insert's effects without their ids, for comparing chains across inserts and projects.
fn chain(e: &Engine, insert: u64) -> Vec<Fx> {
    e.project().insert(insert).unwrap().fx.iter().map(|f| Fx { id: 0, ..f.clone() }).collect()
}

fn names(v: &Value) -> Vec<String> {
    v["presets"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_saved_chain_loads_into_another_project_exactly_and_leaves_the_insert_alone() {
    let dir = tmp("roundtrip");
    let (e, src) = with_chain();
    let saved = save(&e, &dir, src, "Vocal chain", false).unwrap();
    assert_eq!(saved, json!({ "name": "Vocal chain", "effects": 4 }));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("Vocal chain.wavechain")).unwrap()).unwrap();
    assert_eq!((file["format"].as_str(), file["version"].as_u64()), (Some("wave-fx-chain"), Some(1)));
    assert!(file["fx"].as_array().unwrap().iter().all(|f| f["id"] == 0), "no project ids in the file");
    assert!(file.get("vol").is_none() && file.get("name").is_none(), "insert settings are not part of a chain");
    let listed = list(&dir);
    assert_eq!(
        listed,
        [json!({ "name": "Vocal chain", "effects": [
        { "type": "eq", "name": "Parametric EQ" }, { "type": "comp", "name": "Compressor" },
        { "type": "delay", "name": "Delay" }, { "type": "plugin", "name": "Nowhere Verb" }], "plugins": 1 })],
        "{listed:?}"
    );

    // another project: an empty insert with its own volume, pan, name and colour
    let other = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let p = other.project();
    let target = p.inserts.iter().find(|m| m.id != 0 && m.fx.is_empty()).map(|m| m.id).unwrap_or(0);
    let mut m = p.insert(target).unwrap().clone();
    (m.vol, m.pan, m.name, m.color) = (0.4, -0.3, "Lead Vox".into(), "#ff00aa".into());
    other.apply(&[Op::PutInsert { insert: m.clone() }], false, None).unwrap();
    let before = other.project();
    let next = before.next_id;

    let r = load(&other, &dir, target, "vocal CHAIN", None, None).unwrap();
    assert_eq!(r["fxIds"], json!([next, next + 1, next + 2, next + 3]), "fresh ids in chain order");
    assert_eq!(r["replaced"], false);
    assert_eq!(chain(&other, target), chain(&e, src), "every effect, in order, with its settings and plugin state");
    // with no audio device nothing is instantiated, so nothing failed yet; the slot that cannot
    // load (see `a_plugin_that_will_not_load_is_named_and_keeps_its_slot`) still holds its state
    assert_eq!(r["failed"], json!([]), "{r}");
    assert_eq!(other.project().find_fx(next + 3).unwrap().1.plugin.as_ref().unwrap().state.as_deref(), Some("c3RhdGU="));
    let after = other.project().insert(target).unwrap().clone();
    assert_eq!((after.vol, after.pan, after.name.as_str(), after.color.as_str()), (0.4, -0.3, "Lead Vox", "#ff00aa"));
    assert_eq!(other.project().tracks, before.tracks, "routing is untouched");

    // one undo step takes the whole chain away again, redo brings it back
    assert!(other.undo());
    assert_eq!(other.project().insert(target).unwrap().fx, before.insert(target).unwrap().fx);
    assert!(other.redo());
    assert_eq!(chain(&other, target), chain(&e, src));
    // the plugin that would not load kept its preset intact
    assert_eq!(list(&dir)[0]["effects"][3]["name"], "Nowhere Verb");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_insert_with_effects_needs_replace_or_append_and_each_is_one_undo_step() {
    let dir = tmp("modes");
    let (e, src) = with_chain();
    save(&e, &dir, src, "Four", false).unwrap();
    let p = e.project();
    let dest = p.inserts.iter().find(|m| m.id != 0 && m.id != src).unwrap().id;
    e.apply(&[Op::PutFx { insert_id: dest, fx: new_fx(p.next_id, FxKind::Reverb, p.bpm), index: None }], false, None).unwrap();
    let mine = chain(&e, dest);
    assert_eq!(mine.len(), 1);

    let err = load(&e, &dir, dest, "Four", None, None).unwrap_err();
    assert!(err.contains("replace or append"), "{err}");
    assert_eq!(chain(&e, dest), mine, "nothing was wiped");

    load(&e, &dir, dest, "Four", Some(Mode::Append), None).unwrap();
    let got = chain(&e, dest);
    assert_eq!(got.len(), 5);
    assert_eq!(got[0], mine[0], "append keeps the insert's own effects first");
    assert_eq!(&got[1..], &chain(&e, src)[..]);
    assert!(e.undo());
    assert_eq!(chain(&e, dest), mine);

    let r = load(&e, &dir, dest, "Four", Some(Mode::Replace), None).unwrap();
    assert_eq!(r["replaced"], true);
    assert_eq!(chain(&e, dest), chain(&e, src));
    assert!(e.undo());
    assert_eq!(chain(&e, dest), mine, "undo brings the replaced effects back");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn names_overwrite_rename_and_delete() {
    let dir = tmp("names");
    let (e, src) = with_chain();
    for bad in ["", "   ", "a/b", "what?", "trailing.", ".hidden", &"x".repeat(81)] {
        assert!(save(&e, &dir, src, bad, false).is_err(), "{bad:?} was accepted");
    }
    save(&e, &dir, src, "  Drum Bus  ", false).unwrap();
    let err = save(&e, &dir, src, "drum bus", false).unwrap_err();
    assert!(err.starts_with("EXISTS:"), "{err}");
    // overwriting under another spelling keeps one file, with the new spelling
    save(&e, &dir, src, "drum bus", true).unwrap();
    assert_eq!(names(&json!({ "presets": list(&dir) })), ["drum bus"]);

    save(&e, &dir, src, "Alpha", false).unwrap();
    assert!(rename(&dir, "alpha", "Drum Bus").unwrap_err().starts_with("EXISTS:"));
    rename(&dir, "alpha", "ALPHA").unwrap();
    rename(&dir, "drum bus", "Zeta").unwrap();
    assert_eq!(names(&json!({ "presets": list(&dir) })), ["ALPHA", "Zeta"]);
    assert!(rename(&dir, "nope", "x").unwrap_err().contains("no FX chain preset"));

    delete(&dir, "zeta").unwrap();
    assert_eq!(names(&json!({ "presets": list(&dir) })), ["ALPHA"]);
    assert!(delete(&dir, "zeta").is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_empty_insert_cannot_be_saved_and_a_bad_file_is_listed_but_not_loaded() {
    let dir = tmp("bad");
    let (e, src) = with_chain();
    let empty = e.project().inserts.iter().find(|m| m.id != src && m.fx.is_empty()).map(|m| m.id);
    if let Some(empty) = empty {
        assert!(save(&e, &dir, empty, "Nothing", false).unwrap_err().contains("no effects"));
    }
    assert!(save(&e, &dir, 9999, "X", false).unwrap_err().contains("no insert"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Broken.wavechain"), "{ not json").unwrap();
    std::fs::write(dir.join("Future.wavechain"), r#"{"format":"wave-fx-chain","version":9,"fx":[]}"#).unwrap();
    std::fs::write(
        dir.join("Alien.wavechain"),
        r#"{"format":"wave-fx-chain","version":1,"fx":[{"id":0,"type":"warpdrive","bypass":false}]}"#,
    )
    .unwrap();
    std::fs::write(dir.join("notes.txt"), "not a preset").unwrap();
    let rows = list(&dir);
    assert_eq!(rows.iter().map(|r| r["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Alien", "Broken", "Future"]);
    assert!(rows.iter().all(|r| r["error"].is_string()), "{rows:?}");
    let before = e.project();
    for (name, why) in [("Broken", "damaged"), ("Future", "newer WAVE"), ("Alien", "does not have"), ("Missing", "no FX chain preset")] {
        let err = load(&e, &dir, src, name, Some(Mode::Replace), None).unwrap_err();
        assert!(err.contains(why), "{name}: {err}");
    }
    assert_eq!(e.project().inserts, before.inserts, "a failed load changes nothing");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_plugin_that_will_not_load_is_named_and_keeps_its_slot() {
    let dir = tmp("missing");
    let (e, src) = with_chain();
    save(&e, &dir, src, "With plugin", false).unwrap();
    // a running audio graph instantiates effects, so a missing plugin reports why
    let other = Engine::new(Project::empty(), Arc::new(NoPlugins));
    let _rt = other.create_rt(48_000.0, 256);
    let r = load(&other, &dir, 0, "With plugin", Some(Mode::Append), None).unwrap();
    let ids = r["fxIds"].as_array().unwrap().clone();
    let failed = r["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1, "{r}");
    assert_eq!((&failed[0]["fxId"], failed[0]["name"].as_str()), (&ids[3], Some("Nowhere Verb")));
    assert!(failed[0]["error"].is_string());
    assert!(
        ids.iter().all(|id| other.project().find_fx(id.as_u64().unwrap()).is_some()),
        "the slot stays, so the project can be opened where the plugin is"
    );
    assert_eq!(list(&dir)[0]["effects"].as_array().unwrap().len(), 4, "the preset is untouched");
    std::fs::remove_dir_all(&dir).ok();
}
