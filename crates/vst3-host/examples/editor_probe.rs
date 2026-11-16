//! Opens a VST3 plugin's editor the way the app does, and reports every step.
//!
//! `cargo run -p wave-vst3 --example editor_probe -- <name substring> [seconds] [screenshot-delay-ms]`
//!
//! Scans the installed plugins (effects and instruments), hosts the first one whose name
//! contains the substring, opens its editor from the UI thread through the window service,
//! and pumps the UI loop for a few seconds (default 6) while printing whether the editor
//! is still open. Windows pumps Win32 messages; Linux installs a small poll loop (the app
//! installs glib's) that also serves the plugin's `IRunLoop`. A diagnostic for "the plugin
//! GUI button does nothing".

use std::time::{Duration, Instant};

fn main() {
    if wave_vst3::run_scan_worker_if_requested() {
        return;
    }
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
    log::set_max_level(if std::env::var_os("PROBE_DEBUG").is_some() { log::LevelFilter::Debug } else { log::LevelFilter::Info });
    let mut args = std::env::args().skip(1);
    let want = args.next().expect("usage: editor_probe <plugin name substring> [seconds]").to_lowercase();
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(6);

    // the UI thread is this one: jobs posted to it run in the loop below
    let (tx, rx) = std::sync::mpsc::channel::<wave_vst3::MainThreadJob>();
    let tx = std::sync::Mutex::new(tx);
    wave_vst3::set_main_thread_executor(move |job| {
        let _ = tx.lock().unwrap().send(job);
    });
    let lp = wave_plugin_windows::event_loop::PollLoop::new();
    wave_plugin_windows::event_loop::install(lp.clone());
    let windows = wave_plugin_windows::Service::new();
    windows.set_settings_handler(std::rc::Rc::new(|tag| println!("  header gear clicked (tag {tag})")));
    if let Err(e) = wave_plugin_windows::available() {
        println!("plugin windows unavailable: {e}");
    }

    let mut all = wave_vst3::scan(true);
    let inst = wave_vst3::scan_instruments(false);
    all.extend(inst.iter().cloned());
    let Some(info) = all.iter().find(|p| p.name.to_lowercase().contains(&want)) else {
        eprintln!("no installed VST3 named like {want:?}; found: {:?}", all.iter().map(|p| &p.name).collect::<Vec<_>>());
        return;
    };
    let is_inst = inst.iter().any(|p| p.cid == info.cid);
    println!("hosting {} ({}) instrument={is_inst} path={}", info.name, info.cid, info.path);
    let t = Instant::now();
    let handle = if is_inst {
        wave_vst3::instantiate_instrument(&info.path, &info.cid, None, 48_000.0, 512, false).map(|x| x.1)
    } else {
        wave_vst3::instantiate(&info.path, &info.cid, None, 48_000.0, 512).map(|x| x.1)
    };
    let handle = match handle {
        Ok(h) => h,
        Err(e) => return eprintln!("instantiate failed: {e}"),
    };
    println!("instantiated in {:?}; has_editor={}; run-loop sources so far: {}", t.elapsed(), handle.has_editor(), lp.len());
    let before: Vec<(u32, f64)> = handle.params().iter().map(|p| (p.id, p.value)).collect();
    let t = Instant::now();
    let label = wave_plugin_windows::Label {
        title: info.name.clone(),
        subtitle: format!("VST3 {}", if is_inst { "instrument" } else { "effect" }),
        tag: 7,
    };
    match handle.open_editor(&windows, &label) {
        Ok(()) => {
            println!("open_editor: ok in {:?}; is_editor_open={}; run-loop sources: {}", t.elapsed(), handle.is_editor_open(), lp.len())
        }
        Err(e) => println!("open_editor: ERROR {e} (after {:?})", t.elapsed()),
    }
    let end = Instant::now() + Duration::from_secs(secs);
    let mut last = Instant::now();
    while Instant::now() < end {
        while let Ok(job) = rx.try_recv() {
            job();
        }
        pump_native();
        lp.run_once(Duration::from_millis(5));
        if last.elapsed() > Duration::from_secs(1) {
            println!("  editor open: {} (run-loop sources {})", handle.is_editor_open(), lp.len());
            last = Instant::now();
        }
    }
    let after: Vec<(u32, f64)> = handle.params().iter().map(|p| (p.id, p.value)).collect();
    let changed: Vec<_> = before.iter().zip(&after).filter(|(a, b)| a != b).map(|(_, b)| *b).collect();
    println!("parameters changed through the editor: {changed:?}");
    let _ = handle.close_editor();
    println!("closed; run-loop sources left: {}", lp.len());
    drop(handle);
    lp.run_for(Duration::from_millis(50));
    println!("dropped; run-loop sources left: {}", lp.len());
}

#[cfg(windows)]
fn pump_native() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};
    // SAFETY: a plain message pump on this thread.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// X11 is read by the installed loop.
#[cfg(not(windows))]
fn pump_native() {}
