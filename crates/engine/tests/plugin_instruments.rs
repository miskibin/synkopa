//! Plugin-instrument channels (a hosted VST3 in the Channel Rack) from the engine's side,
//! with a stand-in factory: the project format, validation, instances made and released as
//! channels come and go, sample-accurate notes from patterns, live keys and auditions, the
//! render's own offline instance, saved state, and a plugin that does not load.

use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

use wave_engine::midi::MidiMsg;
use wave_engine::*;

const SR: u32 = 48000;
const BLOCK: usize = 256;

/// Holds a level per sounding note (velocity × `level`); outputs their sum as DC on both
/// sides. `level` comes from the state ("level=0.5").
struct Toy {
    level: f32,
    on: Vec<(i32, f32)>,
    events: Arc<Mutex<Vec<InstMsg>>>,
}

impl Instrument for Toy {
    fn prepare(&mut self, _: f32, _: usize) {}
    fn process(&mut self, events: &[InstEvent], l: &mut [f32], r: &mut [f32], _: &ProcessCtx) {
        let mut next = 0;
        for i in 0..l.len() {
            while next < events.len() && events[next].offset as usize == i {
                let e = events[next];
                self.events.lock().unwrap().push(e.msg);
                match e.msg {
                    InstMsg::NoteOn { vel, id, .. } => self.on.push((id, vel)),
                    InstMsg::NoteOff { id, .. } => self.on.retain(|n| n.0 != id),
                    _ => {}
                }
                next += 1;
            }
            let v: f32 = self.on.iter().map(|n| n.1 * self.level).sum();
            l[i] = v;
            r[i] = v;
        }
    }
}

#[derive(Default)]
struct Factory {
    live: AtomicUsize,
    offline: AtomicUsize,
    released: Mutex<Vec<Id>>,
    /// What `state` reports for a live channel (as if its editor had changed it).
    state: Mutex<Option<String>>,
    events: Arc<Mutex<Vec<InstMsg>>>,
}

impl PluginFactory for Factory {
    fn create(&self, _: Id, p: &PluginRef, _: f32, _: usize, _: bool) -> Result<Box<dyn Processor>, String> {
        Err(format!("{} is an instrument", p.name))
    }
    fn create_instrument(&self, _: Id, p: &PluginRef, _: f32, _: usize, live: bool) -> Result<Box<dyn Instrument>, String> {
        if p.cid == "missing" {
            return Err(format!("{} is not installed", p.name));
        }
        let level = p.state.as_deref().and_then(|s| s.strip_prefix("level=")).and_then(|v| v.parse().ok()).unwrap_or(1.0);
        if live { &self.live } else { &self.offline }.fetch_add(1, Relaxed);
        Ok(Box::new(Toy { level, on: vec![], events: Arc::clone(&self.events) }))
    }
    fn release(&self, id: Id) {
        self.released.lock().unwrap().push(id);
    }
    fn state(&self, _: Id) -> Option<String> {
        self.state.lock().unwrap().clone()
    }
}

fn plugin(cid: &str, state: Option<&str>) -> PluginRef {
    PluginRef {
        format: "vst3".into(),
        path: "C:\\Program Files\\Common Files\\VST3\\Toy.vst3".into(),
        cid: cid.into(),
        name: "Toy Synth".into(),
        vendor: "Test".into(),
        state: state.map(Into::into),
    }
}

/// An empty project (no master limiter) with one plugin channel and one pattern clip.
fn song(state: Option<&str>, notes: &[(f32, f32, u8)], beats: f64) -> (Project, Id) {
    let mut p = Project::routed();
    p.inserts[0].fx.clear();
    let id = p.alloc_id();
    p.channels.push(Channel::new(id, "Toy", "#9b7cf8", ChannelSrc::Plugin { plugin: plugin("0011", state) }));
    let pid = p.alloc_id();
    p.patterns = vec![Pattern {
        id: pid,
        name: "P".into(),
        color: "#4f8ef7".into(),
        len: beats,
        min_len: beats,
        notes: notes.iter().map(|&(s, l, k)| Note::new(id, s, l, Some(k))).collect(),
        swing: 0.0,
    }];
    let cid = p.alloc_id();
    p.clips = vec![Clip {
        id: cid,
        track: 0,
        start: 0.0,
        len: beats,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: pid,
        muted: false,
        gain: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        pan: 0.0,
        norm: false,
        rev: false,
        tune: None,
        stretch: 1.0,
        resample: false,
        pitch: 0.0,
    }];
    (p, id)
}

fn run(rt: &mut Rt, frames: usize) -> Vec<f32> {
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let mut out = Vec::with_capacity(frames);
    let mut done = 0;
    while done < frames {
        let n = BLOCK.min(frames - done);
        rt.process(&mut l[..n], &mut r[..n], None);
        out.extend_from_slice(&l[..n]);
        done += n;
    }
    out
}

fn first_sound(x: &[f32]) -> Option<usize> {
    x.iter().position(|v| v.abs() > 1e-6)
}

