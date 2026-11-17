// The window layout saved per user (`wave.layout` in localStorage, never in the project) and the
// docked mixer's collapse math. Where docked panels go is `lib/dock.ts`.
// Pure: tested in layout.test.ts.
import type { Tool, UiState } from '../state/ui';
import { migrateDock, migrateSizes, parseDock, parseHomes, parseSizes } from './dock';

/** Floating window geometry (px). */
export interface Geo { x: number; y: number; w: number; h: number }

/** Width of a splitter between docked panels (`.vsplit` / `.docksplit`). */
export const SPLIT = 5;
/** The Playlist never gets narrower / lower than this while panels are docked beside / under it. */
export const PLAYLIST_MIN_W = 320, PLAYLIST_MIN_H = 140;

/**
 * The docked mixer alone in its strip: dragging the strip's splitter below `MIX_COLLAPSE_AT`
 * collapses it to its title bar (`MIX_HANDLE_H`) and keeps the size it had for when it opens
 * again; between the breakpoint and `MIX_MIN_H` it stays at its minimum. Dragging the collapsed
 * handle toward the Playlist by `MIX_OPEN_DRAG` opens it under the pointer. The sizes are the
 * mixer's thickness: its height at the bottom, its width on a side edge (`min` there).
 */
export const MIX_MIN_H = 170, MIX_COLLAPSE_AT = 110, MIX_HANDLE_H = 28, MIX_OPEN_DRAG = 24;
/** A wanted docked size clamped to what fits (whole px). */
export function fitMixH(h: number, max: number, min = MIX_MIN_H): number {
  return Math.round(Math.min(Math.max(h, min), Math.max(min, max)));
}
/** Where the docked mixer goes while its splitter is dragged by `dy` (away from the Playlist = positive) from size `h0`: collapsed below the breakpoint (the size before the drag is kept), else the fitted size. */
export function mixSplitDrag(h0: number, dy: number, max: number, min = MIX_MIN_H): { min: boolean; h: number } {
  const raw = h0 - dy;
  return raw < MIX_COLLAPSE_AT ? { min: true, h: h0 } : { min: false, h: fitMixH(raw, max, min) };
}
/** Dragging the collapsed handle by `dy` (toward the Playlist = negative): it opens once pulled by `MIX_OPEN_DRAG` and then follows the pointer; pushed back it collapses again, keeping `last`. */
export function mixHandleDrag(last: number, dy: number, max: number, min = MIX_MIN_H): { min: boolean; h: number } {
  const raw = -dy;
  return raw < MIX_OPEN_DRAG ? { min: true, h: last } : { min: false, h: fitMixH(raw, max, min) };
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

/** The fields a layout saved before #90 kept the docked panels in. */
const OLD_DOCK_KEYS = ['rackDock', 'rollDock', 'mixDock', 'rackDockW', 'rollDockH', 'mixH'];
const TOOLS_OK: Tool[] = ['draw', 'paint', 'erase', 'mute', 'slice', 'select'];
const num = (v: unknown, lo: number, hi: number): number | undefined => (typeof v === 'number' && isFinite(v) && v >= lo && v <= hi ? v : undefined);
const bool = (v: unknown): boolean | undefined => (typeof v === 'boolean' ? v : undefined);
/** Which whole Library sections (STANDARD LIBRARY, KITS, FOLDERS) the user collapsed; `true` = closed. */
const SECS = ['std', 'kits', 'folders'] as const;
const parseSec = (v: unknown): Record<(typeof SECS)[number], boolean | undefined> | undefined => {
  if (!v || typeof v !== 'object') return undefined;
  const o = v as Record<string, unknown>;
  return { std: bool(o.std), kits: bool(o.kits), folders: bool(o.folders) };
};
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
    snap: num(v.snap, -1, 64), bw: num(v.bw, 4, 1024), follow: bool(v.follow), // 1024: keep in step with actions.BW_MAX
    // the docked mixer can no longer be closed: a layout saved with it closed opens collapsed
    mixMin: v.mixOpen === false || v.mixMin === true ? true : bool(v.mixMin),
    headW: num(v.headW, 120, 600), pickW: num(v.pickW, 140, 800), pickOpen: bool(v.pickOpen),
    // 'project' (Patterns and Audio in one tab, before they were split) opens on Patterns
    browserTab: v.browserTab === 'project' ? 'patterns' : v.browserTab === 'patterns' || v.browserTab === 'audio' || v.browserTab === 'library' ? v.browserTab : undefined,
    browserSec: parseSec(v.browserSec),
    rackFloat: parseGeo(v.rackFloat), rollFloat: parseGeo(v.rollFloat), rollMax: bool(v.rollMax),
    mixWin: ((g: Geo | undefined) => g && { ...g, z: 0 })(parseGeo(v.mixFloat)),
    // where the panels are docked; a layout from before panels could go anywhere is translated
    ...(v.dock !== undefined
      ? { dock: parseDock(v.dock), paneSize: parseSizes(v.paneSize), dockHome: parseHomes(v.dockHome) }
      : OLD_DOCK_KEYS.some(k => k in v) ? { dock: migrateDock(v), paneSize: migrateSizes(v) } : {}),
  };
  for (const k of Object.keys(p) as (keyof UiState)[]) if (p[k] === undefined) delete p[k];
  return p;
}
