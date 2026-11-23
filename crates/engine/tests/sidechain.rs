//! Sidechain compression (`Fx::key`): a Compressor that listens to another insert ducks its own
//! insert while that one plays (a kick pumping the bass), live and in a stem, and the routing
//! stays a DAG: keys that would close a loop are refused.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use wave_engine::catalog::new_fx;
use wave_engine::model::*;
use wave_engine::{Engine, NoPlugins, Op, Rt};

const SR: u32 = 48000;

fn insert_id(p: &Project, name: &str) -> Id {
    p.inserts.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no insert {name}")).id
}

/// The sketch song: kick and clap on the Drums insert, a held 808 on the Bass insert.
fn sketch() -> (Project, Id, Id) {
    let p = wave_engine::demo::sketch_project();
    let (drums, bass) = (insert_id(&p, "Drums"), insert_id(&p, "Bass"));
    (p, drums, bass)
}

/// A ducking Compressor on `on`, listening to `key`.
fn duck(id: Id, key: Option<Id>, bpm: f64) -> Fx {
    let mut f = new_fx(id, FxKind::Comp, bpm);
    for (k, v) in [("thr", -40.0), ("ratio", 10.0), ("att", 0.5), ("rel", 60.0), ("gain", 0.0), ("det", 0.0)] {
        f.params.insert(k.into(), v);
    }
    f.key = key;
    f
}

fn put(e: &Engine, on: Id, fx: Fx) {
    e.apply(&[Op::PutFx { insert_id: on, fx, index: None }], false, None).unwrap();
}

/// Mean energy (dB) of the interleaved stereo `x` over the frames whose beat (at `bpm`) has a
/// fractional part in `[a, b)`, over the first `beats` beats.
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

/// How much the bass loses right after each kick (the first 40 ms of a beat, where the sketch's
/// kick hits) and between them (the last fifth of a beat), keyed against not keyed.
fn ducking(dry: &[f32], keyed: &[f32], bpm: f64) -> (f32, f32) {
    let hit = 0.04 / (60.0 / bpm);
    let on = window_db(keyed, bpm, 8.0, 0.0, hit) - window_db(dry, bpm, 8.0, 0.0, hit);
    let off = window_db(keyed, bpm, 8.0, 0.8, 1.0) - window_db(dry, bpm, 8.0, 0.8, 1.0);
    (on, off)
}

#[test]
fn a_keyed_compressor_ducks_its_insert_only_while_the_key_plays_in_a_stem() {
    let (p, drums, bass) = sketch();
    let bpm = p.bpm;
    let e = Engine::new(p, Arc::new(NoPlugins));
    let cancel = AtomicBool::new(false);
    // the same compressor, without a key, hears the bass itself; with one, the drums
    put(&e, bass, duck(7001, None, bpm));
    let own = e.render_stem(bass, 0.0, 8.0, 0.0, SR, &cancel).unwrap();
    e.apply(&[Op::RemoveFx { fx_id: 7001 }], false, None).unwrap();
    let dry = e.render_stem(bass, 0.0, 8.0, 0.0, SR, &cancel).unwrap();
    put(&e, bass, duck(7001, Some(drums), bpm));
    let keyed = e.render_stem(bass, 0.0, 8.0, 0.0, SR, &cancel).unwrap();

    let (on, off) = ducking(&dry, &keyed, bpm);
    assert!(on < -12.0, "the bass ducks on every kick: {on:.1} dB");
    assert!(off > -3.0 && off - on > 10.0, "and comes back between kicks: {off:.1} dB (on the hit {on:.1} dB)");
    // the stem keeps the drums out: what it plays is only the bass, ducked
    assert!(window_db(&keyed, bpm, 8.0, 0.0, 1.0) < window_db(&dry, bpm, 8.0, 0.0, 1.0));
    // not keyed, the same settings flatten the bass evenly: the key makes the difference
    let (own_on, own_off) = ducking(&dry, &own, bpm);
    assert!((own_on - own_off).abs() < (on - off).abs(), "own signal {own_on:.1}/{own_off:.1} vs keyed {on:.1}/{off:.1}");
}

fn run(rt: &mut Rt, blocks: usize) -> Vec<f32> {
    let (mut l, mut r) = (vec![0f32; 256], vec![0f32; 256]);
    let mut out = Vec::with_capacity(blocks * 512);
    for _ in 0..blocks {
        rt.process(&mut l, &mut r, None);
        for i in 0..256 {
            out.push(l[i]);
            out.push(r[i]);
        }
    }
    out
}

