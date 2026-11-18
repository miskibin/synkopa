//! The non-real-time side of the engine: owns the project, undo history and the
//! loaded audio, turns edits into ready-built structures for the audio thread,
//! finalises recordings, and publishes telemetry.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::analysis::Analyzer;
use crate::catalog::param_block;
use crate::errors::{ERR_INVALID_TARGET, ERR_REVISION_CONFLICT};
use crate::midi::{self, MidiMsg, MidiRecorder};
use crate::model::*;
use crate::ops::{self, Dirty, Op};
use crate::processor::{ParamBlock, Processor};
use crate::rt::*;
use crate::sources::{write_wav, Audio, SourceStore};

/// Creates processors the engine cannot build itself (hosted plugins).
pub trait PluginFactory: Send + Sync {
    /// `live` is false for offline renders: the factory must not replace the live instance's handle.
    fn create(&self, fx_id: Id, plugin: &PluginRef, sample_rate: f32, max_block: usize, live: bool) -> Result<Box<dyn Processor>, String>;
    /// A plugin instrument for channel `chan_id`, prepared for `sample_rate` / `max_block`.
    /// `live` as for [`PluginFactory::create`] (false: an offline render's own instance, in
    /// offline processing mode). [`PluginFactory::release`] and [`PluginFactory::state`] take
    /// the channel id for it (ids are unique across effects and channels).
    fn create_instrument(
        &self,
        _chan_id: Id,
        plugin: &PluginRef,
        _sample_rate: f32,
        _max_block: usize,
        _live: bool,
    ) -> Result<Box<dyn crate::processor::Instrument>, String> {
        Err(format!("{} cannot be loaded: this build has no plugin host", plugin.name))
    }
    /// Prepare everything about these plugins that may be slow and needs no instance (Synkopa's
    /// safety check of a plugin used for the first time), so the `create` calls the engine
    /// makes under its lock are quick. Called without the engine lock, before an edit or a
    /// project load that adds them; `instrument` per plugin. Default: nothing.
    fn vet(&self, _plugins: &[(&PluginRef, bool)]) {}
    /// The effect left the project (or the engine is rebuilding everything).
    fn release(&self, _fx_id: Id) {}
    /// Current opaque state of a live instance (base64), for saving.
    fn state(&self, _fx_id: Id) -> Option<String> {
        None
    }
    /// Whether hosted plugin `fx_id` currently lists parameter `param_id`.
    /// Automation targets (`AutoTarget::Plugin`) are validated against this at
    /// `apply` time, so a stale or unknown id is an error, not a dead envelope.
    fn has_plugin_param(&self, _fx_id: Id, _param_id: u32) -> bool {
        false
    }
    /// The latency a live instance reports now, when it may have changed since it was
    /// created (a plugin switching to a look-ahead mode). `None`: unknown or unchanged
    /// is the processor's own [`Processor::latency_samples`]. Called without the engine
    /// lock held, from the engine's housekeeping thread (about twice a second).
    fn latency(&self, _fx_id: Id) -> Option<usize> {
        None
    }
}

pub struct NoPlugins;
impl PluginFactory for NoPlugins {
    fn create(&self, _: Id, p: &PluginRef, _: f32, _: usize, _: bool) -> Result<Box<dyn Processor>, String> {
        Err(format!("{} cannot be loaded: this build has no plugin host", p.name))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransportDto {
    pub playing: bool,
    pub rec: bool,
    pub monitor: bool,
    pub metro: bool,
    pub count_in: bool,
    pub start_beat: f64,
    /// The recording filter (FL's right-click on Record): the input's audio becomes takes.
    pub rec_audio: bool,
    /// The recording filter: notes played on a MIDI keyboard become a pattern.
    pub rec_notes: bool,
    /// Input quantize of recorded notes: their starts move to multiples of this many beats
    /// (lengths kept); 0 = as played.
    pub rec_quantize: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDto {
    pub project: Project,
    pub revision: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub transport: TransportDto,
    pub path: Option<String>,
    pub dirty: bool,
    /// Effects that failed to load (plugin missing etc.): fx id → message.
    pub fx_errors: HashMap<Id, String>,
    /// Comp id → take being auditioned (played instead of the comp). Live only.
    pub audition: HashMap<Id, Id>,
    /// Audio clips whose time-stretch is being rendered; they play resampled until it is.
    pub stretching: Vec<Id>,
    /// Audio clips whose time-stretch failed (they play resampled): clip id → message.
    pub stretch_errors: HashMap<Id, String>,
    /// Sources whose audio is not loaded (file moved, deleted, unreadable; they play
    /// silence): source id → why. Relink them with [`Engine::relink_sources`].
    pub missing: HashMap<Id, String>,
}

/// A source to point at another file ([`Engine::relink_sources`]).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relink {
    pub source_id: Id,
    pub path: String,
}

/// How one relink went.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Relinked {
    pub source_id: Id,
    pub path: String,
    /// `None` = done; otherwise why not (the source is unchanged).
    pub error: Option<String>,
    /// Done, but the file's length differs from the one the project remembers.
    pub length_differs: bool,
}

/// What [`Engine::relink_search`] did.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkSearch {
    /// Sources relinked (one undo step).
    pub relinked: Vec<Relinked>,
    /// Missing sources the user has to choose for: id → files, best first (several files of
    /// that name, only a converted one, or a different length).
    pub candidates: HashMap<Id, Vec<crate::relink::Candidate>>,
    /// Missing sources nothing was found for.
    pub not_found: Vec<Id>,
    /// Directory entries looked at.
    pub scanned: usize,
    /// The search stopped at its limit before seeing the whole folder.
    pub truncated: bool,
}

/// What `Engine::make_unique_audio` made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MadeUnique {
    pub source_id: Id,
    /// Index of the new track (right below the clips' old one).
    pub track: u32,
    /// The insert the new track plays into: the old track's (the master unless routed).
    pub insert_id: Id,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Op(#[from] ops::OpError),
    #[error("{0}")]
    Msg(String),
    /// Offline render was aborted by the caller before a file was written.
    #[error("cancelled")]
    Cancelled,
}

#[derive(Clone, Copy, Debug)]
pub enum RenderTail {
    Seconds(f64),
    /// Let one full pass initialize effects, then keep a second pass of the same length.
    Wrap,
}

/// Progress of an offline render, reported between DSP blocks (on the rendering thread).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderProgress {
    /// Time-stretching clips nobody has played yet: `done` of `total` finished.
    Preparing { done: usize, total: usize },
    /// `done` of `total` frames rendered (a wrapped loop's warm-up pass and the delay
    /// compensation count too); `peak` is the largest |sample| since the previous report and
    /// `beat` the song position just rendered (past `to` while the tail rings out).
    Rendering { done: usize, total: usize, peak: f32, beat: f64 },
}

impl From<String> for EngineError {
    fn from(s: String) -> Self {
        EngineError::Msg(s)
    }
}

type Listener = Box<dyn Fn(&StateDto) + Send + Sync>;
type FrameSink = Box<dyn Fn(Vec<u8>) + Send + Sync>;

/// Input being recorded. With loop recording it holds several passes; `marks` are the
/// frames where each later pass begins (the audio thread sends a NaN frame at the wrap).
struct RecBuf {
    track: usize,
    /// Beat where the first pass starts (NaN until the audio thread has punched in).
    start: f64,
    /// Frames of input to drop from the front so the take lines up with what the
    /// performer heard: device round trip + plugin delay compensation of the playback.
    /// Set at the start from the device estimate then; replaced in [`RecBuf::resolve`] by the
    /// mean round trip measured while the take was capturing, when there is one.
    skip: usize,
    /// The mixer's playback latency `skip` was taken with: the graph keeps it until the take ends.
    pdc: usize,
    /// Loop range when recording in passes.
    lap: Option<(f64, f64)>,
    sr: u32,
    data: Vec<f32>,
    marks: Vec<usize>,
    nans: u32,
    /// Peaks of the current pass only (the live clip restarts every pass).
    peaks: Vec<f32>,
    peaks_sent: usize,
    acc: (f32, f32, usize),
}

impl RecBuf {
    fn frames(&self) -> usize {
        self.data.len() / 2
    }
    fn pass_start_frame(&self) -> usize {
        self.marks.last().copied().unwrap_or(0)
    }
    fn pass_start_beat(&self) -> f64 {
        match (self.lap, self.marks.is_empty()) {
            (Some((a, _)), false) => a,
            _ => self.start,
        }
    }
    /// Reads what the audio thread learned during the take (before its `Shared` goes away):
    /// a punch-in take starts where capturing began, and the device round trip is the one
    /// measured while it captured (the estimate at the start may not have settled).
    fn resolve(&mut self, shared: &Shared) {
        if self.start.is_nan() {
            self.start = shared.rec_start();
        }
        if let Some(io) = shared.take_latency() {
            self.skip = io as usize + self.pdc;
        }
    }
    fn push(&mut self, x: f32) {
        if x.is_nan() {
            self.nans += 1;
            if self.nans.is_multiple_of(2) {
                self.marks.push(self.frames());
                self.peaks.clear();
                self.peaks_sent = 0;
                self.acc = (0.0, 0.0, 0);
            }
            return;
        }
        self.data.push(x);
        if self.data.len() % 2 == 1 {
            return;
        }
        let f = self.frames() - 1;
        let v = (self.data[f * 2] + self.data[f * 2 + 1]) * 0.5;
        self.acc.0 = self.acc.0.min(v);
        self.acc.1 = self.acc.1.max(v);
        self.acc.2 += 1;
        if self.acc.2.is_multiple_of((self.sr as usize / 100).max(1)) {
            self.peaks.push(self.acc.0);
            self.peaks.push(self.acc.1);
            self.acc.0 = 0.0;
            self.acc.1 = 0.0;
        }
    }
}

struct Wires {
    cmds: rtrb::Producer<Cmd>,
    garbage: rtrb::Consumer<Garbage>,
    record: rtrb::Consumer<f32>,
    analyzer: Analyzer,
    sr: f32,
    max_block: usize,
}

struct Core {
    project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    revision: u64,
    gesture: Option<Project>,
    /// A transient edit inside the open gesture was not reported to listeners yet.
    gesture_unreported: bool,
    /// A file imported on the way to another edit (`import_audio_with`, `step` false): the
    /// project before it, and the revision the import left. The next undo step taken while the
    /// project is still at that revision starts from here, so undoing that edit takes the
    /// imported source back out too.
    import_base: Option<(u64, Project)>,
    sources: SourceStore,
    path: Option<PathBuf>,
    saved_revision: u64,
    transport: TransportDto,
    /// Effect ids the audio thread currently holds a processor for.
    live_fx: HashSet<Id>,
    sent_params: HashMap<Id, (Option<ParamBlock>, f32)>,
    /// Warp maps the audio thread has, by effect id (a change is sent as `Cmd::FxData`).
    sent_maps: HashMap<Id, crate::model::WarpMaps>,
    fx_errors: HashMap<Id, String>,
    /// Latency each live hosted plugin reported, by effect id (plugin delay compensation).
    plugin_lat: HashMap<Id, usize>,
    /// Why a source's audio did not load when the project was opened (by source id).
    source_errors: HashMap<Id, String>,
    wires: Option<Wires>,
    take: Option<RecBuf>,
    /// The pattern the loop preview plays and the part of it that loops (PAT mode).
    pat_loop: Option<(Id, Option<(f64, f64)>)>,
    /// The track a recording added because no track had room (removed again if nothing
    /// lands on it, and left out of the undo snapshots taken while it was empty).
    rec_added: Option<Id>,
    /// A mixer change that would alter the playback latency arrived during a take: the
    /// audio thread keeps the old graph until the take ends (its `skip` stays right).
    graph_deferred: bool,
    watch: Vec<Id>,
    /// Comp id → take played instead of the comp (live only: not saved, not undone).
    audition: HashMap<Id, Id>,
    midi: MidiState,
    /// Synth channels the audio thread holds an instance for, with the patch it was last sent.
    synths: HashMap<Id, crate::poly::Patch>,
    /// Plugin-instrument channels an instance was made for (or failed to be: the error is in
    /// `fx_errors`), with the plugin it hosts. A failed one is retried when its plugin changes
    /// or the audio device is rebuilt, not on every edit.
    plugs: HashMap<Id, PluginRef>,
    /// The Synkopa Keys piano, once the app has loaded it.
    keys: Option<Arc<crate::keys::KeysBank>>,
}

/// Live MIDI input state kept under the core lock (see [`Engine::midi_configure`]).
#[derive(Default)]
struct MidiState {
    /// Channel the keyboard plays; `None` = the first pitched channel.
    target: Option<Id>,
    /// A device is connected: notes play, and an armed recording captures them.
    live: bool,
}

/// The MIDI take being recorded; its own lock, so keyboard events never wait for the core.
#[derive(Default)]
struct MidiTake {
    /// The take being played in: recorder, armed track index, target channel.
    rec: Option<(MidiRecorder, usize, Id)>,
    /// The running take received notes (then its audio take is dropped).
    played: bool,
    /// Set when the take records into the looping pattern (PAT mode) instead of the song.
    pat: Option<PatLoopRec>,
}

/// A MIDI take over the pattern loop: its notes are added to pattern `id` (FL's pattern-mode
/// recording, overdubbing what the pattern holds).
struct PatLoopRec {
    id: Id,
    /// The loop: from beat `la` of the pattern, `span` beats long.
    la: f64,
    span: f64,
    /// The last pattern beat seen, and the beats of the passes completed before it.
    prev: f64,
    laps: f64,
}

impl PatLoopRec {
    /// Pattern beat `b` counted on from the first pass (a jump back is the loop wrapping);
    /// a stopped loop (`b` < 0) stays where it was.
    fn unwrap(&mut self, b: f64) -> f64 {
        if b < 0.0 {
            return self.prev.max(self.la) + self.laps;
        }
        if b + 1e-9 < self.prev {
            self.laps += self.span;
        }
        self.prev = b;
        b + self.laps
    }
}

impl MidiTake {
    /// The beat a keyboard event lands on: the song's, or the pattern loop's while recording into it.
    fn beat(&mut self, song: f64, pat: f64) -> f64 {
        match self.pat.as_mut() {
            Some(p) => p.unwrap(pat),
            None => song,
        }
    }
}

/// What a keyboard event needs, readable without the core lock: whether a device is live,
/// the channel and insert the keys play (refreshed whenever the project changes) and the
/// audio thread's MIDI ring (separate from the command ring, so it has its own producer).
struct MidiFast {
    live: AtomicBool,
    chan: std::sync::atomic::AtomicU64,
    ins: std::sync::atomic::AtomicUsize,
    tx: Mutex<Option<rtrb::Producer<Cmd>>>,
}

impl MidiFast {
    fn send(&self, cmd: Cmd) {
        if let Some(tx) = self.tx.lock().as_mut() {
            if tx.push(cmd).is_err() {
                log::warn!("audio MIDI queue full; dropping an event");
            }
        }
    }
}

pub struct Engine {
    core: Mutex<Core>,
    shared: Mutex<Arc<Shared>>,
    factory: Arc<dyn PluginFactory>,
    listeners: Mutex<Vec<Listener>>,
    sink: Mutex<Option<FrameSink>>,
    /// Effect chains that are silent past their tails are not processed (on by default).
    skip_silent: std::sync::atomic::AtomicBool,
    midi_fast: MidiFast,
    midi_take: Mutex<MidiTake>,
    /// Serializes transport changes while the callback drains a stopped take.
    transport_control: Mutex<()>,
    /// Threads an offline render may use for the insert chains.
    render_workers: std::sync::atomic::AtomicUsize,
}

const UNDO_LIMIT: usize = 200;

impl Engine {
    pub fn new(mut project: Project, factory: Arc<dyn PluginFactory>) -> Arc<Self> {
        project.migrate();
        let mut sources = SourceStore::default();
        load_library_sources(&project, &mut sources);
        let e = Arc::new(Self {
            core: Mutex::new(Core {
                project,
                undo: vec![],
                redo: vec![],
                revision: 1,
                gesture: None,
                gesture_unreported: false,
                import_base: None,
                sources,
                path: None,
                saved_revision: 1,
                transport: TransportDto {
                    playing: false,
                    rec: false,
                    monitor: false,
                    metro: false,
                    count_in: true,
                    start_beat: 0.0,
                    rec_audio: true,
                    rec_notes: true,
                    rec_quantize: 0.0,
                },
                live_fx: HashSet::new(),
                sent_params: HashMap::new(),
                sent_maps: HashMap::new(),
                fx_errors: HashMap::new(),
                plugin_lat: HashMap::new(),
                source_errors: HashMap::new(),
                wires: None,
                take: None,
                pat_loop: None,
                rec_added: None,
                graph_deferred: false,
                watch: vec![],
                audition: HashMap::new(),
                midi: MidiState::default(),
                synths: HashMap::new(),
                plugs: HashMap::new(),
                keys: None,
            }),
            shared: Mutex::new(Arc::new(Shared::new())),
            factory,
            listeners: Mutex::new(vec![]),
            sink: Mutex::new(None),
            skip_silent: std::sync::atomic::AtomicBool::new(true),
            midi_fast: MidiFast {
                live: AtomicBool::new(false),
                chan: std::sync::atomic::AtomicU64::new(u64::MAX),
                ins: std::sync::atomic::AtomicUsize::new(usize::MAX),
                tx: Mutex::new(None),
            },
            midi_take: Mutex::new(MidiTake::default()),
            transport_control: Mutex::new(()),
            render_workers: std::sync::atomic::AtomicUsize::new(
                std::thread::available_parallelism().map(|n| n.get().saturating_sub(2).clamp(1, 8)).unwrap_or(1),
            ),
        });
        let weak = Arc::downgrade(&e);
        thread::Builder::new()
            .name("wave-housekeeping".into())
            .spawn(move || {
                // fixed 60 Hz cadence (sleeping a fixed time *after* the work made frames uneven)
                let period = Duration::from_micros(16_667);
                let mut next = std::time::Instant::now();
                let mut tick = 0u32;
                while let Some(e) = weak.upgrade() {
                    e.housekeeping(tick);
                    drop(e);
                    tick = tick.wrapping_add(1);
                    next += period;
                    let now = std::time::Instant::now();
                    if next > now {
                        thread::sleep(next - now);
                    } else {
                        next = now;
                    }
                }
            })
            .expect("spawn housekeeping thread");
        e
    }

    /// Turns silence skipping of effect chains on/off for Rts created from now on and for
    /// offline renders. The output is the same (below -120 dB); off is for A/B tests.
    pub fn set_silence_skip(&self, on: bool) {
        self.skip_silent.store(on, Relaxed);
    }

    /// Threads the synth instances and insert chains may use, in offline renders and in the
    /// streams (`create_rt`) made from now on (1 = single-threaded). The default leaves two
    /// cores to the rest of the machine, at most 8. The output is identical whatever the count.
    pub fn set_render_workers(&self, n: usize) {
        self.render_workers.store(n.max(1), Relaxed);
    }

    // ------------------------------------------------------------ device attach

    /// Builds the audio-thread half for a device running at `sample_rate` with
    /// blocks of at most `max_block`. Any previous `Rt` becomes inert: drop it
    /// (off the audio thread) once its stream has stopped.
    pub fn create_rt(&self, sample_rate: f32, max_block: usize) -> Rt {
        let max_block = max_block.clamp(16, 8192);
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::<Cmd>::new(4096);
        let (midi_tx, midi_rx) = rtrb::RingBuffer::<Cmd>::new(1024);
        let (gb_tx, gb_rx) = rtrb::RingBuffer::<Garbage>::new(4096);
        let (rec_tx, rec_rx) = rtrb::RingBuffer::<f32>::new(sample_rate as usize * 2 * 20);
        let mut inputs = vec![];
        let mut outputs = vec![];
        let mut ain = vec![];
        let mut aout = vec![];
        for _ in 0..WATCH_SLOTS {
            let (a, b) = rtrb::RingBuffer::<f32>::new(TAP_CAPACITY);
            inputs.push(a);
            ain.push(b);
            let (a, b) = rtrb::RingBuffer::<f32>::new(TAP_CAPACITY);
            outputs.push(a);
            aout.push(b);
        }
        let shared = Arc::new(Shared::new());
        {
            // a punched-in take learns its start beat from the stream that is going away
            let mut c = self.core.lock();
            let old = self.shared.lock().clone();
            if let Some(t) = c.take.as_mut() {
                t.resolve(&old);
            }
        }
        // ...and a MIDI take (keyboard or computer keyboard) ends at its playhead
        self.finish_midi_take();
        *self.shared.lock() = shared.clone();
        let mut rt = Rt::new(sample_rate, max_block, cmd_rx, gb_tx, shared, Taps { input: inputs, output: outputs }, rec_tx);
        rt.set_skip_silent(self.skip_silent.load(Relaxed));
        // synth instances and insert chains spread over several cores while playing too
        rt.set_workers(self.render_workers.load(Relaxed));
        rt.set_midi_ring(midi_rx);
        *self.midi_fast.tx.lock() = Some(midi_tx);
        let mut c = self.core.lock();
        // everything is rebuilt for the new rate: hosted plugins keep their state through the project
        self.snapshot_plugin_states(&mut c.project);
        for id in c.live_fx.drain().collect::<Vec<_>>() {
            self.factory.release(id);
        }
        c.plugin_lat.clear();
        c.sent_params.clear();
        c.sent_maps.clear();
        c.synths.clear();
        self.forget_plugs(&mut c);
        // the old stream is gone: keep what it captured instead of orphaning the take
        self.finalize_take(&mut c);
        c.wires = Some(Wires {
            cmds: cmd_tx,
            garbage: gb_rx,
            record: rec_rx,
            analyzer: Analyzer::new(ain, aout, sample_rate),
            sr: sample_rate,
            max_block,
        });
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        let flags = Cmd::Flags { metro: c.transport.metro, monitor: c.transport.monitor };
        let watch = watch_cmd(&c.watch);
        send(&mut c, flags);
        send(&mut c, watch);
        c.transport.playing = false;
        c.pat_loop = None; // the new stream plays nothing
        self.settle_rec_track(&mut c);
        self.changed(c);
        rt
    }

