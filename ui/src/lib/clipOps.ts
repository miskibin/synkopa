// Pure builders for playlist edits. They return new clips / ops; nothing here touches the store.
import { STRETCH_MAX, STRETCH_MIN, TOTAL_BEATS, VOL_UNITY, WARP_MAX, WARP_SPACING, type Clip, type Id, type Op, type Project, type Track } from '../api/types';
import { clamp } from './format';

export const TOTAL = TOTAL_BEATS;

/** A new playlist clip of `kind` with the house defaults; audio callers spread `sharedSound` over it. */
export function newClip(id: Id, track: number, start: number, len: number, kind: Clip['kind'], ref: Id): Clip {
  return { id, track, start, len, offset: 0, kind, ref, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false };
}

/** Clamp a group move so every clip stays on the timeline and on an existing track. */
export function clampMove(orig: Clip[], d: number, dt: number, nTracks: number): [number, number] {
  if (!orig.length) return [0, 0];
  let minS = Infinity, maxE = -Infinity, minT = Infinity, maxT = -Infinity;
  for (const c of orig) {
    minS = Math.min(minS, c.start); maxE = Math.max(maxE, c.start + c.len);
    minT = Math.min(minT, c.track); maxT = Math.max(maxT, c.track);
  }
  return [clamp(d, -minS, TOTAL - maxE), clamp(dt, -minT, nTracks - 1 - maxT)];
}

export function moveClips(orig: Clip[], d: number, dt: number): Clip[] {
  return orig.map(c => ({ ...c, start: c.start + d, track: c.track + dt }));
}

/** Place another instance of an existing clip, keeping the exact source region and its
 * clip-local settings (trim/offset, stretch, fades and pitch edits), not the full file. */
export function placeClipCopy(source: Clip, id: number, track: number, beat: number): Clip {
  const len = Math.min(source.len, TOTAL);
  return { ...source, id, track, start: clamp(beat, 0, TOTAL - len), len };
}

/** Split `c` at beat `b`; null when `b` is not strictly inside. */
export function sliceClip(c: Clip, b: number, newId: number): [Clip, Clip] | null {
  if (b <= c.start + 1e-6 || b >= c.start + c.len - 1e-6) return null;
  const a = { ...c, len: b - c.start, fadeOut: 0 };
  const z = { ...c, id: newId, start: b, len: c.start + c.len - b, offset: c.offset + (b - c.start), fadeIn: 0 };
  return [a, z];
}

// Warp markers (audio clips). The map lives on the clip in seconds on both sides — source
// `at`, stretched-audio `to` — so splits and trims never touch it, and only the stretch
// builders below rescale it (with the offset they already scale).

/** Whether a warp map (not the plain `stretch` slope) shapes this clip's audio. */
export function warpActive(c: Clip): boolean {
  return c.kind === 'audio' && !c.resample && !!c.warp?.length;
}

/** The stretched-audio second the source second `s` plays at (the mirror of `Clip::warped_to`). */
export function warpedTo(c: Clip, s: number, dur: number): number {
  let prev: [number, number] = [0, 0];
  for (const w of c.warp ?? []) {
    if (s <= w.at) return prev[1] + (s - prev[0]) * (w.to - prev[1]) / (w.at - prev[0]);
    prev = [w.at, w.to];
  }
  const end = dur * (c.stretch ?? 1);
  return prev[1] + (s - prev[0]) * (end - prev[1]) / (dur - prev[0]);
}

/** The source second played at stretched second `t` (the mirror of `Clip::warped_from`). */
export function warpedFrom(c: Clip, t: number, dur: number): number {
  let prev: [number, number] = [0, 0];
  for (const w of c.warp ?? []) {
    if (t <= w.to) return prev[0] + (t - prev[1]) * (w.at - prev[0]) / (w.to - prev[1]);
    prev = [w.at, w.to];
  }
  const end = dur * (c.stretch ?? 1);
  return prev[0] + (t - prev[1]) * (dur - prev[0]) / (end - prev[1]);
}

