//! Opens an LV2 plugin's own interface the way the app does, and reports every step.
//!
//! `cargo run -p wave-lv2 --example ui_probe -- <name or URI substring> [seconds]`
//!
//! Hosts the first installed LV2 effect whose name or URI contains the substring, opens its
//! interface through Synkopa's window service under a small poll loop (the app installs glib's),
//! runs audio through it, and prints whether the interface is open and what it set.

#[cfg(target_os = "linux")]
fn main() {
    use std::time::{Duration, Instant};
    use wave_engine::ProcessCtx;

    struct Stderr;
    impl log::Log for Stderr {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, r: &log::Record) {
            eprintln!("[{}] {}", r.level(), r.args());
        }
        fn flush(&self) {}
    }
    let _ = log::set_logger(&Stderr);
    log::set_max_level(log::LevelFilter::Debug);
    let mut args = std::env::args().skip(1);
    let want = args.next().expect("usage: ui_probe <plugin name or URI substring> [seconds]").to_lowercase();
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(6);
    let lp = wave_plugin_windows::event_loop::PollLoop::new();
    wave_plugin_windows::event_loop::install(lp.clone());
    let windows = wave_plugin_windows::Service::new();
    windows.set_settings_handler(std::rc::Rc::new(|tag| println!("  header gear clicked (tag {tag})")));

    let all = wave_lv2::scan(false).expect("LV2 available");
    let Some(info) =
        all.iter().find(|p| p.unsupported.is_none() && (p.name.to_lowercase().contains(&want) || p.cid.to_lowercase().contains(&want)))
    else {
        return eprintln!("no usable LV2 effect like {want:?}");
    };
    println!("hosting {} <{}>", info.name, info.cid);
    let (mut effect, handle) = wave_lv2::instantiate(&info.cid, None, 48_000.0, 512).expect("instantiate");
    println!("has_editor={} ({:?})", handle.has_editor(), handle.editor_unavailable());
    let before: Vec<(u32, f64)> = handle.params().iter().map(|p| (p.id, p.value)).collect();
    let t = Instant::now();
    let label = wave_plugin_windows::Label { title: info.name.clone(), subtitle: "LV2 effect".into(), tag: 9 };
    match handle.open_editor(&windows, &label) {
        Ok(()) => println!("open_editor: ok in {:?}; open={}", t.elapsed(), handle.is_editor_open()),
        Err(e) => println!("open_editor: ERROR {e}"),
    }
    let ctx = ProcessCtx { sample_rate: 48_000.0, bpm: 120.0, beat: 0.0, playing: true };
    let (mut l, mut r) = (vec![0.1f32; 256], vec![0.1f32; 256]);
    let end = Instant::now() + Duration::from_secs(secs);
    let mut last = Instant::now();
    while Instant::now() < end {
        effect.process(&mut l, &mut r, &ctx);
        lp.run_once(Duration::from_millis(5));
        if last.elapsed() > Duration::from_secs(1) {
            println!("  open: {}", handle.is_editor_open());
            last = Instant::now();
        }
    }
    let after: Vec<(u32, f64)> = handle.params().iter().map(|p| (p.id, p.value)).collect();
    let changed: Vec<_> = before.iter().zip(&after).filter(|(a, b)| a != b).map(|(_, b)| *b).collect();
    println!("parameters changed through the interface: {changed:?}");
    let _ = handle.close_editor();
    println!("closed; open={} loop sources {}", handle.is_editor_open(), lp.len());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("LV2 interfaces are hosted on Linux only");
}
