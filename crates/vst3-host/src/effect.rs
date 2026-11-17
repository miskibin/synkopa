//! One hosted VST3 effect: its UI-thread instance, the RT [`Vst3Effect`] and
//! the non-RT [`Vst3Handle`].
//!
//! # Instantiation (UI thread)
//!
//! ```text
//!  factory->setHostContext(host)                   [IPluginFactory3 only]
//!  factory->createInstance(cid, IComponent)
//!  component->initialize(host)
//!  component->getControllerClassId(&ccid)
//!      ccid == cid or none  -> single component: query IEditController off it
//!      otherwise            -> createInstance(ccid) + initialize + connect the two IConnectionPoints
//!  controller->setComponentHandler(host)
//!  state given?   component->setState, controller->setComponentState + setState
//!  otherwise      component->getState -> controller->setComponentState   (editor shows real values)
//!  check: IAudioProcessor, 32-bit float, stereo main in + out
//!  setBusArrangements(stereo) -> setupProcessing -> activateBus -> setActive(1) -> setProcessing(1)
//! ```
//!
//! # Threading
//!
//! The [`Instance`] (every COM pointer, the editor) lives behind a mutex in
//! [`Shared`] and is only ever touched from jobs run by
//! [`main_thread`](crate::main_thread). The audio thread owns the
//! [`Vst3Effect`]: its own reference to the `IAudioProcessor`, preallocated
//! buffers and parameter lists, and the consumer side of the parameter
//! mailbox. It never locks. When the last `Vst3Effect`/`Vst3Handle` goes away,
//! the instance is torn down on the UI thread (posted), in the order VST3
//! wants: editor detached, `setProcessing(0)`, `setActive(0)`, halves
//! disconnected, `terminate`, release.

#![allow(clippy::unnecessary_cast)] // enum constants are c_int on Windows, c_uint elsewhere

use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use serde::Serialize;
use vst3::Steinberg::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, BusDirections_, BusInfo, Event, Event_::EventTypes_, IAudioProcessor, IAudioProcessorTrait,
    IComponent, IComponentTrait, IConnectionPoint, IConnectionPointTrait, IEditController, IEditControllerTrait, IEventList, IMidiMapping,
    IMidiMappingTrait, IParameterChanges, MediaTypes_, NoteOffEvent, NoteOnEvent, ParamID, ParameterInfo, ParameterInfo_::ParameterFlags_,
    ProcessContext, ProcessContext_, ProcessData, ProcessModes_, ProcessSetup, SpeakerArr, SpeakerArrangement, String128,
    SymbolicSampleSizes_,
};
use vst3::Steinberg::{
    kNotImplemented, kResultOk, kResultTrue, IBStream, IPluginBaseTrait, IPluginFactory3, IPluginFactory3Trait, IPluginFactoryTrait, TUID,
};
use vst3::{ComPtr, ComWrapper};
use wave_engine::{InstEvent, InstMsg, Instrument, ProcessCtx, Processor, MAX_INST_EVENTS};

use crate::editor::EditorSession;
use crate::host::{Context, Counters, EmptyEventList, GuiRequests, HostFlags, MemoryStream, NoteEventList, ParamMailbox, ParameterChanges};
use crate::module::{utf16_to_string, ClassId, Module};
use crate::run_loop::RunLoop;
use crate::scan::{read_classes, PluginInfo};
use crate::{main_thread, Vst3Error};

/// Audio buses of one direction a plugin may declare (sidechains etc.); only
/// bus 0 carries audio, the others get silence / a scratch sink.
const MAX_BUSES: usize = 8;
/// Channels one of those extra buses may declare.
const MAX_BUS_CHANNELS: u32 = 32;
/// Parameters the controller may list.
const MAX_PARAMS: i32 = 1 << 16;

/// One parameter of a hosted plugin, as the UI shows it. All values are VST3
/// *normalized* (0..=1); `display` is the plugin's own text for the current
/// value, units included. Hidden and read-only parameters are not listed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub value: f64,
    pub display: String,
    pub def: f64,
}

// ---------------------------------------------------------------------------
// Bus layout
// ---------------------------------------------------------------------------

/// What an instance is hosted as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// A stereo insert effect (audio in, audio out).
    Effect,
    /// A Channel Rack instrument (events in, audio out).
    Instrument,
}

/// Channel count of every audio bus, per direction, and the event inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BusLayout {
    pub(crate) inputs: Vec<u32>,
    pub(crate) outputs: Vec<u32>,
    /// Event input buses (notes reach bus 0).
    pub(crate) event_inputs: u32,
}

impl BusLayout {
    pub(crate) fn check(&self, role: Role) -> Result<(), String> {
        match role {
            Role::Effect => self.check_stereo_effect(),
            Role::Instrument => self.check_instrument(),
        }
    }

    /// `Ok` for an instrument: an event input, a mono or stereo main output, and at most a
    /// few extra buses (side-chain inputs are fed silence, extra outputs discarded).
    pub(crate) fn check_instrument(&self) -> Result<(), String> {
        if self.event_inputs == 0 {
            return Err("it takes no notes (no event input)".to_owned());
        }
        let Some(&main_out) = self.outputs.first() else {
            return Err("it has no audio output".to_owned());
        };
        if !(1..=2).contains(&main_out) {
            return Err(format!("its main output has {main_out} channels, not mono or stereo"));
        }
        if self.inputs.len() > MAX_BUSES
            || self.outputs.len() > MAX_BUSES
            || self.inputs.iter().chain(&self.outputs).any(|count| *count > MAX_BUS_CHANNELS)
        {
            return Err("it declares more buses or channels than a channel supports".to_owned());
        }
        Ok(())
    }

    /// `Ok` for the shape of an insert effect: a mono or stereo main input and output,
    /// plus at most a few extra (side-chain) buses.
    pub(crate) fn check_stereo_effect(&self) -> Result<(), String> {
        let (Some(&main_in), Some(&main_out)) = (self.inputs.first(), self.outputs.first()) else {
            return Err(if self.inputs.is_empty() {
                "it has no audio input (an instrument or a generator)".to_owned()
            } else {
                "it has no audio output".to_owned()
            });
        };
        // a mono bus is fine: a mono input is fed the sum of both sides, a mono output is
        // copied to both (what LV2's mono wirings do, and what other hosts do with one)
        if !(1..=2).contains(&main_in) || !(1..=2).contains(&main_out) {
            return Err(format!("its main buses are {main_in} channel(s) in and {main_out} out, not mono or stereo"));
        }
        if self.inputs.len() > MAX_BUSES
            || self.outputs.len() > MAX_BUSES
            || self.inputs.iter().chain(&self.outputs).any(|count| *count > MAX_BUS_CHANNELS)
        {
            return Err("it declares more buses or channels than an insert slot supports".to_owned());
        }
        Ok(())
    }
}

fn read_bus_layout(component: &ComPtr<IComponent>) -> BusLayout {
    // SAFETY: live, initialised component; scalar arguments and a live
    // out-parameter. Nonsense answers produce a layout that is then refused.
    unsafe {
        let read = |direction: i32| -> Vec<u32> {
            let count = component.getBusCount(MediaTypes_::kAudio as i32, direction).clamp(0, 64);
            (0..count)
                .map(|index| {
                    let mut info: BusInfo = std::mem::zeroed();
                    if component.getBusInfo(MediaTypes_::kAudio as i32, direction, index, &mut info) == kResultOk {
                        u32::try_from(info.channelCount).unwrap_or(0)
                    } else {
                        0
                    }
                })
                .collect()
        };
        let event_inputs = component.getBusCount(MediaTypes_::kEvent as i32, BusDirections_::kInput as i32).clamp(0, 64) as u32;
        BusLayout { inputs: read(BusDirections_::kInput as i32), outputs: read(BusDirections_::kOutput as i32), event_inputs }
    }
}

// ---------------------------------------------------------------------------
// The UI-thread instance
// ---------------------------------------------------------------------------

/// Every COM object of one plugin instance. UI thread only.
///
/// Field order is the release order: editor, then the plugin's interfaces,
/// then the host context they hold raw pointers to, then the module.
pub(crate) struct Instance {
    editor: Option<EditorSession>,
    processor: Option<ComPtr<IAudioProcessor>>,
    controller: Option<ComPtr<IEditController>>,
    component_connection: Option<ComPtr<IConnectionPoint>>,
    controller_connection: Option<ComPtr<IConnectionPoint>>,
    component: Option<ComPtr<IComponent>>,
    context: Context,
    /// The plugin's run-loop registrations (Linux), removed at teardown.
    run_loop: Arc<RunLoop>,
    component_initialized: bool,
    controller_initialized: bool,
    single_component: bool,
    active: bool,
    layout: BusLayout,
    role: Role,
    /// Activated for offline processing (a render's own instance).
    offline: bool,
    name: String,
    _module: Arc<Module>,
}

