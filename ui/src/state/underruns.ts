// Output underruns and CPU load: what FL Studio's toolbar CPU meter and the "Underruns" counter
// in its Audio settings show. The engine counts every callback that took longer than the audio it
// rendered (the device ran dry: a click) plus the xruns the driver reports, and sends the running
// total in each telemetry frame. The UI keeps its own reset point, so clicking the meter (or Reset
// in Settings → Audio) starts a fresh count without a round trip to the audio thread.
import { createSignal } from 'solid-js';

/** How long the meter stays in its warning colour after a new underrun. */
export const FLASH_MS = 900;

const [count, setCount] = createSignal(0);
const [flash, setFlash] = createSignal(false);
/** Engine total at the last reset, and at the last frame (−1 before the first frame). */
let base = 0, last = -1;
let flashTimer: ReturnType<typeof setTimeout> | undefined;

/** Underruns since the user last reset the counter (or since the app started). */
export const underruns = count;
/** True for a moment after a new underrun: the meter turns its warning colour. */
export const underrunFlash = flash;

/** Feed one telemetry frame's engine total. */
export function noteUnderruns(total: number) {
  if (total === last) return;
  // a smaller total is a fresh engine (it never counts down): its count starts from zero
  if (total < last || total < base) base = 0;
  const first = last < 0;
  last = total;
  setCount(total - base);
  // the first frame only syncs: those happened before the window was there to show them
  if (!first) {
    setFlash(true);
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => setFlash(false), FLASH_MS);
  }
}

export function resetUnderruns() {
  base = Math.max(last, 0);
  setCount(0);
  clearTimeout(flashTimer);
  setFlash(false);
}

/** For tests: forget everything, as at startup. */
export function _resetUnderrunState() {
  base = 0; last = -1;
  resetUnderruns();
}

/** Colour band of a callback load (0..1 of the buffer period): FL's meter crackles near 100 %. */
export function cpuBand(load: number): 'ok' | 'warn' | 'hot' {
  return load >= 0.85 ? 'hot' : load >= 0.6 ? 'warn' : 'ok';
}

/** The load as the meter prints it: whole percent, capped at 999 so the readout keeps its width. */
export function cpuText(load: number): string {
  if (!Number.isFinite(load) || load <= 0) return '0%';
  return Math.min(999, Math.round(load * 100)) + '%';
}
