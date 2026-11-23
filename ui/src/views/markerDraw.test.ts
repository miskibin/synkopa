import { describe, expect, it } from 'vitest';
import type { Marker } from '../api/types';
import { markerBoxes } from './markerDraw';

describe('markerBoxes', () => {
  const mk = (id: number, at: number, name: string): Marker => ({ id, at, name }) as Marker;

  it('reuses the layout while nothing changed and redoes it when a marker or the zoom does', () => {
    const ms = [mk(1, 0, 'Intro'), mk(2, 8, 'Drop')];
    const a = markerBoxes(ms, 20);
    expect(markerBoxes(ms, 20)).toBe(a);
    // changed in place (the store may keep the array): never the stale layout
    ms[1] = mk(2, 16, 'Drop');
    const b = markerBoxes(ms, 20);
    expect(b).not.toBe(a);
    expect(b[1].x).toBe(16 * 20);
    ms[0].name = 'A much longer intro name';
    expect(markerBoxes(ms, 20)).not.toBe(b);
    expect(markerBoxes(ms, 40)[1].x).toBe(16 * 40);
  });
});
