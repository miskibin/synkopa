import { describe, expect, it } from 'vitest';
import type { Marker, Project } from '../api/types';
import { fitText, jumpTarget, layoutMarkers, markerAtX, markerSection, nextMarkerName, putMarker, removeMarker } from './markers';

const mk = (id: number, at: number, name = 'M' + id): Marker => ({ id, at, name });
const proj = (markers?: Marker[], clips: Project['clips'] = []) => ({ markers, clips, nextId: 100 }) as unknown as Project;
/** 6 px per character: a monospace stand-in for the canvas. */
const mono = (s: string) => [...s].length * 6;

describe('marker ops (the engine port; ops.json pins the parity)', () => {
  it('keeps the list sorted by position, then id, and bumps nextId', () => {
    const p = proj([mk(1, 16)]);
    putMarker(p, mk(200, 4));
    putMarker(p, mk(150, 16));
    expect(p.markers!.map(m => m.id)).toEqual([200, 1, 150]);
    expect(p.nextId).toBe(201);
  });
  it('drops the field with the last marker, as the engine omits an empty list', () => {
    const p = proj([mk(1, 0)]);
    removeMarker(p, 1);
    expect('markers' in p).toBe(false);
    expect(() => removeMarker(p, 1)).toThrow('no marker with id 1');
  });
  it('refuses what the engine refuses, with its words', () => {
    const p = proj([]);
    expect(() => putMarker(p, mk(5, -1))).toThrow('marker 5: position must be 0..2048 beats');
    expect(() => putMarker(p, mk(5, 0, 'ż'.repeat(81)))).toThrow('marker 5: name is longer than 80 characters');
    putMarker(p, mk(5, 0, 'ż'.repeat(80)));
  });
});

describe('sections and jumps', () => {
  const ms = [mk(1, 0, 'Intro'), mk(2, 16, 'Hook'), mk(3, 32, 'Verse')];
  it('a section runs to the next marker, the last one to the song end (at least a bar)', () => {
    const clips = [{ start: 0, len: 40 }] as Project['clips'];
    expect(markerSection(proj(ms, clips), 1)).toEqual({ a: 0, b: 16 });
    expect(markerSection(proj(ms, clips), 3)).toEqual({ a: 32, b: 40 });
    expect(markerSection(proj([mk(9, 100)], clips), 9)).toEqual({ a: 100, b: 104 });
    expect(markerSection(proj(ms), 99)).toBeNull();
  });
  it('next / previous from a position; "previous" can skip the marker just crossed while playing', () => {
    expect(jumpTarget(ms, 0, 1)?.name).toBe('Hook');
    expect(jumpTarget(ms, 16, 1)?.name).toBe('Verse');
    expect(jumpTarget(ms, 32, 1)).toBeNull();
    expect(jumpTarget(ms, 16, -1)?.name).toBe('Intro');
    expect(jumpTarget(ms, 16.2, -1)?.name).toBe('Hook');
    expect(jumpTarget(ms, 16.2, -1, 1)?.name).toBe('Intro');
    expect(jumpTarget(ms, 0, -1)).toBeNull();
    expect(jumpTarget(undefined, 4, 1)).toBeNull();
  });
  it('a new marker gets the first free "Marker n"', () => {
    expect(nextMarkerName([], 'Marker')).toBe('Marker 1');
    expect(nextMarkerName([mk(1, 0, 'Marker 2')], 'Marker')).toBe('Marker 3');
    expect(nextMarkerName([mk(1, 0, 'Marker 2'), mk(2, 1, 'x')], 'Marker')).toBe('Marker 3');
  });
});

describe('drawing the strip', () => {
  it('cuts a long name with an ellipsis, and gives up when nothing fits', () => {
    expect(fitText('Hook', 100, mono)).toBe('Hook');
    expect(fitText('Pre-chorus build', 48, mono)).toBe('Pre-cho…');
    expect(fitText('Pre chorus', 30, mono)).toBe('Pre…');
    expect(fitText('Hook', 8, mono)).toBe('');
  });
  it('a label never runs into the next marker; crowded markers keep only their flags', () => {
    const ms = [mk(1, 0, 'Intro'), mk(2, 4, 'A very long section name'), mk(3, 4.5, 'Drop')];
    const b = layoutMarkers(ms, 10, mono); // 40 px between the first two, 5 px between the last two
    expect(b[0].text).toBe('Intro');
    expect(b[0].x1).toBeLessThanOrEqual(b[1].x);
    expect(b[1].text).toBe(''); // 5 px of room: flag only
    expect(b[2].text).toBe('Drop'); // the last one has the rest of the strip (up to its cap)
    // zoomed in: everything fits
    expect(layoutMarkers(ms, 400, mono)[1].text).toBe('A very long section name');
  });
  it('a click finds the flag or the label, the later marker winning where they meet', () => {
    const b = layoutMarkers([mk(1, 0, 'Intro'), mk(2, 8, 'Hook')], 10, mono);
    expect(markerAtX(b, 2)).toBe(1);
    expect(markerAtX(b, 30)).toBe(1); // on "Intro"
    expect(markerAtX(b, 77)).toBe(2); // just left of the Hook flag
    expect(markerAtX(b, 60)).toBeNull(); // between them
    expect(markerAtX([], 0)).toBeNull();
  });
});