/** `c` with a marker pinned at source second `at`, landing where the current map puts it
 *  (so pinning never jumps the audio; dragging the pin then re-times around it). Null when
 *  the clip is full or the spot is taken (markers sit `WARP_SPACING` apart). */
export function pinWarp(c: Clip, at: number, dur: number): Clip | null {
  if ((c.warp?.length ?? 0) >= WARP_MAX) return null;
  if (c.warp?.some(w => Math.abs(w.at - at) < WARP_SPACING)) return null;
  const warp = [...(c.warp ?? []), { at, to: warpedTo(c, at, dur) }].sort((a, b) => a.at - b.at);
  return { ...c, warp };
}

/** `c` with marker `i` moved to land at stretched second `to` (its audio moment stays). */
export function moveWarp(c: Clip, i: number, to: number): Clip {
  const warp = (c.warp ?? []).map((w, k) => (k === i ? { ...w, to } : w));
  return { ...c, warp };
}

/** `c` without marker `i`. */
export function removeWarp(c: Clip, i: number): Clip {
  const warp = (c.warp ?? []).filter((_, k) => k !== i);
  return { ...c, warp };
}

/** Markers pinned at every onset position (`seconds` into the source), landing where the
 *  current map puts each; the clip keeps playing as it does until they are dragged. The
 *  existing markers are never touched: an onset too close to one (or to an accepted one)
 *  is skipped, so pinning can never move audio that was already pinned. */
export function markersAtTransients(c: Clip, seconds: readonly number[], dur: number): Clip | null {
  const warp = [...(c.warp ?? [])];
  for (const at of [...seconds].sort((a, b) => a - b)) {
    if (warp.length >= WARP_MAX) break;
    if (warp.some(w => Math.abs(w.at - at) < WARP_SPACING)) continue;
    warp.push({ at, to: warpedTo(c, at, dur) });
  }
  return { ...c, warp: warp.sort((a, b) => a.at - b.at) };
}

/** The stretch factors a marker map still allows: every segment slope (the markers' and the
 *  two implicit ends) scales with the whole-clip factor, and no segment may leave
 *  `STRETCH_MIN..STRETCH_MAX`. Null when there is no map (the baseline bounds apply). */
export function warpScaleBounds(c: Clip, dur: number): [number, number] | null {
  if (!c.warp?.length || !(dur > 0)) return null;
  let prev: [number, number] = [0, 0];
  const pts: [number, number][] = [...c.warp.map(w => [w.at, w.to] as [number, number]), [dur, dur * (c.stretch ?? 1)]];
  let kLo = 0, kHi = Infinity;
  for (const [at, to] of pts) {
    const slope = (to - prev[1]) / (at - prev[0]);
    kLo = Math.max(kLo, STRETCH_MIN / slope);
    kHi = Math.min(kHi, STRETCH_MAX / slope);
    prev = [at, to];
  }
  return [kLo, kHi];
}

/** The stretched-second bounds a drag may move marker `i` into: its neighbours' spacing and
 *  the ratio limits of both adjacent segments (including the implicit ends), so the map the
 *  drag submits is always one the engine accepts. */
export function warpDragBounds(c: Clip, i: number, dur: number): [number, number] {
  const ws = c.warp ?? [], s = c.stretch ?? 1;
  const at = ws[i]?.at ?? 0, to0 = ws[i]?.to ?? 0;
  const left: [number, number] = i > 0 ? [ws[i - 1].at, ws[i - 1].to] : [0, 0];
  const right: [number, number] = i + 1 < ws.length ? [ws[i + 1].at, ws[i + 1].to] : [dur, dur * s];
  let lo = left[1] + STRETCH_MIN * (at - left[0]);
  let hi = left[1] + STRETCH_MAX * (at - left[0]);
  lo = Math.max(lo, right[1] - STRETCH_MAX * (right[0] - at));
  hi = Math.min(hi, right[1] - STRETCH_MIN * (right[0] - at));
  lo = Math.max(lo, WARP_SPACING, (i > 0 ? ws[i - 1].to : 0) + WARP_SPACING);
  hi = Math.min(hi, right[1] - WARP_SPACING);
  return [Math.min(lo, to0), Math.max(hi, to0)];
}

