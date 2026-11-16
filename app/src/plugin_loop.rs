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

/// The X11 id of the app's main window when GTK runs on X11 (plugin windows are then
/// kept above it); `None` under Wayland, where an X11 window cannot belong to it.
pub fn owner_xid(window: &tauri::WebviewWindow) -> Option<u64> {
    use glib::object::Cast;
    use gtk::prelude::WidgetExt;
    let gdk_window = window.gtk_window().ok()?.window()?;
    let x11 = gdk_window.downcast::<gdkx11::X11Window>().ok()?;
    Some(x11.xid())
}
