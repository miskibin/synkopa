// The Playlist's marker strip: a thin row above the ruler that shows the song's time markers
// (FL Studio's time markers) as flags with their names. Only drawn while the song has markers.
import type { Id, Marker } from '../api/types';
import { LABEL_PAD, layoutMarkers, type MarkerBox } from '../lib/markers';
import { rgba, T } from '../ui/theme';

/** Strip height (px). */
export const MARKER_H = 18;
export const MARKER_FONT = '500 11px "Geist Variable", "Geist", system-ui, sans-serif';

let measureCtx: CanvasRenderingContext2D | null = null;
/** Text width in the strip's font (a shared offscreen context: hit tests need it before a paint). */
export function markerTextWidth(s: string): number {
  if (!measureCtx) {
    measureCtx = typeof document !== 'undefined' ? document.createElement('canvas').getContext('2d') : null;
    if (!measureCtx) return s.length * 6.5;
  }
  measureCtx.font = MARKER_FONT;
  return measureCtx.measureText(s).width;
}

/** Boxes of `markers` at `bw` px per beat (see `layoutMarkers`). */
export const markerBoxes = (markers: readonly Marker[] | undefined, bw: number): MarkerBox[] => layoutMarkers(markers, bw, markerTextWidth);

/**
 * Draws the strip `W` px wide from content x `offX`. `focus` = the marker under the pointer or
 * being dragged (drawn brighter).
 */
export function drawMarkers(g: CanvasRenderingContext2D, offX: number, W: number, boxes: readonly MarkerBox[], focus: Id | null, H = MARKER_H) {
  g.fillStyle = T.rulerBg; g.fillRect(0, 0, W, H);
  g.save(); g.translate(-offX, 0);
  g.font = MARKER_FONT; g.textBaseline = 'middle';
  for (const b of boxes) {
    if (b.x1 < offX - 4 || b.x - 4 > offX + W) continue;
    const on = b.id === focus, x = Math.round(b.x);
    if (b.text) {
      g.fillStyle = rgba(T.amber, on ? 0.32 : 0.16);
      g.fillRect(x, 2, Math.max(0, b.x1 - x), H - 5);
    }
    g.fillStyle = T.amber; g.fillRect(x, 0, on ? 2 : 1.5, H - 1);
    if (b.text) { g.fillStyle = on ? T.fgHi : T.fg2; g.fillText(b.text, x + LABEL_PAD, (H - 1) / 2 + 0.5); }
  }
  g.restore();
  g.fillStyle = T.bg0; g.fillRect(0, H - 1, W, 1);
}

/** Thin lines on the ruler under each marker, so a section's start reads against the bar numbers. */
export function drawMarkerTicks(g: CanvasRenderingContext2D, offX: number, W: number, markers: readonly Marker[] | undefined, bw: number, H: number) {
  if (!markers?.length) return;
  g.fillStyle = rgba(T.amber, 0.7);
  for (const m of markers) {
    const x = Math.round(m.at * bw - offX);
    if (x >= -1 && x <= W + 1) g.fillRect(x, 0, 1, H - 1);
  }
}
