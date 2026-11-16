//! An LV2 plugin's own interface (`ui:X11UI`) in a WAVE plugin window (Linux).
//!
//! The UI is a separate library (`lv2ui_descriptor`) that draws into a window it creates
//! inside the parent it is given (`ui:parent`: the area under WAVE's header). It talks to
//! the plugin only through the host:
//!
//! - **UI -> plugin**: `write_function` with a control port's new value (protocol 0); WAVE
//!   stores it in the same per-port atomic the parameter list and automation write, so the
//!   audio thread takes it at its next block and the project's saved state follows.
//! - **plugin -> UI**: `port_event` with every control port (inputs and outputs: meters)
//!   whose value changed, at each window tick (~60 Hz).
//! - **size**: `ui:resize` both ways (the UI asks for a size; a resizable UI is told the
//!   size the user dragged to), `ui:idleInterface` called every tick.
//!
//! Only X11 UIs are embedded. GTK and Qt UIs would need their toolkit inside WAVE's process
//! (suil wraps them, and GTK2 cannot share a process with GTK3 at all); such plugins keep
//! WAVE's parameter list, and `has_editor` says why. Atom messages (`patch:Set`, files,
//! scopes) between UI and plugin are not carried yet: a UI that edits through them shows
//! its controls but those edits do not reach the plugin.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_int, c_void, CStr, CString};
use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};

use wave_plugin_windows::{Label, Service, Window, WindowEvent, WindowOptions, WindowSize};

use crate::effect::Shared;
use crate::lilv::{self, node_path, node_string, take_strings};
use crate::sys::*;
use crate::{urid, world};

/// Size used when a UI does not say how big it is.
const FALLBACK: WindowSize = WindowSize { width: 480, height: 320 };
/// `ui:updateRate` told to UIs: the window tick.
const UPDATE_RATE: f32 = 60.0;

/// The UI WAVE would open for a plugin.
#[derive(Debug, Clone)]
pub(crate) struct UiSpec {
    uri: CString,
    binary: String,
    bundle: CString,
}

/// The plugin's embeddable UI, or why it has none.
pub(crate) fn find(plugin_uri: &str) -> Result<UiSpec, String> {
    let l = lilv::lilv()?;
    let world = world::world(false)?;
    let _g = lilv::lock();
    // SAFETY: lilv calls under the lock on the world's own objects; every node created
    // here is freed here, `uis` once.
    unsafe {
        let plugin = world.plugin(l, plugin_uri).ok_or_else(|| format!("the LV2 plugin {plugin_uri} is not installed"))?;
        let uis = (l.lilv_plugin_get_uis)(plugin);
        if uis.is_null() {
            return Err("it has no interface of its own".into());
        }
        let x11 = (l.lilv_new_uri)(world.ptr, UI_X11.as_ptr().cast());
        let required = (l.lilv_new_uri)(world.ptr, LV2_CORE_REQUIRED_FEATURE.as_ptr().cast());
        let mut why = String::from("its interface is not an X11 one (GTK and Qt interfaces cannot be embedded)");
        let mut found = None;
        let mut it = (l.lilv_uis_begin)(uis);
        while found.is_none() && !(l.lilv_uis_is_end)(uis, it) {
            let ui = (l.lilv_uis_get)(uis, it);
            it = (l.lilv_uis_next)(uis, it);
            if !(l.lilv_ui_is_a)(ui, x11) {
                continue;
            }
            let uri_node = (l.lilv_ui_get_uri)(ui);
            (l.lilv_world_load_resource)(world.ptr, uri_node);
            let needs = take_strings(l, (l.lilv_world_find_nodes)(world.ptr, uri_node, required, std::ptr::null()));
            if let Some(missing) = needs.iter().find(|f| !SUPPORTED_UI_FEATURES.iter().any(|s| plain(s) == f.as_str())) {
                why = format!("its interface needs {}, which WAVE does not provide", short_name(missing));
                continue;
            }
            let binary = node_path(l, (l.lilv_ui_get_binary_uri)(ui));
            let bundle = node_path(l, (l.lilv_ui_get_bundle_uri)(ui));
            let uri = node_string(l, uri_node);
            if let (Ok(uri), Ok(bundle), false) = (CString::new(uri), CString::new(bundle_dir(&bundle)), binary.is_empty()) {
                found = Some(UiSpec { uri, binary, bundle });
            }
        }
        (l.lilv_node_free)(x11);
        (l.lilv_node_free)(required);
        (l.lilv_uis_free)(uis);
        found.ok_or(why)
    }
}

