// In-browser fake engine, used when the page is not running inside Tauri.
// Holds the demo project, applies ops with undo/gestures, plays a fake transport and
// streams binary telemetry frames (encoded exactly like the real ones).
import { askUrl, MAX_ASK_URL, type AskService } from '../lib/askLink';
import { delayMs, loopGain } from '../lib/delay';
import { loopPos, type BeatRange } from '../lib/patLoop';
import type { Backend, EventMap, OsDropEvent } from './backend';
import type { Json } from '../lib/projectEq';
import { encodeFrame, SCOPE_LEN, SPEC_BANDS, type EncFx } from './telemetry';
import { BUILTIN, demoProject, emptyProject, mockCatalog, sketchProject } from './mockData';
import { FACTORY_FILES, FACTORY_KIT, FACTORY_LISTED_FILES, factoryFile, LIB_SCHEME, libPath } from '../lib/factory';

/** Reverb mix (%) the engine computes for the research plate at a -18 dB send (autovocal::reverb_mix_for_send). */
const MOCK_REVERB_MIX = 37.2;
import { ERR_EXISTS, ERR_INVALID_TARGET, ERR_NO_EDITOR, ERR_NO_STEM_MODEL, ERR_REVISION_CONFLICT } from './errors';
import { projectEq } from '../lib/projectEq';
import { applyAll, stripClipDefaults } from '../lib/applyOps';
import { stripPatternDefaults } from '../lib/pattern';
import { landTakes, takeEnd } from '../lib/comp';
import { dropEmptyTrack, recTarget } from '../lib/recTarget';
import { newTrack } from '../lib/clipOps';
import { satDriveDb } from '../lib/saturator';
import { songEnd } from '../lib/export';
import { eqCurve } from '../lib/eq';
import { clamp, dbg } from '../lib/format';
import { t } from '../lib/i18n';
import { midiToHz, noteAt, scaleMask, simpleTarget, snapNote } from '../lib/pitch';
import { collectMelody, melodyFit, melodyTarget } from '../lib/melody';
import { newFx } from '../lib/presets';
import { synthVocal, vocalAt, vocalPeaks, type VocalSample } from './mockVocal';
import type {
  AudioConfig, AudioStatus, LatencyReport, AnyBuiltinKind, AutoVocalPlan, AutoVocalStyle, Catalog, Clip, ExportAudioOptions, ExportResult, ExportStemsOptions, ExportStemsResult, F0ModelStatus, Fx, Id, KeysStatus, LibraryFiles, LibraryList, Melody, MelodyChannel, MidiDevice, MidiStatus, MusicAnalysis, Op, OpenResult, PitchAnalysis, PitchTarget, Project, RefTune, MidiBecomes, MidiExportResult, MidiImportArgs, MidiImportResult, MidiInfo, Channel, Note, Pattern, SaveResult, Source, StateDto, Track, TransportDto, PluginInfo, PluginParam, PluginRef, PluginScan, PluginCrashSuspect, RelinkCandidate,
  ExtractStemsResult, StemId, StemModelStatus, StemsProgress,
} from './types';
import { exportRange, exportSeconds, patternProject, patternSilent, stemInserts, uniqueFileStems } from '../lib/export';
import { MockUpdates } from './mockUpdates';
import { MOCK_LV2 } from './mockPlugins';
import { UNSAFE_PLUGIN } from './types';
import { fromNormalized, lv2Param } from '../lib/pluginParam';
import { KEYS_DYNAMICS, KEYS_RELEASE, SAMPLE_ROOT, samplerDefault } from './types';
import { newSynthChannel } from '../lib/synth';
import { candidate, rank, unambiguous } from '../lib/relink';

const TC = ['#4f8ef7', '#5aa9e6', '#69c3d6', '#47b8a8', '#5fbf8a', '#7c8cf8', '#e0a44a', '#9a8cf0', '#d9678f', '#8b93a1'];
const BAND_F = new Float32Array(SPEC_BANDS).map((_, k) => 20 * Math.pow(1000, k / (SPEC_BANDS - 1)));
const delay = (ms = 0) => new Promise<void>(r => setTimeout(r, ms));
/** The stem model download (crates/stems `MODEL_BYTES`). */
const MOCK_STEM_MODEL_BYTES = 90_358_774;
const STEM_IDS: StemId[] = ['drums', 'bass', 'other', 'vocals'];
/** stems_actions.rs `STEMS` (names and colours of the new tracks); `scale` shapes the mock's fake waveforms. */
const STEM_META: Record<StemId, { label: string; color: string; scale: number }> = {
  drums: { label: 'Drums', color: '#e8a33d', scale: 0.9 }, bass: { label: 'Bass', color: '#8f6ee8', scale: 0.7 },
  other: { label: 'Other', color: '#47b8a8', scale: 0.55 }, vocals: { label: 'Vocals', color: '#e05a8a', scale: 0.6 },
};
const hash = (s: string) => { let h = 2166136261; for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619); return h >>> 0; };

function rng(seed: number) {
  let s = seed || 7;
  return () => ((s = Math.imul(s, 16807) % 2147483647) & 0x7fffffff) / 2147483647;
}

/** Synthetic waveform peaks: a loop of transients with decays, 100 [min,max] pairs per second. */
function synthPeaks(seed: number, dur: number, bpm: number): Float32Array {
  const n = Math.max(1, Math.round(dur * 100)), out = new Float32Array(n * 2), r = rng(seed);
  const style = seed % 3, spb = 60 / bpm;
  for (let i = 0; i < n; i++) {
    const t = i / 100, ph = (t % (spb / (style === 1 ? 2 : 1))) / spb;
    const env = style === 0 ? Math.exp(-ph * 9) * 0.85 + 0.05 : style === 1 ? Math.exp(-ph * 4) * 0.6 + 0.2 : 0.45 + 0.25 * Math.sin(t * 2.1) * Math.sin(t * 0.7);
    const a = clamp(env * (0.75 + 0.25 * r()), 0, 1);
    out[2 * i] = -a * (0.85 + 0.15 * r()); out[2 * i + 1] = a;
  }
  return out;
}

/** Peaks of a one-shot (a standard-library sample): a hit that decays over its length. */
function oneShotPeaks(seed: number, dur: number): Float32Array {
  const n = Math.max(1, Math.round(dur * 100)), out = new Float32Array(n * 2), r = rng(seed);
  for (let i = 0; i < n; i++) {
    const a = clamp(Math.exp(-(i / n) * 5) * (0.8 + 0.2 * r()), 0, 1);
    out[2 * i] = -a * (0.85 + 0.15 * r()); out[2 * i + 1] = a;
  }
  return out;
}

interface MockSource extends Source { seed: number; vocal?: boolean; trimFrom?: { id: Id; offSec: number } }
const isVocalName = (n: string) => /vocal|vox|wokal|śpiew/i.test(n);
const VOCAL_DUR = 14;

/** Per-effect DSP state of the fake processors (so meters move like the engine's). */
interface FxSim { t: number; corr: number; center: number; held: number; last: number; m: number[]; gr: number; dyn: Float32Array }
const newSim = (): FxSim => ({ t: 0, corr: 0, center: 0, held: 0, last: 0, m: [0, 0, 0, 0, 0, 0], gr: 0, dyn: new Float32Array(32) });

/** Spectrum (dBFS per band) of a sung vowel at `midi` with level `lv`, plus sibilant noise `sib`. */
function vocalSpec(s: VocalSample, out: Float32Array) {
  const pw = new Float64Array(SPEC_BANDS);
  const lvDb = s.level > 1e-4 ? 20 * Math.log10(s.level) : -120;
  const idx = (f: number) => Math.round(Math.log(f / 20) / Math.log(1000) * (SPEC_BANDS - 1));
  if (s.midi != null && s.level > 0.01) {
    const f0 = midiToHz(s.midi);
    for (let h = 1; h * f0 < 12000; h++) {
      const f = h * f0, oct = Math.log2(f / 500);
      // formants around 700 Hz and 2.6 kHz on a −9 dB/oct tilt
      const env = -9 * Math.max(0, oct) + 9 * Math.exp(-((Math.log2(f / 700)) ** 2) * 3) + 7 * Math.exp(-((Math.log2(f / 2600)) ** 2) * 4);
      const k = idx(f); if (k < 0 || k >= SPEC_BANDS) continue;
      pw[k] += Math.pow(10, (lvDb - 14 + env) / 10);
    }
  }
  for (let k = 0; k < SPEC_BANDS; k++) {
    const f = BAND_F[k];
    const hiss = s.sib > 0 ? Math.pow(10, (-24 + 6 * s.sib - 18 * Math.max(0, Math.log2(4500 / f)) ** 2 * (f < 4500 ? 1 : 0) - 3 * Math.max(0, Math.log2(f / 9000))) / 10) : 0;
    const floor = Math.pow(10, (-92 - 3 * Math.log2(f / 1000)) / 10);
    out[k] = 10 * Math.log10(pw[k] + hiss + floor) + (Math.random() - 0.5) * 2.5;
  }
}

/** A VST3 plugin the mock reports as installed (class id derived from its name, like a real scan's stable id). */
const mockVst3 = (name: string, vendor: string, category: string): PluginInfo => ({ format: 'vst3', name, vendor, category, path: 'C:\\Program Files\\Common Files\\VST3\\' + name + '.vst3', cid: hash(name).toString(16).padStart(32, '0') });
/** The VST3 instruments the mock's scan finds: name, vendor, category. */
const MOCK_INSTRUMENTS: [string, string, string][] = [['Serum', 'Xfer Records', 'Instrument|Synth'], ['Analog Lab V', 'Arturia', 'Instrument|Synth'], ['Kontakt 7', 'Native Instruments', 'Instrument|Sampler'], ['Vital', 'Vital Audio', 'Instrument|Synth']];

export class MockBackend implements Backend {
  readonly isMock = true;
  /** No window chrome in a browser; `?chrome` previews the custom title bar with inert buttons. */
  readonly win = typeof location !== 'undefined' && new URLSearchParams(location.search).has('chrome')
    ? { minimize: async () => {}, toggleMaximize: async () => {}, close: async () => {}, isMaximized: async () => false, startDragging: async () => {}, onResized: async () => () => {} }
    : null;
  private cat: Catalog = mockCatalog();
  // perf harness hook: perf/ui_bench.mjs injects a generated long project here (see perf/README)
  private p: Project = this.adoptFixture();
  private revision = 1;
  private savedRev = 1;
  private path: string | null = null;
  private undoStack: Project[] = [];
  private redoStack: Project[] = [];
  private gesture: Project | null = null;
  /** A gesture's transient batches changed the project without a state event (engine.rs gesture_unreported). */
  private gestureUnreported = false;
  private tr: TransportDto = { playing: false, rec: false, monitor: false, metro: false, countIn: true, startBeat: 0, recAudio: true, recNotes: true, recQuantize: 0 };
  private listeners = new Map<string, Set<(p: unknown) => void>>();
  private teleCb: ((b: ArrayBuffer) => void) | null = null;
  private watched: Id[] = [];
  private seq = 0;
  private playT0 = 0; private playFrom = 0;
  private rec: null | { track: number; clipStart: number; t0: number; peaks: number[]; sent: number; lap: [number, number] | null; pass: number } = null;
  private audition: Record<string, Id> = {};
  private preview: null | { t0: number; dur: number; off: number; rate?: number } = null;
  /** Pattern loop preview (engine `pattern_play`) and recent auditions (insert id → time) for the meters. */
  /** The pattern loop: since `t0` (ms) it plays on from `from` beats, within `range`. */
  private patPlay: null | { id: Id; t0: number; from: number; range: BeatRange | null } = null;
  private flash = new Map<Id, number>();
  private srcMeta = new Map<Id, MockSource>();
  private takeN = 0;
  private kits = [
    { id: FACTORY_KIT, name: 'Standard library', description: 'Drums, percussion and tonal one-shots that ship with Synkopa', bytes: FACTORY_FILES.reduce((n, f) => n + f.bytes, 0), count: FACTORY_LISTED_FILES.length, installed: true },
    { id: 'wave-drums', name: 'Synkopa Drums', description: 'Stopy, werble, hi-haty i pętle perkusyjne', bytes: 86e6, count: 240, installed: false },
    { id: 'wave-keys', name: 'Synkopa Keys & Pads', description: 'Akordy, pady i tekstury', bytes: 142e6, count: 96, installed: true },
    { id: 'wave-fx', name: 'Synkopa FX', description: 'Uplightery, impakty, szumy', bytes: 38e6, count: 64, installed: false },
  ];
  private folders = ['C:\\Users\\michal\\Music\\Samples'];
  /** Live plugin instances' parameters (VST3: normalized only; LV2: by port, with their real values). */
  private plugParams = new Map<Id, PluginParam[]>();
  private lv2Real = new Map<Id, Map<number, number>>();
  private audio: AudioStatus = {
    host: 'wasapi', output: 'Głośniki (Realtek High Definition Audio)', input: 'Mikrofon (USB Audio)', sampleRate: null, bufferSize: null, inputChannel: null,
    running: true, actualRate: 48000, actualBuffer: 480, latencyMs: 20, inputChannels: 2, error: null, note: t('d.mock.noaudio'), asioCompiled: true,
    unavailableOutput: null,
    recOffsetMs: 0, inputUnderruns: 0, inputOverruns: 0, inputDriftPpm: 0, restarts: 0,
  };
  /** Output underruns since the engine started (telemetry `underruns`): the browser renders
   *  nothing, so none happen unless a test injects them with {@link mockUnderrun}. */
  private underrunTotal = 0;
  private focusritePresent = true;
  private wasapiOutputPresent = true;

  private midiPorts: MidiDevice[] = [{ id: 'mock-keys', name: 'Mock Keystation 61' }, { id: 'mock-pads', name: 'Mock Pad Controller' }];
  private midi: MidiStatus = { connected: null, selected: null, error: null, lastNote: null, noteCount: 0, target: null, fixedVelocity: null };

  private clock: ReturnType<typeof setInterval>;
  constructor() {
    this.clock = setInterval(() => this.tick(), 1000 / 60);
    (window as unknown as { __waveMock: MockBackend }).__waveMock = this;
  }

  /** Stops the frame clock (tests call this so intervals do not outlive a case). */
  dispose() { clearInterval(this.clock); }

  /** The engine never writes a field at its serde default, so its state JSON never carries
   * one; an injected fixture may. Normalize once at the door — the engine compares structs,
   * where "missing" and "default value" are not two states, and the no-op check below must
   * see what the engine sees. */
  private adoptFixture(): Project {
    const f = typeof window !== 'undefined' && (window as unknown as { __waveFixture?: Project }).__waveFixture;
    if (!f) return sketchProject(this.cat);
    const p = structuredClone(f);
    for (const c of p.clips) stripClipDefaults(c);
    for (const pt of p.patterns) stripPatternDefaults(pt);
    for (const a of p.automations ?? []) {
      for (const q of a.points) {
        if (!q.tension) delete (q as { tension?: number }).tension;
        if (q.shape === 'curve') delete (q as { shape?: unknown }).shape;
      }
    }
    // old projects may lack `key`; the engine writes it as null (no skip on the field)
    if (p.key === undefined) p.key = null;
    return p;
  }

  // ---------- state ----------
  private dto(): StateDto {
    return structuredClone({ project: this.p, revision: this.revision, canUndo: this.undoStack.length > 0, canRedo: this.redoStack.length > 0,
      transport: this.tr, path: this.path, dirty: this.revision !== this.savedRev, audition: this.liveAudition(), missing: this.missing(), fxErrors: this.fxErrors() });
  }
  /**
   * Like the engine's `fx_errors`: LV2 slots whose plugin is not "installed" here, or cannot run, and plugin
   * channels whose instrument is not one of the mock's installed ones (by channel id).
   */
  private fxErrors(): Record<string, string> {
    const out: Record<string, string> = {};
    const installed = new Set(MOCK_INSTRUMENTS.map(([name, vendor, category]) => mockVst3(name, vendor, category).cid));
    for (const c of this.p.channels ?? []) {
      if (c.kind === 'plugin' && !installed.has(c.plugin.cid)) out[c.id] = `${c.plugin.name}: VST3 plugin module not found: ${c.plugin.path}`;
    }
    for (const f of this.p.inserts.flatMap(m => m.fx)) {
      if (f.type !== 'plugin' || f.plugin?.format !== 'lv2') continue;
      const p = MOCK_LV2[f.plugin.cid];
      if (!p) out[f.id] = `${f.plugin.name} (LV2): the LV2 plugin ${f.plugin.cid} is not installed`;
      else if (p.unsupported) out[f.id] = `${f.plugin.name} (LV2): ${p.name} cannot be used: ${p.unsupported}`;
      else if (p.crashes && !this.trusted.has(f.plugin.cid)) out[f.id] = `${f.plugin.name}: ${UNSAFE_PLUGIN}it crashed in Synkopa's safety check (the test process was killed by signal 11), so it was not loaded: loaded here it would close WAVE. Update the plugin, or load it anyway at your own risk.`;
    }
    return out;
  }
  private emit<K extends keyof EventMap>(ev: K, payload: EventMap[K]) {
    const ls = this.listeners.get(ev); if (!ls) return;
    setTimeout(() => ls.forEach(cb => cb(payload)), 0);
  }
  private changed() { this.revision++; this.emit('state', this.dto()); }
  /** engine.rs `import_base`: a file loaded for the next edit (and the revision it left) is undone with that edit. */
  private importBase: { rev: number; p: Project } | null = null;
  private pushUndo(prev: Project) {
    const base = this.importBase; this.importBase = null;
    if (base && base.rev === this.revision) prev = base.p;
    this.undoStack.push(prev); if (this.undoStack.length > 300) this.undoStack.shift(); this.redoStack = []; }

