// Pure helpers of the mixer: strip labels, FL-style channel routing (Ctrl+L / Shift+Ctrl+L),
// the Channel Rack's multi-selection and the Mix view's snapshot of the arrange layout.
// Tested in mixer.test.ts.
import { VOL_UNITY, type Id, type Insert, type Op, type Project } from '../api/types';
import type { UiState } from '../state/ui';

/** A name the app gave an insert ("Insert 7"): the strip shows only its number then. */
export const isDefaultInsertName = (name: string) => /^\s*Insert\s+\d+\s*$/i.test(name);

/**
 * What a strip's name row shows: `num` (the insert's place, "" for the Master) and `name` (the
 * user's name, "" for a default one, so the number says it all). `full` is the whole name for the tooltip.
 */
export function stripLabel(name: string, idx: number): { num: string; name: string; full: string } {
  if (idx === 0) return { num: '', name: 'Master', full: 'Master' };
  const def = isDefaultInsertName(name) || !name.trim();
  return { num: String(idx), name: def ? '' : name, full: idx + ' · ' + (name.trim() || 'Insert ' + idx) };
}

export interface RoutePlan {
  /** channel id → insert id */
  assign: [Id, Id][];
  /** inserts to create first (in this order, appended after the last one), named after their channel */
  create: { id: Id; name: string }[];
}

/**
 * Route `chans` (in Channel Rack order) to the insert `targetId`: all of them to it, or with
 * `spread` one per insert from it on (FL's "starting from this track"), creating the inserts past
 * the last one. `inserts` is the mixer in order (Master first); `newId` gives ids for new inserts.
 * Null when there is nothing to do (no channel, no such insert, spreading from the Master).
 */
export function planRoute(chans: readonly { id: Id; name: string }[], inserts: readonly { id: Id }[], targetId: Id, spread: boolean, newId: () => Id): RoutePlan | null {
  const at = inserts.findIndex(m => m.id === targetId);
  if (!chans.length || at < 0 || (spread && at === 0)) return null;
  if (!spread) return { assign: chans.map(c => [c.id, targetId]), create: [] };
  const plan: RoutePlan = { assign: [], create: [] };
  chans.forEach((c, i) => {
    const k = at + i;
    let id: Id;
    if (k < inserts.length) id = inserts[k].id;
    else { id = newId(); plan.create.push({ id, name: c.name }); }
    plan.assign.push([c.id, id]);
  });
  return plan;
}

/** Shift+click: the channels from the anchor to `id` (inclusive, rack order). */
export function rangeSelect(order: readonly Id[], anchor: Id | null, id: Id): Id[] {
  const a = anchor == null ? -1 : order.indexOf(anchor), b = order.indexOf(id);
  if (b < 0) return [];
  if (a < 0) return [id];
  return order.slice(Math.min(a, b), Math.max(a, b) + 1);
}

/** Shift+Ctrl+click: add `id` to the selection or take it out (`cur` empty = only the anchor is selected). */
export function toggleSelect(cur: readonly Id[], anchor: Id | null, id: Id): Id[] {
  const base = cur.length ? [...cur] : anchor == null ? [] : [anchor];
  return base.includes(id) ? base.filter(x => x !== id) : [...base, id];
}

/** The parts of the layout the Mix view hides and must give back exactly as they were. */
const ARRANGE_KEYS = ['rackWin', 'rollWin', 'dock', 'paneSize', 'dockHome', 'rollMax', 'rackFloat', 'rollFloat',
  'plugsHidden', 'mixMin', 'pickOpen', 'pickW'] as const;
export type Arrange = Pick<UiState, (typeof ARRANGE_KEYS)[number]>;

/** A deep copy (the dock layout nests strips in edges). */
const copy = <T>(v: T): T => (v && typeof v === 'object' ? (JSON.parse(JSON.stringify(v)) as T) : v);
/** A detached copy of the arrange layout (window places are copied, not shared with the store). */
export function takeArrange(u: Arrange): Arrange {
  const a = {} as Record<string, unknown>;
  for (const k of ARRANGE_KEYS) a[k] = copy(u[k]);
  return a as unknown as Arrange;
}
/** The patch that puts a taken layout back (fresh copies, so the snapshot stays reusable). */
export function restoreArrange(a: Arrange): Partial<UiState> {
  return takeArrange(a);
}

/**
 * The insert FL's "route to a free mixer track" picks: the first one (in mixer order, never the
 * Master) with no effects that nothing plays into: no channel, no playlist track, no audio clip
 * routed to it and no insert sending to it. Null when every insert is in use.
 */
export function freeInsert(p: Project): Insert | null {
  const used = new Set<Id>();
  for (const c of p.channels ?? []) if (c.insert != null) used.add(c.insert);
  for (const t of p.tracks) used.add(t.insert);
  for (const c of p.clips) if (c.insert != null) used.add(c.insert);
  for (const m of p.inserts) for (const s of m.sends ?? []) used.add(s.to);
  return p.inserts.find(m => m.id !== 0 && !m.fx.length && !used.has(m.id)) ?? null;
}

/**
 * Give `name` (a channel or a sample) a mixer insert of its own: the first free one, else a new one
 * after the last, named and coloured after it (FL). `route` makes the ops that send it there.
 * Returns the insert and every op, to commit as one undo step.
 */
export function routeToFreeInsert(p: Project, name: string, color: string, newId: () => Id, route: (id: Id) => Op[]): { insert: Insert; ops: Op[]; created: boolean } {
  const free = freeInsert(p);
  const insert: Insert = free
    ? { ...free, name, color }
    : { id: newId(), name, color, vol: VOL_UNITY, pan: 0, muted: false, solo: false, fx: [] };
  return { insert, ops: [{ op: 'putInsert', insert }, ...route(insert.id)], created: !free };
}
