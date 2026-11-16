//! [`Service`] and [`Window`] -- the portable half of the window manager.
//!
//! Identity, sizing rules, the event channel, thread-affinity enforcement and
//! the bookkeeping of which windows are open. The calls that make a window
//! exist live in the platform backend behind a handful of free functions, so
//! this file can be read (and mostly tested) without a display.

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Weak};
use std::thread::ThreadId;

use parking_lot::Mutex;

#[cfg(windows)]
use crate::win32 as backend;

#[cfg(target_os = "linux")]
use crate::x11 as backend;

#[cfg(not(any(windows, target_os = "linux")))]
use crate::unsupported as backend;

use crate::header::Header;

/// Smallest client area a plugin window may be given. A 1x1 window is a way to
/// lose the close button, not a window.
pub const MIN_WINDOW_SIZE: WindowSize = WindowSize { width: 120, height: 80 };

/// Largest client area. Bigger than any real editor, small enough that a
/// plugin returning garbage cannot ask the compositor for a gigabyte surface.
pub const MAX_WINDOW_SIZE: WindowSize = WindowSize { width: 8192, height: 8192 };

/// Period of the wake-up timer each plugin window carries, in milliseconds.
///
/// It makes the UI thread's `GetMessage` return while the user is idle, and it
/// is when the window's tick handler runs. ~60 Hz.
pub const PUMP_INTERVAL_MS: u32 = 16;

/// Size of a window's **client area**, in physical pixels (VST3's `HWND`
/// editors work in device pixels and are told the scale separately).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// This size pulled inside [`MIN_WINDOW_SIZE`]..=[`MAX_WINDOW_SIZE`]. Both
    /// bounds are reachable from plugin-controlled data.
    #[must_use]
    pub const fn clamped(self) -> Self {
        Self {
            width: clamp_u32(self.width, MIN_WINDOW_SIZE.width, MAX_WINDOW_SIZE.width),
            height: clamp_u32(self.height, MIN_WINDOW_SIZE.height, MAX_WINDOW_SIZE.height),
        }
    }

    /// Whether this size is already within the limits.
    pub const fn is_within_limits(self) -> bool {
        self.width >= MIN_WINDOW_SIZE.width
            && self.height >= MIN_WINDOW_SIZE.height
            && self.width <= MAX_WINDOW_SIZE.width
            && self.height <= MAX_WINDOW_SIZE.height
    }
}

const fn clamp_u32(value: u32, low: u32, high: u32) -> u32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// Identity of an open window. Never reused within a process run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(u64);

impl WindowId {
    /// The id of no window (errors raised before one exists).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) const fn none() -> Self {
        Self(0)
    }

    /// The raw counter value, for logs and tests.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "window#{}", self.0)
    }
}

/// What a plugin window tells its owner.
///
/// Queued by the window procedure (which runs inside the UI loop's
/// `DispatchMessage`) and drained by the owner from its tick, so no owner code
/// runs inside a nested window-procedure call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEvent {
    /// The user clicked the close button -- or [`Service::close_all`] is
    /// shutting down. The window is still alive: closing is the owner's job,
    /// because a plugin's editor must be detached before its parent dies.
    Closed,
    /// The client area changed size (user drag, DPI change, programmatic).
    Resized(WindowSize),
    /// The window moved to a monitor with a different DPI. It has already been
    /// moved to the rectangle Windows suggested; the resulting client size
    /// arrives as a separate [`Resized`](Self::Resized).
    DpiChanged { dpi: u32 },
}

