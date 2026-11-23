// The engine's state, mirrored. Rust owns the project and undo; this store holds the latest
// StateDto plus local optimistic edits (applied immediately so drags feel instant).
//
// Solid 2: the project lives twice. `S.project` is the reactive store the views read; `M` is a
// plain copy of the same data that only this module writes (every edit is applied to both, and an
// engine state is diffed against `M`). `M` gives what Solid 1's `unwrap` gave for free and Solid 2
// has no public equivalent of: the current project as plain objects, read without a proxy per
// field (`rawOf`, `plain`, `plainNotes`, the arrangement listeners, the canvas views). It also lets
// `replaceState` write only what changed, in place, so store items keep their identity and a held
// reference stays live (Solid 2's `reconcile` swaps in the engine's objects instead: a reference
// taken before reads stale data afterwards). `M` never shares an object with the store.
import { $PROXY, createMemo, createRoot, snapshot, untrack } from 'solid-js';
import type { Catalog, Clip, Fx, Id, Insert, Note, Op, Pattern, Project, StateDto, TransportDto } from '../api/types';
import { applyCompOp, compExtent } from '../lib/comp';
import { batch, createSignal, createStore } from '../lib/reactive';
import { audibleInserts, forgetInsert } from '../lib/routing';
import { applyNotebookOp } from '../lib/notebook';
import { putMarker, removeMarker } from '../lib/markers';
import { applyPatternOp, fitLen as fitPatternLen, followPatternLength } from '../lib/pattern';
import { applyTuneEdit } from '../lib/pitch';
import { applyTransform } from '../lib/transforms';
import { dropSourcePeaks } from './peaks';

export interface EngineState {
  ready: boolean;
  project: Project;
  revision: number;
  canUndo: boolean;
  canRedo: boolean;
  undoLabel: string | null;
  redoLabel: string | null;
  transport: TransportDto;
  path: string | null;
  dirty: boolean;
  catalog: Catalog | null;
  /** Comp id → take auditioned instead of the comp (live only). */
  audition: Record<string, Id>;
  /** Clip id → stretch status: being rendered (`''`) or failed (the message). Live only. */
  stretching: Record<string, string>;
  /** Source id → why its audio is not loaded (it plays silence until relinked). */
  missing: Record<string, string>;
  /** Fx id → why its hosted plugin did not load (the slot passes audio through; its data is kept). */
  fxErrors: Record<string, string>;
  /** Automation ids a manual tweak overrode while playing (knobs show the override; re-enable or stop clears). */
  overriddenAutos: Id[];
}

const emptyProject = (): Project => ({ version: 2, bpm: 128, tracks: [], inserts: [], patterns: [], sources: [], clips: [], loop: null, nextId: 1 });

export const [S, setS] = createStore<EngineState>({
  ready: false, project: emptyProject(), revision: 0, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  transport: { playing: false, rec: false, monitor: false, metro: false, countIn: true, startBeat: 0, recAudio: true, recNotes: true, recQuantize: 0, recAutomation: true, recAutoLatch: false },
  path: null, dirty: false, catalog: null, audition: {}, stretching: {}, missing: {}, fxErrors: {}, overriddenAutos: [],
});

/** The project as plain objects, equal to `S.project` (see the header). Read only: never mutate it. */
let M: Project = emptyProject();

function stretchStatus(dto: StateDto): Record<string, string> {
  const out: Record<string, string> = { ...(dto.stretchErrors ?? {}) };
  for (const id of dto.stretching ?? []) out[id] = '';
  return out;
}

/**
 * Bumped on every change to the project (local edit or engine state). Canvas views that draw
 * whole lists (clips, patterns, automations) depend on this one signal instead of subscribing
 * to every field of every item, which cost ~7 ms per clip move on a 1400-clip song.
 */
const [projVersion, setProjVersion] = createSignal(0);
export { projVersion };
/**
 * Like `projVersion`, but not bumped by changes (local edits or engine states) that only touch the
 * mixer, the notebook or the loop range (effects, inserts, lyrics): the arrangement canvases
 * (playlist, pattern editor) skip a full redraw when an effect is added, bypassed or tweaked.
 */
