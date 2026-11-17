//! One hosted LV2 effect: the real-time [`Lv2Effect`] and the non-RT [`Lv2Handle`].
//!
//! # Threading
//!
//! The effect owns its instance(s) and every buffer they are connected to; the audio
//! thread runs it without locks or allocation. Parameter values live in atomics in
//! [`Shared`] (one per port): the handle and automation write them, the effect copies
//! them into the control port buffers before each `run()`. State is saved by the handle
//! while the effect may be running (the LV2 state extension allows `save()` concurrently
//! with `run()`), through a pointer the effect withdraws (under a mutex the audio thread
//! never takes) before its instances go.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use wave_engine::{ProcessCtx, Processor};

use crate::instance::Inst;
use crate::layout::Layout;
use crate::lilv::{self, Lilv, Plugin};
use crate::params::ParamInfo;
use crate::sys::{self, LV2_Atom, STATE_POD_PORTABLE};
use crate::urid;
use crate::world::{self, Designation, Model, PortKind, World};

/// Bytes of each atom port buffer (an empty input sequence; room for output events,
/// which WAVE discards).
const ATOM_BYTES: usize = 8192;
/// The subject URI of a saved state.
const STATE_URI: &str = "urn:wave:lv2-state\0";

/// One installed LV2 effect, as the plugin browser lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    /// The bundle directory.
    pub path: String,
    /// The plugin URI (what a project stores as `cid`).
    pub cid: String,
    pub name: String,
    pub vendor: String,
    /// The plugin class label (`Delay`, `Equaliser`, ...).
    pub category: String,
    /// Why it cannot be used here; `None` when it can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported: Option<String>,
    /// The plugin declares `lv2:hardRTCapable` (safe for live use).
    pub realtime: bool,
    /// It has a worker (`worker:interface`): background work runs on a host thread.
    #[serde(skip)]
    pub worker: bool,
}

impl PluginInfo {
    pub(crate) fn of(m: &Model) -> Self {
        Self {
            path: m.bundle.clone(),
            cid: m.uri.clone(),
            name: if m.name.is_empty() { m.uri.clone() } else { m.name.clone() },
            vendor: m.vendor.clone(),
            category: m.category.clone(),
            unsupported: m.unsupported.clone(),
            realtime: m.realtime,
            worker: m.has_worker,
        }
    }
}

/// What the handle needs to save the state of a running instance.
struct StateTarget {
    raw: *mut lilv::Instance,
    features: *const *const sys::LV2_Feature,
    plugin: *const Plugin,
    world: Arc<World>,
}

// SAFETY: the pointers are valid while the effect that published them keeps its
// instances; it withdraws them (under the mutex) before dropping or replacing them.
unsafe impl Send for StateTarget {}

pub(crate) struct Shared {
    info: PluginInfo,
    model: Model,
    /// Control port values (f32 bits) by port index: inputs as set, outputs as the plugin
    /// last wrote them (meters, for its own interface); other ports' slots are unused.
    values: Vec<AtomicU32>,
    latency: AtomicU32,
    dirty: AtomicBool,
    target: Mutex<Option<StateTarget>>,
    /// The plugin's own interface WAVE can embed, or why none (looked up once).
    ui: std::sync::OnceLock<Result<crate::ui::UiSpec, String>>,
    ui_open: AtomicBool,
}

impl Shared {
    fn new(model: Model) -> Self {
        let values = model
            .ports
            .iter()
            .map(|p| match &p.kind {
                PortKind::ControlIn { spec, .. } => AtomicU32::new(spec.def.to_bits()),
                _ => AtomicU32::new(0),
            })
            .collect();
        Self {
            info: PluginInfo::of(&model),
            model,
            values,
            latency: AtomicU32::new(0),
            dirty: AtomicBool::new(false),
            target: Mutex::new(None),
            ui: std::sync::OnceLock::new(),
            ui_open: AtomicBool::new(false),
        }
    }

    pub(crate) fn uri(&self) -> &str {
        &self.model.uri
    }

    pub(crate) fn port_count(&self) -> usize {
        self.model.ports.len()
    }

