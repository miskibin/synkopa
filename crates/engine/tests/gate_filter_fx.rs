//! Gate and Filter as mixer effects: keyed by another insert (a trance gate chopping a held
//! bass on the kick, a filter pumped by the kick), look-ahead compensated against the other
//! tracks, saved and loaded, and bounced offline deterministically.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use wave_engine::catalog::{latency, load_preset, new_fx, presets};
use wave_engine::model::*;
use wave_engine::{Engine, NoPlugins, Op, RenderTail};

const SR: u32 = 48000;

fn insert_id(p: &Project, name: &str) -> Id {
    p.inserts.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no insert {name}")).id
}

fn with_preset(kind: FxKind, id: Id, name: &str) -> Fx {
    let mut f = new_fx(id, kind, 120.0);
    let i = presets(kind).iter().position(|p| p.name == name).unwrap_or_else(|| panic!("no preset {name}"));
    assert!(load_preset(&mut f, i, 120.0));
    f
}

fn put(e: &Engine, on: Id, fx: Fx) {
    e.apply(&[Op::PutFx { insert_id: on, fx, index: None }], false, None).unwrap();
}

/// Mean energy (dB) of interleaved stereo `x` over the frames whose beat has a fractional part
/// in `[a, b)`, over the first `beats` beats.
fn window_db(x: &[f32], bpm: f64, beats: f64, a: f64, b: f64) -> f32 {
    let spb = 60.0 / bpm * SR as f64;
    let (mut e, mut n) = (0f64, 0usize);
    for (i, c) in x.chunks(2).enumerate() {
        let beat = i as f64 / spb;
        if beat >= beats {
            break;
        }
        let f = beat.fract();
        if f >= a && f < b {
            let m = 0.5 * (c[0] + c[1]) as f64;
            e += m * m;
            n += 1;
        }
    }
    10.0 * (e / n.max(1) as f64).max(1e-20).log10() as f32
}

#[test]
fn a_keyed_gate_opens_the_bass_only_on_the_kick() {
    let p = wave_engine::demo::sketch_project();
    let (drums, bass, bpm) = (insert_id(&p, "Drums"), insert_id(&p, "Bass"), p.bpm);
    let e = Engine::new(p, Arc::new(NoPlugins));
    let cancel = AtomicBool::new(false);
    let dry = e.render_stem(bass, 0.0, 8.0, 0.0, SR, &cancel).unwrap();
    let mut g = with_preset(FxKind::Gate, 7001, "Trance gate (sidechain)");
    // the sketch's drums also have off-beat claps and hats: the side-chain high cut keeps the
    // gate on the kick, as a producer would set it
    g.params.insert("sclp".into(), 200.0);
    g.params.insert("thr".into(), -15.0);
    g.key = Some(drums);
    put(&e, bass, g);
    let gated = e.render_stem(bass, 0.0, 8.0, 0.0, SR, &cancel).unwrap();
    // the sketch's kick lands at the top of each beat: the bass is there, and gone in between
    let hit = 0.04 / (60.0 / bpm);
    let on = window_db(&gated, bpm, 8.0, 0.0, hit) - window_db(&dry, bpm, 8.0, 0.0, hit);
    let off = window_db(&gated, bpm, 8.0, 0.8, 1.0) - window_db(&dry, bpm, 8.0, 0.8, 1.0);
    assert!(on > -6.0, "the bass plays with the kick: {on:.1} dB");
    assert!(off < -30.0, "and is shut between kicks: {off:.1} dB");
}

#[test]
fn a_keyed_filter_follows_the_kick_and_duck_mode_inverts_the_gate() {
    let p = wave_engine::demo::sketch_project();
    let (drums, bass, bpm) = (insert_id(&p, "Drums"), insert_id(&p, "Bass"), p.bpm);
    let e = Engine::new(p, Arc::new(NoPlugins));
    let cancel = AtomicBool::new(false);
    let dry = e.render_stem(bass, 0.0, 8.0, 0.0, SR, &cancel).unwrap();
    // a low-pass that closes on each kick: the bass darkens and drops on the hits
    let mut f = with_preset(FxKind::Filter, 7001, "Pompowanie (sidechain od stopy)");
    // the preset is set for pads (it closes to ~600 Hz); for an 808 it has to reach lower
    f.params.insert("cutoff".into(), 800.0);
    f.params.insert("env".into(), -6.0);
    f.key = Some(drums);
    put(&e, bass, f);
    let pumped = e.render_stem(bass, 0.0, 8.0, 0.0, SR, &cancel).unwrap();
    assert!(pumped.iter().all(|v| v.is_finite()));
    let hit = 0.06 / (60.0 / bpm);
    let on = window_db(&pumped, bpm, 8.0, 0.01, hit) - window_db(&dry, bpm, 8.0, 0.01, hit);
    let off = window_db(&pumped, bpm, 8.0, 0.8, 1.0) - window_db(&dry, bpm, 8.0, 0.8, 1.0);
    assert!(on < off - 6.0, "the kick closes the filter: {on:.1} dB on the hit vs {off:.1} dB between");
    e.apply(&[Op::RemoveFx { fx_id: 7001 }], false, None).unwrap();
    // a ducking gate keyed by the drums: the opposite of the trance gate
    let mut d = with_preset(FxKind::Gate, 7002, "Ducking — bas pod stopę");
    d.key = Some(drums);
    put(&e, bass, d);
    let ducked = e.render_stem(bass, 0.0, 8.0, 0.0, SR, &cancel).unwrap();
    let hit = 0.04 / (60.0 / bpm);
    let on = window_db(&ducked, bpm, 8.0, 0.0, hit) - window_db(&dry, bpm, 8.0, 0.0, hit);
    let off = window_db(&ducked, bpm, 8.0, 0.8, 1.0) - window_db(&dry, bpm, 8.0, 0.8, 1.0);
    assert!(on < -6.0 && off > -1.5, "ducked {on:.1} dB on the kick, {off:.1} dB between");
}