const [arrVersion, setArrVersion] = createSignal(0);
export { arrVersion };
const bumpProject = (arrangement = true) => batch(() => { setProjVersion(v => v + 1); if (arrangement) setArrVersion(v => v + 1); });
const MIXER_ONLY = new Set<Op['op']>(['putFx', 'removeFx', 'moveFx', 'putInsert', 'removeInsert', 'putNotebook']);
/** Not mixer edits, but nothing an arrangement canvas draws from the clip list either: the loop
 *  range is read directly by the views that show it (a loop drag must not redraw every clip). */
const NOT_ARRANGEMENT = new Set<Op['op']>([...MIXER_ONLY, 'setLoop', 'putMarker', 'removeMarker']);

/**
 * Told about every arrangement change before it is applied: the local ops (with the project as it
 * is before them, plain), or `null` when the whole project was replaced from the engine. The
 * playlist uses it to repaint only the clips an edit touched instead of the whole view.
 */
export type ArrangementListener = (ops: readonly Op[] | null, before: Project) => void;
const arrListeners = new Set<ArrangementListener>();
export function onArrangementChange(f: ArrangementListener): () => void {
  arrListeners.add(f);
  return () => { arrListeners.delete(f); };
}
const tellArrangement = (ops: readonly Op[] | null) => {
  if (!arrListeners.size) return;
  for (const f of arrListeners) f(ops, M);
};

// ---- plain copies ----
type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => v !== null && typeof v === 'object';
const isProxy = (v: unknown): boolean => isObj(v) && !!(v as Record<symbol, unknown>)[$PROXY];

/**
 * An item of `M` and where it is in the project: `list[i]`, or `list[i][sub][j]` for an effect or a
 * take (the same place in `S.project` holds its store proxy). Flat fields, no array per item: the
 * index is rebuilt on the first lookup after every edit (a drag frame).
 */
interface Indexed { r: object; list: string; i: number; sub: string | null; j: number }
/**
 * Project items by id: rebuilt on the first lookup after a change of `M`. New ids come from one
 * counter (`nextId`), but a project can still use one id in several lists (the engine's demo song
 * numbers its patterns, clips and inserts from 1 each), so an id can hold several places.
 */
let byId: Map<Id, Indexed | Indexed[]> | null = null;
/**
 * Store proxy of a project item → its object in `M`, filled by `rawOf` once the proxy was found at
 * that object's place in `S.project` (so a hit needs no check). Valid while `M` is unchanged: it is
 * dropped with the id index by `mChanged`, which every writer of `M` calls.
 */
let rawCache = new WeakMap<object, object>();
/** `M` (and the store's project) change: the id index and the `rawOf` cache are stale. */
function mChanged(): void {
  byId = null;
  rawCache = new WeakMap();
}
function idIndex(): Map<Id, Indexed | Indexed[]> {
  if (byId) return byId;
  const m = new Map<Id, Indexed | Indexed[]>();
  const add = (xs: readonly { id: Id }[] | undefined, list: string, i = -1, sub: string | null = null) => {
    if (xs) for (let k = 0; k < xs.length; k++) {
      const e: Indexed = sub ? { r: xs[k], list, i, sub, j: k } : { r: xs[k], list, i: k, sub, j: -1 };
      const was = m.get(xs[k].id);
      m.set(xs[k].id, was === undefined ? e : Array.isArray(was) ? (was.push(e), was) : [was, e]);
    }
  };
  add(M.tracks, 'tracks'); add(M.inserts, 'inserts'); M.inserts.forEach((ins, i) => add(ins.fx, 'inserts', i, 'fx'));
  add(M.patterns, 'patterns'); add(M.sources, 'sources'); add(M.clips, 'clips');
  add(M.automations, 'automations'); add(M.channels, 'channels'); add(M.notebook?.pages, 'notebook.pages');
  if (M.comps) { add(M.comps, 'comps'); M.comps.forEach((c, i) => add(c.takes, 'comps', i, 'takes')); }
  return (byId = m);
}
/** The store proxy at an indexed item's place in `S.project` (read untracked by the caller). */
function storeAt(e: Indexed): unknown {
  const p = S.project as unknown as Obj;
  const xs = (e.list === 'notebook.pages' ? (p.notebook as Obj | undefined)?.pages : p[e.list]) as Obj[] | undefined;
  const x = xs?.[e.i];
  return e.sub ? (x?.[e.sub] as Obj[] | undefined)?.[e.j] : x;
}
const sameKeys = (a: object, b: object) => {
  const ka = Object.keys(a), kb = Object.keys(b);
  return ka.length === kb.length && ka.every(k => k in b);
};

