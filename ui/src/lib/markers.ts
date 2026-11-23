// Playlist time markers (FL Studio's time markers, Ableton's locators): pure logic shared by the
// Playlist, the store's actions and the mock backend. Mirrors crates/engine/src/model.rs
// (`Marker`, `Project::marker_section`) and ops.rs (`putMarker`, `removeMarker`,
// `validate_marker`); the error strings are the engine's (ops.json pins them).
import { TOTAL_BEATS, type Id, type Marker, type Project } from '../api/types';
import { songEnd } from './export';

export const MARKERS_MAX = 256;
export const MARKER_NAME_MAX = 80;
const BAR = 4;

const byPos = (a: Marker, b: Marker) => a.at - b.at || a.id - b.id;

/** The engine's `putMarker`: insert or replace by id, keep the list sorted, bump `nextId`. */
export function putMarker(p: Project, m: Marker): void {
  if (!(Number.isFinite(m.at) && m.at >= 0 && m.at <= TOTAL_BEATS)) throw new Error(`marker ${m.id}: position must be 0..${TOTAL_BEATS} beats`);
  if ([...m.name].length > MARKER_NAME_MAX) throw new Error(`marker ${m.id}: name is longer than ${MARKER_NAME_MAX} characters`);
  const list = [...(p.markers ?? [])];
  const i = list.findIndex(x => x.id === m.id);
  if (i >= 0) list[i] = { id: m.id, at: m.at, name: m.name };
  else if (list.length >= MARKERS_MAX) throw new Error(`a project has at most ${MARKERS_MAX} markers`);
  else list.push({ id: m.id, at: m.at, name: m.name });
  list.sort(byPos);
  p.markers = list;
  if (m.id >= p.nextId) p.nextId = m.id + 1;
}

/** The engine's `removeMarker` (the field goes when the last one does: the engine omits an empty list). */
export function removeMarker(p: Project, id: Id): void {
  const list = p.markers ?? [];
  if (!list.some(m => m.id === id)) throw new Error(`no marker with id ${id}`);
  const rest = list.filter(m => m.id !== id);
  if (rest.length) p.markers = rest; else delete p.markers;
}

/**
 * The section a marker starts (`Project::marker_section`): up to the next marker further on, or to
 * the song end when it is the last one (at least a bar). Null for an unknown marker.
 */
export function markerSection(p: Pick<Project, 'markers' | 'clips'>, id: Id): { a: number; b: number } | null {
  const list = p.markers ?? [];
  const m = list.find(x => x.id === id); if (!m) return null;
  let next = Infinity;
  for (const x of list) if (x.at > m.at + 1e-9) next = Math.min(next, x.at);
  const b = Number.isFinite(next) ? next : Math.max(songEnd(p), m.at + BAR);
  return { a: m.at, b: Math.max(Math.min(b, TOTAL_BEATS), m.at + 1 / 64) };
}

/**
 * The marker to jump to from `beat`: the first one after it (`dir` 1) or the last one before it
 * (−1). `grace` (beats) skips a marker just behind the position, so "previous" while playing goes
 * past the marker the playhead has only just crossed (REAPER and FL behave the same).
 */
export function jumpTarget(markers: readonly Marker[] | undefined, beat: number, dir: 1 | -1, grace = 0): Marker | null {
  const list = markers ?? [];
  if (dir > 0) return list.find(m => m.at > beat + 1e-6) ?? null;
  for (let i = list.length - 1; i >= 0; i--) if (list[i].at < beat - Math.max(grace, 1e-6)) return list[i];
  return null;
}

/** "Marker 3": the first `${base} n` no marker is called yet. */
export function nextMarkerName(markers: readonly Marker[] | undefined, base: string): string {
  const used = new Set((markers ?? []).map(m => m.name));
  for (let n = (markers?.length ?? 0) + 1; ; n++) if (!used.has(`${base} ${n}`)) return `${base} ${n}`;
}

/** `text` cut with an ellipsis to fit `maxW` px by `measure`; '' when not even "x…" fits. */
export function fitText(text: string, maxW: number, measure: (s: string) => number): string {
  if (measure(text) <= maxW) return text;
  const chars = [...text];
  let lo = 0, hi = chars.length - 1;
  // the longest prefix that fits with the ellipsis
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (measure(chars.slice(0, mid).join('') + '…') <= maxW) lo = mid; else hi = mid - 1;
  }
  return lo ? chars.slice(0, lo).join('').trimEnd() + '…' : '';
}

/** Px a marker's label may take before the next marker (or its own `cap`). */
export const LABEL_PAD = 4, LABEL_MIN = 14, LABEL_CAP = 220;

export interface MarkerBox { id: Id; x: number; x1: number; text: string }

/**
 * Where each marker and its label go at `bw` px per beat: x of the flag, `x1` the end of what can
 * be clicked, `text` the name as it fits. A label never runs into the next marker; when markers
 * crowd (zoomed out), names shorten and then only the flags stay. Two markers at one place: the
 * later one in the list carries the label.
 */
export function layoutMarkers(markers: readonly Marker[] | undefined, bw: number, measure: (s: string) => number): MarkerBox[] {
  const list = markers ?? [], out: MarkerBox[] = [];
  for (let i = 0; i < list.length; i++) {
    const m = list[i], x = m.at * bw;
    const nx = i + 1 < list.length ? list[i + 1].at * bw : Infinity;
    const room = Math.min(nx - x, LABEL_CAP) - LABEL_PAD * 2;
    const name = m.name.trim();
    const text = name && room >= LABEL_MIN ? fitText(name, room, measure) : '';
    const w = text ? measure(text) + LABEL_PAD * 2 : 0;
    out.push({ id: m.id, x, x1: x + Math.max(w, 6), text });
  }
  return out;
}

/** The marker under content x (px): its flag (±5 px) or its label; the one drawn last wins. */
export function markerAtX(boxes: readonly MarkerBox[], x: number): Id | null {
  for (let i = boxes.length - 1; i >= 0; i--) {
    const b = boxes[i];
    if (x >= b.x - 5 && x <= Math.max(b.x1, b.x + 5)) return b.id;
  }
  return null;
}