/// Everything that can go wrong opening or driving a plugin window.
#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    /// A window call was made from a thread other than the one that created the
    /// [`Service`]. Win32 pins a window to its creating thread.
    #[error("plugin windows are owned by thread {owner:?}; this call came from {caller:?}")]
    WrongThread { owner: ThreadId, caller: ThreadId },

    /// This platform has no plugin-window backend (macOS).
    #[error("native plugin windows are not implemented on this platform")]
    Unsupported,

    /// Linux: no X server to open plugin windows on. Plugin editors embed into X11 windows
    /// only; under Wayland that is XWayland.
    #[error("plugin windows need an X11 display (XWayland on Wayland): {0}")]
    NoDisplay(String),

    /// Linux: the UI thread has no event loop installed (see [`crate::event_loop`]), so
    /// nothing would ever read the window's events.
    #[error("plugin windows need the UI thread's event loop, and none is installed")]
    NoEventLoop,

    /// An X11 request on a live window failed.
    #[error("{what} failed on {id}: {message}")]
    X11 { id: WindowId, what: &'static str, message: String },

    /// `RegisterClassW` failed. Reports `GetLastError`.
    #[error("could not register the plugin window class (win32 error {0})")]
    ClassRegistration(u32),

    /// `CreateWindowExW` failed. Reports `GetLastError`.
    #[error("could not create the plugin window (win32 error {0})")]
    Creation(u32),

    /// The window has already been closed (by its owner or by
    /// [`Service::close_all`]).
    #[error("{0} is not open")]
    Closed(WindowId),

    /// A Win32 call on a live window failed.
    #[error("{api} failed on {id} (win32 error {code})")]
    Win32 { id: WindowId, api: &'static str, code: u32 },
}

/// What a plugin window's header says, and whose it is: what the app hands a plugin host
/// opening an editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Label {
    /// The channel or effect name.
    pub title: String,
    /// Dimmer, after the title: the format and kind ("VST3 instrument").
    pub subtitle: String,
    /// Handed to the settings handler when the header's gear is clicked.
    pub tag: u64,
}

/// How a window should be created.
#[derive(Debug, Clone)]
pub struct WindowOptions {
    /// The header's (and the task bar's) name: the channel or effect the plugin plays as.
    pub title: String,
    /// Dimmer text after the title (the format and kind: "VST3 instrument").
    pub subtitle: String,
    /// Requested size of the **plugin's area** (below WAVE's header). Clamped to the
    /// service's limits.
    pub size: WindowSize,
    /// Whether the user may drag the frame. `false` gives a fixed-size window
    /// (no thick frame, no maximise box), which a plugin that cannot resize
    /// requires: otherwise the user drags a frame the editor will not follow.
    pub resizable: bool,
    /// The owner's id for this window, handed to the settings handler when the header's
    /// gear is clicked ([`Service::set_settings_handler`]): in WAVE the effect or channel id.
    pub tag: u64,
}

impl WindowOptions {
    pub fn new(title: impl Into<String>, size: WindowSize) -> Self {
        Self { title: title.into(), subtitle: String::new(), size, resizable: false, tag: 0 }
    }

    #[must_use]
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    #[must_use]
    pub fn tag(mut self, tag: u64) -> Self {
        self.tag = tag;
        self
    }

    /// Options for a window showing `label`.
    pub fn labelled(label: &Label, size: WindowSize) -> Self {
        Self::new(label.title.clone(), size).subtitle(label.subtitle.clone()).tag(label.tag)
    }

    pub(crate) fn header(&self) -> Header {
        Header { title: self.title.clone(), subtitle: self.subtitle.clone(), hover: None, pressed: None }
    }
}

/// What the header's gear calls, with the window's tag.
type SettingsHandler = Rc<dyn Fn(u64)>;

thread_local! {
    static SETTINGS: std::cell::RefCell<Option<SettingsHandler>> = const { std::cell::RefCell::new(None) };
}

/// The header's gear was clicked on the window tagged `tag`. UI thread (called by backends).
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub(crate) fn settings_requested(tag: u64) {
    let handler = SETTINGS.with(|h| h.borrow().clone());
    match handler {
        Some(handler) => handler(tag),
        None => log::debug!("plugin window {tag:#x}: settings asked for, but nobody listens"),
    }
}

/// The windows a [`Service`] still owns: `(id, native handle)`.
type Registry = Mutex<Vec<(WindowId, isize)>>;

/// Native windows for external plugin editors. Create it **on the UI thread**.
///
/// `Send + Sync` so the app can keep it in shared state, but every call that
/// touches a window must come from the thread that created it (in Tauri: run it
/// through `AppHandle::run_on_main_thread`). See the crate docs.
pub struct Service {
    owner: ThreadId,
    next_id: AtomicU64,
    registry: Arc<Registry>,
    /// The application window plugin windows belong to (0 = none): they stay above it,
    /// minimise with it and come up in front of it however the click reached the app.
    owner_window: std::sync::atomic::AtomicIsize,
}

impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}

impl Service {
    /// A service owned by the calling thread, which must be the thread running
    /// the application's event loop.
    pub fn new() -> Self {
        Self {
            owner: std::thread::current().id(),
            next_id: AtomicU64::new(1),
            registry: Arc::new(Mutex::new(Vec::new())),
            owner_window: std::sync::atomic::AtomicIsize::new(0),
        }
    }