/// A bundle path as UIs expect it: a directory ending in `/`.
fn bundle_dir(path: &str) -> String {
    if path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}/")
    }
}

/// UI libraries stay loaded once opened: unloading toolkit code with live threads or
/// static destructors crashes hosts, and they are few.
fn library(binary: &str) -> Result<usize, String> {
    static LIBS: OnceLock<StdMutex<HashMap<String, usize>>> = OnceLock::new();
    let mut libs = LIBS.get_or_init(Default::default).lock().map_err(|_| "UI library table poisoned".to_owned())?;
    if let Some(&h) = libs.get(binary) {
        return Ok(h);
    }
    let c = CString::new(binary).map_err(|_| format!("{binary}: bad path"))?;
    // SAFETY: a plain dlopen of the UI library lilv named; never closed.
    let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if h.is_null() {
        // SAFETY: dlerror returns a thread-local string or null.
        let e = unsafe { libc::dlerror() };
        let e = if e.is_null() { String::new() } else { unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned() };
        return Err(format!("its interface library could not be loaded: {e}"));
    }
    libs.insert(binary.to_owned(), h as usize);
    Ok(h as usize)
}

/// What the UI's callbacks reach (boxed: the UI keeps its address).
struct Controller {
    shared: Weak<Shared>,
    /// The size the UI last asked for.
    resize: Cell<Option<(u32, u32)>>,
}

unsafe extern "C" fn write_port(controller: LV2UI_Controller, port: u32, size: u32, protocol: u32, buffer: *const c_void) {
    if controller.is_null() || buffer.is_null() || protocol != 0 || size != 4 {
        return;
    }
    // SAFETY: the controller is the box this session owns (alive while the UI is); the
    // UI passes a float for protocol 0.
    let (c, v) = unsafe { (&*controller.cast::<Controller>(), *buffer.cast::<f32>()) };
    if let Some(shared) = c.shared.upgrade() {
        shared.set_from_ui(port, v);
    }
}

unsafe extern "C" fn ui_asks_size(handle: *mut c_void, width: c_int, height: c_int) -> c_int {
    if handle.is_null() || width <= 0 || height <= 0 {
        return 1;
    }
    // SAFETY: as in `write_port`.
    unsafe { &*handle.cast::<Controller>() }.resize.set(Some((width as u32, height as u32)));
    0
}

/// The features handed to the UI (boxed: it keeps pointers into them).
struct Features {
    scale: f32,
    rate: f32,
    options: [LV2_Options_Option; 3],
    resize: LV2UI_Resize,
    log: LV2_Log_Log,
    list: Vec<LV2_Feature>,
    ptrs: Vec<*const LV2_Feature>,
}

