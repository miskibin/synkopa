// The playlist's tempo lane: the song's tempo map (api TempoPoint[], the engine's compiled map in
// crates/engine/src/engine/tempo.rs) as an editable strip above the ruler. Pure logic only, like
// lib/envEdit.ts: where a point is on screen, the BPM the map plays at a beat, the segments the
// strip draws, and the point edits the lane dispatches as `setTempoPoints` ops. A ramp glides
// linearly in beats from a point to the next one, so the drawn line is straight; the flag belongs
// to the segment's START point.
import { MAX_TEMPO_BPM, MAX_TEMPO_POINTS, MIN_TEMPO_BPM, TOTAL_BEATS, type TempoPoint } from '../api/types';

/** Strip height (px), like MARKER_H for the marker strip above it. */
export const TEMPO_H = 20;
/** Pointer radius (px) that picks a point. */
export const TEMPO_POINT_R = 9;
/** The value axis never shows less than this many BPM, so a flat map still has room to drag. */
const MIN_SPAN = 60;
const PAD = 3;

/**
 * The BPM the map plays at `beat`: the base tempo before the first point, then the point's value
 * — gliding to the next point when it carries `ramp` (the engine's `bpm_at`; a duplicate beat
 * plays the last of them). Beyond the map it holds the last value.
 */
export function bpmAt(points: readonly TempoPoint[] | undefined, base: number, beat: number): number {
  const list = points ?? [];
  let i = -1;
  for (let k = 0; k < list.length; k++) if (list[k].beat <= beat + 1e-9) i = k;
  if (i < 0) return base;
  const p = list[i], n = list[i + 1];
  if (!n || !p.ramp) return p.bpm;
  const d = n.beat - p.beat;
  if (d <= 1e-9) return p.bpm;
  return p.bpm + (n.bpm - p.bpm) * Math.min(1, Math.max(0, (beat - p.beat) / d));
}

/** The points as the engine compiles them: sorted, and two at one beat play the last of them. */
export function dedupeTempo(points: readonly TempoPoint[] | undefined): TempoPoint[] {
  const out: TempoPoint[] = [];
  for (const p of [...(points ?? [])].sort((a, b) => a.beat - b.beat)) {
    const last = out[out.length - 1];
    if (last && p.beat - last.beat < 1e-9) out[out.length - 1] = p;
    else out.push(p);
  }
  return out;
}

/**
 * The map as the engine PLAYS it: clamped, sorted (stable — the stored order of ties is kept),
 * capped at MAX_TEMPO_POINTS — the mirror of `TempoMap::compile`, duplicates INCLUDED (a ramp
 * glides toward the next stored point, and two at one beat play the last). The op path
 * normalizes too, but a project FILE loads raw (nothing truncates `tempo_points` on load), so
 * the lane draws, hits and reads from this bounded list: everything per-frame here costs at
 * most what plays.
 */
export function compiledTempo(points: readonly TempoPoint[] | undefined): TempoPoint[] {
  return (points ?? []).map(p => ({
    beat: Math.min(Math.max(p.beat, 0), TOTAL_BEATS),
    bpm: Math.min(Math.max(p.bpm, MIN_TEMPO_BPM), MAX_TEMPO_BPM),
    ramp: p.ramp ?? false,
  })).sort((a, b) => a.beat - b.beat).slice(0, MAX_TEMPO_POINTS);
}

/** The compiled map, kept per points array (ops replace the array, never mutate it): the
 *  toolbar's readout asks for the playhead BPM every frame, and compiling per frame would
 *  sort the whole map each time. */
const playheadCache = new WeakMap<object, TempoPoint[]>();

/** The BPM the toolbar's readout shows at `beat`: what the engine plays there (`bpmAt` over
 *  the compiled map). Callers pass the playhead's beat; the base before the first point. */
export function playheadBpm(points: readonly TempoPoint[] | undefined, base: number, beat: number): number {
  let map: TempoPoint[];
  if (points && playheadCache.has(points)) map = playheadCache.get(points)!;
  else {
    map = compiledTempo(points);
    if (points) playheadCache.set(points, map);
  }
  return bpmAt(map, base, beat);
}

export interface TempoSeg { a: number; b: number; va: number; vb: number }

/** The curve as straight pieces over `[0, end]`: flats and ramps, what the strip draws. Walks
 *  the STORED order (not the deduped one): a ramp glides toward the next stored point — what
 *  `bpmAt` and the engine play — and a duplicate's own piece is zero-length and draws nothing. */
export function tempoSegments(points: readonly TempoPoint[] | undefined, base: number, end: number): TempoSeg[] {
  const list = points ?? [], out: TempoSeg[] = [];
  let pos = 0, v = base;
  for (let i = 0; i < list.length; i++) {
    const p = list[i], n = list[i + 1];
    if (p.beat >= end - 1e-9) break;
    if (p.beat > pos + 1e-9) { out.push({ a: pos, b: p.beat, va: v, vb: v }); pos = p.beat; }
    const to = Math.min(n ? n.beat : end, end);
    // a ramp cut short by the view's end still shows the value it REACHES there (the glide
    // continues past the edge, it does not bend to hit its target early)
    const vb = n && p.ramp ? p.bpm + (n.bpm - p.bpm) * ((to - p.beat) / (n.beat - p.beat)) : p.bpm;
    if (to > pos + 1e-9) out.push({ a: pos, b: to, va: p.bpm, vb });
    pos = to;
    v = vb;
    if (!n || pos >= end - 1e-9) break;
  }
  if (pos < end - 1e-9) out.push({ a: pos, b: end, va: v, vb: v });
  return out;
}

