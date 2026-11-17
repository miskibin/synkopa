//! Synkopa Demo Synth: a small polyphonic VST3 instrument that ships with Synkopa.
//!
//! It is what a new user can load from the Channel Rack's plugin list to hear Synkopa host a
//! real VST3 instrument, and the plugin the host's end-to-end tests run against
//! (`crates/vst3-host/tests/instrument.rs`). The sound is in [`dsp`]; this file is the VST3
//! glue, after the `vst3` crate's `gain` example: a processor (`IComponent` +
//! `IAudioProcessor`) and a separate edit controller (`IEditController` + `IMidiMapping`),
//! classified `Instrument|Synth` through `IPluginFactory2`.
//!
//! Threading follows VST3: `process` runs on one audio thread; state, activation and setup
//! calls never run concurrently with it. The processor's voices live in an `UnsafeCell`
//! touched only by those calls; parameter values are atomics, so a state restore from the UI
//! thread is safe at any time. `process` does not allocate.
//!
//! No editor window: hosts show the generic parameter list.

#![allow(non_snake_case)] // the VST3 interface methods
#![allow(clippy::unnecessary_cast)] // enum constants are c_int on Windows, c_uint elsewhere

pub mod dsp;

use std::cell::{Cell, UnsafeCell};
use std::ffi::{c_char, c_void};
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use dsp::{id, Synth, PARAMS};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{uid, Class, ComRef, ComWrapper};

pub const PLUGIN_NAME: &str = "Synkopa Demo Synth";
pub const VENDOR: &str = "Synkopa";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const SUBCATEGORIES: &str = "Instrument|Synth";

fn copy_cstring(src: &str, dst: &mut [c_char]) {
    let n = src.len().min(dst.len().saturating_sub(1));
    for (d, s) in dst.iter_mut().zip(src.bytes().take(n)) {
        *d = s as c_char;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let mut n = 0;
    let room = dst.len().saturating_sub(1);
    for (d, s) in dst.iter_mut().zip(src.encode_utf16()).take(room) {
        *d = s as TChar;
        n += 1;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

/// Reads a whole stream (a state blob is a few hundred bytes).
unsafe fn read_stream(stream: *mut IBStream) -> Option<Vec<u8>> {
    let stream = ComRef::from_raw(stream)?;
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let mut got = 0;
        if stream.read(chunk.as_mut_ptr().cast(), chunk.len() as i32, &mut got) != kResultOk || got <= 0 {
            break;
        }
        out.extend_from_slice(&chunk[..got as usize]);
        if out.len() > 1 << 16 {
            return None;
        }
    }
    Some(out)
}

unsafe fn write_stream(stream: *mut IBStream, bytes: &[u8]) -> tresult {
    let Some(stream) = ComRef::from_raw(stream) else { return kInvalidArgument };
    let mut written = 0;
    if stream.write(bytes.as_ptr() as *mut c_void, bytes.len() as i32, &mut written) == kResultOk && written as usize == bytes.len() {
        kResultOk
    } else {
        kResultFalse
    }
}

// ---------------------------------------------------------------------------
// The processor
// ---------------------------------------------------------------------------

struct Processor {
    /// Normalized parameter values (f64 bits), by id: what `process` plays.
    params: [AtomicU64; PARAMS.len()],
    /// A state restore changed `params`: `process` reloads them.
    restored: AtomicBool,
    synth: UnsafeCell<Synth>,
}

// SAFETY: `synth` is touched only by `process`, `setupProcessing` and `setActive`, which a
// VST3 host never calls concurrently; everything else is atomic.
unsafe impl Sync for Processor {}

impl Class for Processor {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements);
}

impl Processor {
    const CID: TUID = uid(0x57415645, 0x44454D4F, 0x53594E54, 0x48000001);

    fn new() -> Self {
        Self {
            params: std::array::from_fn(|i| AtomicU64::new(PARAMS[i].def.to_bits())),
            restored: AtomicBool::new(true),
            synth: UnsafeCell::new(Synth::new(48_000.0)),
        }
    }

    fn param(&self, i: usize) -> f64 {
        f64::from_bits(self.params[i].load(Ordering::Acquire))
    }
}

impl IPluginBaseTrait for Processor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Processor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        if class_id.is_null() {
            return kInvalidArgument;
        }
        *class_id = Controller::CID;
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, dir: BusDirection) -> i32 {
        match (media_type as MediaTypes, dir as BusDirections) {
            (MediaTypes_::kAudio, BusDirections_::kOutput) | (MediaTypes_::kEvent, BusDirections_::kInput) => 1,
            _ => 0,
        }
    }

    unsafe fn getBusInfo(&self, media_type: MediaType, dir: BusDirection, index: i32, bus: *mut BusInfo) -> tresult {
        let Some(bus) = bus.as_mut() else { return kInvalidArgument };
        if index != 0 {
            return kInvalidArgument;
        }
        match (media_type as MediaTypes, dir as BusDirections) {
            (MediaTypes_::kAudio, BusDirections_::kOutput) => {
                bus.channelCount = 2;
                copy_wstring("Output", &mut bus.name);
            }
            (MediaTypes_::kEvent, BusDirections_::kInput) => {
                bus.channelCount = 1;
                copy_wstring("MIDI In", &mut bus.name);
            }
            _ => return kInvalidArgument,
        }
        bus.mediaType = media_type;
        bus.direction = dir;
        bus.busType = BusTypes_::kMain as BusType;
        bus.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
        kResultOk
    }

    unsafe fn getRoutingInfo(&self, _in_info: *mut RoutingInfo, _out_info: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(&self, _media_type: MediaType, _dir: BusDirection, _index: i32, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, state: TBool) -> tresult {
        if state == 0 {
            (*self.synth.get()).reset();
        }
        kResultOk
    }

    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        let Some(bytes) = read_stream(state) else { return kResultFalse };
        let mut values: [f64; PARAMS.len()] = std::array::from_fn(|i| self.param(i));
        if !dsp::load_params(&bytes, &mut values) {
            return kResultFalse;
        }
        for (slot, v) in self.params.iter().zip(values) {
            slot.store(v.to_bits(), Ordering::Release);
        }
        self.restored.store(true, Ordering::Release);
        kResultOk
    }

    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        let mut values: Vec<f64> = (0..PARAMS.len()).map(|i| self.param(i)).collect();
        // where the pedal and the pitch wheel happen to be is not part of the sound: a project
        // saved (or an export started) with the pedal down must not load with every note held
        for held in [id::PEDAL, id::BEND] {
            values[held as usize] = PARAMS[held as usize].def;
        }
        write_stream(state, &dsp::save_params(&values))
    }
}

