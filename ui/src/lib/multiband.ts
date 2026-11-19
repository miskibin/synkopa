// The Multiband compressor's gain computers (crates/engine/src/dsp/multiband.rs), for the
// in-browser mock and the window's labels.

export const BAND_IN_DB = 5.2;
export const MAKEUP_DB = [16.8, 12.2, 16.8];
const DOWN_SLOPE = [1 / 66.7 - 1, 1 / 66.7 - 1, -1];
const UP_SLOPE = 1 - 1 / 4.17;
const KNEE = 6;
const UP_CAP = 18;
const FLOOR = -60, FLOOR_FADE = 18;
/** Base attack / release (s) at Time = 100 %. */
export const ATTACK = [0.0478, 0.0224, 0.0135];
export const RELEASE = [0.282, 0.282, 0.132];

export function downGain(x: number, t: number, s: number): number {
  const o = x - t;
  if (2 * o <= -KNEE) return 0;
  if (2 * o < KNEE) { const u = o + KNEE / 2; return s * u * u / (2 * KNEE); }
  return s * o;
}

export function upGain(x: number, t: number): number {
  const o = t - x;
  if (2 * o <= -KNEE) return 0;
  if (2 * o < KNEE) { const u = o + KNEE / 2; return UP_SLOPE * u * u / (2 * KNEE); }
  return UP_SLOPE * o;
}

function floorWeight(x: number): number {
  const u = Math.min(1, Math.max(0, (x - (FLOOR - FLOOR_FADE)) / FLOOR_FADE));
  return u * u * (3 - 2 * u);
}

/** Target gain (dB) of band `b` at level `lvl` dB (after the band drive). */
export function bandGain(b: number, lvl: number, p: Record<string, number>): number {
  const ids = ['l', 'm', 'h'][b];
  const up = (p.up ?? 100) / 100, down = (p.down ?? 100) / 100;
  const g = downGain(lvl, p['d' + ids] ?? -33.8, DOWN_SLOPE[b]) * down;
  const u = Math.min(upGain(lvl, p['u' + ids] ?? -40.8) * up, UP_CAP * up) * floorWeight(lvl);
  return g + u;
}