  async getState() { return this.dto(); }
  async applyOps(ops: Op[], opts?: { label?: string; transient?: boolean }) {
    await delay();
    // like the engine: a plugin automation target must name a parameter the hosted
    // plugin lists, else it would be a dead envelope
    for (const op of ops) {
      if (op.op === 'putAutomation' && op.automation.target.kind === 'plugin') {
        const { fxId, paramId } = op.automation.target;
        const fx = this.p.inserts.flatMap(m => m.fx).find(f => f.id === fxId);
        if (fx?.type !== 'plugin' || !(await this.pluginParams(fxId).catch(() => [])).some(q => q.id === paramId))
          throw new Error(`${ERR_INVALID_TARGET} effect ${fxId} has no plugin parameter ${paramId} to automate`);
      }
    }
    const prev = this.p;
    const next = applyAll(prev, ops); // throws (rejects) on an invalid batch
    if (projectEq(prev as unknown as Json, next as unknown as Json)) {
      // like the engine (engine.rs apply): an unchanged batch is not an edit — no undo
      // step, no revision, no state event
      return { revision: this.revision };
    }
    this.p = next;
    // like the engine (engine.rs only_track_heights): lane heights are view state — no undo step, and
    // the history takes the new heights so undoing an older edit keeps the lanes as they are now
    const heightsOnly = ops.every(op => op.op === 'putTrack') && prev.tracks.length === next.tracks.length
      && prev.tracks.every((t, i) => projectEq({ ...t, h: next.tracks[i].h } as unknown as Json, next.tracks[i] as unknown as Json));
    if (heightsOnly) {
      // only the lanes that changed, and only into the same track (ids stay with their slots when lanes
      // swap): like the engine (carry_height) the history is walked from now, nearest state first, and the
      // carry stops where the lane at that id became another track (a rename keeps it the same track)
      const changed = next.tracks.filter((t, i) => prev.tracks[i].h !== t.h);
      const sameLane = (a: Track, b: Track) => a.name === b.name
        || projectEq({ ...a, name: b.name, h: b.h } as unknown as Json, b as unknown as Json);
      const carry = (states: Project[]) => {
        for (const now of changed) {
          let last: Track = now;
          for (let i = 0; i < states.length; i++) {
            const t = states[i].tracks.find(x => x.id === now.id);
            if (!t || !sameLane(last, t)) break;
            last = t;
            states[i] = { ...states[i], tracks: states[i].tracks.map(x => x.id === now.id ? { ...x, h: now.h } : x) };
          }
        }
        return states;
      };
      // nearest first: the undo stack's newest is last, the redo stack's next state is last
      this.undoStack = carry([...this.undoStack].reverse()).reverse();
      this.redoStack = carry([...this.redoStack].reverse()).reverse();
      if (this.gesture) this.gesture = carry([this.gesture])[0];
      if (opts?.transient && this.gesture) { this.revision++; this.gestureUnreported = true; } else this.changed();
      return { revision: this.revision };
    }
    // like the engine: no state event per transient batch of a gesture (endGesture reports it)
    if (opts?.transient && this.gesture) { this.revision++; this.gestureUnreported = true; }
    else { this.pushUndo(prev); this.changed(); }
    return { revision: this.revision };
  }
  async beginGesture() { this.gesture = structuredClone(this.p); }
  async endGesture() {
    const g = this.gesture; this.gesture = null;
    const unreported = this.gestureUnreported; this.gestureUnreported = false;
    if (g && JSON.stringify(g) !== JSON.stringify(this.p)) {
      this.pushUndo(g);
      // pitch edits are rendered offline at the end of a gesture: the state event comes after the render
      const tuned = this.p.clips.some(c => { const o = g.clips.find(x => x.id === c.id); return JSON.stringify(o?.tune ?? null) !== JSON.stringify(c.tune ?? null); });
      if (tuned) { this.revision++; this.renderReady = performance.now() + 380; setTimeout(() => this.emit('state', this.dto()), 380); } else this.changed();
    } else if (g && unreported) {
      // like the engine (end_gesture, gesture_unreported): nothing to undo — dragged and back, or lane
      // heights only — but listeners have not seen the revisions of the gesture's batches
      this.emit('state', this.dto());
    }
  }
  async undo() {
    const prev = this.undoStack.pop();
    if (prev) { this.redoStack.push(this.p); this.p = prev; this.changed(); }
    return this.dto();
  }
  async redo() {
    const next = this.redoStack.pop();
    if (next) { this.undoStack.push(this.p); this.p = next; this.changed(); }
    return this.dto();
  }

  // ---------- transport ----------
  private spb() { return 60 / this.p.bpm; }
  private songEnd() { return songEnd(this.p); }
  /** Playhead without loop wrapping (beats since start, negative during count-in). */
  private rawBeat(): number { return this.playFrom + (performance.now() - this.playT0) / 1000 / this.spb(); }
  private beatNow(): number {
    if (!this.tr.playing) return this.tr.startBeat;
    let b = this.rawBeat();
    if (this.rec) {
      // loop recording cycles over the loop, like the engine (one take per pass)
      const lap = this.rec.lap;
      if (lap && b >= lap[1]) b = lap[0] + ((b - lap[1]) % (lap[1] - lap[0]));
      return b;
    }
    const lp = this.p.loop, a = lp ? lp.a : 0, e = lp ? lp.b : this.songEnd();
    if (b >= e && e > a) { b = a + ((b - a) % (e - a)); }
    return b;
  }
  async transportPlay(fromBeat: number) {
    if (this.tr.playing) this.finishRec();
    this.patPlay = null; // the song and the pattern preview are exclusive (like the engine)
    const pre = this.tr.rec && this.tr.countIn ? 4 : 0;
    const lp = this.p.loop;
    if (this.tr.rec && lp && lp.b > lp.a && (fromBeat < lp.a || fromBeat >= lp.b)) fromBeat = lp.a;
    this.tr = { ...this.tr, playing: true, startBeat: fromBeat };
    this.playFrom = fromBeat - pre; this.playT0 = performance.now();
    if (this.tr.rec) this.startRec(fromBeat);
    this.changedTransport();
  }
  async transportStop() {
    this.finishRec();
    this.tr = { ...this.tr, playing: false };
    this.changedTransport();
  }
  /** Like the engine's Silence: the preview stops (the mock renders no tails to cut). */
  async transportSilence() { this.preview = null; }
  async transportSet(patch: Partial<TransportDto>) {
    const wasRec = this.tr.rec;
    // like the engine: the recording filter keeps one kind on
    const ra = patch.recAudio ?? this.tr.recAudio, rn = patch.recNotes ?? this.tr.recNotes;
    if (!ra && !rn) patch = { ...patch, recAudio: this.tr.recAudio, recNotes: this.tr.recNotes };
    this.tr = { ...this.tr, ...patch };
    if (wasRec && !this.tr.rec) this.finishRec();
    if (!wasRec && this.tr.rec && this.tr.playing) this.startRec(this.beatNow());
    this.changedTransport();
    return { ...this.tr };
  }
  private changedTransport() { this.emit('state', this.dto()); }
  private startRec(clipStart: number) {
    // the mock has no MIDI keyboard: with Audio unticked nothing is recorded
    if (!this.tr.recAudio) return;
    // like the native engine: with no running audio device there is no take either, so a
    // reset stays allowed (a take needs the input the device would deliver)
    if (!this.audio.running) return;
    // like the engine: the armed track, else the first one with room, else a new track
    const lp = this.p.loop;
    const lap: [number, number] | null = lp && lp.b > lp.a && clipStart >= lp.a && clipStart < lp.b ? [lp.a, lp.b] : null;
    let t = recTarget(this.p, clipStart, lap ? lap[1] : Infinity);
    if (t === 'new') {
      t = this.p.tracks.length;
      const id = this.p.nextId++;
      this.p.tracks.push(newTrack(id, t));
      this.recAdded = id;
      this.changed();
    }
    this.rec = { track: t, clipStart: Math.max(0, clipStart), t0: 0, peaks: [], sent: 0, lap, pass: 0 };
  }
  /** The track a recording added for want of room (the engine's `rec_added`). */
  private recAdded: Id | null = null;
  /** After a take: a track added for it that stayed empty goes, also from the undo snapshots. */
  private settleRecTrack() {
    const id = this.recAdded; this.recAdded = null;
    if (id == null) return;
    for (const snap of this.undoStack) dropEmptyTrack(snap, id);
    if (dropEmptyTrack(this.p, id)) this.changed();
  }
  /** Peak-pair index where each pass after the first begins (100 pairs per second). */
  private passMarks(r: NonNullable<MockBackend['rec']>, total: number): number[] {
    if (!r.lap) return [];
    const spb = this.spb(), first = (r.lap[1] - r.clipStart) * spb * 100, lapLen = (r.lap[1] - r.lap[0]) * spb * 100, out: number[] = [];
    for (let m = first; m < total; m += lapLen) out.push(Math.round(m));
    return out;
  }
  private finishRec() {
    const r = this.rec; this.rec = null;
    if (!r) return;
    const total = r.peaks.length / 2, spb = this.spb();
    const bounds = [0, ...this.passMarks(r, total), total];
    const passes: [Float32Array, number][] = [];
    for (let k = 0; k < bounds.length - 1; k++) {
      const n = bounds[k + 1] - bounds[k];
      const cutShort = k > 0 && k === bounds.length - 2;
      if (n < (cutShort ? 100 * spb : 10)) continue; // a pass cut off right after the wrap is not a take
      passes.push([Float32Array.from(r.peaks.slice(bounds[k] * 2, bounds[k + 1] * 2)), k === 0 ? r.clipStart : r.lap![0]]);
    }
    if (!passes.length) return this.settleRecTrack();
    const prev = structuredClone(this.p);
    const landed: [Id, number, number][] = [];
    for (const [pk, start] of passes) {
      const id = this.p.nextId++, dur = pk.length / 2 / 100;
      const src: MockSource = { id, name: 'Take ' + (++this.takeN), color: '#d9678f', path: 'recordings/take' + this.takeN + '.wav', dur, sr: 48000, ch: 1, peakDb: -6, seed: id };
      this.srcMeta.set(id, src);
      this.recPeaks.set(id, pk);
      this.p.sources.push(strip(src));
      landed.push([id, start, dur / spb]);
    }
    landTakes(this.p, r.track, landed, () => this.p.nextId++);
    this.pushUndo(prev); this.changed();
    this.settleRecTrack();
  }
  private recPeaks = new Map<Id, Float32Array>();

  // ---------- telemetry ----------
  async subscribeTelemetry(cb: (b: ArrayBuffer) => void) { this.teleCb = cb; }
  async watchFx(ids: Id[]) { this.watched = ids.slice(0, 8); }

  private tick() {
    if (!this.teleCb) return;
    const seq = ++this.seq, beat = this.beatNow(), playing = this.tr.playing, t = performance.now() / 1000;
    const spb = this.spb(), frac = ((beat % 1) + 1) % 1;
    // meters: inserts whose tracks have a live clip under the playhead
    const live = new Map<Id, number>();
    if (playing && beat >= 0) for (const c of this.p.clips) {
      if (c.muted || beat < c.start || beat >= c.start + c.len) continue;
      const tr = this.p.tracks[c.track]; if (!tr || tr.muted) continue;
      const lvl = (c.kind === 'pat' ? 0.25 + 0.55 * Math.exp(-frac * 5) : 0.45 + 0.2 * Math.sin(t * 7)) * 2 * tr.vol ** 3 * dbg(c.gain);
      live.set(tr.insert, Math.max(live.get(tr.insert) ?? 0, lvl));
    }
    // pattern loop preview and auditions (engine: rt.rs schedule_pattern / Cmd::Audition)
    let patBeat = -1, direct = 0; // `direct`: what plays straight into the master
    if (this.patPlay) {
      const pt = this.p.patterns.find(x => x.id === this.patPlay!.id);
      if (!pt) this.patPlay = null;
      else {
        patBeat = this.patPos(pt.len, spb);
        // each channel into its own insert, else the master: never the playlist track's insert
        const pf = ((patBeat % 1) + 1) % 1, lvl = 0.25 + 0.55 * Math.exp(-pf * 5);
        for (const id of new Set(pt.notes.map(n => this.p.channels?.find(c => c.id === n.ch)?.insert ?? 0))) {
          if (id === 0) direct = Math.max(direct, lvl); else live.set(id, Math.max(live.get(id) ?? 0, lvl));
        }
      }
    }
    for (const [id, at] of this.flash) {
      const age = (performance.now() - at) / 1000;
      if (age > 0.6) { this.flash.delete(id); continue; }
      live.set(id, Math.max(live.get(id) ?? 0, 0.7 * Math.exp(-age * 9)));
    }
    let master = direct;
    const lvOf = new Map<Id, number>();
    for (const m of this.p.inserts) if (m.id !== 0) {
      const lv = (live.get(m.id) ?? 0) * (m.muted ? 0 : 2 * m.vol ** 3) * (0.9 + 0.1 * Math.random());
      lvOf.set(m.id, lv); master += lv * 0.6;
    }
    const ins = this.p.inserts.map(m => {
      const lv = m.id === 0 ? Math.min(1.2, master * (m.muted ? 0 : 2 * m.vol ** 3)) : lvOf.get(m.id) ?? 0;
      return { id: m.id, l: lv * Math.min(1, 1 - m.pan), r: lv * Math.min(1, 1 + m.pan) };
    });
    const insLvl = new Map(ins.map(e => [e.id, Math.max(e.l, e.r)]));
    const fx: EncFx[] = [];
    if (seq % 2 === 0) for (const id of this.watched) {
      const found = this.findFx(id); if (!found) continue;
      fx.push(this.fakeAnalysis(found.fx, found.ins, playing ? (insLvl.get(found.ins) ?? 0) + 0.05 : 0, t, beat, playing, patBeat));
    }
    let rec = null;
    if (this.rec && playing && this.rawBeat() >= this.rec.clipStart) {
      const r = this.rec;
      if (!r.t0) r.t0 = performance.now();
      const want = Math.floor((performance.now() - r.t0) / 10);
      while (r.peaks.length / 2 < want) { const a = 0.15 + 0.6 * Math.abs(Math.sin(r.peaks.length / 37)) * Math.random(); r.peaks.push(-a, a); }
      // the live clip restarts every loop pass, like the engine's telemetry
      const marks = this.passMarks(r, r.peaks.length / 2), pass = marks.length, p0 = pass ? marks[pass - 1] : 0;
      if (pass !== r.pass) { r.pass = pass; r.sent = p0; }
      const off = r.sent - p0; r.sent = r.peaks.length / 2;
      rec = { track: r.track, clipStart: pass ? r.lap![0] : r.clipStart, seconds: (r.peaks.length / 2 - p0) / 100, peakOffset: off, peaks: r.peaks.slice((off + p0) * 2) };
    }
    let previewPos = -1;
    if (this.preview) {
      const el = (performance.now() - this.preview.t0) / 1000;
      previewPos = this.preview.off + el * (this.preview.rate ?? 1);
      if (el > this.preview.dur) { this.preview = null; previewPos = -1; }
    }
    const inLevel = this.tr.rec ? 0.2 + 0.5 * Math.abs(Math.sin(t * 3)) * Math.random() : 0;
    void spb;
    this.teleCb(encodeFrame({ seq, beat, playing, recording: !!this.rec && this.rawBeat() >= (this.rec?.clipStart ?? 0), previewing: !!this.preview,
      inLevel, previewPos, cpu: playing ? 0.12 + 0.03 * Math.random() : 0.02, ins, fx, rec, patBeat, underruns: this.underrunTotal }));
  }

  private findFx(id: Id): { fx: Fx; ins: Id } | null {
    for (const m of this.p.inserts) { const f = m.fx.find(x => x.id === id); if (f) return { fx: f, ins: m.id }; }
    return null;
  }