// SAFETY: an `Instance` holds COM pointers (already `Send`) and, while an
// editor is open, a `wave_plugin_windows::Window` (not `Send`: its handle is
// only meaningful on the UI thread). It is moved between threads only as an
// inert value -- created inside a UI-thread job and handed back, or handed to
// the UI thread for teardown -- and every method is called from a job run by
// `main_thread`, i.e. on the UI thread. Only the editor's window enforces that
// (it re-checks the thread on every call), which is enough: nothing else in
// here is thread-affine at the Rust level.
unsafe impl Send for Instance {}

impl Instance {
    fn component(&self) -> &ComPtr<IComponent> {
        self.component.as_ref().expect("live until Drop")
    }

    fn processor(&self) -> &ComPtr<IAudioProcessor> {
        self.processor.as_ref().expect("checked at creation")
    }

    /// Create and initialise the plugin class `cid` of the module at `path`,
    /// restore `state`, and check it is a stereo effect. Not yet active.
    fn create(
        path: &Path,
        cid: ClassId,
        state: Option<&[u8]>,
        flags: Arc<HostFlags>,
        role: Role,
        offline: bool,
    ) -> Result<(Self, PluginInfo), Vst3Error> {
        let module = Module::shared(path)?;
        let class = read_classes(module.factory(), path)
            .into_iter()
            .find(|class| class.cid == cid)
            .ok_or_else(|| Vst3Error::ClassNotFound { path: path.to_path_buf(), cid: cid.to_hex() })?;
        let name = class.name.clone();
        let run_loop = RunLoop::new();
        let context = Context::new(flags, &run_loop);
        let context_ptr = context.ptr();
        let factory = module.factory();

        // SAFETY (all plugin calls below): every pointer handed over outlives
        // the call (`context` is owned by the instance being built, ids are
        // locals), out-parameters are live locals, and every returned COM
        // pointer is adopted exactly once by `ComPtr::from_raw`.
        let component = unsafe {
            if let Some(factory3) = factory.cast::<IPluginFactory3>() {
                let _ = factory3.setHostContext(context_ptr);
            }
            let tuid: TUID = cid.to_tuid();
            let mut raw: *mut std::ffi::c_void = std::ptr::null_mut();
            let result = factory.createInstance(tuid.as_ptr(), vst3::Steinberg::Vst::IComponent_iid.as_ptr(), &mut raw);
            ComPtr::<IComponent>::from_raw(raw.cast())
                .filter(|_| result == kResultOk)
                .ok_or(Vst3Error::CreateInstance { cid: cid.to_hex(), result })?
        };
        let mut instance = Self {
            editor: None,
            processor: None,
            controller: None,
            component_connection: None,
            controller_connection: None,
            component: Some(component),
            context,
            run_loop,
            component_initialized: false,
            controller_initialized: false,
            single_component: true,
            active: false,
            layout: BusLayout::default(),
            role,
            offline,
            name: name.clone(),
            _module: Arc::clone(&module),
        };
        // From here on an early return drops `instance`, which terminates
        // whatever was initialised.
        unsafe {
            let component = instance.component().clone();
            let result = component.initialize(context_ptr);
            if result != kResultOk {
                return Err(Vst3Error::Initialize { name, result });
            }
            instance.component_initialized = true;

            let mut controller_tuid: TUID = [0; 16];
            let separate = component.getControllerClassId(&mut controller_tuid) == kResultOk
                && controller_tuid != [0; 16]
                && ClassId::from_tuid(&controller_tuid) != cid;
            if separate {
                instance.single_component = false;
                let mut raw: *mut std::ffi::c_void = std::ptr::null_mut();
                let result = factory.createInstance(controller_tuid.as_ptr(), vst3::Steinberg::Vst::IEditController_iid.as_ptr(), &mut raw);
                if let Some(controller) = ComPtr::<IEditController>::from_raw(raw.cast()).filter(|_| result == kResultOk) {
                    if controller.initialize(context_ptr) == kResultOk {
                        instance.controller_initialized = true;
                        instance.controller = Some(controller);
                    } else {
                        // Costs the editor and the parameter list, not the audio.
                        log::warn!("VST3 plugin '{name}': its controller refused to initialise");
                    }
                }
                if let (Some(from_component), Some(from_controller)) =
                    (component.cast::<IConnectionPoint>(), instance.controller.as_ref().and_then(ComPtr::cast::<IConnectionPoint>))
                {
                    let _ = from_component.connect(from_controller.as_ptr());
                    let _ = from_controller.connect(from_component.as_ptr());
                    instance.component_connection = Some(from_component);
                    instance.controller_connection = Some(from_controller);
                }
            } else {
                // The controller *is* the component (already initialised).
                instance.controller = component.cast::<IEditController>();
            }
            if let Some(controller) = instance.controller.as_ref() {
                let handler = instance.context.handler();
                if !handler.is_null() {
                    let _ = controller.setComponentHandler(handler);
                }
            }
        }

        let processor = component_processor(&instance, &name)?;
        instance.processor = Some(processor);
        instance.layout = read_bus_layout(instance.component());
        instance.layout.check(role).map_err(|message| layout_error(role, &name, message))?;
        match state {
            Some(bytes) => instance.restore_state(bytes)?,
            None => instance.sync_controller_from_component(),
        }
        Ok((instance, class.to_info()))
    }

    /// The controller's parameter ids, for the mailboxes.
    fn param_ids(&self) -> Vec<ParamID> {
        let Some(controller) = self.controller.as_ref() else {
            return Vec::new();
        };
        // SAFETY: live controller, live out-parameter.
        unsafe {
            let count = controller.getParameterCount().clamp(0, MAX_PARAMS);
            (0..count)
                .filter_map(|index| {
                    let mut info: ParameterInfo = std::mem::zeroed();
                    (controller.getParameterInfo(index, &mut info) == kResultOk).then_some(info.id)
                })
                .collect()
        }
    }

    /// Copy the component's state into the controller, so the editor opens
    /// showing what the processor holds rather than class defaults.
    fn sync_controller_from_component(&self) {
        let Some(controller) = self.controller.as_ref() else {
            return;
        };
        let stream = MemoryStream::empty();
        let Some(bstream) = stream.as_com_ref::<IBStream>() else {
            return;
        };
        // SAFETY: live objects; the stream outlives both calls.
        unsafe {
            if self.component().getState(bstream.as_ptr()) == kResultOk && !stream.is_empty() {
                stream.rewind();
                let _ = controller.setComponentState(bstream.as_ptr());
            }
        }
    }

    fn restore_state(&self, bytes: &[u8]) -> Result<(), Vst3Error> {
        let (component_bytes, controller_bytes) = crate::state::decode(bytes)?;
        let stream = MemoryStream::reading(component_bytes);
        let bstream = stream.as_com_ref::<IBStream>().ok_or_else(|| Vst3Error::InvalidState("no state stream".to_owned()))?;
        // SAFETY: live objects; each stream outlives the calls reading it. The
        // rewind matters: `setState` leaves the cursor at the end.
        unsafe {
            let result = self.component().setState(bstream.as_ptr());
            if result != kResultOk {
                return Err(Vst3Error::State { name: self.name.clone(), direction: "restore", result });
            }
            if let Some(controller) = self.controller.as_ref() {
                stream.rewind();
                let _ = controller.setComponentState(bstream.as_ptr());
                if !controller_bytes.is_empty() {
                    let editor_stream = MemoryStream::reading(controller_bytes);
                    if let Some(editor_stream) = editor_stream.as_com_ref::<IBStream>() {
                        let _ = controller.setState(editor_stream.as_ptr());
                    }
                }
            }
        }
        Ok(())
    }

    fn save_state(&self) -> Result<Vec<u8>, Vst3Error> {
        let stream = MemoryStream::empty();
        let bstream = stream.as_com_ref::<IBStream>().ok_or_else(|| Vst3Error::InvalidState("no state stream".to_owned()))?;
        // SAFETY: live objects; streams outlive the calls writing them.
        let result = unsafe { self.component().getState(bstream.as_ptr()) };
        if result != kResultOk {
            return Err(Vst3Error::State { name: self.name.clone(), direction: "save", result });
        }
        // The controller's own stream (editor-only settings). A separate stream
        // for single-component plugins too.
        let controller_state = match self.controller.as_ref() {
            Some(controller) => {
                let editor_stream = MemoryStream::empty();
                match editor_stream.as_com_ref::<IBStream>() {
                    // SAFETY: as above.
                    Some(bstream) if unsafe { controller.getState(bstream.as_ptr()) } == kResultOk => editor_stream.contents(),
                    _ => Vec::new(),
                }
            }
            None => Vec::new(),
        };
        crate::state::encode(&stream.contents(), &controller_state)
    }

