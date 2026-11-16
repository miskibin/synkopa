// Mixer routing (FL Studio's): an insert sends its post-fader output to other inserts at a
// level, and to the master unless `master` is false. Mirrors crates/engine/src/routing.rs (the
// engine validates every change; this side greys out what it would refuse and shows solo).
import type { Id, Insert, InsertSend, Project } from '../api/types';

/** The highest send level (linear, +6 dB). */
export const SEND_MAX = 2;

export const sendsOf = (m: Insert): InsertSend[] => m.sends ?? [];
export const toMaster = (m: Insert) => m.master !== false;

/** Whether `a` is `b` or reaches it through sends. */
export function reaches(p: Pick<Project, 'inserts'>, a: Id, b: Id): boolean {
  const stack = [a], seen = new Set<Id>();
  while (stack.length) {
    const x = stack.pop()!;
    if (x === b) return true;
    if (seen.has(x)) continue;
    seen.add(x);
    const m = p.inserts.find(i => i.id === x);
    if (m) for (const s of sendsOf(m)) stack.push(s.to);
  }
  return false;
}

/** Whether insert `from` can send to `to`: another non-master insert that does not already reach `from`. */
export function canSend(p: Pick<Project, 'inserts'>, from: Id, to: Id): boolean {
  return from !== 0 && to !== 0 && from !== to && p.inserts.some(m => m.id === to) && !reaches(p, to, from);
}

/** Why the engine would refuse `m`'s routing, or null (crates/engine/src/routing.rs `validate`). */
export function routingError(p: Pick<Project, 'inserts'>, m: Insert): string | null {
  const sends = sendsOf(m);
  if (m.id === 0) return sends.length || !toMaster(m) ? 'the master sends nowhere: it is the output' : null;
  for (let i = 0; i < sends.length; i++) {
    const s = sends[i];
    if (s.to === 0) return 'a send to the master: set `master` instead';
    if (s.to === m.id) return `insert ${m.id} cannot send to itself`;
    if (!p.inserts.some(x => x.id === s.to)) return `insert ${m.id} sends to insert ${s.to}, which does not exist`;
    if (sends.slice(0, i).some(o => o.to === s.to)) return `insert ${m.id} sends to insert ${s.to} twice`;
    if (!Number.isFinite(s.gain) || s.gain < 0 || s.gain > SEND_MAX) return `send level ${s.gain} out of range 0..${SEND_MAX}`;
    if (reaches(p, s.to, m.id)) return `insert ${m.id} → ${s.to} would make a loop: ${s.to} already sends into ${m.id}`;
  }
  return null;
}

/**
 * Which inserts are heard, by id: muted ones are not; with any non-master insert soloed, only
 * the soloed ones, the inserts feeding them and the inserts they feed (as in FL Studio).
 */
export function audibleInserts(p: Pick<Project, 'inserts'>): Set<Id> {
  const soloed = p.inserts.filter(m => m.id !== 0 && m.solo).map(m => m.id);
  const out = new Set<Id>();
  for (const m of p.inserts) {
    if (m.muted) continue;
    if (m.id === 0 || !soloed.length || soloed.some(s => reaches(p, m.id, s) || reaches(p, s, m.id))) out.add(m.id);
  }
  return out;
}

/** After insert `id` is removed: sends to it go; an insert that fed only it goes to the master. */
export function forgetInsert(p: Pick<Project, 'inserts'>, id: Id) {
  for (const m of p.inserts) {
    const had = sendsOf(m).length > 0;
    if (!had) continue;
    m.sends = sendsOf(m).filter(s => s.to !== id);
    if (!m.sends.length) { delete m.sends; if (m.master === false) delete m.master; }
  }
}

/** `m` with its send to `to` set to `gain` (added when missing). */
export function withSend(m: Insert, to: Id, gain: number): Insert {
  const sends = sendsOf(m);
  const next = sends.some(s => s.to === to) ? sends.map(s => (s.to === to ? { to, gain } : s)) : [...sends, { to, gain }];
  return { ...m, sends: next };
}

/** `m` without its send to `to`; left with no output at all, it goes to the master again. */
export function withoutSend(m: Insert, to: Id): Insert {
  const sends = sendsOf(m).filter(s => s.to !== to);
  return { ...m, sends, master: sends.length ? toMaster(m) : true };
}