#[test]
fn plugin_channels_round_trip_as_kind_plugin() {
    let (p, id) = song(Some("level=0.5"), &[], 4.0);
    let json = serde_json::to_value(&p).unwrap();
    let ch = json["channels"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
    assert_eq!(ch["kind"], "plugin");
    assert_eq!(ch["plugin"]["cid"], "0011");
    assert_eq!(ch["plugin"]["state"], "level=0.5");
    let back: Project = serde_json::from_value(json).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.channel(id).unwrap().plugin().unwrap().name, "Toy Synth");
}

#[test]
fn put_channel_refuses_formats_and_references_that_cannot_be_hosted() {
    let (p, id) = song(None, &[], 4.0);
    let e = Engine::new(p, Arc::new(Factory::default()));
    let mut c = e.project().channel(id).unwrap().clone();
    let ChannelSrc::Plugin { plugin } = &mut c.src else { unreachable!() };
    plugin.format = "lv2".into();
    let err = e.apply(&[Op::PutChannel { channel: c.clone(), index: None }], false, None).unwrap_err();
    assert!(err.to_string().contains("VST3 only"), "{err}");
    let ChannelSrc::Plugin { plugin } = &mut c.src else { unreachable!() };
    plugin.format = "vst3".into();
    plugin.cid = " ".into();
    assert!(e.apply(&[Op::PutChannel { channel: c, index: None }], false, None).is_err());
}

#[test]
fn a_render_plays_pattern_notes_on_the_offline_instance_sample_accurately() {
    // a note on beat 1 starts exactly one beat in and lasts one beat
    let (p, _) = song(Some("level=0.5"), &[(4.0, 4.0, 60)], 4.0);
    let beat = (60.0 / p.bpm * f64::from(SR)).round() as usize;
    let f = Arc::new(Factory::default());
    let e = Engine::new(p, f.clone());
    let x = e.render(0.0, 4.0, 0.0, SR).unwrap();
    let l: Vec<f32> = x.chunks(2).map(|s| s[0]).collect();
    let on = first_sound(&l).expect("the note sounds");
    assert_eq!(on, beat, "note-on lands on its sample");
    let off = l.iter().rposition(|v| v.abs() > 1e-6).unwrap() + 1;
    assert_eq!(off, 2 * beat, "note-off one beat later");
    assert!((l[on + 100] - 0.5).abs() < 1e-5, "velocity 1 × state level 0.5 at unity: {}", l[on + 100]);
    assert_eq!(f.offline.load(Relaxed), 1, "the render made its own instance");
    assert_eq!(f.live.load(Relaxed), 0, "and did not touch a live one");
}

#[test]
fn the_channel_fader_pan_and_mute_shape_the_instruments_output() {
    let (mut p, id) = song(None, &[(0.0, 4.0, 60)], 4.0);
    let c = p.channels.iter_mut().find(|c| c.id == id).unwrap();
    c.pan = 1.0;
    let x = Engine::new(p.clone(), Arc::new(Factory::default())).render(0.0, 1.0, 0.0, SR).unwrap();
    let mid = (SR as usize / 4) * 2;
    assert!(x[mid].abs() < 1e-6 && x[mid + 1] > 0.9, "hard right: {} {}", x[mid], x[mid + 1]);
    p.channels.iter_mut().find(|c| c.id == id).unwrap().mute = true;
    let x = Engine::new(p, Arc::new(Factory::default())).render(0.0, 1.0, 0.0, SR).unwrap();
    assert!(x.iter().all(|v| v.abs() < 1e-6), "muted");
}

#[test]
fn live_keys_auditions_and_the_wheel_reach_the_live_instance() {
    let (p, id) = song(None, &[], 4.0);
    let f = Arc::new(Factory::default());
    let e = Engine::new(p, f.clone());
    let mut rt = e.create_rt(SR as f32, BLOCK);
    assert_eq!(f.live.load(Relaxed), 1, "a live instance per plugin channel");
    e.midi_configure(Some(id), true);
    assert!(first_sound(&run(&mut rt, BLOCK * 4)).is_none());
    e.midi_event(MidiMsg::NoteOn { ch: 0, key: 60, vel: 127 });
    e.midi_event(MidiMsg::ModWheel { ch: 0, value: 127 });
    e.midi_event(MidiMsg::Sustain { ch: 0, down: true });
    let held = run(&mut rt, BLOCK * 4);
    assert!(held.iter().any(|v| *v > 0.9), "the held key sounds");
    e.midi_event(MidiMsg::NoteOff { ch: 0, key: 60 });
    run(&mut rt, BLOCK);
    let ev = f.events.lock().unwrap().clone();
    assert!(ev.contains(&InstMsg::Control { cc: 1, value: 1.0 }), "{ev:?}");
    assert!(ev.contains(&InstMsg::Control { cc: 64, value: 1.0 }), "the pedal goes to the plugin: {ev:?}");
    assert!(ev.iter().any(|m| matches!(m, InstMsg::NoteOff { key: 60, .. })), "{ev:?}");
    assert!(first_sound(&run(&mut rt, BLOCK)).is_none(), "the toy has no release");
    // a Channel Rack / piano-roll audition
    e.note_audition(id, Some(64), 1.0, None).unwrap();
    assert!(first_sound(&run(&mut rt, BLOCK * 2)).is_some(), "the audition sounds");
}