    /// Indices of the control ports (inputs and outputs).
    pub(crate) fn control_ports(&self) -> impl Iterator<Item = usize> + '_ {
        self.model
            .ports
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p.kind, PortKind::ControlIn { .. } | PortKind::ControlOut { .. }))
            .map(|(i, _)| i)
    }

    pub(crate) fn port_value(&self, port: usize) -> f32 {
        self.value(port)
    }

    /// The plugin's interface moved a control input (a plain value, in the port's range).
    pub(crate) fn set_from_ui(&self, port: u32, v: f32) {
        if let Some(PortKind::ControlIn { spec, .. }) = self.model.ports.get(port as usize).map(|p| &p.kind) {
            if v.is_finite() {
                self.values[port as usize].store(spec.snap(v).to_bits(), Ordering::Relaxed);
                self.dirty.store(true, Ordering::Release);
            }
        }
    }

    /// The embeddable interface (looked up on first use).
    pub(crate) fn ui(&self) -> Result<crate::ui::UiSpec, String> {
        self.ui.get_or_init(|| crate::ui::find(&self.model.uri)).clone()
    }

    pub(crate) fn set_ui_open(&self, open: bool) {
        self.ui_open.store(open, Ordering::Release);
    }

    fn port_by_symbol(&self, symbol: &str) -> Option<usize> {
        self.model.ports.iter().position(|p| p.symbol == symbol)
    }

    /// Set a control input to a normalized value. Audio-thread safe (no allocation).
    fn set_normalized(&self, port: u32, n: f64) -> bool {
        match self.model.ports.get(port as usize).map(|p| &p.kind) {
            Some(PortKind::ControlIn { spec, .. }) => {
                self.values[port as usize].store(spec.from_normalized(n).to_bits(), Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    fn value(&self, port: usize) -> f32 {
        f32::from_bits(self.values[port].load(Ordering::Relaxed))
    }
}

/// Storage behind one port.
enum PortBuf {
    None,
    Floats(Vec<f32>),
    Atom(Vec<u64>),
}

impl PortBuf {
    fn ptr(&mut self) -> *mut c_void {
        match self {
            Self::None => std::ptr::null_mut(),
            Self::Floats(v) => v.as_mut_ptr().cast(),
            Self::Atom(v) => v.as_mut_ptr().cast(),
        }
    }
    fn floats(&mut self) -> &mut [f32] {
        match self {
            Self::Floats(v) => v,
            _ => &mut [],
        }
    }
}

/// One instance and the buffers its ports are connected to.
struct Unit {
    inst: Inst,
    bufs: Vec<PortBuf>,
}

/// Port indices the effect touches per block, worked out once.
#[derive(Default)]
struct Wiring {
    main_in: Vec<usize>,
    main_out: Vec<usize>,
    /// Inputs fed silence (side chains, CV, main inputs beyond the two used).
    silent_in: Vec<usize>,
    controls: Vec<(usize, Designation)>,
    atom_in: Vec<usize>,
    atom_out: Vec<usize>,
    latency_out: Option<usize>,
    /// Control outputs, published for the plugin's interface (meters).
    control_out: Vec<usize>,
}

impl Wiring {
    fn of(m: &Model, layout: Layout) -> Self {
        let mut w = Self::default();
        let used_in = if matches!(layout, Layout::Stereo | Layout::StereoToMono) { 2 } else { 1 };
        for (i, p) in m.ports.iter().enumerate() {
            match &p.kind {
                PortKind::AudioIn { side_chain: false } if w.main_in.len() < used_in => w.main_in.push(i),
                PortKind::AudioIn { .. } | PortKind::CvIn => w.silent_in.push(i),
                PortKind::AudioOut => w.main_out.push(i),
                PortKind::ControlIn { designation, .. } => w.controls.push((i, *designation)),
                PortKind::ControlOut { latency } => {
                    if *latency && w.latency_out.is_none() {
                        w.latency_out = Some(i);
                    }
                    w.control_out.push(i);
                }
                PortKind::AtomIn => w.atom_in.push(i),
                PortKind::AtomOut => w.atom_out.push(i),
                _ => {}
            }
        }
        w
    }
}

/// A hosted LV2 effect on the audio thread. Get one from [`instantiate`].
///
/// [`process`](Processor::process) copies the block into the input buffers, refreshes the
/// control ports from the parameter atomics, runs the instance(s) in chunks of at most
/// the prepared block size, delivers worker answers and copies the outputs back. No
/// allocation, no lock, no system call. [`prepare`](Processor::prepare) with another
/// sample rate or a larger block re-instantiates the plugin (off the audio thread) and
/// carries its state over.
pub struct Lv2Effect {
    shared: Arc<Shared>,
    units: Vec<Unit>,
    wiring: Wiring,
    layout: Layout,
    sample_rate: f32,
    max_block: usize,
    sequence: u32,
    chunk: u32,
}

/// The non-real-time side of a hosted LV2 effect: parameters, state, latency.
/// `Send + Sync`, cheap to clone.
#[derive(Clone)]
pub struct Lv2Handle {
    shared: Arc<Shared>,
}

unsafe extern "C" fn set_port_value(symbol: *const c_char, user: *mut c_void, value: *const c_void, size: u32, ty: u32) {
    if symbol.is_null() || user.is_null() || value.is_null() {
        return;
    }
    // SAFETY: `user` is the `Shared` passed to `lilv_state_restore`; lilv passes a live
    // symbol and a value of `size` bytes of type `ty`.
    let shared = unsafe { &*user.cast::<Shared>() };
    let v = unsafe {
        if ty == urid::map_str(sys::ATOM_FLOAT) && size == 4 {
            *value.cast::<f32>()
        } else if ty == urid::map_str(sys::ATOM_INT) && size == 4 {
            *value.cast::<i32>() as f32
        } else if size == 8 {
            *value.cast::<f64>() as f32
        } else {
            return;
        }
    };
    let symbol = unsafe { CStr::from_ptr(symbol) }.to_string_lossy();
    if let Some(i) = shared.port_by_symbol(&symbol) {
        if let PortKind::ControlIn { spec, .. } = &shared.model.ports[i].kind {
            shared.values[i].store(spec.snap(v).to_bits(), Ordering::Relaxed);
        }
    }
}

/// Port values handed to `lilv_state_new_from_instance`.
struct PortSnapshot {
    values: Vec<(CString, f32)>,
}

unsafe extern "C" fn get_port_value(symbol: *const c_char, user: *mut c_void, size: *mut u32, ty: *mut u32) -> *const c_void {
    if symbol.is_null() || user.is_null() {
        return std::ptr::null();
    }
    // SAFETY: `user` is the snapshot passed to lilv, alive for the call.
    let snap = unsafe { &*user.cast::<PortSnapshot>() };
    let symbol = unsafe { CStr::from_ptr(symbol) };
    match snap.values.iter().find(|(s, _)| s.as_c_str() == symbol) {
        Some((_, v)) => {
            // SAFETY: lilv passes live out-parameters.
            unsafe {
                *size = 4;
                *ty = urid::map_str(sys::ATOM_FLOAT);
            }
            std::ptr::from_ref(v).cast()
        }
        None => {
            // SAFETY: as above.
            unsafe {
                *size = 0;
                *ty = 0;
            }
            std::ptr::null()
        }
    }
}

/// Create the instance(s) for `model` and restore `state` (or the plugin's default state)
/// into them and into `shared`'s values.
#[allow(clippy::too_many_arguments)]
fn create_units(
    l: &Lilv,
    world: &Arc<World>,
    plugin: *const Plugin,
    shared: &Shared,
    layout: Layout,
    state: Option<&[u8]>,
    sample_rate: f32,
    max_block: usize,
) -> Result<Vec<Unit>, String> {
    let _g = lilv::lock();
    let m = &shared.model;
    let mut units = Vec::with_capacity(layout.instances());
    for _ in 0..layout.instances() {
        // SAFETY: `plugin` belongs to `world`; lock held.
        let inst = unsafe { Inst::new(l, Arc::clone(world), plugin, &m.name, sample_rate, max_block, m.has_worker)? };
        let bufs = m
            .ports
            .iter()
            .map(|p| match p.kind {
                PortKind::AudioIn { .. } | PortKind::AudioOut | PortKind::CvIn | PortKind::CvOut => PortBuf::Floats(vec![0.0; max_block]),
                PortKind::ControlIn { .. } | PortKind::ControlOut { .. } => PortBuf::Floats(vec![0.0; 1]),
                PortKind::AtomIn | PortKind::AtomOut => PortBuf::Atom(vec![0; ATOM_BYTES / 8]),
                PortKind::Unconnected => PortBuf::None,
            })
            .collect();
        units.push(Unit { inst, bufs });
    }
    // SAFETY (state): lilv calls under the lock, on instances not yet running; the
    // feature arrays and `shared` outlive the calls.
    unsafe {
        let st = match state {
            Some(bytes) => {
                let text = CString::new(bytes.to_vec()).map_err(|_| format!("{}: the saved state is not text", m.name))?;
                let st = (l.lilv_state_new_from_string)(world.ptr, urid::map_feature(), text.as_ptr());
                if st.is_null() {
                    return Err(format!("{}: the saved state could not be read", m.name));
                }
                st
            }
            None if m.load_default_state => {
                let uri = CString::new(m.uri.clone()).unwrap_or_default();
                let node = (l.lilv_new_uri)(world.ptr, uri.as_ptr());
                let st = (l.lilv_state_new_from_world)(world.ptr, urid::map_feature(), node);
                (l.lilv_node_free)(node);
                st
            }
            None => std::ptr::null_mut(),
        };
        if !st.is_null() {
            for u in &units {
                (l.lilv_state_restore)(
                    st,
                    u.inst.raw(),
                    Some(set_port_value),
                    std::ptr::from_ref(shared).cast_mut().cast(),
                    0,
                    u.inst.features(),
                );
            }
            (l.lilv_state_free)(st);
        }
    }
    Ok(units)
}

impl Lv2Effect {
    /// Connect every port, activate, and run a short block of silence so a latency port
    /// has reported. Off the audio thread.
    fn start(&mut self) {
        let shared = Arc::clone(&self.shared);
        for u in &mut self.units {
            for (i, b) in u.bufs.iter_mut().enumerate() {
                // SAFETY: each buffer lives (unmoved: heap) as long as the unit.
                unsafe { u.inst.connect(i as u32, b.ptr()) };
            }
            u.inst.activate();
        }
        *shared.target.lock() = self.units.first().map(|u| StateTarget {
            raw: u.inst.raw(),
            features: u.inst.features(),
            plugin: u.inst.plugin,
            world: Arc::clone(&u.inst.world),
        });
        let n = self.max_block.min(64);
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        self.run_chunk(&mut l, &mut r, &ProcessCtx { sample_rate: self.sample_rate, bpm: 120.0, beat: 0.0, playing: false });
    }

    fn run_chunk(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        let n = l.len();
        let shared = &*self.shared;
        let w = &self.wiring;
        let (seq, chunk) = (self.sequence, self.chunk);
        for (k, u) in self.units.iter_mut().enumerate() {
            for &(i, des) in &w.controls {
                let v = match des {
                    Designation::None => shared.value(i),
                    Designation::Enabled => 1.0,
                    Designation::FreeWheeling => 0.0,
                    Designation::Bpm => ctx.bpm as f32,
                };
                u.bufs[i].floats()[0] = v;
            }
            for &i in &w.silent_in {
                u.bufs[i].floats()[..n].fill(0.0);
            }
            for &i in &w.atom_in {
                if let PortBuf::Atom(v) = &mut u.bufs[i] {
                    // an empty sequence: header {size: 8 (the body's unit + pad), type}
                    let atom = v.as_mut_ptr().cast::<LV2_Atom>();
                    // SAFETY: the buffer is 8-byte aligned and larger than the header + body.
                    unsafe {
                        *atom = LV2_Atom { size: 8, type_: seq };
                        *v.as_mut_ptr().add(1) = 0;
                    }
                }
            }
            for &i in &w.atom_out {
                if let PortBuf::Atom(v) = &mut u.bufs[i] {
                    // SAFETY: as above; the capacity the plugin may write is the rest.
                    unsafe { *v.as_mut_ptr().cast::<LV2_Atom>() = LV2_Atom { size: (ATOM_BYTES - 8) as u32, type_: chunk } };
                }
            }
            let ins = &w.main_in;
            match self.layout {
                Layout::Stereo | Layout::StereoToMono => {
                    u.bufs[ins[0]].floats()[..n].copy_from_slice(l);
                    u.bufs[ins[1]].floats()[..n].copy_from_slice(r);
                }
                Layout::DualMono => u.bufs[ins[0]].floats()[..n].copy_from_slice(if k == 0 { l } else { r }),
                Layout::MonoToStereo => {
                    for ((d, a), b) in u.bufs[ins[0]].floats()[..n].iter_mut().zip(l.iter()).zip(r.iter()) {
                        *d = 0.5 * (a + b);
                    }
                }
            }
            // SAFETY: every port is connected to a buffer of max_block >= n samples.
            unsafe { u.inst.run(n as u32) };
        }
        let outs = &w.main_out;
        match self.layout {
            Layout::Stereo | Layout::MonoToStereo => {
                l.copy_from_slice(&self.units[0].bufs[outs[0]].floats()[..n]);
                r.copy_from_slice(&self.units[0].bufs[outs[1]].floats()[..n]);
            }
            Layout::DualMono => {
                l.copy_from_slice(&self.units[0].bufs[outs[0]].floats()[..n]);
                r.copy_from_slice(&self.units[1].bufs[outs[0]].floats()[..n]);
            }
            Layout::StereoToMono => {
                l.copy_from_slice(&self.units[0].bufs[outs[0]].floats()[..n]);
                r.copy_from_slice(l);
            }
        }
        for &i in &w.control_out {
            shared.values[i].store(self.units[0].bufs[i].floats()[0].to_bits(), Ordering::Relaxed);
        }
        if let Some(i) = w.latency_out {
            let lat = self.units[0].bufs[i].floats()[0];
            let lat = if lat.is_finite() && lat > 0.0 { lat.round().min(u32::MAX as f32) as u32 } else { 0 };
            shared.latency.store(lat, Ordering::Relaxed);
        }
    }

    /// Withdraw the state target (the handle can no longer reach the instances).
    fn withdraw(&self) {
        *self.shared.target.lock() = None;
    }
}

impl Drop for Lv2Effect {
    fn drop(&mut self) {
        self.withdraw();
    }
}

impl Processor for Lv2Effect {
    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        if (sample_rate - self.sample_rate).abs() < 1e-3 && max_block <= self.max_block {
            return;
        }
        // LV2 fixes the rate and block bound at instantiation: make new instances and
        // carry the state over
        let state = Lv2Handle { shared: Arc::clone(&self.shared) }.get_state().ok();
        let Some(world) = self.units.first().map(|u| Arc::clone(&u.inst.world)) else { return };
        let plugin = self.units[0].inst.plugin;
        let created = lilv::lilv()
            .and_then(|l| create_units(l, &world, plugin, &self.shared, self.layout, state.as_deref(), sample_rate, max_block.max(1)));
        match created {
            Ok(units) => {
                self.withdraw();
                self.units = units;
                self.sample_rate = sample_rate;
                self.max_block = max_block.max(1);
                self.start();
            }
            Err(e) => log::error!("LV2 plugin '{}' could not be re-created at {sample_rate} Hz / {max_block}: {e}", self.shared.info.name),
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        let frames = l.len().min(r.len());
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(self.max_block);
            self.run_chunk(&mut l[done..done + n], &mut r[done..done + n], ctx);
            done += n;
        }
    }

    fn set_plugin_param(&mut self, param_id: u32, normalized: f32) {
        if normalized.is_finite() {
            self.shared.set_normalized(param_id, f64::from(normalized));
        }
    }

    fn latency_samples(&self) -> usize {
        self.shared.latency.load(Ordering::Relaxed) as usize
    }
}

impl std::fmt::Debug for Lv2Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lv2Effect")
            .field("plugin", &self.shared.info.cid)
            .field("layout", &self.layout)
            .field("sample_rate", &self.sample_rate)
            .field("max_block", &self.max_block)
            .finish()
    }
}

