// The select tool (E): a marquee box on empty grid, and a drag on a note that moves it with
// its selection. The marquee is also the shared Shift/Ctrl box other tools reach through the
// dispatch chain.
import { t } from '../../lib/i18n';
import { keyOf, noteWidth, notesText } from '../../lib/pattern';
import { rollX, rollY } from '../patternDraw';
import { moveDrag } from './draw';
import type { RollCtx, RollMove, RollPtr, RollTool } from './types';

export function marqueeDrag(ctx: RollCtx, e: RollPtr, add: boolean) {
  const p0 = e.p, base = add ? new Set(ctx.sel()) : new Set<number>();
  // the box's anchor in content px: it stays on its notes while the view auto-scrolls
  const G0 = ctx.geo(), ax = p0.x + G0.sx, ay = p0.y + G0.sy;
  let moved = false;
  // a plain press on empty grid drops the selection at once (FL): a click without a drag deselects too
  if (!add && ctx.sel().size) ctx.setSel(new Set<number>());
  ctx.setMarq({ x0: p0.x, y0: p0.y, x1: p0.x, y1: p0.y });
  const run = (m: RollMove) => {
    const q = m.p, G = ctx.geo(), sx = ax - G.sx, sy = ay - G.sy;
    if (Math.hypot(q.x - p0.x, q.y - p0.y) >= 3) moved = true;
    ctx.setMarq({ x0: sx, y0: sy, x1: q.x, y1: q.y });
    const [x0, x1] = [Math.min(sx, q.x), Math.max(sx, q.x)], [y0, y1] = [Math.min(sy, q.y), Math.max(sy, q.y)];
    const s = new Set(base);
    for (const { n, i } of ctx.chanNotes()) {
      const nx = rollX(G, n.step), nw = noteWidth(n, G.pps), ny = rollY(G, keyOf(n, ctx.ch()));
      if (nx < x1 && nx + nw > x0 && ny < y1 && ny + G.rowH > y0) s.add(i);
    }
    ctx.setSel(s);
  };
  const es = ctx.autoScroll(run);
  ctx.beginDrag({ frame: true }, m => { run(m); if (moved) es.move(m); }, () => {
    es.stop(); ctx.setMarq(null);
    ctx.say(ctx.sel().size ? t('b.pe.selHint', { x: notesText(ctx.sel().size) }) : t('b.pe.noSel'));
  });
}

/** Select (E): a box on empty space; a note drag moves it with its selection. */
export const selectTool: RollTool = {
  id: 'select',
  cursor: (_c, _p, _m, i) => (i >= 0 ? 'grab' : 'crosshair'),
  down(ctx, e, i) {
    if (i < 0) return marqueeDrag(ctx, e, false);
    const idx = ctx.sel().has(i) ? [...ctx.sel()] : [i];
    if (!ctx.sel().has(i)) ctx.setSel(new Set([i]));
    ctx.setLastLen(ctx.notes()[i].len, !!ctx.notes()[i].trig); ctx.setLastVel(ctx.notes()[i].v ?? 1);
    return moveDrag(ctx, e, idx.filter(k => k !== i).concat(i), 'move');
  },
};