    pub fn shared(&self) -> Arc<Shared> {
        self.shared.lock().clone()
    }

    // ------------------------------------------------------------ state & listeners

    pub fn on_change(&self, f: impl Fn(&StateDto) + Send + Sync + 'static) {
        self.listeners.lock().push(Box::new(f));
    }

    pub fn set_frame_sink(&self, f: Option<FrameSink>) {
        *self.sink.lock() = f;
    }

    fn dto(&self, c: &Core) -> StateDto {
        let (stretching, stretch_errors) = c.sources.stretch_status(&c.project.clips);
        StateDto {
            project: c.project.clone(),
            revision: c.revision,
            can_undo: !c.undo.is_empty(),
            can_redo: !c.redo.is_empty(),
            transport: c.transport.clone(),
            path: c.path.as_ref().map(|p| p.display().to_string()),
            dirty: c.revision != c.saved_revision,
            fx_errors: c.fx_errors.clone(),
            audition: live_audition(c),
            stretching,
            stretch_errors,
            missing: missing_sources(c),
        }
    }

    pub fn state(&self) -> StateDto {
        let c = self.core.lock();
        self.dto(&c)
    }

    pub fn project(&self) -> Project {
        self.core.lock().project.clone()
    }

    /// The project with every VST3 slot's state read from its running plugin, as a save writes it.
    pub fn project_with_plugin_states(&self) -> Project {
        let mut project = self.project();
        self.snapshot_plugin_states(&mut project);
        project
    }

    pub fn revision(&self) -> u64 {
        self.core.lock().revision
    }

    fn notify(&self, dto: StateDto) {
        for l in self.listeners.lock().iter() {
            l(&dto);
        }
    }

    fn changed(&self, c: parking_lot::MutexGuard<'_, Core>) {
        let dto = self.dto(&c);
        drop(c);
        self.notify(dto);
    }

    // ------------------------------------------------------------ edits

    /// Applies a batch. `transient` edits (inside a gesture) do not create undo steps.
    /// With `if_revision`, fails if the project changed since that revision.
    pub fn apply(&self, ops: &[Op], transient: bool, if_revision: Option<u64>) -> Result<u64, EngineError> {
        let added: Vec<(&PluginRef, bool)> = ops
            .iter()
            .filter_map(|op| match op {
                Op::PutFx { fx, .. } => fx.plugin.as_ref().map(|p| (p, false)),
                Op::PutChannel { channel, .. } => channel.plugin().map(|p| (p, true)),
                _ => None,
            })
            .collect();
        if !added.is_empty() {
            self.factory.vet(&added);
        }
        self.apply_locked(self.core.lock(), ops, transient, if_revision)
    }

    /// [`PluginFactory::vet`] for every hosted plugin of `p` (outside the lock).
    fn vet_project(&self, p: &Project) {
        let fx = p.inserts.iter().flat_map(|m| m.fx.iter()).filter_map(|f| f.plugin.as_ref().map(|p| (p, false)));
        let inst = p.channels.iter().filter_map(|c| c.plugin().map(|p| (p, true)));
        let all: Vec<(&PluginRef, bool)> = fx.chain(inst).collect();
        if !all.is_empty() {
            self.factory.vet(&all);
        }
    }

