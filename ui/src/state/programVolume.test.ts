import { describe, expect, it, vi } from 'vitest';

const calls: [number, boolean][] = [];
vi.mock('../api/backend', () => ({
  be: () => ({
    programVolume: async () => 0.5,
    setProgramVolume: async (v: number, save: boolean) => { calls.push([v, save]); return v; },
  }),
}));

const { VOL_UNITY } = await import('../api/types');
const { programVolume, setProgramVolume } = await import('./programVolume');

describe('program volume (the toolbar main volume)', () => {
  it('plays at once and is saved once, after the knob settles', () => {
    vi.useFakeTimers();
    try {
      setProgramVolume(0.3);
      setProgramVolume(0.2);
      expect(programVolume()).toBe(0.2);
      expect(calls).toEqual([[0.3, false], [0.2, false]]);
      vi.advanceTimersByTime(1000);
      expect(calls.at(-1)).toEqual([0.2, true]);
      expect(calls.filter(c => c[1]).length).toBe(1);
    } finally { vi.useRealTimers(); }
  });

  it('clamps to the knob range and falls back to 0 dB', () => {
    setProgramVolume(7);
    expect(programVolume()).toBe(1);
    setProgramVolume(NaN);
    expect(programVolume()).toBe(VOL_UNITY);
  });
});
