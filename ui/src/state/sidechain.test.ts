import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { syncEngine } from '../test/engine';

// Sidechain compression from the mixer strip ("Sidechain to ▸") and the Compressor window, against
// the mock backend (which validates keys exactly like the engine: see the ops fixture).
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
let SC: typeof import('./sidechain');
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
  SC = await import('./sidechain');
  store = await import('./store');
  ui = await import('./ui');
  store.setS('catalog', await mock.fxCatalog());
  await sync();
  ui.setU({ rackWin: null, rollWin: null, mixView: false });
});

const ins = (id: number) => store.S.project.inserts.find(m => m.id === id)!;
/** Two non-master inserts: the second has no Compressor (the effects of the demo are cleared). */
async function pair() {
  const [a, b] = store.S.project.inserts.filter(m => m.id !== 0);
  const clear = [...a.fx, ...b.fx].map(f => ({ op: 'removeFx' as const, fxId: f.id }));
  if (clear.length) edit.commit(clear, 'clear');
  await sync();
  return { kick: a.id, bass: b.id };
}

describe('Sidechain to ▸', () => {
  it('adds a ducking Compressor listening to the source, opens it, and is one undo step', async () => {
    const { kick, bass } = await pair();
    await SC.sidechainTo(kick, bass);
    await sync();
    const c = ins(bass).fx.find(f => f.type === 'comp')!;
    expect(c.key).toBe(kick);
    expect(c.params.thr, "the engine's ducking setup (actions.rs DUCK_PARAMS)").toBe(-24);
    expect(ui.U.mixSel).toBe(bass);
    expect(ui.U.plugs.some(w => w.fxId === c.id)).toBe(true);
    A.undo(); await sync();
    expect(ins(bass).fx.some(f => f.type === 'comp')).toBe(false);
  });

  it('keys a Compressor that is already there instead of adding a second one', async () => {
    const { kick, bass } = await pair();
    A.addFx(bass, 'comp'); await sync();
    const id = ins(bass).fx[0].id;
    await SC.sidechainTo(kick, bass); await sync();
    expect(ins(bass).fx.length).toBe(1);
    expect(ins(bass).fx[0]).toMatchObject({ id, key: kick });
  });

  it('keys a free Compressor, not a free Gate that happens to come first', async () => {
    const { kick, bass } = await pair();
    A.addFx(bass, 'gate'); await sync();
    A.addFx(bass, 'comp'); await sync();
    await SC.sidechainTo(kick, bass); await sync();
    const gate = ins(bass).fx.find(f => f.type === 'gate')!;
    const comp = ins(bass).fx.find(f => f.type === 'comp')!;
    expect(gate.key, 'ducking means a Compressor: the Gate is keyed from its own window').toBeUndefined();
    expect(comp.key).toBe(kick);
  });

  it('a second time removes the sidechain and keeps the Compressor', async () => {
    const { kick, bass } = await pair();
    await SC.sidechainTo(kick, bass); await sync();
    await SC.sidechainTo(kick, bass); await sync();
    const c = ins(bass).fx.find(f => f.type === 'comp')!;
    expect(c).toBeDefined();
    expect(c.key).toBeUndefined();
  });

  it('leaves a keyed Filter alone and adds the ducking Compressor beside it', async () => {
    // a Filter pumped by the kick, set up in its own window, is not "already sidechained"
    const { kick, bass } = await pair();
    A.addFx(bass, 'filter'); await sync();
    const filter = ins(bass).fx[0].id;
    SC.setKey(filter, kick); await sync();
    SC.sidechainTo(kick, bass); await sync();
    expect(ins(bass).fx.find(f => f.id === filter)?.key).toBe(kick);
    expect(ins(bass).fx.find(f => f.type === 'comp')?.key).toBe(kick);
    // and the cable's Remove takes every listener off it, Filter included
    SC.unkey(kick, bass); await sync();
    expect(ins(bass).fx.every(f => f.key == null)).toBe(true);
  });

  it('lets a key insert go nowhere (a ghost kick), but not one that drives nothing', async () => {
    const { kick, bass } = await pair();
    const R = await import('./routing');
    R.setToMaster(kick, false); await sync();
    expect(ins(kick).master).not.toBe(false); // sends nowhere, drives nothing: it would just fall silent
    SC.sidechainTo(kick, bass); await sync();
    R.setToMaster(kick, false); await sync();
    expect(ins(kick).master).toBe(false);
  });

  it('greys out a target that would close a loop, saying why', async () => {
    const { kick, bass } = await pair();
    await SC.sidechainTo(kick, bass); await sync();
    // the bass driving the kick back: the kick would wait for itself
    const back = SC.sidechainItems(ins(bass));
    expect(back.t).toBe('sub');
    const row = back.t === 'sub' ? back.items.find(i => i.t === 'item' && i.label.length && !i.checked && i.disabled) : undefined;
    expect(row && row.t === 'item' && row.title).toBeTruthy();
    expect(SC.keyBlock(kick, bass)).toMatch(/loop|pętl/);
    // and a send from the bass into the kick is refused for the same reason
    const { canSend } = await import('../lib/routing');
    expect(canSend(store.S.project, bass, kick)).toBe(false);
  });

  it('the mock honours ifRevision like the engine (apply_labeled\'s guard)', async () => {
    const { kick, bass } = await pair();
    const rev = (await mock.getState()).revision;
    await expect(mock.sidechain({ from: kick, to: bass, ifRevision: rev - 1 })).rejects.toThrow(/REVISION_CONFLICT/);
    expect(ins(bass).fx.some(f => f.type === 'comp')).toBe(false);
    await expect(mock.sidechain({ from: kick, to: bass, ifRevision: rev })).resolves.toMatchObject({ added: true });
  });
});

describe('the Compressor window source menu', () => {
  it('sets and clears the key; the old insert removed returns it to its own input', async () => {
    const { kick, bass } = await pair();
    A.addFx(bass, 'comp'); await sync();
    const id = ins(bass).fx[0].id;
    await SC.setKey(id, kick); await sync();
    expect(ins(bass).fx[0].key).toBe(kick);
    const menu = SC.keyMenu(id);
    expect(menu.some(i => i.t === 'item' && i.checked && i.label === store.insName(ins(kick)))).toBe(true);
    await SC.setKey(id, null); await sync();
    expect(ins(bass).fx[0].key).toBeUndefined();
    await SC.setKey(id, kick); await sync();
    A.removeInsert(kick); await sync();
    expect(ins(bass).fx[0].key).toBeUndefined();
  });

  it('a sidechain moved with its effect onto the source itself is dropped, not refused', async () => {
    const { kick, bass } = await pair();
    await SC.sidechainTo(kick, bass); await sync();
    const c = ins(bass).fx.find(f => f.type === 'comp')!;
    await A.moveFxTo([c.id], kick, false); await sync();
    const moved = ins(kick).fx.find(f => f.id === c.id)!;
    expect(moved).toBeDefined();
    expect(moved.key).toBeUndefined();
  });
});
