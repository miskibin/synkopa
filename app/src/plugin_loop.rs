//! Linux: GTK's glib main loop, lent to plugin windows and plugins.
//!
//! Tauri runs GTK's main loop on the main thread. Installed as
//! `wave_plugin_windows::event_loop` there, it reads WAVE's X11 connection for the plugin
//! windows and serves the file descriptors and timers VST3 plugins register through
//! `Linux::IRunLoop` (JUCE- and DPF-built plugins depend on it). Every callback runs on the
//! main thread, where VST3 wants all non-audio plugin work.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use glib::ControlFlow;
use wave_plugin_windows::event_loop::{EventLoop, SourceKey};

#[derive(Default)]
struct GlibLoop {
    next: std::cell::Cell<u64>,
    sources: RefCell<HashMap<u64, glib::SourceId>>,
}

impl GlibLoop {
    fn key(&self) -> u64 {
        let k = self.next.get() + 1;
        self.next.set(k);
        k
    }
}

impl EventLoop for GlibLoop {
    fn watch_fd(&self, fd: i32, mut callback: Box<dyn FnMut()>) -> Option<SourceKey> {
        let key = self.key();
        let condition = glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR;
        let id = glib::source::unix_fd_add_local(fd, condition, move |_, _| {
            callback();
            ControlFlow::Continue
        });
        self.sources.borrow_mut().insert(key, id);
        Some(SourceKey(key))
    }

    fn add_timer(&self, period: Duration, mut callback: Box<dyn FnMut()>) -> SourceKey {
        let key = self.key();
        let id = glib::source::timeout_add_local(period.max(Duration::from_millis(1)), move || {
            callback();
            ControlFlow::Continue
        });
        self.sources.borrow_mut().insert(key, id);
        SourceKey(key)
    }

    fn remove(&self, key: SourceKey) {
        // removing a source from inside its own callback is fine in glib: the closure is
        // dropped after the dispatch returns
        if let Some(id) = self.sources.borrow_mut().remove(&key.0) {
            id.remove();
        }
    }
}

/// Install the loop. Call on the main thread (Tauri's `setup`).
pub fn install() {
    wave_plugin_windows::event_loop::install(Rc::new(GlibLoop::default()));
}

/// Whether WAVE should run its GTK window through XWayland. Plugin interfaces on Linux are X11
/// windows (VST3 `X11EmbedWindowID`, LV2 `X11UI`), and an X11 window can only stay above the
/// app's window (`WM_TRANSIENT_FOR`) when that window is an X11 window too: under a native
/// Wayland window every click on WAVE sends the plugin windows behind it. Bitwig, REAPER and
/// Ardour run as X11 clients for the same reason. `WAVE_WAYLAND=1`, or a `GDK_BACKEND` the
/// user set, keeps their choice.
pub fn wants_x11(gdk_backend: Option<&str>, wave_wayland: Option<&str>, wayland_display: Option<&str>, display: Option<&str>) -> bool {
    let set = |v: Option<&str>| v.is_some_and(|v| !v.trim().is_empty());
    !set(gdk_backend) && wave_wayland.map(str::trim) != Some("1") && set(wayland_display) && set(display)
}

/// Apply [`wants_x11`] to this process. Call first thing, before GTK starts and before any
/// other thread exists (it sets an environment variable).
pub fn prefer_x11() {
    let var = |k: &str| std::env::var(k).ok();
    if wants_x11(
        var("GDK_BACKEND").as_deref(),
        var("WAVE_WAYLAND").as_deref(),
        var("WAYLAND_DISPLAY").as_deref(),
        var("DISPLAY").as_deref(),
    ) {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

/// The X11 id of the app's main window when GTK runs on X11 (plugin windows are then
/// kept above it); `None` under Wayland, where an X11 window cannot belong to it.
pub fn owner_xid(window: &tauri::WebviewWindow) -> Option<u64> {
    use glib::object::Cast;
    use gtk::prelude::WidgetExt;
    let gdk_window = window.gtk_window().ok()?.window()?;
    let x11 = gdk_window.downcast::<gdkx11::X11Window>().ok()?;
    Some(x11.xid())
}

#[cfg(test)]
mod tests {
    use super::wants_x11;

    #[test]
    fn a_wayland_session_with_xwayland_runs_the_window_on_x11() {
        assert!(wants_x11(None, None, Some("wayland-0"), Some(":0")));
        assert!(!wants_x11(Some("wayland"), None, Some("wayland-0"), Some(":0")), "the user's GDK_BACKEND wins");
        assert!(!wants_x11(None, Some("1"), Some("wayland-0"), Some(":0")), "WAVE_WAYLAND=1 keeps native Wayland");
        assert!(!wants_x11(None, None, Some("wayland-0"), None), "no XWayland: nothing to switch to");
        assert!(!wants_x11(None, None, None, Some(":0")), "already an X11 session");
        assert!(!wants_x11(Some(""), None, None, Some(":0")));
    }
}
