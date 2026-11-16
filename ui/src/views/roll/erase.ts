// The eraser: the D tool, and any right-drag on the grid. Sweeping over notes removes them
// one by one (per note — the stamp tool has its own per-chord eraser).
import { t } from '../../lib/i18n';
import { noteAt, notesText } from '../../lib/pattern';
import type { RollCtx, RollMove, RollPtr, RollTool } from './types';

export function eraseDrag(ctx: RollCtx, e: RollPtr) {
  const g = ctx.gest(t('b.pe.gestDelNotes'));
  // the drag's own copy of the notes: deletions shrink it, indices stay stable against it
  let notes = ctx.notes().slice(), n0 = notes.length;
  const del = (m: RollMove) => {
    const i = noteAt(notes, ctx.ch().id, ctx.ch(), m.p.step, m.p.key, ctx.geo().pps);
    if (i < 0) return;
    notes = notes.filter((_, k) => k !== i);
    g.update([ctx.notesOp(notes)]);
  };
  ctx.setSel(new Set<number>());
  del(e);
  ctx.beginDrag({ cursor: 'not-allowed' }, del, () => { g.end(); if (notes.length < n0) ctx.say(t('b.pe.deleted', { x: notesText(n0 - notes.length) })); });
}

/** Erase (D): the sweep, and the cursor that says so. */
export const eraseTool: RollTool = {
  id: 'erase',
  cursor: () => 'not-allowed',
  down(ctx, e) { eraseDrag(ctx, e); },
};
