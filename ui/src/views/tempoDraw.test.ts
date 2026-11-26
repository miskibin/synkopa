import { describe, expect, it } from 'vitest';
import type { TempoPoint } from '../api/types';
import { tempoGeom } from '../lib/tempoLane';
import { drawTempoLane, tempoLabels } from './tempoDraw';

const pt = (beat: number, bpm: number, ramp = false): TempoPoint => ({ beat, bpm, ramp });

describe('tempoLabels', () => {
  const G = tempoGeom(10, 20, 50, 130);

  it("places a point's BPM right of it, out of the next one's way", () => {
    const labels = tempoLabels([pt(4, 128), pt(12, 60)], 10, G, 16);
    expect(labels).toHaveLength(2);
    expect(labels[0]).toEqual({ i: 0, x: 45, y: 8, text: '128' });
    expect(labels[1]).toEqual({ i: 1, x: 125, y: 14, text: '60' });
  });

  it('drops a label that would run into the next point, keeps the last one', () => {
    const labels = tempoLabels([pt(2, 90), pt(2.5, 91)], 10, G, 16);
    expect(labels[0]).toMatchObject({ i: 1, x: 30, text: '91' });
    expect(labels[0].y).toBeCloseTo(9.825, 6);
  });
});

describe('drawTempoLane', () => {
  /** A context that records the path calls, so the strip's segments are checkable. */
  const recordingCtx = () => {
    const calls: string[] = [];
    const g = {
      moveTo: (x: number, y: number) => calls.push(`move ${Math.round(x)},${Math.round(y)}`),
      lineTo: (x: number, y: number) => calls.push(`line ${Math.round(x)},${Math.round(y)}`),
      fillRect: () => calls.push('fill'),
      fillText: () => calls.push('text'),
      stroke: () => calls.push('stroke'),
      save: () => {}, restore: () => {}, translate: () => {}, beginPath: () => {},
    };
    return { g: g as unknown as CanvasRenderingContext2D, calls };
  };

  it('draws a ramp scrolled past the left edge along its TRUE slope (the canvas clips)', () => {
    // the glide 90 -> 60 over beats 4..8, viewed from beat 5: the line starts off-screen at the
    // segment's real start value, not pinned at the viewport edge
    const G2 = tempoGeom(10, 20, 60, 120);
    const c = recordingCtx();
    drawTempoLane(c.g, 50, 40, [pt(4, 90, true), pt(8, 60)], 90, 10, G2, null);
    expect(c.calls.slice(0, 7)).toEqual(['fill', 'move 40,10', 'line 80,17', 'move 80,17', 'line 90,17', 'stroke', 'fill']);
  });

  it('draws the base flat when the map is empty and flats plus a ramp with points', () => {
    const G = tempoGeom(10, 20, 90, 150);
    const a = recordingCtx();
    drawTempoLane(a.g, 0, 200, undefined, 120, 10, G, null);
    expect(a.calls).toEqual(['fill', 'move 0,10', 'line 200,10', 'stroke', 'fill']);

    const b = recordingCtx();
    drawTempoLane(b.g, 0, 200, [pt(8, 90, true), pt(16, 150)], 120, 10, G, null);
    expect(b.calls.slice(0, 8)).toEqual(['fill', 'move 0,10', 'line 80,10', 'move 80,17', 'line 160,3', 'move 160,3', 'line 200,3', 'stroke']);
  });
});