    /// Make plugin windows opened from now on *owned* by the application window `handle`
    /// (an `HWND` on Windows): an unowned window may open behind the app when the click
    /// came through another process (a webview), and it would hide behind the app later.
    pub fn set_owner_window(&self, handle: isize) {
        self.owner_window.store(handle, Ordering::Relaxed);
    }

    /// What a click on a header's gear does: called on the UI thread with the window's
    /// [`WindowOptions::tag`]. Call on the UI thread.
    pub fn set_settings_handler(&self, handler: Rc<dyn Fn(u64)>) {
        SETTINGS.with(|h| *h.borrow_mut() = Some(handler));
    }

    /// The thread this service (and every window it opens) belongs to.
    pub fn owner_thread(&self) -> ThreadId {
        self.owner
    }

    /// Whether the caller is on the owning thread.
    pub fn on_owner_thread(&self) -> bool {
        std::thread::current().id() == self.owner
    }

    fn check_thread(&self) -> Result<(), WindowError> {
        check_thread(self.owner)
    }

    /// Open a **hidden** top-level window. Show it with [`Window::show`] once
    /// the plugin has been embedded, so the user never sees an empty frame.
    ///
    /// # Errors
    /// [`WindowError::WrongThread`] off the owning thread,
    /// [`WindowError::Unsupported`] without a backend, and
    /// [`WindowError::ClassRegistration`] / [`WindowError::Creation`] when the
    /// OS refuses.
    pub fn open_window(&self, options: &WindowOptions) -> Result<Window, WindowError> {
        self.check_thread()?;
        let size = options.size.clamped();
        let (sender, receiver) = channel();
        let ticks = Arc::new(AtomicU64::new(0));
        let hwnd = backend::create_window(options, size, sender, Arc::clone(&ticks), self.owner_window.load(Ordering::Relaxed))?;

        let id = WindowId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.registry.lock().push((id, hwnd));
        log::debug!("opened plugin {id} '{}' ({}x{}) handle {hwnd:#x}", options.title, size.width, size.height);
        Ok(Window {
            id,
            hwnd,
            owner: self.owner,
            events: receiver,
            ticks,
            registry: Arc::downgrade(&self.registry),
            _thread_bound: std::marker::PhantomData,
        })
    }

    /// Whether a window with that id is still open.
    pub fn is_open(&self, id: WindowId) -> bool {
        self.registry.lock().iter().any(|(known, _)| *known == id)
    }

    /// How many windows are open.
    pub fn open_count(&self) -> usize {
        self.registry.lock().len()
    }

    /// Close every open window; returns how many were open. Call it on the UI
    /// thread at shutdown, **before** audio stops and plugins are dropped.
    ///
    /// Each window's owner is asked first: a [`WindowEvent::Closed`] is queued
    /// and its tick handler runs once, which is when an editor detaches itself
    /// and closes its own window. Only what is still open after that is
    /// destroyed outright. Off the owning thread this closes nothing and logs.
    pub fn close_all(&self) -> usize {
        if !self.on_owner_thread() {
            if self.open_count() > 0 {
                log::warn!("plugin-windows close_all called off the UI thread; {} window(s) left open", self.open_count());
            }
            return 0;
        }
        // Snapshot: the owners' handlers lock the registry to unregister.
        let open: Vec<(WindowId, isize)> = self.registry.lock().clone();
        for (id, hwnd) in &open {
            if self.is_open(*id) {
                backend::request_close(*hwnd);
            }
        }
        let leftover: Vec<(WindowId, isize)> = std::mem::take(&mut *self.registry.lock());
        for (id, hwnd) in &leftover {
            log::debug!("destroying plugin {id}: its owner did not close it");
            backend::destroy_window(*hwnd);
        }
        open.len()
    }
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service").field("owner", &self.owner).field("open", &self.open_count()).finish()
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let open = self.close_all();
        if open > 0 {
            log::debug!("plugin-windows Service dropped with {open} window(s) still open");
        }
    }
}

fn check_thread(owner: ThreadId) -> Result<(), WindowError> {
    let caller = std::thread::current().id();
    if caller == owner {
        Ok(())
    } else {
        Err(WindowError::WrongThread { owner, caller })
    }
}

/// One open window, owned by whoever opened it.
///
/// **Not `Send`**: the native handle is only meaningful on the UI thread, and
/// every method re-checks the thread anyway. Dropping a `Window` closes it
/// (when still open and on the right thread); detach the plugin's editor first.
pub struct Window {
    id: WindowId,
    hwnd: isize,
    owner: ThreadId,
    events: Receiver<WindowEvent>,
    ticks: Arc<AtomicU64>,
    registry: Weak<Registry>,
    _thread_bound: std::marker::PhantomData<*const ()>,
}

