// Commands shared by the toolbar, keyboard, menus and views.
import { batch } from 'solid-js';
import { shownFloating } from './winOrder';
import { be } from '../api/backend';
import { t, tName } from '../lib/i18n';
import { PLUGIN_FORMAT_LABEL, VOL_UNITY, type AnyBuiltinKind, type PluginInfo, type Clip, type Fx, type Id, type Insert, type Op, type RecoveredProject, type Track } from '../api/types';
import { clampMove, duplicateSpan, insertTrackOps, moveClips, muteOps, pasteClips, sliceClip, swapTrackOps, TOTAL } from '../lib/clipOps';
import { parseViews, rememberView, viewFor } from '../lib/projView';
import { clamp, fmtPos } from '../lib/format';
import { loadPreset as loadPresetFx, newFx, presetInfo, presetList } from '../lib/presets';
import { snapName, snapRes } from '../lib/snap';
import { samePath } from '../lib/paths';
import { deletePoints } from '../lib/envEdit';
import { acceptReply, commit, endWheelGestures, enqueue, inGesture, projectRevision, resync } from './edit';
import { flushNotebook } from './notebook';
import { clearPeaks } from './peaks';
import { modePattern, openRack, patMode, setPatMode, stopPreview, togglePreview } from './pattern';
import { allocId, findFx, insName, plain, S, setTransport } from './store';
import { say, setU, tucked, U, type SampleTarget } from './ui';
import { notify } from './notice';
import { midiStatus } from './midi';
import { typingOn } from './typingKeys';
import { recTarget, recUntil } from '../lib/recTarget';
import { curBeat, markPlayCmd } from './telemetry';
import { confirmRecover, confirmUnsaved } from '../ui/Confirm';
import { showPlugin } from './pluginWindows';

// ---------- transport ----------
/** Recording is on but no track is armed: the engine plays and records nothing. */
/** The name of the track a recording from `from` goes to (see `recTarget`), or "a new track". */
export function recTargetName(from: number): string {
  const i = recTarget(S.project, from, recUntil(S.project));
  return i === 'new' ? t('d.act.newTrack') : S.project.tracks[i].name;
}
export async function play(from = U.start) {
  if (U.patPlaying) await stopPreview();
  markPlayCmd();
  setU('playing', true);
  try {
    const dest = S.transport.rec ? recTargetName(from) : '';
    await be().transportPlay(from); markPlayCmd(); setU('playing', true); setTransport({ playing: true });
    say(!S.transport.rec ? 'Play' : t('d.act.recon', { name: dest }));
  }
  catch (e) { setU('playing', false); say(t('d.act.playerr', { err: String(e) })); }
}
/** `rewind` also moves the start marker (and the stopped playhead) to the beginning of the song. */
export async function stop(rewind = false) {
  // Stop while nothing plays (the second press) also cuts what still rings: reverb and delay tails, held notes, a preview
  const again = !U.playing && !U.patPlaying;
  if (U.patPlaying) void stopPreview();
  markPlayCmd();
  setU('playing', false);
  if (rewind) setU('start', 0);
  try { await be().transportStop(); markPlayCmd(); setU('playing', false); setTransport({ playing: false }); } catch (e) { console.warn(e); }
  if (again) return silence(rewind);
  say(rewind ? t('d.act.stop.rew') : 'Stop');
}
/** Ctrl+H / the second Stop (FL's "Stop sound"): silence every tail still ringing. Not an edit. */
export async function silence(rewound = false) {
  if (U.patPlaying) await stopPreview();
  try { await be().transportSilence(); } catch (e) { console.warn(e); }
  if (U.previewPath != null) setU('previewPath', null);
  say(t(rewound ? 'd.act.silence.rew' : 'd.act.silence'));
}
/**
 * PAT mode with Record on records into the looping pattern when Record captures notes and there is
 * something to play them on (a MIDI keyboard, or the computer keyboard as piano) (FL); any other
 * recording runs the song.
 */
export const patRecords = () => patMode() && S.transport.rec && S.transport.recNotes && (!!midiStatus()?.connected || typingOn());
/** Space: stop whatever plays; else PAT mode loops the current pattern, SONG mode plays the song
 * (a recording that is not PAT-mode notes runs the song, and the lit mode says so). */
export function togglePlay() {
  if (U.playing) return stop();
  if (U.patPlaying) { void stopPreview(); say('Stop'); return; }
  const p = patMode() && (!S.transport.rec || patRecords()) ? modePattern() : undefined;
  if (p) return togglePreview(p.id);
  if (S.transport.rec && patMode()) setPatMode(false);
  // SONG with an empty playlist would play silence: switch to PAT (the lit mode is always what
  // plays) and offer to put the pattern in the song
  const cur = modePattern();
  if (!S.transport.rec && !S.project.clips.length && cur?.notes.length) {
    setPatMode(true);
    const id = cur.id;
    notify(t('d.act.songEmpty', { name: cur.name }), [{ label: t('d.act.addToSong'), act: () => addPatternToSong(id) }]);
    return togglePreview(id);
  }
  return play();
}
/** Puts pattern `id` in the playlist at bar 1, on the first track with room for it (one undo step). */
export function addPatternToSong(id: Id) {
  const p = S.project.patterns.find(x => x.id === id); if (!p) return;
  const len = Math.min(p.len, TOTAL);
  const free = (i: number) => !S.project.clips.some(c => c.track === i && c.start < len && c.start + c.len > 0);
  const track = S.project.tracks.findIndex((_, i) => free(i));
  if (track < 0) return say(t('d.act.noFreeTrack'));
  const nc: Clip = { id: allocId(), track, start: 0, len, offset: 0, kind: 'pat', ref: id, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false };
  commit([{ op: 'putClips', clips: [nc] }], t('a.pl.insertClip'));
  say(t('d.act.addedToSong', { name: p.name, track: S.project.tracks[track].name }));
}
/**
 * The PAT / SONG switch (L): while something plays, what plays switches with it (FL) — the song
 * hands over to the current pattern's loop, the loop to the song from its start marker. A
 * recording keeps going.
 */
export function switchMode(on: boolean) {
  if (on === patMode()) return;
  setPatMode(on);
  if (S.transport.rec && U.playing) return;
  if (on && U.playing) { const p = modePattern(); if (p) void togglePreview(p.id); }
  else if (!on && U.patPlaying) void play(U.start);
}
/**
 * A click on the playlist's time ruler (FL): SONG mode from there. A pattern loop that is
 * playing hands over to the song at that position, so moving between them needs no L key.
 */
export function seekSong(b: number) {
  const from = Math.max(0, b);
  if (patMode()) setPatMode(false);
  if (U.patPlaying) { setU('start', from); void play(from); return; }
  seek(from);
}
/**
 * A press on the playlist's grid: working there is working on the song, so SONG lights up and
 * Space plays what is being arranged. A pattern loop that is playing keeps playing (the lit mode
 * is always what plays); the ruler hands it over to the song.
 */
export function songHere() {
  if (patMode() && !U.patPlaying) setPatMode(false);
}
/** Play / stop the song from the start marker, whatever PAT / SONG says (the pitch editor: a vocal is
 *  heard in its song, never alone in a pattern loop). */
