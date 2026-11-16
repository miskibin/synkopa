// The stamp tool (C): one chord per click — the clicked key is the root, the header picks the
// chord type and inversion. Right-drag erases whole chords; hovering previews what a click
// would write, as a ghost in the grid and as text in the status line.
import { t, type TranslationKey } from '../../lib/i18n';
import { noteAt, notesText, SCALES, scaleMask, snapStep } from '../../lib/pattern';
import { NOTE_NAMES, noteName } from '../../lib/pitch';
import { CHORD_TYPES, chordName, chordPitches } from '../../lib/transforms';
import { MAX_STEP } from '../../lib/rollEdit';
import { velProp, unitOf, grewText, type RollCtx, type RollMove, type RollPtr, type RollTool } from './types';

/** The stamp picker's chord types and inversions (the header selects shown with the tool). */
export const STAMP_TYPES: { v: string; l: TranslationKey }[] = CHORD_TYPES.map(([id]) => ({ v: id, l: `b.pr.chord.${id}` as TranslationKey }));
export const STAMP_INVS: { v: number; l: TranslationKey }[] = [
  { v: 0, l: 'b.pr.stamp.inv0' }, { v: 1, l: 'b.pr.stamp.inv1' }, { v: 2, l: 'b.pr.stamp.inv2' }, { v: 3, l: 'b.pr.stamp.inv3' },
];

/** One chord per click, the clicked key the root, snapped to the grid. Re-stamping a step
 * replaces its chord on the channel (one op per stamp, clean undo). */
export function stampAt(ctx: RollCtx, e: RollPtr) {
  const p = e.p;
  const { type, inv } = ctx.stamp();
  const pitches = chordPitches(p.key, type, inv);
  if (!pitches) { ctx.say(t('b.pr.stampRange')); return; }
  const s = snapStep(p.step, unitOf(ctx, e), true);
  if (s < 0 || s + ctx.lastLen() > MAX_STEP) return;
  const kept = ctx.notes().filter(n => !(n.ch === ctx.ch().id && n.step === s));
  // trig:false like a drawn note: a len-1 stamp is a held note, not a step trigger
  const stamped = pitches.map(k => ({ ch: ctx.ch().id, step: s, len: ctx.lastLen(), trig: false, p: k, ...velProp(ctx) }));
  ctx.setStampGhost(null); // the ghost did its job; the next mousemove re-previews the next cell
  const lenBefore = ctx.pt().len;
  ctx.commitNotes([...kept, ...stamped], t('b.pr.gestStamp'));
  // one status line for both facts: a lone growth notice would be overwritten by the stamp's
  const grew = grewText(ctx, lenBefore);
  const label = t('b.pr.stamped', { label: chordName(pitches) });
  // the replacement moves notes around: the selection becomes the fresh chord, so a
  // stale index can never point at an unrelated note (Delete would eat the wrong one)
  ctx.setSel(new Set(stamped.map((_, j) => kept.length + j)));
  stamped.forEach(n => ctx.audition(n.p ?? p.key, n.v ?? 1));
  ctx.say(grew ? `${label} · ${grew}` : label);
}

/** The stamp tool's eraser: right-click removes all of the channel's notes at the
 * clicked step (the chord is the unit), sweeping with the drag like the eraser does. */
export function stampEraseDrag(ctx: RollCtx, e: RollPtr) {
  const g = ctx.gest(t('b.pe.gestDelNotes'));
  let notes = ctx.notes().slice(), n0 = notes.length;
  let lastStep: number | null = null;
  const del = (m: RollMove) => {
    // over a note, its own start decides which chord goes (a chord's body reaches
    // past its start cell); over empty grid, the snapped cell
    const hit = noteAt(notes, ctx.ch().id, ctx.ch(), m.p.step, m.p.key, ctx.geo().pps);
    const s = hit >= 0 ? notes[hit].step : snapStep(m.p.step, unitOf(ctx, m), true);
    if (s === lastStep) return;
    lastStep = s;
    const before = notes.length;
    notes = notes.filter(n => !(n.ch === ctx.ch().id && n.step === s));
    if (notes.length !== before) g.update([ctx.notesOp(notes)]);
  };
  ctx.setSel(new Set<number>());
  del(e);
  ctx.beginDrag({ cursor: 'not-allowed' }, del, () => { g.end(); if (notes.length < n0) ctx.say(t('b.pe.deleted', { x: notesText(n0 - notes.length) })); });
}

/** What a stamp click at `key` would write (status-bar preview): the root, the resulting
 * bass-first name, and whether that root sits in the key the roll header shows — stamping
 * itself stays key-free (any root is allowed; written notes are concrete). */
export function stampPreview(ctx: RollCtx, key: number) {
  const { type, inv } = ctx.stamp();
  const pitches = chordPitches(key, type, inv);
  if (!pitches) return '';
  const { root, scale } = ctx.keyScale();
  const pc = ((key - root) % 12 + 12) % 12;
  const inScale = scale !== 0 && (scaleMask(scale) & (1 << pc)) !== 0;
  const sc = SCALES.find(s => s[1] === scale);
  return `${noteName(key)} → ${chordName(pitches)}${inScale ? ' · ' + t('b.pr.stamp.inScale', { key: NOTE_NAMES[root], scale: sc ? t(sc[0]) : '' }) : ''}`;
}

/** The stamp hover: the grid ghost for the snapped cell, the preview in the status line. */
export function stampHover(ctx: RollCtx, m: RollMove) {
  const { type, inv } = ctx.stamp();
  const pitches = chordPitches(m.p.key, type, inv);
  const s = pitches ? snapStep(m.p.step, unitOf(ctx, m), true) : -1;
  const ok = !!pitches && s >= 0 && s + ctx.lastLen() <= MAX_STEP;
  const cur = ctx.stampGhost();
  if (!ok) {
    if (cur) ctx.setStampGhost(null);
    ctx.setHoverChord(pitches ? stampPreview(ctx, m.p.key) : '');
    return;
  }
  if (!cur || cur.s !== s || cur.len !== ctx.lastLen() || cur.pitches.length !== pitches.length
    || cur.pitches.some((q, i) => q !== pitches[i])) {
    ctx.setStampGhost({ s, len: ctx.lastLen(), pitches });
  }
  ctx.setHoverChord(stampPreview(ctx, m.p.key));
}

/** Stamp (C): the click writes the chord; the ghost and the copy cursor advertise it. */
export const stampTool: RollTool = {
  id: 'stamp',
  cursor: () => 'copy',
  hover: stampHover,
  down(ctx, e) { stampAt(ctx, e); },
};