    /// (Re)activate for `sample_rate` and blocks of at most `max_block`.
    /// Returns the bus layout the plugin settled on.
    fn activate(&mut self, sample_rate: f64, max_block: usize) -> Result<BusLayout, Vst3Error> {
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(Vst3Error::InvalidAudioConfig(format!("sample rate {sample_rate} is not a positive number")));
        }
        let max_samples = i32::try_from(max_block)
            .ok()
            .filter(|max| *max > 0)
            .ok_or_else(|| Vst3Error::InvalidAudioConfig(format!("block size {max_block}")))?;
        self.deactivate();

        let component = self.component().clone();
        let processor = self.processor().clone();
        let name = self.name.clone();
        let refused = |stage: &'static str, result: i32| Vst3Error::Activate { name: name.clone(), stage, result };
        // SAFETY: live objects, the component is inactive, every pointer is to
        // a live local. This is the activation order VST3 requires.
        unsafe {
            let role = self.role;
            let arrangements = |direction: i32, count: usize| -> Vec<SpeakerArrangement> {
                (0..count)
                    .map(|index| {
                        // the main pair is stereo; an instrument's inputs are side-chains
                        if index == 0 && (role == Role::Effect || direction == BusDirections_::kOutput as i32) {
                            return SpeakerArr::kStereo;
                        }
                        let mut current: SpeakerArrangement = 0;
                        let _ = processor.getBusArrangement(direction, index as i32, &mut current);
                        current
                    })
                    .collect()
            };
            let mut inputs = arrangements(BusDirections_::kInput as i32, self.layout.inputs.len());
            let mut outputs = arrangements(BusDirections_::kOutput as i32, self.layout.outputs.len());
            let result = processor.setBusArrangements(inputs.as_mut_ptr(), inputs.len() as i32, outputs.as_mut_ptr(), outputs.len() as i32);
            if result != kResultTrue {
                // Not fatal: the layout check below decides.
                log::debug!("VST3 plugin '{name}' declined the stereo arrangement ({result:#x})");
            }
            let layout = read_bus_layout(&component);
            layout.check(self.role).map_err(|message| layout_error(self.role, &name, message))?;

            let mut setup = ProcessSetup {
                processMode: process_mode(self.offline),
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
                maxSamplesPerBlock: max_samples,
                sampleRate: sample_rate,
            };
            let result = processor.setupProcessing(&mut setup);
            if result != kResultOk {
                return Err(refused("setting up processing", result));
            }
            // Buses default to inactive unless flagged otherwise; only the
            // main pair carries audio.
            for (direction, count) in
                [(BusDirections_::kInput as i32, layout.inputs.len()), (BusDirections_::kOutput as i32, layout.outputs.len())]
            {
                for index in 0..count {
                    let main = index == 0 && (self.role == Role::Effect || direction == BusDirections_::kOutput as i32);
                    let _ = component.activateBus(MediaTypes_::kAudio as i32, direction, index as i32, u8::from(main));
                }
            }
            if self.role == Role::Instrument {
                for index in 0..layout.event_inputs {
                    let _ = component.activateBus(
                        MediaTypes_::kEvent as i32,
                        BusDirections_::kInput as i32,
                        index as i32,
                        u8::from(index == 0),
                    );
                }
            }
            let result = component.setActive(1);
            if result != kResultOk {
                return Err(refused("being activated", result));
            }
            let result = processor.setProcessing(1);
            if result != kResultOk && result != kNotImplemented {
                let _ = component.setActive(0);
                return Err(refused("starting processing", result));
            }
            self.active = true;
            self.layout = layout.clone();
            Ok(layout)
        }
    }

    fn deactivate(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        // SAFETY: live objects; the counterparts of `activate`'s calls. No
        // `process` runs concurrently: callers are `prepare` (contract: not
        // while processing) and teardown (the effect is gone).
        unsafe {
            let _ = self.processor().setProcessing(0);
            let _ = self.component().setActive(0);
        }
    }

    fn params(&self) -> Vec<ParamInfo> {
        let Some(controller) = self.controller.as_ref() else {
            return Vec::new();
        };
        // SAFETY: live controller; live out-parameters.
        unsafe {
            let count = controller.getParameterCount().clamp(0, MAX_PARAMS);
            let mut list = Vec::with_capacity(count as usize);
            for index in 0..count {
                let mut info: ParameterInfo = std::mem::zeroed();
                if controller.getParameterInfo(index, &mut info) != kResultOk {
                    continue;
                }
                let hidden = ParameterFlags_::kIsHidden as i32 | ParameterFlags_::kIsReadOnly as i32;
                if info.flags & hidden != 0 {
                    continue;
                }
                let value = controller.getParamNormalized(info.id);
                let mut text: String128 = [0; 128];
                let mut display = if controller.getParamStringByValue(info.id, value, &mut text) == kResultOk {
                    utf16_to_string(&text)
                } else {
                    format!("{value:.3}")
                };
                let units = utf16_to_string(&info.units);
                if !units.is_empty() && !display.ends_with(&units) {
                    display = format!("{display} {units}");
                }
                let mut name = utf16_to_string(&info.title);
                if name.is_empty() {
                    name = utf16_to_string(&info.shortTitle);
                }
                list.push(ParamInfo { id: info.id, name, value, display, def: info.defaultNormalizedValue });
            }
            list
        }
    }

    /// The processing delay the plugin reports, in samples.
    fn latency(&self) -> u32 {
        // SAFETY: live processor; a scalar getter.
        unsafe { self.processor().getLatencySamples() }
    }

    /// The parameter each MIDI controller (and pitch bend) of channel 1 on event bus 0 is
    /// mapped to, from the controller's `IMidiMapping` (all `None` without one).
    fn midi_controllers(&self) -> ControllerMap {
        let mut map: ControllerMap = Box::new([None; CONTROLLERS]);
        let Some(mapping) = self.controller.as_ref().and_then(ComPtr::cast::<IMidiMapping>) else {
            return map;
        };
        for (number, slot) in map.iter_mut().enumerate() {
            let mut id: ParamID = 0;
            // SAFETY: live interface, scalar arguments, a live out-parameter.
            if unsafe { mapping.getMidiControllerAssignment(0, 0, number as i16, &mut id) } == kResultTrue {
                *slot = Some(id);
            }
        }
        map
    }

    fn set_controller_param(&self, id: ParamID, value: f64) {
        if let Some(controller) = self.controller.as_ref() {
            // SAFETY: live controller, scalar arguments.
            let _ = unsafe { controller.setParamNormalized(id, value) };
        }
    }
}

fn layout_error(role: Role, name: &str, message: String) -> Vst3Error {
    match role {
        Role::Effect => Vst3Error::NotAStereoEffect { name: name.to_owned(), message },
        Role::Instrument => Vst3Error::NotAnInstrument { name: name.to_owned(), message },
    }
}

fn process_mode(offline: bool) -> i32 {
    if offline {
        ProcessModes_::kOffline as i32
    } else {
        ProcessModes_::kRealtime as i32
    }
}

fn component_processor(instance: &Instance, name: &str) -> Result<ComPtr<IAudioProcessor>, Vst3Error> {
    let processor = instance.component().cast::<IAudioProcessor>().ok_or_else(|| Vst3Error::NoAudioProcessor { name: name.to_owned() })?;
    // SAFETY: live processor, scalar argument.
    if unsafe { processor.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as i32) } != kResultTrue {
        return Err(Vst3Error::NoFloat32 { name: name.to_owned() });
    }
    Ok(processor)
}

impl Drop for Instance {
    fn drop(&mut self) {
        // The editor's child window belongs to the plugin: it must go first.
        drop(self.editor.take());
        if self.processor.is_some() && self.component.is_some() {
            self.deactivate();
        }
        // SAFETY: live handles, each terminated once (a single-component
        // plugin's controller *is* its component).
        unsafe {
            if let (Some(from_component), Some(from_controller)) = (self.component_connection.as_ref(), self.controller_connection.as_ref())
            {
                let _ = from_component.disconnect(from_controller.as_ptr());
                let _ = from_controller.disconnect(from_component.as_ptr());
            }
            if let Some(controller) = self.controller.as_ref() {
                let _ = controller.setComponentHandler(std::ptr::null_mut());
                if !self.single_component && self.controller_initialized {
                    let _ = controller.terminate();
                }
            }
            if self.component_initialized {
                if let Some(component) = self.component.as_ref() {
                    let _ = component.terminate();
                }
            }
        }
        drop(self.controller_connection.take());
        drop(self.component_connection.take());
        drop(self.controller.take());
        drop(self.processor.take());
        drop(self.component.take());
        // whatever the plugin left registered must not outlive its library
        self.run_loop.clear();
    }
}

