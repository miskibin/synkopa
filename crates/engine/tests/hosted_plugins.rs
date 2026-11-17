//! Hosted plugins (VST3, LV2) from the engine's side: the project format of a plugin slot
//! (old `"vst3"` files load, a plugin that cannot be loaded keeps its saved data), and
//! plugin delay compensation for the latency a hosted plugin reports.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;

use std::sync::Mutex;
use wave_engine::*;

const SR: u32 = 48000;
const BLOCK: usize = 256;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("wave-hosted-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plugin_fx(id: Id, format: &str, cid: &str, state: Option<&str>) -> Fx {
    let mut f = catalog::new_fx(id, FxKind::Plugin, 120.0);
    f.plugin = Some(PluginRef {
        format: format.into(),
        path: "/usr/lib/lv2/delay.lv2".into(),
        cid: cid.into(),
        name: "Look-ahead".into(),
        vendor: "Test".into(),
        state: state.map(Into::into),
    });
    f
}

/// A project file written before LV2 support: the slot's type is `"vst3"`.
fn old_project_json() -> serde_json::Value {
    let mut v = serde_json::to_value(Project::routed()).unwrap();
    let fx = serde_json::json!({
        "id": 7000, "type": "vst3", "bypass": false, "mix": 1.0, "preset": null, "params": {},
        "plugin": {"format": "vst3", "path": "C:\\Program Files\\Common Files\\VST3\\Old.vst3",
                   "cid": "0123456789ABCDEF0123456789ABCDEF", "name": "Old", "vendor": "Someone",
                   "state": "c2F2ZWQgc3RhdGU="}
    });
    v["inserts"][1]["fx"].as_array_mut().unwrap().push(fx);
    v
}

#[test]
fn a_project_from_before_lv2_loads_its_vst3_slot_as_a_plugin_and_writes_the_new_tag() {
    let p = Project::from_json(old_project_json()).expect("old project loads");
    let (_, f) = p.find_fx(7000).expect("the slot is there");
    assert_eq!(f.kind, FxKind::Plugin);
    let pl = f.plugin.as_ref().unwrap();
    assert_eq!((pl.format.as_str(), pl.name.as_str(), pl.state.as_deref()), ("vst3", "Old", Some("c2F2ZWQgc3RhdGU=")));
    assert_eq!(pl.format_label(), "VST3");
    let back = serde_json::to_value(&p).unwrap();
    let slot = back["inserts"][1]["fx"].as_array().unwrap().iter().find(|f| f["id"] == 7000).unwrap();
    assert_eq!(slot["type"], "plugin");
    assert_eq!(slot["plugin"]["format"], "vst3");
    // and the new tag reads back
    assert_eq!(Project::from_json(back).unwrap(), p);
}

#[test]
fn a_plugin_that_cannot_load_is_reported_and_saved_back_intact() {
    let d = dir("missing");
    let path = d.join("old.wave");
    std::fs::write(&path, serde_json::to_vec(&old_project_json()).unwrap()).unwrap();
    // and an LV2 slot whose plugin is not installed
    let e = Engine::new(Project::empty(), Arc::new(NoPlugins));
    e.open(&path).unwrap();
    let ins = e.project().inserts[1].id;
    e.apply(
        &[Op::PutFx { insert_id: ins, fx: plugin_fx(7001, "lv2", "urn:missing:plugin", Some("bHYyIHN0YXRl")), index: None }],
        false,
        None,
    )
    .unwrap();
    let _rt = e.create_rt(SR as f32, BLOCK);
    let st = e.state();
    assert!(st.fx_errors.contains_key(&7000), "{:?}", st.fx_errors);
    assert!(st.fx_errors.contains_key(&7001), "{:?}", st.fx_errors);
    let saved = d.join("saved.wave");
    e.save(&saved).unwrap();
    let reopened = Engine::new(Project::empty(), Arc::new(NoPlugins));
    reopened.open(&saved).unwrap();
    let back = reopened.project();
    let before = e.project();
    for id in [7000, 7001] {
        assert_eq!(back.find_fx(id).unwrap().1.plugin, before.find_fx(id).unwrap().1.plugin, "slot {id} lost data");
    }
    assert_eq!(back.find_fx(7000).unwrap().1.plugin.as_ref().unwrap().state.as_deref(), Some("c2F2ZWQgc3RhdGU="));
    assert_eq!(back.find_fx(7001).unwrap().1.plugin.as_ref().unwrap().state.as_deref(), Some("bHYyIHN0YXRl"));
    let _ = std::fs::remove_dir_all(&d);
}

