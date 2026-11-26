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
  it('a committed tempo point leaves S.canUndo true', async () => {
    edit.commit([{ op: 'setTempoPoints', points: [{ beat: 8, bpm: 90, ramp: false }] }], 'Add tempo point');
    await flush();
    expect(store.S.canUndo).toBe(true);
    expect(store.S.undoLabel).toBe('Add tempo point');
  });

  it('a gesture tempo drag leaves S.canUndo true after it ends', async () => {
    const g = new edit.Gesture('Move tempo point');
    g.update([{ op: 'setTempoPoints', points: [{ beat: 8, bpm: 90, ramp: false }] }]);
    g.end();
    await flush(60);
    expect(store.S.canUndo).toBe(true);
  });

  it('undo after a tempo delete restores the point (engine and store agree)', async () => {
    edit.commit([{ op: 'setTempoPoints', points: [{ beat: 8, bpm: 90, ramp: false }] }], 'Add tempo point');
    await flush();
    edit.commit([{ op: 'setTempoPoints', points: [] }], 'Delete tempo point');
    await flush();
    expect(store.S.canUndo).toBe(true);
    await (await import('../api/backend')).be().undo().then(edit.acceptReply);
    expect(store.S.project.tempoPoints ?? []).toHaveLength(1);
  });
});
