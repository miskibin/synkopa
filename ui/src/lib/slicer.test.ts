import { describe, expect, it } from 'vitest';
import { addSlice, dumpNotes, equalSlices, MAX_SLICES, moveSlice, onsetsFromPeaks, pickSlices, removeSlice, sliceAt, threshold } from './slicer';

describe('slicer', () => {
  const onsets = [
    { pos: 0.1, strength: 1 }, { pos: 0.25, strength: 0.3 }, { pos: 0.26, strength: 0.9 },
    { pos: 0.5, strength: 0.05 }, { pos: 0.75, strength: 0.6 },
  ];

  it('sensitivity keeps more candidates as it rises, the first slice always at 0', () => {
    const lo = pickSlices(onsets, 0, 4), mid = pickSlices(onsets, 0.5, 4), hi = pickSlices(onsets, 1, 4);
    expect(lo[0]).toBe(0);
    expect(lo.length).toBeLessThan(mid.length);
    expect(mid.length).toBeLessThanOrEqual(hi.length);
    expect(threshold(0)).toBeGreaterThan(threshold(1));
  });

  it('never leaves two slices closer than the minimum', () => {
    // 0.25 and 0.26 of a 1 s file are 10 ms apart: the later one is dropped
    const s = pickSlices(onsets, 1, 1);
    expect(s).toContain(0.25);
    expect(s).not.toContain(0.26);
  });

  it('equal slices, add, move between neighbours, remove (not the first)', () => {
    expect(equalSlices(4)).toEqual([0, 0.25, 0.5, 0.75]);
    expect(addSlice([0, 0.5], 0.25, 4)).toEqual([0, 0.25, 0.5]);
    expect(addSlice([0, 0.5], 0.501, 4)).toBeNull();
    expect(moveSlice([0, 0.25, 0.5], 1, 0.9, 4)[1]).toBeLessThan(0.5);
    expect(moveSlice([0, 0.25, 0.5], 0, 0.1, 4)).toEqual([0, 0.25, 0.5]);
    expect(removeSlice([0, 0.25, 0.5], 1)).toEqual([0, 0.5]);
    expect(removeSlice([0, 0.25], 0)).toEqual([0, 0.25]);
    expect(sliceAt([0, 0.25, 0.5], 0.3)).toBe(1);
    expect(equalSlices(1000).length).toBe(MAX_SLICES);
  });

  it('dumps notes in real time, one key per slice', () => {
    // a 2 s loop at 120 BPM is one bar: slice starts land on 16ths in seconds × 8
    const n = dumpNotes([0, 0.25, 0.5, 0.75], 2, 120, 7, 60);
    expect(n.map(x => x.step)).toEqual([0, 4, 8, 12]);
    expect(n.map(x => x.len)).toEqual([4, 4, 4, 4]);
    expect(n.map(x => x.p)).toEqual([60, 61, 62, 63]);
    expect(n.every(x => x.ch === 7 && x.trig === false)).toBe(true);
  });

  it('finds jumps in a peak envelope (mock detection)', () => {
    const pk = new Float32Array(200);
    for (let i = 0; i < 100; i++) { const lv = (i % 25 < 3 ? 0.9 : 0.05); pk[2 * i] = -lv; pk[2 * i + 1] = lv; }
    const o = onsetsFromPeaks(pk).filter(x => x.strength > 0.5);
    expect(o.length).toBe(3);
  });
});
