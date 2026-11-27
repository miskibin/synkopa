// Patina's curves, mirrored from crates/engine/src/dsp/patina.rs for the window and the mock.
import { clamp } from './format';

/** dsp::patina::scaled: module amounts after Magnitude — noise, wobble, drive, digital, wear, dropouts (0..1). */
export function patinaScaled(p: Record<string, number>): number[] {
  const m = clamp((p.amount ?? 100) / 100, 0, 1), v = (id: string) => clamp((p[id] ?? 0) / 100, 0, 1);
  return [v('noise') * m ** 1.5, v('wobble') * m ** 1.3, v('drive') * m, v('digital') * m ** 1.3, v('wear') * m, v('drops') * m * m];
}

/** dsp::patina::wow_depth: deepest pitch swing as a fraction of the speed. */
export const wowDepth = (a: number) => 0.025 * clamp(a, 0, 1) ** 1.5;

/** Fraction of speed → cents (1200 / ln 2 for small swings). */
export const toCents = (frac: number) => frac * 1731.234;

const TAPE_HZ = [11000, 15000, 19000];
/** dsp::patina::wear_cutoff: the head-loss low-pass at wear `w` (0..1) on `tape`, Hz. */
export function wearCutoff(w: number, tape: number, sr = 48000): number {
  const base = TAPE_HZ[clamp(Math.round(tape), 0, 2)] * 2 ** (-2.5 * w);
  const top = Math.min(sr * 0.45, 22000), t = clamp(w * 6, 0, 1);
  return Math.exp(Math.log(top) + (Math.log(base) - Math.log(top)) * t);
}

/** Clicks + pops per second of each noise type (dsp::patina TYPES), for the readout and the mock. */
export const CLICK_RATE = [74.2, 194.6, 142.2, 0, 0, 0];