export function toggleSong() {
  if (U.playing) return stop();
  if (U.patPlaying) void stopPreview();
  if (patMode()) setPatMode(false);
  return play(U.start);
}
export function seek(b: number) {
  setU('start', Math.max(0, b));
  if (U.playing) void be().transportPlay(U.start);
  say('Start: ' + fmtPos(U.start));
}
export async function setTransportFlag(k: 'metro' | 'countIn' | 'rec' | 'monitor', v: boolean) {
  setTransport({ [k]: v });
  try { const t = await be().transportSet({ [k]: v }); setTransport(t); } catch (e) { say(t('d.act.err', { err: String(e) })); }
}
export async function toggleRec() {
  const on = !S.transport.rec;
  const dest = recTargetName(U.playing ? curBeat() : U.start);
  await setTransportFlag('rec', on);
  const pat = on && patRecords() ? modePattern() : undefined;
  if (pat) return say(t(U.patPlaying ? 'd.act.recPatNow' : 'd.act.recPat', { name: pat.name }));
  say(!on ? (U.playing || U.patPlaying ? 'Punch out' : t('d.act.recoff'))
    : U.playing ? t('d.act.punchin', { name: dest })
    : t(S.transport.countIn ? 'd.act.recspace.ci' : 'd.act.recspace', { name: dest }));
}
// ---- the recording filter (FL's right-click on Record): what a recording captures. A setting of
// this computer, not of the song: kept in localStorage and handed to the engine at start-up.
const REC_FILTER_KEY = 'wave.recFilter';
/** Ticks or unticks one kind; the last one ticked stays on (a recording must capture something). */
export async function setRecFilter(kind: 'audio' | 'notes', on: boolean) {
  const cur = { audio: S.transport.recAudio, notes: S.transport.recNotes }, next = { ...cur, [kind]: on };
  if (!next.audio && !next.notes) return say(t('d.act.recFilterLast'));
  try {
    const tr = await be().transportSet({ recAudio: next.audio, recNotes: next.notes });
    setTransport(tr);
    saveRecFilter();
    say(t('d.act.recFilter', { what: recFilterName() }));
  } catch (e) { say(t('d.act.err', { err: String(e) })); }
}
const saveRecFilter = () => {
  const tr = S.transport;
  try { localStorage.setItem(REC_FILTER_KEY, JSON.stringify({ audio: tr.recAudio, notes: tr.recNotes, quantize: tr.recQuantize })); } catch { /* private window */ }
};
/** Input quantize of recorded notes (FL's snap while recording): beats, 0 = as played. */
export const REC_QUANTIZE = [
  { beats: 0, label: 'd.act.q.off' as const }, { beats: 0.25, label: 'd.act.q.step' as const }, { beats: 0.5, label: 'd.act.q.eighth' as const }, { beats: 1, label: 'd.act.q.beat' as const },
];
export async function setRecQuantize(beats: number) {
  try {
    setTransport(await be().transportSet({ recQuantize: beats }));
    saveRecFilter();
    const q = REC_QUANTIZE.find(x => x.beats === beats);
    say(t('d.act.recQuantize', { q: q ? t(q.label) : String(beats) }));
  } catch (e) { say(t('d.act.err', { err: String(e) })); }
}
/** At start-up: the filter last chosen on this computer. */
export async function restoreRecFilter() {
  let v: { audio?: unknown; notes?: unknown; quantize?: unknown } | null = null;
  try { v = JSON.parse(localStorage.getItem(REC_FILTER_KEY) ?? 'null'); } catch { /* none or unreadable */ }
  if (!v || typeof v.audio !== 'boolean' || typeof v.notes !== 'boolean' || !(v.audio || v.notes)) return;
  const q = REC_QUANTIZE.some(x => x.beats === v!.quantize) ? v.quantize as number : 0;
  if (v.audio === S.transport.recAudio && v.notes === S.transport.recNotes && q === S.transport.recQuantize) return;
  try { setTransport(await be().transportSet({ recAudio: v.audio, recNotes: v.notes, recQuantize: q })); } catch (e) { console.warn(e); }
}
/** "audio and notes", "audio", "notes": what Record captures. */
export function recFilterName() {
  const a = S.transport.recAudio, n = S.transport.recNotes;
  return t(a && n ? 'd.act.recWhat.both' : a ? 'd.act.recWhat.audio' : 'd.act.recWhat.notes');
}

export function toggleMonitor() {
  const on = !S.transport.monitor;
  void setTransportFlag('monitor', on);
  say(t(on ? 'd.act.mon.on' : 'd.act.mon.off'));
}

