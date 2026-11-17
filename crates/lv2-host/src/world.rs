//! The lilv world (every installed bundle's Turtle, read once) and the per-plugin model
//! this host builds from it: ports, ranges, what the plugin needs, and why it cannot run
//! here when it cannot.

use std::ffi::CString;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::layout::Layout;
use crate::lilv::{self, Lilv, Node, Plugin};
use crate::params::{unit_symbol, ParamSpec};
use crate::sys::{self, plain, short_name};

/// URI nodes looked up again and again.
pub(crate) struct Uris {
    pub input: *mut Node,
    pub output: *mut Node,
    pub audio: *mut Node,
    pub control: *mut Node,
    pub cv: *mut Node,
    pub atom: *mut Node,
    pub event: *mut Node,
    pub optional: *mut Node,
    pub integer: *mut Node,
    pub toggled: *mut Node,
    pub enumeration: *mut Node,
    pub sample_rate: *mut Node,
    pub reports_latency: *mut Node,
    pub designation: *mut Node,
    pub side_chain: *mut Node,
    pub logarithmic: *mut Node,
    pub not_on_gui: *mut Node,
    pub unit: *mut Node,
    pub required_option: *mut Node,
    pub worker_interface: *mut Node,
    pub hard_rt: *mut Node,
    pub load_default_state: *mut Node,
}

/// One loaded lilv world. Plugins found in it stay valid as long as it lives, so every
/// instance keeps an `Arc` of the world it came from (a rescan makes a new one).
pub(crate) struct World {
    pub ptr: *mut lilv::World,
    pub uris: Uris,
}

// SAFETY: every use of the raw pointers happens under `lilv::lock()`.
unsafe impl Send for World {}
unsafe impl Sync for World {}

impl Drop for World {
    fn drop(&mut self) {
        let Ok(l) = lilv::lilv() else { return };
        let _g = lilv::lock();
        // SAFETY: nodes and world were created by this world and are freed once.
        unsafe {
            let u = &self.uris;
            for n in [
                u.input,
                u.output,
                u.audio,
                u.control,
                u.cv,
                u.atom,
                u.event,
                u.optional,
                u.integer,
                u.toggled,
                u.enumeration,
                u.sample_rate,
                u.reports_latency,
                u.designation,
                u.side_chain,
                u.logarithmic,
                u.not_on_gui,
                u.unit,
                u.required_option,
                u.worker_interface,
                u.hard_rt,
                u.load_default_state,
            ] {
                (l.lilv_node_free)(n);
            }
            (l.lilv_world_free)(self.ptr);
        }
    }
}

impl World {
    /// Load every bundle on the LV2 path (`LV2_PATH`, or the standard directories).
    /// Reads Turtle only: no plugin code runs.
    fn load(l: &Lilv) -> Result<Self, String> {
        let _g = lilv::lock();
        // SAFETY: lilv calls under the lock; constants are NUL-terminated.
        unsafe {
            let ptr = (l.lilv_world_new)();
            if ptr.is_null() {
                return Err("lilv could not create a world".into());
            }
            (l.lilv_world_load_all)(ptr);
            let uri = |s: &str| (l.lilv_new_uri)(ptr, s.as_ptr().cast());
            let uris = Uris {
                input: uri(sys::LV2_CORE_INPUT_PORT),
                output: uri(sys::LV2_CORE_OUTPUT_PORT),
                audio: uri(sys::LV2_CORE_AUDIO_PORT),
                control: uri(sys::LV2_CORE_CONTROL_PORT),
                cv: uri(sys::LV2_CORE_CV_PORT),
                atom: uri(sys::ATOM_ATOM_PORT),
                event: uri(sys::EVENT_EVENT_PORT),
                optional: uri(sys::LV2_CORE_CONNECTION_OPTIONAL),
                integer: uri(sys::LV2_CORE_INTEGER),
                toggled: uri(sys::LV2_CORE_TOGGLED),
                enumeration: uri(sys::LV2_CORE_ENUMERATION),
                sample_rate: uri(sys::LV2_CORE_SAMPLE_RATE),
                reports_latency: uri(sys::LV2_CORE_REPORTS_LATENCY),
                designation: uri(sys::LV2_CORE_DESIGNATION),
                side_chain: uri(sys::LV2_CORE_IS_SIDE_CHAIN),
                logarithmic: uri(sys::PPROPS_LOGARITHMIC),
                not_on_gui: uri(sys::PPROPS_NOT_ON_GUI),
                unit: uri(sys::UNITS_UNIT),
                required_option: uri(sys::OPTIONS_REQUIRED_OPTION),
                worker_interface: uri(sys::WORKER_INTERFACE),
                hard_rt: uri(sys::LV2_CORE_HARD_RT_CAPABLE),
                load_default_state: uri(sys::STATE_LOAD_DEFAULT),
            };
            Ok(Self { ptr, uris })
        }
    }

