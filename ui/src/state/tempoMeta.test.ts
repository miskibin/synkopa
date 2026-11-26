import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';

// The tempo lane's edits must leave the store's undo meta current (the toolbar's undo item and
// Ctrl+Z read `S.canUndo`). The engine's events arrive asynchronously; if one is dropped, undo
// says "nothing to undo" while the engine holds the step (an e2e journey caught exactly that).
const noop = () => {};
beforeAll(() => {
  const doc = { addEventListener: noop, removeEventListener: noop };
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout, document: doc });
  vi.stubGlobal('document', doc);
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
  vi.stubGlobal('requestAnimationFrame', (cb: () => void) => setTimeout(cb, 0));
});

let mock: MockBackend;
let store: typeof import('./store');
let edit: typeof import('./edit');

/** The mock's state event is deferred one timer tick past its reply. */
const flush = (ms = 30) => new Promise(r => setTimeout(r, ms));

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('./store');
  edit = await import('./edit');
  await mock.on('state', edit.onStateEvent); // main.tsx wires this in the app
  await mock.projectOpen();
  store.replaceState(await mock.getState());
});
afterEach(() => {
  mock.dispose();
});

describe('tempo edits keep the store meta current', () => {
  it('a committed tempo point leaves S.canUndo true, on the engine too', async () => {
    edit.commit([{ op: 'setTempoPoints', points: [{ beat: 8, bpm: 90, ramp: false }] }], 'Add tempo point');
    await edit.settled();
    await flush(); // the state event (a setTimeout tick) trails the reply the queue drained
    expect(store.S.canUndo).toBe(true);
    expect(store.S.undoLabel).toBe('Add tempo point');
    expect((await mock.getState()).undoLabel).toBe('Add tempo point');
  });

  it('a gesture tempo drag leaves S.canUndo true after it ends', async () => {
    const g = new edit.Gesture('Move tempo point');
    g.update([{ op: 'setTempoPoints', points: [{ beat: 8, bpm: 90, ramp: false }] }]);
    g.end();
    await edit.settled();
    await flush(); // the state event trails the reply
    expect(store.S.canUndo).toBe(true);
    expect(store.S.undoLabel).toBe('Move tempo point');
    expect((await mock.getState()).undoLabel).toBe('Move tempo point');
  });

  it('a commit above the tempo-point cap is refused, the project untouched', async () => {
    const many = Array.from({ length: 4097 }, (_, i) => ({ beat: i % 2048, bpm: 100 + (i % 10), ramp: false }));
    await expect((async () => {
      const be = (await import('../api/backend')).be();
      await be.applyOps([{ op: 'setTempoPoints', points: many }], { label: 'Flood' });
    })()).rejects.toThrow(/at most/);
    expect(store.S.project.tempoPoints ?? []).toHaveLength(0);
  });

  it('undo after a tempo delete restores the point (engine and store agree)', async () => {
    edit.commit([{ op: 'setTempoPoints', points: [{ beat: 8, bpm: 90, ramp: false }] }], 'Add tempo point');
    await edit.settled();
    await flush();
    edit.commit([{ op: 'setTempoPoints', points: [] }], 'Delete tempo point');
    await edit.settled();
    await flush();
    expect(store.S.canUndo).toBe(true);
    await (await import('../api/backend')).be().undo().then(edit.acceptReply);
    expect(store.S.project.tempoPoints ?? []).toHaveLength(1);
    expect((await mock.getState()).project.tempoPoints ?? []).toHaveLength(1);
  });

  it('undoing the first point leaves NO map (the engine omits an empty one)', async () => {
    edit.commit([{ op: 'setTempoPoints', points: [{ beat: 4, bpm: 140, ramp: false }] }], 'Add tempo point');
    await edit.settled();
    await flush();
    await (await import('../api/backend')).be().undo().then(edit.acceptReply);
    expect(store.S.project.tempoPoints).toBeUndefined();
    expect((await mock.getState()).project.tempoPoints).toBeUndefined();
    expect(store.S.canUndo).toBe(false);
  });
});
