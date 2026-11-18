// The draw and paint tools (P/B): moving and resizing notes (with Shift-clone and the axis
// locks), painting many notes in one sweep, and the empty-grid create that starts every note.
import { t } from '../../lib/i18n';
import { cloneNotes } from '../../lib/noteClone';
import { keyOf, moveNotes, notesText, resizeNotes, snapStep, transpose } from '../../lib/pattern';
import { MAX_STEP } from '../../lib/rollEdit';
import type { Note } from '../../api/types';
import { noteShape, velProp, unitOf, grewSay, type Mods, type Pt, type RollCtx, type RollGesture, type RollPtr, type RollMove, type RollTool } from './types';

/** Drags notes `idx` (the last one leads). `clone`: Shift+drag — once the pointer really
 * moves, copies of the notes are made and moved instead (originals stay), one undo step;
 * released without moving, `clone` (the click) runs instead. */
export function moveDrag(ctx: RollCtx, e: RollPtr, idx0: number[], mode: 'move' | 'resize', g?: RollGesture, clone?: () => void) {
  let idx = idx0;
  let orig = ctx.notes().slice();
  const p0 = e.p, lead = orig[idx[idx.length - 1]] ?? orig[idx[0]];
  let gest = clone ? null : g ?? ctx.gest(mode === 'move' ? t('b.pe.gestMove') : t('b.pe.gestLen'));
  let lastKey = keyOf(lead, ctx.ch()), moved = false;
  // Axis lock (FL): Shift pressed after the drag started = time only, Ctrl = pitch only. Held
  // at pointer-down they keep their meaning (clone / select) and lock nothing.
  const shift0 = e.shift, ctrl0 = e.ctrl;
  let mods = { shift: shift0, ctrl: ctrl0 }, lastMove: RollMove | null = null, lockSaid = '';
  const run = (m: RollMove) => {
    lastMove = m;
    const q = m.p;
    if (!moved && Math.hypot(q.x - p0.x, q.y - p0.y) < 3) return;
    if (!moved && clone) {
      const r = cloneNotes(orig, idx);
      orig = r.notes; idx = r.idx;
      gest = ctx.gest(t('b.pe.gestClone'));
      gest.update([ctx.notesOp(orig)]);
      ctx.setSel(new Set(idx)); // after the copies exist: the selection drops indices that do not
      ctx.say(t('b.pe.cloning', { x: notesText(idx.length) }));
    }
    moved = true;
    if (!gest) return;
    const lockPitch = mode === 'move' && mods.shift && !shift0, lockTime = mode === 'move' && mods.ctrl && !ctrl0;
    const lock = lockTime ? 'time' : lockPitch ? 'pitch' : '';
    if (lock && lock !== lockSaid) ctx.say(t(lockTime ? 'b.pr.lockTime' : 'b.pr.lockPitch'));
    lockSaid = lock;
    const dKey = lockPitch ? 0 : q.key - p0.key;
    const notes = mode === 'move'
      ? (lockTime ? transpose(orig, idx, dKey, ctx.ch()) : moveNotes(orig, idx, q.step - p0.step, dKey, unitOf(ctx, m), ctx.ch()))
      : resizeNotes(orig, idx, q.step - p0.step, unitOf(ctx, m));
    gest.update([ctx.notesOp(notes)]);
    const ln = notes[idx[idx.length - 1]];
    if (ln) ctx.setLastLen(ln.len, !!ln.trig);
    if (mode === 'move' && ln && keyOf(ln, ctx.ch()) !== lastKey) { lastKey = keyOf(ln, ctx.ch()); ctx.hold(lastKey, ln.v ?? 1); }
  };
  const es = ctx.autoScroll(run, mode === 'resize' ? { x: true } : undefined);
  // a modifier pressed or released without moving the mouse applies at once
  const unmods = ctx.bindMods(m => {
    if (m.shift === mods.shift && m.ctrl === mods.ctrl) return;
    mods = m;
    if (lastMove) run(lastMove);
  });
  ctx.beginDrag({ frame: true, cursor: mode === 'move' ? 'grabbing' : 'ew-resize' }, m => { mods = { shift: m.shift, ctrl: m.ctrl }; run(m); if (moved) es.move(m); }, () => {
    unmods();
    es.stop();
    ctx.hold(null); // the held audition ends with the button
    gest?.end(); if (!moved) clone?.();
  });
}