/// A "ghost" key, as in FL Studio: the drums go nowhere (not to the master, no sends), yet the
/// compressor still hears them. Live, single- and multi-threaded.
#[test]
fn a_key_that_is_heard_nowhere_still_ducks_live() {
    for workers in [1, 4] {
        let (mut p, drums, bass) = sketch();
        let bpm = p.bpm;
        for m in p.inserts.iter_mut() {
            if m.id == drums {
                m.master = false;
            }
            if m.id == 0 {
                m.fx.clear();
            }
        }
        let e = Engine::new(p, Arc::new(NoPlugins));
        e.set_render_workers(workers);
        let mut rt = e.create_rt(SR as f32, 256);
        let blocks = (8.0 * 60.0 / bpm * SR as f64 / 256.0) as usize;
        let take = |rt: &mut Rt| {
            run(rt, 10);
            let _ = e.stop();
            run(rt, 10);
            e.play(0.0);
            run(rt, blocks)
        };
        put(&e, bass, duck(7001, None, bpm));
        e.apply(&[Op::RemoveFx { fx_id: 7001 }], false, None).unwrap();
        let dry = take(&mut rt);
        put(&e, bass, duck(7002, Some(drums), bpm));
        let keyed = take(&mut rt);
        let (on, off) = ducking(&dry, &keyed, bpm);
        assert!(on < -12.0, "the ghost kick ducks the bass ({workers} workers): {on:.1} dB");
        assert!(off > -3.0, "between kicks the bass is back ({workers} workers): {off:.1} dB");
    }
}

#[test]
fn a_sidechain_must_come_from_another_existing_non_master_insert_without_a_loop() {
    let (p, drums, bass) = sketch();
    let bpm = p.bpm;
    let e = Engine::new(p, Arc::new(NoPlugins));
    let refused = |on: Id, f: Fx| e.apply(&[Op::PutFx { insert_id: on, fx: f, index: None }], false, None).is_err();
    assert!(refused(bass, duck(7001, Some(0), bpm)), "not the master");
    assert!(refused(bass, duck(7001, Some(bass), bpm)), "not its own insert");
    assert!(refused(bass, duck(7001, Some(424242), bpm)), "not a missing insert");
    let mut eq = new_fx(7001, FxKind::Eq, bpm);
    eq.key = Some(drums);
    assert!(refused(bass, eq), "only effects that take a key");

    put(&e, bass, duck(7001, Some(drums), bpm));
    // the drums listening back to the bass would close a loop
    assert!(refused(drums, duck(7002, Some(bass), bpm)), "a key loop");
    // so would a send from the bass into the drums: the drums would wait for themselves
    let mut b = e.project().insert(bass).unwrap().clone();
    b.sends.push(InsertSend { to: drums, gain: 1.0 });
    assert!(e.apply(&[Op::PutInsert { insert: b }], false, None).is_err(), "a send that loops through a key");
    // the other way round is a normal route: the drums feed the bass's insert and key it
    let mut d = e.project().insert(drums).unwrap().clone();
    d.sends.push(InsertSend { to: bass, gain: 0.5 });
    e.apply(&[Op::PutInsert { insert: d }], false, None).unwrap();
}

#[test]
fn removing_the_key_insert_returns_the_compressor_to_its_own_signal_and_undo_brings_it_back() {
    let (p, drums, bass) = sketch();
    let bpm = p.bpm;
    let e = Engine::new(p, Arc::new(NoPlugins));
    put(&e, bass, duck(7001, Some(drums), bpm));
    e.apply(&[Op::RemoveInsert { id: drums }], true, None).unwrap();
    assert_eq!(e.project().find_fx(7001).unwrap().1.key, None);
    assert!(e.undo());
    assert_eq!(e.project().find_fx(7001).unwrap().1.key, Some(drums));
}

#[test]
fn a_key_round_trips_through_the_project_file_and_old_files_have_none() {
    let (mut p, drums, bass) = sketch();
    let f = duck(7001, Some(drums), p.bpm);
    p.inserts.iter_mut().find(|m| m.id == bass).unwrap().fx.push(f);
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains(&format!("\"key\":{drums}")));
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.find_fx(7001).unwrap().1.key, Some(drums));
    // an effect without one writes nothing
    let plain = serde_json::to_string(&new_fx(1, FxKind::Comp, 120.0)).unwrap();
    assert!(!plain.contains("\"key\""));
}

/// Export in place of the bass renders it as it sounds in the mix: still ducked by the kick,
/// although only the bass clips are picked and the drums are not in the result.
#[test]
fn export_in_place_keeps_the_sidechain_its_selection_listens_to() {
    let render = |keyed: bool| {
        let (p, drums, bass) = sketch();
        let bpm = p.bpm;
        let track = p.tracks.iter().position(|t| t.insert == bass).expect("a Bass track") as u32;
        let picked: Vec<Id> = p.clips.iter().filter(|c| c.track == track && c.kind != ClipKind::Auto).map(|c| c.id).collect();
        assert!(!picked.is_empty());
        let dir = std::env::temp_dir().join(format!("wave-sc-inplace-{}-{keyed}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let e = Engine::new(p, Arc::new(NoPlugins));
        e.save(&dir.join("song.wave")).unwrap();
        if keyed {
            put(&e, bass, duck(7001, Some(drums), bpm));
        }
        let r = e.export_in_place(&picked, None, SR, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let p = e.project();
        let src = p.sources.iter().find(|s| s.id == r.source_id).unwrap();
        (
            wave_engine::sources::Audio::load(&dir.join(&src.path))
                .or_else(|_| wave_engine::sources::Audio::load(std::path::Path::new(&src.path)))
                .unwrap()
                .data,
            bpm,
        )
    };
    let ((dry, bpm), (keyed, _)) = (render(false), render(true));
    let (on, off) = ducking(&dry, &keyed, bpm);
    assert!(on < -12.0, "the exported bass is ducked on every kick: {on:.1} dB");
    assert!(off > -3.0, "and comes back between kicks: {off:.1} dB");
}