impl Window {
    pub const fn id(&self) -> WindowId {
        self.id
    }

    /// The native handle plugins embed into (an `HWND`; an X11 window id on Linux) as an
    /// integer, for `IPlugView::attached` / LV2's `ui:parent`: the area below WAVE's header.
    /// Valid only while the window is open, and only on the UI thread (0 otherwise).
    pub fn native_handle(&self) -> isize {
        if self.live().is_err() {
            return 0;
        }
        backend::embed_handle(self.hwnd)
    }

    /// Whether the window still exists ([`Service::close_all`] may have
    /// destroyed it behind its owner's back).
    pub fn is_open(&self) -> bool {
        self.registry.upgrade().is_some_and(|registry| registry.lock().iter().any(|(id, _)| *id == self.id))
    }

    fn live(&self) -> Result<isize, WindowError> {
        check_thread(self.owner)?;
        if self.is_open() {
            Ok(self.hwnd)
        } else {
            Err(WindowError::Closed(self.id))
        }
    }

    /// Make the window visible and bring it to the front.
    pub fn show(&self) -> Result<(), WindowError> {
        backend::show_window(self.live()?);
        Ok(())
    }

    /// Resize the **client area** (clamped), keeping the top-left corner.
    pub fn set_client_size(&self, size: WindowSize) -> Result<(), WindowError> {
        let hwnd = self.live()?;
        backend::set_client_size(self.id, hwnd, size.clamped())
    }

    /// Current client-area size.
    pub fn client_size(&self) -> Result<WindowSize, WindowError> {
        let hwnd = self.live()?;
        backend::client_size(self.id, hwnd)
    }

    /// The window's current DPI (96 = 100%).
    pub fn dpi(&self) -> Result<u32, WindowError> {
        Ok(backend::dpi(self.live()?))
    }

    /// Whether anything has embedded a child window. Lets a host notice an
    /// editor that reported a successful attach but embedded nothing.
    /// [`WindowError::Unsupported`] means "cannot tell", not "no".
    pub fn has_child_window(&self) -> Result<bool, WindowError> {
        backend::has_child_window(self.live()?)
    }

    /// The size of the window a plugin embedded (`None` before it did): for UIs that
    /// create their window at their own size and never say it.
    pub fn embedded_size(&self) -> Result<Option<WindowSize>, WindowError> {
        backend::embedded_size(self.live()?)
    }

    /// Every event queued since the last drain. Non-blocking.
    pub fn drain_events(&self) -> Vec<WindowEvent> {
        self.events.try_iter().collect()
    }

    /// Wake-up ticks delivered so far: proof that the UI loop is pumping this
    /// window.
    pub fn pump_ticks(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }

    /// Run `handler` on the UI thread at every wake-up tick, and once from
    /// [`Service::close_all`]. `None` removes it.
    ///
    /// The handler may close this very window: the tick keeps its own
    /// reference to the handler for the duration of the call. It must tolerate
    /// re-entrancy (a plugin running a modal loop inside a call made from the
    /// handler can deliver another tick).
    pub fn set_tick_handler(&self, handler: Option<Rc<dyn Fn()>>) -> Result<(), WindowError> {
        backend::set_tick_handler(self.live()?, handler);
        Ok(())
    }

    /// Destroy the window. Idempotent.
    ///
    /// # Errors
    /// [`WindowError::WrongThread`] off the UI thread (the window is left open
    /// rather than destroyed from a thread that does not own it).
    pub fn close(&self) -> Result<(), WindowError> {
        check_thread(self.owner)?;
        let Some(registry) = self.registry.upgrade() else {
            return Ok(());
        };
        let removed = {
            let mut open = registry.lock();
            open.iter().position(|(id, _)| *id == self.id).map(|index| open.remove(index))
        };
        if removed.is_some() {
            backend::destroy_window(self.hwnd);
            log::debug!("closed plugin {}", self.id);
        }
        Ok(())
    }
}

impl std::fmt::Debug for Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Window")
            .field("id", &self.id)
            .field("handle", &format_args!("{:#x}", self.hwnd))
            .field("pump_ticks", &self.pump_ticks())
            .finish()
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        if let Err(err) = self.close() {
            log::error!("plugin {} dropped off the UI thread and leaked: {err}", self.id);
        }
    }
}