#[test]
fn look_ahead_is_reported_and_compensated() {
    let mut p = Project::routed();
    for m in p.inserts.iter_mut() {
        m.fx.clear();
    }
    let e = Engine::new(p, Arc::new(NoPlugins));
    let ins = e.project().tracks[2].insert;
    for (look, ms) in [(0.0, 0.0), (1.0, 1.0), (4.0, 10.0)] {
        let mut g = new_fx(7001, FxKind::Gate, 120.0);
        g.params.insert("look".into(), look);
        let want = (ms / 1000.0 * SR as f64).round() as usize;
        assert_eq!(latency(&g, SR as f32), want);
        g.bypass = true;
        assert_eq!(latency(&g, SR as f32), 0, "a bypassed gate adds nothing");
        g.bypass = false;
        put(&e, ins, g);
        e.apply(&[Op::RemoveFx { fx_id: 7001 }], false, None).unwrap();
    }
    // the filter has no latency
    assert_eq!(latency(&new_fx(1, FxKind::Filter, 120.0), SR as f32), 0);
}

#[test]
fn both_survive_a_save_and_load_and_bounce_the_same_twice() {
    let p = wave_engine::demo::sketch_project();
    let (drums, bass) = (insert_id(&p, "Drums"), insert_id(&p, "Bass"));
    let e = Engine::new(p, Arc::new(NoPlugins));
    let mut g = with_preset(FxKind::Gate, 7001, "Werbel — bez przesłuchów");
    g.params.insert("look".into(), 3.0);
    let mut f = with_preset(FxKind::Filter, 7002, "S&H — losowe kroki 1/16");
    f.key = Some(drums);
    put(&e, drums, g);
    put(&e, bass, f);
    let p = e.project();
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"type\":\"gate\"") && json.contains("\"type\":\"filter\""));
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.insert(bass).unwrap().fx, p.insert(bass).unwrap().fx);
    assert_eq!(back.insert(drums).unwrap().fx, p.insert(drums).unwrap().fx);
    let cancel = AtomicBool::new(false);
    let a = e.render_export(None, 0.0, 8.0, RenderTail::Seconds(1.0), SR, &cancel).unwrap();
    assert!(a.iter().all(|v| v.is_finite()));
    let b = e.render_export(None, 0.0, 8.0, RenderTail::Seconds(1.0), SR, &cancel).unwrap();
    assert_eq!(a, b, "nothing random, no state carried between bounces (S&H follows the song)");
}

/// Listening renders for review (not run by default): `cargo test -p wave-engine --test
/// gate_filter_fx -- --ignored renders` writes WAVs to `demo-out/gate-filter/`.
#[test]
#[ignore]
fn renders() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo-out/gate-filter");
    std::fs::create_dir_all(&out).unwrap();
    let cancel = AtomicBool::new(false);
    let take = |name: &str, setup: &dyn Fn(&Engine, Id, Id)| {
        let p = wave_engine::demo::sketch_project();
        let (drums, bass) = (insert_id(&p, "Drums"), insert_id(&p, "Bass"));
        let e = Engine::new(p, Arc::new(NoPlugins));
        setup(&e, drums, bass);
        let x = e.render_export(None, 0.0, 16.0, RenderTail::Seconds(0.5), SR, &cancel).unwrap();
        wave_engine::sources::write_wav(&out.join(format!("{name}.wav")), &x, SR).unwrap();
    };
    take("0-dry", &|_, _, _| {});
    take("1-trance-gate-bass-on-kick", &|e, drums, bass| {
        let mut g = with_preset(FxKind::Gate, 7001, "Trance gate (sidechain)");
        g.params.insert("sclp".into(), 200.0);
        g.params.insert("thr".into(), -15.0);
        g.key = Some(drums);
        put(e, bass, g);
    });
    take("2-dub-filter-on-drums", &|e, drums, _| put(e, drums, with_preset(FxKind::Filter, 7001, "Dub — rezonans stereo")));
    take("3-acid-on-bass", &|e, _, bass| put(e, bass, with_preset(FxKind::Filter, 7001, "Acid — 303")));
    take("4-hp-buildup-on-master", &|e, _, _| put(e, 0, with_preset(FxKind::Filter, 7001, "Narastanie — high-pass (4 takty)")));
}