#[test]
fn instances_follow_the_channel_and_its_plugin_not_its_name_or_state() {
    let (p, id) = song(None, &[], 4.0);
    let f = Arc::new(Factory::default());
    let e = Engine::new(p, f.clone());
    let _rt = e.create_rt(SR as f32, BLOCK);
    assert_eq!(f.live.load(Relaxed), 1);
    // renaming, a new volume or a stale state keep the running instance
    let mut c = e.project().channel(id).unwrap().clone();
    c.name = "Lead".into();
    c.vol = 0.5;
    if let ChannelSrc::Plugin { plugin } = &mut c.src {
        plugin.state = Some("level=0.1".into());
    }
    e.apply(&[Op::PutChannel { channel: c.clone(), index: None }], false, None).unwrap();
    assert_eq!(f.live.load(Relaxed), 1, "same plugin: same instance");
    // another plugin: a new instance, the old one released
    if let ChannelSrc::Plugin { plugin } = &mut c.src {
        plugin.cid = "0022".into();
    }
    e.apply(&[Op::PutChannel { channel: c, index: None }], false, None).unwrap();
    assert_eq!(f.live.load(Relaxed), 2);
    assert_eq!(*f.released.lock().unwrap(), vec![id]);
    // removed, then back by undo
    e.apply(&[Op::RemoveChannel { id }], false, None).unwrap();
    assert_eq!(f.released.lock().unwrap().len(), 2);
    assert!(e.undo());
    assert_eq!(f.live.load(Relaxed), 3, "undo brings an instance back");
}

#[test]
fn saving_stores_the_live_instances_state_in_the_channel() {
    let (p, id) = song(Some("level=0.5"), &[], 4.0);
    let f = Arc::new(Factory::default());
    let e = Engine::new(p, f.clone());
    let _rt = e.create_rt(SR as f32, BLOCK);
    *f.state.lock().unwrap() = Some("level=0.25".into());
    let d = std::env::temp_dir().join(format!("wave-plugin-inst-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("song.wave");
    e.save(&path).unwrap();
    let back = Engine::new(Project::empty(), Arc::new(NoPlugins));
    back.open(&path).unwrap();
    assert_eq!(back.project().channel(id).unwrap().plugin().unwrap().state.as_deref(), Some("level=0.25"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_plugin_that_does_not_load_is_reported_by_channel_and_kept_intact() {
    let (mut p, id) = song(Some("bWlzc2luZw=="), &[(0.0, 4.0, 60)], 4.0);
    if let ChannelSrc::Plugin { plugin } = &mut p.channels.iter_mut().find(|c| c.id == id).unwrap().src {
        plugin.cid = "missing".into();
    }
    let f = Arc::new(Factory::default());
    let e = Engine::new(p, f.clone());
    let mut rt = e.create_rt(SR as f32, BLOCK);
    let err = e.state().fx_errors.get(&id).cloned().expect("an error under the channel id");
    assert!(err.contains("not installed"), "{err}");
    e.play(0.0);
    assert!(first_sound(&run(&mut rt, SR as usize / 2)).is_none(), "silent, not a crash");
    // the render of a project with it is silent for that channel, not an error
    assert!(e.render(0.0, 1.0, 0.0, SR).is_ok());
    let d = std::env::temp_dir().join(format!("wave-plugin-missing-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("song.wave");
    e.save(&path).unwrap();
    let back = Engine::new(Project::empty(), Arc::new(NoPlugins));
    back.open(&path).unwrap();
    assert_eq!(back.project().channel(id).unwrap().plugin().unwrap().state.as_deref(), Some("bWlzc2luZw=="));
    // removing the channel clears its error
    e.apply(&[Op::RemoveChannel { id }], false, None).unwrap();
    assert!(!e.state().fx_errors.contains_key(&id));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_stop_and_replay_ends_every_sounding_note() {
    let (p, _) = song(None, &[(0.0, 64.0, 60)], 16.0);
    let f = Arc::new(Factory::default());
    let e = Engine::new(p, f.clone());
    let mut rt = e.create_rt(SR as f32, BLOCK);
    e.play(0.0);
    assert!(first_sound(&run(&mut rt, BLOCK * 8)).is_some());
    e.play(8.0);
    run(&mut rt, BLOCK * 4);
    let ev = f.events.lock().unwrap().clone();
    assert!(ev.iter().any(|m| matches!(m, InstMsg::NoteOff { key: 60, .. })), "the jump ended the held note: {ev:?}");
    assert!(first_sound(&run(&mut rt, BLOCK * 4)).is_none(), "nothing plays from beat 8");
}
