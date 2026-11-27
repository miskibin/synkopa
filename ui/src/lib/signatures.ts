// The signature map: where the bars fall under mid-song meter changes (api SigPoint[], the
// engine's SigMap in crates/engine/src/model.rs). Beats stay the timeline's axis — a bar is
// `4·num/den` beats (7/8 is 3.5), a change may sit mid-bar and the partial bar before it
// stays a fragment that counts, and bars before beat 0 (a count-in) continue backwards on
// the opening grid. Pure logic only, like lib/tempoLane.ts: the conversions, the bar lines
// a view draws between two beats, and the point edits the ruler dispatches as
// `setSigPoints` ops.
import { MAX_SIG_NUM, MAX_SIG_POINTS, SIG_DENS, TOTAL_BEATS, type SigPoint } from '../api/types';
export type { SigPoint };

/** Two points closer than this are one change, the last governing (engine `SIG_EPS`): a
 *  sliver of a fragment would otherwise count as a whole bar. */
const SIG_EPS = 1e-6;
/** Rounding slack (beats) when flooring a beat to its bar (engine `BEAT_ROUNDING`): `0.35 - 0.1`
 *  is a hair under one 0.25 bar. Far below the 1e-9 probes callers use for "just before". */
const BEAT_ROUNDING = 1e-11;

/** The constant 4/4 map, and the opener every compile starts from. */
export const FOUR_FOUR: SigPoint = { beat: 0, num: 4, den: 4 };

/** The points as the engine compiles them: `num` clamped 1..=32, `den` snapped to the
 *  1/2/4/8/16 ladder (anything else reads as 4 — a bar then lands on whole steps again),
 *  beat-sorted and capped, two points at one beat playing the last. The op path normalizes
 *  too, but a project FILE loads raw (nothing truncates `sig_points` on load), so the
 *  ruler, the grid and the snap read from this bounded list. */
export function compiledSig(points: readonly SigPoint[] | undefined): SigPoint[] {
  const out: SigPoint[] = [];
  for (const p of [...(points ?? [])]
    .map(p => ({
      beat: Math.min(Math.max(p.beat, 0), TOTAL_BEATS),
      num: Math.min(Math.max(Math.round(p.num), 1), MAX_SIG_NUM),
      den: (SIG_DENS as readonly number[]).includes(p.den) ? p.den : 4,
    }))
    .sort((a, b) => a.beat - b.beat)
    .slice(0, MAX_SIG_POINTS)) {
    const last = out[out.length - 1];
    if (last && p.beat - last.beat < SIG_EPS) out[out.length - 1] = { ...p, beat: last.beat }; // one change, at the first beat
    else out.push(p);
  }
  return out;
}

/** The bar length in beats from `p` on. */
export const sigBarBeats = (p: SigPoint): number => (4 * p.num) / p.den;

/** The map as it PLAYS: a 4/4 opener in front when no point sits at beat 0 (the mirror of
 *  the engine's `SigMap::compile` opener — the stored list has none, 4/4 is the default). */
function withOpener(map: readonly SigPoint[]): readonly SigPoint[] {
  return map.length === 0 || map[0].beat > 1e-9 ? [FOUR_FOUR, ...map] : map;
}

/** The governing point at `beat` (the opener before the first point, the last of ties). */
export function sigAt(map: readonly SigPoint[], beat: number): SigPoint {
  const list = withOpener(map);
  let i = 0;
  for (let k = 0; k < list.length; k++) if (list[k].beat <= beat + 1e-9) i = k;
  return list[i];
}

/** The bar length in beats at `beat`. */
export const barBeatsAt = (map: readonly SigPoint[], beat: number): number => sigBarBeats(sigAt(map, beat));

interface Seg { beat: number; bar: number; first: number }

/** The map's segments with each one's first bar index (engine `SigMap::compile`): every
 *  segment but the last holds at least one bar, a fragment included, so bar indices map to
 *  strictly increasing beats and a walk over bars always advances. */
function segments(map: readonly SigPoint[]): Seg[] {
  const segs: Seg[] = [];
  for (const p of withOpener(map)) {
    const last = segs[segs.length - 1];
    if (last && p.beat - last.beat < SIG_EPS) { last.bar = sigBarBeats(p); continue; }
    const first = last ? last.first + Math.max(1, Math.ceil((p.beat - last.beat - 1e-9) / last.bar)) : 0;
    segs.push({ beat: last ? p.beat : 0, bar: sigBarBeats(p), first });
  }
  return segs;
}

/** The last index `i` with `pred(list[i])` true (the list partitioned true-then-false). */
function lastWhere<T>(list: readonly T[], pred: (x: T) => boolean): number {
  let lo = 0, hi = list.length;
  while (lo < hi) { const m = (lo + hi) >> 1; if (pred(list[m])) lo = m + 1; else hi = m; }
  return Math.max(0, lo - 1);
}

/** The 0-based bar `beat` falls in (bar 0 starts at beat 0); negative beats continue
 *  backwards on the opening grid. A fragment (the truncated bar before a change) counts. */
export function barAt(map: readonly SigPoint[], beat: number): number {
  const segs = segments(map);
  if (beat < 0) return Math.floor((beat + BEAT_ROUNDING) / segs[0].bar);
  const i = lastWhere(segs, s => s.beat <= beat + 1e-9);
  const s = segs[i];
  let k = Math.max(0, Math.floor((beat - s.beat + BEAT_ROUNDING) / s.bar));
  if (i + 1 < segs.length) k = Math.min(k, segs[i + 1].first - s.first - 1);
  return s.first + k;
}

