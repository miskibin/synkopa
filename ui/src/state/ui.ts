// UI-only state (not saved, not undone).
import { createEffect, createRoot } from 'solid-js';
import { createStore } from 'solid-js/store';
import { t } from '../lib/i18n';
import type { ClipKind, Id } from '../api/types';
import type { MenuGlyph } from '../ui/menuIcons';
import { parseLayout, RACK_DOCK_W, ROLL_DOCK_H, type Geo } from '../lib/layout';
export { parseLayout } from '../lib/layout';

export type Tool = 'draw' | 'paint' | 'erase' | 'mute' | 'slice' | 'select';

/** `z`: stacking order — the most recently opened/focused window has the highest. */
export interface PlugWin { fxId: Id; x: number; y: number; w: number; z: number }
/** `melody`: open with this channel's notes as the reference (`span`: seconds of the source; a Piano roll selection). */
export interface PitchWin { clipId: Id; x: number; y: number; w: number; h: number; z?: number; melody?: { channel: Id; span?: [number, number] | null } }
export type SampleTarget = { kind: 'clip' | 'channel'; id: Id };
export type EditorWin = SampleTarget & { x: number; y: number; w: number };
export interface MusicWin { clipId?: Id; fxId?: Id }
/**
 * A pattern window: the Channel Rack (F6) or the Piano roll (F7), open while set. Both show the
 * current pattern (`UiState.pat`); the piano roll edits the selected channel. Floating, they live
 * in the effect-window layer (F5 / a playlist click hide them with the effect windows, "Windows"
 * brings them back) at x/y/w/h; docked (`rackDock` / `rollDock`) they sit in the main layout and
 * x/y/w/h keep the floating place for when they float again.
 */
export interface PatWin { x: number; y: number; w: number; h: number; z: number }

export type MenuItem =
  | { t: 'title'; label: string }
  | { t: 'sep' }
  /** Small section heading inside a menu. */
  | { t: 'sec'; label: string }
  /** Opens a nested menu on hover. `glyph` overrides the icon guessed from the label. */
  | { t: 'sub'; label: string; items: MenuItem[]; glyph?: MenuGlyph }
  /** `icon` (24×24 stroke path in a coloured tile) wins over `glyph`, which wins over the label guess. */
  | { t: 'item'; label: string; kbd?: string; act: () => void; danger?: boolean; disabled?: boolean; icon?: string; iconColor?: string; title?: string; glyph?: MenuGlyph;
      /** A toggle: shows a check mark in the icon slot while on (and nothing while off). */
      checked?: boolean }
  | { t: 'swatches'; current?: string; pick: (c: string) => void }
  /** Row of instrument icons; picking the current one clears it. */
  | { t: 'icons'; current?: string | null; color?: string; pick: (id: string | null) => void }
  | { t: 'gain'; clipId: Id }
  | { t: 'rename'; value: string; done: (v: string) => void }
  /** A row of small toggle buttons (one click picks, menu stays coherent with the current value). */
  | { t: 'chips'; label?: string; options: string[]; active: number; pick: (i: number) => void; disabled?: boolean; close?: boolean };

export interface Menu { x: number; y: number; items: MenuItem[] }

export interface UiState {
  tool: Tool; snap: number; bw: number;
  sel: Id[]; cur: { kind: ClipKind; ref: Id } | null;
  start: number; follow: boolean; alt: boolean;
  /** The docked mixer is collapsed to its thin handle (`mixH`: the height it opens to again). */
  mixMin: boolean; mixH: number; mixSel: Id; fxSel: Id | null; fxDrag: Id | null;
  /** F9: the Mix view fills the workspace with the mixer (not saved: a launch starts in the arrange layout). */
  mixView: boolean;
  plugs: PlugWin[];
  /** effect windows tucked behind the playlist (FL-style: clicking the playlist brings it to the front) */
  plugsHidden: boolean;
  editor: EditorWin | null; menu: Menu | null; help: boolean; settings: boolean; exportOpen: boolean;
  /** True while `exportAudio` is in flight (so Escape can abort and report). */
  exportBusy: boolean;
  projectZipBusy: boolean;
  openingProject: boolean;
  vstPicker: { insId: Id } | null;
  /** add-effect picker, anchored at (x, y) (−1 = centred) */
  fxPicker: { insId: Id; x: number; y: number } | null;
  /** "Load FX chain preset" list for an insert, opened at x/y. */
  fxChainWin: { insId: Id; x: number; y: number } | null;
  musicWin: MusicWin | null;
  /** The "Missing audio" dialog (relink moved files). */
  missingOpen: boolean;
  /** The "Import MIDI" dialog: the file, where a new pattern goes, or the channel a part goes into. */
  midiWin: { path: string; beat?: number; track?: number; into?: { pattern: Id; ch: Id } } | null;
  readout: string;
  headW: number; pickW: number; pickOpen: boolean; browserTab: 'project' | 'library';
  ghost: { x: number; y: number; name: string; color: string } | null;
  dragOver: boolean;
  eqSel: Record<number, number>;
  fetMode: Record<number, 'gr' | 'out'>;
  pitchEd: PitchWin | null;
  /** The current pattern (FL's pattern selector): what the Channel Rack, the Piano roll and PAT mode show. */
  pat: Id | null;
  /** The selected Channel Rack channel: the Piano roll's channel, the graph editor's, the typing keyboard's. */
  selCh: Id | null;
  /** More selected Channel Rack channels (Shift+click / Shift+Ctrl+click), `selCh` among them; empty = only `selCh`. */
  chSel: Id[];
  rackWin: PatWin | null;
  rollWin: PatWin | null;
  /** The Channel Rack is docked left of the Playlist (`rackDockW` px wide; 0 = as wide as the pattern) instead of floating. */
  rackDock: boolean; rackDockW: number;
  /** The Piano roll is docked under the Playlist, above the Mixer (`rollDockH` px high) instead of floating. */
  rollDock: boolean; rollDockH: number;
  /** Where the floating Channel Rack / Piano roll was last (they reopen there, also after a restart). */
  rackFloat: Geo | null; rollFloat: Geo | null;
  /** The floating Piano roll fills everything under the toolbar (follows the app window, reopens full screen). */
  rollMax: boolean;
  /** The Channel Rack's graph editor (per-step velocity / key of the selected channel) is shown. */
  rackGraph: 'vel' | 'key' | null;
  /** True while the engine loops a pattern on its own (telemetry `patBeat` ≥ 0). */
  patPlaying: boolean;
  /** track id → take lanes expanded */
  takesOpen: Record<number, boolean>;
  /** comp time selection (comp beats): the last swiped range, the target of Ctrl+↑/↓ */
  compSel: { clipId: Id; from: number; to: number } | null;
  /** selected points of one automation clip's envelope (indices into its points) */
  autoSel: { clip: Id; idx: number[] } | null;
  /** next z for plugin windows (monotonic) */
  zTop: number;
  marquee: { x0: number; y0: number; x1: number; y1: number } | null;
  /** True while the engine reports playback (from telemetry flags / transport replies). */
  playing: boolean;
  previewing: boolean;
  previewPath: string | null;
  /** Dragging an audio clip's edge stretches it instead of trimming (Shift+M). Not saved: a
   *  launch starts in trim mode, so a forgotten stretch mode cannot surprise the next session. */
  stretchMode: boolean;
}

