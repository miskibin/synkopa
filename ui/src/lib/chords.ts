// Chords of audio, UI side: the voicing the engine writes (crates/engine/src/chords.rs `voice`, mirrored
// for the in-browser mock), chord names, and the dialog's range helpers.
import type { ChordQuality, ChordSegment } from '../api/types';

export const CHORD_INTERVALS: Record<ChordQuality, number[]> = {
  maj: [0, 4, 7], min: [0, 3, 7], '7': [0, 4, 7, 10], maj7: [0, 4, 7, 11], m7: [0, 3, 7, 10], sus2: [0, 2, 7], sus4: [0, 5, 7], dim: [0, 3, 6],
};
const SUFFIX: Record<ChordQuality, string> = { maj: '', min: 'm', '7': '7', maj7: 'maj7', m7: 'm7', sus2: 'sus2', sus4: 'sus4', dim: 'dim' };
const SHARPS = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
const FLATS = ['C', 'Db', 'D', 'Eb', 'E', 'F', 'Gb', 'G', 'Ab', 'A', 'Bb', 'B'];

/** "Bbmaj7", "F#m". */
export const chordName = (root: number, q: ChordQuality, flats: boolean) => (flats ? FLATS : SHARPS)[((root % 12) + 12) % 12] + SUFFIX[q];
/** Major or minor: what a chord is at the "simple" detail. */
export const simpleQuality = (q: ChordQuality): ChordQuality => (q === 'min' || q === 'm7' || q === 'dim' ? 'min' : 'maj');
/** A key written with flats (F, Bb, Eb, Ab, Db, Gb major and their relative minors). */
export const keyUsesFlats = (root: number, minor: boolean) => [5, 10, 3, 8, 1, 6].includes(minor ? (root + 3) % 12 : root % 12);

const BASS_LOW = 36, UPPER_LOW = 54, UPPER_HIGH = 76;

/** One chord's keys, the bass first: the root in E1..D#2, the chord tones close together from F#3 up, in the
 *  inversion nearest `prev` (the previous chord's upper voices). Same as the engine. */
export function voiceChord(root: number, q: ChordQuality, prev?: number[] | null): number[] {
  root = ((root % 12) + 12) % 12;
  const bass = BASS_LOW + ((root + 12 - (BASS_LOW % 12)) % 12);
  const tones = CHORD_INTERVALS[q].map(i => (root + i) % 12);
  let best: { cost: number; v: number[] } | null = null;
  for (let start = UPPER_LOW; start < UPPER_LOW + 12; start++) {
    if (!tones.includes(start % 12)) continue;
    const v = [start];
    while (v.length < tones.length) {
      let k = v[v.length - 1] + 1;
      while (!tones.includes(k % 12) || v.some(x => x % 12 === k % 12)) k++;
      v.push(k);
    }
    if (v[v.length - 1] > UPPER_HIGH) continue;
    const mean = (a: number[]) => a.reduce((s, x) => s + x, 0) / a.length;
    const cost = prev && prev.length
      ? v.reduce((s, x) => s + Math.min(...prev.map(y => Math.abs(x - y))), 0) + 0.5 * Math.abs(mean(prev) - mean(v))
      : Math.abs(mean(v) - 64);
    if (!best || cost < best.cost) best = { cost, v };
  }
  return [bass, ...(best?.v ?? [])];
}

/** The chords overlapping `from..to`, cut to it. */
export function chordsIn(segs: readonly ChordSegment[], from: number, to: number): ChordSegment[] {
  return segs.filter(s => s.end > from + 1e-6 && s.start < to - 1e-6).map(s => ({ ...s, start: Math.max(s.start, from), end: Math.min(s.end, to) }));
}

/** The dialog's starting range: the Playlist's time selection where it overlaps the clip, else the whole clip. */
export function defaultChordRange(clip: { start: number; end: number }, loop: { a: number; b: number } | null | undefined): [number, number] {
  if (loop && loop.b > clip.start + 1e-6 && loop.a < clip.end - 1e-6) return [Math.max(loop.a, clip.start), Math.min(loop.b, clip.end)];
  return [clip.start, clip.end];
}
