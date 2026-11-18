// Which part of the app the keyboard belongs to after a click. The Playlist and other canvases call
// preventDefault on pointerdown, so the browser never moves the focus by itself: a search box or a
// toolbar button kept it, and Space typed a space or pressed the button again.
import { typingIn } from './keys';

/** The minimum of an element these rules need (a DOM element, or a stand-in in tests). */
export interface ZoneEl {
  tagName: string;
  isContentEditable?: boolean;
  getAttribute?(n: string): string | null;
  closest(sel: string): object | null;
  /** `never`: a DOM element takes a Node here, the tests' stand-in takes itself. */
  contains(other: never): boolean;
}

/** Panels that keep the keyboard while you work in them. */
export const FOCUS_ZONES = '.win, .browser, .mixdock, .mixview';
/** Panels whose clicks mean "I am not editing clips": the clip keys (Delete, Ctrl+X/C/V/B, nudge) stay out. */
export const AWAY_ZONES = '.win, .mixdock, .mixview';
/** A press here says nothing about where the user works (a menu belongs to whatever opened it). */
export const NEUTRAL = '.menu, .backdrop';

/**
 * Should a press on `target` take the keyboard away from the focused element `active`?
 * Yes when the press is in another panel (or outside any), unless it lands in a text field (which
 * takes the focus itself) or inside the focused element. A focused text field outside every panel
 * (a dialog's) is left to the browser, so clicking a label beside it does not end the edit.
 */
export function releasesFocus(active: ZoneEl | null, target: ZoneEl | null, isBody: boolean): boolean {
  if (!active || isBody || !target) return false;
  if (active === target || active.contains(target as never)) return false;
  if (typingIn(target)) return false;
  const from = active.closest(FOCUS_ZONES);
  if (!from) return !typingIn(active);
  return from !== target.closest(FOCUS_ZONES);
}

/**
 * The panel a press puts the clip keys away from, `null` when the press gives them (back) to the
 * Playlist, or `undefined` when it changes nothing (a menu).
 */
export function awayZone<T extends ZoneEl>(target: T | null): T | null | undefined {
  if (!target) return undefined;
  if (target.closest(NEUTRAL)) return undefined;
  return (target.closest(AWAY_ZONES) as T | null) ?? null;
}
