//! A real VST3 editor on Linux, end to end: WAVE's window under a real X server (CI: Xvfb),
//! the plugin's `IRunLoop` served by the installed loop, and a user's mouse (XTEST) turning
//! a knob that reaches the plugin's parameter. Uses DISTRHO's Nekobi (Debian/Ubuntu
//! `dpf-plugins-vst3`, the framework Vitalium is built with) or `WAVE_VST3_GUI_PLUGIN`.
//! Skips without a display or the plugin unless `WAVE_REQUIRE_PLUGIN_GUI=1`.
#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use wave_plugin_windows::event_loop::{self, PollLoop};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xtest::ConnectionExt as _;

fn plugin() -> Option<String> {
    let required = std::env::var_os("WAVE_REQUIRE_PLUGIN_GUI").is_some_and(|v| v == "1");
    let path = std::env::var("WAVE_VST3_GUI_PLUGIN").unwrap_or_else(|_| "/usr/lib/vst3/Nekobi.vst3".into());
    let why = match (wave_plugin_windows::available(), std::path::Path::new(&path).exists()) {
        (Err(e), _) => e.to_string(),
        (_, false) => format!("{path} is not installed"),
        _ => return Some(path),
    };
    assert!(!required, "WAVE_REQUIRE_PLUGIN_GUI=1 but {why}");
    eprintln!("skipping: {why}");
    None
}

fn pump(lp: &PollLoop, for_: Duration) {
    let end = Instant::now() + for_;
    while Instant::now() < end {
        lp.run_once(Duration::from_millis(5));
    }
}

#[test]
fn a_dpf_editor_opens_under_waves_header_runs_on_the_loop_and_its_knobs_reach_the_plugin() {
    let Some(path) = plugin() else { return };
    let lp = PollLoop::new();
    event_loop::install(lp.clone());
    let windows = wave_plugin_windows::Service::new();
    // a scan root holding just this bundle (a link to it)
    let scan_root = std::env::temp_dir().join(format!("wave-vst3-gui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scan_root);
    std::fs::create_dir_all(&scan_root).unwrap();
    let link = scan_root.join(std::path::Path::new(&path).file_name().unwrap());
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let instruments = wave_vst3::testing::scan_instrument_roots(std::slice::from_ref(&scan_root), None);
    let (effects, ..) = wave_vst3::testing::scan_roots(std::slice::from_ref(&scan_root), None);
    // an instrument when it is one (Nekobi), an effect otherwise
    let handle = match (instruments.first(), effects.first()) {
        (Some(i), _) => wave_vst3::instantiate_instrument(&i.path, &i.cid, None, 48_000.0, 512, false).map(|x| x.1),
        (None, Some(e)) => wave_vst3::instantiate(&e.path, &e.cid, None, 48_000.0, 512).map(|x| x.1),
        _ => panic!("{path} lists no class"),
    }
    .expect("instantiate");
    let info = instruments.first().or(effects.first()).unwrap();
    assert!(handle.has_editor());
    let label = wave_plugin_windows::Label { title: info.name.clone(), subtitle: "VST3".into(), tag: 1 };
    handle.open_editor(&windows, &label).expect("the editor opens");
    assert!(handle.is_editor_open());
    // ours (the X connection and the tick) and the plugin's own: DPF repaints from a timer
    assert!(lp.len() > 2, "the plugin registered with the run loop ({} sources)", lp.len());
    pump(&lp, Duration::from_millis(600));

    // the plugin's window, under the header, and a user turning what is under the pointer
    let (x, screen) = x11rb::connect(None).unwrap();
    let root = x.setup().roots[screen].root;
    let tree = x.query_tree(root).unwrap().reply().unwrap();
    let frame = tree
        .children
        .into_iter()
        .find(|w| {
            x.get_property(false, *w, x11rb::protocol::xproto::AtomEnum::WM_CLASS, x11rb::protocol::xproto::AtomEnum::STRING, 0, 64)
                .ok()
                .and_then(|c| c.reply().ok())
                .is_some_and(|r| r.value.starts_with(b"wave-plugin"))
        })
        .expect("WAVE's plugin window is on the screen");
    let g = x.get_geometry(frame).unwrap().reply().unwrap();
    assert!(g.height > wave_plugin_windows::header::HEIGHT as u16, "the plugin area is below the header");
    let before = handle.params();
    let at = x.translate_coordinates(frame, root, 0, 0).unwrap().reply().unwrap();
    let (cx, cy) = (at.dst_x + 208, at.dst_y + 97); // Nekobi's cutoff knob
    x.xtest_fake_input(6, 0, 0, root, cx, cy, 0).unwrap();
    for _ in 0..6 {
        x.xtest_fake_input(4, 4, 0, root, 0, 0, 0).unwrap(); // wheel up
        x.xtest_fake_input(5, 4, 0, root, 0, 0, 0).unwrap();
    }
    x.xtest_fake_input(4, 1, 0, root, 0, 0, 0).unwrap();
    for dy in [8, 20, 35] {
        x.xtest_fake_input(6, 0, 0, root, cx, cy - dy, 0).unwrap();
    }
    x.xtest_fake_input(5, 1, 0, root, 0, 0, 0).unwrap();
    x.flush().unwrap();
    pump(&lp, Duration::from_millis(800));
    let after = handle.params();
    let moved = before.iter().zip(&after).filter(|(a, b)| (a.value - b.value).abs() > 1e-6).count();
    assert!(moved > 0, "turning a knob in the plugin's own window changed its parameter");

    handle.close_editor().unwrap();
    assert!(!handle.is_editor_open());
    drop(handle);
    pump(&lp, Duration::from_millis(100));
    assert!(lp.is_empty(), "nothing of the plugin is left on the loop ({} sources)", lp.len());
    event_loop::uninstall();
    let _ = std::fs::remove_dir_all(&scan_root);
}
