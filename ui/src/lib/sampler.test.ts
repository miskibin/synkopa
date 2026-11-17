import { describe, expect, it } from 'vitest';
import { SAMPLER } from '../api/types';
import { samplerDefault, type SamplerState } from '../api/types';
import { attackFromT, attackToT, bend, GAP, curve, decayFromT, decayToT, delayFromT, envelopeGraph, holdFromT, holdToT, msText, releaseFromT, releaseToT } from './sampler';

describe('sampler knob scales', () => {
  it('cover the whole range and round-trip', () => {
    expect([attackFromT(0), attackFromT(1)]).toEqual([SAMPLER.attack.min, SAMPLER.attack.max]);
    expect([decayFromT(0), decayFromT(1)]).toEqual([SAMPLER.decay.min, SAMPLER.decay.max]);
    expect([releaseFromT(0), releaseFromT(1)]).toEqual([SAMPLER.release.min, SAMPLER.release.max]);
    expect([delayFromT(1), holdFromT(1)]).toEqual([SAMPLER.delay.max, SAMPLER.hold.max]);
    for (const ms of [0, 7, 250, 5000]) expect(holdFromT(holdToT(ms))).toBeCloseTo(ms, 0);
    for (const ms of [0, 1.5, 12, 80, 350, 2000]) expect(attackFromT(attackToT(ms))).toBeCloseTo(ms, 0);
    for (const ms of [0, 5, 120, 900, 5000]) expect(decayFromT(decayToT(ms))).toBeCloseTo(ms, 0);
    for (const ms of [1, 20, 90, 700, 5000]) expect(releaseFromT(releaseToT(ms))).toBeCloseTo(ms, 0);
    // out-of-range values (an old file, a script) still give a knob position
    expect(attackToT(9999)).toBe(1);
    expect(releaseToT(0)).toBe(0);
  });
  it('give the short times most of the travel', () => {
    expect(attackFromT(1 / 3)).toBeLessThan(80);
    expect(releaseFromT(0.5)).toBeLessThan(80);
  });
  it('print times', () => {
    expect([msText(0), msText(12.5), msText(350), msText(1200)]).toEqual(['0 ms', '12.5 ms', '350 ms', '1.20 s']);
  });
});

describe('envelope curve', () => {
  it('matches the engine: straight at 0, 8/9 or 1/9 half way at the extremes, ends fixed', () => {
    expect(curve(0.5, bend(0))).toBeCloseTo(0.5, 9);
    expect(curve(0.5, bend(-1))).toBeCloseTo(8 / 9, 9);
    expect(curve(0.5, bend(1))).toBeCloseTo(1 / 9, 9);
    for (const k of [-1, -0.3, 0, 0.7, 1]) expect([curve(0, bend(k)), curve(1, bend(k))]).toEqual([0, 1]);
    // out of range is clamped
    expect(bend(5)).toBe(8);
    expect(curve(2, 1)).toBe(1);
  });
});

describe('envelope graph', () => {
  const env = (patch: Partial<SamplerState>) => ({ ...samplerDefault(), env: true, ...patch });
  const W = 606, H = 112, PAD = 6, SLOT = (W - 2 * PAD) / 6;
  it('gives each stage a slot and places its point like its knob', () => {
    const g = envelopeGraph(env({}), W, H, PAD), travel = SLOT - GAP;
    expect(g.travel).toBeCloseTo(travel, 9);
    // all zero: every stage still takes GAP px, so no point hides another
    expect([g.points.delay.x, g.points.attack.x, g.points.hold.x, g.points.decay.x]).toEqual([PAD + GAP, PAD + 2 * GAP, PAD + 3 * GAP, PAD + 4 * GAP]);
    expect(g.noteOff).toBeCloseTo(PAD + 4 * GAP + SLOT, 9);
    expect(g.points.release.x).toBeCloseTo(g.noteOff + GAP + releaseToT(SAMPLER.release.def) * travel, 9);
    // a 2 s attack (its knob's maximum) fills its slot; the points after it move along
    const slow = envelopeGraph(env({ attack: SAMPLER.attack.max }), W, H, PAD);
    expect(slow.points.attack.x - slow.points.delay.x).toBeCloseTo(SLOT, 9);
    expect(slow.points.hold.x - g.points.hold.x).toBeCloseTo(travel, 9);
    // everything at its maximum fits the width exactly
    const all = envelopeGraph(env({ delay: SAMPLER.delay.max, attack: SAMPLER.attack.max, hold: SAMPLER.hold.max, decay: SAMPLER.decay.max, release: SAMPLER.release.max }), W, H, PAD);
    expect(all.points.release.x).toBeCloseTo(W - PAD, 9);
  });
  it('draws the sustain level and the levels at each point', () => {
    const g = envelopeGraph(env({ decay: 300, sustain: 0.25 }), W, H, PAD);
    const y = (level: number) => PAD + (1 - level) * (H - 2 * PAD);
    expect(g.points.attack.y).toBe(y(1));
    expect(g.points.decay.y).toBe(y(0.25));
    expect(g.points.release.y).toBe(y(0));
    expect(g.path.startsWith(`M${PAD.toFixed(2)},${y(0).toFixed(2)}`)).toBe(true);
    expect(g.path).toContain(`L${g.noteOff.toFixed(2)},${y(0.25).toFixed(2)}`);
    expect(g.path.endsWith(`${g.points.release.x.toFixed(2)},${y(0).toFixed(2)}`)).toBe(true);
  });
  it('offers a bend handle only on segments that can bend, at the level the engine plays half way', () => {
    const y = (level: number) => PAD + (1 - level) * (H - 2 * PAD);
    // no attack and a full sustain: only the release can be bent
    expect(envelopeGraph(env({ decay: 400 }), W, H, PAD).bends.map(b => b.stage)).toEqual(['release']);
    // sustain 0: nothing to release, but the attack and decay bend
    expect(envelopeGraph(env({ attack: 300, decay: 400, sustain: 0 }), W, H, PAD).bends.map(b => b.stage)).toEqual(['attack', 'decay']);
    const bent = envelopeGraph(env({ attack: 300, attackTension: 1 }), W, H, PAD).bends[0];
    expect(bent.stage).toBe('attack');
    expect(bent.y).toBeCloseTo(y(8 / 9), 9);
  });
});