// ---------- history / project ----------
export async function undo() { if (!S.canUndo) return say(t('d.act.noUndo')); flushNotebook(); endWheelGestures(); acceptReply(await enqueue(() => be().undo())); say(t('d.act.undone')); }
export async function redo() { if (!S.canRedo) return say(t('d.act.noRedo')); flushNotebook(); endWheelGestures(); acceptReply(await enqueue(() => be().redo())); say(t('d.act.redone')); }
/** Ask about unsaved changes; false = the user cancelled (or the save failed). */
export async function guardUnsaved(what: string): Promise<boolean> {
  if (U.openingProject) { say(t('d.act.guard.opening')); return false; }
  flushNotebook();
  const a = await confirmUnsaved(what);
  if (a === 'busy') { say(t('d.act.guard.busy')); return false; }
  if (a === 'cancel') return false;
  if (a === 'save') return projectSave(false);
  return true;
}
/** A new empty project, or with `demo` the built-in demo project (asks about unsaved changes first). */
export async function projectNew(demo = false): Promise<boolean> {
  endWheelGestures();
  if (U.openingProject || inGesture()) { say(t('d.act.guard.busy')); return false; }
  if (!(await guardUnsaved(t(demo ? 'd.act.guard.demo' : 'd.act.guard.new')))) return false;
  try {
    acceptReply(await enqueue(() => be().projectNew(demo)));
    afterProjectLoad();
    if (demo) setPatMode(false);
    else if (!S.project.clips.length) openRack();
    say(t(demo ? 'd.act.demoproj' : 'd.act.newproj'));
    return true;
  } catch (e) { say(t('d.act.newfail', { err: String(e) })); return false; }
}
export async function projectOpen(path?: string) {
  if (path && samePath(path, S.path)) return;
  endWheelGestures();
  flushNotebook();
  await enqueue(() => Promise.resolve());
  if (U.openingProject || inGesture()) { say(t('d.act.guard.busy.open')); return; }
  const revision = projectRevision();
  const answer = await confirmUnsaved(t('d.act.guard.open'));
  if (answer === 'busy') { say(t('d.act.guard.busy.open')); return; }
  if (answer === 'cancel' || U.openingProject) return;
  if (inGesture()) { say(t('d.act.guard.busy.open')); return; }
  setU('openingProject', true);
  try {
    if (answer === 'save' && !(await projectSave(false))) return;
    if (answer === 'save' && S.dirty) { say(t('d.act.guard.busy.open')); return; }
    const approvedRevision = answer === 'save' ? projectRevision() : revision;
    const r = await enqueue(() => be().projectOpen(path, approvedRevision));
    if (r.cancelled) return;
    acceptReply(r.state); afterProjectLoad();
    const openedPath = S.path ?? '';
    say(r.warnings.length ? t('d.act.openedwarn', { path: openedPath, n: r.warnings.length, first: r.warnings[0] }) : t('d.act.opened', { path: openedPath }));
    offerRelink();
  } catch (e) { say(t('d.act.openfail', { err: String(e) })); }
  finally { setU('openingProject', false); }
}
/** A project just loaded with audio files missing: show them with the ways to find them. */
export function offerRelink() {
  if (Object.keys(S.missing).length) setU('missingOpen', true);
}
/** File → Import MIDI: pick a .mid (unless given) and show its parts; a new pattern goes at `beat`. */
export async function midiImport(opts: { path?: string; beat?: number; track?: number; into?: { pattern: Id; ch: Id } } = {}) {
  try {
    const path = opts.path ?? (await be().pickMidiFile());
    if (path) setU('midiWin', { path, beat: opts.beat ?? U.start, track: opts.track, into: opts.into });
  } catch (e) { say(t('d.midi.fail', { err: String(e) })); }
}
/** Writes the song (pattern clips as placed on the Playlist) or one pattern as a .mid file. */
export async function midiExport(pattern?: Id) {
  const pat = pattern != null ? S.project.patterns.find(p => p.id === pattern) : undefined;
  const base = (pat?.name ?? (S.path?.split(/[\\/]/).pop()?.replace(/\.(synkopa|wave)$/i, '') || 'Synkopa')).replace(/[<>:"/\\|?*]+/g, '_');
  try {
    const out = await be().pickMidiSave(base + '.mid');
    if (!out) return;
    await enqueue(() => Promise.resolve());
    const r = await be().midiExport({ out, pattern });
    say(t('d.midi.exported', { path: r.out, n: r.notes, tracks: r.tracks }));
  } catch (e) { say(t('d.midi.fail', { err: String(e) })); }
}
export async function projectSave(saveAs = false, version = false): Promise<boolean> {
  flushNotebook();
  try {
    const r = await enqueue(() => be().projectSave(saveAs, version));
    if (r.cancelled) return false; // the save dialog was dismissed: nothing was written
    if (r.path) rememberProjView(r.path);
    say(t('d.act.saved', { path: r.path })); await resync(); return true;
  } catch (e) { say(t('d.act.savefail', { err: String(e) })); return false; }
}
export async function projectExportZip() {
  if (U.projectZipBusy || U.openingProject) return;
  flushNotebook();
  endWheelGestures();
  setU('projectZipBusy', true);
  try {
    const revision = await enqueue(async () => projectRevision());
    say(t('d.act.zipWorking'));
    const result = await be().projectExportZip(revision);
    if (result.cancelled) say(t('d.act.exportcancel'));
    else say(t('d.act.zipDone', { path: result.path }));
  } catch (e) {
    say(t('d.act.zipFailed', { err: String(e) }));
  } finally {
    setU('projectZipBusy', false);
  }
}
/** Autosave tick for a project without a file: keeps its crash-recovery copy current. */
export async function recoveryTick() {
  try { await enqueue(() => be().recoveryWrite()); } catch (e) { console.warn('[wave] recovery copy:', e); }
}
/**
 * Startup: offer back the unsaved projects a Synkopa that crashed or was killed left behind,
 * newest first. "Not now" keeps them for the next start; one restore ends the round.
 * Also runs after a launch that opened a file (Explorer double-click): the question names
 * that project, and its unsaved edits go through the usual Save / Discard / Cancel.
 */
export async function offerRecovery() {
  let list: RecoveredProject[];
  try { list = await be().recoveryList(); } catch { return; }
  for (const r of list) {
    const a = await confirmRecover(r.savedMs, S.path);
    if (a === 'later') return;
    if (a === 'discard') { await be().recoveryDiscard(r.id).catch(e => say(t('d.act.err', { err: String(e) }))); continue; }
    if (!(await guardUnsaved(t('d.act.guard.open')))) return;
    try {
      const res = await enqueue(() => be().recoveryRestore(r.id));
      acceptReply(res.state); afterProjectLoad();
      say(res.warnings.length ? t('d.act.openedwarn', { path: t('d.ui.unnamed'), n: res.warnings.length, first: res.warnings[0] }) : t('d.act.recovered'));
      offerRelink();
    } catch (e) { say(t('d.act.recoverfail', { err: String(e) })); }
    return;
  }
}
/** Window close: ask about unsaved changes, then really close. */
export async function closeWindowGuarded(): Promise<boolean> {
  return guardUnsaved(t('d.act.guard.close'));
}
/** Escape on the export dialog: abort an in-flight bounce, otherwise close. */
export function cancelExportOrClose() {
  if (!U.exportOpen) return;
  if (U.exportBusy) {
    void be().exportCancel().catch(() => {});
    say(t('d.act.exportcancel'));
  }
  setU({ exportOpen: false, exportBusy: false });
}
const PROJ_VIEWS_KEY = 'wave.projViews';
function loadProjViews() { try { return parseViews(localStorage.getItem(PROJ_VIEWS_KEY)); } catch { return []; } }
/** Remember where the user is in the project just saved at `path` (see lib/projView.ts). */
function rememberProjView(path: string) {
  try { localStorage.setItem(PROJ_VIEWS_KEY, JSON.stringify(rememberView(loadProjViews(), path, { pat: U.pat, selCh: U.selCh, start: U.start }))); } catch { /* private window */ }
}
function afterProjectLoad() {
  clearPeaks();
  batch(() => {
    // back where this project was last saved (FL): its pattern, channel and start marker; else the first ones
    const v = viewFor(loadProjViews(), S.path, S.project, TOTAL);
    setU({ sel: [], chSel: [], plugs: [], editor: null, start: v.start, menu: null, mixMulti: [] });
    const m = S.project.inserts[1] ?? S.project.inserts[0];
    if (m) setU({ mixSel: m.id, fxSel: m.fx[0]?.id ?? null });
    setU({ cur: v.pat != null ? { kind: 'pat', ref: v.pat, clipId: null } : null, pat: v.pat, selCh: v.selCh });
  });
}

// ---------- tempo / loop ----------
export const snapUnit = (alt = false) => (alt || U.alt ? 0 : snapRes(U.snap, U.bw));
export function setLoop(loop: { a: number; b: number } | null, label = t('d.act.loop')) {
  commit([{ op: 'setLoop', loop }], label);
}

// ---------- selection & clips ----------
export const selClips = (): Clip[] => { const s = new Set(U.sel); return S.project.clips.filter(c => s.has(c.id)); };
let clipboard: Clip[] = [];
/** Where Ctrl+V puts the next copy: right after the copied block, then after each paste (chains like the piano roll). */
let pasteAt: number | null = null;
export const hasClipboard = () => clipboard.length > 0;

/** Delete / Backspace with envelope points selected: they go (one undo step; one point always stays). */
export function deleteAutoPoints() {
  const s = U.autoSel; if (!s) return;
  const c = S.project.clips.find(x => x.id === s.clip), a = c && S.project.automations?.find(x => x.id === c.ref);
  if (!a) { setU('autoSel', null); return; }
  const pts = deletePoints(plain(a.points), new Set(s.idx));
  if (!pts) { say(t('a.pl.autoMinPoint')); return; }
  commit([{ op: 'putAutomation', automation: { ...plain(a), points: pts } }], t('a.pl.deletePoint'));
  setU('autoSel', null);
  say(t('a.pl.pointsDeleted', { n: s.idx.length }));
}
export function delIds(ids: Id[]) {
  if (!ids.length) return;
  commit([{ op: 'removeClips', ids: [...ids] }], t('d.act.op.delclips'));
  const s = new Set(ids);
  setU('sel', U.sel.filter(x => !s.has(x)));
}
export function duplicate() {
  const cs = selClips(); if (!cs.length) return;
  const span = duplicateSpan(cs, snapUnit());
  if (Math.max(...cs.map(c => c.start + c.len)) + span > TOTAL) return say(t('d.act.nospace'));
  const copies = cs.map(c => ({ ...plain(c), id: allocId(), start: c.start + span }));
  commit([{ op: 'putClips', clips: copies }], t('d.act.op.dup'));
  setU('sel', copies.map(c => c.id));
  say(t('d.act.duplicated', { n: copies.length }));
}
export function copy() { const cs = selClips(); if (!cs.length) return; clipboard = cs.map(plain); pasteAt = Math.max(...clipboard.map(c => c.start + c.len)); say(t('d.act.copied', { n: cs.length })); }
export function cut() { copy(); delIds(U.sel); }
export function paste(at?: { beat: number; track: number }) {
  if (!clipboard.length) return;
  const beat = at ? at.beat : pasteAt ?? U.start;
  const copies = pasteClips(clipboard, beat, at ? at.track : null, S.project.tracks.length, allocId);
  if (!at) pasteAt = Math.max(...copies.map(c => c.start + c.len));
  commit([{ op: 'putClips', clips: copies }], t('d.act.op.paste'));
  setU('sel', copies.map(c => c.id));
  say(t('d.act.pasted', { n: copies.length, pos: fmtPos(beat) }));
}
/** Playlist resize mode: trim ↔ stretch (Shift+M, the switch in the playlist's corner). */
export function toggleStretchMode() {
  setU('stretchMode', !U.stretchMode);
  say(t(U.stretchMode ? 'a.pl.stretchOn' : 'a.pl.stretchOff'));
}
/** How the selected audio clips (or `c`) stretch: keeping pitch, or resampled (speed and pitch together). */
export function setStretchMethod(c: Clip, resample: boolean) {
  const ids = U.sel.includes(c.id) ? U.sel : [c.id];
  const cs = S.project.clips.filter(x => ids.includes(x.id) && x.kind === 'audio' && !!x.resample !== resample).map(plain);
  if (!cs.length) return;
  commit([{ op: 'putClips', clips: cs.map(x => ({ ...x, resample })) }], t(resample ? 'a.pl.resampleOp' : 'a.pl.keepPitchOp'));
}
export function toggleMuteSel() {
  const cs = selClips(); if (!cs.length) return;
  commit(muteOps(cs.map(plain), !cs.every(c => c.muted)), t('d.act.op.muteclips'));
}
export function nudge(db: number, dt: number) {
  const cs = selClips().map(plain); if (!cs.length) return;
  const [d, dtr] = clampMove(cs, db, dt, S.project.tracks.length);
  if (!d && !dtr) return;
  commit([{ op: 'putClips', clips: moveClips(cs, d, dtr) }], t('d.act.op.move'));
}
/** Split clip `id` at beat `b`; with `withSel`, when it is part of the selection, every selected clip that crosses `b` (one undo step). */
export function sliceAt(id: Id, b: number, withSel = true) {
  const ids = withSel && U.sel.includes(id) ? U.sel : [id];
  const r = S.project.clips.filter(x => ids.includes(x.id)).flatMap(c => sliceClip(plain(c), b, allocId()) ?? []);
  if (!r.length) return;
  commit([{ op: 'putClips', clips: r }], t('d.act.op.split'));
  say(t('d.act.cut', { pos: fmtPos(b) }));
}
export function selectAll() { setU('sel', S.project.clips.map(c => c.id)); }
export function makeUnique(clip: Clip) {
  if (clip.kind === 'audio') return void makeUniqueAudio(clip);
  const p = S.project.patterns.find(x => x.id === clip.ref); if (!p) return;
  const baseName = p.name.replace(/ #\d+$/, '');
  const count = S.project.patterns.filter(x => x.name.startsWith(baseName)).length;
  const np = { ...plain(p), id: allocId(), name: baseName + ' #' + (count + 1) };
  commit([{ op: 'putPattern', pattern: np }, { op: 'putClips', clips: [{ ...plain(clip), ref: np.id }] }], 'Make unique');
  setU('cur', { kind: 'pat', ref: np.id, clipId: null });
  say(t('d.act.newpat', { name: np.name }));
}
/**
 * FL's "Make unique" for audio: the clip — with the selected clips of the same sample on its
 * track, when it is selected — gets its own copy of the sample on a new track right below,
 * playing where the old track does (no mixer insert is claimed). One undo step (`Backend.makeUnique`).
 */
async function makeUniqueAudio(clip: Clip) {
  const ids = U.sel.includes(clip.id)
    ? S.project.clips.filter(c => U.sel.includes(c.id) && c.kind === 'audio' && c.ref === clip.ref && c.track === clip.track).map(c => c.id)
    : [clip.id];
  endWheelGestures();
  try {
    // queued behind pending edits; then the new state, so the names below are the engine's
    const r = await enqueue(() => be().makeUnique(ids));
    await resync();
    setU({ sel: ids, cur: { kind: 'audio', ref: r.sourceId, clipId: null } });
    const name = S.project.sources.find(s => s.id === r.sourceId)?.name ?? '';
    say(t('d.act.madeUnique', { name }));
  } catch (e) {
    say(t('d.act.uniqueFail', { err: String(e) }));
    void resync();
  }
}
/**
 * FL's *Export in place*: the selected clips (or `clip` alone when it is not selected) rendered
 * into one audio clip on a new track right below them; the originals are muted. One undo step.
 */
export async function exportInPlace(clip?: Clip) {
  const ids = clip && !U.sel.includes(clip.id) ? [clip.id] : S.project.clips.filter(c => U.sel.includes(c.id) && c.kind !== 'auto').map(c => c.id);
  if (!ids.length) { say(t('d.act.inPlaceNone')); return; }
  endWheelGestures();
  say(t('d.act.inPlaceBusy', { n: ids.length }));
  try {
    const r = await enqueue(() => be().exportInPlace(ids));
    if ('cancelled' in r) { say(t('d.act.inPlaceCancelled')); return; }
    await resync();
    setU({ sel: [r.clipId], cur: { kind: 'audio', ref: r.sourceId, clipId: null } });
    say(t('d.act.inPlaceDone', { name: S.project.sources.find(s => s.id === r.sourceId)?.name ?? '' }));
  } catch (e) {
    say(t('d.act.inPlaceFail', { err: String(e) }));
    void resync();
  }
}
export function setSrcProp(clip: Clip, patch: { name?: string; color?: string }) {
  if (clip.kind === 'auto') {
    const a = S.project.automations?.find(x => x.id === clip.ref); if (a) commit([{ op: 'putAutomation', automation: { ...plain(a), ...patch } }], t('d.act.op.auto'));
  } else if (clip.kind === 'pat') {
    const p = S.project.patterns.find(x => x.id === clip.ref); if (p) commit([{ op: 'putPattern', pattern: { ...plain(p), ...patch } }], 'Pattern');
  } else {
    const s = S.project.sources.find(x => x.id === clip.ref); if (s) commit([{ op: 'putSource', source: { ...plain(s), ...patch } }], t('d.act.op.source'));
  }
}

// ---------- tracks ----------
export function trackOp(i: number, patch: Partial<Track>): Op | null {
  const t = S.project.tracks[i]; if (!t) return null;
  return { op: 'putTrack', track: { ...plain(t), ...patch } };
}
export function setTrack(i: number, patch: Partial<Track>, label = t('d.act.track')) { const op = trackOp(i, patch); if (op) commit([op], label); }
export function armTrack(i: number) {
  const on = !S.project.tracks[i]?.arm;
  const ops = S.project.tracks.map((t, j) => ({ t, arm: j === i ? on : false })).filter(x => x.t.arm !== x.arm)
    .map(x => ({ op: 'putTrack', track: { ...plain(x.t), arm: x.arm } }) as Op);
  commit(ops, t('d.act.op.arm'));
  say(on ? t('d.act.recon2', { name: S.project.tracks[i].name }) : t('d.act.disarmed'));
}
/** Mute and solo are mutually exclusive per channel: turning one on clears the other (one undo step). */
export function toggleTrackMute(i: number) {
  const t = S.project.tracks[i]; if (!t) return;
  setTrack(i, t.muted ? { muted: false } : { muted: true, solo: false }, 'Mute');
}
/** Solo is exclusive (FL): soloing a track un-solos the others; Ctrl+click adds to the solo set. */
export function toggleTrackSolo(i: number, add = false) {
  const t = S.project.tracks[i]; if (!t) return;
  if (t.solo) { setTrack(i, { solo: false }, 'Solo'); return; }
  const ops = S.project.tracks.map((x, j) => j === i ? trackOp(j, { solo: true, muted: false }) : (!add && x.solo ? trackOp(j, { solo: false }) : null)).filter((o): o is Op => !!o);
  commit(ops, 'Solo');
}
export function toggleInsertMute(id: Id) {
  const m = S.project.inserts.find(x => x.id === id); if (!m) return;
  // a strip of the selected group mutes or unmutes the whole group
  const group = selectedInserts(), ids = group.length > 1 && group.includes(id) ? group : [id];
  const patch = m.muted ? { muted: false } : { muted: true, solo: false };
  const ops = ids.map(x => insertOp(x, patch)).filter((o): o is Op => !!o);
  commit(ops, 'Mute');
}
export function toggleInsertSolo(id: Id, add = false) {
  const m = S.project.inserts.find(x => x.id === id); if (!m || id === 0) return;
  if (m.solo) { setInsert(id, { solo: false }, 'Solo'); return; }
  const ops = S.project.inserts.map(x => x.id === id ? insertOp(x.id, { solo: true, muted: false }) : (!add && x.solo ? insertOp(x.id, { solo: false }) : null)).filter((o): o is Op => !!o);
  commit(ops, 'Solo');
}
export function removeTrack(id: number) {
  commit([{ op: 'removeTrack', id }], t('d.act.op.deltrack'));
}

export function insertTrack(i: number) {
  commit(insertTrackOps(plain(S.project), i, allocId()), t('d.act.op.addtrack'));
}
export const addTrack = () => insertTrack(S.project.tracks.length);
/** Move lane `i` one place up (`d` −1) or down (+1), with its clips. */
export function moveTrack(i: number, d: -1 | 1) {
  const ops = swapTrackOps(plain(S.project), i, i + d);
  if (!ops.length) return;
  const a = S.project.tracks[i].id, b = S.project.tracks[i + d].id, open = { a: U.takesOpen[a], b: U.takesOpen[b] };
  commit(ops, t('d.act.op.movetrack'));
  // ids stay with their slots, so what is keyed by a slot's id trades places with the lanes: the open take lanes
  setU('takesOpen', { ...U.takesOpen, [a]: open.b, [b]: open.a });
  say(t(d < 0 ? 'd.act.trackUp' : 'd.act.trackDown', { name: S.project.tracks[i + d]?.name ?? '' }));
}

// ---------- mixer ----------
export function insertOp(id: Id, patch: Partial<Insert>): Op | null {
  const m = S.project.inserts.find(x => x.id === id); if (!m) return null;
  const { fx: _fx, ...rest } = plain(m); void _fx;
  return { op: 'putInsert', insert: { ...rest, fx: [], ...patch } as Insert };
}
export function setInsert(id: Id, patch: Partial<Insert>, label = t('d.act.mixer')) { const op = insertOp(id, patch); if (op) commit([op], label); }
export function selectInsert(id: Id) {
  const m = S.project.inserts.find(x => x.id === id);
  // a plain click on a strip outside the group drops the group; inside it, the group stays (to drag its faders)
  if (U.mixMulti.length && !U.mixMulti.includes(id)) setU('mixMulti', []);
  if (m && U.mixSel !== id) setU({ mixSel: id, fxSel: m.fx[0]?.id ?? null });
}
/**
 * The selected mixer strips: the current one first, then the rest of its group (Ctrl+Shift+click).
 * `mixMulti` holds every member of the group, and the group only counts while the current strip is
 * one of them: when something else moves the selection (a rack channel followed to its insert, an
 * effect opened), that strip is alone and never drags a stale group with it.
 */
export function selectedInserts(): Id[] {
  const ids = new Set(S.project.inserts.map(m => m.id));
  const g = U.mixMulti.filter(id => ids.has(id));
  return g.length > 1 && g.includes(U.mixSel) ? [U.mixSel, ...g.filter(id => id !== U.mixSel)] : [U.mixSel];
}
/** Ctrl+Shift+click on a strip (FL): add it to the selection, or take it out. */
export function toggleInsertSelected(id: Id) {
  const cur = selectedInserts();
  if (!cur.includes(id)) setU('mixMulti', [...cur, id]);
  else {
    const rest = cur.filter(x => x !== id);
    if (!rest.length) return; // the only selected strip stays selected
    const next = S.project.inserts.find(m => m.id === rest[0]);
    setU({ mixMulti: rest.length > 1 ? rest : [], ...(id === U.mixSel ? { mixSel: rest[0], fxSel: next?.fx[0]?.id ?? null } : {}) });
  }
  say(t('d.act.stripsSel', { n: selectedInserts().length }));
}
export function addInsert() {
  const id = allocId();
  commit([{ op: 'putInsert', insert: { id, name: 'Insert ' + S.project.inserts.length, color: '#8b93a1', vol: VOL_UNITY, pan: 0, muted: false, solo: false, fx: [] } }], t('d.act.op.addinsert'));
  setU({ mixSel: id, fxSel: null });
}
export function removeInsert(id: Id) {
  const m = S.project.inserts.find(x => x.id === id);
  m?.fx.forEach(f => closePlug(f.id));
  commit([{ op: 'removeInsert', id }], t('d.act.op.delinsert'));
  if (U.mixSel === id) setU({ mixSel: 0, fxSel: null });
}

// ---------- effects ----------
export function fxOp(fx: Fx, insId?: Id): Op {
  const ins = insId ?? findFx(fx.id)?.m.id ?? 0;
  return { op: 'putFx', insertId: ins, fx: plain(fx) };
}
export function addFx(insId: Id, type: AnyBuiltinKind, preset?: number) {
  if (!S.catalog) return;
  let f = newFx(S.catalog, allocId(), type, S.project.bpm);
  if (preset != null && preset > 0 && presetList(S.catalog, type)[preset]) f = loadPresetFx(S.catalog, f, preset, S.project.bpm);
  commit([{ op: 'putFx', insertId: insId, fx: f }], t('d.act.op.addfx'));
  setU({ mixSel: insId, fxSel: f.id });
  openPlug(f.id);
  const pr = presetList(S.catalog, type)[preset ?? 0];
  say(t('d.act.added', { name: tName(S.catalog.types[type]?.name ?? type) }) + (pr ? t('d.act.added.preset', { name: tName(pr.name) }) : ''));
}
/** Put a scanned plugin on an insert. One that cannot run here (`unsupported`) is refused with its reason. */
export function addPlugin(insId: Id, info: PluginInfo): boolean {
  if (info.unsupported) { say(t('d.act.pluginUnsupported', { name: info.name, why: info.unsupported })); return false; }
  const f: Fx = { id: allocId(), type: 'plugin', bypass: false, preset: null, params: {}, plugin: { format: info.format, path: info.path, cid: info.cid, name: info.name, vendor: info.vendor } };
  commit([{ op: 'putFx', insertId: insId, fx: f }], t('d.act.op.addplugin', { format: PLUGIN_FORMAT_LABEL[info.format] }));
  setU({ mixSel: insId, fxSel: f.id });
  openPlug(f.id);
  say(t('d.act.added', { name: info.name }));
  return true;
}
/**
 * Effects dragged onto another mixer insert (FL): moved to the end of its chain — the running
 * instance, its automation and an open window go with it — or, with `copy`, copied there (a hosted
 * plugin with its current sound). One undo step.
 */
export async function moveFxTo(fxIds: Id[], toIns: Id, copy: boolean) {
  const target = S.project.inserts.find(m => m.id === toIns); if (!target) return;
  const list = fxIds.map(id => findFx(id)).filter((x): x is NonNullable<typeof x> => !!x && (copy || x.m.id !== toIns));
  if (!list.length) return;
  const ops: Op[] = [];
  for (const { f } of list) {
    if (!copy) { ops.push({ op: 'removeFx', fxId: f.id }, { op: 'putFx', insertId: toIns, fx: plain(f) }); continue; }
    let fx: Fx = { ...plain(f), id: allocId() };
    if (fx.type === 'plugin' && fx.plugin && !S.fxErrors[f.id]) {
      try { fx = { ...fx, plugin: { ...fx.plugin, state: await be().pluginState(f.id) } }; } catch (e) { console.warn(e); }
    }
    ops.push({ op: 'putFx', insertId: toIns, fx });
  }
  const one = list.length === 1 ? fxName(list[0].f) : t('d.act.fxN', { n: list.length });
  commit(ops, t(copy ? 'd.act.op.copyfx' : 'd.act.op.movefx'));
  setU({ mixSel: toIns, fxSel: copy ? (ops[ops.length - 1] as { fx: Fx }).fx.id : list[list.length - 1].f.id });
  say(t(copy ? 'd.act.fxCopied' : 'd.act.fxMoved', { name: one, to: insName(target) }));
}
export function removeFx(fxId: Id) {
  closePlug(fxId);
  commit([{ op: 'removeFx', fxId }], t('d.act.op.delfx'));
  if (U.fxSel === fxId) setU('fxSel', null);
}
export function toggleBypass(fxId: Id) {
  const ff = findFx(fxId); if (!ff) return;
  commit([fxOp({ ...plain(ff.f), bypass: !ff.f.bypass }, ff.m.id)], 'Bypass');
  say(fxName(ff.f) + (ff.f.bypass ? ' · bypass' : t('d.act.active')));
}
/** FL-style: bypass every effect of an insert, or turn them all back on if all are bypassed. */
/** Effects that were on when "N FX" switched an insert off, so switching back restores them. */
const fxWasOn = new Map<Id, Set<Id>>();
/** Switch an effect to another built-in type in place (e.g. clean compressor ↔ FET); default preset. */
export function swapFxType(fxId: Id, type: AnyBuiltinKind) {
  const ff = findFx(fxId); if (!ff || !S.catalog || ff.f.type === type) return;
  const idx = ff.m.fx.findIndex(f => f.id === fxId);
  const f = newFx(S.catalog, allocId(), type, S.project.bpm);
  f.bypass = ff.f.bypass;
  commit([{ op: 'removeFx', fxId }, { op: 'putFx', insertId: ff.m.id, fx: f, index: idx }], t('d.act.op.comptype'));
  closePlug(fxId); openPlug(f.id);
  say(t('d.act.defaults', { name: tName(S.catalog.types[type]?.name ?? type) }));
}
export function toggleInsertFx(insId: Id) {
  const m = S.project.inserts.find(x => x.id === insId); if (!m || !m.fx.length) return;
  const on = m.fx.some(f => !f.bypass);
  if (on) fxWasOn.set(m.id, new Set(m.fx.filter(f => !f.bypass).map(f => f.id)));
  const keep = fxWasOn.get(m.id);
  const restore = !on && keep && m.fx.some(f => keep.has(f.id));
  commit(m.fx.map(f => fxOp({ ...plain(f), bypass: on ? true : restore ? !keep!.has(f.id) : false }, m.id)), on ? t('d.act.op.fxoff') : t('d.act.op.fxon'));
  if (!on) fxWasOn.delete(m.id);
  say(insName(m) + t(on ? 'd.act.fxoff' : 'd.act.fxon'));
}
export const fxName = (f: Fx) => f.type === 'plugin' ? (f.plugin?.name ?? 'Plugin') : (S.catalog?.types[f.type]?.name ?? f.type);
export function loadPreset(fxId: Id, i: number) {
  const ff = findFx(fxId); if (!ff || !S.catalog) return;
  const list = presetList(S.catalog, ff.f.type); if (!list.length) return;
  const nf = loadPresetFx(S.catalog, plain(ff.f), i, S.project.bpm);
  commit([fxOp(nf, ff.m.id)], 'Preset');
  say(fxName(ff.f) + ' · ' + t('d.act.preset', { i: (nf.preset ?? 0) + 1, n: list.length, name: tName(list[nf.preset ?? 0].name) }));
}
export function stepPreset(fxId: Id, d: number) {
  const ff = findFx(fxId); if (!ff) return;
  const info = presetInfo(S.catalog, ff.f, S.project.bpm);
  loadPreset(fxId, info.idx < 0 ? (d > 0 ? 0 : -1) : info.idx + d);
}

// ---------- windows ----------
const PLUG_W: Partial<Record<string, number>> = { eq: 780, reverb: 640, plugin: 560, tune: 800, fet: 700, deess: 680, denoise: 640, voice: 700, delay: 760, warp: 820 };
/** Open (or bring to front) an effect window. The newest/most recently focused window is always on top. */
/** Playlist to the front: floating windows go behind it (they keep their place and state); docked panels stay. */
/** F5 / a click on the Playlist: every floating window steps aside (effects, pattern windows, the sample, channel and pitch editors). */
export function playlistFront() { tuckBelow(U.zTop); }
/** The floating windows up to stack place `z` step behind the workspace; one opened or raised later shows on its own. */
export function tuckBelow(z: number) {
  if (!shownFloating().some(f => f.z <= z)) return;
  setU({ plugsHidden: true, tuckZ: U.plugsHidden ? Math.max(U.tuckZ, z) : z });
}
/**
 * A plain click in the workspace (Browser, a docked Channel Rack / Piano roll) sends the floating
 * windows behind it, like a click on the Playlist. It acts on release and only without a drag, so dragging
 * a sample from the Browser onto a floating Channel Rack keeps the rack; a window the click itself opens
 * (an effect slot) comes after `z` and stays in front.
 */
export function tuckOnClick(e: PointerEvent) {
  if (e.button !== 0) return;
  // the docked Mixer's effect windows belong to it: muting, selecting or adjusting a strip keeps the EQ you are listening to
  if ((e.target as Element | null)?.closest?.('.mixdock')) return;
  const sx = e.clientX, sy = e.clientY, z = U.zTop;
  const done = (ev: PointerEvent) => {
    window.removeEventListener('pointerup', done, true); window.removeEventListener('pointercancel', done, true);
    if (ev.type === 'pointerup' && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 5) tuckBelow(z);
  };
  window.addEventListener('pointerup', done, true); window.addEventListener('pointercancel', done, true);
}
export function showPlugs() { setU('plugsHidden', false); }

/** The real-time Pitch Correct effect onto insert `insId` (key and scale from the caller, e.g. the pitch editor's). */
export function addPitchCorrect(insId: Id, ks?: { key: number; scale: number }) {
  if (!S.catalog || insId === 0) return;
  const f = newFx(S.catalog, allocId(), 'tune', S.project.bpm);
  if (ks) f.params = { ...f.params, key: ks.key, scale: ks.scale };
  commit([{ op: 'putFx', insertId: insId, fx: f }], t('d.act.op.addfx'));
  setU({ mixSel: insId, fxSel: f.id });
  openPlug(f.id);
  const m = S.project.inserts.find(x => x.id === insId);
  say(t('d.pc.added', { name: m ? insName(m) : '' }));
}
/**
 * Open an effect. A hosted plugin shows its own window (FL: Synkopa's header with a gear above the plugin's
 * interface); Synkopa's window for it comes up when it has none, or from that gear (`settings`).
 */
export function openPlug(fxId: Id, settings = false) {
  if (!settings && findFx(fxId)?.f.type === 'plugin') { void showPlugin(fxId, () => openPlugWindow(fxId)); return; }
  openPlugWindow(fxId);
}
/** Synkopa's own window for an effect (for a plugin: its parameters, under the gear). */
function openPlugWindow(fxId: Id) {
  const ex = U.plugs.find(p => p.fxId === fxId);
  if (ex) { plugFront(fxId, true); return; }
  const ty = findFx(fxId)?.f.type ?? '';
  const w = Math.min(PLUG_W[ty] ?? 580, window.innerWidth - 40), n = U.plugs.length;
  const z = U.zTop + 1;
  setU({ zTop: z, plugs: [...U.plugs, { fxId, x: clamp(140 + (n % 8) * 28, 10, window.innerWidth - w - 10), y: clamp(64 + (n % 8) * 28, 10, window.innerHeight - 200), w, z }] });
}
export const closePlug = (fxId: Id) => setU('plugs', U.plugs.filter(p => p.fxId !== fxId));
/** Raise a window above all others (array order stays put, so canvases are never re-created). */
export function plugFront(fxId: Id, force = false) {
  const p = U.plugs.find(x => x.fxId === fxId); if (!p) return;
  if (!force && p.z === U.zTop && !tucked(p.z)) return;
  const z = U.zTop + 1;
  setU('zTop', z);
  setU('plugs', x => x.fxId === fxId, 'z', z);
}
export const topPlug = () => U.plugs.reduce<typeof U.plugs[number] | null>((a, p) => (!a || p.z > a.z ? p : a), null);
export function movePlug(fxId: Id, x: number, y: number) { setU('plugs', p => p.fxId === fxId, { x, y }); }
export function openEditor(id: Id) {
  const c = S.project.clips.find(x => x.id === id); if (!c || c.kind !== 'audio') return;
  openSampleEditor({ kind: 'clip', id });
  setU('sel', [id]);
}
export function openSampleEditor(target: SampleTarget, position?: { x: number; y: number }) {
  if (U.previewing) void be().previewStop();
  const w = Math.min(U.editor?.w ?? 620, window.innerWidth - 20);
  const x = U.editor?.x ?? position?.x ?? (window.innerWidth - w) / 2;
  const y = U.editor?.y ?? position?.y ?? 64;
  const z = U.zTop + 1;
  setU({ zTop: z, editor: { ...target, x: clamp(x, 0, window.innerWidth - w), y, w, z } });
}
/** The sample editor over the plugin / pattern windows (it is used last). */
export function editorFront() {
  if (!U.editor || ((U.editor.z ?? 0) === U.zTop && !tucked(U.editor.z))) return;
  const z = U.zTop + 1;
  setU('zTop', z); setU('editor', ed => ed && { ...ed, z });
}
/** Writes a new file with only the part the clip plays; the clip plays it (one undo step). */
export async function trimClip(id: Id) {
  try { await enqueue(() => be().trimClip(id)); say(t('d.smp.trimmed')); }
  catch (e) { say(t('d.smp.trimfail', { err: String(e) })); void resync(); }
}
export function closeEditor() { stopSamplePreview(); setU('editor', null); }
/** Stops a ringing sample preview (the editor window stays open). */
export function stopSamplePreview() { if (U.previewing) void be().previewStop(); }
/** The note-based pitch editor of an audio clip. */
export function openPitchEditor(id: Id, melody?: { channel: Id; span?: [number, number] | null }) {
  const c = S.project.clips.find(x => x.id === id); if (!c || c.kind !== 'audio') return;
  // already open on this clip: keep its place, only pass the melody on
  const z = U.zTop + 1;
  if (U.pitchEd?.clipId === id) { setU({ zTop: z }); setU('pitchEd', ed => ed && { ...ed, z, ...(melody ? { melody } : {}) }); return; }
  const w = Math.min(1180, window.innerWidth - 40), h = Math.min(600, window.innerHeight - 90);
  setU({ zTop: z, pitchEd: { clipId: id, x: Math.max(20, (window.innerWidth - w) / 2), y: Math.max(44, (window.innerHeight - h) / 2 - 20), w, h, z, melody }, sel: [id] });
}
/** The pitch editor over the plugin / pattern windows (it is used last). */
export function pitchFront() {
  if (!U.pitchEd || ((U.pitchEd.z ?? 0) === U.zTop && !tucked(U.pitchEd.z))) return;
  const z = U.zTop + 1;
  setU('zTop', z); setU('pitchEd', ed => ed && { ...ed, z });
}
export const closePitchEditor = () => { stopSamplePreview(); setU('pitchEd', null); };

export function openMusicAnalysis(args: { clipId?: Id; fxId?: Id }) {
  setU('musicWin', args);
}
export const closeMusicAnalysis = () => setU('musicWin', null);

/** Set project BPM and scale `clipId`'s length/offset so it still covers the same seconds of audio. */
export function applyDetectedTempo(bpm: number, clipId?: Id) {
  const nb = clamp(Math.round(bpm * 100) / 100, 20, 999);
  const oldBpm = S.project.bpm;
  const ops: Op[] = [{ op: 'setBpm', bpm: nb }];
  if (clipId != null && Math.abs(nb - oldBpm) > 1e-6) {
    const c = S.project.clips.find(x => x.id === clipId);
    if (c && c.kind === 'audio') {
      const ratio = nb / oldBpm;
      const nc: Clip = {
        ...plain(c),
        offset: c.offset * ratio,
        len: clamp(c.len * ratio, 1 / 16, TOTAL - c.start),
      };
      ops.push({ op: 'putClips', clips: [nc] });
    }
  }
  commit(ops, t('d.act.op.detected'));
  say(t('d.act.tempoSet', { bpm: nb.toFixed(2) }));
}

/** Apply detected key/scale (1 = Major, 2 = Minor) to one Pitch Correct instance or all of them in the project. */
export function applyKeyToTune(root: number, minor: boolean, targetFxId?: Id | 'all') {
  const scale = minor ? 2 : 1;
  const ops: Op[] = [];
  for (const ins of S.project.inserts) {
    for (const f of ins.fx) {
      if (f.type !== 'tune') continue;
      if (targetFxId != null && targetFxId !== 'all' && f.id !== targetFxId) continue;
      ops.push(fxOp({ ...plain(f), params: { ...plain(f.params), key: root, scale } }, ins.id));
    }
  }
  if (!ops.length) return;
  commit(ops, t('d.act.op.pckey'));
  const names = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
  say(t('d.act.pckey', { key: names[root % 12], mode: t(minor ? 'd.act.minor' : 'd.act.major') }) + (ops.length > 1 ? t('d.act.inst', { n: ops.length }) : ''));
}

// ---------- view ----------
/** Playlist zoom limits, px per beat (at the most, a 16th is 256 px: detailed drum editing, as in FL Studio). */
export const BW_MIN = 4, BW_MAX = 1024;
export function zoomAt(f: number) { setU('bw', clamp(U.bw * f, BW_MIN, BW_MAX)); }
export function setSnap(v: number) {
  setU('snap', v);
  say(v < 0 ? t('d.act.snapauto', { name: snapName(snapRes(-1, U.bw)) }) : t('d.act.snap', { name: snapName(v) }));
}

// ---------- auto vocal ----------
let autoVocalBusy = false;
/** Analyse an audio clip and put the calibrated vocal chain on its track's insert (one undo step).
 *  Effects of the types the chain brings are replaced; others (e.g. an EQ) stay in front. */
export async function autoVocal(clipId: Id, style: 'natural' | 'hard' = 'natural') {
  const c = S.project.clips.find(x => x.id === clipId);
  if (!c || c.kind !== 'audio' || !S.catalog || autoVocalBusy) return;
  const tr = S.project.tracks[c.track]; if (!tr) return;
  const ins = S.project.inserts.find(m => m.id === tr.insert);
  if (!ins || ins.id === 0) { say(t('d.act.av.assign')); return; }
  autoVocalBusy = true;
  say(t('d.act.av.analyze'));
  try {
    // the rider (≈1.6 s look-ahead) is baked into the audio instead of living on the insert,
    // so playback and monitoring get no extra delay; a clip that was baked before is not baked twice
    const src = S.project.sources.find(x => x.id === c.ref);
    const baked = !!src && / \(rider\)$/.test(src.name);
    const plan = await be().autoVocalPlan({ clipId, style, bakeRider: true });
    if (plan.riderBake && !baked) {
      say(t('d.act.av.level'));
      await enqueue(() => be().bakeRider({ clipId, params: plan.riderBake!.params }));
      await resync();
    }
    // The chain leaves the vocal far under the limiter; bring it up to mix level in the limiter
    // (its gain + −1 dBTP ceiling): to sit under the beat when the plan knows it, else −18 LUFS.
    const makeup = Math.round(clamp(plan.trackGainDb ?? (-18 - plan.stages.lufsAfter), 0, 24) * 10) / 10;
    const types = new Set<string>(plan.chain.map(p => p.type));
    types.add('rider'); // an old live rider would ride the vocal a second time
    const ops: Op[] = ins.fx.filter(f => types.has(f.type)).map(f => ({ op: 'removeFx', fxId: f.id }) as Op);
    for (const p of plan.chain) {
      const f = newFx(S.catalog, allocId(), p.type, S.project.bpm);
      f.params = { ...f.params, ...p.params };
      f.mix = p.mix;
      f.preset = null; // calibrated values, not a preset (the header then reads "Własne ustawienia")
      f.bypass = !p.enabled;
      if (p.type === 'limit') f.params.gain = clamp((f.params.gain ?? 0) + makeup, 0, 24);
      ops.push({ op: 'putFx', insertId: ins.id, fx: f });
    }
    commit(ops, t('d.act.op.av'));
    setU({ mixSel: ins.id });
    const a = plan.analysis, st = plan.stages;
    say(t(plan.riderBake ? 'd.act.av.done.rb' : 'd.act.av.done', { n: plan.chain.filter(p => p.enabled).length, name: insName(ins) })
      + t('d.act.av.spread', { a: st.iqr400Db[0].toFixed(1), b: st.iqr400Db[1].toFixed(1) })
      + (a.key ? t('d.act.av.key', { key: a.key.name }) : t('d.act.av.nokey'))
      + (plan.warnings.length ? ' · ' + plan.warnings[0] : ''));
  } catch (e) { say(t('d.act.av.fail', { err: String(e) })); }
  autoVocalBusy = false;
}

/** Auto vocal on a comp: flatten it to one audio clip first (Ctrl+Z undoes both steps). */
export async function autoVocalComp(clipId: Id, style: 'natural' | 'hard' = 'natural') {
  const c = S.project.clips.find(x => x.id === clipId); if (!c || c.kind !== 'comp') return;
  const { track, start } = c;
  say(t('d.act.av.flatten'));
  try { await enqueue(() => be().flattenComp(clipId)); } catch (e) { say(t('d.act.av.flatfail', { err: String(e) })); return; }
  const a = S.project.clips.filter(x => x.kind === 'audio' && x.track === track && Math.abs(x.start - start) < 1e-6)[0]
    ?? S.project.clips.filter(x => x.kind === 'audio' && x.track === track).sort((p, q) => Math.abs(p.start - start) - Math.abs(q.start - start))[0];
  if (!a) { say(t('d.act.av.noflat')); return; }
  await autoVocal(a.id, style);
}
