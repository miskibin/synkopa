//! The Master insert's volume (the toolbar's main volume knob) sets the level of what plays,
//! single- and multi-threaded alike.

use std::sync::Arc;
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

fn master_with(e: &Engine, vol: f32) -> Op {
    let mut m = e.project().inserts.into_iter().find(|m| m.id == 0).unwrap();
    m.vol = vol;
    m.fx.clear();
    Op::PutInsert { insert: m }
}

#[test]
fn the_master_volume_sets_the_level_while_the_song_plays() {
    for workers in [1, 4] {
        let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
        e.set_render_workers(workers);
        let mut rt = e.create_rt(SR, BLOCK);
        e.play(0.0);
        peak(&mut rt, 40);
        let loud = peak(&mut rt, 200);
        assert!(loud > 0.01, "the demo plays ({workers} workers): {loud}");
        e.apply(&[master_with(&e, 0.0)], false, None).unwrap();
        peak(&mut rt, 20); // the fader's smoothing
        let quiet = peak(&mut rt, 200);
        assert!(quiet < 1e-4, "master at 0 silences the song ({workers} workers): {quiet} vs {loud}");
        e.apply(&[master_with(&e, VOL_UNITY * 0.5)], true, None).unwrap();
        peak(&mut rt, 20);
        let half = peak(&mut rt, 200);
        assert!(half > 1e-3 && half < loud * 0.9, "a lower master is quieter ({workers} workers): {half} vs {loud}");
    }
}

#[test]
fn the_master_volume_still_works_after_the_audio_device_changes() {
    let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
    e.set_render_workers(4);
    let rt0 = e.create_rt(SR, BLOCK);
    e.play(0.0);
    // another device: a new stream at another rate replaces the first one mid-session
    drop(rt0);
    let mut rt = e.create_rt(44100.0, 512);
    let _ = e.stop();
    e.play(0.0);
    peak(&mut rt, 40);
    let loud = peak(&mut rt, 200);
    assert!(loud > 0.01, "plays after the switch: {loud}");
    e.begin_gesture(None);
    e.apply(&[master_with(&e, 0.0)], true, None).unwrap();
    e.end_gesture();
    assert_eq!(e.project().inserts.iter().find(|m| m.id == 0).unwrap().vol, 0.0, "the project took the new volume");
    peak(&mut rt, 20);
    let quiet = peak(&mut rt, 200);
    assert!(quiet < 1e-4, "master at 0 silences the song after a device change: {quiet} vs {loud}");
}

#[test]
fn the_program_volume_is_not_the_master_and_follows_the_device_to_a_new_stream() {
    let e = Engine::new(demo::sketch_project(), Arc::new(NoPlugins));
    let rt0 = e.create_rt(SR, BLOCK);
    assert_eq!(e.shared().program_gain(), 1.0, "0 dB by default");
    e.set_program_volume(0.0);
    assert_eq!(e.shared().program_gain(), 0.0, "the running stream hears it at once");
    // the project and its Master fader are untouched: nothing to undo, nothing saved
    let m = e.project().inserts.into_iter().find(|m| m.id == 0).unwrap();
    assert_eq!(m.vol, VOL_UNITY);
    // the engine renders the mix as before: the gain belongs to the device output, so an
    // offline render (export, bounce) plays at full level
    let mut rt0 = rt0;
    e.play(0.0);
    peak(&mut rt0, 40);
    assert!(peak(&mut rt0, 200) > 0.01, "the mix itself is not scaled");
    // a new device stream keeps the setting
    drop(rt0);
    let _rt = e.create_rt(44100.0, 512);
    assert_eq!(e.shared().program_gain(), 0.0);
    e.set_program_volume(VOL_UNITY);
    assert!((e.shared().program_gain() - 1.0).abs() < 1e-6, "VOL_UNITY is 0 dB");
    e.set_program_volume(1.0);
    assert!(e.shared().program_gain() > 1.9, "the top of the knob is about +6 dB like a fader");
}
