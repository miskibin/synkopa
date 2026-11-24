import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { syncEngine } from '../test/engine';

// Every effect type's window width is decided in PLUG_W (state/actions.ts) — the table is
// exhaustive over Fx['type'] and `null` means the generic 580 px on purpose, so a new effect
// cannot skip the decision. Gate and Filter once landed with widths only in a dead copy of
// that table, so their windows fell back to the generic 580 px.
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
let ui: typeof import('./ui');
let edit: typeof import('./edit');
const sync = () => syncEngine(mock, store, edit);

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  edit = await import('./edit');
  A = await import('./actions');
  store = await import('./store');
  ui = await import('./ui');
  store.setS('catalog', await mock.fxCatalog());
  await sync();
});

describe('effect window widths', () => {
  it('Gate and Filter open at their type widths, not the generic fallback', async () => {
    const m = store.S.project.inserts.find(x => x.id !== 0)!;
    A.addFx(m.id, 'gate');
    A.addFx(m.id, 'filter');
    await sync();
    const fx = store.S.project.inserts.find(x => x.id === m.id)!.fx;
    const gate = fx.find(f => f.type === 'gate')!;
    const filter = fx.find(f => f.type === 'filter')!;
    A.openPlug(gate.id);
    A.openPlug(filter.id);
    expect(ui.U.plugs.find(p => p.fxId === gate.id)?.w).toBe(720);
    expect(ui.U.plugs.find(p => p.fxId === filter.id)?.w).toBe(760);
  });

  it('a type without a table entry still opens, at the fallback width', async () => {
    const m = store.S.project.inserts.find(x => x.id !== 0)!;
    A.addFx(m.id, 'comp');
    await sync();
    const comp = store.S.project.inserts.find(x => x.id === m.id)!.fx.at(-1)!;
    A.openPlug(comp.id);
    expect(ui.U.plugs.find(p => p.fxId === comp.id)?.w).toBe(580);
  });
});