impl Features {
    fn new(parent: *mut c_void, controller: *mut c_void, scale: f32) -> Box<Self> {
        let zero = || LV2_Options_Option { context: OPTIONS_INSTANCE, subject: 0, key: 0, size: 0, type_: 0, value: std::ptr::null() };
        let mut f = Box::new(Self {
            scale,
            rate: UPDATE_RATE,
            options: [zero(), zero(), zero()],
            resize: LV2UI_Resize { handle: controller, ui_resize: ui_asks_size },
            log: LV2_Log_Log { handle: std::ptr::null_mut(), printf: crate::instance::log_printf, vprintf: crate::instance::log_vprintf },
            list: vec![],
            ptrs: vec![],
        });
        let float = urid::map_str(ATOM_FLOAT);
        f.options[0] = LV2_Options_Option {
            context: OPTIONS_INSTANCE,
            subject: 0,
            key: urid::map_str(UI_SCALE_FACTOR),
            size: 4,
            type_: float,
            value: std::ptr::addr_of!(f.scale).cast(),
        };
        f.options[1] = LV2_Options_Option {
            context: OPTIONS_INSTANCE,
            subject: 0,
            key: urid::map_str(UI_UPDATE_RATE),
            size: 4,
            type_: float,
            value: std::ptr::addr_of!(f.rate).cast(),
        };
        let feature = |uri: &'static str, data: *mut c_void| LV2_Feature { URI: uri.as_ptr().cast(), data };
        f.list = vec![
            feature(UI_PARENT, parent),
            feature(UI_RESIZE, std::ptr::addr_of_mut!(f.resize).cast()),
            feature(UI_IDLE_INTERFACE, std::ptr::null_mut()),
            feature(URID_MAP, urid::map_feature().cast()),
            feature(URID_UNMAP, urid::unmap_feature().cast()),
            feature(OPTIONS_OPTIONS, f.options.as_mut_ptr().cast()),
            feature(LOG_LOG, std::ptr::addr_of_mut!(f.log).cast()),
        ];
        f.ptrs = f.list.iter().map(|x| x as *const LV2_Feature).chain(std::iter::once(std::ptr::null())).collect();
        f
    }
}

/// One open UI. UI thread only.
pub(crate) struct UiSession {
    desc: *const LV2UI_Descriptor,
    handle: LV2UI_Handle,
    idle: Option<unsafe extern "C" fn(LV2UI_Handle) -> c_int>,
    /// The UI's own `ui:resize` (told the size the user dragged to).
    resize_ext: *const LV2UI_Resize,
    window: Window,
    controller: Box<Controller>,
    _features: Box<Features>,
    shared: Weak<Shared>,
    /// Each port's value as last sent to the UI (f32 bits); `None` = never.
    sent: Vec<Option<u32>>,
    size: WindowSize,
}

