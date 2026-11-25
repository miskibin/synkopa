//! Mix assistant (`wave_engine::masking`): the masking detector and its fix, an ordinary EQ whose
//! dynamic band is keyed from the protected insert through the standard sidechain (`Fx::key`).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use wave_engine::masking::{fix_fx, Collision, Detector};
use wave_engine::model::*;
use wave_engine::{Engine, NoPlugins, Op, ProcessCtx, Processor};

const SR: f32 = 48000.0;

fn engine(p: Project) -> Arc<Engine> {
    Engine::new(p, Arc::new(NoPlugins))
}

fn scan(e: &Engine, from: f64, to: f64) -> Vec<Collision> {
    e.masking_scan(from, to, &AtomicBool::new(false)).unwrap()
}

fn insert_id(p: &Project, name: &str) -> Id {
    p.inserts.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("no insert {name}")).id
}

/// The empty project: inserts 1 and 2 at indices 1 and 2, more after them.
fn two_inserts() -> Project {
    let p = Project::empty();
    assert_eq!((p.inserts[1].id, p.inserts[2].id), (1, 2));
    p
}

// ---- deterministic test signals (120 BPM: a kick every 0.5 s)

const BEAT: usize = SR as usize / 2;

/// An 808-style kick: a sine sweeping 90 → 48 Hz in 40 ms, decaying with a 120 ms time constant,
/// on every beat. Frame `i` of the song.
fn kick(i: usize) -> f32 {
    let t = (i % BEAT) as f32 / SR;
    // phase of the exponential sweep f(t) = 48 + 42·e^(−t/0.01)
    let ph = std::f32::consts::TAU * (48.0 * t + 42.0 * 0.01 * (1.0 - (-t / 0.01).exp()));
    0.8 * ph.sin() * (-t / 0.12).exp()
}

/// A held 808 bass: 50 Hz, steady, a little under the kick's peak.
fn bass(i: usize) -> f32 {
    0.3 * (std::f32::consts::TAU * 50.0 * i as f32 / SR).sin()
}

/// A hi-hat stand-in: an 8 kHz tone burst on the off-beats, nowhere near the bass.
fn hat(i: usize) -> f32 {
    let t = ((i + BEAT / 2) % BEAT) as f32 / SR;
    0.3 * (std::f32::consts::TAU * 8000.0 * i as f32 / SR).sin() * (-t / 0.03).exp()
}

/// Feeds `seconds` of three lanes (1 = kick, 2 = bass, 3 = hat) to a detector configured for `p`.
fn listen(p: &Project, seconds: f32) -> Detector {
    let mut d = Detector::new(SR);
    d.configure(p);
    let n = (seconds * SR) as usize;
    let block = 512;
    let mut buf = vec![0f32; block];
    for at in (0..n).step_by(block) {
        for (lane, f) in [(1usize, kick as fn(usize) -> f32), (2, bass), (3, hat)] {
            for (j, x) in buf.iter_mut().enumerate() {
                *x = f(at + j);
            }
            d.push(lane, &buf);
        }
        // the other lanes hear silence
        buf.fill(0.0);
        for lane in 4..p.inserts.len() {
            d.push(lane, &buf);
        }
    }
    d
}

#[test]
fn a_held_808_under_a_kick_is_reported_after_a_few_seconds_and_the_808_gives_way() {
    let p = two_inserts();
    // 2 s: still warming up, nothing yet (one loud bar is not a problem)
    assert!(listen(&p, 2.0).collisions().is_empty());
    let found = listen(&p, 8.0).collisions();
    let c = found.iter().find(|c| [c.masker, c.protect] == [2, 1]).unwrap_or_else(|| panic!("808 over kick: {found:?}"));
    assert!(c.lo >= 25.0 && c.hi <= 120.0 && c.lo < 50.0 && c.hi > 50.0, "the sub range, got {}..{} Hz", c.lo, c.hi);
    assert!(c.overlap > 0.4, "most of the time: {}", c.overlap);
    // the hat shares nothing with either
    assert!(!found.iter().any(|c| c.masker == 3 || c.protect == 3), "{found:?}");
    assert!(!found.iter().any(|c| [c.masker, c.protect] == [1, 2]), "never both directions");
}