// ---------------------------------------------------------------------------
// Shared between the RT effect and the handles
// ---------------------------------------------------------------------------

pub(crate) struct Shared {
    info: PluginInfo,
    has_editor: bool,
    instance: Mutex<Option<Instance>>,
    /// Host -> processor.
    inbox: Arc<ParamMailbox>,
    /// Processor -> host (output parameter changes, e.g. meters, automation
    /// the plugin generates). Drained into the controller on the UI thread.
    feedback: ParamMailbox,
    flags: Arc<HostFlags>,
    gui: Arc<GuiRequests>,
    counters: Counters,
    editor_open: AtomicBool,
    /// What `IAudioProcessor::getLatencySamples` said at the last (re)activation or
    /// latency-change request.
    latency: AtomicU32,
}

/// `IComponentHandler::restartComponent` flag: the plugin's latency changed.
const LATENCY_CHANGED: i32 = 8;

impl Shared {
    /// Hand the processor's output parameter values to the controller. UI
    /// thread, with the instance locked.
    fn apply_feedback(&self, instance: &Instance) {
        self.feedback.drain(usize::MAX, |id, value| instance.set_controller_param(id, value));
    }

    /// The editor's per-tick work. UI thread.
    fn tick_editor(&self) {
        // `try_lock`: a plugin running a modal loop inside a call we made can
        // deliver a nested tick; skip it rather than deadlock.
        let Some(mut guard) = self.instance.try_lock() else {
            return;
        };
        let Some(instance) = guard.as_mut() else {
            return;
        };
        self.apply_feedback(instance);
        let closed = instance.editor.as_mut().is_some_and(EditorSession::pump);
        if closed {
            drop(instance.editor.take());
            self.editor_open.store(false, Ordering::Release);
        }
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        if let Some(instance) = self.instance.get_mut().take() {
            main_thread::post("being torn down", move || drop(instance));
        }
    }
}

// ---------------------------------------------------------------------------
// The RT side
// ---------------------------------------------------------------------------

/// Preallocated audio I/O for one activation.
struct Io {
    main_in: [Vec<f32>; 2],
    main_out: [Vec<f32>; 2],
    /// What extra input buses read (re-zeroed each block: a plugin may write
    /// into its inputs).
    silence: Vec<f32>,
    /// Where extra output buses write; discarded.
    sink: Vec<f32>,
    in_channels: Vec<u32>,
    out_channels: Vec<u32>,
    in_ptrs: Vec<*mut f32>,
    out_ptrs: Vec<*mut f32>,
    in_buses: Vec<AudioBusBuffers>,
    out_buses: Vec<AudioBusBuffers>,
}

impl Io {
    fn new(layout: &BusLayout, max_block: usize) -> Self {
        let total = |channels: &[u32]| channels.iter().map(|count| *count as usize).sum::<usize>();
        let bus = |channels: u32| AudioBusBuffers {
            numChannels: channels as i32,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 { channelBuffers32: std::ptr::null_mut() },
        };
        Self {
            main_in: [vec![0.0; max_block], vec![0.0; max_block]],
            main_out: [vec![0.0; max_block], vec![0.0; max_block]],
            silence: vec![0.0; max_block],
            sink: vec![0.0; max_block],
            in_ptrs: vec![std::ptr::null_mut(); total(&layout.inputs)],
            out_ptrs: vec![std::ptr::null_mut(); total(&layout.outputs)],
            in_buses: layout.inputs.iter().map(|count| bus(*count)).collect(),
            out_buses: layout.outputs.iter().map(|count| bus(*count)).collect(),
            in_channels: layout.inputs.clone(),
            out_channels: layout.outputs.clone(),
        }
    }

    /// Point every bus at its buffers. Rewritten each block (a few stores):
    /// nothing a plugin did to the arrays last block can leak into this one.
    fn wire(&mut self) {
        let mut cursor = 0;
        for (index, (bus, channels)) in self.in_buses.iter_mut().zip(&self.in_channels).enumerate() {
            let start = cursor;
            for channel in 0..*channels as usize {
                self.in_ptrs[cursor] = match (index, channel) {
                    (0, 0 | 1) => self.main_in[channel].as_mut_ptr(),
                    _ => self.silence.as_mut_ptr(),
                };
                cursor += 1;
            }
            bus.numChannels = *channels as i32;
            bus.silenceFlags = 0;
            bus.__field0.channelBuffers32 = self.in_ptrs[start..].as_mut_ptr();
        }
        let mut cursor = 0;
        for (index, (bus, channels)) in self.out_buses.iter_mut().zip(&self.out_channels).enumerate() {
            let start = cursor;
            for channel in 0..*channels as usize {
                self.out_ptrs[cursor] = match (index, channel) {
                    (0, 0 | 1) => self.main_out[channel].as_mut_ptr(),
                    _ => self.sink.as_mut_ptr(),
                };
                cursor += 1;
            }
            bus.numChannels = *channels as i32;
            bus.silenceFlags = 0;
            bus.__field0.channelBuffers32 = self.out_ptrs[start..].as_mut_ptr();
        }
    }

    fn has_extra_inputs(&self) -> bool {
        self.in_channels.len() > 1
    }
}

/// A hosted VST3 effect on the audio thread. Implements
/// [`wave_engine::Processor`]; get one from [`instantiate`].
///
/// [`process`](Processor::process) is real-time safe: it copies the block into
/// preallocated buffers, drains the parameter mailbox (atomics) into
/// preallocated `IParameterChanges`, calls the plugin, copies the result back
/// and posts the plugin's output parameter values (atomics). No allocation, no
/// lock, no syscall, no logging. Blocks longer than the activated maximum are
/// processed in chunks. If the plugin reports an error for a block, that
/// block passes through unprocessed.
///
/// [`prepare`](Processor::prepare) with a new sample rate or a larger block
/// re-activates the plugin on the UI thread (blocking); with the same
/// configuration it does nothing. [`reset`](Processor::reset) is a no-op:
/// VST3 has no real-time-safe reset (only `setActive` cycling, which is UI
/// thread work).
///
/// Drop it off the audio thread (the engine's contract already does); the
/// plugin itself is torn down on the UI thread once every handle is gone too.
pub struct Vst3Effect {
    /// Released before `shared`, i.e. before the instance can be torn down.
    processor: ComPtr<IAudioProcessor>,
    io: Io,
    input_changes: ComWrapper<ParameterChanges>,
    output_changes: ComWrapper<ParameterChanges>,
    events: ComWrapper<EmptyEventList>,
    /// Consumer side of the inbox's overflow ring.
    unlisted: rtrb::Consumer<(ParamID, f64)>,
    sample_rate: f64,
    max_block: usize,
    continuous_samples: i64,
    /// Re-activation failed: pass audio through untouched.
    failed: bool,
    shared: Arc<Shared>,
}

// SAFETY: the raw channel pointers in `io` point into `io`'s own heap buffers,
// which do not move when the effect moves, and they are rewritten before every
// use. The `ComWrapper`s are owned exclusively by this effect (their `Send` /
// `Sync` assertions are documented in `host`). The effect is moved to the audio
// thread as a whole and used there by one thread at a time.
unsafe impl Send for Vst3Effect {}

