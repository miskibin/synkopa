// Synkopa Instruments knobs: each one's range, knob travel, text and default, shared by the window,
// automation and the mock so a value means the same everywhere (the engine's
// crates/engine/src/keys.rs constants and rt.rs `KeysKnobs::set`).
import { KEYS_ATTACK, KEYS_DYNAMICS, KEYS_NOISE, KEYS_RELEASE, KEYS_TONE, KEYS_TUNE, KEYS_WIDTH, type KeysParam, type KeysState } from '../api/types';
import { clamp } from './format';

/** The value a channel's knob has (absent fields are older projects: the default). */
export function keysValue(k: KeysState, p: KeysParam): number {
  switch (p) {
    case 'release': return k.release ?? KEYS_RELEASE.def;
    case 'dynamics': return k.dynamics;
    case 'attack': return k.attack ?? KEYS_ATTACK.def;
    case 'tone': return k.tone ?? KEYS_TONE.def;
    case 'width': return k.width ?? KEYS_WIDTH.def;
    case 'tune': return k.tune ?? KEYS_TUNE.def;
    case 'noise': return k.noise ?? KEYS_NOISE.def;
  }
}

const RANGE = { release: KEYS_RELEASE, dynamics: KEYS_DYNAMICS, attack: KEYS_ATTACK, tone: KEYS_TONE, width: KEYS_WIDTH, tune: KEYS_TUNE, noise: KEYS_NOISE } as const;
export const keysRange = (p: KeysParam) => RANGE[p];
export const keysDefault = (p: KeysParam) => RANGE[p].def;

/** Knob travel 0..1 of a value. Release is logarithmic over its range; attack over 1 ms .. 2 s with
 *  the bottom of the travel = none; the rest linear. */
export function keysNorm(p: KeysParam, x: number): number {
  const r = RANGE[p];
  if (p === 'release') return Math.log(clamp(x, r.min, r.max) / r.min) / Math.log(r.max / r.min);
  if (p === 'attack') return x < 1 ? 0 : clamp(Math.log(x) / Math.log(r.max), 0, 1);
  return (clamp(x, r.min, r.max) - r.min) / (r.max - r.min);
}

/** The value at knob travel `v`, exactly as the engine maps an automation (rt.rs); a knob rounds it. */
export function keysDenorm(p: KeysParam, v: number): number {
  const r = RANGE[p], t = clamp(v, 0, 1);
  if (p === 'release') return r.min * Math.pow(r.max / r.min, t);
  if (p === 'attack') return t <= 0 ? 0 : Math.pow(r.max, t);
  return r.min + (r.max - r.min) * t;
}

/** The value as the knob shows it. */
export function keysText(p: KeysParam, x: number): string {
  switch (p) {
    case 'release': case 'attack': return x >= 1000 ? (x / 1000).toFixed(2) + ' s' : Math.round(x) + ' ms';
    case 'tune': return (x > 0 ? '+' : x < 0 ? '−' : '') + Math.abs(Math.round(x)) + ' ct';
    case 'tone': return x === 0 ? '0' : (x > 0 ? '+' : '−') + Math.abs(Math.round(x));
    default: return Math.round(x) + ' %';
  }
}