export const [U, setU] = createStore<UiState>({
  tool: 'draw', snap: -1, bw: 22, sel: [], cur: { kind: 'pat', ref: 1 },
  start: 0, follow: true, alt: false,
  mixMin: false, mixH: 300, mixView: false, mixSel: 6, fxSel: 903, fxDrag: null,
  plugs: [], plugsHidden: false, editor: null, menu: null, help: false, settings: false, exportOpen: false, exportBusy: false, vstPicker: null, fxPicker: null, fxChainWin: null, musicWin: null, missingOpen: false, midiWin: null,
  openingProject: false,
  projectZipBusy: false,
  readout: t('d.ui.ready'),
  headW: 196, pickW: 220, pickOpen: true, browserTab: 'project',
  ghost: null, dragOver: false, eqSel: {}, fetMode: {}, pitchEd: null, pat: 1, selCh: null, chSel: [], rackWin: null, rollWin: null,
  rackDock: false, rackDockW: RACK_DOCK_W, rollDock: false, rollDockH: ROLL_DOCK_H, rackFloat: null, rollFloat: null, rollMax: true,
  rackGraph: null, patPlaying: false, takesOpen: {}, compSel: null, autoSel: null, zTop: 1, marquee: null,
  playing: false, previewing: false, previewPath: null, stretchMode: false,
});

// Until the first status message the readout follows the language (a live switch re-translates "Ready").
let readoutPristine = true;
createRoot(() => createEffect(() => { const ready = t('d.ui.ready'); if (readoutPristine) setU('readout', ready); }));

// The working set-up survives a restart: tool, snap, zoom, follow and the panel layout used to
// reset on every launch (the mixer reopened, the browser went back to the Project tab, ...).
const LAYOUT_KEY = 'wave.layout';
try { setU(parseLayout(localStorage.getItem(LAYOUT_KEY))); } catch { /* private window */ }
const geo = (w: PatWin): Geo => ({ x: w.x, y: w.y, w: w.w, h: w.h });
createRoot(() => createEffect(() => {
  const s = JSON.stringify({ tool: U.tool, snap: U.snap, bw: U.bw, follow: U.follow, mixMin: U.mixMin, mixH: U.mixH, headW: U.headW, pickW: U.pickW, pickOpen: U.pickOpen, browserTab: U.browserTab,
    rackDock: U.rackDock, rackDockW: U.rackDockW, rollDock: U.rollDock, rollDockH: U.rollDockH, rollMax: U.rollMax,
    // the floating place: live while the window is open (a restart with it open keeps it), else where it closed
    rackFloat: U.rackWin ? geo(U.rackWin) : U.rackFloat, rollFloat: U.rollWin ? geo(U.rollWin) : U.rollFloat });
  try { localStorage.setItem(LAYOUT_KEY, s); } catch { /* private window */ }
}));
export const say = (msg: string) => { readoutPristine = false; setU('readout', msg); };
export const closeMenu = () => { if (U.menu) setU('menu', null); };
export function menuPos(x: number, y: number, h: number) {
  return { x: Math.max(4, Math.min(x, window.innerWidth - 240)), y: Math.max(4, Math.min(y, window.innerHeight - h)) };
}
export function openMenu(x: number, y: number, items: MenuItem[], h = 300) {
  setU('menu', { ...menuPos(x, y, h), items });
}
export function openRename(x: number, y: number, value: string, done: (v: string) => void, title = t('d.ui.renameTitle')) {
  openMenu(x, y, [{ t: 'title', label: title }, { t: 'rename', value, done }], 80);
}
