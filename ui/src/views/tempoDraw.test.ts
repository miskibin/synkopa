import { describe, expect, it } from 'vitest';
import type { TempoPoint } from '../api/types';
import { tempoGeom } from '../lib/tempoLane';
import { tempoLabels } from './tempoDraw';

const pt = (beat: number, bpm: number, ramp = false): TempoPoint => ({ beat, bpm, ramp });

describe('tempoLabels', () => {
  const G = tempoGeom(10, 20, 50, 130);

  it("places a point's BPM right of it, out of the next one's way", () => {
    const labels = tempoLabels([pt(4, 128), pt(12, 60)], 10, G, 16);
    expect(labels).toHaveLength(2);
    expect(labels[0].i).toBe(0);
    expect(labels[0].x).toBe(45);
    expect(labels[0].text).toBe('128');
    expect(labels[1].x).toBe(125);
  });

  it('drops a label that would run into the next point, keeps the last one', () => {
    const labels = tempoLabels([pt(2, 90), pt(2.5, 91)], 10, G, 16);
    expect(labels).toEqual([{ i: 1, x: 30, y: expect.any(Number), text: '91' }]);
  });
});
