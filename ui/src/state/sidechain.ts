// Sidechain compression: a Compressor whose detector listens to another insert (`Fx.key`), so a
// kick ducks the bass. Two ways in, as in FL Studio and Ableton: from the Compressor window (pick
// the source) and from the source's mixer strip ("Sidechain to ▸ Bass", which keys the target's
// Compressor or adds one set up for ducking). The engine validates every key (`ops::validate_key`);
// these helpers grey out what it would refuse and say why.
import type { Fx, Id, Insert } from '../api/types';
import { t } from '../lib/i18n';
import { newFx } from '../lib/presets';
import { keyError, takesKey } from '../lib/routing';
import * as A from './actions';
import { commit } from './edit';
import { allocId, findFx, insName, plain, S } from './store';
import { say, setU, type MenuItem } from './ui';

/** Ducking settings for a Compressor added by "Sidechain to": fast attack, a release that lets
 *  the sound swell back before the next beat, deep enough to hear the pump on a typical kick. */
export const DUCK_PARAMS: Record<string, number> = { thr: -24, ratio: 6, att: 1, rel: 150, gain: 0, det: 0 };

const insertOf = (id: Id) => S.project.inserts.find(m => m.id === id);

/** Why `fx` on insert `on` cannot listen to insert `key` (translated), or null. */
export function keyBlock(on: Id, key: Id, fx: Pick<Fx, 'type'> = { type: 'comp' }): string | null {
  const why = keyError(S.project, on, { type: fx.type, key });
  if (!why) return null;
  const k = insertOf(key);
  return why.includes('loop') ? t('c.sc.loop', { a: k ? insName(k) : String(key), b: insName(insertOf(on)!) }) : why;
}

/** Points the effect's detector at insert `key` (null = its own input). One undo step. */
export function setKey(fxId: Id, key: Id | null) {
  const ff = findFx(fxId); if (!ff || !takesKey(ff.f.type) || (ff.f.key ?? null) === key) return;
  const fx = plain(ff.f) as Fx;
  if (key == null) delete fx.key; else fx.key = key;
  commit([A.fxOp(fx, ff.m.id)], t('c.sc.undo'));
  const k = key == null ? null : insertOf(key);
  say(k ? t('c.sc.saidOn', { from: insName(k), to: insName(ff.m) }) : t('c.sc.saidOff', { to: insName(ff.m) }));
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

/** The Compressors on `to` that listen to `from`. */
export const keyedFrom = (to: Insert, from: Id) => to.fx.filter(f => takesKey(f.type) && f.key === from);

/**
 * FL's "Sidechain to this track", from the source strip: insert `from` drives a Compressor on
 * `to`. Keys the first Compressor of `to`'s chain that has no sidechain yet; without one, adds a
 * Compressor set up for ducking at the end of the chain and opens it. Already keyed: removes the
 * sidechain (the Compressor stays, back on its own input). One undo step.
 */
export function sidechainTo(from: Id, to: Id) {
  const src = insertOf(from), dst = insertOf(to); if (!src || !dst || !S.catalog) return;
  const keyed = keyedFrom(dst, from);
  if (keyed.length) {
    commit(keyed.map(f => { const x = plain(f) as Fx; delete x.key; return A.fxOp(x, to); }), t('c.sc.undo'));
    say(t('c.sc.saidOff', { to: insName(dst) }));
    return;
  }
  const why = keyBlock(to, from); if (why) { say(why); return; }
  // ducking means a Compressor: a Gate or Filter is keyed from its own window
  const free = dst.fx.find(f => f.type === 'comp' && f.key == null);
  if (free) {
    commit([A.fxOp({ ...(plain(free) as Fx), key: from }, to)], t('c.sc.undo'));
    say(t('c.sc.saidOn', { from: insName(src), to: insName(dst) }));
    setU({ mixSel: to, fxSel: free.id });
    A.openPlug(free.id);
    return;
  }
  const f = newFx(S.catalog, allocId(), 'comp', S.project.bpm);
  f.params = { ...f.params, ...DUCK_PARAMS };
  f.preset = null;
  f.key = from;
  commit([{ op: 'putFx', insertId: to, fx: f }], t('c.sc.undo'));
  say(t('c.sc.saidAdded', { from: insName(src), to: insName(dst) }));
  setU({ mixSel: to, fxSel: f.id });
  A.openPlug(f.id);
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
