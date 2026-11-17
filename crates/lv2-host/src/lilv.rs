//! liblilv, opened at run time.
//!
//! Synkopa does not link liblilv: a distribution without it (or an AppImage on a system
//! without it) still starts, and LV2 is reported as unavailable with the reason. The
//! declarations follow `lilv/lilv.h` (lilv 0.24, the API every current distribution
//! ships); only the functions this host calls are bound. All calls into lilv go through
//! [`lock`]: lilv is not thread-safe.

#![allow(clippy::type_complexity)]

use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::OnceLock;

use parking_lot::{ReentrantMutex, ReentrantMutexGuard};

use crate::sys::{LV2_Feature, LV2_URID_Map, LV2_URID_Unmap};

pub type World = c_void;
pub type Plugins = c_void;
pub type Plugin = c_void;
pub type PluginClass = c_void;
pub type Port = c_void;
pub type Node = c_void;
pub type Nodes = c_void;
pub type Iter = c_void;
pub type ScalePoints = c_void;
pub type ScalePoint = c_void;
pub type State = c_void;

/// `LilvInstance`: public in lilv.h, its run/connect helpers are inline functions there.
#[repr(C)]
pub struct Instance {
    pub lv2_descriptor: *const crate::sys::LV2_Descriptor,
    pub lv2_handle: *mut c_void,
    pub pimpl: *mut c_void,
}

pub type GetPortValue = unsafe extern "C" fn(*const c_char, *mut c_void, *mut u32, *mut u32) -> *const c_void;
pub type SetPortValue = unsafe extern "C" fn(*const c_char, *mut c_void, *const c_void, u32, u32);