impl IAudioProcessorTrait for Processor {
    unsafe fn setBusArrangements(
        &self,
        _inputs: *mut SpeakerArrangement,
        num_ins: i32,
        outputs: *mut SpeakerArrangement,
        num_outs: i32,
    ) -> tresult {
        if num_ins == 0 && num_outs == 1 && !outputs.is_null() && *outputs == SpeakerArr::kStereo {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn getBusArrangement(&self, dir: BusDirection, index: i32, arr: *mut SpeakerArrangement) -> tresult {
        if dir as BusDirections == BusDirections_::kOutput && index == 0 && !arr.is_null() {
            *arr = SpeakerArr::kStereo;
            kResultOk
        } else {
            kInvalidArgument
        }
    }

    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size as SymbolicSampleSizes == SymbolicSampleSizes_::kSample32 {
            kResultOk
        } else {
            kNotImplemented
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        let Some(setup) = setup.as_ref() else { return kInvalidArgument };
        (*self.synth.get()).set_sample_rate(setup.sampleRate);
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let Some(data) = data.as_ref() else { return kInvalidArgument };
        let synth = &mut *self.synth.get();
        if self.restored.swap(false, Ordering::AcqRel) {
            for i in 0..PARAMS.len() {
                synth.set_param(i as u32, self.param(i));
            }
        }
        let frames = data.numSamples.max(0) as usize;
        let out = (data.numOutputs > 0 && !data.outputs.is_null()).then(|| &*data.outputs);
        // the only arrangement accepted is stereo
        let Some(out) = out.filter(|bus| bus.numChannels == 2 && !bus.__field0.channelBuffers32.is_null()) else {
            return kResultOk;
        };
        let channels = slice::from_raw_parts(out.__field0.channelBuffers32, 2);
        if channels[0].is_null() || channels[1].is_null() {
            return kResultOk;
        }
        let l = slice::from_raw_parts_mut(channels[0], frames);
        let r = slice::from_raw_parts_mut(channels[1], frames);
        l.fill(0.0);
        r.fill(0.0);

        // parameter changes: the last point of each, at the block start (sample-accurate
        // points are applied at their offset below for the pedal and bend)
        let changes = ComRef::from_raw(data.inputParameterChanges);
        let mut timed: [(i32, u32, f64); 32] = [(0, 0, 0.0); 32];
        let mut ntimed = 0;
        if let Some(changes) = changes {
            for q in 0..changes.getParameterCount() {
                let Some(queue) = ComRef::from_raw(changes.getParameterData(q)) else { continue };
                let pid = queue.getParameterId();
                if pid as usize >= PARAMS.len() {
                    continue;
                }
                for p in 0..queue.getPointCount() {
                    let (mut offset, mut value) = (0, 0.0);
                    if queue.getPoint(p, &mut offset, &mut value) != kResultOk {
                        continue;
                    }
                    self.params[pid as usize].store(value.clamp(0.0, 1.0).to_bits(), Ordering::Release);
                    if ntimed < timed.len() {
                        timed[ntimed] = (offset.max(0), pid, value);
                        ntimed += 1;
                    } else {
                        synth.set_param(pid, value);
                    }
                }
            }
        }
        timed[..ntimed].sort_unstable_by_key(|t| t.0);

        // notes and timed parameter points in order, rendering the stretches between them
        let events = ComRef::from_raw(data.inputEvents);
        let count = events.as_ref().map_or(0, |list| list.getEventCount());
        let (mut at, mut e, mut t) = (0usize, 0, 0usize);
        loop {
            let mut note: Option<(usize, Event)> = None;
            while note.is_none() && e < count {
                let mut ev: Event = std::mem::zeroed();
                match events.as_ref() {
                    Some(list) if list.getEvent(e, &mut ev) == kResultOk => note = Some((ev.sampleOffset.max(0) as usize, ev)),
                    _ => e += 1,
                }
            }
            let param = (t < ntimed).then(|| timed[t].0 as usize);
            let take_param = match (note.as_ref(), param) {
                (None, None) => break,
                (Some((n, _)), Some(p)) => p <= *n,
                (None, Some(_)) => true,
                (Some(_), None) => false,
            };
            let until = if take_param { param.unwrap_or(frames) } else { note.as_ref().map_or(frames, |(n, _)| *n) }.min(frames);
            if until > at {
                render_into(synth, l, r, at, until);
                at = until;
            }
            if take_param {
                let (_, pid, value) = timed[t];
                synth.set_param(pid, value);
                t += 1;
                continue;
            }
            if let Some((_, ev)) = note {
                apply_note(synth, &ev);
            }
            e += 1;
        }
        if frames > at {
            render_into(synth, l, r, at, frames);
        }
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> u32 {
        kInfiniteTail
    }
}

impl IProcessContextRequirementsTrait for Processor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

/// Renders frames `from..to`.
fn render_into(synth: &mut Synth, l: &mut [f32], r: &mut [f32], from: usize, to: usize) {
    synth.render(&mut l[from..to], &mut r[from..to]);
}

/// A note-on (velocity 0 = note-off) or note-off event.
unsafe fn apply_note(synth: &mut Synth, ev: &Event) {
    match ev.r#type as u32 {
        x if x == Event_::EventTypes_::kNoteOnEvent as u32 => {
            let n = ev.__field0.noteOn;
            let key = n.pitch.clamp(0, 127) as u8;
            if n.velocity > 0.0 {
                synth.note_on(key, n.velocity, n.noteId);
            } else {
                synth.note_off(key, n.noteId);
            }
        }
        x if x == Event_::EventTypes_::kNoteOffEvent as u32 => {
            let n = ev.__field0.noteOff;
            synth.note_off(n.pitch.clamp(0, 127) as u8, n.noteId);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// The edit controller
// ---------------------------------------------------------------------------

struct Controller {
    values: [Cell<f64>; PARAMS.len()],
}

impl Class for Controller {
    type Interfaces = (IEditController, IMidiMapping);
}

impl Controller {
    const CID: TUID = uid(0x57415645, 0x44454D4F, 0x53594E54, 0x48000002);

    fn new() -> Self {
        Self { values: std::array::from_fn(|i| Cell::new(PARAMS[i].def)) }
    }
}

impl IPluginBaseTrait for Controller {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for Controller {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        let Some(bytes) = read_stream(state) else { return kResultFalse };
        let mut values: [f64; PARAMS.len()] = std::array::from_fn(|i| self.values[i].get());
        if !dsp::load_params(&bytes, &mut values) {
            return kResultFalse;
        }
        for (cell, v) in self.values.iter().zip(values) {
            cell.set(v);
        }
        kResultOk
    }

    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }

    unsafe fn getParameterCount(&self) -> i32 {
        PARAMS.len() as i32
    }

    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let (Some(def), Some(info)) = (usize::try_from(index).ok().and_then(|i| PARAMS.get(i)), info.as_mut()) else {
            return kInvalidArgument;
        };
        info.id = def.id;
        copy_wstring(def.name, &mut info.title);
        copy_wstring(def.name, &mut info.shortTitle);
        copy_wstring(def.units, &mut info.units);
        info.stepCount = def.steps;
        info.defaultNormalizedValue = def.def;
        info.unitId = 0;
        let mut flags = ParameterInfo_::ParameterFlags_::kCanAutomate as i32;
        if def.id == id::WAVE {
            flags |= ParameterInfo_::ParameterFlags_::kIsList as i32;
        }
        if !def.listed {
            flags = ParameterInfo_::ParameterFlags_::kIsHidden as i32;
        }
        info.flags = flags;
        kResultOk
    }

    unsafe fn getParamStringByValue(&self, id: u32, value: f64, string: *mut String128) -> tresult {
        let Some(string) = string.as_mut() else { return kInvalidArgument };
        if id as usize >= PARAMS.len() {
            return kInvalidArgument;
        }
        copy_wstring(&dsp::display(id, value), string);
        kResultOk
    }

    unsafe fn getParamValueByString(&self, _id: u32, _string: *mut TChar, _value: *mut f64) -> tresult {
        kNotImplemented
    }

    unsafe fn normalizedParamToPlain(&self, id: u32, value: f64) -> f64 {
        dsp::plain(id, value)
    }

    unsafe fn plainParamToNormalized(&self, _id: u32, plain: f64) -> f64 {
        plain.clamp(0.0, 1.0)
    }

    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        self.values.get(id as usize).map_or(0.0, Cell::get)
    }

    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        match self.values.get(id as usize) {
            Some(cell) if value.is_finite() => {
                cell.set(value.clamp(0.0, 1.0));
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }

    unsafe fn createView(&self, _name: *const c_char) -> *mut IPlugView {
        std::ptr::null_mut()
    }
}

impl IMidiMappingTrait for Controller {
    unsafe fn getMidiControllerAssignment(&self, bus: i32, _channel: i16, controller: CtrlNumber, id: *mut ParamID) -> tresult {
        if bus != 0 || id.is_null() {
            return kResultFalse;
        }
        let target = match controller as u32 {
            1 => id::MOD_WHEEL,
            64 => id::PEDAL,
            x if x == ControllerNumbers_::kPitchBend as u32 => id::BEND,
            _ => return kResultFalse,
        };
        *id = target;
        kResultTrue
    }
}

// ---------------------------------------------------------------------------
// The factory
// ---------------------------------------------------------------------------

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}

impl Factory {
    fn class(index: i32) -> Option<(TUID, &'static str)> {
        match index {
            0 => Some((Processor::CID, "Audio Module Class")),
            1 => Some((Controller::CID, "Component Controller Class")),
            _ => None,
        }
    }
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let Some(info) = info.as_mut() else { return kInvalidArgument };
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring("https://github.com/miskibin/synkopa", &mut info.url);
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }

    unsafe fn countClasses(&self) -> i32 {
        2
    }

    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let (Some((cid, category)), Some(info)) = (Self::class(index), info.as_mut()) else { return kInvalidArgument };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(PLUGIN_NAME, &mut info.name);
        kResultOk
    }

    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, obj: *mut *mut c_void) -> tresult {
        if cid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        let instance = match *(cid as *const TUID) {
            Processor::CID => ComWrapper::new(Processor::new()).to_com_ptr::<FUnknown>(),
            Controller::CID => ComWrapper::new(Controller::new()).to_com_ptr::<FUnknown>(),
            _ => None,
        };
        match instance {
            Some(instance) => {
                let ptr = instance.as_ptr();
                ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj)
            }
            None => kInvalidArgument,
        }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        let (Some((cid, category)), Some(info)) = (Self::class(index), info.as_mut()) else { return kInvalidArgument };
        info.cid = cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(category, &mut info.category);
        copy_cstring(PLUGIN_NAME, &mut info.name);
        info.classFlags = 0;
        copy_cstring(if index == 0 { SUBCATEGORIES } else { "" }, &mut info.subCategories);
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring(VERSION, &mut info.version);
        copy_cstring("VST 3.7", &mut info.sdkVersion);
        kResultOk
    }
}

// ---------------------------------------------------------------------------
// Module entry points
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
#[no_mangle]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(target_os = "windows")]
#[no_mangle]
extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "macos")]
#[no_mangle]
extern "system" fn BundleEntry(_bundle_ref: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[no_mangle]
extern "system" fn BundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[no_mangle]
extern "system" fn ModuleEntry(_library_handle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[no_mangle]
extern "system" fn ModuleExit() -> bool {
    true
}

/// The module's entry point: the class factory.
#[no_mangle]
pub extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>().map_or(std::ptr::null_mut(), |f| f.into_raw())
}
