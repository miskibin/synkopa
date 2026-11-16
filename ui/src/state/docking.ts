// Docking the Channel Rack, the Piano roll and the Mixer (#90): to the left, right or bottom edge,
// beside or sharing a strip with another docked panel, or floating. The where is `lib/dock.ts`; this
// is the doing: the title bar's dock button and right-click menu, the View menu, and dragging a
// title bar onto the dock guides (Visual Studio's), docked panels included (they float off first).
import { createSignal } from 'solid-js';
import { t, type TranslationKey } from '../lib/i18n';
import { clamp } from '../lib/format';
import {
  defaultDock, defaultHomes, defaultSizes, dockGuides, edgeTarget, EDGES, guideAt, homeOf, placePane, removePane,
  type DockTarget, type Edge, type Guide, type PaneId, type PaneRect, type Rect,
} from '../lib/dock';
import { track } from '../ui/drag';
import type { MenuItem } from './ui';
import { docked, openMenu, say, setU, U } from './ui';
import { leaveMixView, mixFloated, mixFront, showMixer } from './mixView';
import { openRack, openRoll, patWinFloated, rollMoved } from './pattern';

const NAME: Record<PaneId, TranslationKey> = { rack: 'a.mn.rack', roll: 'a.mn.roll', mix: 'a.mn.mixer' };
const WHERE: Record<Edge, TranslationKey> = { left: 'b.dk.atLeft', right: 'b.dk.atRight', bottom: 'b.dk.atBottom' };
export const paneName = (id: PaneId) => t(NAME[id]);

/** The panel is open (shown), docked or floating. */
export const paneOpen = (id: PaneId) => (id === 'rack' ? !!U.rackWin : id === 'roll' ? !!U.rollWin : !U.mixMin);
function showPane(id: PaneId) { if (id === 'rack') openRack(); else if (id === 'roll') openRoll(); else showMixer(); }

/** Dock `id` at `target` and show it. */
export function dockPane(id: PaneId, target: DockTarget) {
  leaveMixView();
  const l = placePane(U.dock, id, target);
  setU({ dock: l, dockHome: { ...U.dockHome, [id]: homeOf(l, id, U.dockHome[id]) } });
  say(t('b.dk.dockedAt', { name: paneName(id), where: t(WHERE[target.edge]) }));
  // after the message: opening can say why it cannot (no pattern, no channel)
  showPane(id);
}
/** Float `id` as a window (its dock place is remembered for the dock button). */
export function floatPane(id: PaneId) {
  if (!docked(id)) return;
  leaveMixView();
  setU({ dockHome: { ...U.dockHome, [id]: homeOf(U.dock, id, U.dockHome[id]) }, dock: removePane(U.dock, id) });
  say(t('b.dk.floats', { name: paneName(id) }));
  if (id === 'mix') { if (U.mixMin) setU('mixMin', false); mixFloated(); }
  else if (paneOpen(id)) patWinFloated(id); else showPane(id);
}
/** Dock `id` to an edge (menus): where it was on that edge before, else beside the Playlist. A panel already on that edge stays where it is. */
export function dockPaneTo(id: PaneId, edge: Edge) {
  const f = U.dock[edge].some(s => s.some(p => p.id === id));
  if (f) { if (!paneOpen(id)) showPane(id); return; }
  const h = U.dockHome[id];
  dockPane(id, edgeTarget(U.dock, id, edge, h.edge === edge && h.outer));
}
/** The title bar's dock button: a docked panel floats, a floating one goes back to where it was docked. */
export function toggleDock(id: PaneId) {
  if (docked(id)) floatPane(id); else dockPaneTo(id, U.dockHome[id].edge);
}
/** View → Reset window layout: the first-launch layout (the Mixer docked at the bottom, the pattern windows floating). */
export function resetLayout() {
  leaveMixView();
  setU({ dock: defaultDock(), paneSize: defaultSizes(), dockHome: defaultHomes(), mixMin: false, pickOpen: true });
  say(t('b.dk.reset'));
}

/** The dock / float choices for a panel (its title bar's right-click menu and the View menu). */
export function dockMenu(id: PaneId): MenuItem[] {
  const on = (e: Edge) => U.dock[e].some(s => s.some(p => p.id === id));
  const item = (e: Edge, label: TranslationKey): MenuItem => ({ t: 'item', label: t(label), checked: on(e), act: () => dockPaneTo(id, e) });
  return [
    item('left', 'b.dk.toLeft'), item('right', 'b.dk.toRight'), item('bottom', 'b.dk.toBottom'),
    { t: 'item', label: t('b.dk.toFloat'), checked: !docked(id), act: () => (docked(id) ? floatPane(id) : showPane(id)) },
  ];
}
/** The title bar's dock button: what it does, as a tooltip. */
export function dockTip(id: PaneId) {
  return docked(id) ? t('b.dk.floatTip') : t('b.dk.dockTip', { where: t(WHERE[U.dockHome[id].edge]) });
}

// ---------------------------------------------------------------- dragging a title bar

