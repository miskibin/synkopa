// The same cases as `crates/engine/src/audio_midi.rs`'s tests: the mock's port must answer what the engine does.
import { describe, expect, it } from 'vitest';
import { MELODY_DEFAULTS, melodyNotes } from './audioMidi';

const h = (start: number, end: number, pitch: number, level: number) => ({ start, end, pitch, level });
const beat = (t: number) => 8 + t * 2;

describe('melodyNotes', () => {
  it('keeps timing, keys and accents as sung', () => {
    const n = melodyNotes([h(0.5, 1, 60.2, 0.5), h(1, 1.5, 61.6, 0.25)], [0, 10], beat, MELODY_DEFAULTS);
    expect(n.map(x => [x.step, x.len, x.p, x.v])).toEqual([[36, 4, 60, 1], [40, 4, 62, expect.closeTo(0.675, 1)]]);
  });
  it('leaves out blips, keys out of range and what the clip does not play', () => {
    const n = melodyNotes([h(0, 0.03, 60, 1), h(0.1, 0.6, 40, 1), h(0.7, 1.2, 64, 1), h(1.3, 2.5, 67, 1), h(3, 3.5, 69, 1)], [0, 2], beat, { ...MELODY_DEFAULTS, low: 48, high: 84 });
    expect(n.map(x => x.p)).toEqual([64, 67]);
    expect(n[1].step + n[1].len).toBe(beat(2) * 4);
  });
  it('quantizes without collapsing notes; two on one step keep the later', () => {
    const n = melodyNotes([h(0.02, 0.13, 60, 1), h(0.14, 0.2, 62, 1), h(0.26, 0.6, 64, 1)], [0, 10], beat, { ...MELODY_DEFAULTS, quantize: 1, shortest: 0, velocity: 0.8 });
    expect(n.map(x => [x.step, x.p])).toEqual([[32, 60], [33, 62], [34, 64]]);
    expect(n.every(x => x.len >= 1 && x.v === 0.8)).toBe(true);
    expect(melodyNotes([h(0, 0.05, 60, 1), h(0.05, 0.3, 62, 1)], [0, 10], beat, { ...MELODY_DEFAULTS, quantize: 4, shortest: 0 }).map(x => x.p)).toEqual([62]);
  });
  it('is monophonic and fine with nothing', () => {
    expect(melodyNotes([h(0, 1, 60, 1), h(0.5, 1, 62, 1)], [0, 10], beat, MELODY_DEFAULTS)[0].len).toBe(4);
    expect(melodyNotes([], [0, 1], beat, MELODY_DEFAULTS)).toEqual([]);
    expect(melodyNotes([h(0, 1, NaN, 1)], [0, 1], beat, MELODY_DEFAULTS)).toEqual([]);
  });
});