    fn apply_locked(
        &self,
        mut c: parking_lot::MutexGuard<'_, Core>,
        ops: &[Op],
        transient: bool,
        if_revision: Option<u64>,
    ) -> Result<u64, EngineError> {
        if let Some(r) = if_revision {
            if r != c.revision {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        let before = c.project.clone();
        // plugin automation targets are validated here, not in `ops::apply_all`:
        // whether a parameter id exists is knowledge the plugin factory holds
        for op in ops {
            if let Op::PutAutomation { automation } = op {
                if let AutoTarget::Plugin { fx_id, param_id } = &automation.target {
                    let in_project = c.project.find_fx(*fx_id).is_some_and(|(_, f)| f.kind == FxKind::Plugin);
                    if !in_project || !self.factory.has_plugin_param(*fx_id, *param_id) {
                        return Err(EngineError::Msg(format!(
                            "{ERR_INVALID_TARGET} effect {fx_id} has no plugin parameter {param_id} to automate"
                        )));
                    }
                }
            }
        }
        let d = ops::apply_all(&mut c.project, ops)?;
        if c.project == before {
            return Ok(c.revision);
        }
        let in_gesture = transient && c.gesture.is_some();
        // Lane heights are how the Playlist is viewed, not what the song is: zooming the tracks
        // is not an undo step (Ctrl+Z after it must take back the last edit, not the zoom). The
        // height is saved with the song, so it is carried into the history instead: undoing an
        // older edit keeps the lanes as they are now.
        if only_track_heights(ops, &before, &c.project) {
            // only the lanes whose height changed, and only into the same track: a track's id stays
            // with its slot when lanes swap (Move up / down), so the history is walked step by step
            // from now, and the carry stops at the first step where the lane at that id became
            // another track (see `same_lane`: a rename keeps it the same track)
            let changed: Vec<Track> =
                c.project.tracks.iter().filter(|t| before.tracks.iter().any(|b| b.id == t.id && b.h != t.h)).cloned().collect();
            let core = &mut *c;
            for now in &changed {
                // undo: the newest snapshot is last; redo: the next state is last
                for stack in [&mut core.undo, &mut core.redo] {
                    carry_height(now, stack.iter_mut().rev());
                }
                carry_height(now, core.gesture.iter_mut());
            }
        } else if !in_gesture {
            push_undo(&mut c, before);
        }
        c.revision += 1;
        self.sync(&mut c, d);
        let rev = c.revision;
        // inside a gesture (a drag sends one batch per frame) listeners are not sent the whole
        // project each time: the UI holds those states back anyway, and `end_gesture` reports
        // the result. On a big song that was ~1 MB serialised, sent and parsed per frame.
        if in_gesture {
            c.gesture_unreported = true;
        } else {
            self.changed(c);
        }
        Ok(rev)
    }

    pub fn begin_gesture(&self) {
        let mut c = self.core.lock();
        if c.gesture.is_none() {
            c.gesture = Some(c.project.clone());
        }
    }

    pub fn end_gesture(&self) {
        let mut c = self.core.lock();
        let unreported = std::mem::take(&mut c.gesture_unreported);
        if let Some(g) = c.gesture.take() {
            if g != c.project {
                // the gesture's start predates any file imported during it: that start is the
                // state Ctrl+Z returns to, not the import's base (taken mid-gesture)
                c.import_base = None;
                push_undo(&mut c, g);
                // renders skipped during the drag (pitch edits) happen now
                self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
                self.changed(c);
            } else if unreported {
                // dragged and back: nothing to undo, but listeners have not seen the revisions
                self.changed(c);
            }
        }
    }

    pub fn undo(&self) -> bool {
        let mut c = self.core.lock();
        c.gesture = None;
        c.gesture_unreported = false;
        let Some(p) = c.undo.pop() else { return false };
        let cur = std::mem::replace(&mut c.project, p);
        c.redo.push(cur);
        let Core { sources, project, .. } = &mut *c;
        sources.reconcile(&project.sources);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        self.changed(c);
        true
    }

    /// Replaces the whole project with `project` as one undo step, without stopping the
    /// transport (`synkopa watch`: a script's result, made on a copy, applied in one go). Audio
    /// already loaded for a source of the same id and file is kept; a new source's file is
    /// decoded first, outside the lock (a file that does not decode makes the source missing,
    /// as on open). Hosted plugins of the same class keep their running instance and sound;
    /// effects keep their DSP state by id, as after an undo. `Ok(revision)`; a project equal to
    /// the current one changes nothing. Refused while recording, or when `if_revision` is not
    /// the current revision.
    pub fn set_project(&self, project: Project, if_revision: Option<u64>) -> Result<u64, EngineError> {
        let mut project = project;
        project.migrate();
        if project.inserts.first().map(|m| m.id) != Some(0) {
            return Err(EngineError::Msg("the project has no master insert".into()));
        }
        self.vet_project(&project);
        let have =
            |c: &Core, s: &Source| c.sources.get(s.id).is_some() && c.project.sources.iter().any(|o| o.id == s.id && o.path == s.path);
        let todo: Vec<(Id, String)> = {
            let c = self.core.lock();
            project.sources.iter().filter(|s| !have(&c, s)).map(|s| (s.id, s.path.clone())).collect()
        };
        let loaded: Vec<(Id, String, Result<Audio, String>)> =
            todo.into_iter().map(|(id, path)| (id, path.clone(), Audio::load_path(&path, Path::new(".")))).collect();
        let mut c = self.core.lock();
        if let Some(r) = if_revision {
            if r != c.revision {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        if c.take.is_some() || self.midi_take.lock().rec.is_some() {
            return Err(EngineError::Msg("recording: stop it first".into()));
        }
        if c.project == project {
            return Ok(c.revision);
        }
        for (id, path, r) in loaded {
            // the project may have changed while the files decoded: only what is still wanted
            if !project.sources.iter().any(|s| s.id == id && s.path == path) {
                continue;
            }
            match r {
                Ok(a) => {
                    c.sources.insert(id, Arc::new(a));
                    c.source_errors.remove(&id);
                }
                Err(e) => {
                    c.sources.drop_audio(id);
                    c.source_errors.insert(id, e);
                }
            }
        }
        c.gesture = None;
        c.gesture_unreported = false;
        let before = std::mem::replace(&mut c.project, project);
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        let rev = c.revision;
        self.changed(c);
        Ok(rev)
    }

    pub fn redo(&self) -> bool {
        let mut c = self.core.lock();
        c.gesture = None;
        c.gesture_unreported = false;
        let Some(p) = c.redo.pop() else { return false };
        let cur = std::mem::replace(&mut c.project, p);
        c.undo.push(cur);
        let Core { sources, project, .. } = &mut *c;
        sources.reconcile(&project.sources);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        self.changed(c);
        true
    }

    // ------------------------------------------------------------ transport

    pub fn play(&self, from: f64) {
        let control = self.transport_control.lock();
        let mut c = self.core.lock();
        // starting the song stops the pattern loop: notes recorded over it join their pattern
        let recorded = self.finish_pat_take(&mut c);
        if c.take.is_some() || self.midi_take.lock().rec.is_some() {
            drop(control);
            if recorded {
                self.changed(c);
            }
            return;
        }
        let from = from.max(0.0);
        let p = &c.project;
        let rec = c.transport.rec;
        let (ls, le) = p.loop_range.map(|l| (l.a, l.b)).unwrap_or((0.0, p.song_end()));
        // with a loop set, recording cycles over it: one take per pass
        let lap = p.loop_range.filter(|l| rec && l.b > l.a).map(|l| (l.a, l.b));
        let from = match lap {
            Some((a, b)) if from < a || from >= b => a,
            Some(_) => from,
            None if !rec && (from >= le || from < ls && p.loop_range.is_some()) => ls,
            None => from,
        };
        c.pat_loop = None; // the song and the pattern loop are exclusive
        let rec_track = rec.then(|| self.rec_target(&mut c, from, lap.map_or(f64::INFINITY, |l| l.1)));
        let count_in = if rec_track.is_some() && c.transport.count_in { 4.0 } else { 0.0 };
        if let Some(track) = rec_track {
            if c.transport.rec_audio {
                c.take = self.new_take(&c, track, from, lap);
                self.send_monitor(&mut c);
            }
            self.begin_midi_take(&mut c, track, from);
        }
        c.transport.start_beat = from;
        c.transport.playing = true;
        send(&mut c, Cmd::Play { from, count_in, rec: rec_track.is_some(), rec_loop: lap.is_some() });
        drop(control);
        self.changed(c);
    }

    pub fn stop(&self) {
        let control = self.transport_control.lock();
        let mut c = self.core.lock();
        let drain = self.end_take(&mut c, true);
        if drain.is_none() {
            send(&mut c, Cmd::Stop);
        }
        c.transport.playing = false;
        drop(c);
        if let Some((shared, timeout)) = drain {
            wait_for_take_input(&shared, timeout);
        }
        self.finish_take();
        self.finish_midi_take();
        let mut c = self.core.lock();
        self.settle_rec_track(&mut c);
        drop(control);
        self.changed(c);
    }

    /// The recording filter: what a recording captures (the input's audio, keyboard notes).
    /// At least one stays on; a recording running keeps what it started with.
    pub fn set_rec_filter(&self, audio: Option<bool>, notes: Option<bool>) -> TransportDto {
        let mut c = self.core.lock();
        let (a, n) = (audio.unwrap_or(c.transport.rec_audio), notes.unwrap_or(c.transport.rec_notes));
        if a || n {
            c.transport.rec_audio = a;
            c.transport.rec_notes = n;
        }
        let t = c.transport.clone();
        self.changed(c);
        t
    }

    /// Input quantize of recorded notes, in beats (0 = off; 1/4 = a step). A take running
    /// uses the value it ends with.
    pub fn set_rec_quantize(&self, grid: f64) -> TransportDto {
        let mut c = self.core.lock();
        if grid.is_finite() && (0.0..=4.0).contains(&grid) {
            c.transport.rec_quantize = grid;
        }
        let t = c.transport.clone();
        self.changed(c);
        t
    }

    pub fn set_start(&self, beat: f64) {
        let mut c = self.core.lock();
        c.transport.start_beat = beat.max(0.0);
        self.changed(c);
    }

    pub fn set_transport(&self, metro: Option<bool>, count_in: Option<bool>, rec: Option<bool>, monitor: Option<bool>) -> TransportDto {
        let control = self.transport_control.lock();
        let mut c = self.core.lock();
        if let Some(v) = metro {
            c.transport.metro = v;
        }
        if let Some(v) = count_in {
            c.transport.count_in = v;
        }
        if let Some(v) = rec {
            c.transport.rec = v;
        }
        if let Some(v) = monitor {
            c.transport.monitor = v;
        }
        let punch_out = !c.transport.rec && (c.take.is_some() || self.midi_take.lock().rec.is_some());
        let drain = if punch_out { self.end_take(&mut c, false) } else { None };
        // arming while the song plays punches in right away (on the armed track, else where
        // there is room from the playhead on)
        let punch_in = c.transport.rec && c.transport.playing && c.take.is_none() && self.midi_take.lock().rec.is_none();
        if punch_in {
            let at = self.shared().beat();
            let lap = c.project.loop_range.filter(|l| l.b > l.a).map(|l| (l.a, l.b));
            let track = self.rec_target(&mut c, at, lap.map_or(f64::INFINITY, |l| l.1));
            let take = if c.transport.rec_audio { self.new_take(&c, track, f64::NAN, lap) } else { None };
            if let Some(t) = take {
                self.shared().set_rec_start(f64::NAN);
                c.take = Some(t);
                self.send_monitor(&mut c);
                send(&mut c, Cmd::PunchIn { rec_loop: lap.is_some() });
            }
            self.begin_midi_take(&mut c, track, at);
        }
        // Record turned on while a pattern loops (PAT mode): the keyboard records into it
        if c.transport.rec && !c.transport.playing {
            self.begin_pat_take(&mut c);
        }
        let f = Cmd::Flags { metro: c.transport.metro, monitor: c.transport.monitor };
        send(&mut c, f);
        let t = c.transport.clone();
        if punch_out {
            drop(c);
            if let Some((shared, timeout)) = drain {
                wait_for_take_input(&shared, timeout);
            }
            self.finish_take();
            self.finish_midi_take();
            let mut c = self.core.lock();
            self.settle_rec_track(&mut c);
            drop(control);
            self.changed(c);
        } else {
            drop(control);
            self.changed(c);
        }
        t
    }

    /// The track a recording starting at `from` goes to: the armed one ("Record into this
    /// track"), else, as in FL, the first track with nothing on it from `from` to `until`
    /// (the loop end, or for good), else a new track added below the others.
    fn rec_target(&self, c: &mut Core, from: f64, until: f64) -> usize {
        let p = &c.project;
        if let Some(i) = p.tracks.iter().position(|t| t.arm).or_else(|| free_track(p, from, until)) {
            return i;
        }
        let n = c.project.tracks.len();
        let id = c.project.alloc_id();
        c.project.tracks.push(Track {
            id,
            name: format!("Track {}", n + 1),
            color: "#8b93a1".into(),
            muted: false,
            solo: false,
            arm: false,
            vol: VOL_UNITY,
            h: 56.0,
            insert: 0,
            icon: None,
        });
        c.rec_added = Some(id);
        c.revision += 1;
        self.sync(c, Dirty { timeline: true, mixer: true, params: true });
        n
    }

    /// Tells the audio thread which track the live input is monitored through (see [`monitor_of`]).
    fn send_monitor(&self, c: &mut Core) {
        let mut m = mix_state(&c.project);
        m.monitor_track = monitor_of(c);
        send(c, Cmd::Mix(Box::new(m)));
    }

    /// After a take landed (or did not): a track added for it that stayed empty goes away,
    /// and undo snapshots holding it empty forget it, so undoing the take removes it too.
    fn settle_rec_track(&self, c: &mut Core) {
        self.send_monitor(c);
        let Some(id) = c.rec_added.take() else { return };
        for snap in c.undo.iter_mut() {
            drop_empty_track(snap, id);
        }
        if drop_empty_track(&mut c.project, id) {
            c.revision += 1;
            self.sync(c, Dirty { timeline: true, mixer: true, params: true });
        }
    }

    fn end_take(&self, c: &mut Core, stop: bool) -> Option<(Arc<Shared>, Duration)> {
        let take = c.take.as_ref()?;
        let shared = self.shared();
        let frames = take.skip.max(shared.take_latency().unwrap_or_else(|| shared.io_latency()) as usize + take.pdc);
        let timeout = Duration::from_millis(100) + Duration::from_secs_f64((frames as f64 / f64::from(take.sr)).min(2.0));
        let cmd = Cmd::EndTake { stop, fallback_skip: take.skip, pdc: take.pdc };
        shared.finishing_take.store(true, Release);
        send(c, cmd);
        Some((shared, timeout))
    }

    /// An empty take buffer for the live device (`None` without one). Its start is `start`
    /// (NaN = learned from the audio thread at punch-in).
    fn new_take(&self, c: &Core, track: usize, start: f64, lap: Option<(f64, f64)>) -> Option<RecBuf> {
        let w = c.wires.as_ref()?;
        let sr = w.sr as u32;
        let pdc = playback_latency_with(&c.project, w.sr, &c.plugin_lat);
        let shared = self.shared();
        let skip = shared.io_latency() as usize + pdc;
        shared.reset_take_latency();
        Some(RecBuf {
            track,
            start,
            skip,
            pdc,
            lap,
            sr,
            data: Vec::with_capacity(sr as usize * 2 * 60),
            marks: vec![],
            nans: 0,
            peaks: vec![],
            peaks_sent: 0,
            acc: (0.0, 0.0, 0),
        })
    }

    pub fn watch_fx(&self, ids: &[Id]) {
        let mut c = self.core.lock();
        c.watch = ids.iter().copied().take(WATCH_SLOTS).collect();
        let w = watch_cmd(&c.watch);
        send(&mut c, w);
    }

    // ------------------------------------------------------------ audio sources

    /// Decodes files (slow; call off the UI thread) and adds them as sources, as one undo step.
    /// With `place`, also puts one clip per file on consecutive tracks starting there.
    pub fn import_audio(&self, paths: &[PathBuf], place: Option<(u32, f64)>) -> Result<(Vec<Id>, Vec<Id>), EngineError> {
        self.import_audio_with(paths, place, true)
    }

    /// [`Engine::import_audio`] where the caller says whether the import is an undo step of its own.
    /// `step` false is for a file loaded on the way to another edit (a sampler channel made from
    /// it, a channel's sample replaced): that edit is the step, and Ctrl+Z after dropping a sample
    /// on the rack took two presses, one of which changed nothing you could see. A new source still
    /// clears redo, so ids only ever grow along the history.
    pub fn import_audio_with(&self, paths: &[PathBuf], place: Option<(u32, f64)>, step: bool) -> Result<(Vec<Id>, Vec<Id>), EngineError> {
        let mut loaded = vec![];
        let mut errors = vec![];
        // A file of the sample library is one source however often it is used (a sampler
        // channel and audio clips share it), stored by its portable `lib:` path.
        let known: HashMap<String, Id> = {
            let c = self.core.lock();
            c.project
                .sources
                .iter()
                .filter(|s| s.path.starts_with(crate::factory::SCHEME) && c.sources.get(s.id).is_some())
                .map(|s| (s.path.clone(), s.id))
                .collect()
        };
        for p in paths {
            let stored = match p.to_str() {
                Some(s) if s.starts_with(crate::factory::SCHEME) => s.to_string(),
                _ => crate::factory::portable(p),
            };
            if known.contains_key(&stored) {
                loaded.push((p.clone(), stored, None));
                continue;
            }
            match Audio::load_path(&stored, Path::new(".")) {
                Ok(a) => loaded.push((p.clone(), stored, Some(a))),
                Err(e) => errors.push(e),
            }
        }
        if loaded.is_empty() {
            return Err(EngineError::Msg(errors.join("; ")));
        }
        let mut c = self.core.lock();
        let before = c.project.clone();
        let mut src_ids = vec![];
        let mut clip_ids = vec![];
        let bpm = c.project.bpm;
        for (k, (path, stored, audio)) in loaded.into_iter().enumerate() {
            let reuse =
                c.project.sources.iter().find(|s| s.path == stored && s.path.starts_with(crate::factory::SCHEME)).map(|s| (s.id, s.dur));
            let (id, seconds) = match (reuse, audio) {
                (Some(r), _) if c.sources.get(r.0).is_some() => r,
                (_, None) => continue,
                (_, Some(audio)) => {
                    let id = c.project.alloc_id();
                    let factory = crate::factory::embedded(&stored);
                    let name = match factory {
                        Some(f) => f.name.to_string(),
                        None => path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Audio".into()),
                    };
                    let colors = ["#69c3d6", "#47b8a8", "#5fbf8a", "#7c8cf8", "#e0a44a", "#9a8cf0", "#d9678f", "#5aa9e6"];
                    let color = factory.map(crate::factory::color).unwrap_or(colors[c.project.sources.len() % colors.len()]).to_string();
                    let seconds = audio.seconds();
                    let src = Source {
                        id,
                        name,
                        color,
                        path: stored,
                        dur: seconds,
                        sr: audio.sr,
                        ch: audio.channels,
                        peak_db: crate::dsp::gain_to_db(audio.peak()),
                    };
                    c.sources.insert(id, Arc::new(audio));
                    c.project.sources.push(src);
                    (id, seconds)
                }
            };
            let len_beats = seconds / (60.0 / bpm);
            src_ids.push(id);
            if let Some((track, beat)) = place {
                let ntracks = c.project.tracks.len() as u32;
                if ntracks == 0 {
                    continue;
                }
                let track = (track + k as u32).min(ntracks - 1);
                let cid = c.project.alloc_id();
                let start = beat.max(0.0);
                c.project.clips.push(Clip {
                    id: cid,
                    track,
                    start,
                    len: len_beats.min(TOTAL_BEATS - start).max(1.0 / 16.0),
                    offset: 0.0,
                    kind: ClipKind::Audio,
                    source: id,
                    muted: false,
                    gain: 0.0,
                    fade_in: 0.0,
                    fade_out: 0.0,
                    pan: 0.0,
                    norm: false,
                    rev: false,
                    tune: None,
                    stretch: 1.0,
                    resample: false,
                    pitch: 0.0,
                    insert: None,
                    root: crate::model::CLIP_ROOT,
                });
                clip_ids.push(cid);
            }
        }
        let added = c.project.sources.len() != before.sources.len();
        let fold = if step || !clip_ids.is_empty() {
            push_undo(&mut c, before);
            None
        } else if added {
            c.redo.clear();
            Some(before)
        } else {
            None
        };
        c.revision += 1;
        if let Some(before) = fold {
            // the edit that follows (the sampler channel) is the step, and it takes the source with it
            let base = match c.import_base.take() {
                Some((r, b)) if r + 1 == c.revision => b,
                _ => before,
            };
            c.import_base = Some((c.revision, base));
        }
        self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
        self.changed(c);
        Ok((src_ids, clip_ids))
    }

    /// Points sources at other files (slow: decodes them; call off the UI thread), as one undo
    /// step. A file that does not decode leaves its source unchanged and is reported; when no
    /// file decodes nothing changes and it is an error. Library (`lib:`) sources come with
    /// their kit and cannot be relinked. The source keeps its name; its length, rate and
    /// channels become the new file's. Undo brings back the old path and what it played.
    pub fn relink_sources(&self, items: &[Relink]) -> Result<Vec<Relinked>, EngineError> {
        let mut out = vec![];
        let mut loaded = vec![];
        {
            let c = self.core.lock();
            for it in items {
                let Some(s) = c.project.sources.iter().find(|s| s.id == it.source_id) else {
                    return Err(EngineError::Msg(format!("no source {}", it.source_id)));
                };
                if s.path.starts_with(crate::factory::SCHEME) {
                    return Err(EngineError::Msg(format!("{}: a library sample; install its kit instead", s.name)));
                }
            }
        }
        for it in items {
            let p = PathBuf::from(&it.path);
            if !p.is_absolute() {
                out.push(Relinked {
                    source_id: it.source_id,
                    path: it.path.clone(),
                    error: Some(format!("{}: not an absolute path", it.path)),
                    length_differs: false,
                });
                continue;
            }
            match Audio::load(&p) {
                Ok(a) => loaded.push((it.source_id, p.display().to_string(), Arc::new(a))),
                Err(e) => out.push(Relinked { source_id: it.source_id, path: it.path.clone(), error: Some(e), length_differs: false }),
            }
        }
        if loaded.is_empty() {
            let why: Vec<String> = out.iter().filter_map(|r| r.error.clone()).collect();
            return Err(EngineError::Msg(why.join("; ")));
        }
        let mut c = self.core.lock();
        let before = c.project.clone();
        for (id, path, audio) in loaded {
            // the source may have gone while the files decoded
            let Some(k) = c.project.sources.iter().position(|s| s.id == id) else {
                out.push(Relinked { source_id: id, path, error: Some("the source was removed".into()), length_differs: false });
                continue;
            };
            let seconds = audio.seconds();
            let s = &mut c.project.sources[k];
            let length_differs = !crate::relink::same_length(s.dur, seconds);
            let old = std::mem::replace(&mut s.path, path.clone());
            s.dur = seconds;
            s.sr = audio.sr;
            s.ch = audio.channels;
            s.peak_db = crate::dsp::gain_to_db(audio.peak());
            c.sources.relink(id, &old, &path, audio);
            c.source_errors.remove(&id);
            out.push(Relinked { source_id: id, path, error: None, length_differs });
        }
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
        self.changed(c);
        Ok(out)
    }

    /// Looks for the missing sources' files under `folder` (slow: walks it and decodes the
    /// files it takes; call off the UI thread). A source whose file is the only best match by
    /// name and has the remembered length is relinked (all of them as one undo step); the
    /// others are returned as candidates for the user to choose from.
    pub fn relink_search(&self, folder: &Path) -> Result<RelinkSearch, EngineError> {
        if !folder.is_dir() {
            return Err(EngineError::Msg(format!("{}: not a folder", folder.display())));
        }
        let wanted: Vec<(Id, String, f64)> = {
            let c = self.core.lock();
            let missing = missing_sources(&c);
            c.project
                .sources
                .iter()
                .filter(|s| missing.contains_key(&s.id) && !s.path.starts_with(crate::factory::SCHEME))
                .map(|s| (s.id, s.path.clone(), s.dur))
                .collect()
        };
        let names: Vec<(Id, String)> = wanted.iter().map(|w| (w.0, w.1.clone())).collect();
        let found = crate::relink::search(folder, &names, crate::relink::Limits::default());
        let mut candidates = found.candidates;
        let mut take = vec![];
        let mut not_found = vec![];
        for (id, _, dur) in &wanted {
            let Some(list) = candidates.get(id) else {
                not_found.push(*id);
                continue;
            };
            let Some(best) = crate::relink::unambiguous(list) else { continue };
            // the length decides; a file that does not decode stays a candidate
            if let Ok(a) = Audio::load(Path::new(&best.path)) {
                if crate::relink::same_length(*dur, a.seconds()) {
                    take.push(Relink { source_id: *id, path: best.path.clone() });
                }
            }
        }
        let relinked = if take.is_empty() { vec![] } else { self.relink_sources(&take)? };
        for r in relinked.iter().filter(|r| r.error.is_none()) {
            candidates.remove(&r.source_id);
        }
        Ok(RelinkSearch { relinked, candidates, not_found, scanned: found.scanned, truncated: found.truncated })
    }

    /// Applies ops built against the project once the standard-library samples `files` (file
    /// names, e.g. `kick-808.flac`) are project sources, as one undo step: `build` gets the
    /// project with them and each file's source id. Nothing changes when `build` or the ops
    /// fail. Library samples are decoded from the program, so this is quick.
    pub fn apply_with_library<F>(&self, files: &[&str], if_revision: Option<u64>, build: F) -> Result<u64, EngineError>
    where
        F: FnOnce(&Project, &HashMap<String, Id>) -> Result<Vec<Op>, String>,
    {
        let have: HashSet<String> = {
            let c = self.core.lock();
            c.project.sources.iter().filter(|s| c.sources.get(s.id).is_some()).map(|s| s.path.clone()).collect()
        };
        let mut decoded = vec![];
        let mut seen = HashSet::new();
        for f in files {
            let path = crate::factory::lib_path(f);
            if !seen.insert(path.clone()) || have.contains(&path) {
                continue;
            }
            let a = Audio::load_path(&path, Path::new(".")).map_err(EngineError::Msg)?;
            decoded.push((path, a));
        }
        let mut c = self.core.lock();
        if let Some(r) = if_revision {
            if r != c.revision {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        let before = c.project.clone();
        let mut ids: HashMap<String, Id> = HashMap::new();
        for f in files {
            let path = crate::factory::lib_path(f);
            if let Some(s) = c.project.sources.iter().find(|s| s.path == path && c.sources.get(s.id).is_some()) {
                ids.insert(f.to_string(), s.id);
            }
        }
        let mut added = vec![];
        for (path, audio) in decoded {
            let Some(ff) = crate::factory::embedded(&path) else { continue };
            if ids.contains_key(ff.file) {
                continue;
            }
            let id = c.project.alloc_id();
            c.project.sources.push(Source {
                id,
                name: ff.name.to_string(),
                color: crate::factory::color(ff).to_string(),
                path,
                dur: audio.seconds(),
                sr: audio.sr,
                ch: audio.channels,
                peak_db: crate::dsp::gain_to_db(audio.peak()),
            });
            c.sources.insert(id, Arc::new(audio));
            ids.insert(ff.file.to_string(), id);
            added.push(id);
        }
        let result = build(&c.project, &ids).map_err(EngineError::Msg).and_then(|ops| {
            let mut p = c.project.clone();
            ops::apply_all(&mut p, &ops).map(|_| p).map_err(EngineError::from)
        });
        let p = match result {
            Ok(p) => p,
            Err(e) => {
                c.project = before;
                for id in added {
                    c.sources.audio.remove(&id);
                }
                return Err(e);
            }
        };
        c.project = p;
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        let rev = c.revision;
        self.changed(c);
        Ok(rev)
    }

    /// Pitch curve and notes of an audio clip's source (for the note editor).
    pub fn pitch_analysis(&self, clip_id: Id) -> Result<Arc<crate::pitch::PitchAnalysis>, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))
    }

    fn audio_clip(&self, clip_id: Id) -> Result<Clip, EngineError> {
        let c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
        if clip.kind != ClipKind::Audio {
            return Err(EngineError::Msg("only audio clips have pitch".into()));
        }
        Ok(clip)
    }

    /// Analysis of a source variant; slow detectors run without holding the project lock.
    fn source_analysis(&self, id: Id, norm: bool, rev: bool) -> Option<Arc<crate::pitch::PitchAnalysis>> {
        let (audio, est) = {
            let mut c = self.core.lock();
            if let Some(a) = c.sources.cached_analysis(id, norm, rev) {
                return Some(a);
            }
            (c.sources.variant(id, norm, rev)?, c.sources.estimator())
        };
        let a = Arc::new(crate::pitch::analyze_with(&crate::sources::mono_of(&audio), audio.sr as f32, est.as_deref()));
        let mut c = self.core.lock();
        c.sources.put_analysis(id, norm, rev, a.clone());
        Some(c.sources.cached_analysis(id, norm, rev).unwrap_or(a))
    }

    fn source_content(&self, id: Id, norm: bool, rev: bool) -> Option<Arc<crate::content::ContentFeatures>> {
        let audio = {
            let mut c = self.core.lock();
            if let Some(x) = c.sources.cached_content(id, norm, rev) {
                return Some(x);
            }
            c.sources.variant(id, norm, rev)?
        };
        let x = Arc::new(crate::content::mfcc(&crate::sources::mono_of(&audio), audio.sr as f32));
        self.core.lock().sources.put_content(id, norm, rev, x.clone());
        Some(x)
    }

    /// Tune to Reference target of an audio clip with `params` (the clip's own when `None`).
    /// The slow parts (analysis, features, alignment) run without the project lock and are
    /// cached, so committing the same reference afterwards only renders.
    pub fn pitch_target(&self, clip_id: Id, params: Option<RefTune>) -> Result<Arc<crate::reference::PitchTarget>, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let p = params
            .or_else(|| clip.tune.as_ref().and_then(|t| t.reference.clone()))
            .ok_or(EngineError::Msg("the clip has no reference".into()))?;
        if let Some(m) = &p.melody {
            let region = clip.source_span(self.core.lock().project.bpm);
            let take = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
            let spans = crate::sources::note_spans(&clip, &take);
            let al = crate::melody::alignment(&take, region, &spans, &m.notes);
            return Ok(Arc::new(crate::reference::target(&take, &spans, &al, &p)));
        }
        let key = {
            let mut c = self.core.lock();
            if !c.project.sources.iter().any(|s| s.id == p.source) {
                return Err(EngineError::Msg("the reference source is not in the project".into()));
            }
            let bpm = c.project.bpm;
            c.sources.align_key(&clip, bpm, p.source).ok_or(EngineError::Msg("audio not loaded".into()))?
        };
        let take = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let cached = self.core.lock().sources.cached_alignment(&key);
        let al = match cached {
            Some(a) => a,
            None => {
                let reference =
                    self.source_analysis(p.source, false, false).ok_or(EngineError::Msg("reference audio not loaded".into()))?;
                let tc = self.source_content(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
                let rc = self.source_content(p.source, false, false).ok_or(EngineError::Msg("reference audio not loaded".into()))?;
                let r = crate::pitch::ANALYSIS_RATE as f64;
                let al = Arc::new(crate::reference::align(
                    &crate::reference::Performance { pitch: &take, content: &tc },
                    (key.2 as f64 / r, key.3 as f64 / r),
                    &crate::reference::Performance { pitch: &reference, content: &rc },
                ));
                self.core.lock().sources.put_alignment(key, al.clone());
                al
            }
        };
        Ok(Arc::new(crate::sources::target_for(&clip, &take, &al, &p)))
    }

    /// Everything the pitch editor shows for an audio clip, read without holding the project
    /// lock during analysis: the clip, its analysis, the Tune to Reference target (if any), the
    /// notes it is edited by (its own, else the analysis' segmentation: `own` says which) and the
    /// pitch every analysis frame will sound at.
    pub fn pitch_view(&self, clip_id: Id) -> Result<PitchView, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let analysis = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let target = match clip.tune.as_ref().and_then(|t| t.reference.as_ref()) {
            Some(_) => Some(self.pitch_target(clip_id, None)?),
            None => None,
        };
        let (notes, own) = crate::pitch_api::clip_notes(clip.tune.as_ref(), &analysis);
        let tune = clip.tune.clone().unwrap_or_default();
        let out = crate::tune_render::edited_curve(&analysis, &tune, target.as_deref());
        let (bpm, sr, seconds) = {
            let mut c = self.core.lock();
            let bpm = c.project.bpm;
            let a = c.sources.variant(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
            (bpm, a.sr, a.seconds())
        };
        Ok(PitchView { clip, analysis, target, notes, own, out, bpm, sr, seconds })
    }

    /// Compares an audio clip's singing with another source's (`crate::pitch_api::compare`).
    /// Read-only; seconds per minute of audio (two alignments at most).
    pub fn pitch_compare(&self, clip_id: Id, reference: Id) -> Result<crate::pitch_api::Comparison, EngineError> {
        let view = self.pitch_view(clip_id)?;
        if !self.core.lock().project.sources.iter().any(|s| s.id == reference) {
            return Err(EngineError::Msg(format!("no source {reference}")));
        }
        let tc = self.source_content(view.clip.source, view.clip.norm, view.clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let ra = self.source_analysis(reference, false, false).ok_or(EngineError::Msg("reference audio not loaded".into()))?;
        let rc = self.source_content(reference, false, false).ok_or(EngineError::Msg("reference audio not loaded".into()))?;
        let spans: Vec<(f64, f64)> = view.notes.iter().map(|n| (n.start, n.end)).collect();
        Ok(crate::pitch_api::compare(&view.analysis, &tc, view.region(), &spans, &ra, &rc))
    }

    /// What an audio clip plays before time-stretching — with its pitch edits rendered (now, not
    /// in the background) when `edits`, else its plain source — and the part of it the clip
    /// shows (source seconds).
    pub fn clip_tuned_audio(&self, clip_id: Id, edits: bool) -> Result<(Arc<crate::sources::Audio>, (f64, f64)), EngineError> {
        let mut clip = self.audio_clip(clip_id)?;
        if !edits {
            clip.tune = None;
        }
        let mut c = self.core.lock();
        let bpm = c.project.bpm;
        let a = c.sources.clip_source_audio(&clip, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?;
        Ok((a, clip.source_span(bpm)))
    }

    /// The notes `channel` plays over an audio clip, as a melody reference (`span`: seconds of
    /// the source, only notes overlapping it), and how well they fit what is sung (0..1).
    pub fn melody_reference(&self, clip_id: Id, channel: Id, span: Option<[f64; 2]>) -> Result<(crate::model::Melody, f32), EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let (notes, region) = {
            let c = self.core.lock();
            if c.project.channels.iter().all(|ch| ch.id != channel) {
                return Err(EngineError::Msg("no such channel".into()));
            }
            (crate::melody::collect(&c.project, &clip, channel, span), clip.source_span(c.project.bpm))
        };
        let take = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let fit = crate::melody::fit(&take, region, &notes);
        Ok((crate::model::Melody { channel, span, notes }, fit))
    }

    /// Channels whose notes play over an audio clip: `(channel, notes, fit 0..1)`, best fit first.
    pub fn melody_channels(&self, clip_id: Id) -> Result<Vec<(Id, usize, f32)>, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let (sets, region) = {
            let c = self.core.lock();
            let sets: Vec<(Id, Vec<MelodyNote>)> = crate::melody::channels_over(&c.project, &clip)
                .into_iter()
                .map(|(id, _)| (id, crate::melody::collect(&c.project, &clip, id, None)))
                .collect();
            (sets, clip.source_span(c.project.bpm))
        };
        if sets.is_empty() {
            return Ok(vec![]);
        }
        let take = self.source_analysis(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let mut out: Vec<(Id, usize, f32)> = sets.iter().map(|(id, ns)| (*id, ns.len(), crate::melody::fit(&take, region, ns))).collect();
        out.sort_by(|a, b| b.2.total_cmp(&a.2).then(b.1.cmp(&a.1)));
        Ok(out)
    }

    /// Uses `est` for pitch analysis from now on (`None` = built-in YIN). Pitch-edited
    /// clips are re-rendered with the new analysis.
    pub fn set_f0_estimator(&self, est: Option<Arc<dyn crate::pitch::F0Estimator>>) {
        let mut c = self.core.lock();
        c.sources.set_estimator(est);
        self.sync(&mut c, Dirty { timeline: true, mixer: false, params: false });
    }

    /// Name of the pitch detector in use ("yin" or the estimator's).
    pub fn f0_backend(&self) -> &'static str {
        self.core.lock().sources.backend()
    }

    fn source_music(&self, id: Id, norm: bool, rev: bool) -> Option<Arc<crate::music::SourceMusic>> {
        if let Some(m) = self.core.lock().sources.cached_music(id, norm, rev) {
            return Some(m);
        }
        let audio = self.core.lock().sources.variant(id, norm, rev)?;
        let m = crate::music::analyze_audio(&audio);
        self.core.lock().sources.put_music(id, norm, rev, m.clone());
        Some(m)
    }

    /// Tempo and key of an audio clip, or key of the project's audio clips
    /// (excluding muted tracks/clips and tracks routed to `exclude_fx`'s insert,
    /// so key detection for Pitch Correct listens to the backing rather than the vocal).
    pub fn music_analysis(&self, clip_id: Option<Id>, exclude_fx: Option<Id>) -> Result<crate::music::MusicAnalysis, EngineError> {
        self.music_analysis_ex(clip_id, exclude_fx, None)
    }

    /// [`Engine::music_analysis`], also excluding the tracks routed to insert `exclude_insert`.
    fn music_analysis_ex(
        &self,
        clip_id: Option<Id>,
        exclude_fx: Option<Id>,
        exclude_insert: Option<Id>,
    ) -> Result<crate::music::MusicAnalysis, EngineError> {
        if let Some(cid) = clip_id {
            let (clip, bpm) = {
                let c = self.core.lock();
                let clip = c.project.clips.iter().find(|x| x.id == cid).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
                (clip, c.project.bpm)
            };
            if clip.kind != ClipKind::Audio {
                return Err(EngineError::Msg("only audio clips have tempo and key".into()));
            }
            let m = self.source_music(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
            let (a, b) = clip.source_span(bpm);
            let (mut cs, n) = m.chroma_sum(a, b);
            if n == 0 {
                (cs, _) = m.chroma_sum(0.0, 1e9);
            }
            let key = crate::music::estimate_key(&cs, m.tuning_cents);
            return Ok(crate::music::MusicAnalysis { tempo: m.tempo.clone(), key });
        }

        let (all_clips, excluded_insert, bpm) = {
            let c = self.core.lock();
            let p = &c.project;
            let ex_ins = exclude_insert.or_else(|| exclude_fx.and_then(|fid| p.find_fx(fid).map(|(ins, _)| ins.id)));
            let any_solo = p.tracks.iter().any(|t| t.solo);
            let active: Vec<(Clip, bool)> = p
                .clips
                .iter()
                .filter(|cl| cl.kind == ClipKind::Audio && !cl.muted)
                .filter_map(|cl| {
                    let tr = p.tracks.get(cl.track as usize)?;
                    if tr.muted || (any_solo && !tr.solo) {
                        return None;
                    }
                    let is_excluded = ex_ins == Some(tr.insert);
                    Some((cl.clone(), is_excluded))
                })
                .collect();
            (active, ex_ins, p.bpm)
        };
        let use_non_excluded = excluded_insert.is_some() && all_clips.iter().any(|(_, ex)| !*ex);
        let chosen: Vec<Clip> = all_clips.into_iter().filter(|(_, ex)| !use_non_excluded || !*ex).map(|(cl, _)| cl).collect();
        let mut total_chroma = [0f32; 12];
        let mut tuning_sum = 0f32;
        let mut weight_sum = 0f32;
        let mut best_tempo: Option<(f64, crate::music::TempoEstimate)> = None;
        for cl in chosen {
            let Some(m) = self.source_music(cl.source, cl.norm, cl.rev) else { continue };
            let (a, b) = cl.source_span(bpm);
            let (cs, n) = m.chroma_sum(a, b);
            if n > 0 {
                for k in 0..12 {
                    total_chroma[k] += cs[k];
                }
                tuning_sum += m.tuning_cents * n as f32;
                weight_sum += n as f32;
            }
            if let Some(ref t) = m.tempo {
                let score = cl.len * t.confidence.max(0.1) as f64;
                if best_tempo.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                    best_tempo = Some((score, t.clone()));
                }
            }
        }
        let tuning = if weight_sum > 0.0 { tuning_sum / weight_sum } else { 0.0 };
        let key = crate::music::estimate_key(&total_chroma, tuning);
        Ok(crate::music::MusicAnalysis { tempo: best_tempo.map(|(_, t)| t), key })
    }

    /// The mixer insert the clip is heard on (its own, else its track's); `None` when the
    /// clip does not exist. What Auto Vocal's chain replaces the effects of.
    pub fn clip_insert(&self, clip_id: Id) -> Option<Id> {
        let c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id)?;
        Some(c.project.clip_insert(clip))
    }

    /// "Auto vocal": measures the vocal clip (what it plays, its region only) and returns
    /// the calibrated insert chain for its track. The key comes from `beat_clip` when given,
    /// else from the project's other audio (tracks not routed to the vocal's insert); the
    /// level suggestion needs `beat_clip`. Slow (seconds): call off the UI thread.
    ///
    /// `bake_rider`: the plan's rider comes out of the chain into `rider_bake`, for
    /// [`Self::bake_rider`] (no live look-ahead on the insert, so no playback delay).
    pub fn auto_vocal_plan(
        &self,
        clip_id: Id,
        beat_clip: Option<Id>,
        style: crate::autovocal::Style,
        bake_rider: bool,
    ) -> Result<crate::autovocal::AutoVocalPlan, EngineError> {
        let clip = self.audio_clip(clip_id)?;
        let region = |cl: &Clip| -> Result<(Vec<f32>, f32), EngineError> {
            let ((audio, rate), bpm) = {
                let mut c = self.core.lock();
                let bpm = c.project.bpm;
                (c.sources.clip_audio(cl, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?, bpm)
            };
            let mono = crate::sources::mono_of(&audio);
            let sr = audio.sr as f64;
            let (a, b) = crate::sources::clip_region(cl, bpm, rate, mono.len() as f64 / sr);
            let (i0, i1) = ((a * sr) as usize, ((b * sr) as usize).min(mono.len()));
            let gain = crate::dsp::db_to_gain(cl.gain);
            Ok((mono[i0.min(i1)..i1].iter().map(|v| v * gain).collect(), audio.sr as f32))
        };
        let (vocal, sr) = region(&clip)?;
        if vocal.len() < (sr * 1.0) as usize {
            return Err(EngineError::Msg("the clip is too short to analyse (under 1 s)".into()));
        }
        let vocal_insert = {
            let c = self.core.lock();
            Some(c.project.clip_insert(&clip))
        };
        let key = match beat_clip {
            Some(b) => self.music_analysis(Some(b), None)?.key,
            None => self.music_analysis_ex(None, None, vocal_insert).ok().and_then(|m| m.key),
        };
        let pitch = self.pitch_analysis(clip_id).ok();
        let beat = match beat_clip {
            Some(b) => {
                let bc = {
                    let c = self.core.lock();
                    c.project.clips.iter().find(|x| x.id == b).cloned().ok_or(EngineError::Msg("no such beat clip".into()))?
                };
                Some(region(&bc)?)
            }
            None => None,
        };
        let plan = crate::autovocal::plan(&crate::autovocal::PlanInput {
            vocal: &vocal,
            sr,
            style,
            key,
            pitch: pitch.as_deref(),
            beat: beat.as_ref().map(|(x, s)| (x.as_slice(), *s)),
        });
        Ok(if bake_rider { plan.baked() } else { plan })
    }

    /// Renders what an audio clip plays (its region, clip gain, normalise/reverse and pitch
    /// edits; not its fades, which stay on the clip) through the Vocal Rider offline, as
    /// `autovocal::render` does — the rider's latency removed, sample-aligned, any
    /// look-ahead — into a new source, and points the clip at it (gain 0 dB, no
    /// norm/rev/pitch edits, the region's sub-sample start kept as the offset). The original
    /// source stays in the project. One undo step. `params` are Rider params (catalog ids;
    /// missing ones take the catalog defaults), e.g. an auto-vocal plan's `rider_bake`.
    /// Returns (new source id, clip id). Slow (the whole region is rendered): call it off
    /// the UI thread. `if_revision` is checked under the commit lock, so a project that
    /// moved on while the rider was rendering refuses the bake before anything is written.
    pub fn bake_rider(
        &self,
        clip_id: Id,
        params: &std::collections::BTreeMap<String, f64>,
        if_revision: Option<u64>,
    ) -> Result<(Id, Id), EngineError> {
        let (clip, audio, bpm, src_name, color) = {
            let mut c = self.core.lock();
            let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
            if clip.kind != ClipKind::Audio {
                return Err(EngineError::Msg("only audio clips can be baked".into()));
            }
            let bpm = c.project.bpm;
            let (audio, rate) = c.sources.clip_audio(&clip, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?;
            if clip.needs_stretch() && rate != clip.rendered_rate() {
                return Err(EngineError::Msg("the clip's time-stretch could not be rendered".into()));
            }
            let src = c.project.sources.iter().find(|s| s.id == clip.source);
            let (name, color) = src.map(|s| (s.name.clone(), s.color.clone())).unwrap_or_else(|| ("Vocal".into(), "#e05263".into()));
            (clip, (audio, rate), bpm, name, color)
        };
        // a resampled clip bakes its source region, and keeps playing it resampled
        let (audio, rate) = audio;
        let sr = audio.sr;
        let (a, b) = crate::sources::clip_region(&clip, bpm, rate, audio.seconds());
        // the same frames `auto_vocal_plan` measures (truncated start); the fraction of a
        // frame the region starts past it stays on the clip as its offset
        let pos = a * sr as f64;
        let i0 = (pos as usize).min(audio.frames);
        let i1 = ((b * sr as f64) as usize).clamp(i0, audio.frames);
        if i1 - i0 < 16 {
            return Err(EngineError::Msg("the clip is empty".into()));
        }
        // two guard frames (as in `flatten_comp`): playback reads a frame or two past the
        // region's last whole frame (interpolation, the clip end landing between frames)
        let i1 = (i1 + 2).min(audio.frames);
        let g = crate::dsp::db_to_gain(clip.gain);
        let content = audio.content();
        let l: Vec<f32> = (i0..i1).map(|i| content[2 * i] * g).collect();
        let r: Vec<f32> = (i0..i1).map(|i| content[2 * i + 1] * g).collect();
        let (yl, yr) = crate::autovocal::render_stereo(FxKind::Rider, params, &l, &r, sr as f32);
        let data: Vec<f32> = yl.iter().zip(&yr).flat_map(|(a, b)| [*a, *b]).collect();
        let spb = 60.0 / bpm;
        // beats of the clip (a resampled clip reads `rate` source beats per clip beat)
        let offset = (pos - i0 as f64) / sr as f64 / spb / rate;
        let (stretch, resample) = if rate != 1.0 { (clip.stretch, clip.resample) } else { (1.0, false) };

        let mut c = self.core.lock();
        if let Some(r) = if_revision {
            if c.revision != r {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        let now = c.project.clips.iter().find(|x| x.id == clip_id).cloned();
        if now.as_ref() != Some(&clip) || c.project.bpm != bpm {
            return Err(EngineError::Msg("the clip changed while the rider was rendering; try again".into()));
        }
        let id = c.project.alloc_id();
        let name = format!("{src_name} (rider)");
        let path = fresh_path(&audio_dir(&c), &format!("{} rider {}", sanitize(&src_name), id));
        write_wav(&path, &data, sr)?;
        let baked = Audio::from_stereo(data, sr, audio.channels);
        let before = c.project.clone();
        c.project.sources.push(Source {
            id,
            name,
            color,
            path: path.display().to_string(),
            dur: baked.seconds(),
            sr,
            ch: audio.channels,
            peak_db: crate::dsp::gain_to_db(baked.peak()),
        });
        c.sources.insert(id, Arc::new(baked));
        if let Some(x) = c.project.clips.iter_mut().find(|x| x.id == clip_id) {
            // a time-stretched bake holds the stretched audio: it plays as it is
            *x = Clip { source: id, offset, gain: 0.0, norm: false, rev: false, tune: None, stretch, resample, ..clip };
        }
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
        self.changed(c);
        Ok((id, clip_id))
    }

    /// Peaks of what the clip actually plays (including pitch edits), for drawing. In source
    /// time: a stretched clip draws them stretched (`Clip::stretch`).
    pub fn clip_peaks(&self, clip_id: Id) -> Option<Vec<f32>> {
        let mut c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned()?;
        let bpm = c.project.bpm;
        c.sources.clip_source_audio(&clip, true, bpm).map(|a| a.peaks.clone())
    }

    /// [min,max] pairs at 100/s for drawing.
    pub fn source_peaks(&self, id: Id, norm: bool, rev: bool) -> Option<Vec<f32>> {
        let mut c = self.core.lock();
        c.sources.variant(id, norm, rev).map(|a| a.peaks.clone())
    }

    /// Replaces the clip's source by a new file holding only the clip's region.
    pub fn trim_clip(&self, clip_id: Id) -> Result<Id, EngineError> {
        let mut c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
        if clip.kind != ClipKind::Audio {
            return Err(EngineError::Msg("only audio clips can be trimmed".into()));
        }
        let audio = c.sources.variant(clip.source, clip.norm, clip.rev).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let sr = audio.sr as f64;
        // the source region (a stretched clip keeps its stretch over the trimmed source)
        let (sa, sb) = clip.source_span(c.project.bpm);
        let a = ((sa * sr) as usize).min(audio.frames);
        let b = ((sb * sr) as usize).min(audio.frames);
        if b <= a + 16 {
            return Err(EngineError::Msg("the region is empty".into()));
        }
        let data = audio.content()[a * 2..b * 2].to_vec();
        let old = c.project.sources.iter().find(|s| s.id == clip.source).cloned().unwrap();
        let id = c.project.alloc_id();
        let path = fresh_path(&audio_dir(&c), &format!("{} trim {}", sanitize(&old.name), id));
        write_wav(&path, &data, audio.sr)?;
        let new = Audio::from_stereo(data, audio.sr, audio.channels);
        let before = c.project.clone();
        c.project.sources.push(Source {
            id,
            path: path.display().to_string(),
            dur: new.seconds(),
            peak_db: crate::dsp::gain_to_db(new.peak()),
            ..old
        });
        c.sources.insert(id, Arc::new(new));
        if let Some(x) = c.project.clips.iter_mut().find(|x| x.id == clip_id) {
            x.source = id;
            x.offset = 0.0;
            x.norm = false;
            x.rev = false;
        }
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
        self.changed(c);
        Ok(id)
    }

    /// FL Studio's "Make unique" for audio clips. The clips (all of one source, on one track)
    /// get their own copy of the source entry — the same file and decoded audio, nothing is
    /// written — with its own name ("Name #2") and colour, and move to a new track right below
    /// theirs, named and coloured after the copy. The new track plays where the old one does
    /// (the master unless the user routed it): no mixer insert is claimed or created on its
    /// behalf, routing is the user's choice. One undo step. Other clips of the source keep it,
    /// their track and their routing.
    pub fn make_unique_audio(&self, clip_ids: &[Id]) -> Result<MadeUnique, EngineError> {
        let mut c = self.core.lock();
        let err = |m: &str| EngineError::Msg(m.into());
        let clips: Vec<Clip> = clip_ids
            .iter()
            .map(|id| c.project.clips.iter().find(|x| x.id == *id).cloned().ok_or(err("no such clip")))
            .collect::<Result<_, _>>()?;
        let first = clips.first().ok_or(err("no clips given"))?.clone();
        if clips.iter().any(|x| x.kind != ClipKind::Audio) {
            return Err(err("only audio clips can be made unique this way"));
        }
        if clips.iter().any(|x| x.source != first.source || x.track != first.track) {
            return Err(err("the clips must play the same sample on the same track"));
        }
        let old = c.project.sources.iter().find(|s| s.id == first.source).cloned().ok_or(err("the clip's source is missing"))?;
        let audio = c.sources.get(old.id).cloned().ok_or(err("audio not loaded"))?;
        let track = c.project.tracks.get(first.track as usize).cloned().ok_or(err("the clip's track is missing"))?;
        // "Guitar", "Guitar #2", … : the next number no source of that name has yet
        let base = unique_base(&old.name);
        let name = (2..).map(|k| format!("{base} #{k}")).find(|n| !c.project.sources.iter().any(|s| &s.name == n)).unwrap();

        let before = c.project.clone();
        let id = c.project.alloc_id();
        c.project.sources.push(Source { id, name: name.clone(), ..old.clone() });
        c.sources.insert(id, audio);
        let insert_id = track.insert;
        let track_id = c.project.alloc_id();
        let at = first.track + 1;
        c.project.tracks.insert(
            at as usize,
            Track {
                id: track_id,
                name: name.clone(),
                color: old.color.clone(),
                muted: false,
                solo: false,
                arm: false,
                vol: VOL_UNITY,
                h: track.h,
                insert: insert_id,
                icon: track.icon.clone(),
            },
        );
        for x in c.project.clips.iter_mut() {
            if clip_ids.contains(&x.id) {
                x.track = at;
                x.source = id;
            } else if x.track >= at {
                x.track += 1;
            }
        }
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, ..Default::default() });
        self.changed(c);
        Ok(MadeUnique { source_id: id, track: at, insert_id })
    }

    /// Plays one clip alone on the output. `from` (seconds of the source) starts there and plays
    /// the take with its pitch edits at the source's own speed (the pitch editor's Solo), to the
    /// end of the clip's region; without it the whole clip plays as the playlist would.
    pub fn preview_clip(&self, clip_id: Id, from: Option<f64>) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
        let bpm = c.project.bpm;
        // a time-stretch that is not rendered yet previews the source at its own speed
        let ready = if from.is_none() && clip.needs_stretch() {
            c.sources.stretch_request(&clip, true, bpm).and_then(Result::ok).filter(|x| x.1 == clip.rendered_rate())
        } else {
            None
        };
        let (audio, (a, b)) = match ready {
            Some((a, rate)) => {
                let r = crate::sources::clip_region(&clip, bpm, rate, a.seconds());
                (a, r)
            }
            None => {
                (c.sources.clip_source_audio(&clip, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?, clip.source_span(bpm))
            }
        };
        let sr = audio.sr as f64;
        let end = (b * sr).min(audio.frames as f64);
        let pos = from.map_or(a, |f| f.clamp(a, b)) * sr;
        if pos >= end {
            return Err(EngineError::Msg("nothing to play after that point".into()));
        }
        let pv = Preview {
            pos,
            start: a * sr,
            end,
            gain: 10f32.powf(clip.gain / 20.0),
            // a start inside the clip is not its beginning: no fade-in there
            fade_in: if from.is_some_and(|f| f > a + 1e-3) { 0.0 } else { clip.fade_in as f64 },
            fade_out: clip.fade_out as f64,
            rate: 1.0,
            audio,
        };
        send(&mut c, Cmd::Preview(Some(Box::new(pv))));
        Ok(())
    }

    /// Plays an audio clip's sample at MIDI `key`, like a sampler would (the sample editor's
    /// keyboard, FL's audio clip channel): from the start of the clip's region to its end, read
    /// faster or slower by the distance from the clip's root key plus its pitch knob. Its own
    /// speed — the clip's time-stretch does not apply. Not an edit.
    pub fn preview_clip_key(&self, clip_id: Id, key: u8, vel: f32) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
        if clip.kind != crate::model::ClipKind::Audio {
            return Err(EngineError::Msg("not an audio clip".into()));
        }
        let bpm = c.project.bpm;
        let audio = c.sources.clip_source_audio(&clip, false, bpm).ok_or(EngineError::Msg("audio not loaded".into()))?;
        let (a, b) = clip.source_span(bpm);
        let sr = audio.sr as f64;
        let end = (b * sr).min(audio.frames as f64);
        if a * sr >= end {
            return Err(EngineError::Msg("the clip's region is empty".into()));
        }
        let semis = key.min(127) as f64 - clip.root as f64 + clip.pitch as f64;
        let pv = Preview {
            pos: a * sr,
            start: a * sr,
            end,
            gain: 10f32.powf(clip.gain / 20.0) * vel.clamp(0.0, 1.0),
            fade_in: clip.fade_in as f64,
            fade_out: clip.fade_out as f64,
            rate: 2f64.powf(semis / 12.0),
            audio,
        };
        send(&mut c, Cmd::Preview(Some(Box::new(pv))));
        Ok(())
    }

    pub fn preview_file(&self, path: &Path) -> Result<(), EngineError> {
        let audio = Arc::new(match path.to_str() {
            Some(s) if s.starts_with(crate::factory::SCHEME) => Audio::load_path(s, Path::new("."))?,
            _ => Audio::load(path)?,
        });
        let mut c = self.core.lock();
        let end = audio.frames as f64;
        send(
            &mut c,
            Cmd::Preview(Some(Box::new(Preview { audio, pos: 0.0, start: 0.0, end, gain: 0.8, fade_in: 0.0, fade_out: 0.0, rate: 1.0 }))),
        );
        Ok(())
    }

    pub fn preview_stop(&self) {
        let mut c = self.core.lock();
        send(&mut c, Cmd::Preview(None));
    }

    /// Plays a project source alone (Browser ▸ Project ▸ Audio), from its loaded audio: no
    /// file I/O, and a recorded take with no file of its own yet plays too.
    pub fn preview_source(&self, id: Id) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        let audio = c.sources.get(id).cloned().ok_or(EngineError::Msg("audio not loaded".into()))?;
        let end = audio.frames as f64;
        send(
            &mut c,
            Cmd::Preview(Some(Box::new(Preview { audio, pos: 0.0, start: 0.0, end, gain: 0.8, fade_in: 0.0, fade_out: 0.0, rate: 1.0 }))),
        );
        Ok(())
    }

    /// Panic: cut everything still ringing after a stop (see [`Cmd::Silence`]). Not an edit.
    pub fn silence(&self) {
        let mut c = self.core.lock();
        send(&mut c, Cmd::Silence);
    }

    /// Plays one note of a channel right away (step clicks, the piano-roll keyboard, Ctrl+click
    /// on a channel). `pattern`: the pattern being edited, so a channel that follows its clip's
    /// track is heard through that track. Not an edit.
    pub fn note_audition(&self, chan: Id, key: Option<u8>, vel: f32, pattern: Option<Id>) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        let ch = c.project.channel(chan).ok_or(EngineError::Msg(format!("no channel {chan}")))?;
        let key = key.unwrap_or(ch.root).min(127);
        // a one-shot rings out by itself; held sounds get a short note
        let len = 0.35;
        send(&mut c, Cmd::Audition { chan, key, vel: vel.clamp(0.0, 1.0), len, pattern });
        Ok(())
    }

    /// Loops one pattern on its own (`None` stops), from `from` beats into it and within
    /// `range` (beats; `None` = the whole pattern; kept inside the pattern). Starting it stops
    /// the song; starting the song stops it (they are exclusive, like FL Studio's PAT / SONG).
    /// Starting the pattern that plays again jumps to `from`. Not an edit.
    pub fn pattern_play(&self, pattern: Option<Id>, from: f64, range: Option<(f64, f64)>) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        if let Some(id) = pattern {
            if !c.project.patterns.iter().any(|p| p.id == id) {
                return Err(EngineError::Msg(format!("no pattern {id}")));
            }
            let song_notes = {
                let t = self.midi_take.lock();
                t.rec.is_some() && t.pat.is_none()
            };
            if c.take.is_some() || song_notes {
                return Err(EngineError::Msg("recording: stop it first".into()));
            }
        }
        if !from.is_finite() || range.is_some_and(|(a, b)| !a.is_finite() || !b.is_finite()) {
            return Err(EngineError::Msg("pattern position: not a number".into()));
        }
        // notes recorded over the loop so far join their pattern before it stops or restarts
        let recorded = self.finish_pat_take(&mut c);
        send(&mut c, Cmd::PatPlay { id: pattern, from: from.max(0.0), range });
        c.pat_loop = pattern.map(|id| (id, range));
        self.begin_pat_take(&mut c);
        if pattern.is_some() && c.transport.playing {
            c.transport.playing = false;
            self.changed(c);
        } else if recorded {
            self.changed(c);
        }
        Ok(())
    }

