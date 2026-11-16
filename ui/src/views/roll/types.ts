// The Piano roll's pointer-tool layer, extracted from PianoRoll.tsx (the architecture review's
// deep-5): each tool is a module behind the small `RollTool` interface, every tool reaches the
// roll only through `RollCtx` (implemented once by the `Roll` component, faked in tests), and
// pointers arrive as plain data (`RollPtr` / `RollMove`) instead of DOM events — so the
// dispatch chain and the gestures run in plain vitest like the rest of the UI's logic.
// `grid.ts` owns the dispatch order; PianoRoll.tsx owns the view, the ruler and the header.
import type { Channel, Id, Note, Op, Pattern } from '../../api/types';
import { t } from '../../lib/i18n';
import type { RollGeo } from '../patternDraw';

/** The five pointer tools of the roll header's P/B/E/D/C strip. */
export type Tool = 'draw' | 'paint' | 'select' | 'erase' | 'stamp';

/** Keyboard-lane width (px): the keys left of the grid. */
export const KW = 48;

/** Pointer position over the grid: canvas px, plus the note position it names (steps, key). */
export interface Pt { x: number; y: number; step: number; key: number }

/** Keyboard modifiers, mac Command folded into `ctrl` (the roll treats them alike). */
export interface Mods { alt: boolean; shift: boolean; ctrl: boolean }

/** A pointer-down on the roll grid. `raw` is the DOM event (absent in tests) for pan and
 * prevent-default only: every decision reads the plain fields. */
export interface RollPtr extends Mods { button: 0 | 1 | 2; p: Pt; raw?: PointerEvent }

/** A drag move: where the pointer is, which modifiers are held, and the event (for the
 * edge auto-scroll's client coords). */
export interface RollMove extends Mods { p: Pt; raw?: PointerEvent }

/** A rectangle in canvas px (marquee, zoom box). */
export interface Box { x0: number; y0: number; x1: number; y1: number }

/** The stamp tool's header parameters (chord type and inversion). */
export interface StampOpts { type: string; inv: number }

/** A saved view, for the views-back stack (Ctrl+right-click). */
export interface ViewSnap { sx: number; sy: number; zoom: number; rowH: number }

/** An open drag gesture against the engine: one undo step, updates coalesced per frame. */
export interface RollGesture { update(ops: Op[]): void; end(label?: string): void }

/** The stamp tool's grid ghost: what a click at the hovered cell would write. */
export interface StampGhost { s: number; len: number; pitches: number[] }

/** Everything a tool may touch: the roll's state, edits, previews and the drag glue.
 * One object per roll window, built by the `Roll` component. */
