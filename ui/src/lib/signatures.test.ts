import { describe, expect, it } from 'vitest';
import { TOTAL_BEATS } from '../api/types';
import {
  addSigPoint, barAt, barBeatsAt, barLines, beatAtBar, compiledSig, deleteSigPoint, isBarStart,
  moveSigPoint, parseSig, setSigMeter, sigAt, snapToBar, type SigPoint,
} from './signatures';

const pts = (list: [number, number, number][]): SigPoint[] => list.map(([beat, num, den]) => ({ beat, num, den }));

describe('compiledSig', () => {
  it('leaves a 4/4 project empty and the opener governs', () => {
    expect(compiledSig(undefined)).toEqual([]);
    expect(compiledSig([{ beat: 0, num: 4, den: 4 }])).toEqual([{ beat: 0, num: 4, den: 4 }]);
  });

  it('clamps num, snaps den, sorts and folds ties to the last', () => {
    expect(compiledSig(pts([[8, 250, 4], [0, 7, 9], [16, 5, 4]]))).toEqual([
      { beat: 0, num: 7, den: 4 },
      { beat: 8, num: 32, den: 4 },
      { beat: 16, num: 5, den: 4 },
    ]);
    expect(compiledSig(pts([[4, 3, 4], [4, 7, 8]]))).toEqual([{ beat: 4, num: 7, den: 8 }]);
  });
});

describe('bar conversions', () => {
  it('the constant map is the old bar math', () => {
    const m = compiledSig([]);
    expect(barAt(m, 7.9)).toBe(1);
    expect(barAt(m, 8)).toBe(2);
    expect(beatAtBar(m, 2)).toBe(8);
    expect(barBeatsAt(m, 123)).toBe(4);
    expect(isBarStart(m, 4)).toBe(true);
    expect(isBarStart(m, 5)).toBe(false);
  });

  it('a change reflows the bars from its beat', () => {
    const m = compiledSig(pts([[8, 3, 4]]));
    expect(barAt(m, 7.999)).toBe(1);
    expect(barAt(m, 8)).toBe(2);
    expect(beatAtBar(m, 3)).toBe(11);
    expect(barBeatsAt(m, 7)).toBe(4);
    expect(barBeatsAt(m, 8)).toBe(3);
  });

  it('a mid-bar change truncates the bar before it and the fragment counts', () => {
    const m = compiledSig(pts([[10, 3, 4]]));
    expect(barAt(m, 9)).toBe(2);
    expect(barAt(m, 10)).toBe(3);
    expect(beatAtBar(m, 2)).toBe(8);
    expect(beatAtBar(m, 3)).toBe(10);
    expect(beatAtBar(m, 4)).toBe(13);
  });

  it('7/8 bars start between beats, exactly', () => {
    const m = compiledSig(pts([[0, 7, 8]]));
    expect(barBeatsAt(m, 4)).toBe(3.5);
    expect(beatAtBar(m, 1)).toBe(3.5);
    expect(beatAtBar(m, 2)).toBe(7);
    expect(barAt(m, 3.6)).toBe(1);
    expect(isBarStart(m, 3.5)).toBe(true);
    expect(isBarStart(m, 4)).toBe(false);
  });

  it('negative beats continue backwards on the opening grid', () => {
    expect(barAt(compiledSig([]), -4)).toBe(-1);
    expect(beatAtBar(compiledSig([]), -1)).toBe(-4);
    expect(isBarStart(compiledSig([]), -4)).toBe(true);
    expect(beatAtBar(compiledSig(pts([[0, 7, 8]])), -1)).toBe(-3.5);
  });

  it('bar numbers round-trip across changes', () => {
    const m = compiledSig(pts([[0, 5, 4], [12, 7, 8], [30, 3, 4]]));
    for (let bar = -3; bar < 40; bar++) {
      expect(barAt(m, beatAtBar(m, bar) + 1e-9)).toBe(bar);
    }
  });

  it('barLines walks the boundaries, unevenly spaced', () => {
    const m = compiledSig(pts([[0, 3, 4], [9, 7, 8]]));
    expect(barLines(m, 0, 16)).toEqual([
      { beat: 0, bar: 0 }, { beat: 3, bar: 1 }, { beat: 6, bar: 2 }, { beat: 9, bar: 3 }, { beat: 12.5, bar: 4 },
    ]);
    expect(barLines(m, 0, 16, 2)).toEqual([{ beat: 0, bar: 0 }, { beat: 6, bar: 2 }, { beat: 12.5, bar: 4 }]);
    // the boundary at or before b0 is included (a line just left of the view keeps its number)
    expect(barLines(m, 10.5, 16)).toEqual([{ beat: 9, bar: 3 }, { beat: 12.5, bar: 4 }]);
    expect(barLines(m, 16.5, 20)).toEqual([{ beat: 16, bar: 5 }, { beat: 19.5, bar: 6 }]);
  });

  it('snapToBar lands on the nearest boundary, even off the beat', () => {
    const m = compiledSig(pts([[0, 7, 8]]));
    expect(snapToBar(m, 3.2)).toBe(3.5);
    expect(snapToBar(m, 3.4)).toBe(3.5);
    expect(snapToBar(m, 1.0)).toBe(0);
    expect(snapToBar(compiledSig([]), 5.6)).toBe(4);
  });
});