/// State every window procedure needs. Boxed and owned by the native window,
/// freed when it is destroyed.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub(crate) struct WindowState {
    pub(crate) events: Sender<WindowEvent>,
    pub(crate) ticks: Arc<AtomicU64>,
    pub(crate) min_size: WindowSize,
    pub(crate) tick: std::cell::RefCell<Option<Rc<dyn Fn()>>>,
    /// WAVE's header above the plugin, with the button under the pointer.
    pub(crate) header: std::cell::RefCell<Header>,
    pub(crate) tag: u64,
    pub(crate) resizable: bool,
    /// The native child the plugin embeds into (below the header); 0 until created.
    pub(crate) embed: std::cell::Cell<isize>,
}

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
impl WindowState {
    pub(crate) fn new(options: &WindowOptions, events: Sender<WindowEvent>, ticks: Arc<AtomicU64>) -> Self {
        Self {
            events,
            ticks,
            // The service's floor, not the editor's initial size: a resizable editor
            // may be dragged smaller than it opened.
            min_size: MIN_WINDOW_SIZE,
            tick: std::cell::RefCell::new(None),
            header: std::cell::RefCell::new(options.header()),
            tag: options.tag,
            resizable: options.resizable,
            embed: std::cell::Cell::new(0),
        }
    }

    /// A click released on a header button (`part`, the one it was pressed on).
    pub(crate) fn header_click(&self, part: crate::header::Part) {
        match part {
            crate::header::Part::Settings => settings_requested(self.tag),
            crate::header::Part::Close => self.post(WindowEvent::Closed),
            crate::header::Part::Drag => {}
        }
    }

    /// Set the hovered / pressed button; `true` when that changed the picture.
    pub(crate) fn set_header_state(&self, hover: Option<crate::header::Part>, pressed: Option<crate::header::Part>) -> bool {
        let keep = |p: Option<crate::header::Part>| p.filter(|p| *p != crate::header::Part::Drag);
        let mut h = self.header.borrow_mut();
        let (hover, pressed) = (keep(hover), keep(pressed));
        let changed = h.hover != hover || h.pressed != pressed;
        h.hover = hover;
        h.pressed = pressed;
        changed
    }

    /// Queue an event for the owner. A dropped receiver is not an error: the
    /// owner may have let go while the window is being torn down.
    pub(crate) fn post(&self, event: WindowEvent) {
        let _ = self.events.send(event);
    }

