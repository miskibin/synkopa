import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { syncEngine } from '../test/engine';

// A playlist track routed to "a free insert named after it" (as the Channel Rack's insert picker does):
// the first unused insert, or a new one, takes the track's name and colour, in one undo step.
vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
  vi.stubGlobal('document', { querySelector: () => null, activeElement: null });
  vi.stubGlobal('requestAnimationFrame', (f: () => void) => setTimeout(f, 0));
});

let mock: MockBackend;
let A: typeof import('./actions');
let store: typeof import('./store');
let edit: typeof import('./edit');
let IP: typeof import('../ui/InsertPicker');
const sync = () => syncEngine(mock, store, edit);

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  edit = await import('./edit');
  A = await import('./actions');
  store = await import('./store');
  IP = await import('../ui/InsertPicker');
  await sync();
});

const route = (i: number) => {
  const t = store.S.project.tracks[i];
  IP.pickFreeInsert({ value: t.insert, ops: id => { const o = A.trackOp(i, { insert: id ?? 0 }); return o ? [o] : []; }, label: 'Routing', wheelKey: 'x', master: true, free: { name: t.name, color: t.color } });
};

describe('track to a free insert', () => {
  it('routes to the first unused insert, named and coloured after the track, undone in one step', async () => {
    A.setTrack(0, { name: 'Lead vocal', color: '#d9678f' });
    await sync();
    const before = store.S.project.inserts.length;
    const free = (await import('../lib/mixer')).freeInsert(store.rawOf(store.S.project));
    route(0);
    await sync();
    const routed = store.S.project.tracks[0].insert;
    if (free) expect(routed).toBe(free.id);
    const ins = store.S.project.inserts.find(m => m.id === routed)!;
    expect(ins).toMatchObject({ name: 'Lead vocal', color: '#d9678f' });
    expect(store.S.project.inserts).toHaveLength(before + (free ? 0 : 1));
    await A.undo();
    await sync();
    expect(store.S.project.tracks[0].insert).not.toBe(routed);
    expect(store.S.project.inserts.some(m => m.name === 'Lead vocal')).toBe(false);
    expect(store.S.project.inserts).toHaveLength(before);
  });
  it('makes a new insert when every one is taken', async () => {
    for (const m of store.S.project.inserts.slice(1)) A.setTrack(0, {}, 'x'), edit.commit([{ op: 'putInsert', insert: { ...store.plain(m), fx: [{ id: 9000 + m.id, type: 'eq', params: {} } as never] } }], 'fx');
    await sync();
    const before = store.S.project.inserts.length;
    route(0);
    await sync();
    expect(store.S.project.inserts).toHaveLength(before + 1);
    const t = store.S.project.tracks[0];
    expect(store.S.project.inserts.find(m => m.id === t.insert)?.name).toBe(t.name);
  });
});