/// A pure delay of `latency` samples that says so, like a look-ahead plugin.
struct Delay {
    line: Vec<[f32; 2]>,
    w: usize,
    latency: Arc<AtomicUsize>,
}

impl Processor for Delay {
    fn prepare(&mut self, _sample_rate: f32, _max_block: usize) {}
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessCtx) {
        if self.line.is_empty() {
            return;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let old = std::mem::replace(&mut self.line[self.w], [*a, *b]);
            *a = old[0];
            *b = old[1];
            self.w = (self.w + 1) % self.line.len();
        }
    }
    fn latency_samples(&self) -> usize {
        self.latency.load(Relaxed)
    }
}

/// Creates [`Delay`]s; `latency` is what the next instance delays by and what live ones
/// report through [`PluginFactory::latency`].
struct Delays {
    latency: Arc<AtomicUsize>,
    created: Mutex<HashMap<Id, usize>>,
}

impl PluginFactory for Delays {
    fn create(&self, fx_id: Id, _p: &PluginRef, _sr: f32, _max_block: usize, live: bool) -> Result<Box<dyn Processor>, String> {
        let n = self.latency.load(Relaxed);
        if live {
            self.created.lock().unwrap().insert(fx_id, n);
        }
        Ok(Box::new(Delay { line: vec![[0.0; 2]; n], w: 0, latency: Arc::new(AtomicUsize::new(n)) }))
    }
    fn latency(&self, fx_id: Id) -> Option<usize> {
        self.created.lock().unwrap().contains_key(&fx_id).then(|| self.latency.load(Relaxed))
    }
}

fn frames(beats: f64, bpm: f64) -> usize {
    (beats * 60.0 / bpm * SR as f64).round() as usize
}

/// A short burst as a clip on `track` at `beat`.
fn burst(e: &Engine, track: u32, beat: f64, d: &std::path::Path) {
    let wav = d.join(format!("burst{track}.wav"));
    let mut x = vec![0.4f32; 200];
    x.extend(std::iter::repeat_n(0.0, 2000));
    sources::write_wav(&wav, &x, SR).unwrap();
    e.import_audio(&[wav], Some((track, beat))).unwrap();
}

fn routed_without_fx() -> Project {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    p
}

