import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { encodeFrame, TelemetryDecoder, type Collision } from '../api/telemetry';
import { MockBackend } from '../api/mock';
import { mockCatalog } from '../api/mockData';
import type { Project } from '../api/types';
import { comparedPairs, fixBand, fixFx, keyedFrom } from './mixAssist';
import { newFx } from './presets';

// the collision the engine reports for the demo song's 808 over its kick (tests/mix_assistant.rs)
const KICK_808: Collision = { masker: 6, protect: 7, lo: 44.547726, hi: 56.126617, center: 50.00313, overlap: 0.9, keyDb: -12.712119 };

describe('mix assistant fix (port of masking.rs Collision::fix)', () => {
  it('is a bell at least an octave wide, ducking 6 dB from 15 dB under the key peak', () => {
    const b = fixBand(KICK_808);
    expect(b.freq).toBeCloseTo(Math.sqrt(44.547726 * 56.126617), 3);
    expect(b.q).toBe(1.4); // a third-octave collision still gets an octave-wide bell
    expect(b.range).toBe(-6);
    expect(b.thr).toBeCloseTo(-27.712119, 4);
    expect(fixBand({ lo: 200, hi: 1600, keyDb: -10 }).q).toBe(0.7);
    expect(fixBand({ lo: 100, hi: 200, keyDb: 0 }).thr).toBe(-15);
    expect(fixBand({ lo: 100, hi: 200, keyDb: -100 }).thr).toBe(-80);
  });

  it('makes an EQ keyed from the protected insert, one sidechained band', () => {
    const fx = fixFx(mockCatalog(), 500, KICK_808);
    expect(fx).toMatchObject({ id: 500, type: 'eq', bypass: false, key: 7, preset: null });
    expect(fx.bands).toHaveLength(1);
    expect(fx.bands![0]).toMatchObject({ on: true, type: 'peak', g: 0, dynamic: { range: -6 } });
  });
});

function project(): Project {
  const ins = (id: number, name: string, extra = {}) => ({ id, name, color: '#888', vol: 0.79, pan: 0, muted: false, solo: false, fx: [], ...extra });
  return {
    version: 3, bpm: 120, tracks: [], patterns: [], sources: [], clips: [], loop: null, nextId: 100,
    inserts: [ins(0, 'Master'), ins(1, 'Kick', { sends: [{ to: 3, gain: 1 }], master: false }), ins(2, '808'), ins(3, 'Drum Bus')],
  } as unknown as Project;
}

describe('compared pairs (port of masking.rs compared_pairs)', () => {
  it('skips related and ignored pairs but keeps existing sidechains', () => {
    const p = project();
    expect(comparedPairs(p)).toEqual([[1, 2], [2, 3]]);
    p.maskIgnore = [[2, 3]];
    expect(comparedPairs(p)).toEqual([[1, 2]]);
    p.inserts[2].fx.push(fixFx(mockCatalog(), 50, { ...KICK_808, masker: 2, protect: 1 }));
    expect(keyedFrom(p.inserts[2], 1)).toBe(true);
    expect(comparedPairs(p)).toEqual([[1, 2]]);
    p.inserts[2].fx[0].bypass = true;
    expect(comparedPairs(p)).toEqual([[1, 2]]);
  });

  it('counts a Compressor keyed from the other insert as handled (classic sidechain ducking), not a Gate', () => {
    const p = project();
    const comp = { ...newFx(mockCatalog(), 60, 'comp', 120), key: 1 };
    p.inserts[2].fx.push(comp);
    expect(keyedFrom(p.inserts[2], 1)).toBe(true);
    p.inserts[2].fx = [{ ...newFx(mockCatalog(), 61, 'gate', 120), key: 1 }];
    expect(keyedFrom(p.inserts[2], 1)).toBe(false);
  });
});

