// The sampler's slice mode (crates/engine/src/model.rs `ChannelSrc::Sample::slices`): where the
// slices start, which candidates a Sensitivity keeps, and the notes "Dump to pattern" writes.
import type { Id, Note } from '../api/types';

/** model.rs MAX_SLICES. */
export const MAX_SLICES = 128;
/** Shortest slice a detection or an edit leaves, seconds. */
export const MIN_SLICE_S = 0.03;
/** Equal-division choices (MPC "chop" counts). */
export const EQUAL_COUNTS = [4, 8, 16, 32] as const;

/** A candidate slice point: position as a fraction of the file, strength 0..1. */
export interface Onset { pos: number; strength: number }

/** [pos, strength] pairs from the engine (`source_onsets`) → objects. */
export function onsetsFrom(pairs: Float32Array): Onset[] {
  const out: Onset[] = [];
  for (let i = 0; i + 1 < pairs.length; i += 2) out.push({ pos: pairs[i], strength: pairs[i + 1] });
  return out;
}

/**
 * The strength a candidate needs at `sensitivity` 0..1: 0 keeps only the strongest hits, 1 keeps
 * nearly everything (log scale, like a threshold knob in dB).
 */
export const threshold = (sensitivity: number) => 0.6 * Math.pow(0.012 / 0.6, Math.min(1, Math.max(0, sensitivity)));

/** Slice starts for a Sensitivity: 0, then every candidate strong enough and far enough from the last. */
export function pickSlices(onsets: Onset[], sensitivity: number, durS: number): number[] {
  const thr = threshold(sensitivity), minGap = durS > 0 ? MIN_SLICE_S / durS : 0;
  const out = [0];
  for (const o of onsets) {
    if (o.strength < thr || o.pos <= 0 || o.pos >= 1) continue;
    if (o.pos - out[out.length - 1] < minGap) continue;
    out.push(round(o.pos));
    if (out.length >= MAX_SLICES) break;
  }
  return out;
}

/** `n` slices of equal length. */
export const equalSlices = (n: number) => Array.from({ length: Math.min(MAX_SLICES, Math.max(1, n)) }, (_, i) => round(i / n));

const round = (x: number) => Math.round(x * 1e6) / 1e6;

/** Adds a slice at `pos` (kept sorted); null when it would be too close to another or there are too many. */
export function addSlice(slices: number[], pos: number, durS: number): number[] | null {
  if (slices.length >= MAX_SLICES || pos <= 0 || pos >= 1) return null;
  const minGap = durS > 0 ? MIN_SLICE_S / durS : 0;
  if (slices.some(s => Math.abs(s - pos) < minGap)) return null;
  return [...slices, round(pos)].sort((a, b) => a - b);
}

/** Moves slice `i` (not the first, which stays at the start) between its neighbours. */
export function moveSlice(slices: number[], i: number, pos: number, durS: number): number[] {
  if (i <= 0 || i >= slices.length) return slices;
  const minGap = durS > 0 ? MIN_SLICE_S / durS : 0;
  const lo = slices[i - 1] + minGap, hi = (slices[i + 1] ?? 1) - minGap;
  if (hi <= lo) return slices;
  const out = slices.slice();
  out[i] = round(Math.min(hi, Math.max(lo, pos)));
  return out;
}

/** Removes slice `i` (the first one stays: a slice always starts at the top of the file). */
export const removeSlice = (slices: number[], i: number) => (i <= 0 ? slices : slices.filter((_, k) => k !== i));

/** Index of the slice that holds position `pos`. */
export function sliceAt(slices: number[], pos: number): number {
  let k = 0;
  for (let i = 0; i < slices.length; i++) if (slices[i] <= pos) k = i;
  return k;
}

/**
 * "Dump to pattern" (FL Slicex "Dump score"): one note per slice at the time it sits in the file,
 * as long as the slice, on its key (root + index). Placed in real time at the project tempo, so
 * the pattern plays the sample back as it was whatever the tempo; reorder the notes to rechop it.
 */
export function dumpNotes(slices: number[], durS: number, bpm: number, ch: Id, root: number): Note[] {
  const stepsPerS = bpm / 60 * 4;
  return slices.map((s, i) => {
    const end = slices[i + 1] ?? 1;
    const step = Math.round(s * durS * stepsPerS * 1000) / 1000;
    const len = Math.max(0.05, Math.round((end - s) * durS * stepsPerS * 1000) / 1000);
    return { ch, step, len, p: root + i, trig: false };
  });
}

/**
 * Candidates from a waveform's peak envelope ([min, max] pairs, 100 a second), for the in-browser
 * mock: where the level jumps over what came just before. The engine detects from the spectrum.
 */
export function onsetsFromPeaks(peaks: Float32Array): Onset[] {
  const n = peaks.length >> 1;
  if (n < 4) return [];
  const e = new Float32Array(n);
  for (let i = 0; i < n; i++) e[i] = Math.max(Math.abs(peaks[2 * i]), Math.abs(peaks[2 * i + 1]));
  const d = new Float32Array(n);
  for (let i = 3; i < n; i++) d[i] = Math.max(0, e[i] - Math.max(e[i - 1], e[i - 2], e[i - 3]));
  let top = 0; for (const v of d) top = Math.max(top, v);
  if (top <= 1e-6) return [];
  const out: Onset[] = [];
  for (let i = 3; i < n; i++) {
    if (d[i] <= 0.01 * top) continue;
    let peak = true;
    for (let k = Math.max(0, i - 3); k <= Math.min(n - 1, i + 3); k++) if (d[k] > d[i]) peak = false;
    if (peak) out.push({ pos: (i - 1) / n, strength: d[i] / top });
  }
  return out;
}
