import { describe, expect, it } from 'vitest';
import type { ChordSegment } from '../api/types';
import { chordName, chordsIn, defaultChordRange, keyUsesFlats, simpleQuality, voiceChord } from './chords';

describe('chord helpers (mirror crates/engine/src/chords.rs)', () => {
  it('names chords for the key', () => {
    expect(chordName(10, 'maj7', true)).toBe('Bbmaj7');
    expect(chordName(6, 'min', false)).toBe('F#m');
    expect(chordName(-2, 'maj', false)).toBe('A#');
    expect(keyUsesFlats(5, false)).toBe(true); // F major
    expect(keyUsesFlats(2, true)).toBe(true); // D minor
    expect(keyUsesFlats(9, true)).toBe(false); // A minor
    expect(simpleQuality('m7')).toBe('min');
    expect(simpleQuality('sus4')).toBe('maj');
    expect(simpleQuality('dim')).toBe('min');
  });

  it('voices like the engine: root in the bass, the next chord in the nearest inversion', () => {
    // the engine's test: C2 bass, C → F → G7 → C each moving at most a few semitones
    const c = voiceChord(0, 'maj');
    expect(c[0]).toBe(36);
    expect(c.slice(1).map(k => k % 12).sort((a, b) => a - b)).toEqual([0, 4, 7]);
    let prev = c.slice(1);
    for (const [r, q] of [[5, 'maj'], [7, '7'], [0, 'maj']] as const) {
      const v = voiceChord(r, q, prev);
      expect(v[0] % 12).toBe(r);
      expect(v[0]).toBeGreaterThanOrEqual(36);
      expect(v[0]).toBeLessThan(48);
      const moved = v.slice(1).reduce((s, x) => s + Math.min(...prev.map(y => Math.abs(x - y))), 0);
      expect(moved).toBeLessThanOrEqual(6);
      prev = v.slice(1);
    }
    expect(voiceChord(2, 'm7')).toHaveLength(5);
  });

  it('cuts chords to a range and starts from the time selection over the clip', () => {
    const s = (start: number, end: number, name: string): ChordSegment => ({ start, end, root: 0, quality: 'maj', name, confidence: 1 });
    const segs = [s(0, 4, 'C'), s(4, 8, 'F'), s(8, 12, 'G')];
    expect(chordsIn(segs, 2, 9).map(x => [x.name, x.start, x.end])).toEqual([['C', 2, 4], ['F', 4, 8], ['G', 8, 9]]);
    expect(chordsIn(segs, 4, 8).map(x => x.name)).toEqual(['F']);
    const clip = { start: 4, end: 36 };
    expect(defaultChordRange(clip, null)).toEqual([4, 36]);
    expect(defaultChordRange(clip, { a: 0, b: 12 })).toEqual([4, 12]);
    expect(defaultChordRange(clip, { a: 40, b: 48 })).toEqual([4, 36]);
  });
});