/**
 * The plain, current data behind store data (Solid 1's `unwrap`): `S.project` and its items with
 * an id come from `M` without a copy (read only: never mutate them); anything else in a store is
 * copied (`snapshot`, a walk of it); plain data comes back as is. Does not track.
 * A project item's proxy seen before (and `S.project`) is a WeakMap hit until the next change.
 */
export function rawOf<T>(x: T): T {
  if (!isObj(x)) return x;
  const hit = rawCache.get(x);
  if (hit) return hit as T;
  if (!isProxy(x)) return x;
  return untrack(() => {
    if ((x as unknown) === S.project) { rawCache.set(x, M); return M as T; }
    const id = (x as Obj).id;
    if (typeof id === 'number') {
      const hit = idIndex().get(id);
      const es = hit === undefined ? [] : Array.isArray(hit) ? hit : [hit];
      // the store's own item at one of the id's places: certain, and cached
      for (const e of es) if (Array.isArray(e.r) === Array.isArray(x) && storeAt(e) === x) { rawCache.set(x, e.r); return e.r as T; }
      // not where the item is (another store holding a copy, or a mirror out of step): the
      // check this had before the cache, and never cached
      for (const e of es) if (Array.isArray(e.r) === Array.isArray(x) && sameKeys(e.r, x as object)) return e.r as T;
    }
    // a list of items with ids (clips, tracks): each from `M`; else one copy (notes, points)
    if (Array.isArray(x) && x.length && typeof (x[0] as Obj | undefined)?.id === 'number') return x.map(rawOf) as T;
    return snapshot(x);
  });
}

/**
 * A plain deep copy (JSON semantics) of store data or of anything built from it. Store data is
 * copied from `M` (the native fast path, instead of a proxy trap and a tracked read per field:
 * ~0.5 s per drag on a 2000-note pattern); a fresh container that holds proxies is unwrapped item
 * by item.
 */
export function plain<T>(v: T): T {
  if (v === undefined) return v;
  const r = rawOf(v);
  return JSON.parse(r !== v ? JSON.stringify(r) : JSON.stringify(v, (_k, x) => rawOf(x)));
}

/** A copy of notes (flat objects) taken from the store or built from its notes: a spread per note, ~10× faster than `plain`. */
export function plainNotes<T extends object>(notes: readonly T[]): T[] {
  return rawOf(notes).map(n => ({ ...rawOf(n) }));
}

// ---- ids ----
let localNext = 1;
/** Allocate a new id (project.nextId, increased locally). */
export function allocId(): Id {
  const id = Math.max(localNext, M.nextId);
  localNext = id + 1;
  return id;
}

// ---- replacing from the engine ----
/** A copy for the store: an object handed to it must never be one of `M`'s. */
const own = <T,>(v: T): T => (isObj(v) ? structuredClone(v) : v);
/**
 * Write into the draft `d[key]` what changed from `prev` (what it holds, as plain data) to `next`:
 * Solid 1's `reconcile(next, { key: 'id', merge: true })`. Objects with the same id (or both
 * without one) are merged in place, so they keep their identity; arrays of items with ids are
 * reordered by id; anything else is replaced. Only changed leaves are written (and notify).
 */
