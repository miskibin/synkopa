// Sidechain compression: a Compressor whose detector listens to another insert (`Fx.key`), so a
// kick ducks the bass. Two ways in, as in FL Studio and Ableton: from the Compressor window (pick
// the source) and from the source's mixer strip ("Sidechain to ▸ Bass"). One action serves every
// door (`synkopa sidechain`, the app's command, the mock): the engine keys the target's
// Compressor or adds one set up for ducking, and validates every key (`ops::validate_key`); this
// module is the menus, the greyed-out reasons and what the readout says.
import type { Fx, Id, Insert } from '../api/types';
import { be } from '../api/backend';
import { t } from '../lib/i18n';
import { keyError, takesKey } from '../lib/routing';
import * as A from './actions';
import { commit, enqueue, resync } from './edit';
import { findFx, insName, plain, S } from './store';
import { say, setU, type MenuItem } from './ui';

const insertOf = (id: Id) => S.project.inserts.find(m => m.id === id);

/** Why `fx` on insert `on` cannot listen to insert `key` (translated), or null. */
export function keyBlock(on: Id, key: Id, fx: Pick<Fx, 'type'> = { type: 'comp' }): string | null {
  const why = keyError(S.project, on, { type: fx.type, key });
  if (!why) return null;
  const k = insertOf(key);
  return why.includes('loop') ? t('c.sc.loop', { a: k ? insName(k) : String(key), b: insName(insertOf(on)!) }) : why;
}

/** Points the effect's detector at insert `key` (null = its own input). One undo step. */
export async function setKey(fxId: Id, key: Id | null) {
  const ff = findFx(fxId); if (!ff || !takesKey(ff.f.type) || (ff.f.key ?? null) === key) return;
  try {
    await enqueue(() => be().sidechain({ fxId, from: key ?? undefined, off: key == null, label: t('c.sc.undo') }));
    await resync();
    const k = key == null ? null : insertOf(key);
    say(k ? t('c.sc.saidOn', { from: insName(k), to: insName(ff.m) }) : t('c.sc.saidOff', { to: insName(ff.m) }));
  } catch (e) { say(String(e)); void resync(); }
}

/** The Compressor window's source menu: its own input, or any other insert (greyed with why). */
export function keyMenu(fxId: Id): MenuItem[] {
  const ff = findFx(fxId); if (!ff) return [];
  const own = ff.m.id, cur = ff.f.key ?? null;
  const others = S.project.inserts.filter(m => m.id !== 0 && m.id !== own);
  return [
    { t: 'title', label: t('c.sc.title') },
    { t: 'item', label: t('c.sc.own'), checked: cur == null, act: () => setKey(fxId, null) },
    { t: 'sep' },
    ...(others.length
      ? others.map(m => {
        const why = m.id === cur ? null : keyBlock(own, m.id, ff.f);
        return { t: 'item' as const, label: insName(m), checked: cur === m.id, disabled: !!why, title: why ?? undefined, act: () => setKey(fxId, m.id) };
      })
      : [{ t: 'item' as const, label: t('c.sc.noOthers'), disabled: true, act: () => {} }]),
  ];
}

/**
 * The Compressors on `to` that listen to `from`: what "Sidechain to" made (ducking). A Gate or
 * Filter keyed from `from` is set up in its own window and is not this menu's to show or undo.
 */
export const keyedFrom = (to: Insert, from: Id) => to.fx.filter(f => f.type === 'comp' && f.key === from);

/** Every effect on `to` whose detector listens to `from` (Compressor, Gate, Filter): a sidechain cable. */
export const listening = (to: Insert, from: Id) => to.fx.filter(f => takesKey(f.type) && f.key === from);

/** Removes a sidechain cable: every effect on `to` listening to `from` goes back to its own input. One undo step. */
export function unkey(from: Id, to: Id) {
  const dst = insertOf(to); if (!dst) return;
  const fx = listening(dst, from); if (!fx.length) return;
  commit(fx.map(f => { const x = plain(f) as Fx; delete x.key; return A.fxOp(x, to); }), t('c.sc.undo'));
  say(t('c.sc.saidOff', { to: insName(dst) }));
}

/**
 * FL's "Sidechain to this track", from the source strip: insert `from` drives a Compressor on
 * `to`. Keys the first Compressor of `to`'s chain that has no sidechain yet; without one, a
 * Compressor set up for ducking is added at the end and opened. Already keyed: removes the
 * sidechain (the Compressor stays, back on its own input). One undo step.
 */
export async function sidechainTo(from: Id, to: Id) {
  const src = insertOf(from), dst = insertOf(to); if (!src || !dst) return;
  const off = keyedFrom(dst, from).length > 0;
  const why = off ? null : keyBlock(to, from);
  if (why) { say(why); return; }
  try {
    const r = await enqueue(() => be().sidechain({ from, to, off, label: t('c.sc.undo') }));
    await resync();
    if (r.fxId != null) { setU({ mixSel: to, fxSel: r.fxId }); A.openPlug(r.fxId); }
    say(off ? t('c.sc.saidOff', { to: insName(dst) })
      : t(r.added ? 'c.sc.saidAdded' : 'c.sc.saidOn', { from: insName(src), to: insName(dst) }));
  } catch (e) { say(String(e)); void resync(); }
}

/** "Sidechain to ▸": every other insert, checked where this one already drives a Compressor. */
export function sidechainItems(m: Insert): MenuItem {
  const targets = S.project.inserts.filter(x => x.id !== 0 && x.id !== m.id);
  return {
    t: 'sub', label: t('c.sc.to'), items: targets.length
      ? targets.map(x => {
        const on = keyedFrom(x, m.id).length > 0, why = on ? null : keyBlock(x.id, m.id);
        return { t: 'item' as const, label: insName(x), checked: on, disabled: !!why, title: why ?? t('c.sc.toTip', { from: insName(m), to: insName(x) }),
          act: () => sidechainTo(m.id, x.id) };
      })
      : [{ t: 'item', label: t('c.mix.addInsert'), act: A.addInsert }],
  };
}

/** The inserts that drive a Compressor on `m`, for summaries ("SC ← Kick"). */
export const sidechainSources = (m: Insert): Insert[] =>
  [...new Set(m.fx.filter(f => takesKey(f.type) && f.key != null).map(f => f.key!))].map(insertOf).filter((x): x is Insert => !!x);