/** Paint (B): further cells keep writing notes of the last touched length while held. */
export function paintDrag(ctx: RollCtx, e: RollPtr, g: RollGesture, startNotes: Note[]) {
  let notes = startNotes;
  const p0 = e.p;
  let lastCell = Math.floor(p0.step / (unitOf(ctx, e) || 1)), moved = false;
  const run = (m: RollMove) => {
    const q = m.p, u = unitOf(ctx, m) || 1, cell = Math.floor(q.step / u);
    if (Math.hypot(q.x - p0.x, q.y - p0.y) >= 3) moved = true;
    const shape = noteShape(ctx);
    if (cell === lastCell || q.step < 0 || q.step + shape.len > MAX_STEP) return;
    lastCell = cell;
    const s = cell * u;
    if (notes.some(n => n.ch === ctx.ch().id && Math.abs(n.step - s) < 1e-3)) return;
    notes = [...notes, { ch: ctx.ch().id, step: s, ...shape, p: q.key, ...velProp(ctx) }];
    g.update([ctx.notesOp(notes)]);
  };
  const es = ctx.autoScroll(run);
  ctx.beginDrag({ cursor: 'copy' }, m => { run(m); if (moved) es.move(m); }, () => { es.stop(); g.end(); });
}

/** A note under the pointer: the selection with it last. */
const dragIdx = (ctx: RollCtx, i: number, ownChannelToo: boolean) =>
  ctx.sel().has(i)
    ? [...ctx.sel()].filter(k => k !== i && (!ownChannelToo || ctx.notes()[k]?.ch === ctx.ch().id)).concat(i)
    : [i];

const noteToolDown = (ctx: RollCtx, e: RollPtr, i: number, paint: boolean) => {
  if (i >= 0) {
    const n = ctx.notes()[i];
    ctx.setLastLen(n.len, !!n.trig);
    ctx.setLastVel(n.v ?? 1);
    const idx = dragIdx(ctx, i, false);
    if (!ctx.sel().has(i)) ctx.setSel(new Set([i]));
    if (ctx.edgeHit(i, e.p.x)) return moveDrag(ctx, e, idx, 'resize');
    ctx.hold(keyOf(n, ctx.ch()), n.v ?? 1);
    return moveDrag(ctx, e, idx, 'move');
  }
  // a new note: grid start, last touched length, the row's key
  const s = snapStep(e.p.step, unitOf(ctx, e), true);
  const shape = noteShape(ctx);
  if (s < 0 || s + shape.len > MAX_STEP) return;
  const n: Note = { ch: ctx.ch().id, step: s, ...shape, p: e.p.key, ...velProp(ctx) };
  const notes = [...ctx.notes(), n];
  const lenBefore = ctx.pt().len;
  const g = ctx.gest(paint ? t('b.pe.gestPaint') : t('b.pe.gestNew'));
  g.update([ctx.notesOp(notes)]);
  grewSay(ctx, lenBefore);
  ctx.setSel(new Set([notes.length - 1]));
  // heard at the velocity it is written with, for as long as the button is held
  if (paint) ctx.audition(e.p.key, ctx.lastVel()); else ctx.hold(e.p.key, ctx.lastVel());
  if (paint) return paintDrag(ctx, e, g, notes);
  moveDrag(ctx, e, [notes.length - 1], 'move', g);
};

/** Over a note: grab, on its right edge resize; empty grid: the copy cursor (a click writes). */
const noteToolCursor = (ctx: RollCtx, p: Pt, _m: Mods, i: number) =>
  i >= 0 ? (ctx.edgeHit(i, p.x) ? 'ew-resize' : 'grab') : 'copy';

/** Draw (P): place and shape notes. */
export const drawTool: RollTool = {
  id: 'draw',
  cursor: noteToolCursor,
  down(ctx, e, i) { noteToolDown(ctx, e, i, false); },
};

/** Paint (B): draw that keeps note-ing as it sweeps. */
export const paintTool: RollTool = {
  id: 'paint',
  cursor: noteToolCursor,
  down(ctx, e, i) { noteToolDown(ctx, e, i, true); },
};