describe('telemetry: collisions', () => {
  const frame = (collisions: Collision[]) => encodeFrame({ seq: 1, beat: 0, playing: true, recording: false, previewing: false, inLevel: 0, previewPos: -1, cpu: 0, ins: [], fx: [], rec: null, collisions });

  it('round-trips and keeps the same array while nothing moved', () => {
    const d = new TelemetryDecoder();
    expect(d.decode(frame([KICK_808]))).toBe(true);
    const first = d.frame.collisions;
    expect(first).toHaveLength(1);
    expect(first[0].masker).toBe(6);
    expect(first[0].lo).toBeCloseTo(44.5477, 3);
    d.decode(frame([{ ...KICK_808, overlap: 0.95 }]));
    expect(d.frame.collisions).toBe(first); // same report: no new array, no UI churn
    expect(first[0].overlap).toBeCloseTo(0.95, 5);
    d.decode(frame([]));
    expect(d.frame.collisions).toHaveLength(0);
  });

  it('reads an older frame without the section as no collisions', () => {
    const d = new TelemetryDecoder();
    const buf = frame([]);
    expect(d.decode(buf.slice(0, buf.byteLength - 4))).toBe(true);
    expect(d.frame.collisions).toEqual([]);
  });
});

describe('MockBackend mix assistant', () => {
  beforeAll(() => { vi.stubGlobal('window', {}); });
  const created: MockBackend[] = [];
  afterEach(() => { for (const be of created.splice(0)) be.dispose(); });
  const mock = () => { vi.stubGlobal('window', { __waveFixture: project() }); const be = new MockBackend(); created.push(be); return be; };

  it('adds the fix as one undoable EQ and ignores pairs like the engine', async () => {
    const be = mock();
    const r = await be.mixAssist('unmask', { masker: 2, protect: 1, lo: 44.5, hi: 56.1, keyDb: -12 });
    let p = (await be.getState()).project;
    const fx = p.inserts.find(m => m.id === 2)!.fx.at(-1)!;
    expect(fx.id).toBe(r.fxId);
    expect(fx.key).toBe(1);
    await expect(be.mixAssist('unmask', { masker: 2, protect: 1, lo: 44.5, hi: 56.1, keyDb: -12 })).rejects.toThrow(/already has/);
    await be.undo();
    p = (await be.getState()).project;
    expect(p.inserts.find(m => m.id === 2)!.fx).toHaveLength(0);
    await be.mixAssist('mask_ignore', { a: 2, b: 1 });
    expect((await be.getState()).project.maskIgnore).toEqual([[1, 2]]);
    await expect(be.mixAssist('unmask', { masker: 0, protect: 1, lo: 40, hi: 50, keyDb: -12 })).rejects.toThrow(/not a mixer insert/);
    await expect(be.mixAssist('unmask', { masker: 2, protect: 1, lo: 60, hi: 50, keyDb: -12 })).rejects.toThrow(/lo < hi/);
    expect(await be.mixAssist('mix_assistant', { on: false })).toEqual({ on: false });
  });

  it('keys the fix through the standard sidechain: the same loop check, the same Sidechain picker command', async () => {
    const be = mock();
    // the 808 (2) sends into the kick (1): a key from the kick would close a loop
    await be.applyOps([{ op: 'putInsert', insert: { ...(await be.getState()).project.inserts[2], sends: [{ to: 1, gain: 1 }] } }]);
    await expect(be.mixAssist('unmask', { masker: 2, protect: 1, lo: 44.5, hi: 56.1, keyDb: -12 })).rejects.toThrow(/loop/);
    // an EQ is keyed and unkeyed by the same command as a Compressor
    const be2 = mock();
    const { fxId } = await be2.mixAssist('unmask', { masker: 2, protect: 1, lo: 44.5, hi: 56.1, keyDb: -12, label: 'Make room for Kick' }) as { fxId: number };
    await be2.sidechain({ fxId, off: true });
    expect((await be2.getState()).project.inserts[2].fx.at(-1)!.key).toBeUndefined();
    await be2.sidechain({ fxId, from: 1 });
    expect((await be2.getState()).project.inserts[2].fx.at(-1)!.key).toBe(1);
  });
});
