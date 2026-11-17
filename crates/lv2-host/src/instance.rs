//! One instantiated LV2 plugin: the host features it was given (which must outlive it),
//! its worker, and the descriptor calls (`connect_port`, `activate`, `run`, ...).

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::Arc;

use crate::lilv::{self, Lilv, Plugin};
use crate::sys::*;
use crate::urid;
use crate::worker::{self, Worker};
use crate::world::World;

/// Numbers the instances' `state:makePath` directories.
static NEXT_SCRATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Everything handed to the plugin as `LV2_Feature`s. Boxed: the plugin keeps pointers
/// into it for its whole life.
pub(crate) struct Features {
    sample_rate: f32,
    min_block: i32,
    max_block: i32,
    nominal_block: i32,
    options: [LV2_Options_Option; 5],
    schedule: LV2_Worker_Schedule,
    map_path: LV2_State_Map_Path,
    make_path: LV2_State_Make_Path,
    free_path: LV2_State_Free_Path,
    log: LV2_Log_Log,
    /// `state:makePath` root for this instance.
    scratch: CString,
    list: Vec<LV2_Feature>,
    ptrs: Vec<*const LV2_Feature>,
}

impl Features {
    /// Features for an instance at `sample_rate` with blocks of at most `max_block`.
    /// `schedule`: the worker's handle, when the plugin has a worker.
    fn new(sample_rate: f32, max_block: usize, schedule: Option<*mut c_void>) -> Box<Self> {
        let block = i32::try_from(max_block).unwrap_or(i32::MAX);
        let scratch = wave_plugin_scan::data_dir().unwrap_or_else(std::env::temp_dir).join("LV2").join("files").join(format!(
            "{}-{}",
            std::process::id(),
            NEXT_SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut f = Box::new(Self {
            sample_rate,
            min_block: 1,
            max_block: block,
            nominal_block: block,
            options: std::array::from_fn(|_| LV2_Options_Option {
                context: OPTIONS_INSTANCE,
                subject: 0,
                key: 0,
                size: 0,
                type_: 0,
                value: std::ptr::null(),
            }),
            schedule: LV2_Worker_Schedule { handle: schedule.unwrap_or(std::ptr::null_mut()), schedule_work: worker::schedule_work },
            map_path: LV2_State_Map_Path { handle: std::ptr::null_mut(), abstract_path: path_copy, absolute_path: path_copy },
            make_path: LV2_State_Make_Path { handle: std::ptr::null_mut(), path: make_path },
            free_path: LV2_State_Free_Path { handle: std::ptr::null_mut(), free_path },
            log: LV2_Log_Log { handle: std::ptr::null_mut(), printf: log_printf, vprintf: log_vprintf },
            scratch: CString::new(scratch.to_string_lossy().into_owned()).unwrap_or_default(),
            list: vec![],
            ptrs: vec![],
        });
        // the box does not move from here on: point into it
        let (float, int) = (urid::map_str(ATOM_FLOAT), urid::map_str(ATOM_INT));
        let opts: [(&str, u32, *const c_void); 4] = [
            (PARAM_SAMPLE_RATE, float, std::ptr::addr_of!(f.sample_rate).cast()),
            (BUF_SIZE_MIN, int, std::ptr::addr_of!(f.min_block).cast()),
            (BUF_SIZE_MAX, int, std::ptr::addr_of!(f.max_block).cast()),
            (BUF_SIZE_NOMINAL, int, std::ptr::addr_of!(f.nominal_block).cast()),
        ];
        for (i, (key, ty, value)) in opts.into_iter().enumerate() {
            f.options[i] = LV2_Options_Option { context: OPTIONS_INSTANCE, subject: 0, key: urid::map_str(key), size: 4, type_: ty, value };
        }
        f.make_path.handle = f.scratch.as_ptr().cast_mut().cast();
        let feature = |uri: &'static str, data: *mut c_void| LV2_Feature { URI: uri.as_ptr().cast(), data };
        let mut list = vec![
            feature(URID_MAP, urid::map_feature().cast()),
            feature(URID_UNMAP, urid::unmap_feature().cast()),
            feature(OPTIONS_OPTIONS, f.options.as_mut_ptr().cast()),
            feature(BUF_SIZE_BOUNDED, std::ptr::null_mut()),
            feature(STATE_MAP_PATH, std::ptr::addr_of_mut!(f.map_path).cast()),
            feature(STATE_MAKE_PATH, std::ptr::addr_of_mut!(f.make_path).cast()),
            feature(STATE_FREE_PATH, std::ptr::addr_of_mut!(f.free_path).cast()),
            feature(LOG_LOG, std::ptr::addr_of_mut!(f.log).cast()),
        ];
        if schedule.is_some() {
            list.push(feature(WORKER_SCHEDULE, std::ptr::addr_of_mut!(f.schedule).cast()));
        }
        f.list = list;
        f.ptrs = f.list.iter().map(|x| x as *const LV2_Feature).chain(std::iter::once(std::ptr::null())).collect();
        f
    }

    /// The NULL-terminated feature array.
    pub fn as_ptr(&self) -> *const *const LV2_Feature {
        self.ptrs.as_ptr()
    }
}

// SAFETY: plain data plus pointers into itself and into process-wide statics.
unsafe impl Send for Features {}
unsafe impl Sync for Features {}

/// `malloc`'d copy of a C string (what `state:freePath` frees).
unsafe fn malloc_copy(s: &CStr) -> *mut c_char {
    let bytes = s.to_bytes_with_nul();
    // SAFETY: a fresh allocation of the right size, filled completely.
    unsafe {
        let p = libc::malloc(bytes.len()).cast::<c_char>();
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), p, bytes.len());
        }
        p
    }
}

