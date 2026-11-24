// The toolbar's fit (one row, like FL Studio's bar): as room runs out, the least needed parts step
// aside one level at a time — measured, not guessed from the window width, so a long pattern name,
// a longer translation, the UI scale, the CPU meter or the native window buttons are all accounted
// for. Each level adds to the ones before it (styles.css, `.toolbar.fN`). Everything hidden stays
// reachable elsewhere: loop and song position on the timeline, TAP in the tempo's right-click
// menu, transport toggles and panels in the View menu (F5–F9), Ask on F1.

/** The levels, least needed first (what each one hides is in styles.css). */
export const FIT_LEVELS = [
  'loop',        // f1: the loop range text
  'labels',      // f2: the panel buttons' names (Browser, Channel rack, Piano roll, Mixer), the BPM tag, the typing octave
  'toggles',     // f3: 1 BAR count-in and monitor
  'metro',       // f4: metronome
  'typing',      // f5: the typing keyboard's name
  'pattern',     // f6: the pattern name trimmed, its + button
  'clock',       // f7: the song position (the tempo stays)
  'tap',         // f8: TAP, main volume, the unsaved mark
  'ask',         // f9: Ask, a shorter pattern name, icon-only update status
  'meters',      // f10: the clip label, the CPU meter, Ctrl K
  'patname',     // f11: the pattern selector down to its number
  'panels',      // f12: the panel buttons (F5–F9 and the View menu still open them)
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

interface Box { left: number; right: number }
/** The bar's sections as measured: their boxes and whether their content overflows them. */
export interface BarMeasure { bar: Box; left: Box; mid: Box; right: Box; leftOver: boolean; midOver: boolean; rightOver: boolean }
/** No section clips its content, none runs past the bar's right edge, none overlaps the next. */
export function barFits(m: BarMeasure): boolean {
  if (m.leftOver || m.midOver || m.rightOver) return false;
  if (m.right.right > m.bar.right + 1) return false;
  return m.mid.left >= m.left.right - 1 && m.right.left >= m.mid.right - 1;
}
