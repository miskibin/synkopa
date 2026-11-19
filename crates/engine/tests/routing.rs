//! Insert-to-insert routing heard through the real-time path: a bus fed by sends, send levels,
//! "route to this insert only", loops refused, solo along the routing. Single- and
//! multi-threaded (the inserts of one routing level run on the pool).

use std::sync::Arc;
use wave_engine::model::InsertSend;
use wave_engine::*;

const SR: f32 = 48000.0;
const BLOCK: usize = 256;

fn peak(rt: &mut Rt, blocks: usize) -> f32 {
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let mut p = 0f32;
    for _ in 0..blocks {
        rt.process(&mut l, &mut r, None);
        p = l.iter().chain(r.iter()).fold(p, |a, x| a.max(x.abs()));
    }
    p
}

/// The peak of the song's first bars, from the start each time (the same music for every
/// measurement), after a few blocks for the faders' smoothing.
fn level(e: &Engine, rt: &mut Rt) -> f32 {
    peak(rt, 20);
    let _ = e.stop();
    peak(rt, 20);
    e.play(0.0);
    peak(rt, 400)
}

/// Every insert but the master and `bus` sends to `bus` at `gain`, not to the master; the
/// master's own chain is cleared so levels compare linearly.
fn route_all(e: &Engine, bus: Id, gain: f32) -> Vec<Op> {
    // the bus first: a new insert nothing plays into directly
    let mut b = e.project().inserts[0].clone();
    (b.id, b.name, b.fx, b.vol) = (bus, "Bus".into(), vec![], VOL_UNITY);
    // the master's chain (a limiter) out: `PutInsert` keeps an insert's effects
    let master_fx: Vec<Op> = e.project().inserts[0].fx.iter().map(|f| Op::RemoveFx { fx_id: f.id }).collect();
    master_fx
        .into_iter()
        .chain(std::iter::once(Op::PutInsert { insert: b }))
        .chain(e.project().inserts.into_iter().map(|mut m| {
            if m.id == 0 {
                // well below the output's safety clipper, so levels compare linearly
                m.vol = 0.25;
            } else {
                m.sends = vec![InsertSend { to: bus, gain }];
                m.master = false;
            }
            Op::PutInsert { insert: m }
        }))
        .collect()
}

/// The id the bus gets (a new insert, see [`route_all`]).
fn bus_of(_: &Engine) -> Id {
    9000
}

fn with<F: FnOnce(&mut model::Insert)>(e: &Engine, id: Id, f: F) -> Op {
    let mut m = e.project().inserts.into_iter().find(|m| m.id == id).unwrap();
    f(&mut m);
    Op::PutInsert { insert: m }
}

#[test]
fn a_bus_carries_what_is_routed_to_it_and_its_level_scales_it() {
    for workers in [1, 4] {
        let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
        e.set_render_workers(workers);
        let mut rt = e.create_rt(SR, BLOCK);
        let bus = bus_of(&e);
        e.apply(&route_all(&e, bus, 1.0), false, None).unwrap();
        e.play(0.0);
        let full = level(&e, &mut rt);
        assert!(full > 0.01, "the song plays through the bus ({workers} workers): {full}");

        // the bus muted: everything is routed only there, so nothing is heard
        e.apply(&[with(&e, bus, |m| m.muted = true)], false, None).unwrap();
        let muted = level(&e, &mut rt);
        assert!(muted < 1e-4, "a muted bus silences what goes only into it ({workers} workers): {muted}");
        e.apply(&[with(&e, bus, |m| m.muted = false)], false, None).unwrap();

        // half the send level: half the signal (the bus and the master have no effects)
        let ops = e.project().inserts.into_iter().filter(|m| m.id != 0 && m.id != bus).map(|mut m| {
            m.sends[0].gain = 0.5;
            Op::PutInsert { insert: m }
        });
        e.apply(&ops.collect::<Vec<_>>(), false, None).unwrap();
        let half = level(&e, &mut rt);
        assert!((half / full - 0.5).abs() < 0.02, "send at 0.5 halves the level ({workers} workers): {half} vs {full}");
    }
}

#[test]
fn a_send_level_is_a_fast_path_change_and_rewiring_rebuilds_the_graph() {
    let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
    let bus = e.project().inserts.last().unwrap().id;
    let src = e.project().inserts[1].id;
    let d = ops::apply_all(&mut e.project(), &[with(&e, src, |m| m.sends.push(InsertSend { to: bus, gain: 1.0 }))]).unwrap();
    assert!(d.mixer, "a new send rewires the mixer");
    e.apply(&[with(&e, src, |m| m.sends.push(InsertSend { to: bus, gain: 1.0 }))], false, None).unwrap();
    let d = ops::apply_all(&mut e.project(), &[with(&e, src, |m| m.sends[0].gain = 0.3)]).unwrap();
    assert!(!d.mixer && d.params, "a send level only touches parameters");
}

