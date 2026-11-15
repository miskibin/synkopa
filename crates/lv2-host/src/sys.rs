//! The LV2 C ABI this host speaks: the structs from the LV2 headers (lv2core, urid,
//! options, worker, state, log, atom) and the URIs it uses. Declared here from the
//! specification (<https://lv2plug.in/ns/>) rather than generated, because they are few
//! and fixed by the stable LV2 ABI.

#![allow(non_camel_case_types, non_snake_case, dead_code)]

use std::ffi::{c_char, c_int, c_void};

pub type LV2_Handle = *mut c_void;
pub type LV2_URID = u32;

#[repr(C)]
pub struct LV2_Feature {
    pub URI: *const c_char,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct LV2_Descriptor {
    pub URI: *const c_char,
    pub instantiate: Option<unsafe extern "C" fn(*const LV2_Descriptor, f64, *const c_char, *const *const LV2_Feature) -> LV2_Handle>,
    pub connect_port: Option<unsafe extern "C" fn(LV2_Handle, u32, *mut c_void)>,
    pub activate: Option<unsafe extern "C" fn(LV2_Handle)>,
    pub run: Option<unsafe extern "C" fn(LV2_Handle, u32)>,
    pub deactivate: Option<unsafe extern "C" fn(LV2_Handle)>,
    pub cleanup: Option<unsafe extern "C" fn(LV2_Handle)>,
    pub extension_data: Option<unsafe extern "C" fn(*const c_char) -> *const c_void>,
}

#[repr(C)]
pub struct LV2_URID_Map {
    pub handle: *mut c_void,
    pub map: unsafe extern "C" fn(*mut c_void, *const c_char) -> LV2_URID,
}

#[repr(C)]
pub struct LV2_URID_Unmap {
    pub handle: *mut c_void,
    pub unmap: unsafe extern "C" fn(*mut c_void, LV2_URID) -> *const c_char,
}

/// `LV2_OPTIONS_INSTANCE`.
pub const OPTIONS_INSTANCE: u32 = 0;

#[repr(C)]
pub struct LV2_Options_Option {
    pub context: u32,
    pub subject: u32,
    pub key: LV2_URID,
    pub size: u32,
    pub type_: LV2_URID,
    pub value: *const c_void,
}

pub type LV2_Worker_Status = c_int;
pub const WORKER_SUCCESS: LV2_Worker_Status = 0;
pub const WORKER_ERR_UNKNOWN: LV2_Worker_Status = 1;
pub const WORKER_ERR_NO_SPACE: LV2_Worker_Status = 2;

pub type LV2_Worker_Respond_Function = unsafe extern "C" fn(*mut c_void, u32, *const c_void) -> LV2_Worker_Status;

#[repr(C)]
pub struct LV2_Worker_Interface {
    pub work: Option<unsafe extern "C" fn(LV2_Handle, LV2_Worker_Respond_Function, *mut c_void, u32, *const c_void) -> LV2_Worker_Status>,
    pub work_response: Option<unsafe extern "C" fn(LV2_Handle, u32, *const c_void) -> LV2_Worker_Status>,
    pub end_run: Option<unsafe extern "C" fn(LV2_Handle) -> LV2_Worker_Status>,
}

#[repr(C)]
pub struct LV2_Worker_Schedule {
    pub handle: *mut c_void,
    pub schedule_work: unsafe extern "C" fn(*mut c_void, u32, *const c_void) -> LV2_Worker_Status,
}

#[repr(C)]
pub struct LV2_State_Map_Path {
    pub handle: *mut c_void,
    pub abstract_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char,
    pub absolute_path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char,
}

#[repr(C)]
pub struct LV2_State_Make_Path {
    pub handle: *mut c_void,
    pub path: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char,
}

#[repr(C)]
pub struct LV2_State_Free_Path {
    pub handle: *mut c_void,
    pub free_path: unsafe extern "C" fn(*mut c_void, *mut c_char),
}

/// `LV2_Log_Log`. `printf` is C-variadic; the host's function takes the fixed arguments
/// only and logs the format string (the System V x86-64 and AArch64 Linux calling
/// conventions pass those the same way for a variadic call). Plugins using the
/// `lv2_log_*` helpers go through `vprintf`, which is formatted in full.
#[repr(C)]
pub struct LV2_Log_Log {
    pub handle: *mut c_void,
    pub printf: unsafe extern "C" fn(*mut c_void, LV2_URID, *const c_char) -> c_int,
    pub vprintf: unsafe extern "C" fn(*mut c_void, LV2_URID, *const c_char, *mut c_void) -> c_int,
}

#[repr(C)]
pub struct LV2_Atom {
    pub size: u32,
    pub type_: u32,
}

/// `LV2_STATE_IS_POD | LV2_STATE_IS_PORTABLE`.
pub const STATE_POD_PORTABLE: u32 = 1 | 2;

macro_rules! uris {
    ($($name:ident = $uri:literal;)*) => {
        $(pub const $name: &str = concat!($uri, "\0");)*
    };
}

// NUL-terminated, ready for C.
uris! {
    LV2_CORE_INPUT_PORT = "http://lv2plug.in/ns/lv2core#InputPort";
    LV2_CORE_OUTPUT_PORT = "http://lv2plug.in/ns/lv2core#OutputPort";
    LV2_CORE_AUDIO_PORT = "http://lv2plug.in/ns/lv2core#AudioPort";
    LV2_CORE_CONTROL_PORT = "http://lv2plug.in/ns/lv2core#ControlPort";
    LV2_CORE_CV_PORT = "http://lv2plug.in/ns/lv2core#CVPort";
    LV2_CORE_CONNECTION_OPTIONAL = "http://lv2plug.in/ns/lv2core#connectionOptional";
    LV2_CORE_INTEGER = "http://lv2plug.in/ns/lv2core#integer";
    LV2_CORE_TOGGLED = "http://lv2plug.in/ns/lv2core#toggled";
    LV2_CORE_ENUMERATION = "http://lv2plug.in/ns/lv2core#enumeration";
    LV2_CORE_SAMPLE_RATE = "http://lv2plug.in/ns/lv2core#sampleRate";
    LV2_CORE_REPORTS_LATENCY = "http://lv2plug.in/ns/lv2core#reportsLatency";
    LV2_CORE_DESIGNATION = "http://lv2plug.in/ns/lv2core#designation";
    LV2_CORE_LATENCY = "http://lv2plug.in/ns/lv2core#latency";
    LV2_CORE_ENABLED = "http://lv2plug.in/ns/lv2core#enabled";
    LV2_CORE_FREE_WHEELING = "http://lv2plug.in/ns/lv2core#freeWheeling";
    LV2_CORE_IS_SIDE_CHAIN = "http://lv2plug.in/ns/lv2core#isSideChain";
    LV2_CORE_INSTRUMENT_PLUGIN = "http://lv2plug.in/ns/lv2core#InstrumentPlugin";
    LV2_CORE_HARD_RT_CAPABLE = "http://lv2plug.in/ns/lv2core#hardRTCapable";
    LV2_CORE_IN_PLACE_BROKEN = "http://lv2plug.in/ns/lv2core#inPlaceBroken";
    LV2_CORE_IS_LIVE = "http://lv2plug.in/ns/lv2core#isLive";
    PPROPS_LOGARITHMIC = "http://lv2plug.in/ns/ext/port-props#logarithmic";
    PPROPS_NOT_ON_GUI = "http://lv2plug.in/ns/ext/port-props#notOnGUI";
    UNITS_UNIT = "http://lv2plug.in/ns/extensions/units#unit";
    ATOM_ATOM_PORT = "http://lv2plug.in/ns/ext/atom#AtomPort";
    ATOM_SEQUENCE = "http://lv2plug.in/ns/ext/atom#Sequence";
    ATOM_CHUNK = "http://lv2plug.in/ns/ext/atom#Chunk";
    ATOM_FLOAT = "http://lv2plug.in/ns/ext/atom#Float";
    ATOM_INT = "http://lv2plug.in/ns/ext/atom#Int";
    EVENT_EVENT_PORT = "http://lv2plug.in/ns/ext/event#EventPort";
    TIME_BPM = "http://lv2plug.in/ns/ext/time#beatsPerMinute";
    URID_MAP = "http://lv2plug.in/ns/ext/urid#map";
    URID_UNMAP = "http://lv2plug.in/ns/ext/urid#unmap";
    OPTIONS_OPTIONS = "http://lv2plug.in/ns/ext/options#options";
    OPTIONS_REQUIRED_OPTION = "http://lv2plug.in/ns/ext/options#requiredOption";
    BUF_SIZE_BOUNDED = "http://lv2plug.in/ns/ext/buf-size#boundedBlockLength";
    BUF_SIZE_MIN = "http://lv2plug.in/ns/ext/buf-size#minBlockLength";
    BUF_SIZE_MAX = "http://lv2plug.in/ns/ext/buf-size#maxBlockLength";
    BUF_SIZE_NOMINAL = "http://lv2plug.in/ns/ext/buf-size#nominalBlockLength";
    PARAM_SAMPLE_RATE = "http://lv2plug.in/ns/ext/parameters#sampleRate";
    WORKER_SCHEDULE = "http://lv2plug.in/ns/ext/worker#schedule";
    WORKER_INTERFACE = "http://lv2plug.in/ns/ext/worker#interface";
    STATE_INTERFACE = "http://lv2plug.in/ns/ext/state#interface";
    STATE_MAP_PATH = "http://lv2plug.in/ns/ext/state#mapPath";
    STATE_MAKE_PATH = "http://lv2plug.in/ns/ext/state#makePath";
    STATE_FREE_PATH = "http://lv2plug.in/ns/ext/state#freePath";
    STATE_LOAD_DEFAULT = "http://lv2plug.in/ns/ext/state#loadDefaultState";
    STATE_THREAD_SAFE_RESTORE = "http://lv2plug.in/ns/ext/state#threadSafeRestore";
    LOG_LOG = "http://lv2plug.in/ns/ext/log#log";
    LOG_ERROR = "http://lv2plug.in/ns/ext/log#Error";
    LOG_WARNING = "http://lv2plug.in/ns/ext/log#Warning";
    LOG_TRACE = "http://lv2plug.in/ns/ext/log#Trace";
}

/// Features a plugin may require that this host provides (or, for the plugin-side
/// declarations among them, that never stop it from running here).
pub const SUPPORTED_FEATURES: &[&str] = &[
    URID_MAP,
    URID_UNMAP,
    OPTIONS_OPTIONS,
    BUF_SIZE_BOUNDED,
    WORKER_SCHEDULE,
    STATE_MAP_PATH,
    STATE_MAKE_PATH,
    STATE_FREE_PATH,
    STATE_LOAD_DEFAULT,
    STATE_THREAD_SAFE_RESTORE,
    LOG_LOG,
    LV2_CORE_IN_PLACE_BROKEN,
    LV2_CORE_HARD_RT_CAPABLE,
    LV2_CORE_IS_LIVE,
];

/// Options a plugin may require (`opts:requiredOption`) that this host passes.
pub const SUPPORTED_OPTIONS: &[&str] = &[PARAM_SAMPLE_RATE, BUF_SIZE_MIN, BUF_SIZE_MAX, BUF_SIZE_NOMINAL];

/// A URI constant without its NUL, for messages and comparisons with Rust strings.
pub fn plain(uri: &str) -> &str {
    uri.strip_suffix('\0').unwrap_or(uri)
}

/// The short name of a feature or option URI (`fixedBlockLength`), for a sentence.
pub fn short_name(uri: &str) -> &str {
    let uri = plain(uri);
    uri.rsplit(['#', '/']).find(|s| !s.is_empty()).unwrap_or(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_are_nul_terminated_for_c_and_readable_without() {
        assert!(URID_MAP.ends_with('\0'));
        assert_eq!(plain(URID_MAP), "http://lv2plug.in/ns/ext/urid#map");
        assert_eq!(short_name("http://lv2plug.in/ns/ext/buf-size#fixedBlockLength"), "fixedBlockLength");
        assert_eq!(short_name("http://lv2plug.in/ns/ext/instance-access"), "instance-access");
        assert!(SUPPORTED_FEATURES.iter().all(|f| f.ends_with('\0')));
    }
}