export interface RollCtx {
  // the shown pattern and channel (plain objects, not reactive proxies)
  pt(): Pattern;
  ch(): Channel;
  notes(): readonly Note[];
  chanNotes(): { n: Note; i: number }[];
  chans(): Map<Id, Channel>;
  // geometry, snap and hit tests
  geo(): RollGeo;
  snap(): number;
  /** The channel's note under (step, key), topmost first, or −1. */
  hit(step: number, key: number): number;
  /** True when `x` is on note `i`'s drag-right edge (resize). */
  edgeHit(i: number, x: number): boolean;
  /** Index of another channel's note (a ghost) under (step, key), or −1. */
  ghostAt(step: number, key: number): number;
  // the active pointer tool
  tool(): Tool;
  // selection: indices into the pattern's notes, the shown channel's only
  sel(): ReadonlySet<number>;
  setSel(s: Set<number>): void;
  // new-note defaults: the last touched length and velocity (FL)
  lastLen(): number;
  setLastLen(len: number): void;
  lastVel(): number;
  setLastVel(v: number): void;
  // edits: a finished change (one undo step), an op for a gesture's update, a new gesture
  commitNotes(notes: Note[], label: string): void;
  notesOp(notes: readonly Note[]): Op;
  gest(label: string): RollGesture;
  // sound and the status line
  audition(key: number, vel: number): void;
  typingNote(on: boolean, key: number, vel: number): void;
  say(msg: string): void;
  // previews and popovers
  setMarq(m: Box | null): void;
  setZoomBox(b: Box | null): void;
  setStampGhost(g: StampGhost | null): void;
  stampGhost(): StampGhost | null;
  setHoverChord(s: string): void;
  setHoverKey(k: number | null): void;
  propsOpen(): boolean;
  closeProps(): void;
  openProps(idx: number[]): void;
  // the stamp tool's type and inversion (the header's selects)
  stamp(): StampOpts;
  // the key/scale the roll header shows (the stamp preview's in-scale marker)
  keyScale(): { root: number; scale: number };
  // double right-click a ghost note switches the roll to that channel
  switchChannel(id: Id): void;
  // view navigation: zoom to a box (content units), the views-back stack
  zoomTo(s0: number, s1: number, r0: number, r1: number): void;
  pushView(): void;
  popView(): ViewSnap | undefined;
  applyView(v: ViewSnap): void;
  // DOM glue: real events in the app, absent under test
  /** Second tap on `key` within 320 ms (pointerdown-based double click). */
  dbl(key: string): boolean;
  /** Middle-button pan took the event. */
  pan(e: PointerEvent | undefined): boolean;
  /** A drag is open (the view-follow pauses). */
  setDragging(on: boolean): void;
  /** Window-level drag: moves arrive as plain data, `frame` coalesces them per animation
   * frame, `cursor` is forced on the page while it lasts. */
  beginDrag(o: { frame?: boolean; cursor?: string }, move: (m: RollMove) => void, up: (m: RollMove) => void): void;
  /** Modifier changes without a pointer move (a key pressed mid-drag); returns the unbind. */
  bindMods(cb: (m: Mods) => void): () => void;
  /** Edge auto-scroll for a drag: `replay` re-runs the drag after each scroll step. */
  autoScroll(replay: (m: RollMove) => void, axes?: { x?: boolean; y?: boolean }): { move(m: RollMove): void; stop(): void };
}

/** One pointer tool: its hover cursor, optional hover preview, and what a click starts.
 * `down` runs after the shared guards (pan, zoom box, keyboard lane, Ctrl/Shift selection)
 * have had their say — see `grid.ts` for that order. */
export interface RollTool {
  id: Tool;
  cursor(c: RollCtx, p: Pt, m: Mods, i: number): string;
  hover?(c: RollCtx, m: RollMove, i: number): void;
  down(c: RollCtx, e: RollPtr, i: number): void;
}

/** New notes take the last touched velocity (FL); 1 = full, stored as absent. */
export const velProp = (c: RollCtx) => (c.lastVel() < 0.995 ? { v: c.lastVel() } : {});

/** The snap unit the pointer's Alt picks: Alt = free, else the grid snap. */
export const unitOf = (c: RollCtx, m: { alt: boolean }) => (m.alt ? 0 : c.snap());

/** The pattern just grew past its end (a stamp or a new note drawn in the room to the right):
 * the loop follows the length, which is easy to miss while it plays. Returns the notice text,
 * '' when nothing grew — the caller decides how it reaches the status line (the stamp combines
 * it with its own message; a lone say would overwrite one or the other). */
export const grewText = (c: RollCtx, lenBefore: number) => {
  if (c.pt().len - lenBefore <= 1e-6) return '';
  const bars = c.pt().len / 4;
  return t('b.pr.patGrew', { n: bars >= 10 ? Math.round(bars) : +bars.toFixed(bars < 1 ? 2 : 1) });
};

/** The draw path's create has no later say to fight with: just say the growth. */
export const grewSay = (c: RollCtx, lenBefore: number) => {
  const msg = grewText(c, lenBefore);
  if (msg) c.say(msg);
};