#[test]
fn ignored_handled_and_related_pairs_are_not_reported() {
    let mut p = two_inserts();
    p.mask_ignore = vec![[1, 2]];
    assert!(listen(&p, 8.0).collisions().is_empty());
    // the kick goes into the 808's insert: one contains the other
    let mut p = two_inserts();
    p.inserts[1].sends.push(InsertSend { to: 2, gain: 1.0 });
    assert!(listen(&p, 8.0).collisions().is_empty());
    // the 808 already ducks under the kick: a keyed Compressor (classic sidechain), or the fix
    let mut p = two_inserts();
    let mut comp = wave_engine::catalog::new_fx(900, FxKind::Comp, 120.0);
    comp.key = Some(1);
    p.inserts[2].fx.push(comp.clone());
    assert!(listen(&p, 8.0).collisions().is_empty());
    comp.bypass = true;
    p.inserts[2].fx = vec![comp];
    assert!(!listen(&p, 8.0).collisions().is_empty(), "a bypassed one does not count");
}

/// The fix, sample by sample on the deterministic signals: the 808's band goes down while the
/// kick hits and stays where it was between hits.
#[test]
fn the_fix_ducks_the_808_only_while_the_kick_hits() {
    let c = listen(&two_inserts(), 8.0).collisions().into_iter().find(|c| c.masker == 2).expect("a collision");
    let fx = fix_fx(1000, &c);
    assert_eq!(fx.kind, FxKind::Eq);
    assert_eq!(fx.key, Some(1), "keyed from the kick through the standard sidechain");
    let band = &fx.bands.as_ref().unwrap()[0];
    assert!(band.dynamic.is_some() && band.g == 0.0, "flat until the kick plays: {band:?}");

    let ctx = ProcessCtx { sample_rate: SR, ..Default::default() };
    let run = |keyed: bool| {
        let mut eq = wave_engine::dsp::Eq::default();
        eq.prepare(SR, 512);
        eq.set_params(&wave_engine::catalog::param_block(&fx));
        let n = 8 * BEAT;
        let mut out = Vec::with_capacity(n);
        for at in (0..n).step_by(512) {
            let mut l: Vec<f32> = (at..at + 512).map(bass).collect();
            let mut r = l.clone();
            let key: Vec<f32> = (at..at + 512).map(kick).collect();
            if keyed {
                eq.process_keyed(&mut l, &mut r, &key, &ctx);
            } else {
                eq.process(&mut l, &mut r, &ctx);
            }
            out.extend_from_slice(&l);
        }
        out
    };
    let (dry, wet) = (run(false), run(true));
    // the 808 is steady: without the key the band's own level would duck it all the time, so the
    // reference is the unprocessed bass
    let rms_db = |x: &[f32], a: f32, b: f32| {
        let (mut e, mut k) = (0f64, 0usize);
        for (i, v) in x.iter().enumerate().skip(2 * BEAT) {
            let ph = (i % BEAT) as f32 / BEAT as f32;
            if ph >= a && ph < b {
                e += (v * v) as f64;
                k += 1;
            }
        }
        10.0 * (e / k as f64).log10() as f32
    };
    let orig: Vec<f32> = (0..8 * BEAT).map(bass).collect();
    let hit = rms_db(&orig, 0.0, 0.15) - rms_db(&wet, 0.0, 0.15);
    let gap = rms_db(&orig, 0.7, 1.0) - rms_db(&wet, 0.7, 1.0);
    eprintln!(
        "808 −{hit:.1} dB on kick hits, −{gap:.2} dB between (own-signal run −{:.1} dB)",
        rms_db(&orig, 0.7, 1.0) - rms_db(&dry, 0.7, 1.0)
    );
    assert!(hit > 3.0, "the 808 makes room while the kick hits: {hit} dB");
    assert!(gap < 0.5, "and is itself between hits: {gap} dB");
}