impl UiSession {
    /// Instantiate `spec`'s UI for the plugin of `shared` in a new window.
    pub(crate) fn open(shared: &Arc<Shared>, spec: &UiSpec, windows: &Service, label: &Label) -> Result<Self, String> {
        let lib = library(&spec.binary)? as *mut c_void;
        // SAFETY: the symbol is LV2's `lv2ui_descriptor`; descriptors are static data of
        // the loaded library, which is never unloaded.
        let desc = unsafe {
            let sym = libc::dlsym(lib, c"lv2ui_descriptor".as_ptr());
            if sym.is_null() {
                return Err("its interface library has no lv2ui_descriptor".into());
            }
            let get: LV2UI_DescriptorFunction = std::mem::transmute(sym);
            let mut i = 0;
            loop {
                let d = get(i);
                if d.is_null() {
                    return Err("its interface library does not contain the interface its plugin names".into());
                }
                if !(*d).URI.is_null() && CStr::from_ptr((*d).URI) == spec.uri.as_c_str() {
                    break d;
                }
                i += 1;
            }
        };
        // SAFETY: a live descriptor; `extension_data` is a plain function.
        let ext = |uri: &str| unsafe { (*desc).extension_data.map_or(std::ptr::null(), |f| f(uri.as_ptr().cast())) };
        let resize_ext = ext(UI_RESIZE).cast::<LV2UI_Resize>();
        let idle_ext = ext(UI_IDLE_INTERFACE).cast::<LV2UI_Idle_Interface>();
        // a UI that can be told its size can follow the user's drags
        let resizable = !resize_ext.is_null();
        let window = windows.open_window(&WindowOptions::labelled(label, FALLBACK).resizable(resizable)).map_err(|e| e.to_string())?;
        // X11: the parent exists on screen before the UI embeds into it
        window.show().map_err(|e| e.to_string())?;
        let scale = window.dpi().unwrap_or(96) as f32 / 96.0;
        let controller = Box::new(Controller { shared: Arc::downgrade(shared), resize: Cell::new(None) });
        let controller_ptr = std::ptr::from_ref(&*controller).cast_mut().cast::<c_void>();
        let features = Features::new(window.native_handle() as *mut c_void, controller_ptr, scale);
        let plugin_uri = CString::new(shared.uri()).map_err(|_| "bad plugin URI".to_owned())?;
        let mut widget: LV2UI_Widget = std::ptr::null_mut();
        // SAFETY: every pointer outlives the UI (owned by the session built below, or the
        // static library); a panic cannot unwind out of C, so the call is caught.
        let handle = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            (*desc).instantiate.map_or(std::ptr::null_mut(), |f| {
                f(desc, plugin_uri.as_ptr(), spec.bundle.as_ptr(), write_port, controller_ptr, &mut widget, features.ptrs.as_ptr())
            })
        }))
        .unwrap_or(std::ptr::null_mut());
        if handle.is_null() {
            let _ = window.close();
            return Err("its interface could not be created".into());
        }
        let mut session = Self {
            desc,
            handle,
            // SAFETY: a live interface struct of the static library.
            idle: unsafe { idle_ext.as_ref() }.map(|i| i.idle),
            resize_ext,
            window,
            controller,
            _features: features,
            shared: Arc::downgrade(shared),
            sent: vec![None; shared.port_count()],
            size: FALLBACK,
        };
        let size = session
            .controller
            .resize
            .take()
            .map(|(w, h)| WindowSize::new(w, h))
            .or_else(|| session.window.embedded_size().ok().flatten())
            .filter(|s| s.width > 1 && s.height > 1)
            .unwrap_or(FALLBACK);
        session.size = size;
        let _ = session.window.set_client_size(size.clamped());
        session.window.show().map_err(|e| e.to_string())?;
        session.send_ports(shared);
        log::debug!(
            "LV2 interface <{}>: {}x{}, idle interface {}, resizable {}",
            spec.uri.to_string_lossy(),
            size.width,
            size.height,
            session.idle.is_some(),
            resizable
        );
        Ok(session)
    }

    pub(crate) fn window(&self) -> &Window {
        &self.window
    }

    pub(crate) fn raise(&self) {
        let _ = self.window.show();
    }

    /// Tell the UI every control port that changed since it last heard.
    fn send_ports(&mut self, shared: &Shared) {
        // SAFETY: a live descriptor.
        let Some(port_event) = (unsafe { (*self.desc).port_event }) else { return };
        for port in shared.control_ports() {
            let v = shared.port_value(port);
            if self.sent[port] == Some(v.to_bits()) {
                continue;
            }
            self.sent[port] = Some(v.to_bits());
            // SAFETY: a live UI handle; the float outlives the call.
            unsafe { port_event(self.handle, port as u32, 4, 0, std::ptr::from_ref(&v).cast()) };
        }
    }

    /// The window tick. `true` when the UI is to close (the window's close button, the
    /// UI's own close, or the plugin gone).
    pub(crate) fn pump(&mut self) -> bool {
        let Some(shared) = self.shared.upgrade() else { return true };
        let mut closed = false;
        for event in self.window.drain_events() {
            match event {
                WindowEvent::Closed => closed = true,
                WindowEvent::Resized(size) if size != self.size && size.width > 0 && size.height > 0 => {
                    self.size = size;
                    // SAFETY: the UI's own interface, live while the UI is.
                    if let Some(r) = unsafe { self.resize_ext.as_ref() } {
                        unsafe { (r.ui_resize)(self.handle, size.width as c_int, size.height as c_int) };
                    }
                }
                _ => {}
            }
        }
        if let Some((w, h)) = self.controller.resize.take() {
            let wanted = WindowSize::new(w, h);
            if wanted != self.size {
                self.size = wanted;
                let _ = self.window.set_client_size(wanted.clamped());
            }
        }
        self.send_ports(&shared);
        if let Some(idle) = self.idle {
            // SAFETY: a live UI handle.
            if unsafe { idle(self.handle) } != 0 {
                closed = true;
            }
        }
        closed
    }
}

