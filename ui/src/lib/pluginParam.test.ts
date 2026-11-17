import { beforeAll, describe, expect, it, vi } from 'vitest';
import { display, fromNormalized, lv2Param, sliderStep, steps, toNormalized, type PortSpec } from './pluginParam';
import { MockBackend } from '../api/mock';
import type { Fx } from '../api/types';

beforeAll(() => { vi.stubGlobal('window', {}); });

// the same cases as crates/lv2-host/src/params.rs: the mock must map like the engine
describe('LV2 parameter mapping', () => {
  const gain: PortSpec = { id: 3, symbol: 'gain', name: 'Gain', min: -24, max: 12, def: 0, unit: 'dB' };
  it('round-trips a linear range through normalized values', () => {
    expect(fromNormalized(gain, 0)).toBe(-24);
    expect(fromNormalized(gain, 1)).toBe(12);
    expect(toNormalized(gain, 0)).toBeCloseTo(24 / 36, 6);
    for (const v of [-24, -3.5, 0, 11]) expect(fromNormalized(gain, toNormalized(gain, v))).toBeCloseTo(v, 4);
    expect(fromNormalized(gain, NaN)).toBe(0);
    expect(display(gain, -6)).toBe('-6.00 dB');
  });
  it('maps a logarithmic frequency geometrically, and a log range through 0 linearly', () => {
    const f: PortSpec = { id: 0, symbol: 'f', name: 'Freq', min: 20, max: 20000, def: 1000, unit: 'Hz', log: true };
    expect(fromNormalized(f, 0.5)).toBeCloseTo(632.4555, 2);
    expect(toNormalized(f, 632.4555)).toBeCloseTo(0.5, 5);
    expect(fromNormalized({ ...f, min: 0, max: 100 }, 0.5)).toBe(50);
  });
  it('snaps integers, toggles and enumerations, and steps them a whole position at a time', () => {
    const int: PortSpec = { id: 1, symbol: 'n', name: 'N', min: 1, max: 8, def: 2, integer: true };
    expect(fromNormalized(int, 0.5)).toBe(5);
    expect(steps(int)).toBe(8);
    expect(fromNormalized(int, sliderStep({ steps: steps(int) }) * 3)).toBe(4);
    const tog: PortSpec = { id: 2, symbol: 't', name: 'T', min: 0, max: 1, def: 0, toggle: true };
    expect([fromNormalized(tog, 0.4), fromNormalized(tog, 0.6), display(tog, 1)]).toEqual([0, 1, 'On']);
    const mode: PortSpec = { id: 4, symbol: 'mode', name: 'Mode', min: 0, max: 2, def: 0, integer: true, options: [[2, 'Peak'], [0, 'Low shelf'], [1, 'High shelf']] };
    expect([0, 0.5, 1].map(n => fromNormalized(mode, n))).toEqual([0, 1, 2]);
    const p = lv2Param(mode, 2);
    expect(p.display).toBe('Peak');
    expect(p.options!.map(o => o.label)).toEqual(['Low shelf', 'High shelf', 'Peak']);
    expect(fromNormalized(mode, p.options![1].value)).toBe(1);
    expect(p.steps).toBe(3);
  });
});

describe('hosted plugins in the mock', () => {
  const lv2Fx = (id: number, cid: string): Fx => ({ id, type: 'plugin', bypass: false, preset: null, params: {}, plugin: { format: 'lv2', path: '/usr/lib/lv2/x.lv2', cid, name: 'X', vendor: 'Y' } });
  it('lists VST3 and LV2 effects, with the reason for one that cannot run', async () => {
    const be = new MockBackend();
    const { plugins } = await be.pluginScan(false);
    expect(plugins.some(p => p.format === 'vst3')).toBe(true);
    expect(plugins.some(p => p.format === 'lv2' && !p.unsupported)).toBe(true);
    expect(plugins.find(p => p.unsupported)?.unsupported).toMatch(/fixedBlockLength/);
  });
  it('reports LV2 parameters with real ranges and takes normalized values, like the engine', async () => {
    const be = new MockBackend();
    await be.projectNew();
    await be.applyOps([{ op: 'putFx', insertId: 0, fx: lv2Fx(9001, 'http://gareus.org/oss/lv2/dpl#stereo') }]);
    const ps = await be.pluginParams(9001);
    const gain = ps.find(p => p.symbol === 'gain')!;
    expect(gain).toMatchObject({ min: -10, max: 30, real: 0, unit: 'dB', display: '0 dB' });
    await be.pluginSetParam(9001, gain.id, 1);
    expect((await be.pluginParams(9001)).find(p => p.id === gain.id)).toMatchObject({ real: 30, value: 1, display: '30.0 dB' });
    await expect(be.pluginSetParam(9001, gain.id, 2)).rejects.toThrow(/0..1/);
    // an LV2 plugin with an interface of its own opens it under Synkopa's header, like the engine
    await expect(be.pluginEditor(9001, true, { title: 'Amp', subtitle: 'LV2 · Insert 1' })).resolves.toBeUndefined();
    expect(be.editorLabel(9001)).toEqual({ title: 'Amp', subtitle: 'LV2 · Insert 1' });
    await be.pluginEditor(9001, false);
    expect(be.editorLabel(9001)).toBeUndefined();
    // automation targets are checked against the plugin's parameters
    await be.applyOps([{ op: 'putAutomation', automation: { id: 9100, name: 'Gain', color: '#fff', target: { kind: 'plugin', fxId: 9001, paramId: gain.id }, points: [{ t: 0, v: 0.5 }] } }]);
    await expect(be.applyOps([{ op: 'putAutomation', automation: { id: 9101, name: 'X', color: '#fff', target: { kind: 'plugin', fxId: 9001, paramId: 999 }, points: [] } }]))
      .rejects.toThrow(/no plugin parameter 999/);
  });
});