function mergeInto(d: Obj, key: string | number, prev: unknown, next: unknown): void {
  if (prev === next) return;
  if (!isObj(prev) || !isObj(next) || Array.isArray(prev) !== Array.isArray(next) || (!Array.isArray(next) && prev.id !== next.id)) {
    if (next === undefined) delete d[key]; else d[key] = own(next);
    return;
  }
  let dd: Obj | undefined;
  const draft = () => (dd ??= d[key] as Obj);
  if (Array.isArray(next)) {
    const p = prev as unknown as unknown[], n = next as unknown[];
    let aligned = p.length === n.length;
    if (aligned) for (let i = 0; i < n.length; i++) { const a = p[i], b = n[i]; if (isObj(a) && isObj(b) && a.id !== b.id) { aligned = false; break; } }
    const keyed = !aligned && n.length > 0 && n.every(x => isObj(x) && x.id != null);
    if (!keyed) {
      // positional (Solid 1 too, when the items have no ids): merge the common part, then grow or cut
      const common = Math.min(p.length, n.length);
      for (let i = 0; i < common; i++) if (p[i] !== n[i] && !same(p[i], n[i])) mergeInto(draft(), i, p[i], n[i]);
      if (n.length > p.length) for (let i = p.length; i < n.length; i++) (draft() as unknown as unknown[])[i] = own(n[i]);
      else if (n.length < p.length) (draft() as unknown as unknown[]).length = n.length;
      return;
    }
    // keyed: items keep their store identity by id, in the new order
    const at = new Map<unknown, number>();
    p.forEach((x, i) => { if (isObj(x) && x.id != null && !at.has(x.id)) at.set(x.id, i); });
    const dArr = draft() as unknown as unknown[];
    const order = n.map(x => at.get((x as Obj).id));
    d[key] = n.map((x, i) => (order[i] !== undefined ? dArr[order[i]!] : own(x)));
    dd = undefined;
    n.forEach((x, i) => { const j = order[i]; if (j !== undefined && !same(p[j], x)) mergeInto(draft(), i, p[j], x); });
    return;
  }
  for (const k of Object.keys(next)) { const a = prev[k], b = next[k]; if (a !== b && !same(a, b)) mergeInto(draft(), k, a, b); }
  for (const k of Object.keys(prev)) if (next[k] === undefined && prev[k] !== undefined) delete draft()[k];
}
/** Deep equality of plain JSON data (an equal subtree is skipped without touching the draft). */
function same(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (!isObj(a) || !isObj(b) || Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    const bb = b as unknown as unknown[];
    if (a.length !== bb.length) return false;
    for (let i = 0; i < a.length; i++) if (!same(a[i], bb[i])) return false;
    return true;
  }
  let na = 0, nb = 0;
  for (const k in a) { if (a[k] === undefined) continue; na++; if (!same(a[k], b[k])) return false; }
  for (const k in b) if (b[k] !== undefined) nb++;
  return na === nb;
}
/** Replace the transport or a live-only map (`audition`, `missing`, ...) in place, its removed keys too. */
function setLive<K extends 'transport' | 'audition' | 'stretching' | 'missing' | 'fxErrors' | 'overriddenAutos'>(key: K, next: EngineState[K]) {
  const prev = untrack(() => snapshot(S[key]));
  if (!same(prev, next)) setS(d => { mergeInto(d as unknown as Obj, key, prev, next); });
}
function setMeta(dto: StateDto) {
  setS({ revision: dto.revision, canUndo: dto.canUndo, canRedo: dto.canRedo, undoLabel: dto.undoLabel ?? null, redoLabel: dto.redoLabel ?? null, path: dto.path, dirty: dto.dirty, ready: true });
  setLive('transport', dto.transport);
  setLive('audition', dto.audition ?? {});
  setLive('stretching', stretchStatus(dto));
  setLive('missing', dto.missing ?? {});
  setLive('fxErrors', dto.fxErrors ?? {});
  setLive('overriddenAutos', dto.overriddenAutos ?? []);
}

/** Project fields no arrangement canvas draws from (what `NOT_ARRANGEMENT` ops change, and the id counter). */
const NOT_ARRANGEMENT_KEYS = new Set<string>(['inserts', 'notebook', 'loop', 'nextId'] satisfies (keyof Project)[]);