impl Lv2Handle {
    /// What was instantiated.
    pub fn info(&self) -> &PluginInfo {
        &self.shared.info
    }

    /// The plugin's parameters (its visible control inputs) with current values.
    pub fn params(&self) -> Vec<ParamInfo> {
        self.shared
            .model
            .ports
            .iter()
            .enumerate()
            .filter_map(|(i, p)| match &p.kind {
                PortKind::ControlIn { spec, hidden: false, .. } => {
                    Some(ParamInfo::new(p.index, &p.symbol, p.name.clone(), spec, self.shared.value(i)))
                }
                _ => None,
            })
            .collect()
    }

    /// Set parameter `id` (a control port index) to a normalized 0..1 value. Never
    /// blocks: the audio thread takes it at its next block.
    pub fn set_param(&self, id: u32, value: f64) -> Result<(), String> {
        if !(value.is_finite() && (0.0..=1.0).contains(&value)) {
            return Err(format!("parameter values are normalised 0..1, not {value}"));
        }
        if !self.shared.set_normalized(id, value) {
            return Err(format!("{} has no parameter {id}", self.shared.info.name));
        }
        self.shared.dirty.store(true, Ordering::Release);
        Ok(())
    }

    /// The plugin's state as an opaque blob (Turtle text from lilv: the plugin's own
    /// state, if it has the state extension, plus every control port value).
    pub fn get_state(&self) -> Result<Vec<u8>, String> {
        let l = lilv::lilv()?;
        let _g = lilv::lock();
        let target = self.shared.target.lock();
        let t = target.as_ref().ok_or_else(|| format!("{} is not running", self.shared.info.name))?;
        let snap = PortSnapshot {
            values: self
                .shared
                .model
                .ports
                .iter()
                .enumerate()
                .filter(|(_, p)| matches!(p.kind, PortKind::ControlIn { .. }))
                .filter_map(|(i, p)| Some((CString::new(p.symbol.clone()).ok()?, self.shared.value(i))))
                .collect(),
        };
        // SAFETY: lilv calls under the lock; the target is valid while its mutex is held
        // (the effect withdraws it before freeing the instance); `snap` outlives the calls.
        unsafe {
            let st = (l.lilv_state_new_from_instance)(
                t.plugin,
                t.raw,
                urid::map_feature(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                Some(get_port_value),
                std::ptr::from_ref(&snap).cast_mut().cast(),
                STATE_POD_PORTABLE,
                t.features,
            );
            if st.is_null() {
                return Err(format!("{}: its state could not be saved", self.shared.info.name));
            }
            let text = (l.lilv_state_to_string)(
                t.world.ptr,
                urid::map_feature(),
                urid::unmap_feature(),
                st,
                STATE_URI.as_ptr().cast(),
                std::ptr::null(),
            );
            (l.lilv_state_free)(st);
            if text.is_null() {
                return Err(format!("{}: its state could not be written", self.shared.info.name));
            }
            let bytes = CStr::from_ptr(text).to_bytes().to_vec();
            (l.lilv_free)(text.cast());
            Ok(bytes)
        }
    }

    /// Whether a parameter was set through this handle since the last call. Clears it.
    pub fn take_state_changed(&self) -> bool {
        self.shared.dirty.swap(false, Ordering::AcqRel)
    }

    /// The latency (samples) the plugin's `lv2:reportsLatency` port says now.
    pub fn latency(&self) -> usize {
        self.shared.latency.load(Ordering::Relaxed) as usize
    }

    /// Whether the plugin has an interface of its own WAVE can embed (an X11 UI whose
    /// required features WAVE provides). Looks it up once.
    pub fn has_editor(&self) -> bool {
        self.editor_unavailable().is_none()
    }

    /// Why the plugin's own interface cannot be shown, when it cannot.
    pub fn editor_unavailable(&self) -> Option<String> {
        self.shared.ui().err()
    }

    /// Whether its interface window is open (turns `false` when the user closes it).
    pub fn is_editor_open(&self) -> bool {
        self.shared.ui_open.load(Ordering::Acquire)
    }

    /// Open the plugin's interface in a WAVE plugin window under a header showing `label`,
    /// or bring it forward. **UI thread only** (the window service's).
    pub fn open_editor(&self, windows: &wave_plugin_windows::Service, label: &wave_plugin_windows::Label) -> Result<(), String> {
        crate::ui::open(&self.shared, windows, label).map_err(|e| format!("{}: {e}", self.shared.info.name))
    }

    /// Close the interface (no-op when closed). **UI thread only.**
    pub fn close_editor(&self) -> Result<(), String> {
        crate::ui::close(&self.shared);
        Ok(())
    }

    /// Pin the interface's window on top of the app's (no-op when it is closed). **UI thread only.**
    pub fn set_editor_pinned(&self, pinned: bool) {
        crate::ui::set_pinned(&self.shared, pinned);
    }
}

impl std::fmt::Debug for Lv2Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lv2Handle").field("plugin", &self.shared.info).finish()
    }
}

