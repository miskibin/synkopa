// Playlist time markers: the edits and jumps every entry point shares (the marker strip, the
// ruler's menu, the Edit menu, the command palette, Alt+T and the jump keys). Each edit is one
// undo step through `commit` / `Gesture` (ops `putMarker` / `removeMarker`).
import type { Id, Marker } from '../api/types';
import { t } from '../lib/i18n';
import { fmtPos } from '../lib/format';
import { jumpTarget, markerSection, MARKERS_MAX, MARKER_NAME_MAX, nextMarkerName } from '../lib/markers';
import { snapRes, snapRound } from '../lib/snap';
import { TOTAL } from '../lib/clipOps';
import * as A from './actions';
import { commit } from './edit';
import { allocId, plain, S } from './store';
import { curBeat, tele } from './telemetry';
import { openRename, say, U } from './ui';

export const markers = (): readonly Marker[] => S.project.markers ?? [];
const byId = (id: Id) => markers().find(m => m.id === id);

/** Where the Playlist shows a marker on screen (client px), for the rename box; null when it is not mounted. */
let anchorOf: ((id: Id) => { x: number; y: number } | null) | null = null;
export const setMarkerAnchor = (f: typeof anchorOf) => { anchorOf = f; };

/** The song position a new marker goes to: the playhead while playing (on the grid), else the start position. */
export function songPosition(): number {
  if (!tele.frame.playing) return U.start;
  const u = snapRes(U.snap, U.bw); // not `snapUnit()`: Alt is held for Alt+T, which would mean "free"
  return Math.max(0, snapRound(curBeat(), u));
}

/** Adds a marker at `beat` and opens its name for typing (FL's "Add marker…"). */
export function addMarker(beat = songPosition(), rename = true): Id | null {
  if (markers().length >= MARKERS_MAX) { say(t('a.mk.full', { n: MARKERS_MAX })); return null; }
  const m: Marker = { id: allocId(), at: Math.min(Math.max(0, beat), TOTAL), name: nextMarkerName(markers(), t('a.mk.marker')) };
  commit([{ op: 'putMarker', marker: m }], t('a.mk.undoAdd'));
  say(t('a.mk.added', { name: m.name, pos: fmtPos(m.at) }));
  if (rename) queueMicrotask(() => renameMarker(m.id));
  return m.id;
}

/** Opens the rename box under the marker; an unchanged or too long name changes nothing. */
export function renameMarker(id: Id) {
  const m = byId(id); if (!m) return;
  const at = anchorOf?.(id) ?? { x: window.innerWidth / 2 - 120, y: 120 };
  openRename(at.x, at.y, m.name, v => setMarkerName(id, v), t('a.mk.renameTitle'));
}

export function setMarkerName(id: Id, name: string) {
  const m = byId(id); if (!m) return;
  const v = [...name.trim()].slice(0, MARKER_NAME_MAX).join('');
  if (v === m.name) return;
  commit([{ op: 'putMarker', marker: { ...plain(m), name: v } }], t('a.mk.undoRename'));
}

export function deleteMarker(id: Id) {
  const m = byId(id); if (!m) return;
  commit([{ op: 'removeMarker', id }], t('a.mk.undoDelete'));
  say(t('a.mk.deleted', { name: m.name || t('a.mk.unnamed') }));
}

/** Loops the section the marker starts (to the next marker, or the song end) and puts the start position there. */
export function loopSection(id: Id) {
  const r = markerSection(S.project, id), m = byId(id); if (!r || !m) return;
  A.setLoop(r, t('a.mk.undoLoop'));
  A.seekSong(r.a);
  say(t('a.mk.looped', { name: m.name || t('a.mk.unnamed'), a: fmtPos(r.a), b: fmtPos(r.b) }));
}

/** Moves the playhead (or the start position) to a marker. */
export function jumpTo(id: Id) {
  const m = byId(id); if (!m) return;
  A.seekSong(m.at);
  say(t('a.mk.jumped', { name: m.name || t('a.mk.unnamed'), pos: fmtPos(m.at) }));
}

/** Jump to the next (1) or previous (−1) marker from where the song is. */
export function jump(dir: 1 | -1) {
  if (!markers().length) { say(t('a.mk.none')); return; }
  const from = curBeat();
  // while playing, "previous" skips the marker just crossed (else it could never get past it)
  const m = jumpTarget(markers(), from, dir, tele.frame.playing ? 1 : 0);
  if (!m) { say(t(dir > 0 ? 'a.mk.noNext' : 'a.mk.noPrev', { pos: fmtPos(Math.max(0, from)) })); return; }
  jumpTo(m.id);
}
