import { describe, expect, it } from 'vitest';
import { TOTAL_BEATS, type TempoPoint } from '../api/types';
import { addTempoPoint, bpmAt, bpmLabel, compiledTempo, deleteTempoPoint, dedupeTempo, hitTempo, moveTempoPoint, setTempoBpm, setTempoRamp, tempoGeom, tempoScale, tempoSegments } from './tempoLane';

const pt = (beat: number, bpm: number, ramp = false): TempoPoint => ({ beat, bpm, ramp });

describe('bpmAt', () => {
  it('plays the base tempo before the first point and holds the last one beyond the map', () => {
    const pts = [pt(8, 60)];
    expect(bpmAt(pts, 120, 0)).toBe(120);
    expect(bpmAt(pts, 120, 7.5)).toBe(120);
    expect(bpmAt(pts, 120, 8)).toBe(60);
    expect(bpmAt(pts, 120, 100)).toBe(60);
    expect(bpmAt(undefined, 120, 4)).toBe(120);
  });

  it('glides a ramp linearly in beats', () => {
    const pts = [pt(0, 60, true), pt(8, 120)];
    expect(bpmAt(pts, 120, 0)).toBe(60);
    expect(bpmAt(pts, 120, 4)).toBe(90);
    expect(bpmAt(pts, 120, 8)).toBe(120);
    expect(bpmAt(pts, 120, 20)).toBe(120);
  });

  it('plays the last of two points at one beat (the engine compiles them the same way)', () => {
    const pts = [pt(8, 90), pt(8, 60)];
    expect(bpmAt(pts, 120, 8)).toBe(60);
    expect(bpmAt(pts, 120, 4)).toBe(120);
  });
});

describe('tempoSegments', () => {
  it('is one flat piece at the base tempo for an empty map', () => {
    expect(tempoSegments(undefined, 120, 16)).toEqual([{ a: 0, b: 16, va: 120, vb: 120 }]);
  });

  it('cuts flats at points and slants a ramp to its target', () => {
    expect(tempoSegments([pt(8, 60)], 120, 16)).toEqual([{ a: 0, b: 8, va: 120, vb: 120 }, { a: 8, b: 16, va: 60, vb: 60 }]);
    expect(tempoSegments([pt(0, 60, true), pt(8, 120)], 120, 16)).toEqual([{ a: 0, b: 8, va: 60, vb: 120 }, { a: 8, b: 16, va: 120, vb: 120 }]);
    expect(tempoSegments([pt(4, 90, true), pt(8, 60)], 120, 16)).toEqual([{ a: 0, b: 4, va: 120, vb: 120 }, { a: 4, b: 8, va: 90, vb: 60 }, { a: 8, b: 16, va: 60, vb: 60 }]);
  });

  it('a ramp on the last point has nothing to glide to: it holds', () => {
    expect(tempoSegments([pt(4, 90, true)], 120, 16)).toEqual([{ a: 0, b: 4, va: 120, vb: 120 }, { a: 4, b: 16, va: 90, vb: 90 }]);
  });

  it('a ramp glides toward the next STORED point, even when duplicates follow it', () => {
    // the engine glides 60 -> 90 (the next stored entry), then jumps to 120 at beat 8 (last wins)
    const pts = [pt(0, 60, true), pt(8, 90), pt(8, 120)];
    expect(tempoSegments(pts, 120, 16)).toEqual([{ a: 0, b: 8, va: 60, vb: 90 }, { a: 8, b: 16, va: 120, vb: 120 }]);
    // what the strip draws at the midpoint is what the readout plays
    expect(bpmAt(pts, 120, 4)).toBe(75);
    const segs = tempoSegments(pts, 120, 16);
    expect(segs[0].va + (segs[0].vb - segs[0].va) * 0.5).toBe(bpmAt(pts, 120, 4));
  });

  it('stops at the end of the view, at the value the glide REACHES there', () => {
    // 90 gliding to 60 over beats 4..8: at beat 6 (the view's end) it has reached 75, it does
    // not bend to hit 60 early
    expect(tempoSegments([pt(4, 90, true), pt(8, 60)], 120, 6)).toEqual([{ a: 0, b: 4, va: 120, vb: 120 }, { a: 4, b: 6, va: 90, vb: 75 }]);
    expect(tempoSegments([pt(4, 90), pt(4, 60)], 120, 16)).toEqual([{ a: 0, b: 4, va: 120, vb: 120 }, { a: 4, b: 16, va: 60, vb: 60 }]);
  });
});

describe('tempoScale', () => {
  it('widens a flat map to the minimum span', () => {
    expect(tempoScale(undefined, 120)).toEqual({ lo: 90, hi: 150 });
  });

  it('shows the played values with headroom', () => {
    expect(tempoScale([pt(0, 50), pt(10, 250)], 120)).toEqual({ lo: 40, hi: 260 });
  });

  it('stays inside what a tempo may be, at either end', () => {
    expect(tempoScale(undefined, 20).lo).toBe(20);
    expect(tempoScale(undefined, 20).hi).toBeLessThanOrEqual(300);
    // the base knob may sit above the point maximum: the axis still covers it
    const s = tempoScale(undefined, 999);
    expect(s.lo).toBeLessThan(999);
    expect(s.hi).toBe(999);
  });
});