/** The dock guides on screen while a panel's title bar is dragged, and the one under the pointer. */
export const [guides, setGuides] = createSignal<{ list: Guide[]; hot: Guide | null } | null>(null);

const rectOf = (el: Element | null): Rect | null => {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0 ? { x: r.left, y: r.top, w: r.width, h: r.height } : null;
};
/** The guides for the layout on screen now (the dragged panel already floats, so it is not among them). */
function readGuides(): Guide[] {
  const stage = rectOf(document.querySelector('.stage')), center = rectOf(document.querySelector('.workrow > .plwrap'));
  if (!stage || !center) return [];
  const panes: PaneRect[] = [];
  for (const el of document.querySelectorAll<HTMLElement>('[data-dockpane]')) {
    const r = rectOf(el), [edge, strip, at] = (el.dataset.dockpane ?? '').split(':');
    if (r && EDGES.includes(edge as Edge)) panes.push({ edge: edge as Edge, strip: +strip, at: +at, rect: r });
  }
  const counts = { left: U.dock.left.length, right: U.dock.right.length, bottom: U.dock.bottom.length };
  return dockGuides(stage, center, panes, counts);
}

/** A panel's floating place, read and moved through the same paths as its frame. */
function floatGeo(id: PaneId) {
  if (id === 'mix') return { get: () => U.mixWin, set: (x: number, y: number) => setU('mixWin', w => ({ ...w, x, y })) };
  const k = id === 'rack' ? 'rackWin' : 'rollWin';
  return { get: () => U[k], set: (x: number, y: number) => { if (k === 'rollWin') rollMoved(); setU(k, w => w && { ...w, x, y }); } };
}

/**
 * A press on a dockable panel's title bar (floating or docked). A drag moves the window — a docked
 * panel floats off under the pointer first — and shows the dock guides; let go on a guide docks it
 * there, anywhere else it stays a window where it was dropped. Without a drag nothing happens.
 */
export function paneHeadDown(e: PointerEvent, id: PaneId) {
  if (e.button !== 0 || (e.target as HTMLElement).closest('button,select,input,.miniknob,.numf,.seg')) return;
  e.preventDefault();
  const sx = e.clientX, sy = e.clientY, head = (e.currentTarget as HTMLElement).getBoundingClientRect();
  let off: { dx: number; dy: number } | null = null;
  const g = floatGeo(id);
  track(ev => {
    if (!off) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      if (docked(id)) {
        // off the dock under the pointer, at its floating size, held where the title bar was grabbed
        // (a full-screen Piano roll comes off at a window's size: full screen it could not be dropped anywhere)
        const wasMax = id === 'roll' && U.rollMax;
        if (wasMax) rollMoved();
        floatPane(id);
        const o = g.get(); if (!o) return;
        if (wasMax) setU('rollWin', v => v && { ...v, w: Math.round(window.innerWidth * 0.62), h: Math.round(window.innerHeight * 0.58) });
        const w = g.get()!.w;
        off = { dx: Math.round(clamp(((sx - head.left) / Math.max(1, head.width)) * w, 40, Math.max(40, w - 40))), dy: clamp(sy - head.top, 4, 24) };
      } else {
        const o = g.get(); if (!o) return;
        off = { dx: sx - o.x, dy: sy - o.y };
      }
      if (id === 'mix') mixFront();
      setGuides({ list: readGuides(), hot: null });
    }
    const o = g.get(); if (!o) return;
    g.set(clamp(ev.clientX - off.dx, 80 - o.w, window.innerWidth - 80), clamp(ev.clientY - off.dy, 0, window.innerHeight - 40));
    const gs = guides();
    if (gs) { const hot = guideAt(gs.list, ev.clientX, ev.clientY); if (hot !== gs.hot) setGuides({ ...gs, hot }); }
  }, ev => {
    const gs = guides();
    setGuides(null);
    if (!off || !gs) return;
    const hot = guideAt(gs.list, ev.clientX, ev.clientY);
    if (hot) dockPane(id, hot.target);
  }, 'grabbing');
}

const DOCK_ICON: Record<Edge, string> = { left: 'M4 5h16v14H4z M10 5v14', right: 'M4 5h16v14H4z M14 5v14', bottom: 'M4 5h16v14H4z M4 13h16' };
const FLOAT_ICON = 'M9 9h11v10H9z M5 15H4V5h11v1';
/** The dock / float switch of a panel's title bar (`WinControls`): its glyph shows the edge it docks to. */
export function paneDock(id: PaneId) {
  const d = docked(id);
  return { docked: d, toggle: () => toggleDock(id), tip: dockTip(id), icon: d ? FLOAT_ICON : DOCK_ICON[U.dockHome[id].edge] };
}
/** Right-click on a dockable panel's title bar: where it can go. */
export function openDockMenu(e: MouseEvent, id: PaneId) {
  if ((e.target as HTMLElement).closest('button,select,input')) return;
  e.preventDefault(); e.stopPropagation();
  openMenu(e.clientX, e.clientY, [{ t: 'title', label: paneName(id) }, ...dockMenu(id)], 170);
}