    /// Changes the range the pattern loop plays (`None` = the whole pattern) without
    /// restarting it: the position stays, or goes to the range's start when outside it.
    /// Does nothing while no pattern loops. Not an edit.
    pub fn pattern_loop(&self, range: Option<(f64, f64)>) -> Result<(), EngineError> {
        if range.is_some_and(|(a, b)| !a.is_finite() || !b.is_finite()) {
            return Err(EngineError::Msg("pattern loop: not a number".into()));
        }
        let mut c = self.core.lock();
        // notes recorded over the old range are folded into it before the range changes
        let recorded = self.finish_pat_take(&mut c);
        send(&mut c, Cmd::PatRange(range));
        if let Some(p) = c.pat_loop.as_mut() {
            p.1 = range;
        }
        self.begin_pat_take(&mut c);
        if recorded {
            self.changed(c);
        }
        Ok(())
    }

    // ------------------------------------------------------------ Synkopa Keys

    /// Hands the engine the loaded piano (or takes it away). Keys channels are silent
    /// without it; with it they play at once, also notes that are sounding.
    pub fn set_keys_bank(&self, bank: Option<Arc<crate::keys::KeysBank>>) {
        let mut c = self.core.lock();
        c.keys = bank;
        self.sync(&mut c, Dirty { timeline: true, mixer: false, params: false });
    }