/// `state:mapPath` both ways: Synkopa keeps absolute paths (collecting a plugin's files
/// into the project is not done yet).
unsafe extern "C" fn path_copy(_handle: *mut c_void, path: *const c_char) -> *mut c_char {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: a NUL-terminated string from the plugin.
    unsafe { malloc_copy(CStr::from_ptr(path)) }
}

/// `state:makePath`: a file under this instance's scratch directory (created on demand).
unsafe extern "C" fn make_path(handle: *mut c_void, path: *const c_char) -> *mut c_char {
    if handle.is_null() || path.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: `handle` is the instance's `scratch` CString; `path` is from the plugin.
    let (root, rel) = unsafe { (CStr::from_ptr(handle.cast()), CStr::from_ptr(path)) };
    let full: PathBuf = PathBuf::from(root.to_string_lossy().as_ref()).join(rel.to_string_lossy().trim_start_matches('/'));
    if let Some(dir) = full.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match CString::new(full.to_string_lossy().into_owned()) {
        // SAFETY: as `path_copy`.
        Ok(c) => unsafe { malloc_copy(&c) },
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn free_path(_handle: *mut c_void, path: *mut c_char) {
    // SAFETY: `path` came from `malloc_copy` (or is null).
    unsafe { libc::free(path.cast()) };
}

fn log_level(ty: u32) -> Option<log::Level> {
    if ty == urid::map_str(LOG_TRACE) {
        // may come from the audio thread: dropped (formatting would allocate)
        None
    } else if ty == urid::map_str(LOG_ERROR) {
        Some(log::Level::Error)
    } else if ty == urid::map_str(LOG_WARNING) {
        Some(log::Level::Warn)
    } else {
        Some(log::Level::Info)
    }
}

pub(crate) unsafe extern "C" fn log_printf(_handle: *mut c_void, ty: u32, fmt: *const c_char) -> c_int {
    let Some(level) = log_level(ty) else { return 0 };
    if fmt.is_null() {
        return 0;
    }
    // SAFETY: a NUL-terminated format string from the plugin.
    let s = unsafe { CStr::from_ptr(fmt) }.to_string_lossy();
    log::log!(level, "LV2 plugin: {}", s.trim_end());
    s.len() as c_int
}

extern "C" {
    fn vsnprintf(buf: *mut c_char, size: usize, fmt: *const c_char, ap: *mut c_void) -> c_int;
}

pub(crate) unsafe extern "C" fn log_vprintf(_handle: *mut c_void, ty: u32, fmt: *const c_char, ap: *mut c_void) -> c_int {
    let Some(level) = log_level(ty) else { return 0 };
    if fmt.is_null() {
        return 0;
    }
    let mut buf = [0 as c_char; 512];
    // SAFETY: `ap` is the plugin's va_list for `fmt`; the buffer is ours and bounded.
    let n = unsafe { vsnprintf(buf.as_mut_ptr(), buf.len(), fmt, ap) };
    // SAFETY: vsnprintf NUL-terminates within the buffer.
    let s = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy();
    log::log!(level, "LV2 plugin: {}", s.trim_end());
    n
}

/// One live plugin instance. Created and dropped off the audio thread; `run` and
/// `connect_port` from the one thread that processes it at a time.
pub(crate) struct Inst {
    raw: *mut lilv::Instance,
    desc: *const LV2_Descriptor,
    handle: LV2_Handle,
    /// Dropped (thread joined) before the instance is freed: see `Drop`.
    worker: Option<Worker>,
    features: Box<Features>,
    active: bool,
    /// Keeps the plugin's world (and so `plugin`) alive.
    pub world: Arc<World>,
    pub plugin: *const Plugin,
}

// SAFETY: the raw pointers are the plugin's; LV2 lets an instance move between threads
// as long as its threading classes are respected, which the owners do (one processing
// thread at a time, state saved concurrently only as the state extension allows).
unsafe impl Send for Inst {}
unsafe impl Sync for Inst {}

impl Inst {
    /// Instantiate `plugin` (lilv lock held by the caller). Not yet active.
    ///
    /// # Safety
    /// `plugin` belongs to `world`; the lilv lock is held.
    pub unsafe fn new(
        l: &Lilv,
        world: Arc<World>,
        plugin: *const Plugin,
        name: &str,
        sample_rate: f32,
        max_block: usize,
        has_worker: bool,
    ) -> Result<Self, String> {
        let mut worker = has_worker.then(Worker::new);
        let features = Features::new(sample_rate, max_block, worker.as_ref().map(Worker::schedule_handle));
        // SAFETY: lilv call under the caller's lock; the features outlive the instance.
        let raw = unsafe { (l.lilv_plugin_instantiate)(plugin, f64::from(sample_rate), features.as_ptr()) };
        if raw.is_null() {
            return Err(format!("{name} could not be instantiated at {sample_rate} Hz (the plugin refused)"));
        }
        // SAFETY: `raw` is a live LilvInstance.
        let (desc, handle) = unsafe { ((*raw).lv2_descriptor, (*raw).lv2_handle) };
        let inst_worker = worker.take();
        let mut inst = Self { raw, desc, handle, worker: inst_worker, features, active: false, world, plugin };
        let iface = inst.extension_data(WORKER_INTERFACE).cast::<LV2_Worker_Interface>();
        if let Some(w) = inst.worker.as_mut() {
            // SAFETY: a non-null worker interface is valid for the instance's life.
            if !iface.is_null() && unsafe { (*iface).work.is_some() } {
                // SAFETY: handle and iface outlive the worker (dropped first in `Drop`).
                if let Err(e) = unsafe { w.start(handle, iface, name) } {
                    log::warn!("LV2 plugin '{name}': no worker thread ({e}); its background work is refused");
                }
            }
        }
        Ok(inst)
    }

    /// The plugin's extension data for `uri` (NUL-terminated), or null.
    pub fn extension_data(&self, uri: &str) -> *const c_void {
        // SAFETY: the descriptor is valid while the instance lives.
        match unsafe { (*self.desc).extension_data } {
            Some(f) => unsafe { f(uri.as_ptr().cast()) },
            None => std::ptr::null(),
        }
    }

    /// # Safety
    /// `data` stays valid (and large enough) until reconnected or the instance is freed;
    /// not concurrent with `run`.
    pub unsafe fn connect(&self, port: u32, data: *mut c_void) {
        // SAFETY: as documented.
        if let Some(f) = unsafe { (*self.desc).connect_port } {
            unsafe { f(self.handle, port, data) };
        }
    }

    pub fn activate(&mut self) {
        if !self.active {
            // SAFETY: instantiation-class call from the owning (non-RT) thread.
            if let Some(f) = unsafe { (*self.desc).activate } {
                unsafe { f(self.handle) };
            }
            self.active = true;
        }
    }

    fn deactivate(&mut self) {
        if self.active {
            // SAFETY: as `activate`.
            if let Some(f) = unsafe { (*self.desc).deactivate } {
                unsafe { f(self.handle) };
            }
            self.active = false;
        }
    }

    /// Run `frames` frames, then deliver the worker's answers. Audio class: no
    /// allocation, no lock.
    ///
    /// # Safety
    /// Every non-optional port is connected to a buffer of at least `frames` samples
    /// (`frames` ≤ the max block the instance was created for); one thread at a time.
    pub unsafe fn run(&mut self, frames: u32) {
        // SAFETY: as documented.
        unsafe {
            if let Some(f) = (*self.desc).run {
                f(self.handle, frames);
            }
            if let Some(w) = self.worker.as_mut() {
                w.after_run(self.handle);
            }
        }
    }

    /// The plugin's own handle (`instance-access`: what a UI that needs it is given).
    pub fn handle(&self) -> LV2_Handle {
        self.handle
    }

    /// The raw lilv instance (state save/restore).
    pub fn raw(&self) -> *mut lilv::Instance {
        self.raw
    }

    /// The NULL-terminated feature array (state save/restore get the same features).
    pub fn features(&self) -> *const *const LV2_Feature {
        self.features.as_ptr()
    }
}

impl Drop for Inst {
    fn drop(&mut self) {
        // the worker thread calls into the instance: stop it first
        drop(self.worker.take());
        self.deactivate();
        if let Ok(l) = lilv::lilv() {
            let _g = lilv::lock();
            // SAFETY: freed once, nothing runs it any more.
            unsafe { (l.lilv_instance_free)(self.raw) };
        }
        let _ = std::fs::remove_dir(PathBuf::from(self.features.scratch.to_string_lossy().as_ref()));
    }
}
