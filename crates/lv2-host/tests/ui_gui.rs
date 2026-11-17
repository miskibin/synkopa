//! A real LV2 interface on Linux, end to end: x42's equaliser (`x42-plugins`, an X11 UI)
//! opened in Synkopa's window under a real X server (CI: Xvfb), its meter fed from the plugin's
//! control output, and a user's wheel (XTEST) on a knob reaching the plugin's parameter. A
//! plugin without an embeddable interface says why. Skips without a display or the plugin
//! unless `WAVE_REQUIRE_PLUGIN_GUI=1`.
#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use wave_engine::ProcessCtx;
use wave_plugin_windows::event_loop::{self, PollLoop};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
use x11rb::protocol::xtest::ConnectionExt as _;

const EQ: &str = "http://gareus.org/oss/lv2/fil4#mono";
const AMP: &str = "http://plugin.org.uk/swh-plugins/amp";

fn ready() -> bool {
    let required = std::env::var_os("WAVE_REQUIRE_PLUGIN_GUI").is_some_and(|v| v == "1");
    let why = match (wave_plugin_windows::available(), wave_lv2::scan(false)) {
        (Err(e), _) => e.to_string(),
        (_, Err(e)) => e,
        (_, Ok(list)) if !list.iter().any(|p| p.cid == EQ) => format!("{EQ} is not installed"),
        _ => return true,
    };
    assert!(!required, "WAVE_REQUIRE_PLUGIN_GUI=1 but {why}");
    eprintln!("skipping: {why}");
    false
}

#[test]
fn an_x11_interface_opens_shows_meters_and_its_knobs_reach_the_plugin() {
    if !ready() {
        return;
    }
    let lp = PollLoop::new();
    event_loop::install(lp.clone());
    let windows = wave_plugin_windows::Service::new();
    let (mut fx, h) = wave_lv2::instantiate(EQ, None, 48_000.0, 512).expect("instantiate");
    assert!(h.has_editor(), "{:?}", h.editor_unavailable());
    let label = wave_plugin_windows::Label { title: "x42-eq".into(), subtitle: "LV2".into(), tag: 2 };
    h.open_editor(&windows, &label).expect("the interface opens");
    assert!(h.is_editor_open());
    let ctx = ProcessCtx { sample_rate: 48_000.0, bpm: 120.0, beat: 0.0, playing: true };
    let run = |fx: &mut Box<dyn wave_engine::Processor>, ms: u64| {
        let end = Instant::now() + Duration::from_millis(ms);
        let (mut l, mut r) = (vec![0.1f32; 256], vec![0.1f32; 256]);
        while Instant::now() < end {
            fx.process(&mut l, &mut r, &ctx);
            lp.run_once(Duration::from_millis(5));
        }
    };
    run(&mut fx, 800);

    let (x, screen) = x11rb::connect(None).unwrap();
    let root = x.setup().roots[screen].root;
    let frame = x
        .query_tree(root)
        .unwrap()
        .reply()
        .unwrap()
        .children
        .into_iter()
        .find(|w| {
            x.get_property(false, *w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)
                .ok()
                .and_then(|c| c.reply().ok())
                .is_some_and(|r| r.value.starts_with(b"wave-plugin"))
        })
        .expect("Synkopa's plugin window is on the screen");
    let at = x.translate_coordinates(frame, root, 0, 0).unwrap().reply().unwrap();
    let before = h.params();
    // the output gain knob, then the wheel
    let (kx, ky) = (at.dst_x + 49, at.dst_y + 323);
    x.xtest_fake_input(6, 0, 0, root, kx, ky, 0).unwrap();
    x.flush().unwrap();
    run(&mut fx, 300);
    for _ in 0..6 {
        x.xtest_fake_input(4, 4, 0, root, 0, 0, 0).unwrap();
        x.xtest_fake_input(5, 4, 0, root, 0, 0, 0).unwrap();
        x.flush().unwrap();
        run(&mut fx, 60);
    }
    run(&mut fx, 500);
    let moved: Vec<_> = before.iter().zip(h.params()).filter(|(a, b)| (a.value - b.value).abs() > 1e-6).map(|(_, b)| b.name).collect();
    assert!(!moved.is_empty(), "the wheel on a knob in the plugin's own interface changed a parameter");
    assert!(h.take_state_changed(), "an edit in the interface is an edit to save");

    h.close_editor().unwrap();
    assert!(!h.is_editor_open());
    run(&mut fx, 100);
    assert!(lp.is_empty(), "nothing of the interface is left on the loop ({} sources)", lp.len());
    event_loop::uninstall();
}

#[test]
fn a_plugin_without_an_embeddable_interface_says_why() {
    if !ready() {
        return;
    }
    let Ok((_fx, h)) = wave_lv2::instantiate(AMP, None, 48_000.0, 64) else {
        return eprintln!("skipping: {AMP} is not installed");
    };
    assert!(!h.has_editor());
    assert!(h.editor_unavailable().is_some_and(|why| why.contains("interface")), "{:?}", h.editor_unavailable());
    assert!(!h.is_editor_open());
}