/** The beat where 0-based bar `bar` starts (`bar` a whole number; the inverse of `barAt`).
 *  Beyond the song the grid continues at the last bar length — callers clamp. Strictly
 *  increasing in `bar`. */
export function beatAtBar(map: readonly SigPoint[], bar: number): number {
  const segs = segments(map);
  if (bar < 0) return bar * segs[0].bar;
  const s = segs[lastWhere(segs, x => x.first <= bar)];
  return s.beat + (bar - s.first) * s.bar;
}

/** Whether `beat` sits on a bar boundary (a metronome accent lands there). Off-grid
 *  changes make boundaries land between beats (7/8's second bar starts at beat 3.5). */
export const isBarStart = (map: readonly SigPoint[], beat: number): boolean =>
  Math.abs(beatAtBar(map, barAt(map, beat)) - beat) < 1e-6;

/** The bar lines of `[b0, b1)`: `{ beat, bar }` for every boundary — where the ruler and
 *  the grid draw their lines, however unevenly they fall. The boundary at or before `b0`
 *  is included (a line just left of the view still carries its number, as a constant 4/4
 *  ruler always drew it). `step` decimates (every `step`-th bar) at zooms too dense for
 *  one line per bar. */
export function barLines(map: readonly SigPoint[], b0: number, b1: number, step = 1): { beat: number; bar: number }[] {
  const out: { beat: number; bar: number }[] = [];
  let bar = Math.floor(barAt(map, b0) + 1e-9) || 0;
  // bar starts strictly increase (no clamp): the walk ends at `b1`
  for (; ; bar += Math.max(1, step)) {
    const beat = beatAtBar(map, bar);
    if (!(beat < b1 - 1e-9)) break;
    out.push({ beat, bar });
  }
  return out;
}

/** Snaps `beat` to the nearest bar line of the map (the timeline's "Snap: bar"). */
export function snapToBar(map: readonly SigPoint[], beat: number): number {
  const bar = barAt(map, beat);
  const a = beatAtBar(map, bar);
  const b = beatAtBar(map, bar + 1);
  return beat - a <= b - beat ? a : b;
}

/** "3/4", "7/8": a point as it reads. */
export const sigText = (p: SigPoint): string => `${p.num}/${p.den}`;

// The ruler asks for the compiled map every frame. Compiling per frame would sort the
// whole list each time, and a cache keyed on the points ARRAY is wrong: the store's state
// merges (`mergeInto` in state/store.ts) update an id-less list POSITIONALLY IN PLACE —
// after an undo the array is the same object with restored content, and an identity cache
// would draw the old grid forever. Memoize reactively at the call site instead
// (`createMemo(() => compiledSig(S.project.sigPoints))`): Solid tracks the elements'
// leaves, so an in-place merge recomputes, and unchanged frames reuse the map.

const clampN = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** A change added at `beat` with meter `num/den`, after any point at the same beat (the
 *  last one at a beat is the one played, so the new one is live while it is dragged).
 *  Returns the points and the new index. */
export function addSigPoint(points: readonly SigPoint[], beat: number, num: number, den: number): { points: SigPoint[]; i: number } {
  const q: SigPoint = { beat: clampN(beat, 0, TOTAL_BEATS), num: clampN(Math.round(num), 1, MAX_SIG_NUM), den: (SIG_DENS as readonly number[]).includes(den) ? den : 4 };
  let i = 0;
  while (i < points.length && points[i].beat <= q.beat) i++;
  const out = [...points.map(p => ({ ...p }))];
  out.splice(i, 0, q);
  return { points: out, i };
}

/** Point `i` moved to `beat`: between its neighbours (points never pass each other) and
 *  clamped to the song. Tied neighbours leave an empty interval — the beat then stays put
 *  (a nudge past the twin would reshuffle the store's sort). */
export function moveSigPoint(points: readonly SigPoint[], i: number, beat: number): SigPoint[] {
  const p = points[i];
  if (!p) return points.map(q => ({ ...q }));
  const lo = i > 0 ? points[i - 1].beat + 1e-6 : 0;
  const hi = i + 1 < points.length ? points[i + 1].beat - 1e-6 : TOTAL_BEATS;
  // a tie narrows the interval past the point's own beat: it may only move away from the twin
  const nb = clampN(beat, Math.min(lo, p.beat), Math.max(hi, p.beat));
  return points.map((q, k) => (k === i ? { ...q, beat: nb } : { ...q }));
}

export function deleteSigPoint(points: readonly SigPoint[], i: number): SigPoint[] {
  return points.filter((_, k) => k !== i).map(q => ({ ...q }));
}

export function setSigMeter(points: readonly SigPoint[], i: number, num: number, den: number): SigPoint[] {
  return points.map((q, k) =>
    k === i ? { ...q, num: clampN(Math.round(num), 1, MAX_SIG_NUM), den: (SIG_DENS as readonly number[]).includes(den) ? den : 4 } : { ...q });
}

/** Parses "7/8" (a meter as it reads): `null` when it is not one. */
export function parseSig(s: string): { num: number; den: number } | null {
  const m = s.trim().match(/^(\d{1,2})\s*\/\s*(\d{1,2})$/);
  if (!m) return null;
  const num = Number(m[1]), den = Number(m[2]);
  if (!(num >= 1 && num <= MAX_SIG_NUM) || !(SIG_DENS as readonly number[]).includes(den)) return null;
  return { num, den };
}
