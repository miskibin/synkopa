// The Mix view (F9): the mixer fills the workspace under the toolbar; the Playlist, the docked
// panels, the docked mixer and the floating Channel Rack / Piano roll are hidden (kept mounted,
// so their scroll and zoom survive), effect windows still float above. Leaving it gives back the
// arrange layout as it was on the way in (`takeArrange` / `restoreArrange`).
import { restoreArrange, takeArrange, type Arrange } from '../lib/mixer';
import { setU, U } from './ui';

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
