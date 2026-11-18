//! Output underruns: a callback that fits its period is not counted, driver xruns are, and the
//! count rides last in the telemetry frame (the late-callback rule itself is tested in rt.rs).

use std::sync::Arc;

use wave_engine::*;

/// The trailing u32 of a frame with no inserts, fx or recording.
fn frame_underruns(s: &rt::Shared) -> u32 {
    let f = telemetry::frame(1, s, &[], None, None);
    u32::from_le_bytes(f[f.len() - 4..].try_into().unwrap())
}

#[test]
fn a_callback_that_fits_its_period_is_not_an_underrun() {
    let e = Engine::new(Project::routed(), Arc::new(NoPlugins));
    // 4096 frames at 48 kHz: 85 ms to render an idle mix
    let mut rt = e.create_rt(48000.0, 4096);
    let (mut l, mut r) = (vec![0f32; 4096], vec![0f32; 4096]);
    for _ in 0..8 {
        rt.process(&mut l, &mut r, None);
    }
    assert_eq!(e.shared().underruns(), 0);
    assert_eq!(frame_underruns(&e.shared()), 0);
}

#[test]
fn an_empty_frame_has_the_layout_the_ui_decodes() {
    // ui/src/api/telemetry.ts reads the same 56 bytes (logic.test.ts checks its side):
    // 32 header · u32 nIns · u32 nFx · u32 rec · f64 patBeat · u32 underruns
    let s = rt::Shared::new();
    s.add_underrun();
    s.add_underrun();
    let f = telemetry::frame(7, &s, &[], None, None);
    assert_eq!(f.len(), 56);
    assert_eq!(f64::from_le_bytes(f[44..52].try_into().unwrap()), -1.0);
    assert_eq!(u32::from_le_bytes(f[52..56].try_into().unwrap()), 2);
}

#[test]
fn a_driver_xrun_reaches_the_frame() {
    let e = Engine::new(Project::routed(), Arc::new(NoPlugins));
    let mut rt = e.create_rt(48000.0, 4096);
    let (mut l, mut r) = (vec![0f32; 4096], vec![0f32; 4096]);
    rt.process(&mut l, &mut r, None);
    // what the device layer does for a cpal xrun
    e.shared().add_underrun();
    assert_eq!(frame_underruns(&e.shared()), 1);
}
