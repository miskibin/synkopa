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
/** The toolbar's and the mixer's title bar + splitter's heights (px), for the room a docked roll needs. */
export const TOOLBAR_H = 54, MIXER_BAR_H = 31;
/** What the Mixer leaves of the window's height while the Piano roll is docked: toolbar, Playlist, roll, their splitter, its own bar. */
export const ROLL_DOCKED_RESERVE = TOOLBAR_H + PLAYLIST_MIN_H + SPLIT + ROLL_DOCK_MIN_H + MIXER_BAR_H;
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
    mixOpen: bool(v.mixOpen), mixMin: bool(v.mixMin), mixH: num(v.mixH, 100, 2000),
    headW: num(v.headW, 120, 600), pickW: num(v.pickW, 140, 800), pickOpen: bool(v.pickOpen),
    browserTab: v.browserTab === 'project' || v.browserTab === 'library' ? v.browserTab : undefined,
    rackDock: bool(v.rackDock), rackDockW: v.rackDockW === 0 ? 0 : num(v.rackDockW, 200, 8000),
    rollDock: bool(v.rollDock), rollDockH: num(v.rollDockH, 100, 8000),
    rackFloat: parseGeo(v.rackFloat), rollFloat: parseGeo(v.rollFloat), rollMax: bool(v.rollMax),
  };
  for (const k of Object.keys(p) as (keyof UiState)[]) if (p[k] === undefined) delete p[k];
  return p;
}
