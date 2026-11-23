// Mixer routing edits (lib/routing.ts has the rules): sends between inserts and the output into
// the master, each one undo step, with a status line saying what changed.
import type { Id, Op } from '../api/types';
import { t, tName } from '../lib/i18n';
import { canSend, depends, reaches, sendsOf, SEND_MAX, toMaster, withoutSend, withSend } from '../lib/routing';
import { insertOp } from './actions';
import { commit } from './edit';
import { S } from './store';
import { say } from './ui';

const nameOf = (id: Id) => { const m = S.project.inserts.find(x => x.id === id); return m ? tName(m.name) || 'Insert ' + id : String(id); };
export const pct = (g: number) => Math.round(g * 100) + '%';

/** Why `from` cannot send to `to`, or null when it can. */
export function sendBlock(from: Id, to: Id): string | null {
  if (from === 0) return t('c.rt.masterSends');
  if (from === to) return t('c.rt.self');
  if (to === 0) return t('c.rt.toMasterHint');
  if (reaches(S.project, to, from)) return t('c.rt.loop', { a: nameOf(to), b: nameOf(from) });
  if (depends(S.project, to, from)) return t('c.sc.sendLoop', { a: nameOf(to), b: nameOf(from) });
  return null;
}

/** Insert `from` sends to `to` (at 100 %, or the level it had). */
export function sendTo(from: Id, to: Id, gain = 1) {
  if (!canSend(S.project, from, to)) { const why = sendBlock(from, to); if (why) say(why); return; }
  const m = S.project.inserts.find(x => x.id === from); if (!m) return;
  const op = insertOp(from, { sends: withSend(m, to, gain).sends });
  if (!op) return;
  commit([op], t('c.rt.undoSend'));
  say(t('c.rt.sent', { a: nameOf(from), b: nameOf(to), v: pct(gain) }));
}

/** Removes `from`'s send to `to` (left with no output, `from` goes to the master again). */
export function removeSend(from: Id, to: Id) {
  const m = S.project.inserts.find(x => x.id === from); if (!m) return;
  const next = withoutSend(m, to);
  const op = insertOp(from, { sends: next.sends, master: next.master });
  if (!op) return;
  commit([op], t('c.rt.undoUnsend'));
  say(t('c.rt.unsent', { a: nameOf(from), b: nameOf(to) }) + (toMaster(m) || !next.master ? '' : ' · ' + t('c.rt.backToMaster')));
}

/** True when an effect elsewhere listens to insert `id` (its sidechain key). */
export const drivesKey = (id: Id) => S.project.inserts.some(x => x.fx.some(f => f.key === id));

/**
 * The insert's output into the master on or off. Off only while it sends somewhere, or while it
 * drives a sidechain: a "ghost kick", heard nowhere, still ducks the bass (as in FL Studio).
 */
export function setToMaster(id: Id, on: boolean) {
  const m = S.project.inserts.find(x => x.id === id); if (!m || id === 0) return;
  if (!on && !sendsOf(m).length && !drivesKey(id)) { say(t('c.rt.wouldSilence')); return; }
  const op = insertOp(id, { master: on });
  if (!op) return;
  commit([op], t('c.rt.undoMaster'));
  say(t(on ? 'c.rt.masterOn' : sendsOf(m).length ? 'c.rt.masterOff' : 'c.rt.ghost', { a: nameOf(id) }));
}

/** The op setting `from`'s send level to `to` (for a drag: one gesture, many updates). */
export function sendGainOp(from: Id, to: Id, gain: number): Op | null {
  const m = S.project.inserts.find(x => x.id === from); if (!m) return null;
  const g = Math.round(Math.min(SEND_MAX, Math.max(0, gain)) * 100) / 100;
  return insertOp(from, { sends: sendsOf(m).map(s => (s.to === to ? { to, gain: g } : s)) });
}

/** Where an insert's output goes, for a header: "→ Master", "→ Drums", "→ Master · Reverb 35%". */
export function routeSummary(id: Id): string {
  const m = S.project.inserts.find(x => x.id === id); if (!m) return '';
  if (id === 0) return t('c.mix.toOutput');
  const parts = [...(toMaster(m) ? ['Master'] : []), ...sendsOf(m).map(s => nameOf(s.to) + (s.gain === 1 ? '' : ' ' + pct(s.gain)))];
  // the inserts whose Compressor this one drives (sidechain): heard nowhere, but it matters
  const sc = S.project.inserts.filter(x => x.fx.some(f => f.key === id)).map(x => 'SC ' + nameOf(x.id));
  return '→ ' + ([...parts, ...sc].join(' · ') || t('c.rt.nowhere'));
}