    pub fn has_keys_bank(&self) -> bool {
        self.core.lock().keys.is_some()
    }

    /// True when the project has a Synkopa Keys channel.
    pub fn uses_keys(&self) -> bool {
        self.core.lock().project.channels.iter().any(|c| matches!(c.src, ChannelSrc::Keys { .. }))
    }

    // ------------------------------------------------------------ project files

    pub fn new_project(&self, project: Project) {
        self.vet_project(&project);
        let mut c = self.core.lock();
        if c.take.is_some() {
            return;
        }
        send(&mut c, Cmd::Stop);
        send(&mut c, Cmd::PatPlay { id: None, from: 0.0, range: None });
        c.transport.playing = false;
        let mut project = project;
        project.migrate();
        // ids start over in a new project: audio of the previous one must not answer to them
        let mut sources = SourceStore::default();
        load_library_sources(&project, &mut sources);
        c.sources = sources;
        c.source_errors.clear();
        c.project = project;
        c.undo.clear();
        c.redo.clear();
        c.audition.clear();
        c.path = None;
        c.revision += 1;
        c.saved_revision = c.revision;
        c.fx_errors.clear();
        // another project's channel may reuse an id: its plugin instruments start fresh
        self.forget_plugs(&mut c);
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        self.changed(c);
    }

    pub fn save(&self, path: &Path) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        self.snapshot_plugin_states(&mut c.project);
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut p = c.project.clone();
        // move recordings that live in the scratch folder next to the project
        let audio_dir = dir.join(format!("{} Audio", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
        let scratch = scratch_dir();
        for s in p.sources.iter_mut().filter(|s| !s.path.starts_with(crate::factory::SCHEME)) {
            let sp = PathBuf::from(&s.path);
            if sp.starts_with(&scratch) {
                let dst = audio_dir.join(sp.file_name().unwrap_or_default());
                std::fs::create_dir_all(&audio_dir).map_err(|e| e.to_string())?;
                std::fs::copy(&sp, &dst).map_err(|e| format!("copying {}: {e}", sp.display()))?;
                s.path = dst.display().to_string();
            }
            if let Ok(rel) = Path::new(&s.path).strip_prefix(dir) {
                s.path = rel.display().to_string();
            }
        }
        write_project_file(&p, path, true)?;
        // keep absolute paths in memory
        for s in c.project.sources.iter_mut() {
            let sp = PathBuf::from(&s.path);
            if sp.starts_with(&scratch) {
                s.path = audio_dir.join(sp.file_name().unwrap_or_default()).display().to_string();
            }
        }
        c.path = Some(path.to_path_buf());
        c.saved_revision = c.revision;
        self.changed(c);
        Ok(())
    }

    /// Writes the current project to `path` without making it the project's file: the
    /// path, the dirty flag and the sources stay as they are (recordings stay in the
    /// scratch folder, paths are absolute). Crash recovery uses it: the autosave never
    /// writes over the project's own file.
    pub fn save_copy(&self, path: &Path) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        self.snapshot_plugin_states(&mut c.project);
        let p = c.project.clone();
        drop(c);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        write_project_file(&p, path, false)
    }

    /// Exports the current project and its audio sources without changing the open document.
    pub fn export_project_zip(&self, path: &Path, if_revision: Option<u64>) -> Result<(), EngineError> {
        let c = self.core.lock();
        if c.take.is_some() || self.midi_take.lock().rec.is_some() {
            return Err(EngineError::Msg("recording: stop it first".into()));
        }
        if let Some(r) = if_revision {
            if r != c.revision {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        let origin = c.path.clone();
        let mut project = c.project.clone();
        self.snapshot_plugin_states(&mut project);
        drop(c);
        crate::project_zip::write(project, path, origin.as_deref()).map_err(EngineError::Msg)
    }

    /// Opens a copy written by [`Engine::save_copy`] as an unsaved project with unsaved
    /// changes: Save asks where to put it instead of overwriting the copy.
    pub fn open_recovered(&self, path: &Path) -> Result<Vec<String>, EngineError> {
        self.open_recovered_for(path, None)
    }

    /// [`Self::open_recovered`] of a copy of the project saved at `file`: the project is that
    /// file again, with the copy's changes unsaved, so Save writes them there and closing asks
    /// first. `None`: a project that was never saved (Save asks where to put it).
    pub fn open_recovered_for(&self, path: &Path, file: Option<&Path>) -> Result<Vec<String>, EngineError> {
        let warnings = self.open(path)?;
        let mut c = self.core.lock();
        c.path = file.map(Path::to_path_buf);
        c.revision += 1;
        self.changed(c);
        Ok(warnings)
    }

    /// Loads a project file. Missing audio is reported, not fatal.
    pub fn open(&self, path: &Path) -> Result<Vec<String>, EngineError> {
        self.open_at_revision(path, None)
    }

    /// Checks the expected revision after decoding, before replacing the current project.
    pub fn open_at_revision(&self, path: &Path, if_revision: Option<u64>) -> Result<Vec<String>, EngineError> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("not a Synkopa project: {e}"))?;
        let mut p = Project::from_json(doc.get("project").cloned().unwrap_or(doc)).map_err(|e| format!("not a Synkopa project: {e}"))?;
        p.migrate();
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut warnings = vec![];
        let mut errors = HashMap::new();
        let mut store = SourceStore::default();
        for s in p.sources.iter_mut() {
            // library paths stay portable; everything else is absolute in memory
            if !s.path.starts_with(crate::factory::SCHEME) {
                let sp = PathBuf::from(&s.path);
                let abs = if sp.is_absolute() { sp } else { dir.join(sp) };
                s.path = abs.display().to_string();
            }
            match Audio::load_path(&s.path, dir) {
                Ok(a) => store.insert(s.id, Arc::new(a)),
                Err(e) => {
                    errors.insert(s.id, e.clone());
                    warnings.push(e);
                }
            }
        }
        if p.inserts.first().map(|m| m.id) != Some(0) {
            return Err(EngineError::Msg("the project has no master insert".into()));
        }
        self.vet_project(&p);
        let mut c = self.core.lock();
        if c.take.is_some() || self.midi_take.lock().rec.is_some() {
            return Err(EngineError::Msg("recording: stop it first".into()));
        }
        if let Some(r) = if_revision {
            if r != c.revision {
                return Err(EngineError::Msg(format!("{ERR_REVISION_CONFLICT} project is at revision {}, not {r}", c.revision)));
            }
        }
        send(&mut c, Cmd::Stop);
        send(&mut c, Cmd::PatPlay { id: None, from: 0.0, range: None });
        c.transport.playing = false;
        c.sources = store;
        c.source_errors = errors;
        c.project = p;
        c.audition.clear();
        c.undo.clear();
        c.redo.clear();
        c.path = Some(path.to_path_buf());
        c.revision += 1;
        c.saved_revision = c.revision;
        c.fx_errors.clear();
        // another project's channel may reuse an id: its plugin instruments start fresh
        self.forget_plugs(&mut c);
        self.sync(&mut c, Dirty { timeline: true, mixer: true, params: true });
        self.changed(c);
        Ok(warnings)
    }

    // ------------------------------------------------------------ offline render

    /// Beat range the UI and CLI use for a song or loop bounce.
    ///
    /// `song` is always `0..song_end`. `loop` uses the project's loop when one
    /// exists; otherwise it falls back to the song so a missing loop never yields
    /// an empty range.
    pub fn export_range(project: &Project, use_loop: bool) -> (f64, f64) {
        if use_loop {
            if let Some(l) = project.loop_range {
                if l.b > l.a {
                    return (l.a, l.b);
                }
            }
        }
        (0.0, project.song_end())
    }

    /// Renders `[from, to)` beats (plus `tail` seconds) of the current project to
    /// interleaved stereo at `sample_rate`, independently of the live device.
    pub fn render(&self, from: f64, to: f64, tail: f64, sample_rate: u32) -> Result<Vec<f32>, EngineError> {
        self.render_cancelable(from, to, tail, sample_rate, &AtomicBool::new(false))
    }

    /// Same as [`Self::render`], but returns [`EngineError::Cancelled`] as soon as
    /// `cancel` is set. Checked between DSP blocks so a long bounce can stop without
    /// waiting for the whole file.
    pub fn render_cancelable(&self, from: f64, to: f64, tail: f64, sample_rate: u32, cancel: &AtomicBool) -> Result<Vec<f32>, EngineError> {
        self.render_export(None, from, to, RenderTail::Seconds(tail), sample_rate, cancel)
    }

    /// The mixer inserts a stem export writes a file for, in mixer order: every insert except the
    /// master that is not muted and has something routed into it (a track with an unmuted clip, or a
    /// channel sent to it). Like FL Studio's *Split mixer tracks*, unused and muted inserts are left
    /// out; a candidate that still renders silent is dropped by the caller.
    pub fn stem_inserts(project: &Project) -> Vec<(Id, String)> {
        let playing: Vec<Id> = project
            .inserts
            .iter()
            .filter(|m| m.id != 0 && !m.muted)
            .filter(|m| {
                let on_track = project.tracks.iter().enumerate().any(|(i, t)| {
                    !t.muted && project.clips.iter().any(|c| c.track as usize == i && !c.muted && project.clip_insert(c) == m.id)
                });
                let on_channel = project.channels.iter().any(|c| c.insert == Some(m.id) && !c.mute);
                on_track || on_channel
            })
            .map(|m| m.id)
            .collect();
        // a bus has nothing of its own: it has a stem when something playing is routed into it
        project
            .inserts
            .iter()
            .filter(|m| m.id != 0 && !m.muted && playing.iter().any(|&src| crate::routing::reaches(project, src, m.id)))
            .map(|m| (m.id, m.name.clone()))
            .collect()
    }

    /// Renders insert `insert` alone as a stem: what it plays after its own effects, fader, pan and
    /// width, before the master (the master's effects, fader, pan and width are skipped). Same range,
    /// tail, sample rate and delay compensation as [`Self::render`], so every stem lines up with the
    /// others and with the mix. Mute is respected (a muted track or channel stays silent); solo is
    /// ignored, so a soloed insert does not silence the other stems.
    pub fn render_stem(
        &self,
        insert: Id,
        from: f64,
        to: f64,
        tail: f64,
        sample_rate: u32,
        cancel: &AtomicBool,
    ) -> Result<Vec<f32>, EngineError> {
        self.render_export(Some(insert), from, to, RenderTail::Seconds(tail), sample_rate, cancel)
    }

    /// Render a mix or stem, optionally keeping the effect state from a previous loop pass.
    pub fn render_export(
        &self,
        stem: Option<Id>,
        from: f64,
        to: f64,
        tail: RenderTail,
        sample_rate: u32,
        cancel: &AtomicBool,
    ) -> Result<Vec<f32>, EngineError> {
        self.render_export_with(stem, from, to, tail, sample_rate, cancel, &mut |_| {})
    }