impl Vst3Effect {
    fn run_block(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx, offset: usize) {
        let frames = l.len();
        if self.io.in_channels.first().is_some_and(|channels| *channels == 1) {
            // a mono input hears both sides
            for ((m, a), b) in self.io.main_in[0][..frames].iter_mut().zip(l.iter()).zip(r.iter()) {
                *m = 0.5 * (a + b);
            }
        } else {
            self.io.main_in[0][..frames].copy_from_slice(l);
            self.io.main_in[1][..frames].copy_from_slice(r);
        }
        if self.io.has_extra_inputs() {
            self.io.silence[..frames].fill(0.0);
        }
        self.io.wire();
        self.input_changes.fill(&self.shared.inbox, &mut self.unlisted);
        self.output_changes.clear();

        let mut context = process_context(self.sample_rate, self.continuous_samples, ctx, offset);
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: frames as i32,
            numInputs: self.io.in_buses.len() as i32,
            numOutputs: self.io.out_buses.len() as i32,
            inputs: self.io.in_buses.as_mut_ptr(),
            outputs: self.io.out_buses.as_mut_ptr(),
            inputParameterChanges: self
                .input_changes
                .as_com_ref::<IParameterChanges>()
                .map_or(std::ptr::null_mut(), |changes| changes.as_ptr()),
            outputParameterChanges: self
                .output_changes
                .as_com_ref::<IParameterChanges>()
                .map_or(std::ptr::null_mut(), |changes| changes.as_ptr()),
            inputEvents: self.events.as_com_ref::<IEventList>().map_or(std::ptr::null_mut(), |events| events.as_ptr()),
            outputEvents: std::ptr::null_mut(),
            processContext: &mut context,
        };
        // SAFETY: the plugin is active and processing (activation happened on
        // the UI thread before this effect could be handed to the audio thread,
        // and only `prepare` -- never concurrent with `process` -- changes it).
        // Every pointer in `data` is to a live local or this effect's own
        // buffers; each channel buffer holds at least `frames <= max_block`
        // samples, the maximum given to `setupProcessing`.
        // a plugin that flags its output silent may leave the buffer as it was: never replay
        // the previous block (that is a buzz). The buffers are the plugin's, not yet in `data`.
        self.io.main_out[0][..frames].fill(0.0);
        self.io.main_out[1][..frames].fill(0.0);
        let result = unsafe { self.processor.process(&mut data) };
        self.continuous_samples += frames as i64;
        if result == kResultOk {
            l.copy_from_slice(&self.io.main_out[0][..frames]);
            // a mono output plays on both sides
            let stereo = self.io.out_channels.first().is_some_and(|channels| *channels >= 2);
            r.copy_from_slice(&self.io.main_out[usize::from(stereo)][..frames]);
        } else {
            self.shared.counters.process_errors.fetch_add(1, Ordering::Relaxed);
        }
        let shared = &self.shared;
        self.output_changes.for_each_last(|id, value| {
            if !shared.feedback.post_listed(id, value) {
                shared.counters.feedback_dropped.fetch_add(1, Ordering::Relaxed);
            }
        });
    }
}

/// The transport as VST3 sees it. Only what WAVE actually knows is marked valid: a plugin
/// syncing to a field the host left at zero is worse off than one told nothing.
fn process_context(sample_rate: f64, continuous_samples: i64, ctx: &ProcessCtx, offset: usize) -> ProcessContext {
    // SAFETY: a plain aggregate of scalars; all-zero is a valid value.
    let mut context: ProcessContext = unsafe { std::mem::zeroed() };
    context.sampleRate = sample_rate;
    context.continousTimeSamples = continuous_samples;
    context.state = ProcessContext_::StatesAndFlags_::kContTimeValid as u32;
    if ctx.bpm.is_finite() && ctx.bpm > 0.0 {
        let beat = ctx.beat + offset as f64 / sample_rate * ctx.bpm / 60.0;
        context.tempo = ctx.bpm;
        context.projectTimeMusic = beat;
        context.projectTimeSamples = (beat * 60.0 / ctx.bpm * sample_rate).round() as i64;
        context.timeSigNumerator = 4;
        context.timeSigDenominator = 4;
        context.state |= ProcessContext_::StatesAndFlags_::kTempoValid as u32
            | ProcessContext_::StatesAndFlags_::kProjectTimeMusicValid as u32
            | ProcessContext_::StatesAndFlags_::kTimeSigValid as u32;
    }
    if ctx.playing {
        context.state |= ProcessContext_::StatesAndFlags_::kPlaying as u32;
    }
    context
}

impl Processor for Vst3Effect {
    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        let sample_rate = f64::from(sample_rate);
        if (sample_rate - self.sample_rate).abs() < 1e-6 && max_block <= self.max_block && !self.failed {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let activated = main_thread::run("being re-activated", move || {
            let mut guard = shared.instance.lock();
            let instance = guard.as_mut().ok_or(Vst3Error::Closed)?;
            let layout = instance.activate(sample_rate, max_block)?;
            shared.latency.store(instance.latency(), Ordering::Release);
            Ok(layout)
        })
        .and_then(|result| result);
        match activated {
            Ok(layout) => {
                self.io = Io::new(&layout, max_block);
                self.sample_rate = sample_rate;
                self.max_block = max_block;
                self.failed = false;
            }
            Err(err) => {
                log::error!(
                    "VST3 plugin '{}' could not be re-activated at {sample_rate} Hz / {max_block}: {err}; it is bypassed",
                    self.shared.info.name
                );
                self.failed = true;
            }
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        if self.failed || self.max_block == 0 {
            return;
        }
        let frames = l.len().min(r.len());
        let mut done = 0;
        while done < frames {
            let chunk = (frames - done).min(self.max_block);
            let (l_chunk, r_chunk) = (&mut l[done..done + chunk], &mut r[done..done + chunk]);
            self.run_block(l_chunk, r_chunk, ctx, done);
            done += chunk;
        }
    }

    fn latency_samples(&self) -> usize {
        self.shared.latency.load(Ordering::Acquire) as usize
    }

    fn set_plugin_param(&mut self, param_id: u32, normalized: f32) {
        let v = f64::from(normalized);
        // the mailbox is the same lock-free path `set_param` uses: atomics only,
        // drained by the next `run_block`. Automation values are already
        // normalized 0..1; clamp finite strays, drop NaN.
        if v.is_finite() {
            self.shared.inbox.post(param_id, v.clamp(0.0, 1.0));
        }
    }
}

impl std::fmt::Debug for Vst3Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst3Effect")
            .field("plugin", &self.shared.info.name)
            .field("sample_rate", &self.sample_rate)
            .field("max_block", &self.max_block)
            .field("failed", &self.failed)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// The non-RT handle
// ---------------------------------------------------------------------------

/// The non-real-time side of a hosted effect: parameters, state, editor.
///
/// `Send + Sync` and cheap to clone (an `Arc`). Every method that talks to the
/// plugin runs on the UI thread through the installed executor (see
/// [`set_main_thread_executor`](crate::set_main_thread_executor)) and blocks
/// until done, except [`set_param`](Self::set_param), which never blocks. The
/// editor methods must be *called* on the UI thread.
#[derive(Clone)]
pub struct Vst3Handle {
    shared: Arc<Shared>,
}

impl Vst3Handle {
    /// What was instantiated.
    pub fn info(&self) -> &PluginInfo {
        &self.shared.info
    }

    /// The plugin's parameters with their current values and display text.
    /// Empty for a plugin without an edit controller, or after an error
    /// (logged).
    pub fn params(&self) -> Vec<ParamInfo> {
        let shared = Arc::clone(&self.shared);
        main_thread::run("listing its parameters", move || {
            let guard = shared.instance.lock();
            guard.as_ref().map_or_else(Vec::new, |instance| {
                shared.apply_feedback(instance);
                instance.params()
            })
        })
        .unwrap_or_else(|err| {
            log::warn!("VST3 plugin '{}': {err}", self.shared.info.name);
            Vec::new()
        })
    }

    /// Set a parameter to a normalized value (0..=1). Never blocks: the audio
    /// thread picks it up at its next block (coalesced with any newer move of
    /// the same parameter), and the controller -- hence an open editor -- is
    /// told on the UI thread.
    ///
    /// # Errors
    /// [`Vst3Error::InvalidParamValue`] outside 0..=1; [`Vst3Error::ParamQueueFull`]
    /// only for an id the plugin did not list and a full overflow ring.
    pub fn set_param(&self, id: u32, value: f64) -> Result<(), Vst3Error> {
        if !(value.is_finite() && (0.0..=1.0).contains(&value)) {
            return Err(Vst3Error::InvalidParamValue(value));
        }
        if !self.shared.inbox.post(id, value) {
            return Err(Vst3Error::ParamQueueFull);
        }
        self.shared.flags.dirty.store(true, Ordering::Release);
        let shared = Arc::clone(&self.shared);
        main_thread::post("taking a parameter value", move || {
            if let Some(instance) = shared.instance.lock().as_ref() {
                instance.set_controller_param(id, value);
            }
        });
        Ok(())
    }

    /// The plugin's complete state (processor + controller), as an opaque blob
    /// to store in the project and pass back to [`instantiate`].
    pub fn get_state(&self) -> Result<Vec<u8>, Vst3Error> {
        let shared = Arc::clone(&self.shared);
        main_thread::run("saving its state", move || shared.instance.lock().as_ref().ok_or(Vst3Error::Closed)?.save_state())?
    }