describe('tempoGeom', () => {
  it('maps the scale to the strip and back', () => {
    const G = tempoGeom(10, 20, 90, 150);
    expect(G.Y(150)).toBe(3);
    expect(G.Y(90)).toBe(17);
    expect(G.Y(120)).toBe(10);
    expect(G.V(G.Y(117))).toBeCloseTo(117, 6);
    expect(G.X(4)).toBe(40);
    expect(G.T(40)).toBe(4);
  });
});

describe('hitTempo', () => {
  it('picks the nearest point within reach, else nothing', () => {
    const G = tempoGeom(10, 20, 90, 150);
    const pts = [pt(4, 120), pt(12, 60)];
    expect(hitTempo(pts, G, 40, 10)).toBe(0);
    expect(hitTempo(pts, G, 47, 11)).toBe(0);
    expect(hitTempo(pts, G, 120, 17)).toBe(1);
    expect(hitTempo(pts, G, 40, 19.5)).toBe(null);
    expect(hitTempo(undefined, G, 40, 10)).toBe(null);
  });
});

describe('addTempoPoint', () => {
  it('inserts sorted and clamps to the song and the tempo range', () => {
    const { points, i } = addTempoPoint([pt(8, 60)], 2, 100);
    expect(i).toBe(0);
    expect(points).toEqual([pt(2, 100), pt(8, 60)]);
    expect(addTempoPoint([], -5, 5000).points).toEqual([pt(0, 300)]);
    expect(addTempoPoint([], TOTAL_BEATS + 99, 1).points).toEqual([pt(TOTAL_BEATS, 20)]);
  });

  it('lands after the points it ties with, so the new one is the one played', () => {
    const { points, i } = addTempoPoint([pt(4, 90)], 4, 100);
    expect(i).toBe(1);
    expect(points).toEqual([pt(4, 90), pt(4, 100)]);
  });
});

describe('moveTempoPoint', () => {
  it('never passes the neighbours and clamps the value', () => {
    const moved = moveTempoPoint([pt(4, 100), pt(8, 120)], 0, 20, 500);
    expect(moved[0].beat).toBeCloseTo(8, 5);
    expect(moved[0].bpm).toBe(300);
    const back = moveTempoPoint([pt(4, 100), pt(8, 120)], 1, -10, 10);
    expect(back[1].beat).toBeCloseTo(4, 5);
    expect(back[1].bpm).toBe(20);
  });

  it('the ramp flag rides along', () => {
    const moved = moveTempoPoint([pt(0, 60, true), pt(8, 120)], 0, 2, 90);
    expect(moved[0]).toEqual(pt(2, 90, true));
    expect(moved[1]).toEqual(pt(8, 120));
  });

  it('a lone point clamps to the song ends', () => {
    expect(moveTempoPoint([pt(4, 100)], 0, -20, 130)[0].beat).toBe(0);
    expect(moveTempoPoint([pt(4, 100)], 0, TOTAL_BEATS + 99, 130)[0].beat).toBe(TOTAL_BEATS);
  });

  it('tied neighbours pin the beat: only the BPM moves (the store must not reshuffle)', () => {
    const tied = [pt(4, 90), pt(4, 100), pt(4, 110)];
    const moved = moveTempoPoint(tied, 1, 20, 200);
    expect(moved[1].beat).toBe(4);
    expect(moved[1].bpm).toBe(200);
    expect(moved.map(p => p.beat)).toEqual([4, 4, 4]);
    // tied at the song's end, the same rule
    const endTied = [pt(TOTAL_BEATS, 90), pt(TOTAL_BEATS, 100), pt(TOTAL_BEATS, 110)];
    const back = moveTempoPoint(endTied, 1, -20, 200);
    expect(back[1].beat).toBe(TOTAL_BEATS);
    expect(back[1].bpm).toBe(200);
  });
});

describe('the remaining point edits', () => {
  it('delete, set the value, toggle the glide', () => {
    const pts = [pt(0, 60, true), pt(8, 120)];
    expect(deleteTempoPoint(pts, 0)).toEqual([pt(8, 120)]);
    expect(deleteTempoPoint(pts, 5)).toEqual(pts);
    expect(setTempoBpm(pts, 0, 700)).toEqual([pt(0, 300, true), pt(8, 120)]);
    expect(setTempoRamp(pts, 1, true)).toEqual([pt(0, 60, true), pt(8, 120, true)]);
  });

  it('dedupe keeps the last of each beat as the engine plays it', () => {
    expect(dedupeTempo([pt(8, 90), pt(4, 100), pt(8, 60)])).toEqual([pt(4, 100), pt(8, 60)]);
  });
});

describe('compiledTempo', () => {
  it('clamps, sorts, folds duplicates and caps at MAX_TEMPO_POINTS — a raw file cannot make the lane unbounded', () => {
    const many: TempoPoint[] = Array.from({ length: 5000 }, (_, i) => ({ beat: i * 0.4, bpm: 100 + (i % 5), ramp: i % 2 === 0 }));
    const out = compiledTempo(many);
    expect(out).toHaveLength(4096);
    expect(out[0].beat).toBeLessThanOrEqual(out[1].beat);
    expect(out.every(p => p.bpm >= 20 && p.bpm <= 300 && p.beat >= 0)).toBe(true);
    expect(compiledTempo([pt(4, 90), pt(4, 60)])).toEqual([pt(4, 60)]);
    expect(compiledTempo(undefined)).toEqual([]);
  });
});

describe('bpmLabel', () => {
  it('is as short as the strip can draw', () => {
    expect(bpmLabel(128)).toBe('128');
    expect(bpmLabel(128.5)).toBe('128.5');
  });
});