#[test]
fn a_send_that_would_loop_is_refused() {
    let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
    let (a, b) = (e.project().inserts[1].id, e.project().inserts[2].id);
    e.apply(&[with(&e, a, |m| m.sends.push(InsertSend { to: b, gain: 1.0 }))], false, None).unwrap();
    let err = e.apply(&[with(&e, b, |m| m.sends.push(InsertSend { to: a, gain: 1.0 }))], false, None).unwrap_err();
    assert!(err.to_string().contains("loop"), "{err}");
    assert!(e.project().inserts[2].sends.is_empty(), "nothing changed");
}

#[test]
fn soloing_the_bus_keeps_what_feeds_it_and_soloing_an_unrelated_insert_does_not() {
    let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
    e.set_render_workers(4);
    let mut rt = e.create_rt(SR, BLOCK);
    let bus = bus_of(&e);
    e.apply(&route_all(&e, bus, 1.0), false, None).unwrap();
    e.play(0.0);
    let full = level(&e, &mut rt);
    e.apply(&[with(&e, bus, |m| m.solo = true)], false, None).unwrap();
    let solo_bus = level(&e, &mut rt);
    assert!((solo_bus / full - 1.0).abs() < 0.02, "the soloed bus still hears its sources: {solo_bus} vs {full}");
    // a fresh, unrouted insert soloed instead: the bus and everything into it go quiet
    let lone = 9001;
    let mut fresh = e.project().inserts.into_iter().find(|m| m.id == bus).unwrap();
    fresh.id = lone;
    fresh.solo = true;
    fresh.sends.clear();
    fresh.master = true;
    e.apply(&[with(&e, bus, |m| m.solo = false), Op::PutInsert { insert: fresh }], false, None).unwrap();
    let other = level(&e, &mut rt);
    assert!(other < 1e-4, "solo elsewhere silences the bus path: {other}");
}

#[test]
fn removing_the_bus_brings_its_sources_back_to_the_master() {
    let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
    let mut rt = e.create_rt(SR, BLOCK);
    let bus = bus_of(&e);
    e.apply(&route_all(&e, bus, 1.0), false, None).unwrap();
    e.apply(&[Op::RemoveInsert { id: bus }], false, None).unwrap();
    assert!(e.project().inserts.iter().all(|m| m.master && m.sends.is_empty()), "no insert left routed nowhere");
    e.play(0.0);
    assert!(level(&e, &mut rt) > 0.01, "the song still plays");
}

#[test]
fn routing_survives_a_save_and_old_files_open_unrouted() {
    let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
    let bus = bus_of(&e);
    e.apply(&route_all(&e, bus, 0.7), false, None).unwrap();
    let json = serde_json::to_string(&e.project()).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.inserts, e.project().inserts);
    // an insert written before routing existed: no `sends`, no `master`
    let old = r##"{"id":3,"name":"Bass","color":"#fff","vol":0.8,"pan":0,"muted":false,"solo":false,"fx":[]}"##;
    let m: model::Insert = serde_json::from_str(old).unwrap();
    assert!(m.master && m.sends.is_empty());
    assert!(!serde_json::to_string(&m).unwrap().contains("sends"), "an unrouted insert writes nothing new");
}

#[test]
fn delay_compensation_follows_the_routing() {
    // two inserts with a look-ahead limiter; one sends only into the other: their latencies add up
    let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
    let (a, b) = (e.project().inserts[1].id, e.project().inserts[2].id);
    let mut ops: Vec<Op> = e.project().inserts.iter().flat_map(|m| m.fx.iter().map(|f| Op::RemoveFx { fx_id: f.id })).collect();
    ops.push(Op::PutFx { insert_id: a, fx: catalog::new_fx(9101, model::FxKind::Limit, 120.0), index: None });
    ops.push(Op::PutFx { insert_id: b, fx: catalog::new_fx(9102, model::FxKind::Limit, 120.0), index: None });
    e.apply(&ops, false, None).unwrap();
    let one = wave_engine::engine::playback_latency(&e.project(), SR);
    assert!(one > 0, "a look-ahead limiter has latency");
    // in parallel to the master: the slowest path, as before routing existed
    // in series (a → b → master): both chains are on one path
    e.apply(
        &[with(&e, a, |m| {
            m.sends = vec![InsertSend { to: b, gain: 1.0 }];
            m.master = false;
        })],
        false,
        None,
    )
    .unwrap();
    assert_eq!(wave_engine::engine::playback_latency(&e.project(), SR), 2 * one, "in series the latencies add up");
    // a also straight to the master: that path is shorter and gets delayed to meet the long one
    e.apply(&[with(&e, a, |m| m.master = true)], false, None).unwrap();
    assert_eq!(wave_engine::engine::playback_latency(&e.project(), SR), 2 * one);
}