describe('editing', () => {
  it('add places after same-beat points and copies the rest', () => {
    const { points, i } = addSigPoint(pts([[4, 3, 4]]), 4, 7, 8);
    expect(points).toEqual([{ beat: 4, num: 3, den: 4 }, { beat: 4, num: 7, den: 8 }]);
    expect(i).toBe(1);
    expect(addSigPoint([], -3, 5, 4).points[0].beat).toBe(0);
  });

  it('move stays between neighbours and at the song bounds', () => {
    const m = pts([[4, 3, 4], [12, 7, 8]]);
    expect(moveSigPoint(m, 1, 2)[1].beat).toBe(4.000001);
    expect(moveSigPoint(m, 1, 9999)[1].beat).toBe(2048);
    expect(moveSigPoint(m, 0, -5)[0].beat).toBe(0);
    // tied neighbours: the beat stays put
    const tied = pts([[4, 3, 4], [4, 7, 8]]);
    expect(moveSigPoint(tied, 0, 10)[0].beat).toBe(4);
  });

  it('delete, setMeter and parseSig', () => {
    const m = pts([[0, 7, 8], [7, 3, 4]]);
    expect(deleteSigPoint(m, 0)).toEqual([{ beat: 7, num: 3, den: 4 }]);
    expect(setSigMeter(m, 1, 99, 3)).toEqual([{ beat: 0, num: 7, den: 8 }, { beat: 7, num: 32, den: 4 }]);
    expect(setSigMeter(m, 1, 6, 8)[1]).toEqual({ beat: 7, num: 6, den: 8 });
    expect(parseSig('7/8')).toEqual({ num: 7, den: 8 });
    expect(parseSig(' 3 / 4 ')).toEqual({ num: 3, den: 4 });
    expect(parseSig('7/7')).toBe(null);
    expect(parseSig('40/4')).toBe(null);
    expect(parseSig('x')).toBe(null);
    expect(sigAt([], 99)).toEqual({ beat: 0, num: 4, den: 4 });
  });
});

describe('bar starts always advance', () => {
  it('maps every bar index past the song end to a later beat (no clamp: walks terminate)', () => {
    const sixteenth: SigPoint[] = [{ beat: 0, num: 1, den: 16 }];
    const fragments: SigPoint[] = Array.from({ length: 1024 }, (_, i) => ({ beat: i * 1.3 + 0.1, num: 1, den: 16 }));
    for (const m of [sixteenth, compiledSig(fragments)]) {
      const last = barAt(m, TOTAL_BEATS) + 16;
      let prev = -Infinity;
      for (let k = -2; k <= last; k += 97) {
        const b = beatAtBar(m, k);
        expect(b).toBeGreaterThan(prev);
        expect(barAt(m, b)).toBe(k);
        prev = b;
      }
      expect(beatAtBar(m, last)).toBeGreaterThan(TOTAL_BEATS);
      const lines = barLines(m, TOTAL_BEATS - 1, TOTAL_BEATS + 1);
      expect(lines.length).toBeGreaterThan(0);
      expect(lines.length).toBeLessThan(20);
    }
  });
  it('treats points a sliver apart as one change, like the engine', () => {
    const m = compiledSig([{ beat: 4, num: 3, den: 4 }, { beat: 4 + 1e-7, num: 7, den: 8 }]);
    expect(barBeatsAt(m, 4)).toBe(3.5);
    expect(barAt(m, 4)).toBe(1);
    expect(beatAtBar(m, 2)).toBe(7.5);
  });
});
