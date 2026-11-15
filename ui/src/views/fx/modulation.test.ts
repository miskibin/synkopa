import { describe, expect, it } from 'vitest';
import { mockCatalog } from '../../api/mockData';
import { loadPreset, newFx, pFmt } from '../../lib/presets';
import { chorusGeometry, phaserResponseDb } from './modulation';

// The phaser window draws the response the DSP makes (crates/engine/src/dsp/modulation.rs); the
// notches must land where the engine's test puts them: tan(πf/sr) = tan(πfc/sr)·tan(π/(2N)).
describe('phaser response curve', () => {
  it('cuts its first notch where N all-pass stages reach −180°', () => {
    for (const sr of [44100, 48000, 96000]) for (const n of [2, 4, 8]) {
      const fc = 1000;
      const notch = (sr / Math.PI) * Math.atan(Math.tan(Math.PI * fc / sr) * Math.tan(Math.PI / (2 * n)));
      expect(phaserResponseDb(notch, fc, n, 0, 0.5, sr)).toBeLessThan(-40);
      expect(Math.abs(phaserResponseDb(20, fc, n, 0, 0.5, sr))).toBeLessThan(0.3);
    }
  });
  it('turns notches into resonant peaks with feedback, and is flat when dry', () => {
    const peak = Math.max(...Array.from({ length: 400 }, (_, i) => phaserResponseDb(20 * Math.pow(1000, i / 399), 800, 4, 0.85, 1)));
    expect(peak).toBeGreaterThan(6);
    expect(Math.abs(phaserResponseDb(3000, 800, 12, 0.9, 0))).toBeLessThan(1e-6);
  });
});

describe('chorus voices', () => {
  const cat = mockCatalog();
  const fx = (preset: number) => loadPreset(cat, newFx(cat, 1, 'chorus', 120), preset, 120);
  it('has two voices in Chorus, three in Ensemble and one in Vibrato, like the engine', () => {
    const names = cat.presets.chorus!.map(p => p.name);
    const geo = (name: string) => chorusGeometry(fx(names.indexOf(name)));
    expect(geo('Klasyczny chorus')).toMatchObject({ voices: 2, centres: [12, 12 * 1.37] });
    expect(geo('Klasyczny chorus').swing).toBeCloseTo(0.4 * 0.25 * 12, 6);
    expect(geo('Smyczki — ensemble').voices).toBe(3);
    const vib = geo('Vibrato');
    expect(vib.voices).toBe(1);
    // ±6 % pitch at full depth: swing = 0.06·depth / (2π·rate) seconds
    expect(vib.swing).toBeCloseTo(0.06 * 0.35 / (2 * Math.PI * 5.5) * 1000, 6);
  });
  it('lists the presets of both effects, each different', () => {
    for (const kind of ['chorus', 'phaser'] as const) {
      const list = cat.presets[kind]!;
      expect(list.length).toBeGreaterThanOrEqual(6);
      expect(new Set(list.map(p => JSON.stringify(p.params))).size).toBe(list.length);
      const defs = cat.types[kind]!.params;
      for (const p of list) for (const [k, v] of Object.entries(p.params)) {
        const d = defs.find(x => x.id === k)!;
        expect(d, `${p.name}: ${k}`).toBeTruthy();
        expect(v).toBeGreaterThanOrEqual(d.min); expect(v).toBeLessThanOrEqual(d.max);
      }
    }
  });
});

describe('knob values', () => {
  it('keeps the decimals of slow LFO rates and shows degrees plainly', () => {
    const defs = mockCatalog().types.phaser!.params, d = (id: string) => defs.find(p => p.id === id)!;
    expect(pFmt(d('rate'), 0.6)).toBe('0.60 Hz');
    expect(pFmt(d('rate'), 7.25)).toBe('7.25 Hz');
    expect(pFmt(d('center'), 800)).toBe('800 Hz');
    expect(pFmt(d('center'), 1500)).toBe('1.50 kHz');
    expect(pFmt(d('stereo'), 90)).toBe('90°');
  });
});