    /// Whether the plugin's state may have changed since the last call (an
    /// edit in its editor, a parameter set through this handle, a plugin
    /// restart request). Clears the flag. Lets the app re-save the state and
    /// mark the project dirty only when something happened.
    pub fn take_state_changed(&self) -> bool {
        self.shared.flags.dirty.swap(false, Ordering::AcqRel)
    }

    /// The processing delay (samples) the plugin reports, for delay compensation. When
    /// the plugin announced a latency change (`restartComponent(kLatencyChanged)`), it
    /// is asked again on the UI thread (blocking); otherwise this is an atomic read.
    pub fn latency(&self) -> usize {
        if self.shared.flags.restart.fetch_and(!LATENCY_CHANGED, Ordering::AcqRel) & LATENCY_CHANGED != 0 {
            let shared = Arc::clone(&self.shared);
            let asked = main_thread::run("reporting its latency", move || {
                if let Some(instance) = shared.instance.lock().as_ref() {
                    shared.latency.store(instance.latency(), Ordering::Release);
                }
            });
            if let Err(err) = asked {
                log::warn!("VST3 plugin '{}': {err}", self.shared.info.name);
            }
        }
        self.shared.latency.load(Ordering::Acquire) as usize
    }

    /// Whether the plugin has an edit controller, and so *may* have an editor
    /// (`createView` can still decline).
    pub fn has_editor(&self) -> bool {
        self.shared.has_editor
    }

    /// Whether its editor window is open. Turns `false` by itself when the
    /// user closes the window.
    pub fn is_editor_open(&self) -> bool {
        self.shared.editor_open.load(Ordering::Acquire)
    }

    /// Open the plugin's editor in its own native window, or bring it to the
    /// front if already open. **UI thread only** (the thread that created
    /// `windows`); from Tauri, call it inside `AppHandle::run_on_main_thread`.
    ///
    /// After this the editor services itself from its window's timer (resize
    /// requests, user drags, DPI changes, the close button): no per-frame call
    /// is needed.
    ///
    /// `label` is what WAVE's header above the editor shows, and the tag its gear reports.
    ///
    /// # Errors
    /// [`Vst3Error::WrongThread`] off the UI thread, [`Vst3Error::NoEditor`]
    /// for a plugin without one, [`Vst3Error::Window`] where the platform has
    /// no window backend (macOS) or no display (Linux without X11/XWayland), and the
    /// plugin's refusals.
    pub fn open_editor(&self, windows: &wave_plugin_windows::Service, label: &wave_plugin_windows::Label) -> Result<(), Vst3Error> {
        if !windows.on_owner_thread() || !main_thread::on_main_thread() {
            return Err(Vst3Error::WrongThread);
        }
        crate::module::enter_com_apartment();
        let mut guard = self.shared.instance.lock();
        let instance = guard.as_mut().ok_or(Vst3Error::Closed)?;
        if let Some(editor) = instance.editor.as_ref() {
            editor.raise();
            return Ok(());
        }
        let controller = instance.controller.clone().ok_or(Vst3Error::NoEditor)?;
        let session = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            EditorSession::open(&controller, windows, label, Arc::clone(&self.shared.gui), Arc::clone(&instance.run_loop))
        }))
        .map_err(|_| Vst3Error::Panicked { stage: "opening its editor" })??;
        let weak: Weak<Shared> = Arc::downgrade(&self.shared);
        let tick: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(shared) = weak.upgrade() {
                shared.tick_editor();
            }
        });
        session.window().set_tick_handler(Some(tick))?;
        instance.editor = Some(session);
        self.shared.editor_open.store(true, Ordering::Release);
        Ok(())
    }

    /// Close the editor (no-op when closed). **UI thread only.**
    pub fn close_editor(&self) -> Result<(), Vst3Error> {
        if !main_thread::on_main_thread() {
            return Err(Vst3Error::WrongThread);
        }
        let session = self.shared.instance.lock().as_mut().and_then(|instance| instance.editor.take());
        // Dropped outside the lock: closing may run the window's handlers.
        drop(session);
        self.shared.editor_open.store(false, Ordering::Release);
        Ok(())
    }

    /// Pin the editor's window on top of the app's (no-op when it is closed). **UI thread only.**
    pub fn set_editor_pinned(&self, pinned: bool) -> Result<(), Vst3Error> {
        if !main_thread::on_main_thread() {
            return Err(Vst3Error::WrongThread);
        }
        let guard = self.shared.instance.lock();
        if let Some(editor) = guard.as_ref().and_then(|instance| instance.editor.as_ref()) {
            let _ = editor.window().set_pinned(pinned);
        }
        Ok(())
    }
}

impl std::fmt::Debug for Vst3Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst3Handle").field("plugin", &self.shared.info).field("editor_open", &self.is_editor_open()).finish()
    }
}

/// Load the effect `cid` from the module at `path`, restore `state` (a blob
/// from [`Vst3Handle::get_state`]; `None` for a fresh instance) and activate it
/// for `sample_rate` and blocks of at most `max_block` frames.
///
/// Call it off the audio thread; it blocks while the UI thread does the work
/// (see [`set_main_thread_executor`](crate::set_main_thread_executor)), so do
/// not hold a lock the UI thread may wait for. The returned processor is
/// already prepared for (`sample_rate`, `max_block`).
///
/// Runs third-party code in this process: a module that the scanner
/// block-listed should not be passed here.
///
/// # Errors
/// Everything in [`Vst3Error`] that loading, creating, restoring and
/// activating can produce -- notably [`Vst3Error::NotAStereoEffect`] for an
/// instrument or a mono/surround-only plugin.
pub fn instantiate(
    path: &str,
    cid: &str,
    state: Option<&[u8]>,
    sample_rate: f32,
    max_block: usize,
) -> Result<(Box<dyn Processor>, Vst3Handle), Vst3Error> {
    let (effect, handle) = instantiate_effect(path, cid, state, sample_rate, max_block)?;
    Ok((Box::new(effect), handle))
}

/// [`instantiate`] returning the concrete type.
pub(crate) fn instantiate_effect(
    path: &str,
    cid: &str,
    state: Option<&[u8]>,
    sample_rate: f32,
    max_block: usize,
) -> Result<(Vst3Effect, Vst3Handle), Vst3Error> {
    let parts = create_parts(path, cid, state, sample_rate, max_block, Role::Effect, false)?;
    let effect = Vst3Effect {
        processor: parts.processor,
        io: Io::new(&parts.layout, max_block),
        input_changes: ParameterChanges::new(),
        output_changes: ParameterChanges::new(),
        events: EmptyEventList::new(),
        unlisted: parts.unlisted,
        sample_rate: parts.sample_rate,
        max_block,
        continuous_samples: 0,
        failed: false,
        shared: Arc::clone(&parts.shared),
    };
    Ok((effect, Vst3Handle { shared: parts.shared }))
}

/// MIDI controller numbers a plugin may map (`IMidiMapping`): 0..=127 plus the VST3
/// pseudo-controllers up to pitch bend (129).
pub(crate) const CONTROLLERS: usize = 130;
/// VST3's controller number of pitch bend.
const PITCH_BEND: usize = 129;

/// Parameter each MIDI controller is mapped to.
pub(crate) type ControllerMap = Box<[Option<ParamID>; CONTROLLERS]>;

/// What creating any hosted instance yields.
struct Parts {
    shared: Arc<Shared>,
    processor: ComPtr<IAudioProcessor>,
    layout: BusLayout,
    unlisted: rtrb::Consumer<(ParamID, f64)>,
    controllers: ControllerMap,
    sample_rate: f64,
}