macro_rules! lilv_api {
    ($($name:ident: fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        /// The bound liblilv functions.
        pub struct Lilv {
            $(pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl Lilv {
            /// # Safety
            /// `lib` is a live `dlopen` handle of liblilv-0.
            unsafe fn bind(lib: *mut c_void) -> Result<Self, String> {
                Ok(Self {
                    $($name: {
                        let sym = unsafe { libc::dlsym(lib, concat!(stringify!($name), "\0").as_ptr().cast()) };
                        if sym.is_null() {
                            return Err(format!("{} has no `{}` (too old?)", LIB_NAMES[0], stringify!($name)));
                        }
                        // SAFETY: the symbol is the lilv.h function of this signature.
                        unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(sym) }
                    },)*
                })
            }
        }
    };
}

lilv_api! {
    lilv_world_new: fn() -> *mut World;
    lilv_world_free: fn(*mut World);
    lilv_world_load_all: fn(*mut World);
    lilv_world_get_all_plugins: fn(*const World) -> *const Plugins;
    lilv_new_uri: fn(*mut World, *const c_char) -> *mut Node;
    lilv_node_free: fn(*mut Node);
    lilv_node_as_string: fn(*const Node) -> *const c_char;
    lilv_node_as_uri: fn(*const Node) -> *const c_char;
    lilv_node_is_float: fn(*const Node) -> bool;
    lilv_node_is_int: fn(*const Node) -> bool;
    lilv_node_is_bool: fn(*const Node) -> bool;
    lilv_node_as_float: fn(*const Node) -> f32;
    lilv_node_as_int: fn(*const Node) -> c_int;
    lilv_node_as_bool: fn(*const Node) -> bool;
    lilv_file_uri_parse: fn(*const c_char, *mut *mut c_char) -> *mut c_char;
    lilv_free: fn(*mut c_void);
    lilv_plugins_begin: fn(*const Plugins) -> *mut Iter;
    lilv_plugins_get: fn(*const Plugins, *mut Iter) -> *const Plugin;
    lilv_plugins_next: fn(*const Plugins, *mut Iter) -> *mut Iter;
    lilv_plugins_is_end: fn(*const Plugins, *mut Iter) -> bool;
    lilv_plugins_get_by_uri: fn(*const Plugins, *const Node) -> *const Plugin;
    lilv_plugin_get_uri: fn(*const Plugin) -> *const Node;
    lilv_plugin_get_bundle_uri: fn(*const Plugin) -> *const Node;
    lilv_plugin_get_name: fn(*const Plugin) -> *mut Node;
    lilv_plugin_get_author_name: fn(*const Plugin) -> *mut Node;
    lilv_plugin_get_class: fn(*const Plugin) -> *const PluginClass;
    lilv_plugin_class_get_label: fn(*const PluginClass) -> *const Node;
    lilv_plugin_class_get_uri: fn(*const PluginClass) -> *const Node;
    lilv_plugin_get_required_features: fn(*const Plugin) -> *mut Nodes;
    lilv_plugin_get_value: fn(*const Plugin, *const Node) -> *mut Nodes;
    lilv_plugin_has_feature: fn(*const Plugin, *const Node) -> bool;
    lilv_plugin_has_extension_data: fn(*const Plugin, *const Node) -> bool;
    lilv_plugin_get_num_ports: fn(*const Plugin) -> u32;
    lilv_plugin_get_port_by_index: fn(*const Plugin, u32) -> *const Port;
    lilv_nodes_begin: fn(*const Nodes) -> *mut Iter;
    lilv_nodes_get: fn(*const Nodes, *mut Iter) -> *const Node;
    lilv_nodes_next: fn(*const Nodes, *mut Iter) -> *mut Iter;
    lilv_nodes_is_end: fn(*const Nodes, *mut Iter) -> bool;
    lilv_nodes_free: fn(*mut Nodes);
    lilv_port_is_a: fn(*const Plugin, *const Port, *const Node) -> bool;
    lilv_port_has_property: fn(*const Plugin, *const Port, *const Node) -> bool;
    lilv_port_get_symbol: fn(*const Plugin, *const Port) -> *const Node;
    lilv_port_get_name: fn(*const Plugin, *const Port) -> *mut Node;
    lilv_port_get_range: fn(*const Plugin, *const Port, *mut *mut Node, *mut *mut Node, *mut *mut Node);
    lilv_port_get: fn(*const Plugin, *const Port, *const Node) -> *mut Node;
    lilv_port_get_scale_points: fn(*const Plugin, *const Port) -> *mut ScalePoints;
    lilv_scale_points_free: fn(*mut ScalePoints);
    lilv_scale_points_begin: fn(*const ScalePoints) -> *mut Iter;
    lilv_scale_points_get: fn(*const ScalePoints, *mut Iter) -> *const ScalePoint;
    lilv_scale_points_next: fn(*const ScalePoints, *mut Iter) -> *mut Iter;
    lilv_scale_points_is_end: fn(*const ScalePoints, *mut Iter) -> bool;
    lilv_scale_point_get_label: fn(*const ScalePoint) -> *const Node;
    lilv_scale_point_get_value: fn(*const ScalePoint) -> *const Node;
    lilv_plugin_instantiate: fn(*const Plugin, f64, *const *const LV2_Feature) -> *mut Instance;
    lilv_instance_free: fn(*mut Instance);
    lilv_state_new_from_instance: fn(*const Plugin, *mut Instance, *mut LV2_URID_Map, *const c_char, *const c_char, *const c_char, *const c_char, Option<GetPortValue>, *mut c_void, u32, *const *const LV2_Feature) -> *mut State;
    lilv_state_new_from_world: fn(*mut World, *mut LV2_URID_Map, *const Node) -> *mut State;
    lilv_state_new_from_string: fn(*mut World, *mut LV2_URID_Map, *const c_char) -> *mut State;
    lilv_state_to_string: fn(*mut World, *mut LV2_URID_Map, *mut LV2_URID_Unmap, *const State, *const c_char, *const c_char) -> *mut c_char;
    lilv_state_restore: fn(*const State, *mut Instance, Option<SetPortValue>, *mut c_void, u32, *const *const LV2_Feature);
    lilv_state_free: fn(*mut State);
    lilv_plugin_get_uis: fn(*const Plugin) -> *mut Uis;
    lilv_uis_begin: fn(*const Uis) -> *mut Iter;
    lilv_uis_get: fn(*const Uis, *mut Iter) -> *const Ui;
    lilv_uis_next: fn(*const Uis, *mut Iter) -> *mut Iter;
    lilv_uis_is_end: fn(*const Uis, *mut Iter) -> bool;
    lilv_uis_free: fn(*mut Uis);
    lilv_ui_get_uri: fn(*const Ui) -> *const Node;
    lilv_ui_is_a: fn(*const Ui, *const Node) -> bool;
    lilv_ui_get_bundle_uri: fn(*const Ui) -> *const Node;
    lilv_ui_get_binary_uri: fn(*const Ui) -> *const Node;
    lilv_world_load_resource: fn(*mut World, *const Node) -> c_int;
    lilv_world_find_nodes: fn(*mut World, *const Node, *const Node, *const Node) -> *mut Nodes;
}

pub type Uis = c_void;
pub type Ui = c_void;

// SAFETY: plain function pointers into a library that is never unloaded.
unsafe impl Send for Lilv {}
unsafe impl Sync for Lilv {}

/// Sonames tried, in order (the runtime package ships the first; `-dev` the second).
pub const LIB_NAMES: [&str; 2] = ["liblilv-0.so.0", "liblilv-0.so"];

/// liblilv, loaded on first use; the error says what to install.
pub fn lilv() -> Result<&'static Lilv, String> {
    static LILV: OnceLock<Result<Lilv, String>> = OnceLock::new();
    LILV.get_or_init(|| {
        for name in LIB_NAMES {
            let c = std::ffi::CString::new(name).expect("no NUL");
            // SAFETY: a plain dlopen; the handle is never closed.
            let lib = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if !lib.is_null() {
                // SAFETY: `lib` is liblilv.
                return unsafe { Lilv::bind(lib) };
            }
        }
        // SAFETY: dlerror returns a thread-local string or null.
        let why = unsafe {
            let e = libc::dlerror();
            if e.is_null() {
                String::new()
            } else {
                CStr::from_ptr(e).to_string_lossy().into_owned()
            }
        };
        Err(format!(
            "LV2 is unavailable: the lilv library ({}) is not installed. Install it with your package manager \
             (Debian/Ubuntu: liblilv-0-0, Fedora: lilv-libs, Arch: lilv) and restart Synkopa. ({why})",
            LIB_NAMES[0]
        ))
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// The lock every lilv call is made under.
pub fn lock() -> ReentrantMutexGuard<'static, ()> {
    static LOCK: OnceLock<ReentrantMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| ReentrantMutex::new(())).lock()
}

/// A node's string, copied (empty for null).
///
/// # Safety
/// `node` is null or a live lilv node; the lilv lock is held.
pub unsafe fn node_string(l: &Lilv, node: *const Node) -> String {
    if node.is_null() {
        return String::new();
    }
    let s = unsafe { (l.lilv_node_as_string)(node) };
    if s.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
    }
}

/// A node's number (float, int or bool), or NaN.
///
/// # Safety
/// As [`node_string`].
pub unsafe fn node_f32(l: &Lilv, node: *const Node) -> f32 {
    unsafe {
        if node.is_null() {
            f32::NAN
        } else if (l.lilv_node_is_float)(node) {
            (l.lilv_node_as_float)(node)
        } else if (l.lilv_node_is_int)(node) {
            (l.lilv_node_as_int)(node) as f32
        } else if (l.lilv_node_is_bool)(node) {
            f32::from(u8::from((l.lilv_node_as_bool)(node)))
        } else {
            f32::NAN
        }
    }
}

/// Every node of a collection as strings, then frees it.
///
/// # Safety
/// `nodes` is null or a `LilvNodes` owned by the caller; the lilv lock is held.
pub unsafe fn take_strings(l: &Lilv, nodes: *mut Nodes) -> Vec<String> {
    let mut out = vec![];
    if nodes.is_null() {
        return out;
    }
    unsafe {
        let mut it = (l.lilv_nodes_begin)(nodes);
        while !(l.lilv_nodes_is_end)(nodes, it) {
            out.push(node_string(l, (l.lilv_nodes_get)(nodes, it)));
            it = (l.lilv_nodes_next)(nodes, it);
        }
        (l.lilv_nodes_free)(nodes);
    }
    out
}

/// A local path for a `file://` URI node.
///
/// # Safety
/// As [`node_string`].
pub unsafe fn node_path(l: &Lilv, node: *const Node) -> String {
    if node.is_null() {
        return String::new();
    }
    unsafe {
        let uri = (l.lilv_node_as_uri)(node);
        if uri.is_null() {
            return String::new();
        }
        let p = (l.lilv_file_uri_parse)(uri, std::ptr::null_mut());
        if p.is_null() {
            return node_string(l, node);
        }
        let s = CStr::from_ptr(p).to_string_lossy().into_owned();
        (l.lilv_free)(p.cast());
        s
    }
}
