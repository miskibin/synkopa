// The tempo lane's painting: the song's tempo map as a thin strip above the ruler (a sibling of
// the marker strip). Flats are horizontal lines, ramps slant to the next point (a ramp glides
// linearly in beats, so the drawn line is straight — see lib/tempoLane.ts). Pure layout in
// `tempoLabels`; the canvas pass itself is exercised in the e2e journey.
import type { TempoPoint } from '../api/types';
import { bpmLabel, tempoSegments, TEMPO_H, type TempoGeom } from '../lib/tempoLane';
import { MARKER_FONT } from './markerDraw';
import { rgba, T } from '../ui/theme';

let measureCtx: CanvasRenderingContext2D | null = null;
/** Text width in the strip's font (a shared offscreen context: hit tests need it before a paint). */
export function tempoTextWidth(s: string): number {
  if (!measureCtx) {
    measureCtx = typeof document !== 'undefined' ? document.createElement('canvas').getContext('2d') : null;
    if (!measureCtx) return s.length * 6.5;
  }
  measureCtx.font = MARKER_FONT;
  return measureCtx.measureText(s).width;
}

export interface TempoLabel { i: number; x: number; y: number; text: string }

/** Each point's BPM label where it fits: right of the point, never into the next one's. */
export function tempoLabels(points: readonly TempoPoint[] | undefined, bw: number, G: TempoGeom, end: number, H = TEMPO_H): TempoLabel[] {
  const list = points ?? [], out: TempoLabel[] = [];
  for (let i = 0; i < list.length; i++) {
    const p = list[i], x = G.X(p.beat) + 5, nx = i + 1 < list.length ? G.X(list[i + 1].beat) : end * bw;
    const text = bpmLabel(p.bpm);
    if (x + tempoTextWidth(text) > nx - 3) continue;
    out.push({ i, x, y: Math.min(H - 6, Math.max(8, G.Y(p.bpm))), text });
  }
  return out;
}

/**
 * Draws the strip `W` px wide from content x `offX`. `focus` = the point under the pointer or
 * being dragged (drawn brighter).
 */
export function drawTempoLane(g: CanvasRenderingContext2D, offX: number, W: number, points: readonly TempoPoint[] | undefined, base: number, bw: number, G: TempoGeom, focus: number | null, H = TEMPO_H) {
  g.fillStyle = T.rulerBg;
  g.fillRect(0, 0, W, H);
  g.save();
  g.translate(-offX, 0);
  const end = G.T(offX + W);
  // one path for the curve: flats and ramps (a lead-in at the base tempo included). The true
  // endpoints are drawn even off-screen and the canvas clips: clamping x and pinning the value
  // there would bend every ramp that scrolls past either edge.
  g.strokeStyle = T.blue;
  g.lineWidth = 1.5;
  g.beginPath();
  for (const s of tempoSegments(points, base, end)) {
    if (s.b * bw < offX - 4 || s.a * bw - 4 > offX + W) continue;
    g.moveTo(s.a * bw, G.Y(s.va));
    g.lineTo(s.b * bw, G.Y(s.vb));
  }
  g.stroke();
  const list = points ?? [];
  for (let i = 0; i < list.length; i++) {
    const x = G.X(list[i].beat);
    if (x < offX - 4 || x - 4 > offX + W) continue;
    g.fillStyle = i === focus ? T.blueHi : T.blue;
    g.fillRect(x - 1.5, G.Y(list[i].bpm) - 1.5, 3, 3);
  }
  g.restore();
  g.font = MARKER_FONT;
  g.textBaseline = 'middle';
  for (const l of tempoLabels(points, bw, G, G.T(offX + W), H)) {
    if (l.x + tempoTextWidth(l.text) < offX || l.x > offX + W) continue;
    g.fillStyle = l.i === focus ? T.fgHi : rgba(T.fg2, 0.9);
    g.fillText(l.text, l.x - offX, l.y);
  }
  g.fillStyle = T.bg0;
  g.fillRect(0, H - 1, W, 1);
}
