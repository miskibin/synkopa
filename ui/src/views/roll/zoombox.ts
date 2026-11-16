// Ctrl+right-drag on the grid: a box to zoom the view to (both axes); a Ctrl+right-click
// without a drag goes back to the previous view (the component keeps the views-back stack).
import { t } from '../../lib/i18n';
import { KW, type RollCtx, type RollMove, type RollPtr } from './types';

export function zoomBoxDown(ctx: RollCtx, e: RollPtr) {
  const p0 = e.p, G0 = ctx.geo(), ax = p0.x + G0.sx, ay = p0.y + G0.sy;
  let last: RollMove | null = null;
  ctx.beginDrag({ cursor: 'zoom-in' }, m => {
    last = m;
    const G = ctx.geo();
    ctx.setZoomBox({ x0: ax - G.sx, y0: ay - G.sy, x1: Math.max(KW, m.p.x), y1: m.p.y });
  }, () => {
    ctx.setZoomBox(null);
    const q = last ? last.p : p0;
    if (Math.hypot(q.x - p0.x, q.y - p0.y) < 4) {
      const v = ctx.popView();
      if (!v) return ctx.say(t('b.pr.zoomNoBack'));
      ctx.applyView(v); ctx.say(t('b.pr.zoomBack'));
      return;
    }
    const G = ctx.geo();
    // content units: steps across, rows (keys from the top) down
    const s0 = (ax - KW) / G.pps, s1 = (Math.max(KW, q.x) + G.sx - KW) / G.pps;
    const r0 = ay / G.rowH, r1 = (q.y + G.sy) / G.rowH;
    ctx.pushView();
    ctx.zoomTo(s0, s1, r0, r1);
    ctx.say(t('b.pr.zoomBox'));
  });
}
