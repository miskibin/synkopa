// Dispatch: every pointer-down on the roll grid runs the shared guards in order, then the
// active tool — the one place that knows the whole interaction order (FL's):
// properties-popover cancel → middle-button pan → Ctrl+right zoom box → right-click clears
// the selection → the keyboard lane → note double-click properties → Ctrl selection →
// ghost channel switch → the tool itself. `rollMove` is the matching hover: the tool's
// cursor plus its previews (the stamp ghost, the chord under the pointer).
import { t } from '../../lib/i18n';
import { notesText } from '../../lib/pattern';
import { notesOfKey } from '../../lib/rollEdit';
import { noteName } from '../../lib/pitch';
import { chordName } from '../../lib/transforms';
import { moveDrag, drawTool, paintTool } from './draw';
import { eraseDrag, eraseTool } from './erase';
import { selectTool, marqueeDrag } from './select';
import { stampAt, stampEraseDrag, stampTool } from './stamp';
import { zoomBoxDown } from './zoombox';
import { keyboardDrag } from './keys';
import { KW, type RollCtx, type RollMove, type RollPtr, type RollTool, type Tool } from './types';

/** The tools by id — the roll header's strip picks from the same registry the dispatch runs. */
export const TOOL_BY_ID: Record<Tool, RollTool> = {
  draw: drawTool,
  paint: paintTool,
  select: selectTool,
  erase: eraseTool,
  stamp: stampTool,
};

/** The whole grid pointer-down, in the order above. */
export function rollDown(ctx: RollCtx, e: RollPtr): void {
  if (ctx.propsOpen()) ctx.closeProps(); // a click elsewhere cancels the properties popover
  if (ctx.pan(e.raw)) { ctx.setDragging(true); return; }
  if (e.button !== 0 && e.button !== 2) return;
  e.raw?.preventDefault();
  ctx.setDragging(true);
  const p = e.p;
  if (e.button === 2 && e.ctrl && p.x >= KW) return zoomBoxDown(ctx, e);
  // a right-click anywhere in the roll (keyboard, ghost notes, empty grid) drops the selection
  if (e.button === 2 && ctx.sel().size) ctx.setSel(new Set<number>());
  if (p.x < KW) {
    if (e.button !== 0) return;
    // Ctrl+click on a key: every note of that pitch (Shift too = add to the selection)
    if (e.ctrl) {
      const ks = notesOfKey(ctx.notes(), ctx.ch().id, ctx.ch(), p.key);
      const s = e.shift ? new Set([...ctx.sel(), ...ks]) : new Set(ks);
      ctx.setSel(s);
      ctx.say(ks.length ? t('b.pr.keySel', { key: noteName(p.key), x: notesText(s.size) }) : t('b.pr.keyNone', { key: noteName(p.key) }));
      return;
    }
    return keyboardDrag(ctx, e);
  }
  const tool = TOOL_BY_ID[ctx.tool()];
  const i = ctx.hit(p.step, p.key);
  // double-click a note: its properties (the selection's when it is selected). The first
  // click's move-drag has not moved anything (it starts after 3 px), so nothing is nudged.
  // Stamp mode never opens it: there a double-click is just two stamps (re-stamp replaces).
  if (e.button === 0 && i >= 0 && !e.ctrl && !e.shift && tool.id !== 'erase' && tool.id !== 'stamp' && ctx.dbl('rn' + i)) {
    const idx = ctx.sel().has(i)
      ? [...ctx.sel()].filter(k => k !== i && ctx.notes()[k]?.ch === ctx.ch().id).concat(i)
      : [i];
    if (!ctx.sel().has(i)) ctx.setSel(new Set([i]));
    ctx.openProps(idx);
    return;
  }
  // FL: Ctrl+drag selects with any tool; Ctrl+click toggles one note
  if (e.button === 0 && e.ctrl) {
    if (i < 0) return marqueeDrag(ctx, e, e.shift);
    const s = new Set(ctx.sel()); if (s.has(i)) s.delete(i); else s.add(i); ctx.setSel(s);
    ctx.say(s.size ? t('b.pe.selCopy', { x: notesText(s.size) }) : t('b.pe.noSel'));
    return;
  }
  if (e.button === 2 && i < 0) {
    // a ghost note of another channel under the pointer: double right-click edits that channel
    const gi = ctx.ghostAt(p.step, p.key);
    if (gi >= 0) {
      if (ctx.dbl('ghost' + ctx.notes()[gi].ch)) ctx.switchChannel(ctx.notes()[gi].ch);
      return;
    }
  }
  // the stamp tool: a click writes the chord rooted at the clicked key (right-click still erases, below)
  if (tool.id === 'stamp' && e.button === 0 && !e.ctrl && !e.shift) return stampAt(ctx, e);
  // Shift with any tool: drag on empty space adds a box to the roll's marquee or clones (FL)
  if (e.button === 0 && e.shift) {
    if (i < 0) return marqueeDrag(ctx, e, true);
    const toggle = () => {
      const s = new Set(ctx.sel()); if (s.has(i)) s.delete(i); else s.add(i); ctx.setSel(s);
      ctx.say(s.size ? t('b.pe.selCopy', { x: notesText(s.size) }) : t('b.pe.noSel'));
    };
    if (tool.id === 'erase') return toggle();
    const idx = ctx.sel().has(i)
      ? [...ctx.sel()].filter(k => k !== i && ctx.notes()[k]?.ch === ctx.ch().id).concat(i)
      : [i];
    ctx.setLastLen(ctx.notes()[i].len); ctx.setLastVel(ctx.notes()[i].v ?? 1);
    return moveDrag(ctx, e, idx, 'move', undefined, toggle);
  }
  // the stamp tool's right button: the chord is the unit, so the whole chord at the
  // clicked step goes (the plain eraser keeps its per-note sweep on every other tool)
  if (tool.id === 'stamp' && e.button === 2) return stampEraseDrag(ctx, e);
  if (e.button === 2 || tool.id === 'erase') return eraseDrag(ctx, e);
  if (tool.id === 'select') return selectTool.down(ctx, e, i);
  return tool.down(ctx, e, i); // draw and paint
}

/** The hover over the grid: returns the cursor, and updates the previews. The component
 * calls it only when no button is held (like the old `move` did). */
export function rollMove(ctx: RollCtx, m: RollMove): string {
  const p = m.p;
  if (p.x < KW) { ctx.setHoverChord(''); return 'pointer'; }
  const i = ctx.hit(p.step, p.key);
  const tool = TOOL_BY_ID[ctx.tool()];
  // previews run whatever modifiers are held (old move() did): only the cursor switches to
  // the crosshair, which says the click will select, not stamp
  if (tool.hover) tool.hover(ctx, m, i);
  else if (i >= 0) {
    // the chord sounding under the pointer: every note of the channel at that step
    const s0 = ctx.notes()[i].step;
    const ps = ctx.notes().filter(n => n.ch === ctx.ch().id && n.step === s0 && n.p != null).map(n => n.p!);
    ctx.setHoverChord(ps.length >= 2 ? chordName(ps) : '');
  } else ctx.setHoverChord('');
  return m.ctrl ? 'crosshair' : tool.cursor(ctx, p, m, i);
}
