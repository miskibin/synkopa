import { describe, expect, it } from 'vitest';
import { mockCatalog } from '../../api/mockData';
import type { AnyBuiltinKind, ParamDef, Preset } from '../../api/types';
import { pFmt } from '../../lib/presets';
import engine from '../../lib/__fixtures__/fx-catalog.json';
import { djMap, filterResponseDb, svfQ } from './filter';
import { gateCurve } from './gateCurve';

// What the engine serves (crates/engine/tests/ui_fx_catalog_fixture.rs) against the mock's copy.
const eng = engine as unknown as { types: Record<string, { name: string; params: ParamDef[] }>; presets: Record<string, Preset[]> };

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