    /// [`Self::render_export`] that reports its progress to `progress` after every block.
    #[allow(clippy::too_many_arguments)]
    pub fn render_export_with(
        &self,
        stem: Option<Id>,
        from: f64,
        to: f64,
        tail: RenderTail,
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<Vec<f32>, EngineError> {
        self.render_bounce(stem, None, from, to, tail, sample_rate, cancel, progress)
    }

    /// [`Self::render_export_with`] of one pattern alone (`pattern`: FL Studio's pattern export,
    /// what PAT mode plays) or of the song (`None`). See [`pattern_project`] for what a pattern
    /// bounce hears; [`Self::pattern_range`] is its range. The project itself is not changed.
    #[allow(clippy::too_many_arguments)]
    pub fn render_bounce(
        &self,
        stem: Option<Id>,
        pattern: Option<Id>,
        from: f64,
        to: f64,
        tail: RenderTail,
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<Vec<f32>, EngineError> {
        if let Some(id) = pattern {
            if !self.core.lock().project.patterns.iter().any(|p| p.id == id) {
                return Err(EngineError::Msg(format!("no pattern {id}")));
            }
        }
        let scope = move |p: Project| {
            let p = match pattern {
                Some(id) => pattern_project(p, id),
                None => p,
            };
            match stem {
                Some(id) => stem_project(p, id),
                None => p,
            }
        };
        self.render_scoped(&scope, from, to, tail, sample_rate, cancel, progress)
    }

    /// Beat range of a pattern export: the whole pattern from its first beat (`None`: no such pattern).
    pub fn pattern_range(project: &Project, pattern: Id) -> Option<(f64, f64)> {
        project.patterns.iter().find(|p| p.id == pattern).map(|p| (0.0, p.len.max(BAR_BEATS)))
    }

    /// Renders what `scope` makes of the current project (a stem, the selected clips alone …).
    #[allow(clippy::too_many_arguments)]
    fn render_scoped(
        &self,
        scope: &dyn Fn(Project) -> Project,
        from: f64,
        to: f64,
        tail: RenderTail,
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<Vec<f32>, EngineError> {
        if !from.is_finite() || !to.is_finite() || to <= from {
            return Err(EngineError::Msg("render range must be finite and `to` must be after `from`".into()));
        }
        if sample_rate < 8000 {
            return Err(EngineError::Msg("sample rate must be at least 8000 Hz".into()));
        }
        if cancel.load(Relaxed) {
            return Err(EngineError::Cancelled);
        }
        let (project, mut store_snapshot, stretches, takes, keys) = {
            let mut guard = self.core.lock();
            let c = &mut *guard;
            self.snapshot_plugin_states(&mut c.project);
            let mut map = HashMap::new();
            let mut stretches = vec![];
            let bpm = c.project.bpm;
            for clip in &c.project.clips {
                if clip.kind == ClipKind::Audio {
                    match c.sources.stretch_request(clip, false, bpm) {
                        Some(Ok(a)) => {
                            map.insert(clip.id, a);
                        }
                        Some(Err(job)) => stretches.push((clip.id, job)),
                        None => {}
                    }
                }
            }
            let mut audio = comp_audio(&c.sources, &c.project.comps);
            audio.extend(sample_audio(&c.sources, &c.project.channels));
            (c.project.clone(), map, stretches, audio, c.keys.clone())
        };
        let project = scope(project);
        // time-stretches nobody played yet are rendered here, without holding the project
        let mut rendered: HashMap<crate::sources::StretchKey, (Arc<Audio>, f64)> = HashMap::new();
        let stretch_total = stretches.len();
        for (i, (clip_id, job)) in stretches.into_iter().enumerate() {
            progress(RenderProgress::Preparing { done: i, total: stretch_total });
            if cancel.load(Relaxed) {
                return Err(EngineError::Cancelled);
            }
            let got = match rendered.get(&job.key) {
                Some(x) => x.clone(),
                None => {
                    let r = crate::sources::render_stretch(&job);
                    let got = match &r {
                        Ok(a) => (a.clone(), job.rate),
                        Err(_) => (job.audio.clone(), job.fallback_rate),
                    };
                    self.core.lock().sources.put_stretch(job.key, r);
                    rendered.insert(job.key, got.clone());
                    got
                }
            };
            store_snapshot.insert(clip_id, got);
        }
        let sr = sample_rate as f32;
        let block = 512;
        let (mut cmd_tx, cmd_rx) = rtrb::RingBuffer::<Cmd>::new(64 + project.channels.len());
        let (gb_tx, mut gb_rx) = rtrb::RingBuffer::<Garbage>::new(64);
        let (rec_tx, _rec_rx) = rtrb::RingBuffer::<f32>::new(16);
        let mut taps = Taps { input: vec![], output: vec![] };
        let mut keep = vec![];
        for _ in 0..WATCH_SLOTS {
            let (a, b) = rtrb::RingBuffer::<f32>::new(16);
            let (c2, d) = rtrb::RingBuffer::<f32>::new(16);
            taps.input.push(a);
            taps.output.push(c2);
            keep.push((b, d));
        }
        let mut rt = Rt::new(sr, block, cmd_rx, gb_tx, Arc::new(Shared::new()), taps, rec_tx);
        rt.set_skip_silent(self.skip_silent.load(Relaxed));
        rt.set_workers(self.render_workers.load(Relaxed));
        let mut errs = HashMap::new();
        let mut hosted = HashMap::new();
        let graph = build_graph(&project, &HashSet::new(), sr, block, &*self.factory, false, &mut errs, &mut hosted);
        let mut tl = build_timeline(&project, &mut |clip| store_snapshot.get(&clip.id).cloned(), &takes, &HashMap::new(), keys.as_ref());
        let wrap = matches!(tail, RenderTail::Wrap);
        tl.loop_range = wrap.then_some((from, to));
        tl.song_end = TOTAL_BEATS;
        let _ = cmd_tx.push(Cmd::Graph(Box::new(graph)));
        for ch in &project.channels {
            if let ChannelSrc::Synth { synth } = &ch.src {
                let _ = cmd_tx.push(Cmd::SynthAdd(Box::new(crate::poly::SynthInst::new(ch.id, crate::poly::params::resolve(synth), sr))));
            }
        }
        // plugin instruments: the render's own instances, from the saved state, in offline mode
        for ch in &project.channels {
            if let ChannelSrc::Plugin { plugin } = &ch.src {
                match self.factory.create_instrument(ch.id, plugin, sr, block, false) {
                    Ok(inst) => {
                        let _ = cmd_tx.push(Cmd::PlugAdd(Box::new(crate::plug::PlugInst::new(ch.id, inst, block))));
                    }
                    Err(e) => log::warn!("render: plugin instrument '{}' is silent: {e}", plugin.name),
                }
            }
        }
        let _ = cmd_tx.push(Cmd::Timeline(Box::new(tl)));
        let _ = cmd_tx.push(Cmd::Play { from, count_in: 0.0, rec: false, rec_loop: false });
        let spb = 60.0 / project.bpm;
        let duration = (to - from) * spb;
        let play_frames = (duration * f64::from(sample_rate)) as usize;
        let (frames, warmup) = match tail {
            RenderTail::Seconds(tail) => (((duration + tail.max(0.0)) * f64::from(sample_rate)) as usize, 0),
            RenderTail::Wrap => {
                let period = (duration * f64::from(sample_rate)).round() as usize;
                if period == 0 {
                    return Err(EngineError::Msg("loop is shorter than one sample".into()));
                }
                (period, period)
            }
        };
        // plugin delay compensation delays everything by `pdc` frames: render that much
        // longer and drop it from the front, so the file starts on `from` and the last
        // `pdc` frames of the song are not cut off
        let pdc = playback_latency_with(&project, sr, &hosted);
        let too_long = || EngineError::Msg("render is too long".into());
        let discard = warmup.checked_add(pdc).ok_or_else(too_long)?;
        let total = frames.checked_add(discard).ok_or_else(too_long)?;
        let mut out = Vec::new();
        out.try_reserve_exact(frames.checked_mul(2).ok_or_else(too_long)?).map_err(|_| too_long())?;
        let mut l = vec![0f32; block];
        let mut r = vec![0f32; block];
        let mut done = 0;
        let mut stopped = false;
        let frames_per_beat = spb * f64::from(sample_rate);
        let beat_at = |done: usize| {
            let rendered = done.saturating_sub(pdc);
            let at = if wrap { rendered % frames } else { rendered };
            from + at as f64 / frames_per_beat
        };
        let peak_of = |l: &[f32], r: &[f32]| l.iter().chain(r).fold(0f32, |m, x| if x.abs() > m { x.abs() } else { m });
        let keep = |out: &mut Vec<f32>, at: usize, l: &[f32], r: &[f32]| {
            for i in discard.saturating_sub(at)..l.len() {
                out.push(l[i]);
                out.push(r[i]);
            }
        };
        while done < total {
            if cancel.load(Relaxed) {
                return Err(EngineError::Cancelled);
            }
            let mut n = block.min(total - done);
            if wrap {
                let pos = done % frames;
                if done != 0 && pos == 0 {
                    let _ = cmd_tx.push(Cmd::LoopTo(from));
                }
                n = n.min(frames - pos);
            } else if !stopped && done + n > play_frames {
                // stop exactly at `to`: render up to it, then let tails ring
                let k = play_frames.saturating_sub(done);
                if k > 0 {
                    rt.process(&mut l[..k], &mut r[..k], None);
                    keep(&mut out, done, &l[..k], &r[..k]);
                    done += k;
                    progress(RenderProgress::Rendering { done, total, peak: peak_of(&l[..k], &r[..k]), beat: beat_at(done) });
                }
                let _ = cmd_tx.push(Cmd::Stop);
                stopped = true;
                continue;
            }
            rt.process(&mut l[..n], &mut r[..n], None);
            keep(&mut out, done, &l[..n], &r[..n]);
            done += n;
            // the discarded warm-up and delay-compensation frames do not reach the file
            let peak = if done > discard { peak_of(&l[..n], &r[..n]) } else { 0.0 };
            progress(RenderProgress::Rendering { done, total, peak, beat: beat_at(done) });
            while gb_rx.pop().is_ok() {}
        }
        Ok(out)
    }

    // ------------------------------------------------------------ internals

    /// Drops every plugin-instrument instance (the next sync makes new ones from the saved
    /// states). The audio thread's are removed by command (a rebuilt stream has none).
    fn forget_plugs(&self, c: &mut Core) {
        for id in c.plugs.drain().map(|(id, _)| id).collect::<Vec<_>>() {
            self.factory.release(id);
            c.fx_errors.remove(&id);
            send(c, Cmd::PlugRemove(id));
        }
    }

    fn snapshot_plugin_states(&self, project: &mut Project) {
        let factory = &self.factory;
        for m in project.inserts.iter_mut() {
            for f in m.fx.iter_mut() {
                if let Some(pl) = f.plugin.as_mut() {
                    if let Some(st) = factory.state(f.id) {
                        pl.state = Some(st);
                    }
                }
            }
        }
        for ch in project.channels.iter_mut() {
            if let ChannelSrc::Plugin { plugin } = &mut ch.src {
                if let Some(st) = factory.state(ch.id) {
                    plugin.state = Some(st);
                }
            }
        }
    }

    /// Pushes the project to the audio thread.
    fn sync(&self, c: &mut Core, d: Dirty) {
        self.refresh_midi(c);
        let Some(w) = c.wires.as_ref() else { return };
        let (sr, max_block) = (w.sr, w.max_block);
        // synth instances first, so a new timeline never plays a channel without one
        sync_synths(c, sr);
        sync_plugs(c, sr, max_block, &*self.factory);
        // a take's alignment assumes the latency at its start: hold latency changes back
        let defer = d.mixer && c.take.as_ref().is_some_and(|t| t.pdc != playback_latency_with(&c.project, sr, &c.plugin_lat));
        c.graph_deferred |= defer;
        if d.mixer && !defer {
            c.graph_deferred = false;
            let now: HashSet<Id> = c.project.inserts.iter().flat_map(|m| m.fx.iter().map(|f| f.id)).collect();
            for gone in c.live_fx.difference(&now).copied().collect::<Vec<_>>() {
                self.factory.release(gone);
                c.fx_errors.remove(&gone);
            }
            let mut errs = std::mem::take(&mut c.fx_errors);
            let mut hosted = std::mem::take(&mut c.plugin_lat);
            let mut graph = build_graph(&c.project, &c.live_fx, sr, max_block, &*self.factory, true, &mut errs, &mut hosted);
            graph.monitor_track = monitor_of(c);
            c.fx_errors = errs;
            c.plugin_lat = hosted;
            c.live_fx = graph.inserts.iter().flat_map(|i| i.fx.iter().map(|s| s.id)).collect();
            c.sent_params = c.project.inserts.iter().flat_map(|m| m.fx.iter()).map(|f| (f.id, fx_send(f))).collect();
            // fresh processors got their maps in `build_graph`; carried ones keep what they had
            let fresh: HashSet<Id> = graph.inserts.iter().flat_map(|i| i.fx.iter()).filter(|s| s.proc.is_some()).map(|s| s.id).collect();
            c.sent_maps.retain(|id, _| now.contains(id) && !fresh.contains(id));
            for f in c.project.inserts.iter().flat_map(|m| m.fx.iter()) {
                if let (true, Some(m)) = (fresh.contains(&f.id), f.maps.as_ref()) {
                    c.sent_maps.insert(f.id, m.clone());
                }
            }
            send(c, Cmd::Graph(Box::new(graph)));
        } else if d.params || defer {
            let mut m = mix_state(&c.project);
            m.monitor_track = monitor_of(c);
            send(c, Cmd::Mix(Box::new(m)));
            let changed: Vec<(Id, (Option<ParamBlock>, f32))> = c
                .project
                .inserts
                .iter()
                .flat_map(|m| m.fx.iter())
                .map(|f| (f.id, fx_send(f)))
                .filter(|(id, pb)| c.sent_params.get(id) != Some(pb))
                .collect();
            for (id, (params, mix)) in changed {
                c.sent_params.insert(id, (params, mix));
                send(c, Cmd::Params { fx: id, params, mix });
            }
        }
        if d.mixer || d.params || defer {
            let changed: Vec<(Id, crate::model::WarpMaps)> = c
                .project
                .inserts
                .iter()
                .flat_map(|m| m.fx.iter())
                .filter_map(|f| f.maps.as_ref().map(|m| (f.id, m)))
                .filter(|(id, m)| c.sent_maps.get(id) != Some(*m))
                .map(|(id, m)| (id, m.clone()))
                .collect();
            for (id, m) in changed {
                send(c, Cmd::FxData { fx: id, data: Box::new(m.clone()) });
                c.sent_maps.insert(id, m);
            }
        }
        if d.timeline {
            let gesture_open = c.gesture.is_some();
            let audition = live_audition(c);
            let Core { project, sources, keys, .. } = c;
            let stale = gesture_open;
            let mut takes = comp_audio(sources, &project.comps);
            takes.extend(sample_audio(sources, &project.channels));
            let bpm = project.bpm;
            let tl = build_timeline(project, &mut |clip| sources.clip_play(clip, stale, bpm), &takes, &audition, keys.as_ref());
            send(c, Cmd::Timeline(Box::new(tl)));
        }
    }

    /// Asks the live hosted plugins for their latency again; when one changed, the
    /// mixer graph is rebuilt (running instances are kept) so the compensation follows.
    /// The factory is asked without the engine lock: it may wait for the UI thread.
    /// Returns whether anything changed.
    pub fn refresh_plugin_latency(&self) -> bool {
        let ids: Vec<Id> = {
            let c = self.core.lock();
            if c.wires.is_none() {
                return false;
            }
            c.plugin_lat.keys().copied().collect()
        };
        if ids.is_empty() {
            return false;
        }
        let now: Vec<(Id, usize)> = ids.into_iter().filter_map(|id| self.factory.latency(id).map(|l| (id, l))).collect();
        let mut c = self.core.lock();
        let mut changed = false;
        for (id, l) in now {
            if let Some(old) = c.plugin_lat.get_mut(&id) {
                if *old != l {
                    log::info!("effect {id}: plugin latency {old} -> {l} samples; re-planning delay compensation");
                    *old = l;
                    changed = true;
                }
            }
        }
        if changed {
            self.sync(&mut c, Dirty { mixer: true, ..Default::default() });
        }
        changed
    }

    /// Tries every hosted plugin that did not load once more (an effect or a plugin
    /// instrument with an `fx_errors` entry): after the user chose to load one that Synkopa's
    /// safety check refused, or installed what was missing. Running instances are kept.
    pub fn retry_plugins(&self) {
        let mut c = self.core.lock();
        let failed: Vec<Id> = c.fx_errors.keys().copied().collect();
        if failed.is_empty() {
            return;
        }
        for id in failed {
            // an instrument channel is made again by `sync_plugs`; an effect by the graph build,
            // which makes only the slots it does not already hold (a failed one is held empty)
            c.plugs.remove(&id);
            c.live_fx.remove(&id);
        }
        self.sync(&mut c, Dirty { mixer: true, ..Default::default() });
        self.changed(c);
    }

    /// Frames between the timeline and the speakers inside the mixer of the running audio
    /// half, hosted plugins included (`playback_latency` counts built-in effects only).
    pub fn playback_latency(&self) -> usize {
        let c = self.core.lock();
        let sr = c.wires.as_ref().map_or(48_000.0, |w| w.sr);
        playback_latency_with(&c.project, sr, &c.plugin_lat)
    }

    fn housekeeping(&self, tick: u32) {
        {
            // finished time-stretches replace their resampled stand-ins
            let mut c = self.core.lock();
            if c.sources.poll_stretch() {
                self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
                self.changed(c);
            }
        }
        // a plugin may change its latency (a look-ahead switch): re-plan the compensation
        if tick % 30 == 15 {
            self.refresh_plugin_latency();
        }
        let shared = self.shared();
        let frame_parts;
        {
            let mut guard = self.core.lock();
            let c = &mut *guard;
            let Some(w) = c.wires.as_mut() else { return };
            while w.garbage.pop().is_ok() {}
            let analysis = if c.watch.is_empty() { None } else { Some(w.analyzer.analyze(&c_watch(&c.watch), &shared)) };
            let w = c.wires.as_mut().unwrap();
            let sr = w.sr as u32;
            // drain recorded input
            let n = w.record.slots();
            if n > 0 {
                if let Ok(chunk) = w.record.read_chunk(n) {
                    let (a, b) = chunk.as_slices();
                    if let Some(t) = c.take.as_mut() {
                        for &x in a.iter().chain(b.iter()) {
                            t.push(x);
                        }
                    }
                    chunk.commit_all();
                }
            }
            let _ = sr;
            let ids: Vec<Id> = c.project.inserts.iter().map(|m| m.id).collect();
            if let Some(t) = c.take.as_mut() {
                t.resolve(&shared);
            }
            let rec = c.take.as_mut().filter(|t| !t.start.is_nan()).map(|t| {
                let new = t.peaks[t.peaks_sent..].to_vec();
                let off = t.peaks_sent / 2;
                t.peaks_sent = t.peaks.len();
                (t.track as u32, t.pass_start_beat(), (t.frames() - t.pass_start_frame()) as f32 / t.sr as f32, off as u32, new)
            });
            frame_parts = Some((ids, analysis, rec));
        }
        if shared.stopped_at_end.swap(false, Relaxed) {
            {
                let mut c = self.core.lock();
                c.transport.playing = false;
            }
            self.finish_take();
            self.finish_midi_take();
            let c = self.core.lock();
            self.changed(c);
        }
        if let Some((ids, analysis, rec)) = frame_parts {
            let sink = self.sink.lock();
            if let Some(sink) = sink.as_ref() {
                let frame = crate::telemetry::frame(tick, &shared, &ids, analysis.as_deref(), rec.as_ref());
                sink(frame);
            } else {
                // still consume meters so the first frame after subscribing is fresh
                for i in 0..ids.len().min(MAX_INSERTS) {
                    shared.take_meter(i);
                }
            }
        }
    }

    fn finish_take(&self) {
        let shared = self.shared();
        let mut c = self.core.lock();
        if let Some(t) = c.take.as_mut() {
            t.resolve(&shared);
        }
        self.finalize_take(&mut c);
        if c.graph_deferred {
            self.sync(&mut c, Dirty { timeline: false, mixer: true, params: true });
        }
    }

    fn finalize_take(&self, c: &mut Core) {
        let Some(mut take) = c.take.take() else { return };
        // whatever is still in the ring belongs to this recording
        if let Some(w) = c.wires.as_mut() {
            while let Ok(x) = w.record.pop() {
                take.push(x);
            }
        }
        if take.start.is_nan() {
            return; // punched in, but the audio thread never started capturing
        }
        if self.midi_take.lock().played {
            return; // the armed track was played on a MIDI keyboard: this is a MIDI take
        }
        let spb = 60.0 / c.project.bpm;
        let sr = take.sr as f64;
        // Latency compensation: input captured at frame `j` was sung to what was heard
        // `skip` frames earlier. Dropping the first `skip` frames moves the whole take
        // earlier; pass marks (written when the audio thread wrapped) then land exactly
        // on the loop start of what was heard, so they stay where they are.
        let skip = take.skip.min(take.frames());
        let data = &take.data[skip * 2..];
        let total = data.len() / 2;
        // one pass per loop cycle; a last pass cut off within a beat of the wrap is not a take
        let mut bounds = vec![0];
        bounds.extend(take.marks.iter().copied().take_while(|&m| m <= total));
        let complete = bounds.len() - 1;
        if bounds.last() != Some(&total) {
            bounds.push(total);
        }
        let mut passes = vec![];
        for k in 0..bounds.len() - 1 {
            let (f0, f1) = (bounds[k], bounds[k + 1]);
            let secs = f1.saturating_sub(f0) as f64 / sr;
            let cut_short = k > 0 && k >= complete;
            if secs < if cut_short { spb } else { 0.05 } {
                continue;
            }
            let start = if k == 0 { take.start } else { take.lap.map(|l| l.0).unwrap_or(take.start) };
            let full_len = take.lap.filter(|_| k < complete).map(|(_, end)| end - start);
            // Each loop boundary is rounded to a device frame. Normalize that rounding only;
            // a tempo or range edit during recording can change the actual pass length.
            let exact = full_len
                .map(|beats| (beats, (beats * spb * sr).round() as usize))
                .filter(|(_, frames)| *frames > 0 && frames.abs_diff(f1 - f0) <= 1);
            passes.push((start, exact, &data[f0 * 2..f1 * 2]));
        }
        if passes.is_empty() {
            return;
        }
        let before = c.project.clone();
        let dir = audio_dir(c);
        let mut landed = vec![];
        for (start, exact, data) in passes {
            let mut data = data.to_vec();
            if let Some((_, frames)) = exact {
                data.resize(frames * 2, 0.0);
            }
            let secs = data.len() as f64 / 2.0 / sr;
            let id = c.project.alloc_id();
            let n = c.project.sources.iter().filter(|s| s.name.starts_with("Take")).count() + 1;
            let name = format!("Take {n}");
            let path = fresh_path(&dir, &format!("{} {}", name, id));
            if let Err(e) = write_wav(&path, &data, take.sr) {
                log::error!("could not write recording: {e}");
            }
            let audio = Audio::from_stereo(data, take.sr, 1);
            c.project.sources.push(Source {
                id,
                name,
                color: "#e05263".into(),
                path: path.display().to_string(),
                dur: secs,
                sr: take.sr,
                ch: 1,
                peak_db: crate::dsp::gain_to_db(audio.peak()),
            });
            c.sources.insert(id, Arc::new(audio));
            landed.push((id, start, exact.map_or(secs / spb, |(beats, _)| beats).min(TOTAL_BEATS - start)));
        }
        crate::comp::land_takes(&mut c.project, take.track as u32, &landed);
        push_undo(c, before);
        c.revision += 1;
        self.sync(c, Dirty { timeline: true, ..Default::default() });
    }

    // ------------------------------------------------------------ live MIDI

    /// Sets what the MIDI keyboard plays. `target`: the channel (instrument) to play, `None` =
    /// the first pitched channel. `live`: a device is connected; without one nothing is
    /// recorded and playing stops. Anything still sounding is released when this changes.
    pub fn midi_configure(&self, target: Option<Id>, live: bool) {
        let mut c = self.core.lock();
        let changed = c.midi.target != target || c.midi.live != live;
        c.midi.target = target;
        c.midi.live = live;
        self.refresh_midi(&c);
        self.midi_fast.live.store(live, Release);
        if changed {
            self.midi_fast.send(Cmd::MidiOff);
        }
    }

    /// The channel the keyboard plays right now (the chosen one, else the first pitched
    /// channel, else the first channel).
    pub fn midi_target(&self) -> Option<Id> {
        midi_channel(&self.core.lock())
    }

    /// A note from the computer keyboard used as a piano (FL "typing keyboard"): plays `chan`
    /// like a MIDI key, through the same lock-free ring, whether or not a MIDI device is connected.
    pub fn typing_note(&self, chan: Id, key: u8, vel: f32, on: bool) {
        let ins = match self.midi_fast.ins.load(Relaxed) {
            usize::MAX => None,
            i => Some(i),
        };
        let vel = if on { vel.clamp(0.0, 1.0) } else { 0.0 };
        let key = key.min(127);
        self.midi_fast.send(Cmd::MidiNote { chan, key, vel, on, ins: if on { ins } else { None } });
        // while a take records notes, the computer keyboard plays into it like a MIDI keyboard
        let (song, pat) = (self.shared().beat(), self.shared().pat_beat());
        let mut t = self.midi_take.lock();
        let beat = t.beat(song, pat);
        if let Some((r, _, _)) = t.rec.as_mut() {
            if on {
                r.note_on_ch(beat, key, (vel * 127.0).round() as u8, Some(chan));
                t.played = true;
            } else {
                r.note_off_ch(beat, key, Some(chan));
            }
        }
    }

    /// Releases every sounding keyboard note (device unplugged).
    pub fn midi_panic(&self) {
        self.midi_fast.send(Cmd::MidiOff);
    }

    /// Publishes the channel and insert the keyboard plays for the lock-free event path.
    fn refresh_midi(&self, c: &Core) {
        self.midi_fast.chan.store(midi_channel(c).unwrap_or(u64::MAX), Relaxed);
        self.midi_fast.ins.store(armed_insert(c).unwrap_or(usize::MAX), Relaxed);
    }

    /// One decoded message from the keyboard. Plays through the audio thread's MIDI ring
    /// (no allocation there) and, while a take is armed, goes into the recorder at the
    /// playhead. Cheap and never waits for the core lock (the project being edited, saved
    /// or rebuilt); safe to call from the MIDI input thread.
    pub fn midi_event(&self, m: MidiMsg) {
        let mf = &self.midi_fast;
        if !mf.live.load(Acquire) {
            return;
        }
        let chan = match mf.chan.load(Relaxed) {
            u64::MAX => None,
            id => Some(id),
        };
        let ins = match mf.ins.load(Relaxed) {
            usize::MAX => None,
            i => Some(i),
        };
        let (song, pat) = (self.shared().beat(), self.shared().pat_beat());
        match m {
            MidiMsg::NoteOn { key, vel, .. } => {
                let Some(chan) = chan else { return };
                mf.send(Cmd::MidiNote { chan, key, vel: MidiMsg::vel_gain(vel), on: true, ins });
                let mut t = self.midi_take.lock();
                let beat = t.beat(song, pat);
                if let Some((r, _, _)) = t.rec.as_mut() {
                    r.note_on(beat, key, vel);
                    t.played = true;
                }
            }
            MidiMsg::NoteOff { key, .. } => {
                let Some(chan) = chan else { return };
                mf.send(Cmd::MidiNote { chan, key, vel: 0.0, on: false, ins: None });
                let mut t = self.midi_take.lock();
                let beat = t.beat(song, pat);
                if let Some((r, _, _)) = t.rec.as_mut() {
                    r.note_off(beat, key);
                }
            }
            MidiMsg::Sustain { down, .. } => {
                mf.send(Cmd::MidiSustain(down));
                let mut t = self.midi_take.lock();
                let beat = t.beat(song, pat);
                if let Some((r, _, _)) = t.rec.as_mut() {
                    r.sustain(beat, down);
                }
            }
            MidiMsg::PitchBend { value, .. } => mf.send(Cmd::MidiBend(MidiMsg::bend_semitones(value))),
            MidiMsg::ModWheel { value, .. } => mf.send(Cmd::MidiMod(value as f32 / 127.0)),
            MidiMsg::AllNotesOff { .. } => mf.send(Cmd::MidiOff),
        }
    }

    /// PAT mode with Record on and Notes ticked: what the keyboard plays over the pattern loop
    /// goes into that pattern (on the channel the keyboard plays), added to what it holds.
    fn begin_pat_take(&self, c: &mut Core) {
        let Some((id, range)) = c.pat_loop else { return };
        if !(c.transport.rec && c.transport.rec_notes) {
            return;
        }
        let Some(len) = c.project.patterns.iter().find(|p| p.id == id).map(|p| p.len) else { return };
        let Some(ch) = midi_channel(c) else { return };
        let (la, lb) = crate::rt::loop_span(range, len);
        let mut t = self.midi_take.lock();
        if t.rec.is_some() {
            return;
        }
        t.played = false;
        t.rec = Some((MidiRecorder::new(f64::NEG_INFINITY), usize::MAX, ch));
        t.pat = Some(PatLoopRec { id, la, span: lb - la, prev: f64::NEG_INFINITY, laps: 0.0 });
    }

    /// Ends a take over the pattern loop: the notes played join the pattern (one undo step).
    /// False when no such take runs.
    fn finish_pat_take(&self, c: &mut Core) -> bool {
        let pb = self.shared().pat_beat();
        let (rec, ch, mut pl) = {
            let mut t = self.midi_take.lock();
            let Some(pl) = t.pat.take() else { return false };
            t.played = false;
            let Some((rec, _, ch)) = t.rec.take() else { return true };
            (rec, ch, pl)
        };
        let end = pl.unwrap(pb);
        let mut played = rec.finish(end);
        // quantize in pattern beats: unwrapped beats are off the grid after the first pass
        // whenever the loop is not a whole number of grid steps
        if pl.span > 0.0 {
            for n in &mut played {
                let at = pl.la + (n.start - pl.la).rem_euclid(pl.span);
                n.end += at - n.start;
                n.start = at;
            }
        }
        midi::quantize(&mut played, c.transport.rec_quantize);
        let notes = midi::fold_loop(&played, ch, pl.la, pl.span);
        let Some(mut pat) = c.project.patterns.iter().find(|p| p.id == pl.id).cloned() else { return true };
        if notes.is_empty() {
            return true;
        }
        pat.notes.extend(notes);
        let before = c.project.clone();
        match ops::apply_all(&mut c.project, &[Op::PutPattern { pattern: pat }]) {
            Ok(d) => {
                push_undo(c, before);
                c.revision += 1;
                self.sync(c, d);
            }
            Err(e) => log::error!("could not add the recorded notes to the pattern: {e}"),
        }
        true
    }

    /// Starts collecting keyboard notes for a take on armed track `track` from beat `from`.
    fn begin_midi_take(&self, c: &mut Core, track: usize, from: f64) {
        let mut t = self.midi_take.lock();
        t.played = false;
        if !c.transport.rec_notes {
            return;
        }
        if let Some(ch) = midi_channel(c) {
            t.rec = Some((MidiRecorder::new(from), track, ch));
        }
    }

    /// Ends the MIDI take at the playhead: the played notes become a pattern and a pattern
    /// clip on the armed track (one undo step). Nothing played = nothing added.
    fn finish_midi_take(&self) {
        let end = self.shared().beat();
        let mut c = self.core.lock();
        if self.finish_pat_take(&mut c) {
            return;
        }
        let Some((rec, track, ch)) = ({
            let mut t = self.midi_take.lock();
            t.played = false;
            t.rec.take()
        }) else {
            return;
        };
        let mut notes = rec.finish(end);
        midi::quantize(&mut notes, c.transport.rec_quantize);
        let Some(laid) = midi::layout(&notes, ch, 4.0) else { return };
        let color = c.project.channel(ch).map(|x| x.color.clone()).unwrap_or_else(|| "#69c3d6".into());
        let n = c.project.patterns.iter().filter(|p| p.name.starts_with("MIDI take")).count() + 1;
        let pid = c.project.alloc_id();
        let cid = c.project.alloc_id();
        let pattern =
            Pattern { id: pid, name: format!("MIDI take {n}"), color, len: laid.len, min_len: laid.len, notes: laid.notes, swing: 0.0 };
        let clip = Clip {
            id: cid,
            track: track as u32,
            start: laid.start,
            len: laid.len,
            offset: 0.0,
            kind: ClipKind::Pat,
            source: pid,
            muted: false,
            gain: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            pan: 0.0,
            norm: false,
            rev: false,
            tune: None,
            stretch: 1.0,
            resample: false,
            pitch: 0.0,
            insert: None,
            root: crate::model::CLIP_ROOT,
        };
        if let Err(e) = self.apply_locked(c, &[Op::PutPattern { pattern }, Op::PutClips { clips: vec![clip] }], false, None) {
            log::error!("could not place the MIDI take: {e}");
        }
    }

    // ------------------------------------------------------------ comps

    /// Plays one whole take instead of the comp (`None` = back to the comp). Live only.
    pub fn audition(&self, comp: Id, take: Option<Id>) -> Result<(), EngineError> {
        let mut c = self.core.lock();
        let cp = c.project.comp(comp).ok_or(EngineError::Msg(format!("no comp {comp}")))?;
        match take {
            Some(t) if cp.take(t).is_none() => return Err(EngineError::Msg(format!("comp {comp} has no take {t}"))),
            Some(t) => {
                c.audition.insert(comp, t);
            }
            None => {
                c.audition.remove(&comp);
            }
        }
        self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
        self.changed(c);
        Ok(())
    }

    /// Renders what a comp clip plays into a new source and replaces the clip by an
    /// ordinary audio clip (gain, fades and pan stay on the clip). The take group goes
    /// when no other clip plays it. One undo step.
    pub fn flatten_comp(&self, clip_id: Id) -> Result<Id, EngineError> {
        let mut c = self.core.lock();
        let clip = c.project.clips.iter().find(|x| x.id == clip_id).cloned().ok_or(EngineError::Msg("no such clip".into()))?;
        if clip.kind != ClipKind::Comp {
            return Err(EngineError::Msg("only comp clips can be flattened".into()));
        }
        let comp = c.project.comp(clip.source).cloned().ok_or(EngineError::Msg("the clip's comp is missing".into()))?;
        let map = comp_audio(&c.sources, std::slice::from_ref(&comp));
        let first = comp.takes.iter().find_map(|t| map.get(&t.source)).ok_or(EngineError::Msg("the takes' audio is not loaded".into()))?;
        let (sr, ch) = (first.sr, comp.takes.iter().filter_map(|t| map.get(&t.source)).map(|a| a.channels).max().unwrap_or(1));
        let spb = 60.0 / c.project.bpm;
        let frames = (clip.len * spb * sr as f64).round() as usize;
        if frames < 16 {
            return Err(EngineError::Msg("the clip is empty".into()));
        }
        // two guard frames: the playhead's beat may land a hair before the clip end
        let frames = frames + 2;
        let rt = crate::comp::build(&comp, spb, None, &map);
        let (mut l, mut r) = (vec![0f32; frames], vec![0f32; frames]);
        rt.render_add(clip.offset, 1.0 / (spb * sr as f64), &mut l, &mut r, spb);
        let data: Vec<f32> = l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]).collect();
        let id = c.project.alloc_id();
        let name = format!("{} (flat)", comp.name);
        let path = fresh_path(&audio_dir(&c), &format!("{} flat {}", sanitize(&comp.name), id));
        write_wav(&path, &data, sr)?;
        let audio = Audio::from_stereo(data, sr, ch);
        let before = c.project.clone();
        c.project.sources.push(Source {
            id,
            name,
            color: "#e05263".into(),
            path: path.display().to_string(),
            dur: audio.seconds(),
            sr,
            ch,
            peak_db: crate::dsp::gain_to_db(audio.peak()),
        });
        c.sources.insert(id, Arc::new(audio));
        if let Some(x) = c.project.clips.iter_mut().find(|x| x.id == clip_id) {
            *x = Clip {
                kind: ClipKind::Audio,
                source: id,
                offset: 0.0,
                norm: false,
                rev: false,
                tune: None,
                stretch: 1.0,
                resample: false,
                ..clip.clone()
            };
        }
        if !c.project.clips.iter().any(|x| x.kind == ClipKind::Comp && x.source == comp.id) {
            c.project.comps.retain(|x| x.id != comp.id);
            c.audition.remove(&comp.id);
        }
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, ..Default::default() });
        self.changed(c);
        Ok(id)
    }
}

