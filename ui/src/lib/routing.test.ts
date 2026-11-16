import { describe, expect, it } from 'vitest';
import type { Insert } from '../api/types';
import { applyAll } from './applyOps';
import { emptyProject } from './factory';
import { audibleInserts, canSend, forgetInsert, reaches, routingError, withoutSend, withSend } from './routing';

const ins = (id: number, sends: [number, number][] = [], master = true): Insert =>
  ({ id, name: 'I' + id, color: '#fff', vol: 0.8, pan: 0, muted: false, solo: false, fx: [], ...(sends.length ? { sends: sends.map(([to, gain]) => ({ to, gain })) } : {}), ...(master ? {} : { master: false }) });

describe('routing (mirror of the engine)', () => {
  const p = { inserts: [ins(0), ins(1), ins(2, [[1, 1]], false), ins(3), ins(4, [[2, 0.5]], false)] };

  it('follows sends transitively and refuses what would loop', () => {
    expect(reaches(p, 4, 1)).toBe(true);
    expect(reaches(p, 1, 4)).toBe(false);
    expect(canSend(p, 3, 1)).toBe(true);
    expect(canSend(p, 1, 4)).toBe(false); // 4 already reaches 1
    expect(canSend(p, 2, 2)).toBe(false);
    expect(canSend(p, 2, 0)).toBe(false);
    expect(routingError(p, withSend(p.inserts[1], 4, 1))).toMatch(/loop/);
    expect(routingError(p, withSend(p.inserts[3], 1, 3))).toMatch(/range/);
    expect(routingError(p, withSend(p.inserts[3], 1, 0.5))).toBeNull();
    expect(routingError(p, { ...p.inserts[0], master: false })).toMatch(/master/);
  });

  it('keeps a soloed bus\'s sources and a soloed source\'s way out audible', () => {
    const q = structuredClone(p);
    q.inserts[2].solo = true;
    expect([...audibleInserts(q)].sort()).toEqual([0, 1, 2, 4]);
    q.inserts[4].muted = true;
    expect(audibleInserts(q).has(4)).toBe(false);
  });

  it('removing a send that was the only output sends the insert to the master again', () => {
    const m = withoutSend(p.inserts[2], 1);
    expect(m.sends).toEqual([]);
    expect(m.master).toBe(true);
  });

  it('removing an insert drops the sends to it (applyOps, like the engine)', () => {
    const q = structuredClone(p);
    forgetInsert(q, 1);
    expect(q.inserts[2].sends).toBeUndefined();
    expect(q.inserts[2].master).toBeUndefined(); // back to the master
    expect(q.inserts[4].sends).toEqual([{ to: 2, gain: 0.5 }]);
  });

  it('applyOps refuses a looping putInsert, like the engine', () => {
    const proj = emptyProject();
    proj.inserts = [ins(0), ins(1), ins(2)];
    const once = applyAll(proj, [{ op: 'putInsert', insert: withSend(proj.inserts[1], 2, 1) }]);
    expect(once.inserts[1].sends).toEqual([{ to: 2, gain: 1 }]);
    expect(() => applyAll(once, [{ op: 'putInsert', insert: withSend(once.inserts[2], 1, 1) }])).toThrow(/loop/);
    const gone = applyAll(once, [{ op: 'removeInsert', id: 2 }]);
    expect(gone.inserts[1].sends).toBeUndefined();
  });
});