fn create_parts(
    path: &str,
    cid: &str,
    state: Option<&[u8]>,
    sample_rate: f32,
    max_block: usize,
    role: Role,
    offline: bool,
) -> Result<Parts, Vst3Error> {
    let class_id = ClassId::from_hex(cid).ok_or_else(|| Vst3Error::InvalidClassId(cid.to_owned()))?;
    let path = std::path::PathBuf::from(path);
    let state = state.map(<[u8]>::to_vec);
    let flags = Arc::new(HostFlags::default());
    let sample_rate = f64::from(sample_rate);

    let job_flags = Arc::clone(&flags);
    let created = main_thread::run("being instantiated", move || {
        let (mut instance, info) = Instance::create(&path, class_id, state.as_deref(), job_flags, role, offline)?;
        let layout = instance.activate(sample_rate, max_block)?;
        let (inbox, unlisted) = ParamMailbox::new(&instance.param_ids());
        let inbox = Arc::new(inbox);
        instance.context.connect_inbox(Arc::clone(&inbox));
        let (feedback, _) = ParamMailbox::new(&instance.param_ids());
        let processor = instance.processor().clone();
        let latency = instance.latency();
        let controllers = if role == Role::Instrument { instance.midi_controllers() } else { Box::new([None; CONTROLLERS]) };
        Ok::<_, Vst3Error>((instance, info, layout, inbox, unlisted, feedback, processor, latency, controllers))
    })??;
    let (instance, info, layout, inbox, unlisted, feedback, processor, latency, controllers) = created;

    let has_editor = instance.controller.is_some();
    let shared = Arc::new(Shared {
        info,
        has_editor,
        instance: Mutex::new(Some(instance)),
        inbox,
        feedback,
        flags,
        gui: Arc::new(GuiRequests::default()),
        counters: Counters::default(),
        editor_open: AtomicBool::new(false),
        latency: AtomicU32::new(latency),
    });
    // Restoring a state is not an edit.
    shared.flags.dirty.store(false, Ordering::Release);
    Ok(Parts { shared, processor, layout, unlisted, controllers, sample_rate })
}

// ---------------------------------------------------------------------------
// Instruments
// ---------------------------------------------------------------------------

/// A hosted VST3 instrument on the audio thread: a Channel Rack channel's sound. Implements
/// [`wave_engine::Instrument`]; get one from [`instantiate_instrument`].
///
/// [`process`](Instrument::process) is real-time safe like [`Vst3Effect`]'s: the block's
/// notes go into a preallocated `IEventList` (note on/off with their sample offsets and
/// note ids), MIDI controllers and pitch bend become sample-accurate points of the
/// parameters the plugin maps them to (`IMidiMapping`; unmapped ones are dropped), the
/// parameter mailbox is drained as for an effect, and the main output is copied out (a mono
/// output to both sides). Audio inputs (side-chains) read silence, extra outputs are
/// discarded. If the plugin reports an error for a block, that block is silent.
pub struct Vst3Instrument {
    processor: ComPtr<IAudioProcessor>,
    io: Io,
    input_changes: ComWrapper<ParameterChanges>,
    output_changes: ComWrapper<ParameterChanges>,
    events: ComWrapper<NoteEventList>,
    unlisted: rtrb::Consumer<(ParamID, f64)>,
    controllers: ControllerMap,
    sample_rate: f64,
    max_block: usize,
    continuous_samples: i64,
    offline: bool,
    failed: bool,
    shared: Arc<Shared>,
}

// SAFETY: as for `Vst3Effect`.
unsafe impl Send for Vst3Instrument {}

/// The VST3 event for a note message (`None` for controllers, which travel as parameters).
pub(crate) fn note_event(offset: u32, msg: InstMsg) -> Option<Event> {
    // SAFETY: a plain aggregate of scalars and a union of them; all-zero is a valid value.
    let mut event: Event = unsafe { std::mem::zeroed() };
    event.busIndex = 0;
    event.sampleOffset = offset as i32;
    match msg {
        InstMsg::NoteOn { key, vel, id } => {
            event.r#type = EventTypes_::kNoteOnEvent as u16;
            event.__field0.noteOn = NoteOnEvent { channel: 0, pitch: i16::from(key), tuning: 0.0, velocity: vel, length: 0, noteId: id };
        }
        InstMsg::NoteOff { key, id } => {
            event.r#type = EventTypes_::kNoteOffEvent as u16;
            event.__field0.noteOff = NoteOffEvent { channel: 0, pitch: i16::from(key), velocity: 0.0, noteId: id, tuning: 0.0 };
        }
        InstMsg::Control { .. } | InstMsg::Bend(_) => return None,
    }
    Some(event)
}

