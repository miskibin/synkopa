import { describe, expect, it } from 'vitest';
import { mockCatalog } from '../../api/mockData';
import type { AnyBuiltinKind, ParamDef, Preset } from '../../api/types';
import { pFmt } from '../../lib/presets';
import engine from '../../lib/__fixtures__/fx-catalog.json';
import { filterEnvStep, gateStep, type GateSim } from '../../api/mockGateFilter';
import { automatedKeys, setsLatency } from '../../lib/automation';
import type { Project } from '../../api/types';
import { djMap, filterResponseDb, svfQ } from './filter';
import { gateCurve } from './gateCurve';

// What the engine serves (crates/engine/tests/ui_fx_catalog_fixture.rs) against the mock's copy.
const eng = engine as unknown as { types: Record<string, { name: string; params: ParamDef[] }>; presets: Record<string, Preset[]>; latencyParams: string[]; order: string[] };

describe('mock catalog', () => {
  const cat = mockCatalog();
  for (const kind of ['gate', 'filter'] as AnyBuiltinKind[]) {
    it(`mirrors the engine's ${kind}: parameters and presets`, () => {
      const strip = (p: ParamDef) => ({ ...p, options: p.options ?? undefined });
      expect(cat.types[kind]!.name).toBe(eng.types[kind].name);
      expect(cat.types[kind]!.params.map(strip)).toEqual(eng.types[kind].params.map(strip));
      const norm = (ps: Preset[]) => ps.map(p => ({ name: p.name, params: Object.fromEntries(Object.entries(p.params).sort()) }));
      expect(norm(cat.presets[kind]!)).toEqual(norm(eng.presets[kind]));
    });
  }
});

// The filter window draws the engine's response (crates/engine/src/dsp/filter.rs); these are the
// numbers the engine's own tests measure on the running DSP.
describe('filter response curve', () => {
  const sr = 48000;
  it('puts a Butterworth-like 12 dB low-pass at −3 dB on its cutoff, 12 or 24 dB per octave above', () => {
    const res = 0.07143; // Q ≈ 0.707
    expect(svfQ(res)).toBeCloseTo(0.707, 2);
    expect(filterResponseDb(1000, 1000, 0, res, false, sr)).toBeCloseTo(-3.01, 1);
    // a little steeper than 12 dB near Nyquist: the bilinear transform squeezes the top octave
    const oct12 = filterResponseDb(4000, 1000, 0, res, false, sr) - filterResponseDb(8000, 1000, 0, res, false, sr);
    expect(oct12).toBeGreaterThan(11.5);
    expect(oct12).toBeLessThan(13.5);
    expect(filterResponseDb(4000, 1000, 0, res, true, sr) - filterResponseDb(8000, 1000, 0, res, true, sr)).toBeGreaterThan(22);
    expect(Math.abs(filterResponseDb(100, 1000, 0, res, true, sr))).toBeLessThan(0.2);
  });
  it('cuts a deep notch, peaks the band-pass and the resonant ladder at the cutoff', () => {
    expect(filterResponseDb(1000, 1000, 4, 0.3, false, sr)).toBeLessThan(-60);
    const bp = filterResponseDb(1000, 1000, 3, 0.5, false, sr);
    expect(bp).toBeGreaterThan(filterResponseDb(250, 1000, 3, 0.5, false, sr) + 15);
    const lad = (f: number) => filterResponseDb(f, 1000, 1, 0.85, true, sr);
    expect(lad(1000)).toBeGreaterThan(lad(200) + 6);
    // passband compensation: (1 + 0.5k) / (1 + k) at DC
    const k = 4.15 * 0.85;
    expect(lad(20)).toBeCloseTo(20 * Math.log10((1 + 0.5 * k) / (1 + k)), 1);
  });
  it('maps the DJ knob: off in the middle, low-pass left, high-pass right', () => {
    expect(djMap(0).wet).toBe(0);
    expect(djMap(0.005).wet).toBe(0);
    expect(djMap(-1).fc).toBeCloseTo(20, 0);
    expect(djMap(1).fc).toBeCloseTo(20000, -2);
    expect(djMap(-0.5).mode).toBe(0);
    expect(djMap(0.5).mode).toBe(2);
    expect(filterResponseDb(5000, 1000, 5, 0.3, true, sr, 0)).toBeCloseTo(0, 6);
  });
});

