// Pointer drag helper: window-level move/up listeners plus a forced cursor while dragging.
import { edgeSpeed } from '../lib/viewNav';

/**
 * Force `cursor` on the whole page until `null`: a drag shows its own cursor (a closed hand while
 * something is held) even over elements that set their own, such as a canvas showing `grab` on hover.
 * A body cursor alone loses to those (styles.css `html.dragcur`).
 */
export function dragCursor(cursor: string | null) {
  const el = document.documentElement;
  if (cursor) { el.style.setProperty('--drag-cursor', cursor); el.classList.add('dragcur'); }
  else { el.classList.remove('dragcur'); el.style.removeProperty('--drag-cursor'); }
}

export function track(move: (e: PointerEvent) => void, up?: ((e: PointerEvent) => void) | null, cursor?: string) {
  const u = (ev: PointerEvent) => {
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', u);
    window.removeEventListener('pointercancel', u);
    if (cursor) dragCursor(null);
    up?.(ev);
  };
  if (cursor) dragCursor(cursor);
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', u);
  window.addEventListener('pointercancel', u);
}

/** How far (px) the pointer may travel between or during the two presses of a double click. */
export const DBL_SLOP = 4;
let lastTap: { k: string; t: number; x: number; y: number } | null = null;
/** Where the press being handled went down (recorded in capture, before any view's handler). */
let press: { x: number; y: number } | null = null;
let watching = false;
/**
 * A double click is two presses in one place. Without this, grabbing a knob again right after a
 * drag (or pressing two controls that share a key) counted as one and reset the value.
 */
function watchPresses() {
  if (watching || typeof window === 'undefined' || !window.addEventListener) return;
  watching = true;
  window.addEventListener('pointerdown', (e: PointerEvent) => { press = { x: e.clientX, y: e.clientY }; }, true);
  window.addEventListener('pointermove', (e: PointerEvent) => {
    // the first press turned into a drag: the next press starts over
    if (lastTap && e.buttons && Math.hypot(e.clientX - lastTap.x, e.clientY - lastTap.y) > DBL_SLOP) lastTap = null;
  }, true);
}
/**
 * True when this is the second press on `key` within 320 ms, at the same spot, with no drag in
 * between (pointerdown-based double click).
 */
export function dblCheck(key: string): boolean {
  watchPresses();
  const now = performance.now();
  const at = press ?? { x: 0, y: 0 };
  const d = !!lastTap && lastTap.k === key && now - lastTap.t < 320 && Math.hypot(at.x - lastTap.x, at.y - lastTap.y) <= DBL_SLOP;
  lastTap = d ? null : { k: key, t: now, x: at.x, y: at.y };
  return d;
}
watchPresses(); // from page load, so the first double click of a session counts too
/** Test hook: forget the last press. */
export function resetDblCheck() { lastTap = null; press = null; }

/** Drag speed for a continuous control: Ctrl or Shift is fine adjust (FL uses Ctrl; Shift is kept). */
export const fineMul = (e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) => (e.shiftKey || e.ctrlKey || e.metaKey ? 0.2 : 1);

/** `track` with the moves coalesced to one per animation frame (high-rate mice and touchpads send
 * several pointermoves per frame; each one re-applied a whole pattern). The last move is flushed
 * before `up`. */
export function trackFrame(move: (e: PointerEvent) => void, up?: ((e: PointerEvent) => void) | null, cursor?: string) {
  let last: PointerEvent | null = null, raf = 0;
  const run = () => { raf = 0; const e = last; last = null; if (e) move(e); };
  track(e => { last = e; if (!raf) raf = requestAnimationFrame(run); }, e => { if (raf) cancelAnimationFrame(raf); run(); up?.(e); }, cursor);
}

/**
 * Auto-scroll while dragging near the edge of a view. Call `move(e)` from the drag's move
 * handler and `stop()` when it ends. While the pointer sits in an edge zone of `rect()`, every
 * frame `scrollBy(dx, dy)` is called (return false when the view could not scroll any further)
 * and `replay(e)` re-runs the drag with the last pointer event, so what is dragged keeps up
 * with the view. `axes`: which directions scroll.
 */
export function edgeScroller(
  rect: () => { left: number; top: number; right: number; bottom: number },
  scrollBy: (dx: number, dy: number) => boolean,
  replay: (e: PointerEvent) => void,
  axes: { x?: boolean; y?: boolean } = { x: true, y: true },
) {
  let last: PointerEvent | null = null, raf = 0;
  const tick = () => {
    raf = 0;
    const e = last; if (!e) return;
    const r = rect();
    const dx = axes.x ? edgeSpeed(e.clientX, r.left, r.right) : 0, dy = axes.y ? edgeSpeed(e.clientY, r.top, r.bottom) : 0;
    if (!dx && !dy) return;
    if (scrollBy(dx, dy)) { replay(e); raf = requestAnimationFrame(tick); }
  };
  return {
    move(e: PointerEvent) { last = e; if (!raf) raf = requestAnimationFrame(tick); },
    stop() { last = null; if (raf) cancelAnimationFrame(raf); raf = 0; },
  };
}

/**
 * Middle-button pan (FL): call from pointerdown; returns true when it took the event. `at()` is
 * the view's scroll (px), `set(x, y)` applies a new one (the view clamps it).
 */
export function panDrag(e: PointerEvent, at: () => { x: number; y: number }, set: (x: number, y: number) => void): boolean {
  if (e.button !== 1) return false;
  e.preventDefault();
  const o = at(), sx = e.clientX, sy = e.clientY;
  track(ev => set(o.x - (ev.clientX - sx), o.y - (ev.clientY - sy)), null, 'grabbing');
  return true;
}