/// [`instantiate`] returning the concrete type.
pub fn instantiate_effect(uri: &str, state: Option<&[u8]>, sample_rate: f32, max_block: usize) -> Result<(Lv2Effect, Lv2Handle), String> {
    if !(sample_rate.is_finite() && sample_rate > 0.0) || max_block == 0 {
        return Err(format!("invalid audio configuration: {sample_rate} Hz, blocks of {max_block}"));
    }
    let l = lilv::lilv()?;
    let world = world::world(false)?;
    let (model, plugin) = {
        let _g = lilv::lock();
        // SAFETY: lock held; the plugin belongs to `world`.
        let plugin = unsafe { world.plugin(l, uri) }.ok_or_else(|| format!("the LV2 plugin {uri} is not installed"))?;
        (unsafe { world::model(l, &world, plugin, sample_rate) }, plugin)
    };
    if let Some(why) = &model.unsupported {
        return Err(format!("{} cannot be used: {why}", model.name));
    }
    let layout = model.layout.clone()?;
    if !model.realtime {
        // most LV2 plugins never declare it; the audio thread still runs them, as other hosts do
        log::info!("LV2 plugin '{}' does not declare lv2:hardRTCapable: it may not be real-time safe", model.name);
    }
    let wiring = Wiring::of(&model, layout);
    let shared = Arc::new(Shared::new(model));
    let units = create_units(l, &world, plugin, &shared, layout, state, sample_rate, max_block)?;
    let mut effect = Lv2Effect {
        shared: Arc::clone(&shared),
        units,
        wiring,
        layout,
        sample_rate,
        max_block,
        sequence: urid::map_str(sys::ATOM_SEQUENCE),
        chunk: urid::map_str(sys::ATOM_CHUNK),
    };
    effect.start();
    // restoring a state is not an edit
    shared.dirty.store(false, Ordering::Release);
    Ok((effect, Lv2Handle { shared }))
}

