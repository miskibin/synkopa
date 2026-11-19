//! Mix assistant (`wave_engine::masking`): the masking detector, the sidechained dynamic EQ
//! that fixes a collision, and the processing order a sidechain imposes.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use wave_engine::masking::{fix_fx, Collision};
use wave_engine::model::*;
use wave_engine::{Engine, NoPlugins, Op};

fn engine(p: Project) -> Arc<Engine> {
    Engine::new(p, Arc::new(NoPlugins))
}

fn scan(e: &Engine, from: f64, to: f64) -> Vec<Collision> {
    e.masking_scan(from, to, &AtomicBool::new(false)).unwrap()
}

fn insert_id(p: &Project, name: &str) -> Id {
    p.inserts.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no insert {name}")).id
}

/// Mean energy (dB) of `x` (mono) in a band, over the frames where `mask` is set.
fn band_db(x: &[f32], sr: f32, f0: f32, q: f32, mask: &dyn Fn(usize) -> bool) -> f32 {
    // a band-pass biquad (RBJ, constant 0 dB peak gain)
    let w = std::f32::consts::TAU * f0 / sr;
    let alpha = w.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let (b0, b2, a1, a2) = (alpha / a0, -alpha / a0, -2.0 * w.cos() / a0, (1.0 - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0f32, 0f32, 0f32, 0f32);
    let (mut e, mut n) = (0f64, 0usize);
    for (i, &v) in x.iter().enumerate() {
        let y = b0 * v + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = v;
        y2 = y1;
        y1 = y;
        if mask(i) {
            e += (y * y) as f64;
            n += 1;
        }
    }
    10.0 * ((e / n.max(1) as f64).max(1e-20)).log10() as f32
}

fn mono(st: &[f32]) -> Vec<f32> {
    st.chunks(2).map(|c| 0.5 * (c[0] + c[1])).collect()
}

/// Real material: the demo song (Obsidian 2093). Its 808 and its kick are separate inserts that
/// meet only on the Beat Bus, and they share the sub range on every downbeat.
#[test]
fn the_demo_songs_808_masks_its_kick_and_the_fix_clears_it() {
    let p = wave_engine::demo::demo_project();
    let (kick, b808) = (insert_id(&p, "Kick"), insert_id(&p, "808"));
    let e = engine(p);
    // 16 bars where both play
    let (from, to) = (64.0, 128.0);
    let found = scan(&e, from, to);
    for c in &found {
        eprintln!("{c:?}");
    }
    let c = found.iter().find(|c| (c.masker, c.protect) == (b808, kick)).expect("808 over kick reported, 808 gives way").clone();
    assert!(c.lo >= 20.0 && c.hi <= 250.0, "a sub/bass range, got {}..{} Hz", c.lo, c.hi);
    assert!(!found.iter().any(|x| x.masker == kick && x.protect == b808), "never both directions");

    // before/after: the 808 alone, measured in the collision band while the kick hits and between
    let sr: f32 = 48000.0;
    let kick_alone = mono(&e.render_stem(kick, from, to, 0.0, 48000, &AtomicBool::new(false)).unwrap());
    let before = mono(&e.render_stem(b808, from, to, 0.0, 48000, &AtomicBool::new(false)).unwrap());
    let next = e.project().next_id;
    let fix = fix_fx(next, &c);
    e.apply(&[Op::PutFx { insert_id: b808, fx: fix, index: None }], false, None).unwrap();
    let after = mono(&e.render_stem(b808, from, to, 0.0, 48000, &AtomicBool::new(false)).unwrap());

    // where the kick is loud (its envelope within 10 dB of its peak) vs where it is silent
    let env: Vec<f32> = {
        let mut a = 0f32;
        let k = (-1.0 / (sr * 0.01)).exp();
        kick_alone
            .iter()
            .map(|&x| {
                a = (x * x).max(a * k);
                a
            })
            .collect()
    };
    let peak = env.iter().copied().fold(0.0, f32::max);
    let on = |i: usize| env.get(i).is_some_and(|&v| v > peak * 0.1);
    let off = |i: usize| env.get(i).is_some_and(|&v| v < peak * 1e-4);
    let f = c.fix();
    let (q, f0) = (f.q as f32, f.freq as f32);
    let duck_hit = band_db(&before, sr, f0, q, &on) - band_db(&after, sr, f0, q, &on);
    let duck_gap = band_db(&before, sr, f0, q, &off) - band_db(&after, sr, f0, q, &off);
    let strong = |i: usize| env.get(i).is_some_and(|&v| v > peak * 0.5);
    let duck_peak = band_db(&before, sr, f0, q, &strong) - band_db(&after, sr, f0, q, &strong);
    eprintln!(
        "808 at {f0:.0} Hz (Q {q:.2}): −{duck_peak:.1} dB at kick peaks, −{duck_hit:.1} dB on kick hits, −{duck_gap:.2} dB between them"
    );
    assert!(duck_hit > 3.0, "the 808 makes room while the kick hits: {duck_hit} dB");
    // between hits only the 25 ms release is left of it
    assert!(duck_gap < 0.5, "and stays itself between hits: {duck_gap} dB");

    // the handled pair is not reported again
    let again = scan(&e, from, to);
    assert!(!again.iter().any(|x| [x.masker, x.protect].contains(&kick) && [x.masker, x.protect].contains(&b808)));
}

/// The empty project: inserts 1 (Kick) and 2 (Clap) at indices 1 and 2, more after them.
fn two_inserts() -> Project {
    let p = Project::empty();
    assert_eq!((p.inserts[1].id, p.inserts[2].id), (1, 2));
    p
}

#[test]
fn a_sidechain_must_come_from_another_existing_non_master_insert_without_a_loop() {
    let e = engine(two_inserts());
    let eq = |id, key| {
        let mut f = wave_engine::catalog::new_fx(id, FxKind::Eq, 120.0);
        f.key = key;
        f
    };
    assert!(e.apply(&[Op::PutFx { insert_id: 2, fx: eq(e.project().next_id, Some(0)), index: None }], false, None).is_err(), "master");
    assert!(e.apply(&[Op::PutFx { insert_id: 2, fx: eq(e.project().next_id, Some(2)), index: None }], false, None).is_err(), "itself");
    assert!(e.apply(&[Op::PutFx { insert_id: 2, fx: eq(e.project().next_id, Some(999)), index: None }], false, None).is_err(), "missing");
    let id = e.project().next_id;
    let mut comp = wave_engine::catalog::new_fx(id, FxKind::Comp, 120.0);
    comp.key = Some(1);
    assert!(e.apply(&[Op::PutFx { insert_id: 2, fx: comp, index: None }], false, None).is_err(), "only an EQ");
    e.apply(&[Op::PutFx { insert_id: 2, fx: eq(e.project().next_id, Some(1)), index: None }], false, None).unwrap();
    // the key is processed first
    let o = wave_engine::routing::order(&e.project());
    assert!(o.level[2] > o.level[1], "levels {:?}", o.level);
    assert_eq!(o.keys, vec![(1, 2)]);
    // a send from the listener into its key would close a loop: the sidechain is dropped,
    // the send stays (the project is still valid and plays)
    let mut m = e.project().insert(2).cloned().unwrap();
    m.sends.push(InsertSend { to: 1, gain: 1.0 });
    e.apply(&[Op::PutInsert { insert: m }], false, None).unwrap();
    let o = wave_engine::routing::order(&e.project());
    assert!(o.keys.is_empty());
    assert_eq!(o.sends[2], vec![(1, 1.0)]);
    // removing the key's insert removes the sidechain
    e.apply(&[Op::RemoveInsert { id: 1 }], false, None).unwrap();
    assert_eq!(e.project().find_fx(id).unwrap().1.key, None);
}

#[test]
fn ignored_pairs_are_kept_sorted_unique_and_dropped_with_their_insert() {
    let e = engine(two_inserts());
    e.apply(&[Op::SetMaskIgnore { pairs: vec![[2, 1], [1, 2]] }], false, None).unwrap();
    assert_eq!(e.project().mask_ignore, vec![[1, 2]]);
    assert!(e.apply(&[Op::SetMaskIgnore { pairs: vec![[0, 1]] }], false, None).is_err());
    assert!(e.apply(&[Op::SetMaskIgnore { pairs: vec![[1, 1]] }], false, None).is_err());
    assert!(!wave_engine::masking::compared_pairs(&e.project()).contains(&(1, 2)));
    e.apply(&[Op::RemoveInsert { id: 2 }], false, None).unwrap();
    assert!(e.project().mask_ignore.is_empty());
}

#[test]
fn inserts_that_feed_each_other_are_never_compared() {
    let mut p = two_inserts();
    p.inserts[2].sends.push(InsertSend { to: 1, gain: 1.0 });
    p.inserts[2].master = false;
    assert!(!wave_engine::masking::compared_pairs(&p).contains(&(1, 2)));
    p.inserts[2].sends.clear();
    p.inserts[2].master = true;
    assert!(wave_engine::masking::compared_pairs(&p).contains(&(1, 2)));
}

/// The live path: the stream taps every insert while playing, the housekeeping thread analyses
/// what arrives, and the engine reports it; switched off, nothing is reported.
#[test]
fn a_live_stream_reports_what_it_played_and_stops_when_switched_off() {
    let p = wave_engine::demo::demo_project();
    let (kick, b808) = (insert_id(&p, "Kick"), insert_id(&p, "808"));
    let e = engine(p);
    let mut rt = e.create_rt(48000.0, 512);
    let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
    e.play(64.0);
    // 28 s of audio (the 16 bars the offline test scans), paced so the housekeeping thread (60 Hz) drains the taps
    for blk in 0..(28 * 48000 / 512) {
        rt.process(&mut l, &mut r, None);
        if blk % 8 == 7 {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let found = e.collisions();
    assert!(found.iter().any(|c| (c.masker, c.protect) == (b808, kick)), "{found:?}");
    e.set_mix_assistant(false);
    assert!(e.collisions().is_empty());
    assert!(!e.mix_assistant());
}

/// Listening material for a review (not a check): the demo's 16 bars before and after the fix,
/// the full mix and the 808 alone. `MIX_ASSIST_OUT=dir cargo test -p wave-engine --test
/// mix_assistant -- --ignored`.
#[test]
#[ignore]
fn write_before_after_renders() {
    let Ok(dir) = std::env::var("MIX_ASSIST_OUT") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let p = wave_engine::demo::demo_project();
    let (kick, b808) = (insert_id(&p, "Kick"), insert_id(&p, "808"));
    let e = engine(p);
    let (from, to) = (64.0, 128.0);
    let no = AtomicBool::new(false);
    let write = |name: &str, x: &[f32]| wave_engine::sources::write_wav(&dir.join(name), x, 48000).unwrap();
    write("mix-before.wav", &e.render(from, to, 1.0, 48000).unwrap());
    write("808-before.wav", &e.render_stem(b808, from, to, 1.0, 48000, &no).unwrap());
    write("kick.wav", &e.render_stem(kick, from, to, 1.0, 48000, &no).unwrap());
    let c = scan(&e, from, to).into_iter().find(|c| (c.masker, c.protect) == (b808, kick)).unwrap();
    eprintln!("{c:?} -> {:?}", c.fix());
    let next = e.project().next_id;
    e.apply(&[Op::PutFx { insert_id: b808, fx: fix_fx(next, &c), index: None }], false, None).unwrap();
    write("mix-after.wav", &e.render(from, to, 1.0, 48000).unwrap());
    write("808-after.wav", &e.render_stem(b808, from, to, 1.0, 48000, &no).unwrap());
}
