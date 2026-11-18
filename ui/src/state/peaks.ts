// Waveform peaks from `source_peaks`, cached per (source, norm, rev). Consumers poll `getPeaks`
// while drawing; a finished load bumps `peaksVersion` so canvases redraw.
import { createSignal } from 'solid-js';
import { be } from '../api/backend';
import type { Clip, Id } from '../api/types';
import { tuneIsNeutral } from '../lib/pitch';

/** A request that failed: tried again after a growing pause, at most `RETRY_ATTEMPTS` times in all. */
interface Failed { fails: number; retryAt: number }
const cache = new Map<string, Float32Array | 'loading' | Failed>();
const [peaksVersion, setVersion] = createSignal(0);
export { peaksVersion };

/**
 * A failed waveform (the engine was busy, a file was briefly locked, a render errored) is not
 * cached for good: it is asked again after 1 s, 2 s, 4 s … (at most 30 s), `RETRY_ATTEMPTS`
 * requests in all, so a source that stays broken does not hit the backend on every redraw.
 * Dropping or relinking the source, or opening a project, starts over.
 */
export const RETRY_ATTEMPTS = 5;
export const retryDelay = (fails: number) => Math.min(30_000, 1000 * 2 ** Math.max(0, fails - 1));

export function getPeaks(id: Id, norm = false, rev = false): Float32Array | null {
  const key = id + (norm ? 'n' : '') + (rev ? 'r' : '');
  const v = cache.get(key);
  if (v instanceof Float32Array) return v;
  if (v === 'loading') return null;
  if (v && (v.fails >= RETRY_ATTEMPTS || Date.now() < v.retryAt)) return null;
  const fails = v ? v.fails : 0;
  cache.set(key, 'loading');
  const ep = epoch;
  be().sourcePeaks(id, norm, rev)
    .then(p => { if (ep !== epoch) return; cache.set(key, p); setVersion(x => x + 1); })
    .catch(e => {
      if (ep !== epoch) return;
      console.warn('[wave] source_peaks', id, e);
      const failed: Failed = { fails: fails + 1, retryAt: Date.now() + retryDelay(fails + 1) };
      cache.set(key, failed);
      // canvases only ask while they redraw: wake them when the pause is over
      if (failed.fails < RETRY_ATTEMPTS) setTimeout(() => { if (ep === epoch && cache.get(key) === failed) setVersion(x => x + 1); }, retryDelay(failed.fails));
    });
  return null;
}

/**
 * Another project was loaded: forget everything. Source and clip ids start again in every
 * project, so a cached waveform would be drawn for whatever new sample got the same id (a clap
 * showing the previous project's file). Answers still on their way for the old project are dropped.
 */
let epoch = 0;
export function clearPeaks() {
  epoch++;
  cache.clear();
  for (const e of clipCache.values()) clearTimeout(e.timer);
  clipCache.clear();
  setVersion(x => x + 1);
}

/** Forget cached peaks of a source (e.g. after trim, the id changes anyway). */
export function dropPeaks(id: Id) {
  for (const k of [...cache.keys()]) if (k.replace(/[nr]+$/, '') === String(id)) cache.delete(k);
}

/** A source now plays other audio (relinked, or that undone): forget its waveforms, the source's
 *  and those rendered for its clips, and redraw. */
export function dropSourcePeaks(id: Id) {
  dropPeaks(id);
  for (const [cid, e] of clipCache) if (e.ref === id) { clearTimeout(e.timer); clipCache.delete(cid); }
  setVersion(x => x + 1);
}

/** Min/max of peaks over pair indices [i0, i1). */
export function peakRange(pk: Float32Array, i0: number, i1: number): [number, number] {
  const n = pk.length >> 1;
  let mn = 0, mx = 0;
  const a = Math.max(0, i0), b = Math.min(n, Math.max(i0 + 1, i1));
  for (let j = a; j < b; j++) { const lo = pk[2 * j], hi = pk[2 * j + 1]; if (lo < mn) mn = lo; if (hi > mx) mx = hi; }
  return [mn, mx];
}

// ---- clips with pitch edits: what the clip actually plays (`clip_peaks`) ----

// `key`: the edits `pk` was rendered for; `want`: the edits asked for; `fails`: failed renders of
// `want` in a row; `failed`: the edits whose render was given up on (the source's peaks show).
interface ClipEntry { key: string; pk: Float32Array | null; want: string; timer: number; busy: boolean; ref: Id; fails: number; failed: string }
const clipCache = new Map<Id, ClipEntry>();
let gestureOpen: () => boolean = () => false;
/** edit.ts tells us when a drag is in progress (the engine re-renders only at its end). */
export function setGestureProbe(f: () => boolean) { gestureOpen = f; }

const clipKey = (c: Clip) => c.ref + '|' + (c.norm ? 1 : 0) + (c.rev ? 1 : 0) + '|' + JSON.stringify(c.tune ?? null);

/**
 * Peaks to draw for an audio clip: the source's, or — when the clip has pitch edits — the
 * rendered result from `clip_peaks`. While a new render is pending the previous peaks stay
 * on screen (no flicker); requests are debounced and never sent during a gesture.
 */
export function getClipPeaks(c: Clip): Float32Array | null {
  if (tuneIsNeutral(c.tune)) { clipCache.delete(c.id); return getPeaks(c.ref, c.norm, c.rev); }
  const key = clipKey(c);
  let e = clipCache.get(c.id);
  if (!e) { e = { key: '', pk: null, want: '', timer: 0, busy: false, ref: c.ref, fails: 0, failed: '' }; clipCache.set(c.id, e); }
  e.ref = c.ref;
  if (e.key !== key && e.want !== key) {
    e.want = key;
    e.fails = 0;
    clearTimeout(e.timer);
    const ent = e, id = c.id;
    const fire = () => {
      if (gestureOpen() || ent.busy) { ent.timer = window.setTimeout(fire, 120); return; }
      const k = ent.want; ent.busy = true;
      be().clipPeaks(id)
        .then(p => { if (clipCache.get(id) === ent && ent.want === k) { ent.key = k; ent.pk = p; ent.fails = 0; setVersion(x => x + 1); } })
        .catch(err => {
          console.warn('[wave] clip_peaks', id, err);
          // a failed render is not the current one: try it again later, a bounded number of times
          if (clipCache.get(id) !== ent || ent.want !== k) return;
          ent.fails++;
          if (ent.fails >= RETRY_ATTEMPTS) { ent.failed = k; ent.key = ''; ent.pk = null; setVersion(x => x + 1); }
        })
        .finally(() => {
          ent.busy = false;
          // an entry dropped meanwhile (project closed, source relinked, edits undone) asks no more
          if (clipCache.get(id) !== ent || ent.want === ent.key || ent.want === ent.failed) return;
          ent.timer = window.setTimeout(fire, ent.fails ? retryDelay(ent.fails) : 60);
        });
    };
    e.timer = window.setTimeout(fire, 90);
  }
  return e.pk ?? getPeaks(c.ref, c.norm, c.rev);
}

/** True while the rendered peaks of a tuned clip lag behind its current edits (engine re-rendering). */
export function clipPeaksPending(c: Clip): boolean {
  if (tuneIsNeutral(c.tune)) return false;
  const e = clipCache.get(c.id);
  const key = clipKey(c);
  return !e || (e.key !== key && e.failed !== key);
}
