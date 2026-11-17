//! `wave-plugin-windows` -- native top-level windows for plugin editors.
//!
//! A plugin editor (VST3, LV2) paints with its own toolkit into a native window handle
//! (`HWND` on Windows, an X11 window on Linux). That cannot be composited into the Tauri
//! webview, so every editor gets its **own floating native window**: Synkopa's header on top
//! (the name, a gear that opens Synkopa's own settings for the plugin, a close button -- FL
//! Studio's wrapper; [`header`]) and the plugin's interface embedded below it. The header
//! is the title bar: the OS frame is off.
//!
//! # Who pumps the windows
//!
//! **Windows.** A Win32 message queue belongs to a *thread*, not to a library. Tauri's
//! event loop (tao) runs `GetMessageW(hwnd = NULL)` + `DispatchMessageW` on the main
//! thread, which dispatches to **every** window created on that thread. So the windows
//! made here, and the child window the plugin creates inside them, are pumped by Tauri's
//! own loop with no integration at all -- *provided they are created on that thread*.
//!
//! **Linux.** X11 has no per-thread queue: Synkopa opens its own connection to the X server
//! (pure Rust, `x11rb`; the plugin opens another) and reads it from the UI thread's event
//! loop, which the application installs ([`event_loop::install`]; in Synkopa, GTK's glib
//! loop). The same loop runs the timers and file descriptors plugins register.
//!
//! Every call that touches a window checks the calling thread and returns
//! [`WindowError::WrongThread`] otherwise. Each window has a *tick* at
//! [`PUMP_INTERVAL_MS`] ([`Window::set_tick_handler`]): resize requests from the plugin,
//! user drags, DPI changes and the close button are serviced there, on the UI thread,
//! never from inside a nested event dispatch.
//!
//! # How the app uses it
//!
//! ```text
//! main thread (Tauri setup)        wave_plugin_windows::event_loop::install(glib_loop);  // Linux
//!                                  let windows = wave_plugin_windows::Service::new();
//!                                  windows.set_settings_handler(...);  // the header's gear
//!                                  app.manage(windows);            // Service is Send + Sync
//! any thread (a Tauri command)     app.run_on_main_thread(move || {
//!                                      let windows = app.state::<Service>();
//!                                      handle.open_editor(&windows, &label)   // wave-vst3, wave-lv2
//!                                  });
//! main thread (RunEvent::Exit)     windows.close_all();   // before audio stops and plugins drop
//! ```
//!
//! [`Service::close_all`] first *asks* every window's owner to close (so an editor is
//! detached from its window before the window dies) and destroys whatever is left.
//!
//! # Platforms
//!
//! Windows and Linux (X11, or XWayland under Wayland) have backends. Elsewhere the crate
//! compiles and every window call returns [`WindowError::Unsupported`].
//!
//! # What this crate deliberately does not do
//! - It knows nothing about VST3 or LV2. It hands out a native handle and reports resize,
//!   close and DPI changes; the hosts do the negotiation.
//! - It does not touch process-wide DPI awareness (Tauri sets per-monitor-v2 on Windows).

pub mod event_loop;
pub mod header;

#[cfg(not(any(windows, target_os = "linux")))]
mod unsupported;
#[cfg(windows)]
mod win32;
#[cfg(target_os = "linux")]
mod x11;

mod service;

pub use service::{
    Label, Service, Window, WindowError, WindowEvent, WindowId, WindowOptions, WindowSize, MAX_WINDOW_SIZE, MIN_WINDOW_SIZE,
    PUMP_INTERVAL_MS,
};

/// Whether this build has a native window backend. `false` means every
/// [`Service::open_window`] returns [`WindowError::Unsupported`].
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));

/// Whether plugin windows can be opened in this session; the error says why not (no
/// backend; on Linux no X11 display -- a Wayland session without XWayland).
pub fn available() -> Result<(), WindowError> {
    #[cfg(windows)]
    return Ok(());
    #[cfg(target_os = "linux")]
    return x11::available();
    #[cfg(not(any(windows, target_os = "linux")))]
    return Err(WindowError::Unsupported);
}