impl Vst3Instrument {
    /// One plugin call over `l.len()` frames starting at `offset` in the host block; of the
    /// host block's sorted `events`, those inside this chunk are delivered.
    fn run_block(&mut self, events: &[InstEvent], l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx, offset: usize) {
        let frames = l.len();
        for buffer in &mut self.io.main_in {
            buffer[..frames].fill(0.0);
        }
        self.io.silence[..frames].fill(0.0);
        self.io.wire();
        self.input_changes.fill(&self.shared.inbox, &mut self.unlisted);
        self.output_changes.clear();
        self.events.clear();
        let (from, to) = (offset as u32, (offset + frames) as u32);
        for e in events.iter().filter(|e| e.offset >= from && e.offset < to) {
            let at = e.offset - from;
            let mapped = match e.msg {
                InstMsg::Control { cc, value } => Some((usize::from(cc), value)),
                InstMsg::Bend(value) => Some((PITCH_BEND, value)),
                _ => None,
            };
            match mapped {
                Some((controller, value)) => {
                    if let Some(id) = self.controllers.get(controller).copied().flatten() {
                        let _ = self.input_changes.add_point(id, at as i32, f64::from(value.clamp(0.0, 1.0)));
                    }
                }
                None => {
                    if let Some(event) = note_event(at, e.msg) {
                        if !self.events.push(event) {
                            self.shared.counters.events_dropped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }

        let mut context = process_context(self.sample_rate, self.continuous_samples, ctx, offset);
        let mut data = ProcessData {
            processMode: process_mode(self.offline),
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: frames as i32,
            numInputs: self.io.in_buses.len() as i32,
            numOutputs: self.io.out_buses.len() as i32,
            inputs: self.io.in_buses.as_mut_ptr(),
            outputs: self.io.out_buses.as_mut_ptr(),
            inputParameterChanges: self
                .input_changes
                .as_com_ref::<IParameterChanges>()
                .map_or(std::ptr::null_mut(), |changes| changes.as_ptr()),
            outputParameterChanges: self
                .output_changes
                .as_com_ref::<IParameterChanges>()
                .map_or(std::ptr::null_mut(), |changes| changes.as_ptr()),
            inputEvents: self.events.as_com_ref::<IEventList>().map_or(std::ptr::null_mut(), |events| events.as_ptr()),
            outputEvents: std::ptr::null_mut(),
            processContext: &mut context,
        };
        // SAFETY: as in `Vst3Effect::run_block`: active and processing, every pointer is to a
        // live local or this instrument's own buffers, `frames <= max_block`.
        // as in `Vst3Effect::run_block`: a plugin that skips writing must not replay the last block
        self.io.main_out[0][..frames].fill(0.0);
        self.io.main_out[1][..frames].fill(0.0);
        let result = unsafe { self.processor.process(&mut data) };
        self.continuous_samples += frames as i64;
        if result == kResultOk {
            l.copy_from_slice(&self.io.main_out[0][..frames]);
            let stereo = self.io.out_channels.first().is_some_and(|channels| *channels >= 2);
            r.copy_from_slice(&self.io.main_out[usize::from(stereo)][..frames]);
        } else {
            l.fill(0.0);
            r.fill(0.0);
            self.shared.counters.process_errors.fetch_add(1, Ordering::Relaxed);
        }
        let shared = &self.shared;
        self.output_changes.for_each_last(|id, value| {
            if !shared.feedback.post_listed(id, value) {
                shared.counters.feedback_dropped.fetch_add(1, Ordering::Relaxed);
            }
        });
    }
}

impl Instrument for Vst3Instrument {
    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        let sample_rate = f64::from(sample_rate);
        if (sample_rate - self.sample_rate).abs() < 1e-6 && max_block <= self.max_block && !self.failed {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let activated = main_thread::run("being re-activated", move || {
            let mut guard = shared.instance.lock();
            let instance = guard.as_mut().ok_or(Vst3Error::Closed)?;
            let layout = instance.activate(sample_rate, max_block)?;
            shared.latency.store(instance.latency(), Ordering::Release);
            Ok(layout)
        })
        .and_then(|result| result);
        match activated {
            Ok(layout) => {
                self.io = Io::new(&layout, max_block);
                self.sample_rate = sample_rate;
                self.max_block = max_block;
                self.failed = false;
            }
            Err(err) => {
                log::error!(
                    "VST3 instrument '{}' could not be re-activated at {sample_rate} Hz / {max_block}: {err}; it is silent",
                    self.shared.info.name
                );
                self.failed = true;
            }
        }
    }

    fn process(&mut self, events: &[InstEvent], l: &mut [f32], r: &mut [f32], ctx: &ProcessCtx) {
        let frames = l.len().min(r.len());
        if self.failed || self.max_block == 0 {
            l[..frames].fill(0.0);
            r[..frames].fill(0.0);
            return;
        }
        let mut done = 0;
        while done < frames {
            let chunk = (frames - done).min(self.max_block);
            let (l_chunk, r_chunk) = (&mut l[done..done + chunk], &mut r[done..done + chunk]);
            self.run_block(events, l_chunk, r_chunk, ctx, done);
            done += chunk;
        }
    }
}

impl std::fmt::Debug for Vst3Instrument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst3Instrument")
            .field("plugin", &self.shared.info.name)
            .field("sample_rate", &self.sample_rate)
            .field("max_block", &self.max_block)
            .field("offline", &self.offline)
            .field("failed", &self.failed)
            .finish()
    }
}

/// Load the instrument `cid` from the module at `path`, restore `state` and activate it for
/// `sample_rate` and blocks of at most `max_block` frames; `offline` activates it in VST3's
/// offline processing mode (a render's own instance). Same threading and blocking as
/// [`instantiate`]; the [`Vst3Handle`] gives its parameters, state and editor.
///
/// # Errors
/// As [`instantiate`], with [`Vst3Error::NotAnInstrument`] for a plugin without an event
/// input or a mono/stereo output.
pub fn instantiate_instrument(
    path: &str,
    cid: &str,
    state: Option<&[u8]>,
    sample_rate: f32,
    max_block: usize,
    offline: bool,
) -> Result<(Box<dyn Instrument>, Vst3Handle), Vst3Error> {
    let parts = create_parts(path, cid, state, sample_rate, max_block, Role::Instrument, offline)?;
    let instrument = Vst3Instrument {
        processor: parts.processor,
        io: Io::new(&parts.layout, max_block),
        input_changes: ParameterChanges::new(),
        output_changes: ParameterChanges::new(),
        events: NoteEventList::new(MAX_INST_EVENTS),
        unlisted: parts.unlisted,
        controllers: parts.controllers,
        sample_rate: parts.sample_rate,
        max_block,
        continuous_samples: 0,
        offline,
        failed: false,
        shared: Arc::clone(&parts.shared),
    };
    Ok((Box::new(instrument), Vst3Handle { shared: parts.shared }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo() -> BusLayout {
        BusLayout { inputs: vec![2], outputs: vec![2], event_inputs: 0 }
    }

    #[test]
    fn mono_or_stereo_main_buses_make_an_insert_effect() {
        assert!(stereo().check_stereo_effect().is_ok());
        let sidechain = BusLayout { inputs: vec![2, 2], outputs: vec![2], event_inputs: 0 };
        assert!(sidechain.check_stereo_effect().is_ok(), "a side-chain bus is fed silence");

        let instrument = BusLayout { inputs: vec![], outputs: vec![2], event_inputs: 0 };
        let err = instrument.check_stereo_effect().unwrap_err();
        assert!(err.contains("no audio input"), "{err}");
        for (i, o) in [(1, 1), (1, 2), (2, 1)] {
            let mono = BusLayout { inputs: vec![i], outputs: vec![o], event_inputs: 0 };
            assert!(mono.check_stereo_effect().is_ok(), "{i} in, {o} out: a mono bus is wired to both sides");
        }
        let surround = BusLayout { inputs: vec![6], outputs: vec![6], event_inputs: 0 };
        assert!(surround.check_stereo_effect().unwrap_err().contains("6 channel"));
        let sink = BusLayout { inputs: vec![2], outputs: vec![], event_inputs: 0 };
        assert!(sink.check_stereo_effect().is_err());
        let absurd = BusLayout { inputs: vec![2; MAX_BUSES + 1], outputs: vec![2], event_inputs: 0 };
        assert!(absurd.check_stereo_effect().is_err());
    }

    #[test]
    fn an_instrument_needs_an_event_input_and_a_mono_or_stereo_output() {
        let synth = BusLayout { inputs: vec![], outputs: vec![2], event_inputs: 1 };
        assert!(synth.check(Role::Instrument).is_ok());
        assert!(synth.check(Role::Effect).is_err(), "and it is not an insert effect");
        let mono = BusLayout { inputs: vec![], outputs: vec![1], event_inputs: 1 };
        assert!(mono.check_instrument().is_ok(), "a mono output is copied to both sides");
        let sidechained = BusLayout { inputs: vec![2], outputs: vec![2, 2, 2], event_inputs: 2 };
        assert!(sidechained.check_instrument().is_ok());
        let deaf = BusLayout { inputs: vec![2], outputs: vec![2], event_inputs: 0 };
        assert!(deaf.check_instrument().unwrap_err().contains("no notes"));
        let mute = BusLayout { inputs: vec![], outputs: vec![], event_inputs: 1 };
        assert!(mute.check_instrument().unwrap_err().contains("no audio output"));
        let surround = BusLayout { inputs: vec![], outputs: vec![6], event_inputs: 1 };
        assert!(surround.check_instrument().unwrap_err().contains("6 channels"));
    }

    #[test]
    fn notes_become_vst3_events_with_their_offset_and_id() {
        let on = note_event(17, InstMsg::NoteOn { key: 64, vel: 0.5, id: 9 }).unwrap();
        assert_eq!((on.sampleOffset, on.r#type, on.busIndex), (17, EventTypes_::kNoteOnEvent as u16, 0));
        // SAFETY: the variant written for this event type.
        let n = unsafe { on.__field0.noteOn };
        assert_eq!((n.pitch, n.velocity, n.noteId, n.channel), (64, 0.5, 9, 0));
        let off = note_event(3, InstMsg::NoteOff { key: 64, id: 9 }).unwrap();
        assert_eq!(off.r#type, EventTypes_::kNoteOffEvent as u16);
        // SAFETY: as above.
        assert_eq!(unsafe { off.__field0.noteOff }.noteId, 9);
        assert!(note_event(0, InstMsg::Control { cc: 64, value: 1.0 }).is_none(), "controllers go as parameters");
        assert!(note_event(0, InstMsg::Bend(0.5)).is_none());
    }

    #[test]
    fn instantiating_an_instrument_from_garbage_is_a_typed_error() {
        let err = instantiate_instrument("/nowhere/x.vst3", "zz", None, 48_000.0, 256, false).err().expect("must fail");
        assert!(matches!(err, Vst3Error::InvalidClassId(_)), "got {err:?}");
    }

    #[test]
    fn io_wires_the_main_pair_to_real_buffers_and_extras_to_silence_and_a_sink() {
        let layout = BusLayout { inputs: vec![2, 1], outputs: vec![2, 2], event_inputs: 0 };
        let mut io = Io::new(&layout, 64);
        io.wire();
        assert_eq!(io.in_buses.len(), 2);
        assert_eq!(io.out_buses.len(), 2);
        // SAFETY: `wire` pointed every bus at `io`'s own pointer arrays.
        unsafe {
            let main_in = io.in_buses[0].__field0.channelBuffers32;
            assert_eq!(*main_in, io.main_in[0].as_mut_ptr());
            assert_eq!(*main_in.add(1), io.main_in[1].as_mut_ptr());
            assert_eq!(*io.in_buses[1].__field0.channelBuffers32, io.silence.as_mut_ptr());
            let main_out = io.out_buses[0].__field0.channelBuffers32;
            assert_eq!(*main_out.add(1), io.main_out[1].as_mut_ptr());
            let extra_out = io.out_buses[1].__field0.channelBuffers32;
            assert_eq!(*extra_out, io.sink.as_mut_ptr());
            assert_eq!(*extra_out.add(1), io.sink.as_mut_ptr());
        }
        assert!(io.has_extra_inputs());
    }

    #[test]
    fn instantiating_garbage_is_a_typed_error_not_a_panic() {
        let err = instantiate("/nowhere/x.vst3", "not-a-cid", None, 48_000.0, 256).err().expect("must fail");
        assert!(matches!(err, Vst3Error::InvalidClassId(_)), "got {err:?}");

        let dir = crate::module::scratch_dir("instantiate");
        let bogus = dir.join("garbage.vst3");
        std::fs::write(&bogus, b"not a library").unwrap();
        let err = instantiate(bogus.to_str().unwrap(), &"00".repeat(16), None, 48_000.0, 256).err().expect("must fail");
        assert!(matches!(err, Vst3Error::Load { .. } | Vst3Error::UnsupportedPlatform), "got {err:?}");
    }

    #[test]
    fn param_info_serialises_in_the_shape_the_ui_expects() {
        let info = ParamInfo { id: 3, name: "Gain".to_owned(), value: 0.5, display: "-6.0 dB".to_owned(), def: 1.0 };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json, serde_json::json!({"id": 3, "name": "Gain", "value": 0.5, "display": "-6.0 dB", "def": 1.0}));
    }
}
