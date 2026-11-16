// The keyboard lane (left of the grid): pressing a key plays it on the shown channel for as
// long as it is held — always, whatever the "hear notes while editing" preference; dragging
// glides to other keys. Further right on a key (towards its tip) = harder.
import { clamp } from '../../lib/format';
import { KW, type RollCtx, type RollPtr } from './types';

export function keyboardDrag(ctx: RollCtx, e: RollPtr) {
  const velAt = (x: number) => Math.round((0.35 + 0.65 * clamp(x / KW, 0, 1)) * 100) / 100;
  let cur = e.p.key;
  ctx.typingNote(true, cur, velAt(e.p.x)); ctx.setHoverKey(cur);
  ctx.beginDrag({ cursor: 'pointer' }, m => {
    if (m.p.key === cur) return;
    ctx.typingNote(false, cur, 0);
    cur = m.p.key;
    ctx.typingNote(true, cur, velAt(m.p.x)); ctx.setHoverKey(cur);
  }, () => { ctx.typingNote(false, cur, 0); ctx.setHoverKey(null); });
}