export function replaceState(dto: StateDto) {
  const old = M as unknown as Obj, next = dto.project as unknown as Obj;
  // Most engine states are the reply to an edit already applied here, or carry only a mixer change:
  // only the fields that differ are merged and announced, so the arrangement canvases are not
  // redrawn in full (and nothing re-runs) for a state that does not change what they draw.
  const changed: string[] = [];
  for (const k of new Set([...Object.keys(old), ...Object.keys(next)])) if (old[k] !== next[k] && !same(old[k], next[k])) changed.push(k);
  const arrangement = changed.some(k => !NOT_ARRANGEMENT_KEYS.has(k));
  if (arrangement) tellArrangement(null);
  // a relink (or its undo) gives a source other audio under the same id: its waveform is redrawn
  const was = new Map(M.sources.map(s => [s.id, s.path]));
  for (const s of dto.project.sources) { const p = was.get(s.id); if (p != null && p !== s.path) dropSourcePeaks(s.id); }
  mChanged();
  // `M` keeps its own objects where nothing changed: what was read from it (a canvas's lookup
  // tables, kept while `arrVersion` holds) stays current, and the previous project is not kept alive
  const kept: Obj = { ...next };
  for (const k of Object.keys(old)) if (!changed.includes(k)) kept[k] = old[k];
  M = kept as unknown as Project;
  batch(() => {
    if (changed.length) setS(d => { const p = d.project as unknown as Obj; for (const k of changed) mergeInto(p, k, old[k], next[k]); });
    mChanged(); // nothing cached while the store was half merged
    if (changed.length) bumpProject(arrangement);
    setMeta(dto);
  });
  localNext = Math.max(localNext, dto.project.nextId);
}

/** Everything but the project (whose edits are already applied locally). */
export function replaceMeta(dto: StateDto) {
  batch(() => setMeta(dto));
  localNext = Math.max(localNext, dto.project.nextId);
}

// ---- optimistic local application (mutates in place so identities and signals stay fine-grained) ----
function assignClip(dst: Clip, src: Clip) {
  for (const k of Object.keys(src) as (keyof Clip)[]) {
    if (k === 'tune') { if (JSON.stringify(dst.tune ?? null) !== JSON.stringify(src.tune ?? null)) dst.tune = src.tune ? structuredClone(src.tune) : null; continue; }
    if (dst[k] !== src[k]) (dst as unknown as Record<string, unknown>)[k] = src[k];
  }
  if (!('tune' in src) && dst.tune) dst.tune = null;
  // the engine leaves the defaults out (stretch 1, not resampled)
  if (!('stretch' in src) && dst.stretch !== undefined) delete dst.stretch;
  if (!('resample' in src) && dst.resample !== undefined) delete dst.resample;
}
function assignFx(dst: Fx, src: Fx) {
  if (dst.bypass !== src.bypass) dst.bypass = src.bypass;
  if ((dst.mix ?? 1) !== (src.mix ?? 1)) dst.mix = src.mix ?? 1;
  if (dst.preset !== src.preset) dst.preset = src.preset ?? null;
  for (const k of Object.keys(src.params)) if (dst.params[k] !== src.params[k]) dst.params[k] = src.params[k];
  for (const k of Object.keys(dst.params)) if (!(k in src.params)) delete dst.params[k];
  if (src.bands) {
    if (!dst.bands || dst.bands.length !== src.bands.length) dst.bands = src.bands.map(b => ({ ...b }));
    else src.bands.forEach((b, i) => {
      const d = dst.bands![i];
      if (d.on !== b.on) d.on = b.on; if (d.type !== b.type) d.type = b.type; if (d.f !== b.f) d.f = b.f; if (d.g !== b.g) d.g = b.g; if (d.q !== b.q) d.q = b.q;
      if (d.slope !== b.slope) d.slope = b.slope; if (d.place !== b.place) d.place = b.place;
      const dd = d.dynamic ?? null, sd = b.dynamic ?? null;
      if (!sd) { if (dd) d.dynamic = null; } else if (!dd || dd.range !== sd.range || dd.thr !== sd.thr) d.dynamic = { ...sd };
    });
  }
  if (src.plugin) dst.plugin = { ...src.plugin };
}
const clone = <T,>(v: T): T => structuredClone(v);
/** `structuredClone` of a pattern, ~5× faster for its notes (flat objects; a 2000-note piano-roll drag copies them per frame). */
const copyPattern = (pt: Pattern): Pattern => {
  const out = { ...pt } as Record<string, unknown>;
  for (const [k, v] of Object.entries(pt)) if (v !== null && typeof v === 'object') out[k] = k === 'notes' ? (v as Note[]).map(n => ({ ...n })) : structuredClone(v);
  return out as unknown as Pattern;
};