    /// The plugin with `uri`. Lilv lock held by the caller.
    pub unsafe fn plugin(&self, l: &Lilv, uri: &str) -> Option<*const Plugin> {
        let c = CString::new(uri).ok()?;
        unsafe {
            let node = (l.lilv_new_uri)(self.ptr, c.as_ptr());
            if node.is_null() {
                return None;
            }
            let p = (l.lilv_plugins_get_by_uri)((l.lilv_world_get_all_plugins)(self.ptr), node);
            (l.lilv_node_free)(node);
            (!p.is_null()).then_some(p)
        }
    }
}

/// The current world (loaded on first use; `reload` replaces it).
pub(crate) fn world(reload: bool) -> Result<Arc<World>, String> {
    static WORLD: Mutex<Option<Arc<World>>> = Mutex::new(None);
    let l = lilv::lilv()?;
    let mut w = WORLD.lock();
    if reload || w.is_none() {
        *w = Some(Arc::new(World::load(l)?));
    }
    Ok(Arc::clone(w.as_ref().expect("just set")))
}

/// What a port is to this host.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PortKind {
    AudioIn {
        side_chain: bool,
    },
    AudioOut,
    /// A control input with its parameter (`hidden`: a designated port the host drives).
    ControlIn {
        spec: ParamSpec,
        designation: Designation,
        hidden: bool,
    },
    ControlOut {
        latency: bool,
    },
    CvIn,
    CvOut,
    AtomIn,
    AtomOut,
    /// A port type this host does not know, which the plugin says may stay unconnected.
    Unconnected,
}

/// Control inputs the host drives itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Designation {
    None,
    /// `lv2:enabled`: held at 1 (Synkopa bypasses outside the plugin).
    Enabled,
    /// `lv2:freeWheeling`: held at 0.
    FreeWheeling,
    /// `time:beatsPerMinute`: the project tempo.
    Bpm,
}

#[derive(Debug, Clone)]
pub(crate) struct PortInfo {
    pub index: u32,
    pub symbol: String,
    pub name: String,
    pub kind: PortKind,
}

/// Everything about one plugin this host needs, read from its Turtle.
#[derive(Debug, Clone)]
pub(crate) struct Model {
    pub uri: String,
    pub name: String,
    pub vendor: String,
    pub category: String,
    pub bundle: String,
    pub ports: Vec<PortInfo>,
    pub layout: Result<Layout, String>,
    /// Why it cannot run here (`None`: it can).
    pub unsupported: Option<String>,
    pub instrument: bool,
    pub realtime: bool,
    pub has_worker: bool,
    pub load_default_state: bool,
}

impl Model {
    pub fn audio_in(&self) -> impl Iterator<Item = &PortInfo> {
        self.ports.iter().filter(|p| matches!(p.kind, PortKind::AudioIn { side_chain: false }))
    }
    pub fn audio_out(&self) -> impl Iterator<Item = &PortInfo> {
        self.ports.iter().filter(|p| p.kind == PortKind::AudioOut)
    }
}