    /// Run the owner's tick handler, if any. The `Rc` clone keeps the handler
    /// alive even if it destroys this window (and with it this state).
    pub(crate) fn run_tick(&self) {
        let handler = self.tick.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_clamped_into_the_supported_range() {
        assert_eq!(WindowSize::new(400, 250).clamped(), WindowSize::new(400, 250));
        assert_eq!(WindowSize::new(0, 0).clamped(), MIN_WINDOW_SIZE);
        assert_eq!(WindowSize::new(u32::MAX, u32::MAX).clamped(), MAX_WINDOW_SIZE);
        assert_eq!(WindowSize::new(1, 100_000).clamped(), WindowSize::new(MIN_WINDOW_SIZE.width, MAX_WINDOW_SIZE.height));
        assert!(WindowSize::new(400, 250).is_within_limits());
        assert!(!WindowSize::new(400, 1).is_within_limits());
    }

    #[test]
    fn a_service_starts_empty_and_knows_its_thread() {
        let service = Service::new();
        assert_eq!(service.open_count(), 0);
        assert!(service.on_owner_thread());
        assert_eq!(service.owner_thread(), std::thread::current().id());
        assert!(!service.is_open(WindowId(1)));
        assert_eq!(service.close_all(), 0);
        assert_eq!(WindowId(3).to_string(), "window#3");
    }

    #[test]
    fn opening_a_window_from_another_thread_is_refused() {
        let service = Arc::new(Service::new());
        let elsewhere = Arc::clone(&service);
        let (refused, on_owner) = std::thread::spawn(move || {
            let opened = elsewhere.open_window(&WindowOptions::new("x", WindowSize::new(200, 100)));
            (opened.err(), elsewhere.on_owner_thread())
        })
        .join()
        .expect("the probe thread must not panic");
        assert!(matches!(refused, Some(WindowError::WrongThread { .. })), "got {refused:?}");
        assert!(!on_owner);
        assert_eq!(service.close_all(), 0);
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    #[test]
    fn without_a_backend_opening_is_unsupported_and_says_so() {
        let service = Service::new();
        let err = service.open_window(&WindowOptions::new("Editor", WindowSize::new(400, 250))).unwrap_err();
        assert!(matches!(err, WindowError::Unsupported), "got {err:?}");
        const { assert!(!crate::SUPPORTED) };
    }

    /// A window bookkept by a service but with no native backing, so the
    /// portable rules can be tested anywhere. Its handle is 0, which every
    /// backend treats as a no-op.
    fn fake_window(service: &Service) -> Window {
        let id = WindowId(service.next_id.fetch_add(1, Ordering::Relaxed));
        service.registry.lock().push((id, 0));
        let (_sender, receiver) = channel();
        Window {
            id,
            hwnd: 0,
            owner: service.owner,
            events: receiver,
            ticks: Arc::new(AtomicU64::new(0)),
            registry: Arc::downgrade(&service.registry),
            _thread_bound: std::marker::PhantomData,
        }
    }

    #[test]
    fn closing_is_idempotent_and_unregisters() {
        let service = Service::new();
        let window = fake_window(&service);
        assert!(window.is_open());
        assert_eq!(service.open_count(), 1);
        window.close().unwrap();
        assert!(!window.is_open());
        assert_eq!(service.open_count(), 0);
        window.close().unwrap();
        assert!(matches!(window.show(), Err(WindowError::Closed(_))));
    }

    #[test]
    fn dropping_a_window_closes_it_and_close_all_reaps_the_rest() {
        let service = Service::new();
        let first = fake_window(&service);
        let second = fake_window(&service);
        drop(first);
        assert_eq!(service.open_count(), 1);
        assert_eq!(service.close_all(), 1);
        assert!(!second.is_open(), "close_all destroys what its owner left open");
        drop(second);
        assert_eq!(service.open_count(), 0);
    }

    #[test]
    fn a_window_outliving_its_service_is_simply_closed() {
        let service = Service::new();
        let window = fake_window(&service);
        drop(service);
        assert!(!window.is_open());
        window.close().unwrap();
    }

    #[test]
    fn tick_handlers_survive_the_window_they_close() {
        // The window procedure's half of the contract, without a window.
        let (sender, receiver) = channel();
        let state = WindowState::new(&WindowOptions::new("x", WindowSize::new(200, 100)), sender, Arc::new(AtomicU64::new(0)));
        let fired = Rc::new(std::cell::Cell::new(0));
        let seen = Rc::clone(&fired);
        state.run_tick();
        *state.tick.borrow_mut() = Some(Rc::new(move || seen.set(seen.get() + 1)));
        state.run_tick();
        state.post(WindowEvent::Closed);
        state.post(WindowEvent::Resized(WindowSize::new(640, 480)));
        assert_eq!(fired.get(), 1);
        assert_eq!(receiver.try_iter().collect::<Vec<_>>(), vec![WindowEvent::Closed, WindowEvent::Resized(WindowSize::new(640, 480))]);
        drop(receiver);
        state.post(WindowEvent::Closed);
    }

    #[test]
    fn options_default_to_fixed_size() {
        let options = WindowOptions::new("Editor", WindowSize::new(400, 250));
        assert!(!options.resizable);
        assert!(options.clone().resizable(true).resizable);
        assert_eq!(options.title, "Editor");
        let o = options.subtitle("VST3 instrument").tag(42);
        assert_eq!((o.subtitle.as_str(), o.tag), ("VST3 instrument", 42));
    }

    #[test]
    fn header_buttons_report_settings_with_the_tag_and_close_as_an_event() {
        use crate::header::Part;
        let (sender, receiver) = channel();
        let state =
            WindowState::new(&WindowOptions::new("Vitalium", WindowSize::new(400, 300)).tag(77), sender, Arc::new(AtomicU64::new(0)));
        let asked = Rc::new(std::cell::Cell::new(0_u64));
        let a = Rc::clone(&asked);
        Service::new().set_settings_handler(Rc::new(move |tag| a.set(tag)));
        state.header_click(Part::Settings);
        assert_eq!(asked.get(), 77);
        state.header_click(Part::Drag);
        state.header_click(Part::Close);
        assert_eq!(receiver.try_iter().collect::<Vec<_>>(), vec![WindowEvent::Closed]);
        assert!(state.set_header_state(Some(Part::Settings), None));
        assert!(!state.set_header_state(Some(Part::Settings), None), "no change, no repaint");
        assert!(state.set_header_state(Some(Part::Drag), None), "the drag area is not a hover state");
        assert_eq!(state.header.borrow().hover, None);
    }
}