/** Apply `op` to `p` in place: the store's draft, or `M`. `raw` is `M` as it is before the op (plain). */
function applyInPlace(p: Project, op: Op, raw: Project = p) {
  switch (op.op) {
    case 'putClips': {
      // positions from the plain copy (the same as the draft's): no proxy read per clip
      const idx = new Map<Id, number>(); raw.clips.forEach((c, i) => idx.set(c.id, i));
      for (const c of op.clips) {
        const i = idx.get(c.id);
        if (i != null) assignClip(p.clips[i], c); else { p.clips.push(clone(c)); idx.set(c.id, p.clips.length - 1); }
        if (c.id >= p.nextId) p.nextId = c.id + 1;
      }
      return;
    }
    case 'removeClips': { const s = new Set(op.ids); p.clips = p.clips.filter(c => !s.has(c.id)); return; }
    case 'putTrack': {
      const t = p.tracks.find(x => x.id === op.track.id);
      if (t) Object.assign(t, op.track); else p.tracks.push(clone(op.track));
      if (op.track.id >= p.nextId) p.nextId = op.track.id + 1;
      return;
    }
    case 'removeTrack': {
      // engine semantics (applyOps ported): clips on the track go, clips below it shift up
      const idx = p.tracks.findIndex(t => t.id === op.id);
      if (idx < 0 || p.tracks.length === 1) return; // refused: the engine's reply resyncs
      p.tracks.splice(idx, 1);
      p.clips = p.clips.filter(c => c.track !== idx);
      for (const c of p.clips) if (c.track > idx) c.track -= 1;
      return;
    }
    case 'putInsert': {
      const m = p.inserts.find(x => x.id === op.insert.id);
      const { fx, ...rest } = op.insert;
      // the engine writes no `sends` / `master` for an unrouted insert: their absence resets them
      if (m) Object.assign(m, { sends: undefined, master: undefined }, rest); else p.inserts.push({ ...clone(rest), fx: clone(fx.filter(f => f.type !== 'plugin')) } as Insert);
      if (op.insert.id >= p.nextId) p.nextId = op.insert.id + 1;
      return;
    }
    case 'removeInsert':
      p.inserts = p.inserts.filter(m => m.id !== op.id);
      forgetInsert(p, op.id);
      for (const t of p.tracks) if (t.insert === op.id) t.insert = 0;
      for (const c of p.clips) if (c.insert === op.id) c.insert = null;
      return;
    case 'putFx': {
      for (const m of p.inserts) {
        const f = m.fx.find(x => x.id === op.fx.id);
        if (f) { assignFx(f, op.fx); return; }
      }
      const m = p.inserts.find(x => x.id === op.insertId); if (!m) return;
      m.fx.splice(Math.min(op.index ?? m.fx.length, m.fx.length), 0, clone(op.fx));
      if (op.fx.id >= p.nextId) p.nextId = op.fx.id + 1;
      return;
    }
    case 'removeFx': for (const m of p.inserts) { const i = m.fx.findIndex(f => f.id === op.fxId); if (i >= 0) m.fx.splice(i, 1); } return;
    case 'moveFx': {
      const m = p.inserts.find(x => x.fx.some(f => f.id === op.fxId)); if (!m) return;
      const i = m.fx.findIndex(f => f.id === op.fxId), to = Math.min(op.to, m.fx.length - 1);
      if (i === to) return;
      const [f] = m.fx.splice(i, 1); m.fx.splice(to, 0, f);
      return;
    }
    case 'putPattern': case 'putChannel': case 'removeChannel': applyPatternOp(p, op, false, copyPattern); return;
    case 'putSource': { const s = p.sources.find(x => x.id === op.source.id); if (s) { s.name = op.source.name; s.color = op.source.color; } return; }
    case 'putComp': case 'removeComp': applyCompOp(p, op, false); return;
    case 'removeSource':
      applyCompOp(p, op, false);
      applyPatternOp(p, op, false);
      p.sources = p.sources.filter(s => s.id !== op.id);
      p.clips = p.clips.filter(c => !(c.kind === 'audio' && c.ref === op.id));
      return;
    case 'putNotebook': applyNotebookOp(p, op.notebook, false); return;
    // refused (a bad position or name): left to the engine's reply, like the other ports
    case 'putMarker': try { putMarker(p, op.marker); } catch { /* the reply resyncs */ } return;
    case 'removeMarker': try { removeMarker(p, op.id); } catch { /* already gone */ } return;
    case 'setBpm': p.bpm = op.bpm; return;
    case 'setLoop': p.loop = op.loop ? { ...op.loop } : null; return;
    case 'setKey': p.key = op.key; p.scale = op.scale ?? 1; return;
    case 'putAutomation': {
      const a = clone(op.automation); a.points.sort((x, y) => x.t - y.t);
      if (!p.automations) p.automations = [];
      const x = p.automations.find(q => q.id === a.id);
      if (x) { x.name = a.name; x.color = a.color; x.target = a.target; x.points = a.points; } else p.automations.push(a);
      if (a.id >= p.nextId) p.nextId = a.id + 1;
      return;
    }
    case 'removePattern':
      p.patterns = p.patterns.filter(x => x.id !== op.id);
      p.clips = p.clips.filter(c => !(c.kind === 'pat' && c.ref === op.id));
      return;
    case 'removeAutomation':
      p.automations = (p.automations ?? []).filter(a => a.id !== op.id);
      p.clips = p.clips.filter(c => !(c.kind === 'auto' && c.ref === op.id));
      return;
    case 'transformNotes': {
      // like the engine's transform path (applyOps ported): an edit it would refuse is left to its reply to undo
      const pt = p.patterns.find(x => x.id === op.pattern); if (!pt) return;
      const roots = (p.channels ?? []).map(c => ({ id: c.id, root: c.root ?? 60 }));
      // a plain copy, from `M` (`p` may be a Solid draft, whose proxies do not survive structuredClone)
      const notes: Note[] = JSON.parse(JSON.stringify(raw.patterns.find(x => x.id === op.pattern)?.notes ?? []));
      // generator sizes the engine refuses (bounded in ops.rs before anything allocates):
      // refuse them here too, before the optimistic transform allocates per slot
      const sizeOk = (x: unknown) => typeof x === 'number' && Number.isInteger(x) && x >= 1 && x <= 512;
      if (op.verb.kind === 'euclidFill' && (!sizeOk(op.verb.k) || !sizeOk(op.verb.n))) return;
      if (op.verb.kind === 'euclidDrums' && op.verb.voices.some(v => !sizeOk(v.k) || !sizeOk(v.n))) return;
      try { applyTransform(notes, op.verb, op.seed ?? 0, op.select ?? null, roots, p.key ?? null, p.scale ?? 1, Math.max(16, Math.round(pt.len * 4))); } catch { return; }
      pt.notes = notes;
      const oldLen = pt.len;
      fitPatternLen(pt);
      // a generator can push notes past the pattern's end: whole-pass clips follow, like putPattern
      if (Math.abs(oldLen - pt.len) > 1e-9) followPatternLength(p, op.pattern, oldLen, pt.len);
      return;
    }
    case 'editTune': {
      // the engine's own semantics (tune_edit.rs, ported): an edit it would refuse is left to its reply to undo
      const c = p.clips.find(x => x.id === op.clip); if (!c || c.kind !== 'audio') return;
      try { c.tune = applyTuneEdit(c.tune, op.edit); } catch { /* refused: the engine's reply resyncs */ }
      return;
    }
    default: {
      // a new Op kind must be applied optimistically too, or its edit stalls until the reconcile
      const exhaustive: never = op;
      return exhaustive;
    }
  }
}