/** Span to shift a duplicate by: the selection length rounded up to the snap unit. */
export function duplicateSpan(cs: Clip[], unit: number): number {
  const a = Math.min(...cs.map(c => c.start)), b = Math.max(...cs.map(c => c.start + c.len));
  const u = unit || 1;
  return Math.ceil((b - a) / u - 1e-6) * u;
}

export function pasteClips(cb: Clip[], beat: number, track: number | null, nTracks: number, alloc: () => number): Clip[] {
  const a = Math.min(...cb.map(c => c.start)), t0 = Math.min(...cb.map(c => c.track));
  const dt = track == null ? 0 : track - t0;
  return cb.map(c => ({
    ...c, id: alloc(),
    start: clamp(c.start - a + beat, 0, TOTAL - c.len),
    track: clamp(c.track + dt, 0, nTracks - 1),
  }));
}

/**
 * Resize from an edge. `srcLen` is the length in beats of what the clip shows (the audio, the comp,
 * the pattern; Infinity for automation): an edge stops at its end and at its start, so a pattern
 * clip shows its pattern at most once (#245). A clip that already reaches further (a pattern loop
 * from an older project, a pattern since shortened) keeps what it has, but does not grow.
 * `minL` is the shortest a trim leaves a clip, but a clip that is already shorter keeps its length
 * as its floor: an edge never moves against the drag, so trimming a short clip cannot make it longer.
 */
export function resizeClip(o: Clip, edge: 'l' | 'r', target: number, minL: number, srcLen: number): Clip {
  const n = { ...o };
  const bounded = o.kind === 'audio' || o.kind === 'comp' || o.kind === 'pat';
  if (edge === 'r') {
    let len = Math.max(Math.min(minL, o.len), target - o.start);
    if (bounded) len = Math.min(len, Math.max(srcLen - o.offset, o.len));
    n.len = Math.min(len, TOTAL - o.start);
  } else {
    const lo = bounded ? o.start - Math.max(0, o.offset) : 0;
    const ns = clamp(target, Math.max(0, lo), o.start + o.len - Math.min(minL, o.len));
    n.start = ns; n.offset = o.offset + (ns - o.start); n.len = o.len - (ns - o.start);
  }
  return n;
}

/**
 * Stretch an audio clip from an edge to `target`: the opposite edge stays, and the audio the
 * clip shows is played longer or shorter (offset and length scale with the stretch, so the
 * clip keeps showing the same part of its source). The stretch is kept within
 * STRETCH_MIN..STRETCH_MAX and the clip on the timeline and at least `minL` long.
 */
export function stretchClip(o: Clip, edge: 'l' | 'r', target: number, minL: number, dur: number): Clip {
  const s0 = o.stretch ?? 1, end = o.start + o.len;
  // lengths the stretch limits, the marker segments' and the song allow (a factor scales
  // every marker segment's slope with the clip, so the map bounds the drag too)
  let lo = Math.max(minL, o.len * STRETCH_MIN / s0), hi = Math.min(o.len * STRETCH_MAX / s0, edge === 'r' ? TOTAL - o.start : end);
  const wb = warpScaleBounds(o, dur);
  if (wb) {
    lo = Math.max(lo, o.len * wb[0]);
    hi = Math.min(hi, o.len * wb[1]);
  }
  if (hi < lo) return o;
  const len = clamp(edge === 'r' ? target - o.start : end - target, lo, hi);
  const k = len / o.len, s = s0 * k;
  // a stretch back to (almost) 1 snaps to exactly 1, so the clip plays its file untouched —
  // but the snap divides the markers by s0, and that factor was never bounds-checked: a map
  // with a segment already at the limit would tip out of range (s0 = 1.0000005 with a head
  // segment at ×0.25 snaps the segment to ×0.2499999). Snap only when 1/s0 stays in bounds;
  // else keep the stretched result, which k's clamping already made valid.
  const snapOk = !wb || (1 / s0 >= wb[0] - 1e-9 && 1 / s0 <= wb[1] + 1e-9);
  const exact = Math.abs(s - 1) < 1e-6 && snapOk;
  // the markers' `to` lives on the same stretched axis the offset scales on
  const warp = o.warp?.map(w => ({ at: w.at, to: exact ? w.to / s0 : w.to * k }));
  const n: Clip = { ...o, len: exact ? o.len / s0 : len, offset: exact ? o.offset / s0 : o.offset * k, warp };
  if (exact) { delete n.stretch; delete n.resample; } else n.stretch = s;
  if (edge === 'l') n.start = end - n.len;
  return n;
}

