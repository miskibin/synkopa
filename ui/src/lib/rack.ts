// Channel Rack window logic: how wide the window wants to be, where a channel moves.
import { gridW } from './pattern';

/**
 * The widest grid the window grows to on its own, in steps (16 bars). In practice the screen is
 * the limit: the window grows until it spans it, and only then does the grid scroll.
 */
export const RACK_FIT_STEPS = 256;
/** Steps of the greyed bar after the pattern (a click there makes the pattern a bar longer). */
export const RACK_EXTRA_STEPS = 16;
/** Docked at its automatic width, the rack shows at most one bar (FL); a longer pattern scrolls (it shares the row with the Playlist). */
export const RACK_DOCK_FIT_STEPS = 16;

/**
 * The Channel Rack's width (px) for a pattern of `steps` steps: the pattern and the greyed extra
 * bar at step width `cw`, at most `RACK_FIT_STEPS` steps' worth, then no more than `room` (the
 * space to the screen's right edge) and no less than `min`. `left`: the channel strip; `pad`:
 * the frame around the grid.
 */
export function rackFitWidth(steps: number, o: { left: number; cw: number; pad: number; min: number; room: number }): number {
  const shown = Math.min(steps + RACK_EXTRA_STEPS, RACK_FIT_STEPS);
  const want = o.left + gridW(shown, o.cw) + o.pad;
  return Math.round(Math.max(o.min, Math.min(want, o.room)));
}

/**
 * The docked Channel Rack's width (px) when the user has not set one: the pattern's own steps
 * (no greyed extra bar) at the compact step width `cw`, no less than `min`. Docked, the rack shares
 * the row with the Playlist, so it takes what the beat needs, up to `RACK_DOCK_FIT_STEPS` (a 4-bar
 * pattern took over 1000 px and left the Playlist at its minimum); the grid scrolls for longer ones.
 */
export function rackDockFitWidth(steps: number, o: { left: number; cw: number; pad: number; min: number }): number {
  return Math.round(Math.max(o.min, o.left + gridW(Math.min(Math.max(steps, 1), RACK_DOCK_FIT_STEPS), o.cw) + o.pad));
}

/**
 * Where a window of width `w` at `x` goes so that it stays on a screen `screen` px wide with
 * `margin` px to spare: it keeps its place while it fits and slides left when it has grown past
 * the right edge (the Channel Rack widening with its pattern).
 */
export function fitOnScreen(x: number, w: number, screen: number, margin = 10): { x: number; w: number } {
  const ww = Math.min(w, Math.max(0, screen - 2 * margin));
  return { x: Math.max(margin, Math.min(x, screen - margin - ww)), w: ww };
}

/** Alt+↑ / ↓: the index channel `i` of `n` moves to by `d`, or null at the top / bottom. */
export function moveIndex(i: number, n: number, d: number): number | null {
  const to = i + d;
  return i < 0 || to < 0 || to >= n || to === i ? null : to;
}

/** Convert a gap in the current list to the index after removing the dragged channel. */
export function rackMoveIndex(from: number, slot: number): number | null {
  if (from < 0) return null;
  const index = slot > from ? slot - 1 : slot;
  return index === from ? null : index;
}

/** A sample dropped near a row boundary creates a channel. The middle replaces its sound. */
export function rackSampleDrop(offset: number, height: number): 'before' | 'after' | 'replace' {
  const edge = Math.min(6, height / 4);
  if (offset < edge) return 'before';
  if (offset >= height - edge) return 'after';
  return 'replace';
}

/** The Channel Rack's status footer: its height (22 + its top rule) and the window height under which it is hidden (`compactH`). */
export const RACK_FOOT_H = 23, RACK_COMPACT_H = 230;

/**
 * The window height (px) that fits `content` (all the rows and the "+ Channel" row). `chrome` is what
 * the window spends besides the grid right now, measured with or without the footer (`footerShown`);
 * the footer is hidden below `compactH`, so a height reached without it has to make room for it when
 * it is tall enough to bring it back, or the footer lands over the last row.
 */
export function rackFitHeight(o: { chrome: number; content: number; footerShown: boolean; borders: number; footerH?: number; compactH?: number }): number {
  const foot = o.footerH ?? RACK_FOOT_H, compact = o.compactH ?? RACK_COMPACT_H;
  const bare = o.chrome - (o.footerShown ? foot : 0) + o.content;
  return bare - o.borders >= compact ? bare + foot : bare;
}