describe('gate transfer curve', () => {
  it('passes above the close threshold, drops to the floor (or follows the ratio) below it', () => {
    const p = { thr: -30, hyst: 4, ratio: 100, floor: -20 };
    expect(gateCurve(p, -10)).toBe(-10);
    expect(gateCurve(p, -40)).toBe(-60);
    expect(gateCurve({ ...p, ratio: 2 }, -44)).toBe(-44 - 10);
    expect(gateCurve({ ...p, floor: -80 }, -50)).toBe(-Infinity);
    // ducking mirrors it: loud is turned down by the floor, quiet passes
    expect(gateCurve({ ...p, mode: 1 }, -10)).toBe(-30);
    expect(gateCurve({ ...p, mode: 1 }, -40)).toBe(-40);
  });
  it('shows the ends of the gate knobs as words, not numbers', () => {
    const cat = mockCatalog(), d = (id: string) => cat.types.gate!.params.find(p => p.id === id)!;
    expect(pFmt(d('ratio'), 100)).toBe('∞:1');
    expect(pFmt(d('floor'), -80)).toBe('−∞ dB');
    expect(pFmt(d('schp'), 20)).toBe('Off');
    expect(pFmt(d('sclp'), 20000)).toBe('Off');
    const f = (id: string) => cat.types.filter!.params.find(p => p.id === id)!;
    expect(pFmt(f('dj'), 0)).toBe('Off');
    expect(pFmt(f('dj'), -40)).toBe('LP 40%');
    expect(pFmt(f('lfo'), 1.5)).toBe('+1.50 oct');
  });
});

describe('latency parameters', () => {
  it("are the engine's list (catalog::sets_latency): automation leaves them alone", () => {
    const ui = eng.order.flatMap(k => eng.types[k].params.filter(p => setsLatency(k, p.id)).map(p => k + '.' + p.id));
    expect(ui.sort()).toEqual([...eng.latencyParams].sort());
  });
  it("an old automation of the gate's look-ahead does not drive the control", () => {
    const p = {
      inserts: [{ id: 1, fx: [{ id: 7, type: 'gate', params: { look: 0 } }] }],
      clips: [{ id: 1, kind: 'auto', ref: 3 }, { id: 2, kind: 'auto', ref: 4 }],
      automations: [{ id: 3, target: { kind: 'fx', fxId: 7, param: 'look' }, points: [] }, { id: 4, target: { kind: 'fx', fxId: 7, param: 'thr' }, points: [] }],
    } as unknown as Project;
    expect([...automatedKeys(p)]).toEqual(['fx:7:thr']);
  });
});

describe("the mock's gate and filter", () => {
  it('holds the gate open for its hold time after the level drops', () => {
    const pr = { thr: -30, hyst: 4, hold: 100, rel: 50, att: 1 };
    const run = (hold: number) => {
      const s: GateSim = { state: 0, hold: 0, gr: -120 };
      gateStep({ ...pr, hold }, -10, 1 / 30, s);
      const states: number[] = [];
      for (let i = 0; i < 10; i++) { gateStep({ ...pr, hold }, -60, 1 / 60, s); states.push(s.state); }
      return states;
    };
    // 100 ms of hold at 60 frames/s: six frames in hold before it starts closing
    expect(run(100).slice(0, 6).every(st => st === 2)).toBe(true);
    expect(run(100)[7]).not.toBe(2);
    expect(run(0)[0]).toBe(3);
  });
  it("moves the filter's envelope at its attack and release", () => {
    const fast = filterEnvStep({ eatt: 1, erel: 1000 }, 0, 0.01, 0);
    const slow = filterEnvStep({ eatt: 200, erel: 1000 }, 0, 0.01, 0);
    expect(fast).toBeGreaterThan(0.99);
    expect(slow).toBeLessThan(0.1);
    expect(filterEnvStep({ eatt: 1, erel: 10 }, -120, 0.05, 1)).toBeLessThan(0.01);
    expect(filterEnvStep({ eatt: 1, erel: 1000 }, -120, 0.05, 1)).toBeGreaterThan(0.9);
  });
  it("shows −∞ only at the gate's floor, not the vocal rider's noise floor", () => {
    const cat = mockCatalog();
    const rider = cat.types.rider!.params.find(p => p.id === 'floor')!;
    expect(pFmt(rider, -90)).not.toContain('∞');
  });
});