/// Real material: the demo song (Obsidian 2093). Its 808 and its kick are separate inserts that
/// meet only on the Beat Bus, and they share the sub range on every downbeat. The fix goes in
/// through the engine (the graph orders the key before the 808) and keeps working in a stem.
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

    let no = AtomicBool::new(false);
    let mono = |st: Vec<f32>| -> Vec<f32> { st.chunks(2).map(|c| 0.5 * (c[0] + c[1])).collect() };
    let kick_alone = mono(e.render_stem(kick, from, to, 0.0, 48000, &no).unwrap());
    let before = mono(e.render_stem(b808, from, to, 0.0, 48000, &no).unwrap());
    let next = e.project().next_id;
    e.apply(&[Op::PutFx { insert_id: b808, fx: fix_fx(next, &c), index: None }], false, None).unwrap();
    let o = wave_engine::routing::order(&e.project());
    let (ki, bi) =
        (e.project().inserts.iter().position(|m| m.id == kick).unwrap(), e.project().inserts.iter().position(|m| m.id == b808).unwrap());
    assert!(o.keys.contains(&(ki, bi)) && o.level[bi] > o.level[ki], "the key runs first");
    let after = mono(e.render_stem(b808, from, to, 0.0, 48000, &no).unwrap());

    // where the kick is loud (its envelope within 10 dB of its peak) vs where it is silent
    let env: Vec<f32> = {
        let mut a = 0f32;
        let k = (-1.0 / (SR * 0.01)).exp();
        kick_alone
            .iter()
            .map(|&x| {
                a = (x * x).max(a * k);
                a
            })
            .collect()
    };
    let peak = env.iter().copied().fold(0.0, f32::max);
    let f = c.fix();
    let band = |x: &[f32], mask: &dyn Fn(usize) -> bool| band_db(x, f.freq as f32, f.q as f32, mask);
    let on = |i: usize| env.get(i).is_some_and(|&v| v > peak * 0.1);
    let off = |i: usize| env.get(i).is_some_and(|&v| v < peak * 1e-4);
    let duck_hit = band(&before, &on) - band(&after, &on);
    let duck_gap = band(&before, &off) - band(&after, &off);
    eprintln!("808 at {:.0} Hz: −{duck_hit:.1} dB on kick hits, −{duck_gap:.2} dB between them", f.freq);
    assert!(duck_hit > 3.0, "the 808 makes room while the kick hits: {duck_hit} dB");
    assert!(duck_gap < 0.5, "and stays itself between hits: {duck_gap} dB");

    // the handled pair is not reported again
    let again = scan(&e, from, to);
    assert!(!again.iter().any(|x| [x.masker, x.protect].contains(&kick) && [x.masker, x.protect].contains(&b808)));
}