#[test]
fn a_hosted_plugins_reported_latency_is_compensated_like_a_built_in_one() {
    const LAT: usize = 1500;
    let d = dir("pdc");
    let factory = Arc::new(Delays { latency: Arc::new(AtomicUsize::new(LAT)), created: Mutex::new(HashMap::new()) });
    let e = Engine::new(routed_without_fx(), factory.clone());
    e.save(&d.join("song.wave")).unwrap();
    let bpm = e.project().bpm;
    burst(&e, 0, 1.0, &d);
    burst(&e, 1, 1.0, &d);
    let ins0 = e.project().tracks[0].insert;
    e.apply(&[Op::PutFx { insert_id: ins0, fx: plugin_fx(7000, "lv2", "urn:test:delay", None), index: None }], false, None).unwrap();

    // offline: the delayed track still lines up with the other one, on the beat
    let out = e.render(0.0, 2.0, 0.0, SR).unwrap();
    let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
    let beat1 = frames(1.0, bpm);
    let first = left.iter().position(|v| v.abs() > 0.1).expect("nothing came out");
    assert!(first.abs_diff(beat1) <= 1, "render starts at frame {first}, want {beat1}");
    let peak = left[first..first + 150].iter().fold(0f32, |a, b| a.max(b.abs()));
    assert!(peak > 0.7, "the two tracks do not line up (peak {peak}): the plugin's latency was not compensated");

    // live: the audio half knows the latency, and follows when the plugin changes it
    let _rt = e.create_rt(SR as f32, BLOCK);
    assert_eq!(e.playback_latency(), LAT);
    assert!(!e.refresh_plugin_latency(), "nothing changed");
    factory.latency.store(LAT * 2, Relaxed);
    assert!(e.refresh_plugin_latency());
    assert_eq!(e.playback_latency(), LAT * 2);

    // a bypassed plugin delays nothing
    let mut f = e.project().find_fx(7000).unwrap().1.clone();
    f.bypass = true;
    e.apply(&[Op::PutFx { insert_id: ins0, fx: f, index: None }], false, None).unwrap();
    assert_eq!(e.playback_latency(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

/// Refuses plugins until `allow` is set (Synkopa's safety check refusing one, then "Load anyway").
struct Gate {
    allow: std::sync::atomic::AtomicBool,
    vetted: Mutex<Vec<String>>,
}

struct Pass;
impl Processor for Pass {
    fn prepare(&mut self, _sample_rate: f32, _max_block: usize) {}
    fn process(&mut self, _l: &mut [f32], _r: &mut [f32], _ctx: &ProcessCtx) {}
}

impl PluginFactory for Gate {
    fn create(&self, _fx_id: Id, p: &PluginRef, _sr: f32, _max_block: usize, _live: bool) -> Result<Box<dyn Processor>, String> {
        if self.allow.load(Relaxed) {
            Ok(Box::new(Pass))
        } else {
            Err(format!("{}: UNSAFE_PLUGIN: it crashed in Synkopa's safety check", p.name))
        }
    }
    fn vet(&self, plugins: &[(&PluginRef, bool)]) {
        self.vetted.lock().unwrap().extend(plugins.iter().map(|(p, _)| p.cid.clone()));
    }
}

#[test]
fn a_refused_plugin_loads_when_tried_again_and_new_plugins_are_vetted_first() {
    let gate = Arc::new(Gate { allow: false.into(), vetted: Mutex::new(vec![]) });
    let e = Engine::new(Project::empty(), gate.clone());
    let _rt = e.create_rt(SR as f32, BLOCK);
    let ins = e.project().inserts[1].id;
    e.apply(&[Op::PutFx { insert_id: ins, fx: plugin_fx(7100, "lv2", "urn:test:crashy", None), index: None }], false, None).unwrap();
    assert_eq!(*gate.vetted.lock().unwrap(), vec!["urn:test:crashy".to_string()], "vetted before the edit");
    assert!(e.state().fx_errors.get(&7100).is_some_and(|m| m.contains("UNSAFE_PLUGIN")));
    let rev = e.revision();
    // nothing changed: trying again keeps the error
    e.retry_plugins();
    assert!(e.state().fx_errors.contains_key(&7100));
    gate.allow.store(true, Relaxed);
    e.retry_plugins();
    assert!(e.state().fx_errors.is_empty(), "{:?}", e.state().fx_errors);
    assert_eq!(e.revision(), rev, "trying again is not an edit");
}

/// Writes NaN and infinity, like a broken plugin (ZamEQ2 on a quiet sine).
struct Nan;
impl Processor for Nan {
    fn prepare(&mut self, _sample_rate: f32, _max_block: usize) {}
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _ctx: &ProcessCtx) {
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            *a = if i % 2 == 0 { f32::NAN } else { *a };
            *b = f32::INFINITY;
        }
    }
}
struct NanFactory;
impl PluginFactory for NanFactory {
    fn create(&self, _fx_id: Id, _p: &PluginRef, _sr: f32, _max_block: usize, _live: bool) -> Result<Box<dyn Processor>, String> {
        Ok(Box::new(Nan))
    }
}

#[test]
fn an_effect_that_outputs_nan_does_not_poison_the_mix() {
    let e = Engine::new(Project::empty(), Arc::new(NanFactory));
    e.set_silence_skip(false);
    // first on the master, before its limiter; an EQ after it (a filter keeps a NaN forever)
    e.apply(&[Op::PutFx { insert_id: 0, fx: plugin_fx(7200, "lv2", "urn:test:nan", None), index: Some(0) }], false, None).unwrap();
    e.apply(&[Op::PutFx { insert_id: 0, fx: catalog::new_fx(7201, FxKind::Eq, 120.0), index: Some(1) }], false, None).unwrap();
    let mut rt = e.create_rt(SR as f32, BLOCK);
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    e.play(0.0);
    for _ in 0..50 {
        rt.process(&mut l, &mut r, None);
        assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()), "the output stays finite");
    }
}