/** Stretch several clips together by the factor the dragged clip's length changed (`k`). */
export function stretchClips(clips: Clip[], edge: 'l' | 'r', k: number, minL: number, durOf: (c: Clip) => number): Clip[] {
  return clips.map(o => stretchClip(o, edge, edge === 'r' ? o.start + o.len * k : o.start + o.len - o.len * k, minL, durOf(o)));
}

/**
 * Trim several clips together: the edge of each moves by `delta` beats (what the dragged clip's
 * edge moved), each within its own limits (minimum length, its source, the song).
 */
export function resizeClips(clips: Clip[], edge: 'l' | 'r', delta: number, minL: number, srcLenOf: (c: Clip) => number): Clip[] {
  return clips.map(o => resizeClip(o, edge, (edge === 'r' ? o.start + o.len : o.start) + delta, minL, srcLenOf(o)));
}

export function newTrack(id: number, n: number): Track {
  return { id, name: 'Track ' + (n + 1), color: '#8b93a1', muted: false, solo: false, arm: false, vol: VOL_UNITY, h: 56, insert: 0 };
}

/**
 * Insert a lane at index `i`. The op set has no track reorder, so the lane contents are
 * shifted down instead: every track from `i` on takes the data of the one above (ids
 * stay put), a new track is appended at the end, and clips on lanes ≥ i move down one.
 * A track-volume automation follows its lane to the slot that now holds it, like in
 * `moveTrackOps` — otherwise it would keep driving the id of the slot above.
 */
export function insertTrackOps(p: Project, i: number, newId: number): Op[] {
  const ts = p.tracks, n = ts.length, ops: Op[] = [];
  const contents = [...ts.slice(0, i), newTrack(0, n), ...ts.slice(i)];
  // existing ids keep their slot; the last slot is the new id
  for (let j = i; j < n; j++) {
    const src = contents[j];
    ops.push({ op: 'putTrack', track: { ...src, id: ts[j].id, name: j === i ? 'Track ' + (n + 1) : src.name } });
  }
  ops.push({ op: 'putTrack', track: { ...contents[n], id: newId } });
  const moved = p.clips.filter(c => c.track >= i).map(c => ({ ...c, track: c.track + 1 }));
  // putClips validates the track index against the *current* track count, so tracks go first.
  if (moved.length) ops.push({ op: 'putClips', clips: moved });
  for (const au of p.automations ?? []) {
    const t = au.target;
    if (t.kind !== 'trackVol') continue;
    const old = p.tracks.findIndex(tr => tr.id === t.id);
    if (old < i) continue;
    // lane `old` now lives one slot down: under ts[old+1].id, the last lane under the new id
    const id = old + 1 < n ? ts[old + 1].id : newId;
    if (id !== t.id) ops.push({ op: 'putAutomation', automation: { ...au, target: { kind: 'trackVol', id } } });
  }
  return ops;
}

/** Ops that toggle mute for the given clips to `val`. */
export function muteOps(cs: Clip[], val: boolean): Op[] {
  const ch = cs.filter(c => c.muted !== val).map(c => ({ ...c, muted: val }));
  return ch.length ? [{ op: 'putClips', clips: ch }] : [];
}

/** The "Fit to" lengths of the sample window, in beats (1, 2 beats; 1, 2, 4, 8 bars). */
export const FIT_BEATS = [1, 2, 4, 8, 16, 32] as const;

/**
 * The audio clip stretched so the part of its audio it plays lasts `beats` (FL's TIME → Beat/Bar #):
 * the stretch scales by `beats / len`, the offset with it (so the same audio plays), the start stays.
 * `dur` is the source's length in seconds (its marker map is in source seconds). Null when that
 * needs a stretch outside `STRETCH_MIN..STRETCH_MAX` or the marker segments' range, or runs past
 * the song's end.
 */
