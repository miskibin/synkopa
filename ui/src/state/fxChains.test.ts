import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import type { Fx, Insert } from '../api/types';

// FX chain presets against the mock backend, which behaves like app/src/fx_chains.rs: a library
// that outlives the project, load = one undo step, Replace / Append, the insert itself untouched.
vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
});

let mock: MockBackend;
let A: typeof import('./actions');
let store: typeof import('./store');
let ui: typeof import('./ui');
const settle = () => new Promise(r => setTimeout(r, 30));
const sync = async () => { await settle(); store.replaceState(await mock.getState()); };
const noIds = (fx: Fx[]) => fx.map(f => ({ ...store.plain(f), id: 0 }));
const ins = (id: number) => store.S.project.inserts.find(m => m.id === id)!;

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  A = await import('./actions');
  store = await import('./store');
  ui = await import('./ui');
  await sync();
});

/** The sketch's first insert with effects, and one without any. */
function pick(): { src: Insert; empty: Insert } {
  const src = store.S.project.inserts.find(m => m.id !== 0 && m.fx.length > 0)!;
  const empty = store.S.project.inserts.find(m => m.id !== 0 && m.fx.length === 0)!;
  return { src, empty };
}

describe('FX chain presets', () => {
  it('saves an insert’s effects and loads them in order onto an empty insert, leaving its settings alone, in one undo step', async () => {
    const { src, empty } = pick();
    expect(src && empty).toBeTruthy();
    await mock.applyOps([{ op: 'putInsert', insert: { ...store.plain(empty), vol: 0.3, pan: -0.5, name: 'Lead Vox', color: '#ff00aa' } }]);
    await sync();
    const before = store.plain(ins(empty.id));
    expect(await mock.fxChainSave(src.id, '  Vocal chain ')).toEqual({ name: 'Vocal chain', effects: src.fx.length });
    const { presets } = await mock.fxChains();
    expect(presets).toEqual([{ name: 'Vocal chain', effects: src.fx.map(f => ({ type: f.type, name: expect.any(String) })), plugins: 0 }]);

    const r = await mock.fxChainLoad(empty.id, 'VOCAL CHAIN');
    await sync();
    expect(r.replaced).toBe(false);
    const got = ins(empty.id);
    expect(got.fx.map(f => f.id)).toEqual(r.fxIds);
    expect(new Set([...r.fxIds, ...src.fx.map(f => f.id)]).size).toBe(r.fxIds.length + src.fx.length);
    expect(noIds(got.fx)).toEqual(noIds(ins(src.id).fx));
    expect({ ...store.plain(got), fx: [] }).toEqual({ ...before, fx: [] });

    await A.undo(); await sync();
    expect(ins(empty.id).fx).toEqual([]);
  });

  it('on an insert with effects needs a mode: Replace swaps the chain, Append adds after, each undone in one step', async () => {
    const { src, empty } = pick();
    await mock.fxChainSave(src.id, 'Chain');
    const other = store.S.project.inserts.find(m => m.id !== src.id && m.fx.length > 0) ?? src;
    const mine = noIds(ins(other.id).fx);
    await expect(mock.fxChainLoad(other.id, 'Chain')).rejects.toThrow(/replace or append/);

    await mock.fxChainLoad(other.id, 'Chain', 'append'); await sync();
    expect(noIds(ins(other.id).fx)).toEqual([...mine, ...noIds(ins(src.id).fx)]);
    await A.undo(); await sync();
    expect(noIds(ins(other.id).fx)).toEqual(mine);

    const r = await mock.fxChainLoad(other.id, 'Chain', 'replace'); await sync();
    expect(r.replaced).toBe(true);
    expect(noIds(ins(other.id).fx)).toEqual(noIds(ins(src.id).fx));
    await A.undo(); await sync();
    expect(noIds(ins(other.id).fx)).toEqual(mine);
    expect(empty).toBeTruthy();
  });

  it('keeps presets across projects, and handles names, overwrite, rename and delete', async () => {
    const { src } = pick();
    await mock.fxChainSave(src.id, 'Drum Bus');
    await expect(mock.fxChainSave(src.id, 'drum bus')).rejects.toThrow(/^EXISTS:/);
    await mock.fxChainSave(src.id, 'drum bus', true);
    for (const bad of ['', '  ', 'a/b', 'why?', 'dot.', '.x', 'x'.repeat(81)]) await expect(mock.fxChainSave(src.id, bad)).rejects.toThrow();
    await mock.fxChainSave(src.id, 'Alpha');
    await expect(mock.fxChainRename('alpha', 'DRUM BUS')).rejects.toThrow(/^EXISTS:/);
    await mock.fxChainRename('alpha', 'Zeta');
    expect((await mock.fxChains()).presets.map(p => p.name)).toEqual(['drum bus', 'Zeta']);

    await mock.projectNew(); await sync();
    const target = store.S.project.inserts.find(m => m.id !== 0)!;
    expect(target.fx).toEqual([]);
    await mock.fxChainLoad(target.id, 'zeta'); await sync();
    expect(ins(target.id).fx.length).toBe(src.fx.length);

    await mock.fxChainDelete('Zeta');
    expect((await mock.fxChains()).presets.map(p => p.name)).toEqual(['drum bus']);
    await expect(mock.fxChainLoad(target.id, 'Zeta', 'append')).rejects.toThrow(/no FX chain preset/);
  });

  it('refuses to save an insert without effects', async () => {
    const { empty } = pick();
    await expect(mock.fxChainSave(empty.id, 'Nothing')).rejects.toThrow(/no effects/);
    expect(ui.U.fxChainWin).toBeNull();
  });
});