/// Read a plugin's model. `sample_rate` scales `lv2:sampleRate` ranges. Lilv lock held.
pub(crate) unsafe fn model(l: &Lilv, w: &World, p: *const Plugin, sample_rate: f32) -> Model {
    let u = &w.uris;
    unsafe {
        let uri = lilv::node_string(l, (l.lilv_plugin_get_uri)(p));
        let name_node = (l.lilv_plugin_get_name)(p);
        let name = lilv::node_string(l, name_node);
        (l.lilv_node_free)(name_node);
        let vendor_node = (l.lilv_plugin_get_author_name)(p);
        let vendor = lilv::node_string(l, vendor_node);
        (l.lilv_node_free)(vendor_node);
        let class = (l.lilv_plugin_get_class)(p);
        let category = if class.is_null() { String::new() } else { lilv::node_string(l, (l.lilv_plugin_class_get_label)(class)) };
        let class_uri = if class.is_null() { String::new() } else { lilv::node_string(l, (l.lilv_plugin_class_get_uri)(class)) };
        let bundle = lilv::node_path(l, (l.lilv_plugin_get_bundle_uri)(p));
        let required = lilv::take_strings(l, (l.lilv_plugin_get_required_features)(p));
        let options = lilv::take_strings(l, (l.lilv_plugin_get_value)(p, u.required_option));
        let has_worker = (l.lilv_plugin_has_extension_data)(p, u.worker_interface);
        let realtime = (l.lilv_plugin_has_feature)(p, u.hard_rt);
        let load_default_state = (l.lilv_plugin_has_feature)(p, u.load_default_state);
        let instrument = class_uri == plain(sys::LV2_CORE_INSTRUMENT_PLUGIN);

        let mut problems: Vec<String> = vec![];
        let missing: Vec<&str> =
            required.iter().filter(|f| !sys::SUPPORTED_FEATURES.iter().any(|s| plain(s) == f.as_str())).map(|f| short_name(f)).collect();
        if !missing.is_empty() {
            problems.push(format!("it needs host features Synkopa does not provide: {}", missing.join(", ")));
        }
        let missing: Vec<&str> =
            options.iter().filter(|o| !sys::SUPPORTED_OPTIONS.iter().any(|s| plain(s) == o.as_str())).map(|o| short_name(o)).collect();
        if !missing.is_empty() {
            problems.push(format!("it needs options Synkopa does not pass: {}", missing.join(", ")));
        }

        let n = (l.lilv_plugin_get_num_ports)(p);
        let mut ports = Vec::with_capacity(n as usize);
        for index in 0..n {
            let port = (l.lilv_plugin_get_port_by_index)(p, index);
            let symbol = lilv::node_string(l, (l.lilv_port_get_symbol)(p, port));
            let pname_node = (l.lilv_port_get_name)(p, port);
            let mut pname = lilv::node_string(l, pname_node);
            (l.lilv_node_free)(pname_node);
            if pname.is_empty() {
                pname = symbol.clone();
            }
            let is = |n: *mut Node| (l.lilv_port_is_a)(p, port, n);
            let has = |n: *mut Node| (l.lilv_port_has_property)(p, port, n);
            let input = is(u.input);
            let output = is(u.output);
            let optional = has(u.optional);
            let kind = if is(u.audio) && input {
                PortKind::AudioIn { side_chain: has(u.side_chain) }
            } else if is(u.audio) && output {
                PortKind::AudioOut
            } else if is(u.control) && input {
                let (mut def, mut min, mut max) = (std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                (l.lilv_port_get_range)(p, port, &mut def, &mut min, &mut max);
                let (d, lo, hi) = (lilv::node_f32(l, def), lilv::node_f32(l, min), lilv::node_f32(l, max));
                for n in [def, min, max] {
                    (l.lilv_node_free)(n);
                }
                let scale = if has(u.sample_rate) { sample_rate } else { 1.0 };
                let mut points = vec![];
                let sp = (l.lilv_port_get_scale_points)(p, port);
                if !sp.is_null() {
                    let mut it = (l.lilv_scale_points_begin)(sp);
                    while !(l.lilv_scale_points_is_end)(sp, it) {
                        let pt = (l.lilv_scale_points_get)(sp, it);
                        points.push((
                            lilv::node_f32(l, (l.lilv_scale_point_get_value)(pt)) * scale,
                            lilv::node_string(l, (l.lilv_scale_point_get_label)(pt)),
                        ));
                        it = (l.lilv_scale_points_next)(sp, it);
                    }
                    (l.lilv_scale_points_free)(sp);
                }
                let unit_node = (l.lilv_port_get)(p, port, u.unit);
                let unit = if unit_node.is_null() { "" } else { unit_symbol(&lilv::node_string(l, unit_node)) };
                (l.lilv_node_free)(unit_node);
                let des_node = (l.lilv_port_get)(p, port, u.designation);
                let des = lilv::node_string(l, des_node);
                (l.lilv_node_free)(des_node);
                let designation = if des == plain(sys::LV2_CORE_ENABLED) {
                    Designation::Enabled
                } else if des == plain(sys::LV2_CORE_FREE_WHEELING) {
                    Designation::FreeWheeling
                } else if des == plain(sys::TIME_BPM) {
                    Designation::Bpm
                } else {
                    Designation::None
                };
                let spec = ParamSpec::new(
                    lo * scale,
                    hi * scale,
                    d * scale,
                    has(u.integer),
                    has(u.toggled),
                    has(u.enumeration),
                    has(u.logarithmic),
                    points,
                    unit.to_owned(),
                );
                let hidden = designation != Designation::None || has(u.not_on_gui);
                PortKind::ControlIn { spec, designation, hidden }
            } else if is(u.control) && output {
                let des_node = (l.lilv_port_get)(p, port, u.designation);
                let des = lilv::node_string(l, des_node);
                (l.lilv_node_free)(des_node);
                PortKind::ControlOut { latency: has(u.reports_latency) || des == plain(sys::LV2_CORE_LATENCY) }
            } else if is(u.cv) && input {
                PortKind::CvIn
            } else if is(u.cv) && output {
                PortKind::CvOut
            } else if is(u.atom) && input {
                PortKind::AtomIn
            } else if is(u.atom) && output {
                PortKind::AtomOut
            } else if optional {
                PortKind::Unconnected
            } else {
                let what = if is(u.event) { "an old-style event port" } else { "a port of a type Synkopa does not know" };
                problems.push(format!("port `{symbol}` is {what}"));
                PortKind::Unconnected
            };
            ports.push(PortInfo { index, symbol, name: pname, kind });
        }
        let mut model = Model {
            uri,
            name,
            vendor,
            category,
            bundle,
            ports,
            layout: Err(String::new()),
            unsupported: None,
            instrument,
            realtime,
            has_worker,
            load_default_state,
        };
        model.layout = Layout::plan(model.audio_in().count(), model.audio_out().count());
        if let Err(e) = &model.layout {
            problems.insert(0, e.clone());
        }
        if !problems.is_empty() {
            model.unsupported = Some(problems.join("; "));
        }
        model
    }
}