/** Ops whose application copies everything it keeps (the others are copied for the store: `M` must not share objects with it). */
const SELF_COPYING = new Set<Op['op']>(['putClips', 'removeClips', 'removeTrack', 'putPattern', 'putChannel', 'removeChannel', 'removeInsert', 'removeFx', 'moveFx', 'removeSource', 'removeComp', 'removePattern', 'removeAutomation', 'setBpm', 'setLoop', 'setKey', 'putAutomation', 'transformNotes', 'putMarker', 'removeMarker']);
const copyOp = (op: Op): Op => {
  if (SELF_COPYING.has(op.op)) return op;
  try { return structuredClone(op); } catch { return plain(op); } // an op holding store proxies
};

export function applyLocal(ops: Op[]) {
  if (!ops.length) return;
  const arrangement = ops.some(op => !NOT_ARRANGEMENT.has(op.op));
  if (arrangement) tellArrangement(ops);
  const forStore = ops.map(copyOp);
  // one batch: memos that follow `arrVersion` are current before any effect of the edit runs
  batch(() => {
    mChanged();
    // op by op, the store's draft first and then `M`: each op sees `M` as the draft is before it
    setS(d => { ops.forEach((op, i) => { applyInPlace(d.project, forStore[i], M); applyInPlace(M, op); }); });
    mChanged(); // nothing cached while the edit was half applied
    bumpProject(arrangement);
    setS('dirty', true);
  });
}