/// Mean energy (dB) of `x` (mono, 48 kHz) in a band, over the frames where `mask` is set.
fn band_db(x: &[f32], f0: f32, q: f32, mask: &dyn Fn(usize) -> bool) -> f32 {
    // a band-pass biquad (RBJ, constant 0 dB peak gain)
    let w = std::f32::consts::TAU * f0 / SR;
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

#[test]
fn ignored_pairs_are_kept_sorted_unique_and_dropped_with_their_insert() {
    let e = engine(two_inserts());
    e.apply(&[Op::SetMaskIgnore { pairs: vec![[2, 1], [1, 2]] }], false, None).unwrap();
    assert_eq!(e.project().mask_ignore, vec![[1, 2]]);
    assert!(e.apply(&[Op::SetMaskIgnore { pairs: vec![[0, 1]] }], false, None).is_err());
    assert!(e.apply(&[Op::SetMaskIgnore { pairs: vec![[1, 1]] }], false, None).is_err());
    assert!(e.apply(&[Op::SetMaskIgnore { pairs: vec![[1, 77]] }], false, None).is_err());
    assert!(!wave_engine::masking::compared_pairs(&e.project()).contains(&(1, 2)));
    assert!(e.undo());
    assert!(e.project().mask_ignore.is_empty());
    assert!(e.redo());
    // saved with the project
    let back: Project = serde_json::from_value(serde_json::to_value(e.project()).unwrap()).unwrap();
    assert_eq!(back.mask_ignore, vec![[1, 2]]);
    e.apply(&[Op::RemoveInsert { id: 2 }], false, None).unwrap();
    assert!(e.project().mask_ignore.is_empty());
}

#[test]
fn an_eq_takes_the_standard_sidechain() {
    let e = engine(two_inserts());
    let id = e.project().next_id;
    let mut eq = fix_fx(id, &Collision { masker: 2, protect: 1, lo: 40.0, hi: 63.0, center: 50.0, overlap: 0.5, key_db: -12.0 });
    // the same validation as every keyed effect (ops::validate_key)
    eq.key = Some(0);
    assert!(e.apply(&[Op::PutFx { insert_id: 2, fx: eq.clone(), index: None }], false, None).is_err(), "master");
    eq.key = Some(2);
    assert!(e.apply(&[Op::PutFx { insert_id: 2, fx: eq.clone(), index: None }], false, None).is_err(), "itself");
    eq.key = Some(1);
    e.apply(&[Op::PutFx { insert_id: 2, fx: eq, index: None }], false, None).unwrap();
    let o = wave_engine::routing::order(&e.project());
    assert_eq!(o.keys, vec![(1, 2)]);
    assert!(o.level[2] > o.level[1]);
    // removing the key's insert returns the EQ to its own signal
    e.apply(&[Op::RemoveInsert { id: 1 }], false, None).unwrap();
    assert_eq!(e.project().find_fx(id).unwrap().1.key, None);
}

/// The live path: the stream taps every insert while playing, the housekeeping thread analyses
/// what arrives, and the engine reports it; switched off, nothing is reported or tapped.
#[test]
fn a_live_stream_reports_what_it_played_and_stops_when_switched_off() {
    let p = wave_engine::demo::demo_project();
    let (kick, b808) = (insert_id(&p, "Kick"), insert_id(&p, "808"));
    let e = engine(p);
    let mut rt = e.create_rt(48000.0, 512);
    let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
    e.play(64.0);
    // 28 s of audio (the 16 bars the offline test scans), paced so the housekeeping thread
    // (60 Hz) drains the taps
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
    assert!(!e.shared().assist.load(std::sync::atomic::Ordering::Relaxed), "the audio thread stops tapping");
}

/// Listening material for a review (not a check): a kick + 808 loop and the demo's 16 bars, before
/// and after the fix. `MIX_ASSIST_OUT=dir cargo test -p wave-engine --test mix_assistant -- --ignored`.
#[test]
#[ignore]
fn write_before_after_renders() {
    let Ok(dir) = std::env::var("MIX_ASSIST_OUT") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let write = |name: &str, x: &[f32]| wave_engine::sources::write_wav(&dir.join(name), x, 48000).unwrap();
    // the synthetic loop: 8 bars of kick + held 808, the 808 through the fix keyed from the kick
    let c = listen(&two_inserts(), 8.0).collisions().into_iter().find(|c| c.masker == 2).unwrap();
    let fx = fix_fx(1000, &c);
    let mut eq = wave_engine::dsp::Eq::default();
    eq.prepare(SR, 512);
    eq.set_params(&wave_engine::catalog::param_block(&fx));
    let ctx = ProcessCtx { sample_rate: SR, ..Default::default() };
    let n = 32 * BEAT;
    let (mut before, mut after) = (Vec::with_capacity(2 * n), Vec::with_capacity(2 * n));
    for at in (0..n).step_by(512) {
        let mut l: Vec<f32> = (at..at + 512).map(bass).collect();
        let mut r = l.clone();
        let key: Vec<f32> = (at..at + 512).map(kick).collect();
        for j in 0..512 {
            let x = 0.8 * (l[j] + key[j]);
            before.extend([x, x]);
        }
        eq.process_keyed(&mut l, &mut r, &key, &ctx);
        for j in 0..512 {
            after.extend([0.8 * (l[j] + key[j]), 0.8 * (r[j] + key[j])]);
        }
    }
    write("loop-before.wav", &before);
    write("loop-after.wav", &after);
    // the demo song
    let p = wave_engine::demo::demo_project();
    let (kick, b808) = (insert_id(&p, "Kick"), insert_id(&p, "808"));
    let e = engine(p);
    let (from, to) = (64.0, 128.0);
    write("demo-before.wav", &e.render(from, to, 1.0, 48000).unwrap());
    let c = scan(&e, from, to).into_iter().find(|c| (c.masker, c.protect) == (b808, kick)).unwrap();
    eprintln!("{c:?} -> {:?}", c.fix());
    let next = e.project().next_id;
    e.apply(&[Op::PutFx { insert_id: b808, fx: fix_fx(next, &c), index: None }], false, None).unwrap();
    write("demo-after.wav", &e.render(from, to, 1.0, 48000).unwrap());
}