impl Engine {
    /// FL's *Export in place* (consolidate): renders the selected clips — through their mixer
    /// inserts and effects, without the master's — over the span they cover, saves the audio
    /// with the project's media and puts it as one audio clip on a new track right below the
    /// selection, routed to the master; the originals are muted. One undo step. Automation
    /// still applies; other clips are left out. No effect tail: the clip is exactly the span.
    pub fn export_in_place(
        &self,
        clip_ids: &[Id],
        sample_rate: u32,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<InPlace, EngineError> {
        let err = |m: &str| EngineError::Msg(m.into());
        let (picked, from, to) = {
            let c = self.core.lock();
            let picked: Vec<Clip> =
                c.project.clips.iter().filter(|x| clip_ids.contains(&x.id) && x.kind != ClipKind::Auto).cloned().collect();
            if picked.is_empty() {
                return Err(err("select the clips to export in place (automation clips alone make no sound)"));
            }
            let from = picked.iter().map(|x| x.start).fold(f64::INFINITY, f64::min);
            let to = picked.iter().map(|x| x.start + x.len).fold(f64::NEG_INFINITY, f64::max);
            (picked, from, to)
        };
        let ids: HashSet<Id> = picked.iter().map(|x| x.id).collect();
        let tracks: HashSet<u32> = picked.iter().map(|x| x.track).collect();
        let scope = |mut p: Project| {
            // only the selection sounds (automation keeps moving its targets); what the user
            // muted or soloed elsewhere does not decide what an explicit selection renders
            p.clips.retain(|x| ids.contains(&x.id) || x.kind == ClipKind::Auto);
            for x in p.clips.iter_mut() {
                if ids.contains(&x.id) {
                    x.muted = false;
                }
            }
            for (i, t) in p.tracks.iter_mut().enumerate() {
                t.solo = false;
                if tracks.contains(&(i as u32)) {
                    t.muted = false;
                }
            }
            for ch in p.channels.iter_mut() {
                ch.solo = false;
            }
            // the result plays into the master: rendering the master's chain too would apply it twice
            for m in p.inserts.iter_mut() {
                m.solo = false;
                if m.id == 0 {
                    m.fx.clear();
                    m.vol = crate::model::VOL_UNITY;
                    m.pan = 0.0;
                    m.width = 1.0;
                    m.muted = false;
                }
            }
            p
        };
        let data = self.render_scoped(&scope, from, to, RenderTail::Seconds(0.0), sample_rate, cancel, progress)?;
        let audio = Audio::from_stereo(data.clone(), sample_rate, 2);
        if audio.peak() < 1e-6 {
            return Err(err("the selection renders silent: nothing to export"));
        }
        let mut c = self.core.lock();
        let first = picked.iter().min_by(|a, b| a.track.cmp(&b.track).then(a.start.total_cmp(&b.start))).unwrap();
        let low = picked.iter().map(|x| x.track).max().unwrap();
        let name = match &picked[..] {
            [one] => format!("{} (in place)", clip_name(&c.project, one)),
            _ => format!("{} + {} (in place)", clip_name(&c.project, first), picked.len() - 1),
        };
        let color = clip_color(&c.project, first);
        let path = fresh_path(&audio_dir(&c), &sanitize(&name));
        write_wav(&path, &data, sample_rate)?;
        let before = c.project.clone();
        let source_id = c.project.alloc_id();
        c.project.sources.push(Source {
            id: source_id,
            name: name.clone(),
            color: color.clone(),
            path: path.display().to_string(),
            dur: audio.seconds(),
            sr: sample_rate,
            ch: 2,
            peak_db: crate::dsp::gain_to_db(audio.peak()),
        });
        c.sources.insert(source_id, Arc::new(audio));
        // the project may have changed while rendering: the track goes under where the selection is now
        let at = (low + 1).min(c.project.tracks.len() as u32);
        let h = c.project.tracks.get(low as usize).map_or(60.0, |t| t.h);
        let track_id = c.project.alloc_id();
        c.project.tracks.insert(
            at as usize,
            Track {
                id: track_id,
                name: name.clone(),
                color: color.clone(),
                muted: false,
                solo: false,
                arm: false,
                vol: VOL_UNITY,
                h,
                insert: 0,
                icon: None,
            },
        );
        for x in c.project.clips.iter_mut() {
            if x.track >= at {
                x.track += 1;
            }
            if ids.contains(&x.id) {
                x.muted = true;
            }
        }
        let clip_id = c.project.alloc_id();
        c.project.clips.push(Clip {
            id: clip_id,
            track: at,
            start: from,
            len: to - from,
            offset: 0.0,
            kind: ClipKind::Audio,
            source: source_id,
            muted: false,
            gain: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            pan: 0.0,
            norm: false,
            rev: false,
            tune: None,
            stretch: 1.0,
            resample: false,
            pitch: 0.0,
            insert: None,
            root: crate::model::CLIP_ROOT,
        });
        push_undo(&mut c, before);
        c.revision += 1;
        self.sync(&mut c, Dirty { timeline: true, mixer: true, ..Default::default() });
        self.changed(c);
        Ok(InPlace { source_id, clip_id, track: at })
    }
}

/// What [`Engine::export_in_place`] made: the new source, its clip and the track it is on.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InPlace {
    pub source_id: Id,
    pub clip_id: Id,
    pub track: u32,
}

/// A clip's name as the playlist shows it: its sample's or its pattern's.
fn clip_name(p: &Project, c: &Clip) -> String {
    match c.kind {
        ClipKind::Pat => p.patterns.iter().find(|x| x.id == c.source).map(|x| x.name.clone()),
        ClipKind::Comp => p.comp(c.source).map(|x| x.name.clone()),
        _ => p.sources.iter().find(|x| x.id == c.source).map(|x| x.name.clone()),
    }
    .unwrap_or_else(|| "Render".into())
}

fn clip_color(p: &Project, c: &Clip) -> String {
    match c.kind {
        ClipKind::Pat => p.patterns.iter().find(|x| x.id == c.source).map(|x| x.color.clone()),
        _ => p.sources.iter().find(|x| x.id == c.source).map(|x| x.color.clone()),
    }
    .unwrap_or_else(|| "#47b8a8".into())
}

/// Loaded audio of every take source of `comps`.
fn comp_audio(store: &SourceStore, comps: &[Comp]) -> HashMap<Id, Arc<Audio>> {
    comps.iter().flat_map(|cp| cp.takes.iter()).filter_map(|t| store.get(t.source).map(|a| (t.source, a.clone()))).collect()
}

/// Loaded audio of every sampler channel.
fn sample_audio(store: &SourceStore, chans: &[Channel]) -> HashMap<Id, Arc<Audio>> {
    chans
        .iter()
        .filter_map(|c| match c.src {
            ChannelSrc::Sample { source, .. } => store.get(source).map(|a| (source, a.clone())),
            ChannelSrc::Synth { .. } | ChannelSrc::Keys { .. } | ChannelSrc::Plugin { .. } => None,
        })
        .collect()
}

/// Decodes the sample-library sources of a project that did not come from a file (a new
/// project's starter kit, the demo project). A kit that is not installed leaves its channels silent.
fn load_library_sources(p: &Project, store: &mut SourceStore) {
    for s in p.sources.iter().filter(|s| s.path.starts_with(crate::factory::SCHEME)) {
        if let Ok(a) = Audio::load_path(&s.path, Path::new(".")) {
            store.insert(s.id, Arc::new(a));
        }
    }
}

/// Auditions that still point at an existing take.
fn live_audition(c: &Core) -> HashMap<Id, Id> {
    c.audition.iter().filter(|(cp, t)| c.project.comp(**cp).is_some_and(|x| x.take(**t).is_some())).map(|(a, b)| (*a, *b)).collect()
}

fn fx_send(f: &Fx) -> (Option<ParamBlock>, f32) {
    (if f.kind == FxKind::Plugin { None } else { Some(param_block(f)) }, f.mix.clamp(0.0, 1.0))
}

fn c_watch(w: &[Id]) -> Vec<Id> {
    w.to_vec()
}

/// The name without a trailing " #n" (the number "Make unique" adds).
fn unique_base(name: &str) -> &str {
    match name.rsplit_once(" #") {
        Some((b, n)) if !b.is_empty() && !n.is_empty() && n.bytes().all(|d| d.is_ascii_digit()) => b,
        _ => name,
    }
}

/// Sources the project has whose audio is not loaded, with why (see [`StateDto::missing`]).
fn missing_sources(c: &Core) -> HashMap<Id, String> {
    c.project
        .sources
        .iter()
        .filter(|s| c.sources.get(s.id).is_none())
        .map(|s| (s.id, c.source_errors.get(&s.id).cloned().unwrap_or_else(|| format!("{}: not loaded", s.path))))
        .collect()
}

/// True when `ops` changed nothing but lane heights: every op puts a track, and `after` is
/// `before` with other heights.
fn only_track_heights(ops: &[Op], before: &Project, after: &Project) -> bool {
    if ops.is_empty() || !ops.iter().all(|op| matches!(op, Op::PutTrack { .. })) || before.tracks.len() != after.tracks.len() {
        return false;
    }
    let same_but_height = |a: &Track, b: &Track| {
        let mut a = a.clone();
        a.h = b.h;
        a == *b
    };
    before.tracks.iter().zip(&after.tracks).all(|(a, b)| same_but_height(a, b))
}

/// True when `a` and `b` (the lane at one id in two consecutive history states) are the same
/// track: the same name, or everything but the name (and height) the same, a rename. Lanes that
/// swapped places (Move up / down keeps ids with their slots) differ in more than the name.
fn same_lane(a: &Track, b: &Track) -> bool {
    if a.name == b.name {
        return true;
    }
    let mut a = a.clone();
    a.name.clone_from(&b.name);
    a.h = b.h;
    a == *b
}

/// Gives the lane `now` its current height in each history state of `states`, nearest first,
/// until a state where that lane holds another track (or no lane has its id).
fn carry_height<'a>(now: &Track, states: impl Iterator<Item = &'a mut Project>) {
    let mut prev = now.clone();
    for p in states {
        let Some(t) = p.tracks.iter_mut().find(|t| t.id == now.id) else { break };
        if !same_lane(&prev, t) {
            break;
        }
        prev = t.clone();
        t.h = now.h;
    }
}

fn push_undo(c: &mut Core, before: Project) {
    // a file imported for this edit (and nothing else since) is undone with it
    let before = match c.import_base.take() {
        Some((r, b)) if r == c.revision => b,
        _ => before,
    };
    c.undo.push(before);
    if c.undo.len() > UNDO_LIMIT {
        c.undo.remove(0);
    }
    c.redo.clear();
}

/// The channel the keyboard plays: the chosen one if it exists, else the first pitched
/// channel, else the first channel.
fn midi_channel(c: &Core) -> Option<Id> {
    let p = &c.project;
    c.midi.target.filter(|id| p.channel(*id).is_some()).or_else(|| p.channels.first().map(|x| x.id))
}

/// Mixer insert index of the first armed track (where the keyboard is heard through).
fn armed_insert(c: &Core) -> Option<usize> {
    let p = &c.project;
    let t = p.tracks.iter().find(|t| t.arm)?;
    p.inserts.iter().position(|m| m.id == t.insert)
}

/// Brings the audio thread's synth instances in line with the project's synth channels:
/// new channels get an instance (built here, off the audio thread), changed ones their new
/// patch, removed ones are dropped.
fn sync_synths(c: &mut Core, sr: f32) {
    let want: Vec<(Id, crate::poly::Patch)> = c
        .project
        .channels
        .iter()
        .filter_map(|ch| match &ch.src {
            ChannelSrc::Synth { synth } => Some((ch.id, crate::poly::params::resolve(synth))),
            _ => None,
        })
        .collect();
    let gone: Vec<Id> = c.synths.keys().filter(|id| !want.iter().any(|(w, _)| w == *id)).copied().collect();
    for id in gone {
        c.synths.remove(&id);
        send(c, Cmd::SynthRemove(id));
    }
    for (id, patch) in want {
        match c.synths.get(&id) {
            None => {
                c.synths.insert(id, patch);
                send(c, Cmd::SynthAdd(Box::new(crate::poly::SynthInst::new(id, patch, sr))));
            }
            Some(old) if *old != patch => {
                c.synths.insert(id, patch);
                send(c, Cmd::SynthPatch { chan: id, patch: Box::new(patch) });
            }
            _ => {}
        }
    }
}

/// Brings the audio thread's plugin instruments in line with the project's plugin channels:
/// a new channel (or one whose plugin changed) gets an instance made here, off the audio
/// thread, from its saved state; a removed one is released. A plugin that does not load
/// leaves its channel silent with an `fx_errors` entry under the channel id; the channel and
/// its saved state stay in the project.
fn sync_plugs(c: &mut Core, sr: f32, max_block: usize, factory: &dyn PluginFactory) {
    let gone: Vec<Id> = c
        .plugs
        .iter()
        .filter(|(id, had)| !c.project.channel(**id).and_then(Channel::plugin).is_some_and(|p| p.same_class(had)))
        .map(|(id, _)| *id)
        .collect();
    for id in gone {
        c.plugs.remove(&id);
        c.fx_errors.remove(&id);
        factory.release(id);
        send(c, Cmd::PlugRemove(id));
    }
    let want: Vec<(Id, PluginRef)> = c
        .project
        .channels
        .iter()
        .filter_map(|ch| ch.plugin().filter(|_| !c.plugs.contains_key(&ch.id)).map(|p| (ch.id, p.clone())))
        .collect();
    for (id, plugin) in want {
        match factory.create_instrument(id, &plugin, sr, max_block, true) {
            Ok(inst) => {
                c.fx_errors.remove(&id);
                send(c, Cmd::PlugAdd(Box::new(crate::plug::PlugInst::new(id, inst, max_block))));
            }
            Err(e) => {
                log::warn!("plugin instrument '{}' did not load: {e}", plugin.name);
                c.fx_errors.insert(id, e);
            }
        }
        c.plugs.insert(id, plugin);
    }
}

fn send(c: &mut Core, cmd: Cmd) {
    if let Some(w) = c.wires.as_mut() {
        if w.cmds.push(cmd).is_err() {
            log::warn!("audio command queue full; dropping a command");
        }
    }
}

fn wait_for_take_input(shared: &Shared, timeout: Duration) {
    let start = std::time::Instant::now();
    while shared.finishing_take.load(Acquire) {
        if start.elapsed() >= timeout {
            // A disconnected or stalled device must not prevent Stop from completing.
            shared.finishing_take.store(false, Release);
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn watch_cmd(ids: &[Id]) -> Cmd {
    let mut a = [u64::MAX; WATCH_SLOTS];
    for (i, id) in ids.iter().take(WATCH_SLOTS).enumerate() {
        a[i] = *id;
    }
    Cmd::Watch { ids: a, n: ids.len().min(WATCH_SLOTS) as u8 }
}

/// `Song.synkopa` -> `Song (backup).synkopa`: the previous save, kept next to the project.
/// The backup keeps the project's own extension, so a `.wave` project from before the rename
/// gets a `.wave` backup.
/// It is an ordinary project file, so File > Open shows and opens it.
pub fn backup_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "synkopa".into());
    path.with_file_name(format!("{stem} (backup).{ext}"))
}

pub(crate) fn project_document(p: &Project) -> serde_json::Value {
    serde_json::json!({ "format": "wave-project", "version": 2, "project": p })
}

/// Serializes `p` and replaces `path` atomically (temp file + rename). With `backup`,
/// the file being replaced is kept as [`backup_path`] first; a failed backup never
/// blocks the save.
fn write_project_file(p: &Project, path: &Path, backup: bool) -> Result<(), EngineError> {
    let text = serde_json::to_string_pretty(&project_document(p)).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("wave.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    if backup && path.is_file() {
        if let Err(e) = std::fs::copy(path, backup_path(path)) {
            log::warn!("could not keep a backup of {}: {e}", path.display());
        }
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn scratch_dir() -> PathBuf {
    dirs_data().join("WAVE").join("Recordings")
}

fn dirs_data() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(p) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(p);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join("Library/Application Support");
        }
    }
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(p);
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".local/share");
    }
    std::env::temp_dir()
}

fn audio_dir(c: &Core) -> PathBuf {
    match &c.path {
        Some(p) => p
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("{} Audio", p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())),
        None => scratch_dir(),
    }
}

/// `dir/stem.wav`, or `dir/stem (2).wav` … when that file exists: ids restart in every new
/// project and unsaved projects share the scratch folder, so a new take must never
/// overwrite audio another project (or an older save of this one) still points at.
fn fresh_path(dir: &Path, stem: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.wav"));
    let mut k = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({k}).wav"));
        k += 1;
    }
    p
}

/// The project as a pattern export hears it, like PAT mode plays it: pattern `id` once from beat 0
/// and nothing else of the arrangement (no other clip, no automation clip, no take). Its channels play
/// into their own inserts, or the master when they have none, never through a playlist track's insert.
pub fn pattern_project(mut p: Project, id: Id) -> Project {
    let len = p.patterns.iter().find(|x| x.id == id).map_or(BAR_BEATS, |x| x.len.max(BAR_BEATS));
    p.clips.clear();
    p.comps.clear();
    p.loop_range = None;
    if p.tracks.is_empty() {
        p.tracks.push(Track {
            id: 1,
            name: "Pattern".into(),
            color: "#4f8ef7".into(),
            muted: false,
            solo: false,
            arm: false,
            vol: VOL_UNITY,
            h: 56.0,
            insert: 0,
            icon: None,
        });
    }
    for (i, t) in p.tracks.iter_mut().enumerate() {
        t.solo = false;
        if i == 0 {
            t.muted = false;
            t.vol = VOL_UNITY;
            t.insert = 0;
        }
    }
    let cid = p.alloc_id();
    p.clips.push(Clip {
        id: cid,
        track: 0,
        start: 0.0,
        len,
        offset: 0.0,
        kind: ClipKind::Pat,
        source: id,
        muted: false,
        gain: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        pan: 0.0,
        norm: false,
        rev: false,
        tune: None,
        stretch: 1.0,
        resample: false,
        pitch: 0.0,
        insert: None,
        root: crate::model::CLIP_ROOT,
    });
    p
}

