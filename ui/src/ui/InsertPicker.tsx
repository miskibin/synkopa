// The MIXER picker in the header of the sample and channel windows (FL's FX-track box): a list of
// the mixer inserts, and the mouse wheel over it steps through them (down = next), one undo
// step per burst of wheel ticks.
import { For, Show } from 'solid-js';
import { unwrap } from 'solid-js/store';
import type { Id, Op } from '../api/types';
import { t } from '../lib/i18n';
import { routeToFreeInsert } from '../lib/mixer';
import { commit, wheelEdit } from '../state/edit';
import { allocId, insName, S } from '../state/store';
import { say, setU } from '../state/ui';

/** What an insert choice is made of: shared by the picker and the Channel Rack's mixer box. */
export interface InsertChoice {
  /** The insert chosen, or null for `nullLabel` (e.g. "like the clip's track"). */
  value: Id | null;
  /** When given, null is a choice too, listed first. */
  nullLabel?: string;
  /** The ops that route to `id`. */
  ops: (id: Id | null) => Op[];
  /** Undo label. */
  label: string;
  /** Wheel gesture key (one per window or row). */
  wheelKey: string;
  /** Master can be picked (a track can play into it; a channel's null already means "its track"). */
  master?: boolean;
  /** What a free insert of its own is named and coloured after (FL's route to a free mixer track): see `pickFreeInsert`. */
  free?: { name: string; color: string };
}

const choiceIds = (p: InsertChoice): (Id | null)[] => [...(p.nullLabel != null ? [null] : []), ...S.project.inserts.filter(m => p.master || m.id !== 0).map(m => m.id)];
const choiceName = (p: InsertChoice, id: Id | null) => { const m = id == null ? undefined : S.project.inserts.find(x => x.id === id); return m ? insName(m) : p.nullLabel ?? ''; };
const select = (id: Id | null) => { if (id != null) setU('mixSel', id); };

/** Routes to `id` (one undo step) and selects that insert in the mixer. */
export function pickInsert(p: InsertChoice, id: Id | null) {
  commit(p.ops(id), p.label);
  select(id);
  say(t('d.smp.routedTo', { name: choiceName(p, id) }));
}

/**
 * FL: route it to a mixer insert of its own: the first free one (no effects, nothing routed to it),
 * else a new one, named and coloured after the channel or sample. One undo step; the insert is
 * selected in the mixer.
 */
export function pickFreeInsert(p: InsertChoice) {
  if (!p.free) return;
  const plan = routeToFreeInsert(unwrap(S.project), p.free.name, p.free.color, allocId, p.ops);
  commit(plan.ops, p.label);
  select(plan.insert.id);
  const n = S.project.inserts.findIndex(m => m.id === plan.insert.id);
  say(t(plan.created ? 'd.smp.freeNew' : 'd.smp.freeTo', { name: p.free.name, n }));
}

/** The mouse wheel over an insert choice: down = the next insert, up = the previous (FL's FX-track box). */
export function wheelInsert(p: InsertChoice, e: WheelEvent) {
  e.preventDefault();
  e.stopPropagation();
  const d = e.deltaY || e.deltaX;
  if (!d) return;
  const l = choiceIds(p), i = l.indexOf(p.value), j = Math.min(l.length - 1, Math.max(0, i + (d > 0 ? 1 : -1)));
  if (i < 0 || j === i) return;
  wheelEdit(p.wheelKey, p.label, p.ops(l[j]));
  select(l[j]);
  say(t('d.smp.routedTo', { name: choiceName(p, l[j]) }));
}

export function InsertPicker(props: InsertChoice & { title: string }) {
  const pick = (id: Id | null) => pickInsert(props, id);
  // the select shows the old choice again: the store change re-renders it with the new one
  const wheel = (e: WheelEvent) => wheelInsert(props, e);
  return (
    <>
      <span class="tag9">MIXER</span>
      <select class="sel inspick" value={props.value == null ? '' : String(props.value)} title={props.title}
        onChange={e => { const v = e.currentTarget.value; if (v === 'free') { e.currentTarget.value = props.value == null ? '' : String(props.value); pickFreeInsert(props); } else pick(v === '' ? null : +v); }} onWheel={wheel}>
        <Show when={props.nullLabel != null}><option value="">{props.nullLabel}</option></Show>
        <Show when={props.free}><option value="free">{t('d.smp.freeOpt')}</option></Show>
        <For each={S.project.inserts.filter(m => props.master || m.id !== 0)}>{m => <option value={String(m.id)}>{insName(m)}</option>}</For>
      </select>
    </>
  );
}