export function fitClip(c: Clip, beats: number, dur: number): Clip | null {
  if (c.kind !== 'audio' || !(beats > 0) || !(c.len > 0)) return null;
  const s = c.stretch ?? 1, k = beats / c.len, ns = s * k;
  if (ns < STRETCH_MIN - 1e-9 || ns > STRETCH_MAX + 1e-9 || c.start + beats > TOTAL_BEATS + 1e-9) return null;
  // a factor outside what the marker segments allow would build a map the engine refuses
  const wb = warpScaleBounds(c, dur);
  if (wb && (k < wb[0] - 1e-9 || k > wb[1] + 1e-9)) return null;
  const r = (x: number) => Math.round(x * 1e9) / 1e9;
  return { ...c, stretch: r(Math.min(STRETCH_MAX, Math.max(STRETCH_MIN, ns))), offset: r(c.offset * k), len: beats, warp: c.warp?.map(w => ({ at: w.at, to: r(w.to * k) })) };
}

/**
 * What every audio clip of one source shares, as FL's audio clips share their channel: volume,
 * pan, pitch, normalize, reverse and the mixer insert. Editing one clip's sound edits them all; the region, the
 * fades, the stretch and pitch-editor notes stay the clip's own ("Make unique" splits a clip off).
 */
export const SHARED_SOUND = ['gain', 'pan', 'pitch', 'norm', 'rev', 'insert', 'root'] as const;
type SoundKey = typeof SHARED_SOUND[number];

/** The shared sound of `ref`'s audio clips (from the first one), for a new clip of it; empty when it has none yet. */
export function sharedSound(clips: readonly Clip[], ref: number): Partial<Pick<Clip, SoundKey>> {
  const like = clips.find(c => c.kind === 'audio' && c.ref === ref);
  if (!like) return {};
  const out: Partial<Pick<Clip, SoundKey>> = {};
  for (const k of SHARED_SOUND) if (like[k] !== undefined) (out as Record<string, unknown>)[k] = like[k];
  return out;
}

/**
 * `edited` with `patch`, and every other audio clip of the same source with the shared-sound part
 * of it (one op, one undo step). A comp or a pattern clip only changes itself.
 */
export function putSoundClips(clips: readonly Clip[], edited: Clip, patch: Partial<Clip>): Op {
  const self = { ...edited, ...patch };
  if (edited.kind !== 'audio') return { op: 'putClips', clips: [self] };
  const shared: Partial<Clip> = {};
  for (const k of SHARED_SOUND) if (k in patch) (shared as Record<string, unknown>)[k] = patch[k];
  if (!Object.keys(shared).length) return { op: 'putClips', clips: [self] };
  const others = clips.filter(c => c.id !== edited.id && c.kind === 'audio' && c.ref === edited.ref && SHARED_SOUND.some(k => k in shared && (c[k] ?? null) !== (shared[k] ?? null)));
  return { op: 'putClips', clips: [self, ...others.map(c => ({ ...c, ...shared }))] };
}

/**
 * The slice tool dragged down or up across lanes (FL): every clip on tracks `t0`..`t1` that the
 * beat `b` falls inside is cut there. Returns the clips to put (both halves of each), empty when
 * the line crosses none.
 */
export function sliceAcross(clips: readonly Clip[], b: number, t0: number, t1: number, alloc: () => number): Clip[] {
  const lo = Math.min(t0, t1), hi = Math.max(t0, t1);
  return clips.filter(c => c.track >= lo && c.track <= hi).flatMap(c => sliceClip(c, b, alloc()) ?? []);
}

/**
 * The paint tool's sweep (FL): the cells a drag from the first clip to (`beat`, `track`) passes that
 * are still free. The grid is the first clip's own: cells are `len` wide and start at `first.start`,
 * in both directions and on any lane. `taken` holds the cells already painted ("track:cell").
 */
