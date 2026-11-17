// The sampler's knob scales and its volume envelope's graph (pure; the sample editor draws them).
import { SAMPLER, type SamplerState } from '../api/types';

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
/** Delay, attack, hold and decay: cubic, so the first third of the travel covers the short times that matter (0..74 ms of 2 s). */
const cubicTo = (ms: number, max: number) => Math.cbrt(clamp01(ms / max));
const cubicFrom = (t: number, max: number) => { const ms = max * clamp01(t) ** 3; return ms < 100 ? Math.round(ms * 10) / 10 : Math.round(ms); };
export const delayToT = (ms: number) => cubicTo(ms, SAMPLER.delay.max);
export const delayFromT = (t: number) => cubicFrom(t, SAMPLER.delay.max);
export const attackToT = (ms: number) => cubicTo(ms, SAMPLER.attack.max);
export const attackFromT = (t: number) => cubicFrom(t, SAMPLER.attack.max);
export const holdToT = (ms: number) => cubicTo(ms, SAMPLER.hold.max);
export const holdFromT = (t: number) => cubicFrom(t, SAMPLER.hold.max);
export const decayToT = (ms: number) => cubicTo(ms, SAMPLER.decay.max);
export const decayFromT = (t: number) => cubicFrom(t, SAMPLER.decay.max);
/** Release: logarithmic, 1 ms .. 5 s. */
export const releaseToT = (ms: number) => clamp01(Math.log(Math.max(SAMPLER.release.min, ms) / SAMPLER.release.min) / Math.log(SAMPLER.release.max / SAMPLER.release.min));
export const releaseFromT = (t: number) => { const ms = SAMPLER.release.min * (SAMPLER.release.max / SAMPLER.release.min) ** clamp01(t); return ms < 100 ? Math.round(ms * 10) / 10 : Math.round(ms); };

/** "0 ms", "12.5 ms", "350 ms", "1.20 s". */
export function msText(ms: number): string {
  if (ms >= 1000) return (ms / 1000).toFixed(2) + ' s';
  return (ms < 100 && ms % 1 ? ms.toFixed(1) : String(Math.round(ms))) + ' ms';
}

/** The timed stages of the envelope, in order, with the knob scale each one's handle moves on. */
export const STAGES = [
  { key: 'delay', toT: delayToT, fromT: delayFromT },
  { key: 'attack', toT: attackToT, fromT: attackFromT },
  { key: 'hold', toT: holdToT, fromT: holdFromT },
  { key: 'decay', toT: decayToT, fromT: decayFromT },
  { key: 'release', toT: releaseToT, fromT: releaseFromT },
] as const;
export type Stage = typeof STAGES[number]['key'];
export type Bent = 'attack' | 'decay' | 'release';
export const TENSION_KEY = { attack: 'attackTension', decay: 'decayTension', release: 'releaseTension' } as const;

/** A tension −1..1 as the engine's bend factor (`crates/engine/src/synth.rs` `bend`). */
export const bend = (tension: number) => 8 ** Math.min(1, Math.max(-1, tension));
/** A segment's progress 0..1 at `x` 0..1 of its time, bent by `g` (the engine's `curve`). */
export const curve = (x: number, g: number) => { const u = clamp01(x); return u / (u + g * (1 - u)); };

/** The width a stage at zero still takes in the graph. */
export const GAP = 9;

export interface EnvelopeGraph {
  /** SVG path of the envelope from note-on through its release. */
  path: string;
  /** The end of each timed stage: its draggable point. */
  points: Record<Stage, { x: number; y: number }>;
  /** x of the note-off: the sustain lasts up to here, the release starts here. */
  noteOff: number;
  /** Handles in the middle of the curved segments that can be bent (a flat or empty one cannot). */
  bends: { stage: Bent; x: number; y: number }[];
  /** Pixels a stage's point travels over its whole knob range. */
  travel: number;
}

/**
 * The envelope as FL Studio's Channel settings draw it: each timed stage gets an equal slot of
 * the width and its point sits where its knob is in that slot, so dragging a point moves exactly
 * like turning its knob; the sustain is a fixed stretch up to the note-off. A stage at zero still
 * takes `GAP` px, so no point ever hides another and every one can be grabbed. Levels follow the
 * curve the engine plays. `w`×`h` is the drawing area, `pad` the margin kept for the handles.
 */
export function envelopeGraph(s: SamplerState, w: number, h: number, pad = 6): EnvelopeGraph {
  const slot = Math.max(GAP + 1, (w - 2 * pad) / 6), travel = slot - GAP;
  const after = (x: number, t: number) => x + GAP + t * travel;
  const y = (level: number) => pad + (1 - level) * (h - 2 * pad);
  const x0 = pad;
  const xDelay = after(x0, delayToT(s.delay));
  const xAttack = after(xDelay, attackToT(s.attack));
  const xHold = after(xAttack, holdToT(s.hold));
  const xDecay = after(xHold, decayToT(s.decay));
  const noteOff = xDecay + slot;
  const xRelease = after(noteOff, releaseToT(s.release));
  const sus = s.sustain;
  const f = (v: number) => v.toFixed(2);
  const segment = (xa: number, xb: number, level: (p: number) => number) => {
    const n = Math.max(2, Math.min(32, Math.ceil((xb - xa) / 3)));
    let d = '';
    for (let i = 1; i <= n; i++) d += ` L${f(xa + (xb - xa) * i / n)},${f(y(level(i / n)))}`;
    return d;
  };
  const ga = bend(-s.attackTension), gd = bend(s.decayTension), gr = bend(s.releaseTension);
  const attack = (p: number) => curve(p, ga);
  const decay = (p: number) => 1 - (1 - sus) * curve(p, gd);
  const release = (p: number) => sus * (1 - curve(p, gr));
  const path = `M${f(x0)},${f(y(0))} L${f(xDelay)},${f(y(0))}`
    + (xAttack > xDelay ? segment(xDelay, xAttack, attack) : ` L${f(xAttack)},${f(y(1))}`)
    + ` L${f(xHold)},${f(y(1))}`
    + (xDecay > xHold ? segment(xHold, xDecay, decay) : ` L${f(xDecay)},${f(y(sus))}`)
    + ` L${f(noteOff)},${f(y(sus))}` + segment(noteOff, xRelease, release);
  const bends: EnvelopeGraph['bends'] = [];
  // only a segment that takes time and changes level can be bent (anything else would be a dead handle)
  const mid = (stage: Bent, xa: number, xb: number, level: (p: number) => number, bendable: boolean) => {
    if (bendable) bends.push({ stage, x: (xa + xb) / 2, y: y(level(0.5)) });
  };
  mid('attack', xDelay, xAttack, attack, s.attack > 0);
  mid('decay', xHold, xDecay, decay, s.decay > 0 && sus < 1);
  mid('release', noteOff, xRelease, release, sus > 0);
  return {
    path, noteOff, bends, travel,
    points: {
      delay: { x: xDelay, y: y(0) }, attack: { x: xAttack, y: y(1) }, hold: { x: xHold, y: y(1) },
      decay: { x: xDecay, y: y(sus) }, release: { x: xRelease, y: y(0) },
    },
  };
}
