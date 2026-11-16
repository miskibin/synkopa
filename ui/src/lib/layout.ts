// The window layout saved per user (`wave.layout` in localStorage, never in the project) and the
// size math of the docked pattern panels (Channel Rack docked left, Piano roll docked bottom).
// Pure: tested in layout.test.ts.
import type { Tool, UiState } from '../state/ui';

/** Floating window geometry (px). */
export interface Geo { x: number; y: number; w: number; h: number }

/** Width of a splitter between docked panels (`.vsplit` / `.docksplit`). */
export const SPLIT = 5;
/** The Playlist never gets narrower / lower than this while a pattern panel is docked beside / under it. */
export const PLAYLIST_MIN_W = 320, PLAYLIST_MIN_H = 140;
/**
 * The docked Piano roll's lowest height: its title bar, tools, overview, velocity lane and status
 * line take ~200 px, so this keeps a band of keys to draw in (the floating roll's minimum too).
 * The Mixer gives way to it (Mixer.tsx).
 */
export const ROLL_DOCK_MIN_H = 300;
/** The toolbar's height and the docked mixer's splitter (it has no title bar), for the room a docked roll needs. */
export const TOOLBAR_H = 54, MIXER_BAR_H = SPLIT;
/** What the Mixer leaves of the window's height while the Piano roll is docked: toolbar, Playlist, roll, their splitter, its own splitter. */
export const ROLL_DOCKED_RESERVE = TOOLBAR_H + PLAYLIST_MIN_H + SPLIT + ROLL_DOCK_MIN_H + MIXER_BAR_H;

/**
 * The docked mixer's height. It has no title bar: dragging its splitter below `MIX_COLLAPSE_AT`
 * collapses it to a thin handle (`MIX_HANDLE_H`) and keeps the height it had for when it opens
 * again; between the breakpoint and `MIX_MIN_H` it stays at its minimum. Dragging the collapsed
 * handle up by `MIX_OPEN_DRAG` opens it under the pointer.
 */
export const MIX_MIN_H = 170, MIX_COLLAPSE_AT = 110, MIX_HANDLE_H = 6, MIX_OPEN_DRAG = 24;
/** The tallest docked mixer for a window `winH` high: it leaves the Playlist (and a docked Piano roll) their room. */
export function mixMaxH(winH: number, rollDocked: boolean): number {
  return Math.max(MIX_MIN_H, winH - (rollDocked ? ROLL_DOCKED_RESERVE : 200));
}
/** A wanted docked height clamped to what fits (whole px). */
export function fitMixH(h: number, max: number): number {
  return Math.round(Math.min(Math.max(h, MIX_MIN_H), Math.max(MIX_MIN_H, max)));
}
/** Where the docked mixer goes while its splitter is dragged by `dy` (down = positive) from height `h0`: collapsed below the breakpoint (the height before the drag is kept), else the fitted height. */
export function mixSplitDrag(h0: number, dy: number, max: number): { min: boolean; h: number } {
  const raw = h0 - dy;
  return raw < MIX_COLLAPSE_AT ? { min: true, h: h0 } : { min: false, h: fitMixH(raw, max) };
}
/** Dragging the collapsed handle by `dy`: it opens once pulled up by `MIX_OPEN_DRAG` and then follows the pointer; pushed back down it collapses again, keeping `last`. */
export function mixHandleDrag(last: number, dy: number, max: number): { min: boolean; h: number } {
  const raw = -dy;
  return raw < MIX_OPEN_DRAG ? { min: true, h: last } : { min: false, h: fitMixH(raw, max) };
}
/** Default docked sizes. */
export const RACK_DOCK_W = 0, ROLL_DOCK_H = 320;  // rackDockW 0: fit the pattern (`rackDockFitWidth`)

/**
 * The size (px) a docked panel gets: the size the user wants, no less than `min` and leaving
 * `rest` for its neighbour (the Playlist) in `avail`. When both minimums do not fit (a small
 * window, a high UI scale, a tall mixer) the panel gives way down to half of what there is, so
 * neither it nor the Playlist runs off the window. `avail` ≤ 0 means "not measured yet".
 */
export function fitDock(want: number, min: number, avail: number, rest: number): number {
  if (!(avail > 0)) return Math.round(Math.max(min, want));
  const max = avail - rest;
  if (max >= min) return Math.round(Math.min(Math.max(want, min), max));
  return Math.max(0, Math.round(Math.min(min, Math.max(max, avail / 2))));
}

/**
 * F6 / F7 on a docked panel: hidden → show it (and give it the keyboard); shown but the keyboard
 * is elsewhere → give it the keyboard; shown and focused → hide it. A docked panel is never
 * "behind" anything, so the floating windows' bring-to-front step does not apply.
 */
export function dockKeyAction(open: boolean, focused: boolean): 'show' | 'focus' | 'hide' {
  if (!open) return 'show';
  return focused ? 'hide' : 'focus';
}

const TOOLS_OK: Tool[] = ['draw', 'paint', 'erase', 'mute', 'slice', 'select'];
const num = (v: unknown, lo: number, hi: number): number | undefined => (typeof v === 'number' && isFinite(v) && v >= lo && v <= hi ? v : undefined);
const bool = (v: unknown): boolean | undefined => (typeof v === 'boolean' ? v : undefined);
/** A saved floating window's place, or undefined when anything about it is off (it is clamped on screen when used). */
export function parseGeo(v: unknown): Geo | undefined {
  if (!v || typeof v !== 'object') return undefined;
  const o = v as Record<string, unknown>;
  const x = num(o.x, -20000, 20000), y = num(o.y, -20000, 20000), w = num(o.w, 100, 20000), h = num(o.h, 100, 20000);
  return x === undefined || y === undefined || w === undefined || h === undefined ? undefined : { x, y, w, h };
}

/** A saved layout as a patch of valid fields only. */
export function parseLayout(raw: string | null): Partial<UiState> {
  let v: Record<string, unknown>;
  try { v = JSON.parse(raw ?? '{}') ?? {}; } catch { return {}; }
  if (typeof v !== 'object' || Array.isArray(v)) return {};
  const p: Partial<UiState> = {
    tool: TOOLS_OK.includes(v.tool as Tool) ? (v.tool as Tool) : undefined,
    snap: num(v.snap, -1, 64), bw: num(v.bw, 4, 96), follow: bool(v.follow),
    // the docked mixer can no longer be closed: a layout saved with it closed opens collapsed
    mixMin: v.mixOpen === false || v.mixMin === true ? true : bool(v.mixMin), mixH: num(v.mixH, 100, 2000),
    headW: num(v.headW, 120, 600), pickW: num(v.pickW, 140, 800), pickOpen: bool(v.pickOpen),
    browserTab: v.browserTab === 'project' || v.browserTab === 'library' ? v.browserTab : undefined,
    rackDock: bool(v.rackDock), rackDockW: v.rackDockW === 0 ? 0 : num(v.rackDockW, 200, 8000),
    rollDock: bool(v.rollDock), rollDockH: num(v.rollDockH, 100, 8000),
    rackFloat: parseGeo(v.rackFloat), rollFloat: parseGeo(v.rollFloat), rollMax: bool(v.rollMax),
    mixDock: bool(v.mixDock), mixWin: ((g: Geo | undefined) => g && { ...g, z: 0 })(parseGeo(v.mixFloat)),
  };
  for (const k of Object.keys(p) as (keyof UiState)[]) if (p[k] === undefined) delete p[k];
  return p;
}