impl Drop for UiSession {
    fn drop(&mut self) {
        let _ = self.window.set_tick_handler(None);
        // the UI's window goes before its parent
        // SAFETY: a live UI handle, cleaned up once.
        if let Some(cleanup) = unsafe { (*self.desc).cleanup } {
            let h = self.handle;
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { cleanup(h) })).is_err() {
                log::error!("an LV2 interface panicked while closing");
            }
        }
        let _ = self.window.close();
    }
}

thread_local! {
    /// Open UIs by their plugin (`Shared`'s address). Not `Send`: windows live on the UI thread.
    static SESSIONS: RefCell<HashMap<usize, UiSession>> = RefCell::new(HashMap::new());
}

fn key(shared: &Arc<Shared>) -> usize {
    Arc::as_ptr(shared) as usize
}

/// Open the plugin's UI, or raise it. UI thread (the window service's).
pub(crate) fn open(shared: &Arc<Shared>, windows: &Service, label: &Label) -> Result<(), String> {
    if !windows.on_owner_thread() {
        return Err("plugin windows are opened on the UI thread".into());
    }
    let k = key(shared);
    let raised = SESSIONS.with(|s| s.borrow().get(&k).map(UiSession::raise).is_some());
    if raised {
        return Ok(());
    }
    let spec = shared.ui()?;
    let session = UiSession::open(shared, &spec, windows, label)?;
    let tick: Rc<dyn Fn()> = Rc::new(move || {
        // a nested tick (a UI running its own loop inside idle) skips
        let closed = SESSIONS.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.get_mut(&k).map(UiSession::pump)));
        if closed == Some(true) {
            let gone = SESSIONS.with(|s| s.borrow_mut().remove(&k));
            if let Some(shared) = gone.as_ref().and_then(|g| g.shared.upgrade()) {
                shared.set_ui_open(false);
            }
            drop(gone);
        }
    });
    session.window().set_tick_handler(Some(tick)).map_err(|e| e.to_string())?;
    SESSIONS.with(|s| s.borrow_mut().insert(k, session));
    shared.set_ui_open(true);
    Ok(())
}

/// Close the plugin's UI (no-op when closed). UI thread.
pub(crate) fn close(shared: &Arc<Shared>) {
    let gone = SESSIONS.with(|s| s.borrow_mut().remove(&key(shared)));
    drop(gone);
    shared.set_ui_open(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_paths_end_in_a_slash() {
        assert_eq!(bundle_dir("/usr/lib/lv2/fil4.lv2"), "/usr/lib/lv2/fil4.lv2/");
        assert_eq!(bundle_dir("/usr/lib/lv2/fil4.lv2/"), "/usr/lib/lv2/fil4.lv2/");
    }

    #[test]
    fn the_ui_features_are_null_terminated_and_offer_parent_resize_and_idle() {
        let f = Features::new(0x1234 as *mut c_void, std::ptr::null_mut(), 1.5);
        assert!(f.ptrs.last().unwrap().is_null());
        let uris: Vec<&str> = f.list.iter().map(|x| unsafe { CStr::from_ptr(x.URI) }.to_str().unwrap()).collect();
        for want in [UI_PARENT, UI_RESIZE, UI_IDLE_INTERFACE, URID_MAP, OPTIONS_OPTIONS] {
            assert!(uris.contains(&plain(want)), "{want}");
        }
        assert_eq!(f.list[0].data as usize, 0x1234, "ui:parent carries the window id");
        assert_eq!(f.options[2].key, 0, "the options list ends with an empty entry");
        assert!(SUPPORTED_UI_FEATURES.iter().all(|s| s.ends_with('\0')));
    }

    #[test]
    fn a_resize_request_is_latched_for_the_next_tick() {
        let c = Controller { shared: Weak::new(), resize: Cell::new(None) };
        let p = std::ptr::from_ref(&c).cast_mut().cast();
        // SAFETY: a live controller.
        unsafe {
            assert_eq!(ui_asks_size(p, 640, 400), 0);
            assert_eq!(ui_asks_size(p, 0, 400), 1, "a nonsense size is refused");
            write_port(p, 3, 4, 0, std::ptr::from_ref(&1.0f32).cast()); // the plugin is gone: ignored
        }
        assert_eq!(c.resize.take(), Some((640, 400)));
    }
}
