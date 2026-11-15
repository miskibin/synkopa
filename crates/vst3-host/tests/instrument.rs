//! Hosting a real VST3 instrument, end to end: scan, notes in, sound out, state, offline mode.
//!
//! Needs an instrument bundle: set `WAVE_VST3_TEST_INSTRUMENT` to its `.vst3` path (any
//! synth that sounds on a held middle C with its default or saved patch). Without it the
//! test prints why and passes vacuously.

use std::path::{Path, PathBuf};

use wave_engine::{InstEvent, InstMsg, ProcessCtx};

const WORKER: &str = env!("CARGO_BIN_EXE_wave-vst3-scan-worker");
const SR: f32 = 48_000.0;
const BLOCK: usize = 256;

fn fixture() -> Option<PathBuf> {
    let Some(path) = std::env::var_os("WAVE_VST3_TEST_INSTRUMENT") else {
        eprintln!("skipped: WAVE_VST3_TEST_INSTRUMENT is not set (no VST3 instrument to host)");
        return None;
    };
    let path = PathBuf::from(path);
    assert!(path.exists(), "WAVE_VST3_TEST_INSTRUMENT points at nothing: {}", path.display());
    Some(path)
}

fn ctx(playing: bool) -> ProcessCtx {
    ProcessCtx { sample_rate: SR, bpm: 120.0, beat: 0.0, playing }
}

/// Peak of `blocks` blocks; `first` events go into the first one.
fn play(inst: &mut dyn wave_engine::Instrument, first: &[InstEvent], blocks: usize) -> f32 {
    let (mut l, mut r) = (vec![0f32; BLOCK], vec![0f32; BLOCK]);
    let mut peak = 0f32;
    for b in 0..blocks {
        inst.process(if b == 0 { first } else { &[] }, &mut l, &mut r, &ctx(true));
        assert!(l.iter().chain(&r).all(|v| v.is_finite()), "non-finite output");
        peak = l.iter().chain(&r).fold(peak, |m, v| m.max(v.abs()));
    }
    peak
}

#[test]
fn a_scanned_instrument_plays_notes_keeps_its_state_and_renders_offline() {
    let Some(bundle) = fixture() else { return };
    let root = bundle.parent().unwrap().to_path_buf();
    let list = wave_vst3::testing::scan_instrument_roots(&[root], Some(Path::new(WORKER)));
    let info = list.iter().find(|i| Path::new(&i.path) == bundle).unwrap_or_else(|| panic!("not listed as an instrument: {list:?}"));
    eprintln!("hosting {} by {} ({})", info.name, info.vendor, info.category);

    let (mut inst, handle) = wave_vst3::instantiate_instrument(&info.path, &info.cid, None, SR, BLOCK, false).expect("instantiates");
    assert_eq!(handle.info().cid, info.cid);
    let quiet = play(&mut *inst, &[], 8);
    let on = [InstEvent { offset: 10, msg: InstMsg::NoteOn { key: 60, vel: 0.9, id: 1 } }];
    let held = play(&mut *inst, &on, 200);
    assert!(held > 1e-3 && held > quiet * 4.0, "a held C sounds: {held} (silence {quiet})");
    let off = [InstEvent { offset: 0, msg: InstMsg::NoteOff { key: 60, id: 1 } }];
    play(&mut *inst, &off, 400);
    let after = play(&mut *inst, &[], 40);
    assert!(after < held * 0.5, "the note-off ended it: {after} vs {held}");
    // controllers without a mapping are dropped, mapped ones do not break anything
    let ctl = [
        InstEvent { offset: 0, msg: InstMsg::Control { cc: 64, value: 1.0 } },
        InstEvent { offset: 1, msg: InstMsg::Bend(1.0) },
        InstEvent { offset: 2, msg: InstMsg::Control { cc: 1, value: 0.5 } },
    ];
    play(&mut *inst, &ctl, 4);

    let state = handle.get_state().expect("state");
    assert!(!state.is_empty());
    drop(inst);
    let (mut offline, _h) = wave_vst3::instantiate_instrument(&info.path, &info.cid, Some(&state), SR, BLOCK, true)
        .expect("an offline instance from the state");
    let held = play(&mut *offline, &on, 200);
    assert!(held > 1e-3, "the offline instance sounds: {held}");
}
