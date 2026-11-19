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

mod analysis;
mod graph;
mod import;
mod preview;
mod project_files;
mod render;
mod takes;

pub use analysis::PitchView;
pub use graph::playback_latency;
use graph::{build_graph, build_timeline, c_watch, comp_audio, live_audition, mix_state, monitor_of, playback_latency_with, sample_audio};
pub(crate) use project_files::project_document;
use project_files::{audio_dir, fresh_path, load_library_sources, sanitize};
pub use project_files::{backup_path, scratch_dir};
pub use render::{pattern_project, InPlace, PlacedStems, StemFile, StemRegion, IN_PLACE_MAX_TAIL_S, STEMS_MAX_S};

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
    /// Let effects and releases ring out after `to` until the output has stayed below
    /// [`DECAY_FLOOR`] for [`DECAY_HOLD_S`] (a delay's silent gap between echoes is not the end),
    /// at most `max` seconds; the result ends on the last audible frame, never before `to`.
    Decay {
        max: f64,
    },
}

/// Level under which a ringing tail counts as finished (-90 dBFS).
pub const DECAY_FLOOR: f32 = 3.162e-5;
/// How long the output must stay under [`DECAY_FLOOR`] before a decaying render stops: longer
/// than the longest built-in delay, so the gap before an echo is not taken for the end.
pub const DECAY_HOLD_S: f64 = crate::dsp::delay::MAX_DELAY_S as f64 + 0.5;

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

pub(crate) struct Core {
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
    /// Effect ids the audio thread currently holds a processor for, with their kind: a kind
    /// change under the same id is a different effect, and its processor must be rebuilt.
    live_fx: HashMap<Id, FxKind>,
    sent_params: HashMap<Id, (Option<ParamBlock>, f32)>,
    /// Warp maps the audio thread has, by effect id (a change is sent as `Cmd::FxData`).
    sent_maps: HashMap<Id, crate::model::WarpMaps>,
    fx_errors: HashMap<Id, String>,
    /// Latency each live hosted plugin reported, by effect id (plugin delay compensation).
    plugin_lat: HashMap<Id, usize>,
    /// Bumped whenever hosted-plugin instances may have been replaced (a mixer rebuild, a
    /// retry, a stream rebuild): a latency report collected before it is stale.
    plugin_gen: u64,
    /// Why a source's audio did not load when the project was opened (by source id).
    source_errors: HashMap<Id, String>,
    wires: Option<Wires>,
    take: Option<RecBuf>,
    /// The pattern the loop preview plays and the part of it that loops (PAT mode).
    pat_loop: Option<(Id, Option<(f64, f64)>)>,
    /// The loop generation to hand the next pattern loop (see `Shared::pat_gen`).
    pat_gen: u64,
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
    /// The loop generation this recorder is for (notes of other loops are not captured).
    gen: u64,
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
                live_fx: HashMap::new(),
                sent_params: HashMap::new(),
                sent_maps: HashMap::new(),
                fx_errors: HashMap::new(),
                plugin_lat: HashMap::new(),
                plugin_gen: 0,
                source_errors: HashMap::new(),
                wires: None,
                take: None,
                pat_loop: None,
                pat_gen: 0,
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
        for id in c.live_fx.drain().map(|(id, _)| id).collect::<Vec<_>>() {
            self.factory.release(id);
        }
        c.plugin_lat.clear();
        c.sent_params.clear();
        c.sent_maps.clear();
        c.synths.clear();
        self.forget_plugs(&mut c);
        // the old stream is gone: keep what it captured instead of orphaning the take
        if let Err(e) = self.finalize_take(&mut c) {
            log::error!("{e}");
        }
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

    pub fn stop(&self) -> Result<TransportDto, EngineError> {
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
        // the transport stops either way; a take that could not be written is the error the
        // caller came here to hear about (the UI shows it where it shows every action error)
        let written = self.finish_take();
        self.finish_midi_take();
        let mut c = self.core.lock();
        self.settle_rec_track(&mut c);
        let t = c.transport.clone();
        drop(control);
        self.changed(c);
        written.map(|_| t)
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
            if let Err(e) = self.finish_take() {
                log::error!("{e}");
            }
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
}

impl Engine {
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
}

impl Engine {
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
}

impl Engine {
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
            if let Err(e) = self.finish_take() {
                log::error!("{e}");
            }
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
}

impl Engine {
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