// ---- lookups ----
/** Shared by every lane header / mixer strip (each one walked the whole list per update). */
export const anyTrackSolo = createRoot(() => createMemo(() => S.project.tracks.some(t => t.solo)));
export const anyInsertSolo = createRoot(() => createMemo(() => S.project.inserts.some(x => x.id !== 0 && x.solo)));
/** The inserts heard (mute, and solo along the routing: lib/routing.ts). */
export const heardInserts = createRoot(() => createMemo(() => audibleInserts(S.project)));
export function findFx(fxId: Id): { m: Insert; f: Fx } | null {
  for (const m of S.project.inserts) { const f = m.fx.find(x => x.id === fxId); if (f) return { m, f }; }
  return null;
}
export const insertById = (id: Id) => S.project.inserts.find(m => m.id === id);
export const srcOf = (c: Clip) => c.kind === 'pat' ? S.project.patterns.find(p => p.id === c.ref) : c.kind === 'auto' ? S.project.automations?.find(a => a.id === c.ref) : c.kind === 'comp' ? S.project.comps?.find(a => a.id === c.ref) : S.project.sources.find(s => s.id === c.ref);
export const autoById = (id: Id) => S.project.automations?.find(a => a.id === id);
export const spb = () => 60 / S.project.bpm;
export function srcLenBeats(kind: Clip['kind'], ref: Id): number {
  if (kind === 'pat') return S.project.patterns.find(p => p.id === ref)?.len ?? 4;
  if (kind === 'comp') { const cp = S.project.comps?.find(x => x.id === ref); return cp ? compExtent(cp)[1] : 4; }
  if (kind === 'auto') return Infinity; // an envelope holds its last value: auto clips can be any length
  const s = S.project.sources.find(x => x.id === ref);
  return s ? s.dur / spb() : 4;
}
/** How long clip `c` can be, in beats: its source as the clip plays it (audio stretched). */
export function clipSrcLen(c: Clip): number {
  return srcLenBeats(c.kind, c.ref) * (c.kind === 'audio' ? c.stretch ?? 1 : 1);
}
export function insName(m: Insert): string {
  const i = S.project.inserts.indexOf(m);
  return m.id === 0 ? 'Master' : 'Ins ' + i + ' · ' + m.name;
}
export function setTransport(t: Partial<TransportDto>) { setS('transport', t); }