  private sims = new Map<Id, FxSim>();
  /** The vocal heard on insert `ins` now: a vocal clip under the playhead, or (for vocal processors) the looping demo take. */
  private vocalFor(fx: Fx, ins: Id, beat: number, t: number, playing: boolean): VocalSample | null {
    if (!playing) return null;
    for (const c of this.p.clips) {
      if (c.kind !== 'audio' || c.muted || beat < c.start || beat >= c.start + c.len) continue;
      const tr = this.p.tracks[c.track]; if (!tr || tr.insert !== ins || tr.muted) continue;
      const m = this.srcMeta.get(c.ref); if (!m?.vocal) continue;
      // source seconds: a stretched clip reads its source `stretch` times slower
      return vocalAt(synthVocal(m.seed, m.dur), (c.offset + beat - c.start) * this.spb() / (c.stretch ?? 1));
    }
    return fx.type === 'tune' || fx.type === 'voice' || fx.type === 'deess' || fx.type === 'fet' ? vocalAt(synthVocal(4242, VOCAL_DUR), t) : null;
  }

  private fakeAnalysis(fx: Fx, ins: Id, level: number, t: number, beat: number, playing: boolean, patBeat = -1): EncFx {
    const specIn = new Float32Array(SPEC_BANDS), specOut = new Float32Array(SPEC_BANDS);
    const scopeIn = new Float32Array(SCOPE_LEN), scopeOut = new Float32Array(SCOPE_LEN);
    const meters = new Float32Array(32);
    let sim = this.sims.get(fx.id); if (!sim) { sim = newSim(); this.sims.set(fx.id, sim); }
    const dt = sim.t ? clamp(t - sim.t, 0, 0.25) : 1 / 30; sim.t = t;
    const voc = this.vocalFor(fx, ins, beat, t, playing);
    if (voc) { voc.level *= 0.4; level = voc.level; }
    const lvDb = level > 1e-4 ? 20 * Math.log10(level) : -120;
    const pulse = voc ? 1 - voc.level : Math.exp(-(((beat % 1) + 1) % 1) * 4);
    if (voc) vocalSpec(voc, specIn);
    else {
      for (let k = 0; k < SPEC_BANDS; k++) {
        const f = BAND_F[k], oct = Math.log2(f / 1000);
        const shape = -4.5 * oct - 6 * Math.max(0, oct - 2.5) ** 2 + 10 * Math.exp(-((Math.log2(f / 70)) ** 2) * 2) * pulse;
        specIn[k] = level > 0 ? lvDb - 28 + shape + (Math.random() - 0.5) * 5 : -120;
      }
    }
    const pr = fx.params;
    let gr = 0, out = level;
    if (fx.type === 'eq' && fx.bands) {
      // dynamic bands: the band's level vs its threshold, 6 dB soft knee, full range 12 dB over (dsp.rs)
      fx.bands.forEach((b, k) => {
        if (!b.on || !b.dynamic) { sim!.dyn[k] = 0; return; }
        const bi = clamp(Math.round(Math.log(b.f / 20) / Math.log(1000) * (SPEC_BANDS - 1)), 0, SPEC_BANDS - 1);
        let e = 0; for (let j = Math.max(0, bi - 3); j <= Math.min(SPEC_BANDS - 1, bi + 3); j++) e += Math.pow(10, specIn[j] / 10);
        const lvl = 10 * Math.log10(e + 1e-12) + 6;
        const target = b.dynamic.range * clamp((lvl - b.dynamic.thr + 3) / 15, 0, 1);
        const tau = Math.abs(target) > Math.abs(sim!.dyn[k]) ? 0.01 : 0.08; // fast in, slower out
        sim!.dyn[k] += (target - sim!.dyn[k]) * (1 - Math.exp(-dt / tau));
        meters[k] = sim!.dyn[k];
      });
      const c = eqCurve(fx.bands, pr.out ?? 0, BAND_F, new Float32Array(SPEC_BANDS), 'mid', sim.dyn);
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k] + (fx.bypass ? 0 : c[k]);
    } else if (fx.type === 'tune') {
      this.simTune(fx, sim, voc, dt);
      for (let i = 0; i < 6; i++) meters[i] = sim.m[i];
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k];
    } else if (fx.type === 'voice') {
      // vocal.rs `Voice`: [input Hz, output Hz, confidence, applied shift st]
      const mode = Math.round(pr.mode ?? 0), pitch = pr.pitch ?? 0, drive = pr.drive ?? 0;
      if (voc && voc.midi != null && voc.level > 0.02) {
        const m = voc.midi, shift = clamp(mode === 1 ? Math.round(m) - m + pitch : mode === 2 ? 60 + Math.round(pitch) - m : pitch, -24, 24);
        meters[0] = midiToHz(m); meters[1] = midiToHz(m + shift); meters[2] = 0.88 + Math.random() * 0.1; meters[3] = shift;
      } else meters[2] = 0.15 + Math.random() * 0.2;
      const fShift = (pr.formant ?? 0) + ((pr.link ?? 0) >= 0.5 ? meters[3] : 0);
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[clamp(Math.round(k - fShift / 12 * (SPEC_BANDS / 10)), 0, SPEC_BANDS - 1)] + drive * 0.15 * (k > 60 ? 2 : 1) + (pr.out ?? 0);
      out = level * dbg(pr.out ?? 0);
    } else if (fx.type === 'fet') {
      const inDb = level > 1e-5 ? 20 * Math.log10(level) + (pr.input ?? 8) : -120;
      const ri = Math.round(pr.ratio ?? 0), abi = ri >= 4, ratio = [4, 8, 12, 20, 20][clamp(ri, 0, 4)];
      const over = inDb + 10, knee = abi ? 2 : ratio <= 4 ? 8 : 4, slope = 1 / ratio - 1;
      let target = over <= -knee / 2 ? 0 : over >= knee / 2 ? slope * over : slope * (over + knee / 2) ** 2 / (2 * knee);
      if (abi && over > 0) target -= 4 * Math.min(1, over / 6) * (1 - Math.min(1, over / 30));
      const rel = (pr.release ?? 300) / 1000 * (abi ? 1.4 : 1);
      sim.gr = target < sim.gr ? target : target + (sim.gr - target) * Math.exp(-dt / rel);
      gr = sim.gr;
      const outDb = inDb + gr + (pr.output ?? 0);
      meters[0] = gr; meters[1] = Math.max(-120, inDb); meters[2] = Math.max(-120, outDb);
      out = dbg(outDb);
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k] + (pr.input ?? 8) + gr + (pr.output ?? 0);
    } else if (fx.type === 'denoise') {
      // noise floor ~ −62 dB tilted toward the lows, per-band reduction where the input is near it
      const red = pr.reduction ?? 12;
      let tot = 0;
      for (let b = 0; b < 16; b++) {
        const f = 50 * Math.pow(320, (b + 0.5) / 16), nz = -58 - 10 * Math.log10(f / 50);
        let near = 0, c = 0;
        for (let k = 0; k < SPEC_BANDS; k++) if (BAND_F[k] >= 50 * Math.pow(320, b / 16) && BAND_F[k] < 50 * Math.pow(320, (b + 1) / 16)) { near += specIn[k]; c++; }
        const lv = c ? near / c : -120, snr = lv - nz;
        const gdb = -Math.min(red, Math.max(0, 10 - snr) * red / 10);
        meters[b] = nz; meters[16 + b] = gdb; tot += gdb;
      }
      for (let k = 0; k < SPEC_BANDS; k++) { const b = Math.max(0, Math.min(15, Math.floor(Math.log(BAND_F[k] / 50) / Math.log(320) * 16))); specOut[k] = specIn[k] + meters[16 + b]; }
      gr = tot / 16; out = level * dbg(gr * 0.3);
    } else if (fx.type === 'deess') {
      const sib = voc?.sib ?? 0, lv = voc?.level ?? 0;
      const hiDb = lv > 1e-4 ? 20 * Math.log10(lv) + (sib > 0 ? 3 + 4 * sib : -26 + (Math.random() - 0.5) * 3) : -120;
      const over = Math.max(0, hiDb - (pr.thr ?? -32));
      const target = -Math.min(over * 0.8 * sib, pr.range ?? 8);
      sim.gr = target < sim.gr ? target + (sim.gr - target) * Math.exp(-dt / 0.004) : target + (sim.gr - target) * Math.exp(-dt / 0.06);
      gr = sim.gr;
      meters[0] = gr; meters[1] = Math.max(-120, hiDb); meters[2] = sib; meters[3] = pr.thr ?? -32;
      out = level * dbg(gr * (pr.mode ? 1 : 0.85));
      const fc = pr.freq ?? 6000;
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k] + (pr.mode || BAND_F[k] >= fc ? gr : 0);
    } else if (fx.type === 'comp' || fx.type === 'limit') {
      const inDb = lvDb + (fx.type === 'limit' ? pr.gain : 0), thr = fx.type === 'limit' ? pr.ceil : pr.thr;
      const over = inDb - thr;
      gr = level > 0 && over > 0 ? -(fx.type === 'limit' ? over : over - over / pr.ratio) : 0;
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k] + gr + (pr.gain ?? 0);
      out = level * dbg(gr);
    } else if (fx.type === 'reverb') {
      const hi = pr.hi ?? 8000, lo = pr.lo ?? 120, wet = 20 * Math.log10(Math.max(1e-3, (pr.mix ?? 25) / 100));
      const sustain = 6 * Math.log10(1 + (pr.size ?? 2));
      for (let k = 0; k < SPEC_BANDS; k++) {
        const f = BAND_F[k], r = (f / lo) ** 2;
        specOut[k] = level > 0 ? specIn[k] - 6 * (1 - pulse) + sustain + wet - 10 * Math.log10(1 + (f / hi) ** 2) + 20 * Math.log10(r / Math.sqrt(1 + r * r)) : -120;
      }
    } else if (fx.type === 'sat') {
      // harmonics grow with the drive; auto gain keeps the level (dsp/saturator.rs)
      const dd = satDriveDb(pr);
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k] + (pr.auto ?? 1 ? 0 : dd * 0.5) + (k > 60 ? dd * 0.3 : 0);
    } else if (fx.type === 'warp') {
      // dsp::warp meters: [0] bar position (−1 stopped), [1]/[2] playing slots, [3]/[4] queued (−1), [9] hold, [10] seconds buffered
      // like the engine, tempo-locked effects follow the pattern loop preview too
      const b = playing ? beat : patBeat, on = playing || patBeat >= 0;
      meters[0] = on ? ((b % 4) + 4) % 4 : -1;
      meters[1] = Math.round(pr.tslot ?? 0); meters[2] = Math.round(pr.vslot ?? 0); meters[3] = -1; meters[4] = -1;
      meters[9] = Math.round(pr.hold ?? 0) ? 2 : 0; meters[10] = on ? Math.min(12.5, Math.max(0, b) * 60 / this.p.bpm) : 0;
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k];
    } else if (fx.type === 'chorus' || fx.type === 'phaser') {
      // dsp::modulation meters. Chorus: [0] LFO phase, [1..4]/[4..7] left/right voices' delay ms, [7] voices.
      // Phaser: [0]/[1] stages' break frequency left/right, [2] stages, [3] LFO phase.
      const synced = (pr.sync ?? 0) >= 0.5, div = [0.25, 0.5, 1, 2, 4, 8, 16, 32][Math.round(pr.div ?? 4)] ?? 4;
      const ph = synced ? (((beat / div) % 1) + 1) % 1 : (t * (pr.rate ?? 0.5)) % 1;
      if (fx.type === 'chorus') {
        const mode = Math.round(pr.mode ?? 0), voices = mode === 1 ? 3 : mode === 2 ? 1 : 2;
        const depth = (pr.depth ?? 40) / 100, base = pr.delay ?? 12, swing = mode === 2 ? 2 : Math.min(depth * 0.25 * base, 6);
        meters[0] = ph; meters[7] = voices;
        for (let c = 0; c < 2; c++) for (let v = 0; v < voices; v++) {
          const centre = mode === 2 ? swing + 1 : voices === 2 && v === 1 ? base * 1.37 : base;
          meters[1 + c * 3 + v] = centre + swing * Math.sin(2 * Math.PI * (ph + (c ? (mode === 1 ? 1 / 6 : mode === 2 ? 0 : 0.25) : 0) + v / voices));
        }
      } else {
        const tri = (x: number) => { const q = ((x % 1) + 1) % 1; return q < 0.5 ? 4 * q - 1 : 3 - 4 * q; };
        const c = pr.center ?? 800, d = (pr.depth ?? 70) / 100, st = (pr.stereo ?? 90) / 360;
        meters[0] = c * Math.pow(2, 3 * d * tri(ph)); meters[1] = c * Math.pow(2, 3 * d * tri(ph + st));
        meters[2] = [2, 4, 6, 8, 12][Math.round(pr.stages ?? 1)] ?? 4; meters[3] = ph;
      }
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k];
    } else if (fx.type === 'delay') {
      // dsp::delay meters: [0]/[1] read heads ms (LFO on time), [2] loop dB, [3] drive dB, [4] LFO, [5] model, [6] wet dB, [7] time ms, [8] ducking dB
      const model = Math.round(pr.model ?? 1), base = delayMs(pr, this.p.bpm), lfo = Math.sin(t * 2 * Math.PI * (pr.rate ?? 0.5));
      const mod = (pr.mtime ?? 0) / 100 * lfo * Math.min(6, 0.45 * base), off = (pr.offset ?? 0) / 100;
      const loopDb = level > 0 ? lvDb + 20 * Math.log10(Math.max(1e-3, Math.min(1, loopGain(pr)) * 0.9)) : -120;
      meters[0] = model === 3 ? 0 : base * (model === 0 ? 1 + Math.max(0, -off) : 1) + mod;
      meters[1] = model === 3 ? 0 : base * (model === 0 ? 1 + Math.max(0, off) : 1) + mod;
      meters[2] = Math.min(loopDb, pr.level ?? 0); meters[3] = Math.min(0, (pr.level ?? 0) - Math.max(lvDb, -60)) * 0.3;
      // dsp::ducker: −45 … −15 dBFS of input ramps the wet down by 0 … Ducking dB
      const duckDb = level > 0 ? -(pr.duck ?? 0) * Math.min(1, Math.max(0, (lvDb + 45) / 30)) : 0;
      meters[4] = lfo; meters[5] = model; meters[6] = loopDb + duckDb + 20 * Math.log10(Math.max(1e-3, (pr.mix ?? 25) / 100)); meters[7] = base;
      meters[8] = duckDb;
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k] + 20 * Math.log10(1 + (pr.mix ?? 25) / 100);
    } else {
      for (let k = 0; k < SPEC_BANDS; k++) specOut[k] = specIn[k] + 20 * Math.log10(1 + (pr.mix ?? 25) / 100);
    }
    const drive = fx.type === 'sat' ? dbg(satDriveDb(pr)) * 0.6 : 1, f0 = 2 + 3 * Math.sin(t * 0.3);
    for (let i = 0; i < SCOPE_LEN; i++) {
      const x = level * (Math.sin(i / SCOPE_LEN * Math.PI * 2 * f0 + t * 9) * 0.8 + (Math.random() - 0.5) * 0.12);
      scopeIn[i] = x;
      scopeOut[i] = fx.type === 'sat' ? Math.tanh(x * drive) / ((pr.auto ?? 1) ? drive : 0.6) * dbg(pr.out ?? 0) : x * dbg(gr);
    }
    return { id: fx.id, inPeak: level, outPeak: out, grDb: gr, meters, specIn, specOut, scopeIn, scopeOut };
  }

  /** vocal.rs `Tune::process`, stepped every 4 ms hop. */
  private simTune(fx: Fx, s: FxSim, voc: VocalSample | null, dt: number) {
    const p = fx.params, key = Math.round(p.key ?? 0), mask = scaleMask(p.scale ?? 0);
    const vib = (p.vibrato ?? 0) / 100, flex = (p.flex ?? 0) / 100, hum = (p.humanize ?? 0) / 100, tr = p.transpose ?? 0;
    const hop = 0.004, steps = Math.max(1, Math.round(dt / hop));
    for (let i = 0; i < steps; i++) {
      if (voc && voc.midi != null && voc.level > 0.02) {
        const m = voc.midi + (Math.random() - 0.5) * 0.02;
        if (s.center === 0 || Math.abs(m - s.center) > 1.5) s.center = m;
        s.center += (m - s.center) * Math.min(1, hop / 0.15);
        const fol = s.center * vib + m * (1 - vib);
        const note = snapNote(fol, key, mask);
        if (note !== s.last) { s.last = note; s.held = 0; } else s.held += hop;
        const err = note - fol, win = 1 - 0.9 * flex;
        const want = flex > 0 && Math.abs(err) > win * 0.5 ? 0 : err;
        let tau = (p.retune ?? 20) / 1000;
        if (hum > 0 && s.held > 0.25) tau += hum * 0.4 * Math.min(1, (s.held - 0.25) / 0.5);
        const k = tau <= 0.0005 ? 1 : 1 - Math.exp(-hop / tau);
        s.corr += (want - s.corr) * k;
        s.m = [midiToHz(m), midiToHz(note + tr), midiToHz(m + s.corr + tr), 0.88 + Math.random() * 0.1, (m - note) * 100, s.corr * 100];
      } else {
        s.corr *= 0.9; s.center = 0;
        s.m = [0, 0, 0, 0.15 + Math.random() * 0.2, 0, 0];
      }
    }
  }

  // ---------- catalog / audio files ----------
  async fxCatalog() { await delay(); return structuredClone(this.cat); }
  async importAudio(a: { paths: string[]; track?: number; beat?: number; step?: boolean }) {
    await delay(30);
    const prev = structuredClone(this.p);
    const sourceIds: Id[] = [], clipIds: Id[] = [];
    a.paths.forEach((path, k) => {
      // a file of the sample library is one source however often it is used (engine.rs `import_audio`)
      const lib = path.startsWith(LIB_SCHEME), had = lib ? this.p.sources.find(s => s.path === path) : undefined;
      const fac = factoryFile(path);
      const name = fac?.name ?? (path.split(/[\\/]/).pop() ?? path).replace(/\.[^.]+$/, '');
      const h = hash(path), id = had?.id ?? this.p.nextId++;
      const vocal = isVocalName(name);
      const mins = /(\d+)\s*min/i.exec(name); // e.g. "Long take 4min.wav" (dev/perf checks)
      const dur = had?.dur ?? fac?.dur ?? (mins ? +mins[1] * 60 : vocal ? VOCAL_DUR : /loop/i.test(name) ? 4 * 4 * this.spb() : 0.6 + (h % 700) / 100);
      if (!had) {
        const src: MockSource = fac
          ? { id, name, color: fac.color, path, dur, sr: 44100, ch: fac.ch, peakDb: -1, seed: h }
          : { id, name, color: vocal ? '#d9678f' : TC[(this.p.sources.length + 3) % TC.length], path, dur, sr: 48000, ch: vocal ? 1 : 2, peakDb: -1 - (h % 60) / 10, seed: h, vocal };
        this.srcMeta.set(id, src); this.p.sources.push(strip(src));
      }
      sourceIds.push(id);
      if (a.track != null && a.beat != null) {
        const tr = clamp(a.track + k, 0, this.p.tracks.length - 1), cid = this.p.nextId++;
        const len = Math.min(dur / this.spb(), 512 - a.beat);
        this.p.clips.push(mkClip(cid, tr, a.beat, len, 'audio', id)); clipIds.push(cid);
      }
    });
    // like the engine (import_audio_with): one undo step, unless the caller loads the files for an edit
    // that follows; a new source then still clears redo
    const added = this.p.sources.length !== prev.sources.length;
    const fold = a.step === false && !clipIds.length && added;
    if (!fold && (a.step !== false || clipIds.length)) this.pushUndo(prev);
    else if (added) this.redoStack = [];
    this.changed();
    // the edit that follows (the sampler channel) is the step, and it takes the source with it
    if (fold) this.importBase = { rev: this.revision, p: this.importBase && this.importBase.rev + 1 === this.revision ? this.importBase.p : prev };
    return { sourceIds, clipIds };
  }
  async pickAudioFiles(): Promise<string[] | null> {
    return new Promise(res => {
      const inp = document.createElement('input');
      inp.type = 'file'; inp.accept = 'audio/*'; inp.multiple = true;
      inp.onchange = () => res(inp.files && inp.files.length ? [...inp.files].map(f => f.name) : null);
      inp.click();
    });
  }
  async sourcePeaks(id: Id, norm?: boolean, rev?: boolean) {
    await delay(5);
    const s = this.srcMeta.get(id) ?? this.p.sources.find(x => x.id === id);
    if (!s) throw new Error('no source ' + id);
    if (this.moved.has(this.p.sources.find(x => x.id === id)?.path ?? '')) throw new Error('audio not loaded');
    let pk: Float32Array;
    const meta = this.srcMeta.get(id);
    if (this.recPeaks.has(id)) pk = this.recPeaks.get(id)!.slice();
    else if (meta?.trimFrom) {
      const full = await this.sourcePeaks(meta.trimFrom.id);
      const a = Math.round(meta.trimFrom.offSec * 100) * 2;
      pk = full.slice(a, a + Math.round(s.dur * 100) * 2);
    } else if (meta?.vocal) pk = vocalPeaks(synthVocal(meta.seed, s.dur));
    else if (s.path.startsWith(LIB_SCHEME)) pk = oneShotPeaks(meta?.seed ?? id, s.dur);
    else pk = synthPeaks(meta?.seed ?? id, s.dur, 128);
    if (rev) { const n = pk.length / 2, o = new Float32Array(pk.length); for (let i = 0; i < n; i++) { o[2 * i] = pk[2 * (n - 1 - i)]; o[2 * i + 1] = pk[2 * (n - 1 - i) + 1]; } pk = o; }
    if (norm) { let m = 0; for (const v of pk) m = Math.max(m, Math.abs(v)); const g = m > 1e-6 ? dbg(-0.3) / m : 1; for (let i = 0; i < pk.length; i++) pk[i] *= g; }
    return pk;
  }
  private renderReady = 0;
  async clipPeaks(clipId: Id) {
    await delay(Math.max(0, this.renderReady - performance.now())); // the render of the last gesture
    const c = this.p.clips.find(x => x.id === clipId); if (!c || c.kind !== 'audio') throw new Error('no such clip');
    const tune = structuredClone(c.tune ?? null);
    const pk = await this.sourcePeaks(c.ref, c.norm, c.rev);
    await delay(20);
    if (tune?.notes.length) {
      const n = pk.length / 2;
      for (let i = 0; i < n; i++) {
        const j = noteAt(tune.notes, i / 100); if (j < 0) continue;
        const nt = tune.notes[j], edge = clamp(Math.min(i / 100 - nt.start, nt.end - i / 100) / 0.01, 0, 1);
        const g = 1 + (dbg(nt.gain) - 1) * edge;
        pk[2 * i] = clamp(pk[2 * i] * g, -1, 0); pk[2 * i + 1] = clamp(pk[2 * i + 1] * g, 0, 1);
      }
    }
    return pk;
  }
  async pitchAnalyze(clipId: Id): Promise<PitchAnalysis> {
    await delay(120); // the engine analyses on a worker thread; keep the loading state visible
    const c = this.p.clips.find(x => x.id === clipId);
    if (!c) throw new Error('no such clip');
    if (c.kind !== 'audio') throw new Error('only audio clips have pitch');
    const s = this.srcMeta.get(c.ref) ?? this.p.sources.find(x => x.id === c.ref) as MockSource | undefined;
    if (!s) throw new Error('audio not loaded');
    const v = synthVocal(s.seed ?? s.id, s.dur);
    return structuredClone({ rate: v.rate, curve: v.curve, levels: v.levels, notes: v.notes, confidence: v.curve.map(x => (x == null ? 0 : 0.9)), backend: this.f0Backend });
  }
  async pitchTarget(clipId: Id, reference?: RefTune): Promise<PitchTarget> {
    const c = this.p.clips.find(x => x.id === clipId);
    if (!c || c.kind !== 'audio') throw new Error('only audio clips have pitch');
    const p = reference ?? c.tune?.reference;
    if (!p) throw new Error('this clip has no reference');
    const take = await this.pitchAnalyze(clipId);
    if (p.melody) return melodyTarget(take, c.tune?.notes.length ? c.tune.notes : take.notes, p.melody.notes, p);
    const rs = this.srcMeta.get(p.source) ?? this.p.sources.find(x => x.id === p.source) as MockSource | undefined;
    if (!rs) throw new Error(`no reference source ${p.source}`);
    const rv = synthVocal(rs.seed ?? rs.id, rs.dur);
    const notes = c.tune?.notes.length ? c.tune.notes : take.notes;
    return simpleTarget(take, { rate: rv.rate, curve: rv.curve, levels: rv.levels, notes: rv.notes }, notes, p);
  }
  async melodyChannels(clipId: Id): Promise<MelodyChannel[]> {
    const c = this.p.clips.find(x => x.id === clipId);
    if (!c || c.kind !== 'audio') throw new Error('only audio clips have pitch');
    const take = await this.pitchAnalyze(clipId), spb = 60 / this.p.bpm, st = c.stretch ?? 1;
    const region: [number, number] = [c.offset * spb / st, (c.offset + c.len) * spb / st];
    return (this.p.channels ?? [])
      .map(ch => { const ns = collectMelody(this.p, c, ch.id); return { channel: ch.id, name: ch.name, notes: ns.length, fit: melodyFit(take, region, ns) }; })
      .filter(x => x.notes > 0)
      .sort((a, b) => b.fit - a.fit || b.notes - a.notes);
  }
  async melodyReference(clipId: Id, channel: Id, span?: [number, number] | null): Promise<{ melody: Melody; fit: number }> {
    const c = this.p.clips.find(x => x.id === clipId);
    if (!c || c.kind !== 'audio') throw new Error('only audio clips have pitch');
    if (!(this.p.channels ?? []).some(x => x.id === channel)) throw new Error('no such channel');
    const take = await this.pitchAnalyze(clipId), spb = 60 / this.p.bpm, st = c.stretch ?? 1;
    const notes = collectMelody(this.p, c, channel, span);
    return structuredClone({ melody: { channel, ...(span ? { span } : {}), notes }, fit: melodyFit(take, [c.offset * spb / st, (c.offset + c.len) * spb / st], notes) });
  }
  private f0Backend = 'yin';
  async f0ModelStatus(): Promise<F0ModelStatus> {
    return {
      id: 'rmvpe', name: 'RMVPE vocal pitch model', bytes: 361_688_443, installed: this.f0Backend === 'rmvpe', backend: this.f0Backend,
      licenseNote: 'RMVPE weights from the RVC project (lj1995/VoiceConversionWebUI). The RVC code is MIT and the RMVPE reference code Apache-2.0, but the weights\' licence file restricts use to research. Downloading is your decision; without the model WAVE uses its built-in YIN detector.',
      licenseUrl: 'https://huggingface.co/lj1995/VoiceConversionWebUI/blob/main/%E4%BD%BF%E7%94%A8%E9%9C%80%E9%81%B5%E5%AE%88%E7%9A%84%E5%8D%8F%E8%AE%AE-LICENSE.txt',
      path: 'C:\\Users\\michal\\AppData\\Local\\WAVE\\Models\\rmvpe.onnx', error: null,
    };
  }
  async f0ModelInstall(): Promise<F0ModelStatus> {
    const total = 361_688_443;
    for (let k = 1; k <= 20; k++) { await delay(50); this.emit('model-progress', { id: 'rmvpe', done: Math.round(total * k / 20), total }); }
    this.f0Backend = 'rmvpe';
    return this.f0ModelStatus();
  }
  async musicAnalyze(a?: { clipId?: Id; excludeFx?: Id }): Promise<MusicAnalysis> {
    await delay(80);
    if (a?.clipId != null) {
      const c = this.p.clips.find(x => x.id === a.clipId);
      if (!c) throw new Error('no such clip');
      if (c.kind !== 'audio') throw new Error('only audio clips have tempo and key');
      const s = this.p.sources.find(x => x.id === c.ref);
      const bpm = Math.round(this.p.bpm);
      // C minor profile (demo song is in C minor at 128 BPM)
      const root = (s ? s.id % 2 === 0 ? 0 : 0 : 0), minor = true;
      const chroma = [1, 0.08, 0.42, 0.78, 0.09, 0.51, 0.07, 0.89, 0.36, 0.12, 0.44, 0.19];
      return {
        tempo: {
          bpm,
          confidence: 0.86,
          candidates: [
            { bpm, score: 1 },
            { bpm: bpm / 2, score: 0.58 },
            { bpm: bpm * 2, score: 0.44 },
          ],
          firstBeat: 0,
        },
        key: {
          root, minor, confidence: 0.84,
          candidates: [
            { root, minor: true, score: 0.91 },
            { root: (root + 3) % 12, minor: false, score: 0.78 },
            { root: (root + 7) % 12, minor: true, score: 0.62 },
          ],
          chroma,
          tuningCents: 0,
        },
      };
    }
    const exIns = a?.excludeFx != null ? this.p.inserts.find(ins => ins.fx.some(f => f.id === a.excludeFx))?.id : undefined;
    const anySolo = this.p.tracks.some(t => t.solo);
    const active = this.p.clips.filter(c => {
      if (c.kind !== 'audio' || c.muted) return false;
      const tr = this.p.tracks[c.track];
      return tr && !tr.muted && (!anySolo || tr.solo);
    });
    const nonEx = exIns != null ? active.filter(c => this.p.tracks[c.track]?.insert !== exIns) : active;
    const chosen = nonEx.length ? nonEx : active;
    if (!chosen.length) return { tempo: null, key: null };
    return this.musicAnalyze({ clipId: chosen[0].id });
  }
  /** Same contract as the engine's `auto_vocal_plan`, with fixed measurements of a typical take
   *  (the real calibration renders the audio; the mock has no audio to render). */
  async autoVocalPlan(a: { clipId: Id; beatClipId?: Id; style?: AutoVocalStyle; bakeRider?: boolean }): Promise<AutoVocalPlan> {
    await delay(300);
    const c = this.p.clips.find(x => x.id === a.clipId);
    if (!c) throw new Error('no such clip');
    if (c.kind !== 'audio') throw new Error('only audio clips have pitch');
    const style = a.style ?? 'natural';
    const music = a.beatClipId != null ? await this.musicAnalyze({ clipId: a.beatClipId }) : await this.musicAnalyze();
    const cat = mockCatalog();
    const params = (t: AnyBuiltinKind, kv: Record<string, number>) =>
      ({ ...Object.fromEntries(cat.types[t]!.params.map(d => [d.id, d.def])), ...kv });
    const hard = style === 'hard';
    const key = music.key;
    const names = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
    const keyName = key ? `${names[key.root]} ${key.minor ? 'minor' : 'major'}` : '';
    const tune = key
      ? params('tune', { key: key.root, scale: key.minor ? 2 : 1, retune: hard ? 0 : 40, humanize: hard ? 0 : 30, flex: hard ? 0 : 40, vibrato: hard ? 20 : 60 })
      : params('tune', {});
    const chain: AutoVocalPlan['chain'] = [
      { type: 'tune', params: tune, mix: 1, enabled: !!key, note: key ? `${keyName} from the beat (confidence ${key.confidence.toFixed(2)})` : 'no key detected in the beat: pitch correction left off' },
      { type: 'rider', params: params('rider', { amount: 50, phrase: 20, up: 5, down: 6, speed: hard ? 1 : 0, auto: 0, target: -24, floor: -70, gate: -48, look: 1, out: 0 }), mix: 1, enabled: true, note: 'target -24.0 dB, noise floor -70.0 dB; gain p5/p50/p95 -2.1/0.3/2.4 dB' },
      { type: 'fet', params: params('fet', { input: 12, output: -12, ratio: 0, attack: 800, release: 60, mix: 100 }), mix: 1, enabled: true, note: 'fast 4:1 peak stage, 4.8 dB on the loudest 1 %' },
      { type: 'comp', params: params('comp', { thr: -32, ratio: 2.5, att: 10, rel: 250, gain: 0, det: 1 }), mix: 1, enabled: true, note: 'slow 2.5:1 stage, 4.1 dB mean reduction' },
      { type: 'deess', params: params('deess', { thr: -9, range: 12, freq: 5000, mode: 0, look: 5, listen: 0, detect: 1 }), mix: 1, enabled: true, note: 'sibilance -4.0 → -5.1 dB (pro median -6.0)' },
      { type: 'reverb', params: params('reverb', { size: 1.2, pre: 30, lo: 300, hi: 7000, width: 100, mix: MOCK_REVERB_MIX, tone: 0, space: 0, duck: 0, bass: 1 }), mix: 1, enabled: true, note: 'small plate, T60 1.2 s, -18 dB send' },
      { type: 'limit', params: params('limit', { gain: 0, ceil: -1, rel: 80, auto: 0, look: 5, link: 100, out: 0 }), mix: 1, enabled: true, note: 'true-peak ceiling -1 dBTP' },
    ];
    const ri = chain.findIndex(p => p.type === 'rider');
    const riderBake = a.bakeRider ? chain.splice(ri, 1)[0] : null;
    return {
      version: 1, style, riderBake,
      analysis: {
        durationS: c.len * 60 / this.p.bpm, sampleRate: 48000, peakDbfs: -3.2, truePeakDbtp: -3.0, lufs: -21.5,
        wordLevelDb: -24, noiseFloorDb: -70, sungShare: 0.62, sibilanceDb: -4,
        key: key ? { root: key.root, minor: key.minor, name: keyName, confidence: key.confidence } : null,
        scaleShare: key ? 0.86 : null, pitchOffCents: key ? 18 : null, beatLufs: a.beatClipId != null ? -11 : null,
      },
      chain,
      trackGainDb: a.beatClipId != null ? 6.4 : null,
      stages: { riderGainDb: [-2.1, 0.3, 2.4], fetP99GrDb: [4.8, 4.8], compMeanGrDb: [4.1, 4.1], sibilanceAfterDb: [-5.1, -5.1], iqr400Db: [5.6, 2.3], lufsAfter: -23.6, truePeakAfterDbtp: -6.2 },
      warnings: [],
    };
  }
  private liveAudition(): Record<string, Id> {
    const out: Record<string, Id> = {};
    for (const [k, t] of Object.entries(this.audition)) if (this.p.comps?.some(c => String(c.id) === k && c.takes.some(x => x.id === t))) out[k] = t;
    return out;
  }
  async compAudition(compId: Id, takeId: Id | null) {
    const cp = this.p.comps?.find(c => c.id === compId);
    if (!cp) throw new Error('no comp ' + compId);
    if (takeId != null && !cp.takes.some(t => t.id === takeId)) throw new Error(`comp ${compId} has no take ${takeId}`);
    if (takeId == null) delete this.audition[compId]; else this.audition[compId] = takeId;
    this.emit('state', this.dto());
  }
  async flattenComp(clipId: Id) {
    const c = this.p.clips.find(x => x.id === clipId);
    if (!c || c.kind !== 'comp') throw new Error('only comp clips can be flattened');
    const cp = this.p.comps?.find(x => x.id === c.ref); if (!cp) throw new Error("the clip's comp is missing");
    const spb = this.spb(), n = Math.round(c.len * spb * 100), out = new Float32Array(n * 2);
    const pk = new Map<Id, Float32Array>();
    for (const t of cp.takes) pk.set(t.id, await this.sourcePeaks(t.ref));
    for (let i = 0; i < n; i++) {
      const cb = c.offset + i / 100 / spb;
      const r = cp.regions.find(x => cb >= x.from && cb < x.to); if (!r) continue;
      const t = cp.takes.find(x => x.id === r.take)!, src = pk.get(t.id)!;
      if (cb >= takeEnd(t)) continue;
      const j = Math.floor((cb - t.start + t.offset) * spb * 100);
      if (j >= 0 && j * 2 + 1 < src.length) { out[2 * i] = src[2 * j]; out[2 * i + 1] = src[2 * j + 1]; }
    }
    const prev = structuredClone(this.p), id = this.p.nextId++;
    const src: MockSource = { id, name: cp.name + ' (flat)', color: '#e05263', path: 'recordings/' + cp.name + ' flat.wav', dur: n / 100, sr: 48000, ch: 1, peakDb: -3, seed: id };
    this.srcMeta.set(id, src); this.recPeaks.set(id, out); this.p.sources.push(strip(src));
    Object.assign(this.p.clips.find(x => x.id === clipId)!, { kind: 'audio', ref: id, offset: 0, norm: false, rev: false, tune: null });
    if (!this.p.clips.some(x => x.kind === 'comp' && x.ref === cp.id)) { this.p.comps = this.p.comps!.filter(x => x.id !== cp.id); delete this.audition[cp.id]; }
    this.pushUndo(prev); this.changed();
    return { sourceId: id };
  }
  /** The mock has no audio to render: the clip moves to a copy of its region (clip gain applied), as one undo step. */
  async bakeRider(a: { clipId: Id; params: Record<string, number> }) {
    await delay(200);
    const c = this.p.clips.find(x => x.id === a.clipId);
    if (!c || c.kind !== 'audio') throw new Error('only audio clips can be baked');
    const s = this.srcMeta.get(c.ref); if (!s) throw new Error('audio not loaded');
    const prev = structuredClone(this.p), id = this.p.nextId++;
    // the source region (source seconds); a time-stretched clip bakes its stretched audio (engine `bake_rider`)
    const st = c.stretch ?? 1, keep = st !== 1 && !c.resample;
    const spb = this.spb(), a0 = c.offset * spb / st, dur = Math.max(0, Math.min(s.dur, (c.offset + c.len) * spb / st) - a0);
    const src: MockSource = { ...s, id, name: s.name + ' (rider)', path: s.path.replace(/\.wav$/i, '') + ' rider.wav', dur: keep ? dur * st : dur, trimFrom: undefined };
    this.srcMeta.set(id, src);
    const pk = await this.sourcePeaks(c.ref, c.norm, c.rev);
    const g = dbg(c.gain);
    let baked = pk.slice(Math.floor(a0 * 100) * 2, Math.ceil((a0 + dur) * 100) * 2).map(v => v * g);
    if (keep) {
      const n = Math.round(baked.length / 2 * st), out = new Float32Array(n * 2);
      for (let i = 0; i < n; i++) { const j = Math.min(baked.length / 2 - 1, Math.floor(i / st)); out[2 * i] = baked[2 * j]; out[2 * i + 1] = baked[2 * j + 1]; }
      baked = out;
    }
    this.recPeaks.set(id, baked);
    this.p.sources.push(strip(src));
    Object.assign(c, { ref: id, offset: 0, gain: 0, norm: false, rev: false, tune: null });
    if (keep) { delete c.stretch; delete c.resample; }
    this.pushUndo(prev); this.changed();
    return { sourceId: id, clipId: c.id };
  }
  async trimClip(clipId: Id) {
    const c = this.p.clips.find(x => x.id === clipId); if (!c) throw new Error('no clip');
    const s = this.srcMeta.get(c.ref); if (!s) throw new Error('no source');
    const prev = structuredClone(this.p), spb = this.spb(), id = this.p.nextId++;
    // the source region in source seconds: a stretched clip keeps its stretch over the trimmed file
    const st = c.stretch ?? 1;
    const ns: MockSource = { ...s, id, dur: c.len * spb / st, path: s.path.replace(/(\.\w+)?$/, '-trim.wav'), trimFrom: { id: s.id, offSec: c.offset * spb / st } };
    this.srcMeta.set(id, ns); this.p.sources.push(strip(ns));
    Object.assign(this.p.clips.find(x => x.id === clipId)!, { ref: id, offset: 0, norm: false, rev: false });
    this.pushUndo(prev); this.changed();
    return { sourceId: id };
  }
  // ---------- missing audio (engine.rs `relink_sources` / `relink_search`) ----------
  /** Files moved on the fake disk: old path → where copies of it are now. A source whose path is a key is missing. */
  private moved = new Map<string, string[]>();
  private missing(): Record<string, string> {
    const out: Record<string, string> = {};
    for (const s of this.p.sources) if (this.moved.has(s.path)) out[s.id] = `${s.path}: file not found`;
    return out;
  }
  /** Dev/tests: those files moved (each to the given places; [] = deleted). Their sources go missing. */
  simulateMovedFiles(m: Record<string, string[]>) {
    for (const [from, to] of Object.entries(m)) this.moved.set(from, to);
    this.emit('state', this.dto());
  }
  private fileExists(path: string) { return !this.moved.has(path) && /^([a-z]:[\\/]|[\\/])/i.test(path) && /\.(wav|flac|ogg|oga|mp3)$/i.test(path); }
  async relinkSources(items: { sourceId: Id; path: string }[]) {
    await delay(20);
    for (const it of items) {
      const s = this.p.sources.find(x => x.id === it.sourceId);
      if (!s) throw new Error(`no source ${it.sourceId}`);
      if (s.path.startsWith(LIB_SCHEME)) throw new Error(`${s.name}: a library sample; install its kit instead`);
    }
    const out = items.map(it => ({ sourceId: it.sourceId, path: it.path, error: this.fileExists(it.path) ? null : `${it.path}: file not found`, lengthDiffers: false }));
    if (!out.some(r => r.error == null)) throw new Error(out.map(r => r.error).join('; '));
    const prev = structuredClone(this.p);
    for (const r of out) if (r.error == null) this.p.sources.find(x => x.id === r.sourceId)!.path = r.path;
    this.pushUndo(prev); this.changed();
    return out;
  }
  async relinkSearch(folder: string) {
    await delay(60);
    const under = (p: string) => p.toLowerCase().startsWith(folder.replace(/[\\/]+$/, '').toLowerCase());
    const candidates: Record<string, RelinkCandidate[]> = {}, notFound: Id[] = [], take: { sourceId: Id; path: string }[] = [];
    const missing = this.missing();
    let scanned = 0;
    for (const s of this.p.sources) {
      if (!(s.id in missing) || s.path.startsWith(LIB_SCHEME)) continue;
      const found = rank((this.moved.get(s.path) ?? []).filter(under).map(p => candidate(s.path, p)).filter((c): c is RelinkCandidate => !!c));
      scanned += found.length;
      if (!found.length) { notFound.push(s.id); continue; }
      const best = unambiguous(found);
      if (best) take.push({ sourceId: s.id, path: best.path }); else candidates[s.id] = found;
    }
    const relinked = take.length ? await this.relinkSources(take) : [];
    return { relinked, candidates, notFound, scanned, truncated: false };
  }
  /** Tests: what the next folder / file dialog returns (null = cancelled). */
  nextPick: string | null = 'C:\\Users\\michal\\Music';
  async pickFolder() { return this.dialog() ? this.nextPick : null; }
  async pickAudioFile() { return this.dialog() ? this.nextPick : null; }
  // ---------- MIDI files (app/src/midi_actions.rs) ----------
  /** The fake disk's MIDI file: 2 bars at 97 BPM, a fingered bass, a piano chord and GM drums
   *  (the same file app/src/midi_actions_tests.rs writes). Notes: [key, start beats, len beats, velocity]. */
  private midiFile(path: string) {
    const bass = [[36, 0, 1, 100], [36, 1.5, 0.5, 80], [43, 4, 3.75, 127]], piano = [[60, 0, 2, 70], [64, 0, 2, 70], [67, 0, 2, 70], [72, 6.25, 0.25, 1]];
    const drums = [[36, 0, 0.25, 110], [42, 0.5, 0.25, 60], [38, 1, 0.25, 100], [42, 1.5, 0.25, 60], [36, 4, 0.25, 110]];
    const part = (index: number, name: string, channel: number, program: number | null, gmName: string, raw: number[][], becomes: MidiBecomes) => ({
      index, track: index + 1, channel, name, program, gmName, drums: channel === 10, notes: raw.length,
      lowest: Math.min(...raw.map(n => n[0])), highest: Math.max(...raw.map(n => n[0])), end: Math.max(...raw.map(n => n[1] + n[2])), becomes, raw,
    });
    const parts = [
      part(0, 'Bass', 2, 33, 'Fingered Bass', bass, { kind: 'synth', preset: 'Bas Analog' }),
      part(1, 'Piano', 1, 0, 'Acoustic Grand Piano', piano, { kind: 'keys' }),
      part(2, 'Drums', 10, null, 'Drums', drums, { kind: 'drums', drums: [{ key: 36, name: 'Bass Drum', notes: 2 }, { key: 38, name: 'Snare', notes: 1 }, { key: 42, name: 'Closed Hi-hat', notes: 2 }] }),
    ];
    return { path, format: 1, ppq: 960, bpm: 97, tempoChanges: 0, end: 8, warnings: [] as string[], parts, projectBpm: this.p.bpm };
  }
  async midiInspect(path: string): Promise<MidiInfo> {
    await delay(20);
    if (!/\.midi?$/i.test(path)) throw new Error(`${path}: not a MIDI file`);
    const f = this.midiFile(path);
    return { ...f, parts: f.parts.map(({ raw: _r, ...rest }) => { void _r; return rest; }) };
  }
  async midiImport(a: MidiImportArgs): Promise<MidiImportResult> {
    await delay(30);
    const f = this.midiFile(a.path);
    const parts = a.parts.map(i => { const p = f.parts[i]; if (!p) throw new Error(`the file has no part ${i} (it has ${f.parts.length})`); return p; });
    if (!parts.length) throw new Error('no notes to import (choose at least one part with notes)');
    const prev = structuredClone(this.p);
    const note = (ch: Id, n: number[], key: boolean): Note => ({ ch, step: n[1] * 4, len: n[2] * 4, ...(key ? { p: n[0] } : {}), ...(n[3] !== 127 ? { v: n[3] / 127 } : {}), trig: false });
    if (a.into) {
      if (parts.length !== 1) throw new Error('import into a channel takes exactly one part');
      const into = a.into, pat = this.p.patterns.find(x => x.id === into.pattern);
      if (!pat) throw new Error(`no pattern ${into.pattern}`);
      const notes = [...(into.replace ? pat.notes.filter(n => n.ch !== into.ch) : pat.notes), ...parts[0].raw.map(n => note(into.ch, n, true))];
      this.p = applyAll(this.p, [{ op: 'putPattern', pattern: { ...structuredClone(pat), notes } }]);
      this.pushUndo(prev); this.changed();
      return { patternId: pat.id, channelIds: [into.ch], notes: parts[0].raw.length, revision: this.revision };
    }
    // engine.rs `apply_with_library`: the drum samples become sources, then all of it is one undo step
    const DRUM: Record<number, [string, string]> = { 36: ['kick-808.flac', 'Bass Drum'], 38: ['snare-studio.flac', 'Snare'], 42: ['hat-808-closed.flac', 'Closed Hi-hat'] };
    const keys = [...new Set(parts.filter(p => p.drums).flatMap(p => p.raw.map(n => n[0])))].sort((x, y) => x - y);
    const ops: Op[] = [];
    let next = this.p.nextId;
    if (a.setTempo && f.bpm) ops.push({ op: 'setBpm', bpm: f.bpm });
    const patternId = next++, notes: Note[] = [], channelIds: Id[] = [], drumCh = new Map<number, Id>();
    for (const k of keys) {
      const [file, name] = DRUM[k], path = libPath(file), fac = factoryFile(path)!;
      let src = this.p.sources.find(s => s.path === path);
      if (!src) {
        const sm: MockSource = { id: next++, name: fac.name, color: fac.color, path, dur: fac.dur, sr: 44100, ch: fac.ch, peakDb: -1, seed: hash(path) };
        this.srcMeta.set(sm.id, sm); this.p.sources.push(strip(sm)); src = this.p.sources[this.p.sources.length - 1];
      }
      const id = next++;
      ops.push({ op: 'putChannel', channel: { id, name, color: fac.color, kind: 'sample', source: src.id, sampler: samplerDefault(), vol: 0.7937, pan: 0, mute: false, solo: false, insert: null, root: k } });
      drumCh.set(k, id); channelIds.push(id);
    }
    for (const p of parts) {
      if (p.drums) { for (const n of p.raw) notes.push(note(drumCh.get(n[0])!, n, false)); continue; }
      const id = next++;
      const base = { id, name: p.name, vol: 0.7937, pan: 0, mute: false, solo: false, insert: null };
      const b = p.becomes;
      const ch: Channel = b.kind === 'synth'
        ? { ...newSynthChannel(this.cat.synth!, id, b.preset), ...base, root: /Bas|808/.test(b.preset) ? 36 : 60 }
        : { ...base, color: '#e0a44a', kind: 'keys', keys: { release: KEYS_RELEASE.def, dynamics: KEYS_DYNAMICS.def }, root: 60 };
      ops.push({ op: 'putChannel', channel: ch });
      channelIds.push(id);
      for (const n of p.raw) notes.push(note(id, n, true));
    }
    const end = Math.max(...parts.flatMap(p => p.raw.map(n => n[1] + n[2])));
    const len = Math.max(4, Math.ceil(end / 4) * 4), beat = Math.max(0, a.beat ?? 0);
    ops.push({ op: 'putPattern', pattern: { id: patternId, name: (a.path.split(/[\\/]/).pop() ?? 'MIDI').replace(/\.[^.]+$/, ''), color: '#7c8cf8', len, minLen: 4, notes } });
    const free = (t: number) => !this.p.clips.some(c => c.track === t && c.start < beat + len && c.start + c.len > beat);
    const track = a.track != null && a.track < this.p.tracks.length ? a.track : Math.max(0, this.p.tracks.findIndex((_, i) => free(i)));
    const clipId = next++;
    ops.push({ op: 'putClips', clips: [mkClip(clipId, track, beat, len, 'pat', patternId)] });
    try { this.p = applyAll(this.p, ops); } catch (e) { this.p = prev; throw e; }
    this.p.nextId = Math.max(this.p.nextId, next);
    this.pushUndo(prev); this.changed();
    return { patternId, channelIds, clipId, track, beat, len, revision: this.revision, warnings: [], tempoSet: !!(a.setTempo && f.bpm) };
  }
  async midiExport(a: { out: string; pattern?: Id }): Promise<MidiExportResult> {
    await delay(20);
    const pats = a.pattern != null
      ? this.p.patterns.filter(p => p.id === a.pattern)
      : this.p.clips.filter(c => c.kind === 'pat' && !c.muted).map(c => this.p.patterns.find(p => p.id === c.ref)).filter((p): p is Pattern => !!p);
    if (a.pattern != null && !pats.length) throw new Error(`no pattern ${a.pattern}`);
    const notes = pats.reduce((n, p) => n + p.notes.length, 0);
    const what = a.pattern != null ? pats[0].name : 'song';
    if (!notes) throw new Error(`the ${what} has no notes to export`);
    return { out: a.out, what, tracks: new Set(pats.flatMap(p => p.notes.map(n => n.ch))).size, notes, bpm: this.p.bpm };
  }
  // ---- FX chain presets (app/src/fx_chains.rs): kept for the session, across projects
  private chains = new Map<string, { name: string; fx: Fx[] }>();
  private chainName(name: string) {
    const n = name.trim();
    if (!n) throw new Error('give the preset a name');
    if ([...n].length > 80) throw new Error('a preset name can have at most 80 characters');
    if (/[<>:"/\\|?*\u0000-\u001f]/.test(n) || /[. ]$/.test(n) || n.startsWith('.')) throw new Error('a preset name cannot contain < > : " / \\ | ? * or end with a dot');
    return n;
  }
  private chainOf(name: string) {
    const c = this.chains.get(name.trim().toLowerCase());
    if (!c) throw new Error(`no FX chain preset called ${name.trim()}`);
    return c;
  }
  async fxChains() {
    await delay(10);
    const label = (f: Fx) => f.type === 'plugin' ? (f.plugin?.name ?? 'Plugin') : (mockCatalog().types[f.type]?.name ?? f.type);
    const presets = [...this.chains.values()].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()))
      .map(c => ({ name: c.name, effects: c.fx.map(f => ({ type: f.type, name: label(f) })), plugins: c.fx.filter(f => f.type === 'plugin').length }));
    return { dir: 'C:\\Users\\michal\\AppData\\Local\\WAVE\\FX Chains', presets };
  }
  async fxChainSave(insert: Id, name: string, overwrite = false) {
    await delay(10);
    const n = this.chainName(name), m = this.p.inserts.find(x => x.id === insert);
    if (!m) throw new Error(`no insert ${insert}`);
    if (!m.fx.length) throw new Error(`${m.name.trim() || 'the insert'} has no effects to save`);
    if (this.chains.has(n.toLowerCase()) && !overwrite) throw new Error(`${ERR_EXISTS} a preset called ${n} already exists`);
    this.chains.set(n.toLowerCase(), { name: n, fx: m.fx.map(f => ({ ...structuredClone(f), id: 0 })) });
    return { name: n, effects: m.fx.length };
  }
  async fxChainLoad(insert: Id, name: string, mode?: 'replace' | 'append') {
    await delay(10);
    const c = this.chainOf(name), m = this.p.inserts.find(x => x.id === insert);
    if (!m) throw new Error(`no insert ${insert}`);
    if (m.fx.length && !mode) throw new Error('the insert already has effects: say whether the preset replaces them or goes after them (mode replace or append)');
    const prev = structuredClone(this.p);
    const ids = c.fx.map((_, i) => this.p.nextId + i);
    const ops: Op[] = [
      ...(mode === 'replace' ? m.fx.map(f => ({ op: 'removeFx' as const, fxId: f.id })) : []),
      ...c.fx.map((f, i) => ({ op: 'putFx' as const, insertId: insert, fx: { ...structuredClone(f), id: ids[i] } })),
    ];
    this.p = applyAll(this.p, ops);
    this.p.nextId = Math.max(this.p.nextId, ids[ids.length - 1] + 1);
    this.pushUndo(prev); this.changed();
    return { revision: this.revision, fxIds: ids, replaced: mode === 'replace', failed: [] };
  }
  async fxChainRename(name: string, to: string) {
    await delay(10);
    const c = this.chainOf(name), n = this.chainName(to), other = this.chains.get(n.toLowerCase());
    if (other && other !== c) throw new Error(`${ERR_EXISTS} a preset called ${n} already exists`);
    this.chains.delete(c.name.toLowerCase());
    this.chains.set(n.toLowerCase(), { ...c, name: n });
    return { name: n };
  }
  async fxChainDelete(name: string) {
    await delay(10);
    const c = this.chainOf(name);
    this.chains.delete(c.name.toLowerCase());
    return { deleted: c.name };
  }
  async pickMidiFile() { return this.dialog() ? 'C:\\Users\\michal\\Music\\Groove.mid' : null; }
  async pickMidiSave(name: string) { return this.dialog() ? 'C:\\Users\\michal\\Music\\' + name : null; }

  async makeUnique(clipIds: Id[]) {
    const cs = clipIds.map(id => this.p.clips.find(x => x.id === id));
    const first = cs[0];
    if (!first || cs.some(c => !c)) throw new Error(first ? 'no such clip' : 'no clips given');
    if (cs.some(c => c!.kind !== 'audio')) throw new Error('only audio clips can be made unique this way');
    if (cs.some(c => c!.ref !== first.ref || c!.track !== first.track)) throw new Error('the clips must play the same sample on the same track');
    const old = this.p.sources.find(s => s.id === first.ref), meta = this.srcMeta.get(first.ref), tr = this.p.tracks[first.track];
    if (!old || !meta) throw new Error("the clip's source is missing");
    if (!tr) throw new Error("the clip's track is missing");
    // engine.rs `unique_base`: "Guitar #2" counts on from "Guitar"
    const base = old.name.replace(/(.) #\d+$/, '$1');
    let k = 2; while (this.p.sources.some(s => s.name === base + ' #' + k)) k++;
    const name = base + ' #' + k;
    const prev = structuredClone(this.p), id = this.p.nextId++;
    this.srcMeta.set(id, { ...meta, id, name }); this.p.sources.push({ ...old, id, name });
    const rp = this.recPeaks.get(first.ref); if (rp) this.recPeaks.set(id, rp);
    // the new track plays where the old one does (engine.rs `make_unique_audio`): no insert is claimed
    const at = first.track + 1, insertId = tr.insert;
    this.p.tracks.splice(at, 0, { id: this.p.nextId++, name, color: old.color, muted: false, solo: false, arm: false, vol: 0.7937, h: tr.h, insert: insertId, ...(tr.icon ? { icon: tr.icon } : {}) });
    for (const c of this.p.clips) {
      if (clipIds.includes(c.id)) { c.track = at; c.ref = id; } else if (c.track >= at) c.track++;
    }
    this.pushUndo(prev); this.changed();
    return { sourceId: id, track: at, insertId };
  }
  /** engine.rs `export_in_place`: one audio clip over the selection's span on a new track below it, routed to the master.
   *  The engine's clip also carries the effect tail past the span; the mock renders no effects, so it has none. */
  async exportInPlace(clipIds: Id[]) {
    await delay(30);
    const cs = this.p.clips.filter(c => clipIds.includes(c.id) && c.kind !== 'auto');
    if (!cs.length) throw new Error('select the clips to export in place (automation clips alone make no sound)');
    const from = Math.min(...cs.map(c => c.start)), to = Math.max(...cs.map(c => c.start + c.len));
    const first = [...cs].sort((a, b) => a.track - b.track || a.start - b.start)[0];
    const low = Math.max(...cs.map(c => c.track));
    const srcName = (c: Clip) => (c.kind === 'pat' ? this.p.patterns.find(x => x.id === c.ref)?.name : this.p.sources.find(x => x.id === c.ref)?.name) ?? 'Render';
    const color = (first.kind === 'pat' ? this.p.patterns.find(x => x.id === first.ref)?.color : this.p.sources.find(x => x.id === first.ref)?.color) ?? '#47b8a8';
    const name = cs.length === 1 ? srcName(first) + ' (in place)' : srcName(first) + ' + ' + (cs.length - 1) + ' (in place)';
    const prev = structuredClone(this.p), sourceId = this.p.nextId++;
    const src: MockSource = { id: sourceId, name, color, path: 'C:\\Users\\michal\\Music\\' + name + '.wav', dur: (to - from) * this.spb(), sr: 48000, ch: 2, peakDb: -3, seed: hash(name) };
    this.srcMeta.set(sourceId, src); this.p.sources.push(strip(src));
    const at = Math.min(low + 1, this.p.tracks.length);
    this.p.tracks.splice(at, 0, { id: this.p.nextId++, name, color, muted: false, solo: false, arm: false, vol: 0.7937, h: this.p.tracks[low]?.h ?? 60, insert: 0 });
    for (const c of this.p.clips) { if (c.track >= at) c.track++; if (clipIds.includes(c.id) && c.kind !== 'auto') c.muted = true; }
    const clipId = this.p.nextId++;
    this.p.clips.push(mkClip(clipId, at, from, to - from, 'audio', sourceId));
    this.pushUndo(prev); this.changed();
    return { sourceId, clipId, track: at };
  }
  // ---------- stem separation (app/src/stems_actions.rs) ----------
  private stemModel = false;
  private stemsCancelled = false;
  private stemsBusy = false;
  /** Dev/tests: the model download fails (also `?offline` in the URL, like the updater mock). */
  stemsOffline = (() => { try { return new URLSearchParams(location.search).has('offline'); } catch { return false; } })();
  async stemsModelStatus(): Promise<StemModelStatus> {
    return {
      id: 'htdemucs', name: 'HTDemucs stem separation model', bytes: MOCK_STEM_MODEL_BYTES, installed: this.stemModel,
      licenseNote: 'Demucs v4 (HTDemucs) by Meta AI Research: code and pretrained weights under the MIT licence.',
      licenseUrl: 'https://github.com/facebookresearch/demucs/blob/main/LICENSE',
      path: 'C:\\Users\\michal\\AppData\\Local\\WAVE\\Models\\htdemucs.onnx', stems: [...STEM_IDS],
    };
  }
  async stemsModelInstall(): Promise<StemModelStatus | { cancelled: true }> {
    this.stemsCancelled = false;
    const total = MOCK_STEM_MODEL_BYTES;
    for (let k = 0; k <= 20; k++) {
      if (this.stemsCancelled) return { cancelled: true };
      if (this.stemsOffline) { this.emit('model-progress', { id: 'htdemucs', done: 0, total: 0, error: 'download failed: offline' }); throw new Error('download failed: io: offline'); }
      this.emit('model-progress', { id: 'htdemucs', done: Math.round(total * k / 20), total });
      await delay(60);
    }
    this.stemModel = true;
    return this.stemsModelStatus();
  }
  async stemsCancel() { this.stemsCancelled = true; }
  /** stems_actions.rs `extract` + engine.rs `stem_region`/`place_stems`: the region under the clip plus 3 s either side is
   *  "separated" (fake stems: the source's waveform, scaled per stem), then one track per stem right below the clip, a clip at
   *  the same place playing the same part the same way, the clip muted — one undo step. */
  async extractStems(clipId: Id, stems: StemId[]): Promise<ExtractStemsResult | { cancelled: true }> {
    const picked = STEM_IDS.filter(s => stems.includes(s));
    const bad = stems.find(s => !STEM_IDS.includes(s));
    if (bad) throw new Error(`no stem called \`${bad}\` (drums, bass, other, vocals)`);
    if (!picked.length) throw new Error('pick at least one stem');
    if (!this.stemModel) throw new Error(`${ERR_NO_STEM_MODEL} the stem separation model is not installed (90 MB download)`);
    if (this.stemsBusy) throw new Error('stems are already being extracted from another clip; wait for it or cancel it');
    const c0 = this.p.clips.find(x => x.id === clipId);
    if (!c0) throw new Error('no such clip');
    if (c0.kind !== 'audio') throw new Error('stems can be extracted from audio clips only');
    if (this.missing()[c0.ref]) throw new Error(`the clip's audio file is missing (${this.missing()[c0.ref]}); relink it first`);
    const s = this.p.sources.find(x => x.id === c0.ref); if (!s) throw new Error('audio not loaded');
    const snap = { ...c0 }, spb0 = this.spb(), st0 = c0.stretch ?? 1;
    const sa0 = c0.offset * spb0 / st0, sb0 = Math.min(s.dur, (c0.offset + c0.len) * spb0 / st0);
    if (sb0 - sa0 < 0.05) throw new Error('the clip plays less than 50 ms of its audio: nothing to separate');
    if (sb0 - sa0 > 1800) throw new Error(`the clip plays ${Math.round((sb0 - sa0) / 60)} minutes of audio; stems can be extracted from at most 30 minutes at a time (split the clip first)`);
    const fromS = Math.max(0, sa0 - 3), toS = Math.min(s.dur, sb0 + 3);
    this.stemsBusy = true; this.stemsCancelled = false;
    try {
      const prog = (stage: StemsProgress['stage'], frac: number) => this.emit('stems-progress', { clipId, stage, frac });
      prog('prepare', 0); await delay(40);
      prog('load', 0); await delay(120);
      const steps = Math.max(4, Math.min(30, Math.ceil((toS - fromS) / 5.85)));
      for (let k = 0; k < steps; k++) {
        if (this.stemsCancelled) return { cancelled: true };
        prog('separate', k / steps); await delay(150);
      }
      prog('separate', 1);
      if (this.stemsCancelled) return { cancelled: true };
      prog('place', 1);
      // like place_stems: the clip as it is now must still play that region of that source
      const c = this.p.clips.find(x => x.id === clipId);
      if (!c) throw new Error('the clip was deleted while its stems were extracted; nothing was added');
      const spb = this.spb(), st = c.stretch ?? 1, sa = c.offset * spb / st, sb = (c.offset + c.len) * spb / st;
      if (c.kind !== 'audio' || c.ref !== snap.ref || !!c.norm !== !!snap.norm || !!c.rev !== !!snap.rev || sa < fromS - 1e-6 || Math.min(sb, s.dur) > toS + 1e-3)
        throw new Error('the clip changed while its stems were extracted (another sample or region); extract them again');
      const tr = this.p.tracks[c.track]; if (!tr) throw new Error("the clip's track is missing");
      const pk = await this.sourcePeaks(c.ref, c.norm, c.rev);
      const prev = structuredClone(this.p), at = c.track + 1, n = picked.length;
      for (const x of this.p.clips) { if (x.track >= at) x.track += n; if (x.id === c.id) x.muted = true; }
      const name = s.name, offset = (sa - fromS) * st / spb;
      const sourceIds: Id[] = [], clipIds: Id[] = [];
      picked.forEach((stem, k) => {
        const meta = STEM_META[stem], sname = `${name} – ${meta.label}`, id = this.p.nextId++;
        const src: MockSource = { id, name: sname, color: meta.color, path: `C:\\Users\\michal\\Music\\${name} - ${meta.label}.wav`, dur: toS - fromS, sr: 44100, ch: 2, peakDb: s.peakDb - 3, seed: hash(sname) };
        this.srcMeta.set(id, src); this.p.sources.push(strip(src));
        this.recPeaks.set(id, pk.slice(Math.floor(fromS * 100) * 2, Math.ceil(toS * 100) * 2).map((v, i) => v * meta.scale * (stem === 'drums' && (i >> 4) % 3 ? 0.35 : 1)));
        this.p.tracks.splice(at + k, 0, { id: this.p.nextId++, name: sname, color: meta.color, muted: false, solo: false, arm: false, vol: 0.7937, h: tr.h, insert: tr.insert });
        const cid = this.p.nextId++;
        const nc: Clip = { ...structuredClone(c), id: cid, track: at + k, ref: id, offset, muted: false, norm: false, rev: false };
        delete nc.tune;
        this.p.clips.push(nc);
        sourceIds.push(id); clipIds.push(cid);
      });
      this.pushUndo(prev); this.changed();
      return { sourceIds, clipIds, track: at, mutedClip: c.id, revision: this.revision };
    } finally {
      this.stemsBusy = false;
    }
  }
  async previewClip(clipId: Id, from?: number) {
    const c = this.p.clips.find(x => x.id === clipId); if (!c) return;
    // like the engine: positions in seconds of the source (a stretched clip plays `stretch` times longer)
    const st = c.stretch ?? 1, a = c.offset * this.spb() / st, b = (c.offset + c.len) * this.spb() / st;
    if (from == null) { this.preview = { t0: performance.now(), dur: c.len * this.spb(), off: a, rate: 1 / st }; return; }
    const f = clamp(from, a, b);
    if (f >= b) throw new Error('nothing to play after that point');
    this.preview = { t0: performance.now(), dur: b - f, off: f, rate: 1 };
  }
  async previewClipKey(a: { clipId: Id; key: number; vel?: number }) {
    const c = this.p.clips.find(x => x.id === a.clipId && x.kind === 'audio'); if (!c) throw new Error('no such clip');
    // like the engine: the region at the sample's own speed, read faster by the distance from the root
    const st = c.stretch ?? 1, a0 = c.offset * this.spb() / st, b0 = (c.offset + c.len) * this.spb() / st;
    const rate = 2 ** ((Math.min(127, a.key) - (c.root ?? SAMPLE_ROOT) + (c.pitch ?? 0)) / 12);
    this.preview = { t0: performance.now(), dur: (b0 - a0) / rate, off: a0, rate };
  }
  async previewFile(path: string) { this.preview = { t0: performance.now(), dur: 0.6 + (hash(path) % 400) / 100, off: 0 }; }
  async previewStop() { this.preview = null; }
  async previewSource(id: Id) {
    const src = this.p.sources.find(s => s.id === id);
    if (!src) throw new Error('audio not loaded');
    this.preview = { t0: performance.now(), dur: src.dur, off: 0 };
  }
  /** Insert of the track of the pattern's first clip (rt.rs `PatRt.home`). */
  private patHome(id: Id): Id | null {
    const c = this.p.clips.filter(x => x.kind === 'pat' && x.ref === id).sort((a, b) => a.start - b.start)[0];
    return c ? this.p.tracks[c.track]?.insert ?? null : null;
  }
  async noteAudition(a: { ch: Id; key?: number; vel?: number; pattern?: Id }) {
    const c = this.p.channels?.find(x => x.id === a.ch); if (!c) throw new Error('no channel ' + a.ch);
    const heard = this.p.patterns.find(pt => pt.notes.some(n => n.ch === c.id));
    // over a pattern loop it goes where the loop goes (rt.rs `Cmd::Audition`)
    const ins = this.patPlay ? c.insert ?? 0 : c.insert ?? (a.pattern != null ? this.patHome(a.pattern) : null) ?? (heard ? this.patHome(heard.id) : null) ?? 0;
    this.flash.set(ins, performance.now());
  }
  async typingNote(a: { ch: Id; key: number; vel?: number; on: boolean }) { if (a.on) await this.noteAudition({ ch: a.ch, key: a.key, vel: a.vel }); }
  async patternPlay(id: Id | null, o?: { from?: number; range?: BeatRange | null }) {
    const from = o?.from ?? 0, r = o?.range ?? null;
    if (!Number.isFinite(from) || (r && (!Number.isFinite(r.a) || !Number.isFinite(r.b)))) throw new Error('pattern position: not a number');
    if (id != null && !this.p.patterns.some(x => x.id === id)) throw new Error('no pattern ' + id);
    if (id != null && this.tr.playing) { this.finishRec(); this.tr = { ...this.tr, playing: false }; this.changedTransport(); }
    this.patPlay = id == null ? null : { id, t0: performance.now(), from: Math.max(0, from), range: r ? { a: r.a, b: r.b } : null };
  }
  async patternLoop(range: BeatRange | null) {
    if (range && (!Number.isFinite(range.a) || !Number.isFinite(range.b))) throw new Error('pattern loop: not a number');
    const pp = this.patPlay; if (!pp) return;
    const pt = this.p.patterns.find(x => x.id === pp.id); if (!pt) return;
    // like the engine: the position stays (outside the new range it goes to its start)
    this.patPlay = { ...pp, t0: performance.now(), from: this.patPos(pt.len, this.spb()), range: range ? { a: range.a, b: range.b } : null };
  }
  /** The pattern loop's position now (beats into the pattern). */
  private patPos(len: number, spb: number) {
    const pp = this.patPlay!;
    return loopPos(pp.from, (performance.now() - pp.t0) / 1000 / spb, pp.range, len);
  }

  // ---------- audio device ----------
  async audioHosts() { return [{ id: 'wasapi', name: 'WASAPI' }, { id: 'asio', name: 'ASIO' }]; }
  async audioDevices(host: string) {
    return host === 'asio'
      ? { outputs: ['Focusrite USB ASIO'], inputs: [] }
      : { outputs: this.wasapiOutputPresent ? ['Głośniki (Realtek High Definition Audio)', 'Słuchawki (USB Audio)'] : [], inputs: ['Mikrofon (USB Audio)', 'Line In (Realtek)'] };
  }
  async audioGetConfig() { return { ...this.audio }; }
  async audioSetConfig(cfg: AudioConfig) {
    await delay(80);
    // like the app: a config without recOffsetMs keeps the saved calibration
    cfg = { ...cfg, recOffsetMs: cfg.recOffsetMs ?? this.audio.recOffsetMs ?? 0 };
    const unavailableOutput = cfg.host === 'asio' && (!this.focusritePresent || cfg.output && cfg.output !== 'Focusrite USB ASIO')
      ? cfg.output || 'Focusrite USB ASIO' : null;
    const error = cfg.host === 'wasapi' && !this.wasapiOutputPresent ? 'no output device' : null;
    const running = !unavailableOutput && !error;
    this.audio = { ...this.audio, ...cfg, running, error, unavailableOutput,
      actualRate: running ? cfg.sampleRate ?? 48000 : 0, actualBuffer: running ? cfg.bufferSize ?? 480 : 0,
      latencyMs: running ? Math.round(((cfg.bufferSize ?? 480) * 2 / (cfg.sampleRate ?? 48000)) * 10000) / 10 : 0,
      inputChannels: running ? 2 : 0 };
    this.emit('audio-status', { ...this.audio });
    return { ...this.audio };
  }

  /** Simulates the attached hardware independently of the registered ASIO driver. */
  async setFocusritePresent(present: boolean) {
    this.focusritePresent = present;
    if (!present && this.audio.host === 'asio' && this.audio.running) {
      await this.transportStop();
      await this.transportSet({ rec: false, monitor: false });
      await this.audioSetConfig(this.audio);
    }
  }

  setWasapiOutputPresent(present: boolean) { this.wasapiOutputPresent = present; }

  /** Test hook: the output could not be opened (the engine's status after a WASAPI/ASIO open error); the next `audioSetConfig` opens it again. */
  audioOpenFailed(err: string) {
    this.audio = { ...this.audio, running: false, error: err };
    this.emit('audio-status', { ...this.audio });
  }
  /** Dev tools / tests: the device stops by itself and the audio thread reopens it, as in the app. */
  audioDeviceLost(why = 'The requested device is no longer available') {
    this.audio = { ...this.audio, running: true, error: null, restarts: this.audio.restarts + 1, note: `Audio restarted by itself: ${why}.` };
    this.emit('audio-status', { ...this.audio });
  }

  async audioMeasureLatency(): Promise<LatencyReport> {
    if (this.tr.playing) throw new Error('Stop playback before measuring the latency.');
    await delay(5200);
    const reportedMs = 21.3, roundTripMs = 25.8;
    return { roundTripMs, reportedMs, suggestedOffsetMs: Math.round((roundTripMs - reportedMs) * 100) / 100, confidence: 0.93 };
  }

  // ---------- MIDI keyboard ----------
  async midiListDevices() { return this.midiPorts.map(d => ({ ...d })); }
  async midiSelectDevice(id: string | null) {
    const d = id == null ? null : this.midiPorts.find(x => x.id === id) ?? null;
    if (id != null && !d) this.midi = { ...this.midi, connected: null, selected: { id, name: id }, error: `${id} is not connected` };
    else this.midi = { ...this.midi, connected: d && { ...d }, selected: d && { ...d }, error: null };
    this.emit('midi-status', { ...this.midi });
    return { ...this.midi };
  }
  async midiStatus() { return { ...this.midi }; }
  async midiSetVelocity(fixed: number | null) {
    this.midi = { ...this.midi, fixedVelocity: fixed == null ? null : Math.min(127, Math.max(1, Math.round(fixed))) };
    return { ...this.midi };
  }
  async midiSetTarget(channel: Id | null) {
    this.midi = { ...this.midi, target: channel };
    this.emit('midi-status', { ...this.midi });
    return { ...this.midi };
  }

  // ---------- library ----------
  async libraryList(): Promise<LibraryList> { await delay(); return { kits: this.kits.map(k => ({ ...k })), folders: [...this.folders] }; }
  async libraryInstall(id: string) {
    const kit = this.kits.find(k => k.id === id); if (!kit) throw new Error('no kit');
    let done = 0; const total = kit.bytes;
    const iv = setInterval(() => {
      done = Math.min(total, done + total / 25);
      this.emit('library-progress', { id, done, total });
      if (done >= total) { clearInterval(iv); kit.installed = true; }
    }, 60);
  }
  /** The browser preview has no recordings to load: the piano is always "ready" (and silent, like every mock instrument). */
  async keysStatus(): Promise<KeysStatus> { await delay(); return { state: 'ready', error: null, kit: 'salamander-piano', installed: true }; }
  async libraryFiles(q: { kit?: string; folder?: string }): Promise<LibraryFiles> {
    await delay(10);
    // the standard library is listed by its portable paths (the app lists where it is installed; importing either gives `lib:` sources)
    if (q.kit === FACTORY_KIT) return { truncated: false, files: FACTORY_LISTED_FILES.map(f => ({ name: f.name, path: libPath(f.file), group: f.group, root: f.root, ...(f.pitched ? { pitched: true } : {}) })) };
    const root = q.kit ? 'C:\\WAVE\\Library\\' + q.kit : q.folder ?? '';
    const groups: Record<string, string[]> = q.kit === 'wave-keys'
      ? { Pady: ['Warm Pad C', 'Glass Pad Am', 'Choir Pad F'], Akordy: ['Rhodes Chord 1', 'Rhodes Chord 2', 'Piano Stab'], Pętle: ['Keys Loop 120', 'Pad Loop Am-F'] }
      : q.kit === 'wave-fx' ? { Uplightery: ['Riser 8 bar', 'Riser 4 bar'], Impakty: ['Impact Deep', 'Impact Metal'], Szumy: ['White Sweep', 'Vinyl Crackle'] }
      : q.kit ? { Kick: ['Kick 808', 'Kick Punch', 'Kick Deep'], Snare: ['Snare Tight', 'Snare Room', 'Clap 909'], Hats: ['Hat Closed', 'Hat Open', 'Ride'], Loops: ['Drum Loop 128', 'Perc Loop 128', 'Top Loop'] }
      : { '': ['vox_take1', 'vox_take2', 'guitar_di', 'bass_di', 'fx_loop'] };
    return { truncated: false, files: Object.entries(groups).flatMap(([group, names]) => names.map(n => ({ name: n + '.wav', path: root + '\\' + (group ? group + '\\' : '') + n + '.wav', group }))) };
  }
  async libraryAddFolder() { this.folders.push('D:\\Sample packs\\Pack ' + (this.folders.length + 1)); return [...this.folders]; }
  async libraryRemoveFolder(path: string) { this.folders = this.folders.filter(f => f !== path); return [...this.folders]; }

  // ---------- project files ----------
  private resetTo(p: Project, path: string | null) {
    this.patPlay = null;
    // ids start over in a new project: the audio of the previous one must not answer to them
    this.srcMeta.clear(); this.recPeaks.clear();
    this.p = p; this.undoStack = []; this.redoStack = []; this.path = path;
    this.tr = { ...this.tr, playing: false }; this.revision++; this.savedRev = this.revision;
    this.emit('state', this.dto());
  }
  async projectNew(demo?: boolean) {
    // like the engine: a refused reset (recording) keeps the project and its recovery copy
    if (this.rec) throw new Error('recording: stop it first');
    this.resetTo(demo ? demoProject() : emptyProject(), null); this.recovery = null; return this.dto();
  }
  /** Tests: the next native file dialog is dismissed, as when the user presses Cancel. */
  dismissNextDialog = false;
  private dialog(): boolean { const ok = !this.dismissNextDialog; this.dismissNextDialog = false; return ok; }
  async projectOpen(path?: string, ifRevision?: number): Promise<OpenResult> {
    if (path == null && !this.dialog()) return { cancelled: true };
    // the engine's `open_at_revision`: a take in progress first, then the approved revision
    if (this.rec) throw new Error('recording: stop it first');
    if (ifRevision != null && ifRevision !== this.revision) throw new Error(`${ERR_REVISION_CONFLICT} project changed`);
    this.resetTo(sketchProject(this.cat), path ?? 'C:\\Users\\michal\\Music\\Demo.synkopa');
    this.recovery = null;
    return { state: this.dto(), warnings: [] };
  }
  async takeProjectOpenRequests(): Promise<string[]> { return []; }
  async projectSave(saveAs?: boolean, version?: boolean): Promise<SaveResult> {
    // save new version: Name → Name_2 → Name_3 beside the current file (actions.rs next_version_path)
    if (this.path && !saveAs && version) {
      const m = /^(.*[\\/])?(.+?)(\.[^.\\/]+)?$/.exec(this.path)!, v = /^(.+)_(\d{1,6})$/.exec(m[2]);
      this.path = (m[1] ?? '') + (v ? v[1] + '_' + Math.max(2, +v[2] + 1) : m[2] + '_2') + (m[3] ?? '.synkopa');
    }
    if (!this.path || saveAs) {
      if (!this.dialog()) return { cancelled: true };
      this.path = 'C:\\Users\\michal\\Music\\Projekt ' + (1 + (this.revision % 9)) + '.synkopa';
    }
    this.savedRev = this.revision; this.recovery = null; this.emit('state', this.dto());
    return { path: this.path };
  }

  async projectExportZip(ifRevision: number): Promise<SaveResult> {
    if (!this.dialog()) return { cancelled: true };
    if (this.rec) throw new Error('recording: stop it first');
    if (ifRevision !== this.revision) throw new Error(`${ERR_REVISION_CONFLICT} project changed`);
    return { path: this.path?.replace(/\.(synkopa|wave)$/i, '.zip') ?? 'Song.zip' };
  }

  // ---------- crash recovery (app/src/recovery.rs) ----------
  /** This session's snapshot of the unsaved changes; `path`: the project file they belong to. */
  private recovery: { project: Project; savedMs: number; path: string | null } | null = null;
  /** Snapshots left by sessions that "crashed" (see `simulateCrash`). */
  private orphans = new Map<string, { project: Project; savedMs: number; path: string | null }>();
  /** Like recovery.rs: a snapshot whenever something is unsaved, saved project or not; the project's own file is never written. */
  async recoveryWrite() {
    if (this.revision === this.savedRev) { this.recovery = null; return false; }
    this.recovery = { project: JSON.parse(JSON.stringify(this.p)), savedMs: Date.now(), path: this.path };
    return true;
  }
  async recoveryList() {
    return [...this.orphans].map(([id, o]) => ({ id, savedMs: o.savedMs, ...(o.path ? { project: o.path } : {}) })).sort((a, b) => b.savedMs - a.savedMs);
  }
  async recoveryRestore(id: string) {
    const o = this.orphans.get(id);
    if (!o) throw new Error(`no recovered project \`${id}\``);
    this.orphans.delete(id);
    this.resetTo(o.project, o.path);
    this.revision++; // unsaved changes: Save writes them to the project (or asks where, for one never saved)
    this.recovery = o;
    this.emit('state', this.dto());
    return { state: this.dto(), warnings: [] };
  }
  async recoveryDiscard(id: string) {
    if (!this.orphans.delete(id)) throw new Error(`no recovered project \`${id}\``);
  }
  /** Tests: the app dies without a normal exit and starts again with an empty project. */
  simulateCrash() {
    if (this.recovery) this.orphans.set(`${1000 + this.orphans.size}-${this.recovery.savedMs}`, this.recovery);
    this.recovery = null;
    this.resetTo(emptyProject(), null);
  }

  private exportAbort = false;
  async exportAudio(opts: ExportAudioOptions): Promise<ExportResult> {
    this.exportAbort = false;
    this.checkExportRange(opts);
    const { from, to } = exportRange(this.p, opts.useLoop, opts.pattern);
    const tail = opts.format !== 'mp3' && opts.wrapTail ? 0 : opts.tail;
    const secs = exportSeconds(from, to, this.p.bpm, tail);
    const name = (this.path ? this.path.replace(/\.(synkopa|wave)$/i, '') : 'C:\\Users\\michal\\Music\\Song') + (opts.format === 'mp3' ? '.mp3' : '.wav');
    // a short offline bounce with the engine's progress reports, so the dialog's readout and cancel work
    if (!(await this.mockBounce(0, 1, name.split(/[\\/]/).pop()!, from, to, 12))) return { cancelled: true };
    this.emit('export-progress', { done: 1, total: 1, name: '', stage: 'done', frac: 1, beat: to, peak: 0, wave: [] });
    const silent = opts.pattern != null ? patternSilent(this.p, opts.pattern) : this.p.clips.length === 0 || this.p.tracks.every(t => t.muted);
    return {
      out: name,
      fromBeat: from,
      toBeat: to,
      tail,
      stats: {
        seconds: secs,
        sampleRate: opts.sampleRate,
        peakDbfs: silent ? -180 : -3.2,
        rmsDbfs: silent ? -180 : -14.5,
        crestDb: silent ? 0 : 11.3,
        clippedSamples: 0,
        silent,
      },
    };
  }
  /** Like the engine: one file per insert with sound (+ Master), the same names, nothing written when cancelled. */
  async exportStems(opts: ExportStemsOptions): Promise<ExportStemsResult> {
    this.exportAbort = false;
    this.checkExportRange(opts);
    const { from, to } = exportRange(this.p, opts.useLoop, opts.pattern);
    const format = opts.format ?? 'wav';
    const tail = opts.format !== 'mp3' && opts.wrapTail ? 0 : opts.tail;
    const secs = exportSeconds(from, to, this.p.bpm, tail);
    const jobs = [
      ...(opts.includeMaster ? [{ name: 'Master', master: true }] : []),
      ...stemInserts(opts.pattern != null ? patternProject(this.p, opts.pattern) : this.p).map(m => ({ name: m.name.trim() || `Insert ${m.id}`, master: false })),
    ];
    if (!jobs.length) throw new Error('nothing to export: every mixer track is muted or has no sound routed to it');
    const stems = uniqueFileStems(jobs.map(j => j.name));
    const files: Extract<ExportStemsResult, { files: unknown }>['files'] = [];
    for (let i = 0; i < jobs.length; i++) {
      if (!(await this.mockBounce(i, jobs.length, jobs[i].name, from, to, 5))) return { cancelled: true };
      files.push({
        name: jobs[i].name, file: `${stems[i]}.${format}`, master: jobs[i].master,
        stats: { seconds: secs, sampleRate: opts.sampleRate, peakDbfs: -6.1, rmsDbfs: -18.2, crestDb: 12.1, clippedSamples: 0, silent: false },
      });
    }
    this.emit('export-progress', { done: jobs.length, total: jobs.length, name: '', stage: 'done', frac: 1, beat: to, peak: 0, wave: [] });
    const dir = this.path ? this.path.replace(/[\\/][^\\/]*$/, '') : 'C:\\Users\\michal\\Music';
    return { dir, fromBeat: from, toBeat: to, tail, format, files, skipped: [] };
  }
  async exportCancel() { this.exportAbort = true; }
  /** The checks of lib.rs `export_span`: a pattern export names a pattern that exists and has no wrapped tail. */
  private checkExportRange(opts: ExportAudioOptions) {
    if (opts.pattern == null) return;
    if (opts.format !== 'mp3' && opts.wrapTail) throw new Error('wrap tail is for a loop of the song, not a pattern export');
    if (!this.p.patterns.some(x => x.id === opts.pattern)) throw new Error('that pattern no longer exists');
  }
  /** One file of a mock export: prepare, `steps` render reports with a growing waveform, write. False when cancelled. */
  private async mockBounce(done: number, total: number, name: string, from: number, to: number, steps: number): Promise<boolean> {
    const SLICES = 240, wave: number[] = new Array(SLICES).fill(0);
    let peak = 0;
    this.emit('export-progress', { done, total, name, stage: 'prepare', frac: 0, beat: from, peak: 0, wave: [...wave] });
    for (let k = 1; k <= steps; k++) {
      await delay(40);
      if (this.exportAbort) return false;
      for (let i = Math.floor((k - 1) * SLICES / steps); i < Math.floor(k * SLICES / steps); i++) {
        // a beat-shaped envelope, quieter for later stems
        const v = (0.35 + 0.5 * Math.exp(-((i * 16 / SLICES) % 1) * 5)) / (1 + done * 0.4);
        wave[i] = Math.round(v * 255);
        peak = Math.max(peak, v);
      }
      this.emit('export-progress', { done, total, name, stage: 'render', frac: k / steps, beat: from + (to - from) * k / steps, peak, wave: [...wave] });
    }
    this.emit('export-progress', { done, total, name, stage: 'write', frac: 1, beat: to, peak, wave: [...wave] });
    await delay(20);
    return !this.exportAbort;
  }

  // ---------- hosted plugins ----------
  async pluginScan(rescan: boolean): Promise<PluginScan> {
    await delay(rescan ? 600 : 20);
    const mk = mockVst3;
    const lv2 = Object.entries(MOCK_LV2).map(([cid, p]): PluginInfo => ({ format: 'lv2', cid, name: p.name, vendor: p.vendor, category: p.category, path: '/usr/lib/lv2/' + p.bundle, ...(p.unsupported ? { unsupported: p.unsupported } : {}) }));
    return {
      plugins: [mk('Pro-Q 3', 'FabFilter', 'Fx|EQ'), mk('Pro-C 2', 'FabFilter', 'Fx|Dynamics'), mk('Valhalla VintageVerb', 'Valhalla DSP', 'Fx|Reverb'),
        mk('Decapitator', 'Soundtoys', 'Fx|Distortion'), mk('EchoBoy', 'Soundtoys', 'Fx|Delay'), mk('OTT', 'Xfer Records', 'Fx|Dynamics'), mk('Serum FX', 'Xfer Records', 'Fx'), ...lv2],
      instruments: MOCK_INSTRUMENTS.map(([name, vendor, category]) => mk(name, vendor, category)),
      unavailable: {},
    };
  }
  /** A loaded hosted plugin by id, like the engine's handles: an effect slot, or a plugin channel that loaded. */
  private hosted(id: Id): PluginRef | undefined {
    const fx = this.p.inserts.flatMap(m => m.fx).find(f => f.id === id);
    if (fx?.type === 'plugin') return fx.plugin;
    const ch = (this.p.channels ?? []).find(c => c.id === id);
    return ch?.kind === 'plugin' && !this.fxErrors()[id] ? ch.plugin : undefined;
  }
  /** Like the engine: a plugin's own window under Synkopa's header (`label`), pinned on top when `pinned`; WAVE Demo Synth has none (plugins.rs `NO_EDITOR`). */
  async pluginEditor(fxId: Id, open: boolean, label?: { title: string; subtitle: string }, pinned?: boolean) {
    const pl = this.hosted(fxId);
    if (!pl) throw new Error(`${fxId} is not a loaded plugin effect or instrument`);
    if (open && /demo synth/i.test(pl.name)) throw new Error(ERR_NO_EDITOR + ' this plugin has no window of its own; its parameters are listed below');
    if (open) { this.editors.set(fxId, label ?? { title: '', subtitle: '' }); this.editorPins.set(fxId, !!pinned); } else { this.editors.delete(fxId); this.editorPins.delete(fxId); }
  }
  private editors = new Map<Id, { title: string; subtitle: string }>();
  private editorPins = new Map<Id, boolean>();
  async pluginEditorOpen(fxId: Id) { return this.editors.has(fxId); }
  /** Like the engine: the mock's sessions always exit cleanly, so nobody is ever a suspect. */
  async pluginCrashSuspects(): Promise<PluginCrashSuspect[]> { return []; }
  async pluginBlock(_format: string, _path: string) { /* nothing to block in the mock */ }
  /** Plugins loaded although the safety check refused them (the engine keeps this per user, not per project). */
  private trusted = new Set<string>();
  async pluginLoadAnyway(fxId: Id) {
    await delay();
    const f = this.p.inserts.flatMap(m => m.fx).find(x => x.id === fxId);
    const c = (this.p.channels ?? []).find(x => x.id === fxId);
    const plugin = f?.plugin ?? (c?.kind === 'plugin' ? c.plugin : undefined);
    if (!plugin) throw new Error(`no plugin effect or plugin channel has id ${fxId}`);
    this.trusted.add(plugin.cid);
    // not an edit: the state is sent again without a new revision
    this.emit('state', this.dto());
  }
  /** Test hook: the output ran dry `n` times (a late callback or a driver xrun in the engine). */
  mockUnderrun(n = 1) { this.underrunTotal += n; }
  /** Test hook: the header a plugin window shows (undefined when closed). */
  editorLabel(fxId: Id) { return this.editors.get(fxId); }
  /** Test hook: whether the plugin's window is pinned on top (undefined when closed). */
  editorPinned(fxId: Id) { return this.editorPins.get(fxId); }
  /** Test hook: the user closed the plugin's window with its X. */
  closeEditorWindow(fxId: Id) { this.editors.delete(fxId); this.editorPins.delete(fxId); }
  /** Test hook: the gear in a plugin window's header. */
  pressEditorSettings(fxId: Id) { this.emit('plugin-settings', { id: fxId }); }
  /** Test hook: the pin in a plugin window's header (toggles, like the real header). */
  pressEditorPin(fxId: Id) { const pinned = !this.editorPins.get(fxId); this.editorPins.set(fxId, pinned); this.emit('plugin-pin', { id: fxId, pinned }); }
  async pluginState(fxId: Id): Promise<string> {
    if (!this.hosted(fxId)) throw new Error(`${fxId} is not a loaded plugin effect or instrument`);
    // the mock's "state" is its parameter values
    return btoa(JSON.stringify((this.plugParams.get(fxId) ?? []).map(p => [p.id, p.value])));
  }
  async pluginParams(fxId: Id): Promise<PluginParam[]> {
    const fx = { plugin: this.hosted(fxId) };
    if (!fx.plugin) throw new Error(`${fxId} is not a loaded plugin effect or instrument`);
    const lv2 = fx.plugin.format === 'lv2' ? MOCK_LV2[fx.plugin.cid] : undefined;
    if (fx.plugin.format === 'lv2') {
      if (!lv2 || lv2.unsupported) throw new Error(`effect ${fxId} is not a loaded plugin`);
      const real = this.lv2Real.get(fxId) ?? new Map(lv2.ports.map(s => [s.id, s.def]));
      this.lv2Real.set(fxId, real);
      return lv2.ports.map(s => lv2Param(s, real.get(s.id) ?? s.def));
    }
    if (!this.plugParams.has(fxId)) {
      const names = ['Input', 'Output', 'Mix', 'Drive', 'Tone', 'Attack', 'Release', 'Threshold', 'Ratio', 'Width', 'Low', 'High'];
      this.plugParams.set(fxId, names.map((name, i) => { const v = ((hash(name + fxId) % 100) / 100); return { id: i, name, value: v, def: 0.5, display: (v * 100).toFixed(0) + ' %' }; }));
    }
    return structuredClone(this.plugParams.get(fxId)!);
  }
  async pluginSetParam(fxId: Id, id: number, value: number) {
    if (!(value >= 0 && value <= 1)) throw new Error('plugin parameter values are normalised 0..1');
    const fx = { plugin: this.hosted(fxId) };
    if (!fx.plugin) throw new Error(`${fxId} is not a loaded plugin effect or instrument`);
    if (fx.plugin.format === 'lv2') {
      await this.pluginParams(fxId);
      const spec = MOCK_LV2[fx.plugin.cid]?.ports.find(s => s.id === id);
      if (!spec) throw new Error(`${fx.plugin.name} has no parameter ${id}`);
      this.lv2Real.get(fxId)!.set(id, fromNormalized(spec, value));
      return;
    }
    const p = this.plugParams.get(fxId)?.find(x => x.id === id);
    if (p) { p.value = value; p.display = (value * 100).toFixed(0) + ' %'; }
  }

  private readonly upd = new MockUpdates((done, total) => this.emit('update-progress', { done, total }));
  appInfo() { return this.upd.appInfo(); }
  updateCheck() { return this.upd.updateCheck(); }
  updateDownload() { return this.upd.updateDownload(); }
  updateInstall() { return this.upd.updateInstall(); }
  updateSetAuto(on: boolean) { return this.upd.updateSetAuto(on); }
  updateMarkSeen(version: string) { return this.upd.updateMarkSeen(version); }
  openReleasePage(version?: string | null) { return this.upd.openReleasePage(version); }
  async openAsk(service: AskService, prompt: string | null) {
    // like links::ask_url: a ChatGPT question must fit in the link
    const url = askUrl(service, prompt);
    if (!url) throw new Error(`the question is too long for a link (> ${MAX_ASK_URL} characters)`);
    try { window.open(url, '_blank', 'noopener'); } catch { /* no window */ }
  }
  async cliStatus() { return { installed: false, command: 'synkopa', path: null, note: t('d.mock.nocli') }; }

  async on<K extends keyof EventMap>(ev: K, cb: (p: EventMap[K]) => void) {
    let s = this.listeners.get(ev); if (!s) this.listeners.set(ev, s = new Set());
    const f = cb as (p: unknown) => void; s.add(f);
    return () => { s!.delete(f); };
  }
  async onOsDrop(cb: (e: OsDropEvent) => void) {
    const over = (e: DragEvent) => { if (e.dataTransfer && [...e.dataTransfer.types].includes('Files')) { e.preventDefault(); cb({ type: 'over', paths: [], x: e.clientX, y: e.clientY }); } };
    const leave = (e: DragEvent) => { if (!e.relatedTarget) cb({ type: 'leave', paths: [], x: 0, y: 0 }); };
    const drop = (e: DragEvent) => {
      e.preventDefault();
      const files = [...(e.dataTransfer?.files ?? [])].filter(f => f.type.startsWith('audio') || /\.(wav|mp3|ogg|flac|m4a|aif+)$/i.test(f.name));
      cb({ type: 'drop', paths: files.map(f => f.name), x: e.clientX, y: e.clientY });
    };
    window.addEventListener('dragover', over); window.addEventListener('dragleave', leave); window.addEventListener('drop', drop);
    return () => { window.removeEventListener('dragover', over); window.removeEventListener('dragleave', leave); window.removeEventListener('drop', drop); };
  }
  setTitle(title: string) { document.title = title; }

  /** Dev helper: a sung vocal take on the Vocal track and a vocal chain on its insert. */
  async addDemoVocal() {
    const r = await this.importAudio({ paths: ['Vocal take 1.wav'], track: this.vocalTrack(), beat: 0 });
    const ins = this.p.inserts.find(m => m.name === 'Vocal') ?? this.p.inserts[6];
    const mk = (type: Fx['type']) => newFx(this.cat, this.p.nextId++, type as never, this.p.bpm);
    const eq = ins.fx.find(f => f.type === 'eq');
    if (eq?.bands) {
      eq.bands.push({ on: true, type: 'peak', f: 5200, g: 0, q: 2.2, slope: 12, place: 'stereo', dynamic: { range: -6, thr: -40 } });
      eq.bands.push({ on: true, type: 'highshelf', f: 9000, g: 2.5, q: 0.71, slope: 12, place: 'side' });
    }
    const fx = ins.fx.filter(f => f.type !== 'comp');
    ins.fx = [mk('tune'), ...fx.filter(f => f.type === 'eq'), mk('deess'), mk('fet'), ...fx.filter(f => f.type !== 'eq')];
    this.changed();
    return r;
  }

  /**
   * Dev helper: five sung takes of a 4-bar phrase on the Vocal track, as if loop-recorded
   * over bars 9–12 (what `finishRec` produces), so take lanes can be tried without a mic.
   */
  async addDemoTakes(track = this.vocalTrack(), from = 32, bars = 4) {
    const prev = structuredClone(this.p), spb = this.spb(), len = bars * 4, dur = len * spb, landed: [Id, number, number][] = [];
    for (let k = 0; k < 5; k++) {
      const id = this.p.nextId++;
      const src: MockSource = { id, name: 'Take ' + (++this.takeN), color: '#d9678f', path: 'recordings/take' + this.takeN + '.wav', dur, sr: 48000, ch: 1, peakDb: -6, seed: 7100 + k * 13, vocal: true };
      this.srcMeta.set(id, src); this.p.sources.push(strip(src));
      landed.push([id, from, len]);
    }
    landTakes(this.p, Math.min(track, this.p.tracks.length - 1), landed, () => this.p.nextId++);
    this.pushUndo(prev); this.changed();
  }

  /** Dev helper: two audio loops on the vocal track (free of pattern clips in every starting project). */
  async addDemoAudio() {
    await this.importAudio({ paths: ['Pad Loop Am-F.wav'], track: this.vocalTrack(), beat: 32 });
    await this.importAudio({ paths: ['Perc Loop.wav'], track: this.vocalTrack(), beat: 64 });
  }
  /** Index of the track named "Vocal" (the armed one in the demo), else the sixth. */
  private vocalTrack() { const i = this.p.tracks.findIndex(t => t.name === 'Vocal'); return i >= 0 ? i : Math.min(5, this.p.tracks.length - 1); }
}

function strip(s: MockSource): Source {
  const { seed: _s, trimFrom: _t, ...rest } = s; void _s; void _t;
  return rest;
}
function mkClip(id: Id, track: number, start: number, len: number, kind: Clip['kind'], ref: Id): Clip {
  return { id, track, start, len, offset: 0, kind, ref, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false };
}
void BUILTIN;