/** The value axis the strip maps to its height: the played values ± 10, at least MIN_SPAN wide,
 *  clamped to what a tempo may be (a point 20..300, the base up to its own knob maximum). */
export function tempoScale(points: readonly TempoPoint[] | undefined, base: number): { lo: number; hi: number } {
  let lo = base, hi = base;
  for (const p of points ?? []) { lo = Math.min(lo, p.bpm); hi = Math.max(hi, p.bpm); }
  lo -= 10;
  hi += 10;
  const cap = Math.max(MAX_TEMPO_BPM, base);
  lo = Math.max(MIN_TEMPO_BPM, lo);
  hi = Math.min(cap, hi);
  if (hi - lo < MIN_SPAN) {
    const mid = (lo + hi) / 2;
    lo = Math.max(MIN_TEMPO_BPM, mid - MIN_SPAN / 2);
    hi = Math.min(cap, Math.max(mid + MIN_SPAN / 2, lo + MIN_SPAN));
  }
  return { lo, hi };
}

export interface TempoGeom {
  /** Beat ↔ strip x (content px), BPM ↔ strip y, the value axis as drawn. */
  X: (beat: number) => number; T: (x: number) => number;
  Y: (bpm: number) => number; V: (y: number) => number;
  lo: number; hi: number;
}

export function tempoGeom(bw: number, H: number, lo: number, hi: number): TempoGeom {
  const inner = Math.max(1, H - 2 * PAD);
  const span = Math.max(hi - lo, 1e-6);
  return {
    X: b => b * bw,
    T: x => x / bw,
    Y: v => PAD + (1 - Math.min(1, Math.max(0, (v - lo) / span))) * inner,
    V: y => lo + (1 - Math.min(1, Math.max(0, (y - PAD) / inner))) * span,
    lo, hi,
  };
}

/** The point under strip (x, y), nearest within [`TEMPO_POINT_R`], or null. */
export function hitTempo(points: readonly TempoPoint[] | undefined, G: TempoGeom, x: number, y: number): number | null {
  let best = -1, bd = TEMPO_POINT_R;
  (points ?? []).forEach((p, i) => {
    const d = Math.hypot(G.X(p.beat) - x, G.Y(p.bpm) - y);
    if (d <= bd) { bd = d; best = i; }
  });
  return best >= 0 ? best : null;
}

/** "128" or "128.5": a BPM as short as the strip can draw it. */
export function bpmLabel(v: number): string {
  return Number.isInteger(v) ? String(v) : v.toFixed(1);
}

const clampN = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * A point added at `beat` with value `bpm`, after any point at the same beat (the last one at a
 * beat is the one played, so the new one is live while it is being dragged). Returns the points
 * and the new index.
 */
export function addTempoPoint(points: readonly TempoPoint[], beat: number, bpm: number, ramp = false): { points: TempoPoint[]; i: number } {
  const q: TempoPoint = { beat: clampN(beat, 0, TOTAL_BEATS), bpm: clampN(bpm, MIN_TEMPO_BPM, MAX_TEMPO_BPM), ramp };
  let i = 0;
  while (i < points.length && points[i].beat <= q.beat) i++;
  const out = [...points.map(p => ({ ...p }))];
  out.splice(i, 0, q);
  return { points: out, i };
}

/** Point `i` moved to `beat`/`bpm`: between its neighbours (points never pass each other) and
 *  clamped to the song and the tempo range. Tied neighbours leave an empty interval — the beat
 *  then stays put (a nudge past the twin would reshuffle the store's sort), only the BPM moves. */
export function moveTempoPoint(points: readonly TempoPoint[], i: number, beat: number, bpm: number): TempoPoint[] {
  const p = points[i];
  if (!p) return points.map(q => ({ ...q }));
  const lo = i > 0 ? points[i - 1].beat + 1e-6 : 0;
  const hi = i + 1 < points.length ? points[i + 1].beat - 1e-6 : TOTAL_BEATS;
  const nb = lo > hi ? p.beat : clampN(beat, lo, hi);
  return points.map((q, k) => (k === i
    ? { ...q, beat: nb, bpm: clampN(bpm, MIN_TEMPO_BPM, MAX_TEMPO_BPM) }
    : { ...q }));
}

export function deleteTempoPoint(points: readonly TempoPoint[], i: number): TempoPoint[] {
  return points.filter((_, k) => k !== i).map(q => ({ ...q }));
}

export function setTempoBpm(points: readonly TempoPoint[], i: number, bpm: number): TempoPoint[] {
  return points.map((q, k) => (k === i ? { ...q, bpm: clampN(bpm, MIN_TEMPO_BPM, MAX_TEMPO_BPM) } : { ...q }));
}

export function setTempoRamp(points: readonly TempoPoint[], i: number, ramp: boolean): TempoPoint[] {
  return points.map((q, k) => (k === i ? { ...q, ramp } : { ...q }));
}