/// The project as a stem export hears it: only insert `id` is audible, nothing is soloed, and the
/// master is a plain unity sum (no effects, fader at 0 dB, centre, normal width). Inserts keep their
/// effects so delay compensation stays the same as in the mix and every stem lines up.
fn stem_project(mut p: Project, id: Id) -> Project {
    for t in p.tracks.iter_mut() {
        t.solo = false;
    }
    for c in p.channels.iter_mut() {
        c.solo = false;
    }
    // the inserts routed into this one (its sources, through any number of sends): they stay,
    // sending only on the way here; this one goes straight to the master; the rest are muted
    let feeds: Vec<Id> =
        p.inserts.iter().filter(|m| m.id != 0 && m.id != id && crate::routing::reaches(&p, m.id, id)).map(|m| m.id).collect();
    for m in p.inserts.iter_mut() {
        m.solo = false;
        if m.id == 0 {
            m.fx.clear();
            m.vol = crate::model::VOL_UNITY;
            m.pan = 0.0;
            m.width = 1.0;
            m.muted = false;
        } else if m.id == id {
            m.muted = false;
            m.sends.clear();
            m.master = true;
        } else if feeds.contains(&m.id) {
            m.sends.retain(|s| s.to == id || feeds.contains(&s.to));
            m.master = false;
        } else {
            m.muted = true;
        }
    }
    p
}

fn sanitize(s: &str) -> String {
    s.chars().map(|ch| if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' { ch } else { '_' }).collect()
}

fn audible_tracks(p: &Project) -> Vec<(f32, bool)> {
    let any_solo = p.tracks.iter().any(|t| t.solo);
    p.tracks.iter().map(|t| (fader_gain(t.vol), !t.muted && (!any_solo || t.solo))).collect()
}

fn audible_inserts(p: &Project) -> Vec<(Id, f32, f32, f32, bool)> {
    // solo follows the routing: a soloed bus keeps what feeds it, a soloed source its way out
    let heard = crate::routing::audible(p);
    p.inserts
        .iter()
        .zip(heard)
        .map(|(m, heard)| {
            (m.id, fader_gain(m.vol), m.pan.clamp(-1.0, 1.0), if m.width.is_finite() { m.width.clamp(0.0, 2.0) } else { 1.0 }, heard)
        })
        .collect()
}

/// Frames between the timeline and the speakers inside the mixer: the slowest path through the
/// inserts to the master (plugin delay compensation lines every path up with it, see
/// `crate::routing::latency`) plus the master chain.
/// Built-in effects only: hosted plugins count as 0 here (see [`Engine::playback_latency`]).
pub fn playback_latency(p: &Project, sr: f32) -> usize {
    playback_latency_with(p, sr, &HashMap::new())
}

/// [`playback_latency`] with the latency each hosted plugin reports (`hosted`, by effect id).
pub(crate) fn playback_latency_with(p: &Project, sr: f32, hosted: &HashMap<Id, usize>) -> usize {
    let lat: Vec<usize> = p.inserts.iter().map(|m| m.fx.iter().map(|f| fx_latency(f, sr, hosted)).sum()).collect();
    let routes = crate::routing::latency(p, &crate::routing::order(p), &lat);
    lat.first().copied().unwrap_or(0) + routes.at_master
}

/// The delay one effect adds: `catalog::latency` for built-in effects, the reported value
/// for hosted plugins (0 when bypassed or not loaded).
fn fx_latency(f: &crate::model::Fx, sr: f32, hosted: &HashMap<Id, usize>) -> usize {
    match f.kind {
        FxKind::Plugin if f.bypass => 0,
        FxKind::Plugin => hosted.get(&f.id).copied().unwrap_or(0),
        _ => crate::catalog::latency(f, sr),
    }
}

fn monitor_track(p: &Project) -> Option<usize> {
    p.tracks.iter().position(|t| t.arm)
}

/// The track the live input is monitored through: the armed one, else the one being recorded
/// on. `None`: the input is heard dry on the master.
fn monitor_of(c: &Core) -> Option<usize> {
    monitor_track(&c.project).or_else(|| c.take.as_ref().map(|t| t.track))
}

/// The first track with no clip between beats `from` and `until`.
pub fn free_track(p: &Project, from: f64, until: f64) -> Option<usize> {
    (0..p.tracks.len()).find(|&i| !p.clips.iter().any(|c| c.track as usize == i && c.start < until - 1e-9 && c.start + c.len > from + 1e-9))
}

/// Removes track `id` if no clip is on it (the tracks below move up). True when it did.
fn drop_empty_track(p: &mut Project, id: Id) -> bool {
    let Some(i) = p.tracks.iter().position(|t| t.id == id) else { return false };
    if p.clips.iter().any(|c| c.track as usize == i) {
        return false;
    }
    p.tracks.remove(i);
    for c in p.clips.iter_mut().filter(|c| c.track as usize > i) {
        c.track -= 1;
    }
    true
}

fn mix_state(p: &Project) -> MixState {
    let sends = p
        .inserts
        .iter()
        .flat_map(|m| {
            m.sends.iter().map(move |s| (m.id, s.to, if s.gain.is_finite() { s.gain.clamp(0.0, crate::routing::SEND_MAX) } else { 1.0 }))
        })
        .collect();
    MixState { inserts: audible_inserts(p), sends, tracks: audible_tracks(p), monitor_track: monitor_track(p), chans: audible_channels(p) }
}

/// (gain, pan, audible) of every channel, in project order. Solo works like the mixer's:
/// with any channel soloed, only soloed channels play.
fn audible_channels(p: &Project) -> Vec<(f32, f32, bool)> {
    let any_solo = p.channels.iter().any(|c| c.solo);
    p.channels.iter().map(|c| (c.gain(), c.pan.clamp(-1.0, 1.0), !c.mute && (!any_solo || c.solo))).collect()
}

/// The channel table and the patterns for the audio thread. Swing is applied here (when
/// the timeline is built, i.e. at schedule time, never written into the notes); notes of
/// unknown channels and notes past the pattern end are left out.
fn build_patterns(p: &Project, audio: &HashMap<Id, Arc<Audio>>, keys: Option<&Arc<crate::keys::KeysBank>>) -> (Vec<ChanRt>, Vec<PatRt>) {
    let ins_idx = |id: Id| p.inserts.iter().position(|m| m.id == id);
    let track_ins = |track: u32| p.tracks.get(track as usize).and_then(|t| ins_idx(t.insert));
    // first clip (by time) of each pattern → its track's insert
    let mut first: HashMap<Id, (f64, u32)> = HashMap::new();
    for c in p.clips.iter().filter(|c| c.kind == ClipKind::Pat) {
        let e = first.entry(c.source).or_insert((c.start, c.track));
        if c.start < e.0 {
            *e = (c.start, c.track);
        }
    }
    let pat_home = |pid: Id| first.get(&pid).and_then(|&(_, t)| track_ins(t));
    let gains = audible_channels(p);
    let chans: Vec<ChanRt> = p
        .channels
        .iter()
        .zip(gains)
        .map(|(c, (gain, pan, on))| {
            let insert = c.insert.and_then(ins_idx);
            let heard = p.patterns.iter().filter(|pt| pt.notes.iter().any(|n| n.ch == c.id)).find_map(|pt| pat_home(pt.id));
            let (kind, sample) = match c.src {
                ChannelSrc::Sample { source, .. } => (crate::synth::VoiceKind::Sample, audio.get(&source).cloned()),
                ChannelSrc::Synth { .. } => (crate::synth::VoiceKind::Synth, None),
                ChannelSrc::Keys { .. } => (crate::synth::VoiceKind::Keys, None),
                ChannelSrc::Plugin { .. } => (crate::synth::VoiceKind::Plugin, None),
            };
            let ks = match c.src {
                ChannelSrc::Keys { keys: k } => Some(k),
                _ => None,
            };
            let (keys_release, keys_dynamics) = ks.map(|k| (k.release / 1000.0, k.dynamics / 100.0)).unwrap_or((0.35, 1.0));
            let smp = match c.src {
                ChannelSrc::Sample { source, sampler } => {
                    SmpRt::new(&sampler, p.sources.iter().find(|s| s.id == source).map(|s| s.peak_db))
                }
                _ => SmpRt::default(),
            };
            ChanRt {
                id: c.id,
                kind,
                sample,
                smp,
                keys: ks.and(keys.cloned()),
                keys_release,
                keys_dynamics,
                root: c.root,
                gain,
                pan,
                gain0: gain,
                pan0: pan,
                on,
                insert,
                heard,
                home: insert.or(heard).unwrap_or(0),
            }
        })
        .collect();
    let pats = p
        .patterns
        .iter()
        .map(|pt| {
            let end = (pt.len * 4.0) as f32;
            let notes = pt
                .notes
                .iter()
                .filter(|n| n.step < end - 1e-4)
                .filter_map(|n| {
                    let ci = p.channels.iter().position(|c| c.id == n.ch)?;
                    let step = n.step + swing_offset(n.step, pt.swing);
                    Some(NoteRt {
                        beat: step as f64 / 4.0,
                        len: n.len as f64 / 4.0,
                        sampler_trigger: if chans[ci].kind == crate::synth::VoiceKind::Sample { n.trig } else { None },
                        chan: ci,
                        key: n.p.unwrap_or(p.channels[ci].root),
                        vel: n.v.clamp(0.0, 1.0),
                    })
                })
                .collect();
            PatRt { id: pt.id, len: pt.len, notes, home: pat_home(pt.id) }
        })
        .collect();
    (chans, pats)
}

/// Builds the mixer graph. Effects in `live` are left as `proc: None` so the audio
/// thread keeps the running instance; everything else is created and prepared here.
///
/// `hosted`: the latency of every hosted plugin by effect id. Read for the instances in
/// `live` (they keep running, and so keep the latency they had), written for the ones
/// created here (what the new processor reports), pruned of effects that are gone. Plugin
/// delay compensation uses it the way it uses `catalog::latency` for built-in effects.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_graph(
    p: &Project,
    live: &HashSet<Id>,
    sr: f32,
    max_block: usize,
    factory: &dyn PluginFactory,
    live_instances: bool,
    errs: &mut HashMap<Id, String>,
    hosted: &mut HashMap<Id, usize>,
) -> Graph {
    let ins_state = audible_inserts(p);
    // processors first: a hosted plugin's latency is only known once it exists
    let procs: Vec<Vec<Option<Box<dyn Processor>>>> = p
        .inserts
        .iter()
        .map(|m| {
            m.fx.iter()
                .map(|f| {
                    if live.contains(&f.id) {
                        None
                    } else if f.kind == FxKind::Plugin {
                        match factory.create(f.id, f.plugin.as_ref().unwrap(), sr, max_block, live_instances) {
                            Ok(p) => {
                                errs.remove(&f.id);
                                hosted.insert(f.id, p.latency_samples());
                                Some(p)
                            }
                            Err(e) => {
                                errs.insert(f.id, e);
                                hosted.remove(&f.id);
                                None
                            }
                        }
                    } else {
                        let mut pr = crate::dsp::create(f.kind).unwrap();
                        pr.prepare(sr, max_block);
                        pr.set_params(&param_block(f));
                        if let Some(m) = &f.maps {
                            let _ = pr.swap_data(Box::new(m.clone()));
                        }
                        Some(pr)
                    }
                })
                .collect()
        })
        .collect();
    hosted.retain(|id, _| p.find_fx(*id).is_some_and(|(_, f)| f.kind == FxKind::Plugin));
    let lat: Vec<usize> = p.inserts.iter().map(|m| m.fx.iter().map(|f| fx_latency(f, sr, hosted)).sum()).collect();
    let order = crate::routing::order(p);
    let routes = crate::routing::latency(p, &order, &lat);
    let fed: Vec<bool> = (0..p.inserts.len()).map(|k| order.sends.iter().any(|v| v.iter().any(|s| s.0 == k))).collect();
    let inserts = p
        .inserts
        .iter()
        .zip(ins_state)
        .zip(procs)
        .map(|((m, (id, gain, pan, width, on)), procs)| {
            let fx =
                m.fx.iter()
                    .zip(procs)
                    .map(|(f, proc)| {
                        let params = if f.kind == FxKind::Plugin { None } else { Some(param_block(f)) };
                        FxSlot::new(f.id, f.bypass, f.mix.clamp(0.0, 1.0), proc, params, fx_latency(f, sr, hosted))
                    })
                    .collect();
            let k = p.inserts.iter().position(|x| x.id == id).unwrap_or(0);
            let sends = order.sends[k].iter().zip(&routes.sends[k]).map(|(&(t, g), &d)| SendRt::new(t, p.inserts[t].id, g, d)).collect();
            InsertRt::new(id, gain, pan, width, on, fx, max_block).with_delay(routes.master[k]).with_routing(
                order.level[k],
                k == 0 || m.master,
                sends,
                fed[k],
                routes.input[k],
                max_block,
            )
        })
        .collect();
    let track_state = audible_tracks(p);
    let tracks = p
        .tracks
        .iter()
        .zip(track_state)
        .map(|(t, (gain, on))| TrackRt { insert: p.inserts.iter().position(|m| m.id == t.insert).unwrap_or(0), gain, on })
        .collect();
    Graph::new(inserts, tracks, monitor_track(p), max_block).with_latency(routes.at_master, lat.first().copied().unwrap_or(0))
}

/// `takes`: loaded audio by source id of every comp take and sampler channel.
/// What an audio clip plays and the rate it reads it at (`SourceStore::clip_play`).
type ClipAudioFn<'a> = dyn FnMut(&Clip) -> Option<(Arc<Audio>, f64)> + 'a;

pub(crate) fn build_timeline(
    p: &Project,
    audio: &mut ClipAudioFn<'_>,
    takes: &HashMap<Id, Arc<Audio>>,
    audition: &HashMap<Id, Id>,
    keys: Option<&Arc<crate::keys::KeysBank>>,
) -> Timeline {
    // comps are resolved once each, shared by every clip that plays them
    let spb = 60.0 / p.bpm;
    let comps: HashMap<Id, Arc<crate::comp::CompRt>> =
        p.comps.iter().map(|cp| (cp.id, Arc::new(crate::comp::build(cp, spb, audition.get(&cp.id).copied(), takes)))).collect();
    let (chans, pats) = build_patterns(p, takes, keys);
    // automation envelopes whose target still exists
    let mut autos = vec![];
    let mut auto_idx = HashMap::new();
    for a in &p.automations {
        if let Some(target) = resolve_target(p, &a.target) {
            auto_idx.insert(a.id, autos.len());
            autos.push(AutoRt { target, env: a.clone() });
        }
    }
    let mut clips = vec![];
    for c in p.clips.iter().filter(|c| !c.muted) {
        if c.track as usize >= p.tracks.len() {
            continue;
        }
        let mut rate = 1.0;
        let src = match c.kind {
            ClipKind::Pat => match p.patterns.iter().position(|x| x.id == c.source) {
                Some(i) => ClipSrc::Pat(i),
                None => continue,
            },
            ClipKind::Auto => match auto_idx.get(&c.source) {
                Some(&i) => ClipSrc::Auto(i),
                None => continue,
            },
            ClipKind::Audio => match audio(c) {
                Some((a, r)) => {
                    rate = r;
                    ClipSrc::Audio(a)
                }
                None => continue,
            },
            ClipKind::Comp => match comps.get(&c.source) {
                Some(cp) => ClipSrc::Comp(cp.clone()),
                None => continue,
            },
        };
        clips.push(ClipRt {
            id: c.id,
            track: c.track as usize,
            start: c.start,
            end: c.start + c.len,
            offset: c.offset,
            rate,
            gain: c.gain,
            pan: c.pan,
            fade_in: c.fade_in,
            fade_out: c.fade_out,
            insert: if c.kind == ClipKind::Audio { c.insert.and_then(|id| p.inserts.iter().position(|m| m.id == id)) } else { None },
            src,
            gain0: c.gain,
            pan0: c.pan,
        });
    }
    clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    // clip targets point at the sorted list (a muted clip is not in it: its automation rests)
    for a in autos.iter_mut() {
        if let TargetRt::ClipGain(k) | TargetRt::ClipPan(k) = &mut a.target {
            let id = *k as Id;
            *k = clips.iter().position(|c| c.id == id && matches!(c.src, ClipSrc::Audio(_))).unwrap_or(usize::MAX);
        }
    }
    // the clips of each automated target, by start (automations of the same target share one)
    let mut drives: Vec<Drive> = vec![];
    for (ci, c) in clips.iter().enumerate() {
        let ClipSrc::Auto(ai) = c.src else { continue };
        let target = autos[ai].target;
        match drives.iter_mut().find(|d| d.target == target) {
            Some(d) => d.clips.push(ci),
            None => drives.push(Drive { target, clips: vec![ci] }),
        }
    }
    Timeline { bpm: p.bpm, clips, pats, chans, autos, drives, loop_range: p.loop_range.map(|l| (l.a, l.b)), song_end: p.song_end() }
}

/// Turns an automation target into graph indices; `None` if it no longer exists.
fn resolve_target(p: &Project, t: &AutoTarget) -> Option<TargetRt> {
    Some(match t {
        AutoTarget::Fx { fx_id, param } => {
            let (_, f) = p.find_fx(*fx_id)?;
            if let Some(rest) = param.strip_prefix("band:") {
                let mut it = rest.split(':');
                let band: usize = it.next()?.parse().ok()?;
                let field = match it.next()? {
                    "f" => 0,
                    "g" => 1,
                    "q" => 2,
                    _ => return None,
                };
                if band >= MAX_BANDS {
                    return None;
                }
                TargetRt::Band { fx: *fx_id, band, field }
            } else {
                let defs = crate::catalog::params(f.kind);
                let idx = defs.iter().position(|d| d.id == param)?;
                let d = &defs[idx];
                TargetRt::Param { fx: *fx_id, idx, min: d.min as f32, max: d.max as f32, log: d.log }
            }
        }
        AutoTarget::Mix { fx_id } => {
            p.find_fx(*fx_id)?;
            TargetRt::Mix { fx: *fx_id }
        }
        AutoTarget::InsertVol { id } => TargetRt::InsVol(p.inserts.iter().position(|m| m.id == *id)?),
        AutoTarget::InsertPan { id } => TargetRt::InsPan(p.inserts.iter().position(|m| m.id == *id)?),
        AutoTarget::TrackVol { id } => TargetRt::TrackVol(p.tracks.iter().position(|x| x.id == *id)?),
        AutoTarget::ChanVol { id } => TargetRt::ChanVol(p.channels.iter().position(|c| c.id == *id)?),
        AutoTarget::ChanPan { id } => TargetRt::ChanPan(p.channels.iter().position(|c| c.id == *id)?),
        AutoTarget::Synth { chan_id, param } => {
            let c = p.channels.iter().find(|c| c.id == *chan_id)?;
            if !matches!(c.src, ChannelSrc::Synth { .. }) {
                return None;
            }
            let idx = crate::poly::params::index_of(param)?;
            let d = &crate::poly::params::PARAMS[idx];
            TargetRt::Synth { chan: *chan_id, idx, min: d.min as f32, max: d.max as f32, log: d.log, choice: d.options.is_some() }
        }
        AutoTarget::Keys { chan_id, param } => {
            let i = p.channels.iter().position(|c| c.id == *chan_id && matches!(c.src, ChannelSrc::Keys { .. }))?;
            match param.as_str() {
                "release" => TargetRt::KeysRelease(i),
                "dynamics" => TargetRt::KeysDynamics(i),
                _ => return None,
            }
        }
        AutoTarget::Plugin { fx_id, param_id } => {
            p.find_fx(*fx_id).filter(|(_, f)| f.kind == FxKind::Plugin)?;
            TargetRt::PluginParam { fx: *fx_id, param_id: *param_id }
        }
        // the clip id for now: `build_timeline` turns it into the clip's index once sorted
        AutoTarget::ClipGain { id } => TargetRt::ClipGain(p.clips.iter().find(|c| c.id == *id && c.kind == ClipKind::Audio)?.id as usize),
        AutoTarget::ClipPan { id } => TargetRt::ClipPan(p.clips.iter().find(|c| c.id == *id && c.kind == ClipKind::Audio)?.id as usize),
    })
}

/// See [`Engine::pitch_view`].
pub struct PitchView {
    pub clip: Clip,
    pub analysis: Arc<crate::pitch::PitchAnalysis>,
    pub target: Option<Arc<crate::reference::PitchTarget>>,
    pub notes: Vec<crate::model::NoteEdit>,
    /// The notes are the clip's own edits (else the analysis' segmentation, unedited).
    pub own: bool,
    /// MIDI per analysis frame as it will sound (NaN = unvoiced).
    pub out: Vec<f32>,
    pub bpm: f64,
    pub sr: u32,
    /// Length of the source, seconds.
    pub seconds: f64,
}

impl PitchView {
    /// The part of the source the clip plays, seconds (clamped to the audio).
    pub fn region(&self) -> (f64, f64) {
        let (a, b) = self.clip.source_span(self.bpm);
        (a.clamp(0.0, self.seconds), b.clamp(0.0, self.seconds).max(a.clamp(0.0, self.seconds)))
    }
    /// Timeline beat of a source second of this clip.
    pub fn beat_of(&self, t: f64) -> f64 {
        self.clip.start + t * self.clip.stretch / (60.0 / self.bpm.max(1.0)) - self.clip.offset
    }
    pub fn spans(&self) -> Vec<(f64, f64)> {
        self.notes.iter().map(|n| (n.start, n.end)).collect()
    }
}
