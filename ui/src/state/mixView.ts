// The Mix view (F9): the mixer fills the workspace under the toolbar; the Playlist, the docked
// panels, the docked mixer and the floating Channel Rack / Piano roll are hidden (kept mounted,
// so their scroll and zoom survive), effect windows still float above. Leaving it gives back the
// arrange layout as it was on the way in (`takeArrange` / `restoreArrange`).
import { createSignal } from 'solid-js';
import type { Id } from '../api/types';
import { t } from '../lib/i18n';
import { restoreArrange, takeArrange, type Arrange } from '../lib/mixer';
import { insName, S } from './store';
import { docked, say, setU, U } from './ui';
import { topFloating } from './winOrder';

let saved: Arrange | null = null, savedPlugs = new Set<number>();

/** The keyboard was in something the Mix view hides (the Playlist, a docked or floating pattern panel): let it go. */
function releaseHiddenFocus() {
  const ae = document.activeElement as HTMLElement | null;
  if (ae && ae !== document.body && (ae.closest('.workcol') || ae.closest('.patwin') || ae.closest('.mixdock'))) ae.blur();
}

export function enterMixView() {
  if (U.mixView) return;
  saved = takeArrange(U);
  savedPlugs = new Set(U.plugs.map(p => p.fxId));
  releaseHiddenFocus();
  setU('mixView', true);
}

/** Back to the arrange layout. True when the Mix view was on (callers about to show a pattern window then do it in the arrange layout). */
export function leaveMixView(): boolean {
  if (!U.mixView) return false;
  const a = saved; saved = null;
  const patch = a ? restoreArrange(a) : {};
  // an effect window opened in the Mix view stays shown: the arrange layout's "windows hidden" does not swallow it
  if (U.plugs.some(p => !savedPlugs.has(p.fxId))) delete patch.plugsHidden;
  setU({ ...patch, mixView: false });
  return true;
}

export function toggleMixView() { if (!leaveMixView()) enterMixView(); }

/** The insert the mixer was last asked to show (`n` changes on every request, so the same insert scrolls into view again). */
const [revealed, setRevealed] = createSignal<{ id: Id; n: number } | null>(null);
export { revealed };
/**
 * Double-click on something that plays through a mixer insert (a sample or channel window's
 * header, a track's number): the mixer shows that insert — expanded when it was collapsed, the
 * strip selected and scrolled into view, its effects in the chain panel (FL).
 */
export function showInsert(id: Id | null | undefined) {
  const m = id == null ? undefined : S.project.inserts.find(x => x.id === id);
  if (!m) return;
  setU({ mixSel: m.id, fxSel: m.fx[0]?.id ?? null });
  if (!U.mixView) showMixer();
  setRevealed({ id: m.id, n: (revealed()?.n ?? 0) + 1 });
  say(t('c.mix.shown', { name: insName(m) }));
}

// ---- the Mixer as a window (FL: F9 shows / hides it; docked or detached like the other windows)

/** Bring the floating Mixer in front of the other floating windows (showing them if F5 tucked them away). */
export function mixFront() {
  if (docked('mix') || U.mixMin) return;
  if (U.plugsHidden) setU('plugsHidden', false);
  if (U.mixWin.z === U.zTop) return;
  const z = U.zTop + 1;
  setU({ zTop: z, mixWin: { ...U.mixWin, z } });
}
/** Show the Mixer (docked: open its panel; floating: open the window in front). */
export function showMixer() {
  if (U.mixMin) setU('mixMin', false);
  mixFront();
}
/** Hide the Mixer (×, Esc, F9): docked it keeps its thin handle, floating it closes. */
export function hideMixer() {
  leaveMixView();
  setU('mixMin', true);
  say(t(docked('mix') ? 'c.mv.collapsed' : 'c.mv.hidden'));
}
/** The Mixer floats again: its window kept on screen and in front. */
export function mixFloated() {
  const W = window.innerWidth, H = window.innerHeight, o = U.mixWin;
  const w = Math.min(o.w, W - 20), h = Math.min(o.h, H - 60);
  setU('mixWin', { ...o, w, h, x: Math.max(0, Math.min(o.x, W - w)), y: Math.max(44, Math.min(o.y, H - h)) });
  mixFront();
}
/** F9 / the toolbar's mixer button: maximized → back; hidden → shown; floating behind → in front; else hidden. */
export function toggleMixer() {
  if (leaveMixView()) return;
  if (U.mixMin) return showMixer();
  if (!docked('mix') && topFloating()?.kind !== 'mix') return mixFront();
  hideMixer();
}
