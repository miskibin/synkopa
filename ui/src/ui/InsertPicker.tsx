// The MIXER picker in the header of the sample and channel windows (FL's FX-track box): a list of
// the mixer inserts, and the mouse wheel over it steps through them (down = next), one undo
// step per burst of wheel ticks.
import { onCleanup } from '../lib/reactive';
import type { Id, Op } from '../api/types';
import { t } from '../lib/i18n';
import { routeToFreeInsert } from '../lib/mixer';
import { commit, wheelEdit } from '../state/edit';
import { allocId, insName, rawOf, S } from '../state/store';
import { showInsert } from '../state/mixView';
import { openMenu, say, setU, type MenuItem } from '../state/ui';

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
 * else a new one, named and coloured after the channel or sample. One undo step; the mixer opens
 * (when hidden) on that insert, selected and scrolled into view, so its effects can go on next.
 */
export function pickFreeInsert(p: InsertChoice) {
  if (!p.free) return;
  const plan = routeToFreeInsert(rawOf(S.project), p.free.name, p.free.color, allocId, p.ops);
  commit(plan.ops, p.label);
  showInsert(plan.insert.id);
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

/** The box's text: the insert's number in the mixer, M for Master, — for the null choice (FL's FX-track box). */
export function insertBoxLabel(p: InsertChoice): string {
  if (p.value == null) return '—';
  const i = S.project.inserts.findIndex(m => m.id === p.value);
  return i < 0 ? '?' : i === 0 ? 'M' : String(i);
}

/** The list a click on an insert box opens: the null choice, a free insert of its own, then every insert. */
export function insertChoiceMenu(p: InsertChoice, title: string, x: number, y: number) {
  const items: MenuItem[] = [{ t: 'title', label: title }];
  if (p.nullLabel != null) items.push({ t: 'item', label: p.nullLabel, kbd: p.value == null ? '✓' : '', act: () => pickInsert(p, null) });
  if (p.free) items.push({ t: 'item', label: t('d.smp.freeOpt'), kbd: t('d.smp.dblClick'), act: () => pickFreeInsert(p) });
  items.push({ t: 'sep' });
  S.project.inserts.forEach((m, i) => { if (i > 0 || p.master) items.push({ t: 'item', label: i === 0 ? 'Master' : i + ' · ' + m.name, kbd: p.value === m.id ? '✓' : '', act: () => pickInsert(p, m.id) }); });
  openMenu(x, y, items, Math.min(window.innerHeight - 20, 60 + items.length * 26));
}

/**
 * Click / double-click of an insert box, shared by the Channel Rack and the editor headers. One
 * click opens `menu` (after the double-click window), a double-click routes to a free insert of
 * its own when it has none (FL), else shows that insert in the mixer.
 */
export function insertBoxClick(p: () => InsertChoice, menu: (r: DOMRect) => void) {
  let timer = 0;
  onCleanup(() => clearTimeout(timer));
  return (e: MouseEvent) => {
    if (timer) {
      clearTimeout(timer); timer = 0;
      const c = p();
      if (c.value == null && c.free) pickFreeInsert(c); else showInsert(c.value);
      return;
    }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    timer = window.setTimeout(() => { timer = 0; menu(r); }, 250);
  };
}

/** The MIXER box in the sample and channel window headers: the insert's number in the Channel Rack's box, its name beside it. */
export function InsertPicker(props: InsertChoice & { title: string }) {
  const name = () => { const m = props.value == null ? undefined : S.project.inserts.find(x => x.id === props.value); return m ? (m.id === 0 ? 'Master' : m.name) : props.nullLabel ?? ''; };
  const click = insertBoxClick(() => props, r => insertChoiceMenu(props, props.label, r.left, r.bottom + 2));
  return (
    <button class="chip inspick" title={props.title + '\n' + t('d.smp.mixer.boxTip')} aria-haspopup="menu" onClick={click} onWheel={e => wheelInsert(props, e)}>
      {t('a.mn.mixer')}
      <span class={['cins', { auto: props.value == null }]}>{insertBoxLabel(props)}</span>
      <span class="insnm ell">{name()}</span>
    </button>
  );
}
