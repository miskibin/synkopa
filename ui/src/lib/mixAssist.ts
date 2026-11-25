// Ports of crates/engine/src/masking.rs for the in-browser mock: the fix a collision gets and
// which pairs are compared. The engine is the reference; mixAssist.test.ts pins the numbers.
import type { Catalog, Fx, Id, Insert, Project } from '../api/types';
import type { Collision } from '../api/telemetry';
import { newFx } from './presets';
import { reaches } from './routing';

/** `Collision::fix`: a bell over the range (at least an octave wide), ducking up to 6 dB once the
 *  protected insert is within 15 dB of its recent peak in the band. */
export function fixBand(c: Pick<Collision, 'lo' | 'hi' | 'keyDb'>) {
  const lo = Math.max(c.lo, 20), hi = Math.max(c.hi, c.lo + 1);
  const freq = Math.sqrt(lo * hi);
  const q = Math.min(1.4, Math.max(0.7, freq / (hi - lo)));
  const thr = Math.min(-3, Math.max(-80, c.keyDb - 15));
  return { freq, q, range: -6, thr };
}

/** `masking::fix_fx`: the EQ a fix adds to the masker's chain. */
export function fixFx(cat: Catalog, id: Id, c: Collision): Fx {
  const b = fixBand(c);
  const fx = newFx(cat, id, 'eq', 120);
  fx.preset = null;
  fx.key = c.protect;
  fx.bands = [{ on: true, type: 'peak', f: b.freq, g: 0, q: b.q, slope: 12, place: 'stereo', dynamic: { range: b.range, thr: b.thr } }];
  return fx;
}

/** `masking::keyed_from`: `m` already gives way to `key`: an active Compressor keyed from it, or
 *  an EQ keyed from it with a dynamic band (the fix). */
export const keyedEffects = (m: Insert, key: Id) =>
  m.fx.filter(f => !f.bypass && f.key === key
    && (f.type === 'comp' || (f.type === 'eq' && (f.bands ?? []).some(b => b.on && !!b.dynamic))));
export const keyedFrom = (m: Insert, key: Id) => keyedEffects(m, key).length > 0;

/** `masking::compared_pairs`, by id. */
export function comparedPairs(p: Project): [Id, Id][] {
  const out: [Id, Id][] = [];
  const ins = p.inserts.filter(m => m.id !== 0);
  for (let i = 0; i < ins.length; i++) for (let j = i + 1; j < ins.length; j++) {
    const [x, y] = [ins[i], ins[j]];
    if (reaches(p, x.id, y.id) || reaches(p, y.id, x.id)) continue;
    if ((p.maskIgnore ?? []).some(q => (q[0] === x.id && q[1] === y.id) || (q[0] === y.id && q[1] === x.id))) continue;
    out.push([x.id, y.id]);
  }
  return out;
}