export function paintCells(first: { start: number; len: number; track: number }, beat: number, track: number, nTracks: number, total: number, taken: ReadonlySet<string>): { track: number; start: number; key: string }[] {
  const tr = Math.max(0, Math.min(nTracks - 1, track));
  const cell = Math.floor((beat - first.start) / first.len + 1e-9), start = first.start + cell * first.len;
  const key = tr + ':' + cell;
  if (start < -1e-9 || start + first.len > total + 1e-9 || taken.has(key)) return [];
  return [{ track: tr, start: Math.max(0, start), key }];
}

/**
 * Move lane `i` to the place of lane `j` (its neighbour: Move up / Move down). Like `insertTrackOps`,
 * the lanes swap what they hold and ids stay with their slots: names, colours, routing and heights
 * trade places, the clips follow, and a volume automation keeps driving the track it was made for.
 */
/**
 * Lane `from` moved to index `to` (the lanes between shift by one). Track ids stay with their
 * slots (the op set has no reorder), so the lanes' contents move: every slot from the lower to
 * the higher index takes the data of the lane that lands there, its clips follow, and a track
 * volume automation follows its lane to the slot's id. `[]` when nothing moves.
 */
export function moveTrackOps(p: Project, from: number, to: number): Op[] {
  const n = p.tracks.length;
  if (from === to || from < 0 || to < 0 || from >= n || to >= n) return [];
  // order[k] = the old index of the lane that ends up at index k
  const order = p.tracks.map((_, k) => k);
  order.splice(to, 0, ...order.splice(from, 1));
  const newIndex = new Map(order.map((old, k) => [old, k]));
  const lo = Math.min(from, to), hi = Math.max(from, to);
  const ops: Op[] = [];
  for (let k = lo; k <= hi; k++) ops.push({ op: 'putTrack', track: { ...p.tracks[order[k]], id: p.tracks[k].id } });
  const moved = p.clips.filter(c => c.track >= lo && c.track <= hi).map(c => ({ ...c, track: newIndex.get(c.track)! }));
  if (moved.length) ops.push({ op: 'putClips', clips: moved });
  for (const au of p.automations ?? []) {
    const t = au.target;
    if (t.kind !== 'trackVol') continue;
    const old = p.tracks.findIndex(tr => tr.id === t.id);
    if (old < lo || old > hi) continue;
    const id = p.tracks[newIndex.get(old)!].id;
    if (id !== t.id) ops.push({ op: 'putAutomation', automation: { ...au, target: { kind: 'trackVol', id } } });
  }
  return ops;
}

export function swapTrackOps(p: Project, i: number, j: number): Op[] {
  const a = p.tracks[i], b = p.tracks[j];
  if (!a || !b || i === j) return [];
  const ops: Op[] = [{ op: 'putTrack', track: { ...b, id: a.id } }, { op: 'putTrack', track: { ...a, id: b.id } }];
  const moved = p.clips.filter(c => c.track === i || c.track === j).map(c => ({ ...c, track: c.track === i ? j : i }));
  if (moved.length) ops.push({ op: 'putClips', clips: moved });
  for (const au of p.automations ?? []) {
    const t = au.target;
    if (t.kind === 'trackVol' && (t.id === a.id || t.id === b.id)) ops.push({ op: 'putAutomation', automation: { ...au, target: { kind: 'trackVol', id: t.id === a.id ? b.id : a.id } } });
  }
  return ops;
}

/**
 * Several patterns dropped at once (Browser multi-selection): each on its own track, from the
 * drop's track down, all starting at `start` (clipped to the song). Tracks missing below the last
 * one are added. `lenOf`: a pattern's length in beats.
 */
export function stackPatternClips(refs: readonly number[], track: number, start: number, tracks: number, total: number,
  lenOf: (ref: number) => number, newId: () => number): { clips: Clip[]; newTracks: number } {
  const clips = refs.map((ref, i): Clip => {
    const len = Math.max(1e-3, Math.min(lenOf(ref), total - start));
    return newClip(newId(), track + i, start, len, 'pat', ref);
  });
  return { clips, newTracks: Math.max(0, track + refs.length - tracks) };
}
