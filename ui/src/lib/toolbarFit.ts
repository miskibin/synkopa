// The toolbar's fit (two rows on shared columns, like FL Studio's bar): as room runs out, the least needed parts step
// aside one level at a time — measured, not guessed from the window width, so a long pattern name,
// a longer translation, the UI scale, the CPU meter or the native window buttons are all accounted
// for. Each level adds to the ones before it (styles.css, `.toolbar.fN`). Everything hidden stays
// reachable elsewhere: the loop on the timeline, TAP in the tempo's right-click
// menu, transport toggles and panels in the View menu (F5–F9).

/** The levels, least needed first (what each one hides is in styles.css). */
export const FIT_LEVELS = [
  'labels',      // f1: the panel buttons' names (Browser, Channel rack, Mixer)
  'typing',      // f2: the typing keyboard's name and octave, Follow's name
  'loop',        // f3: the loop range text
  'cmdkey',      // f4: Ctrl K's key cap (the search button stays)
  'keyname',     // f5: the song key's name (its tiny keyboard stays)
  'cpu',         // f6: the CPU meter (Settings → Audio keeps the underrun count)
  'tap',         // f7: TAP and the BPM tag (TAP stays in the tempo's right-click menu)
  'pattern',     // f8: the pattern name trimmed
  'key',         // f9: the song key (the Piano roll keeps it)
  'patname',     // f10: the pattern selector down to its number
  'panels',      // f11: the panel buttons (F5–F9 and the View menu still open them)
] as const;
export const FIT_MAX = FIT_LEVELS.length;

/**
 * The lowest level at which the bar fits: `apply(n)` lays the bar out at level `n`, `fits()` says
 * whether it now fits. Starts from nothing hidden every time, so the result depends only on the
 * room there is (no flicker between two levels); ends at the last level when nothing fits.
 */
export function fitLevel(apply: (n: number) => void, fits: () => boolean, max = FIT_MAX): number {
  let n = 0;
  apply(0);
  while (n < max && !fits()) apply(++n);
  return n;
}

/** The bar's grid as measured: it fits when its columns need no more than its width. */
export function gridFits(el: Pick<HTMLElement, 'scrollWidth' | 'clientWidth'>): boolean {
  return el.scrollWidth <= el.clientWidth + 1;
}