/// Instantiate the LV2 effect `uri`, restore `state` (a blob from
/// [`Lv2Handle::get_state`]; `None` for the plugin's defaults) and activate it for
/// `sample_rate` and blocks of at most `max_block`. Off the audio thread.
pub fn instantiate(uri: &str, state: Option<&[u8]>, sample_rate: f32, max_block: usize) -> Result<(Box<dyn Processor>, Lv2Handle), String> {
    let (e, h) = instantiate_effect(uri, state, sample_rate, max_block)?;
    Ok((Box::new(e), h))
}

/// Every installed LV2 effect (plugins with audio in and out; instruments and analysers
/// are left out), with the reason for those that cannot run here. `rescan` re-reads the
/// LV2 directories (new or removed bundles). Reads Turtle metadata only: no plugin code
/// runs. Bundles listed in `lv2-blocklist.txt` (WAVE's data directory) are skipped.
pub fn scan(rescan: bool) -> Result<Vec<PluginInfo>, String> {
    let l = lilv::lilv()?;
    let world = world::world(rescan)?;
    let blocked = wave_plugin_scan::BlockList::load("lv2-blocklist.txt");
    let _g = lilv::lock();
    let mut out = vec![];
    // SAFETY: lilv calls under the lock on the world's own collection.
    unsafe {
        let all = (l.lilv_world_get_all_plugins)(world.ptr);
        let mut it = (l.lilv_plugins_begin)(all);
        while !(l.lilv_plugins_is_end)(all, it) {
            let p = (l.lilv_plugins_get)(all, it);
            it = (l.lilv_plugins_next)(all, it);
            let m = world::model(l, &world, p, 48_000.0);
            if m.instrument || m.layout.is_err() || blocked.contains(std::path::Path::new(&m.bundle)) {
                continue;
            }
            out.push(PluginInfo::of(&m));
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.cid.cmp(&b.cid)));
    Ok(out)
}
