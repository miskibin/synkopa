import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { syncEngine } from '../test/engine';

// Pattern windows that follow a pattern switch, sampler channels made from project audio, and the
// second Stop, against the mock backend (which behaves like the engine).
vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
  vi.stubGlobal('document', { querySelector: () => null, activeElement: null });
  vi.stubGlobal('requestAnimationFrame', (f: () => void) => setTimeout(f, 0));
});

let mock: MockBackend;
let P: typeof import('./pattern');
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
  P = await import('./pattern');
  A = await import('./actions');
  store = await import('./store');
  ui = await import('./ui');
  await sync();
  ui.setU({ rackWin: null, rollWin: null, mixView: false });
});

describe('switching patterns shows them', () => {
  it('opens the closed Channel Rack on the pattern switched to', () => {
    expect(store.S.project.patterns.length).toBeGreaterThan(1);
    P.stepPattern(1);
    expect(ui.U.rackWin).not.toBeNull();
    expect(ui.U.pat).toBe(store.S.project.patterns[1].id);
  });
  it('numpad pick and F4 open it too', () => {
    P.pickPatternNum(2);
    expect(ui.U.rackWin).not.toBeNull();
    P.closeRack();
    P.nextEmptyPattern();
    expect(ui.U.rackWin).not.toBeNull();
  });
  it('leaves an open Piano roll alone: it already shows the current pattern', () => {
    ui.setU('rollWin', { x: 0, y: 0, w: 800, h: 400, z: 2 });
    P.stepPattern(1);
    expect(ui.U.rackWin).toBeNull();
  });
  it('does not pull the Mix view back to the arrange layout', () => {
    ui.setU('mixView', true);
    P.stepPattern(1);
    expect(ui.U.rackWin).toBeNull();
    expect(ui.U.mixView).toBe(true);
  });
  it('a pick that does not exist opens nothing', () => {
    P.pickPatternNum(9);
    expect(ui.U.rackWin).toBeNull();
  });
});

describe('sampler channels from project audio', () => {
  it('adds a sampler of a source at a rack position', async () => {
    const src = store.S.project.sources[0];
    const before = P.channels().length;
    P.addSampler(src.id, undefined, 0);
    await sync();
    expect(P.channels().length).toBe(before + 1);
    const c = P.channels()[0];
    expect(c.kind).toBe('sample');
    expect(c.kind === 'sample' && c.source).toBe(src.id);
  });
  it('replaces the sample of a sampler channel, keeping its settings', async () => {
    const [a, b] = store.S.project.sources;
    P.addSampler(a.id);
    await sync();
    const ch = P.channels()[P.channels().length - 1];
    const was = store.plain(ch); // `ch` is the live store item: it changes with the replace
    if (was.kind !== 'sample') throw new Error('kind');
    P.replaceWithSource(ch, b.id);
    await sync();
    const now = P.chanById(ch.id)!;
    expect(now.kind === 'sample' && now.source).toBe(b.id);
    expect(now.name).toBe(b.name);
    if (now.kind !== 'sample') throw new Error('kind');
    expect(now.sampler).toEqual(was.sampler);
  });
  it('makes another instrument a sampler of the source', async () => {
    const src = store.S.project.sources[0];
    P.addKeys(); // an instrument that is not a sampler
    await sync();
    const other = store.plain(P.channels().find(c => c.kind === 'keys')!);
    P.replaceWithSource(other, src.id);
    await sync();
    const now = P.chanById(other.id)!;
    expect(now.kind).toBe('sample');
    expect(now.insert).toBe(other.insert);
  });
});

describe('the second Stop', () => {
  it('cuts the tails only when nothing was playing', async () => {
    const silence = vi.spyOn(mock, 'transportSilence');
    await A.stop();
    expect(silence).toHaveBeenCalledTimes(1);
    await mock.transportPlay(0);
    ui.setU('playing', true);
    await A.stop();
    expect(silence).toHaveBeenCalledTimes(1);
  });
  it('ends a running preview', async () => {
    const src = store.S.project.sources[0];
    ui.setU('previewPath', 'src:' + src.id);
    await mock.previewSource(src.id);
    await A.silence();
    expect(ui.U.previewPath).toBeNull();
  });
});

describe('double-click shows the insert in the mixer', () => {
  it('selects the insert and expands a collapsed mixer', async () => {
    const mv = await import('./mixView');
    const m = store.S.project.inserts[store.S.project.inserts.length - 1];
    ui.setU({ mixMin: true, mixSel: 0 });
    mv.showInsert(m.id);
    expect(ui.U.mixSel).toBe(m.id);
    expect(ui.U.fxSel).toBe(m.fx[0]?.id ?? null);
    expect(ui.U.mixMin).toBe(false);
    const n = mv.revealed()!.n;
    mv.showInsert(m.id);
    expect(mv.revealed()!.n).toBe(n + 1);
  });
  it('ignores an insert that is gone', async () => {
    const mv = await import('./mixView');
    ui.setU({ mixMin: true, mixSel: 0 });
    mv.showInsert(987654);
    mv.showInsert(null);
    expect(ui.U.mixSel).toBe(0);
    expect(ui.U.mixMin).toBe(true);
  });
});

describe('Channel Rack step spacing', () => {
  it('widens the gaps between step buttons, not the buttons, and sizes the rack to match', async () => {
    const prefs = await import('./prefs');
    const at = (s: 'normal' | 'roomy' | 'wide') => { prefs.setPref('rackSpacing', s); return { cw: P.stepW(), pad: P.stepPad(), w: P.rackWidthFor(16, 0) }; };
    const n = at('normal'), r = at('roomy'), w = at('wide');
    for (const g of [n, r, w]) expect(g.cw - 2 * g.pad).toBe(16);
    expect([n.pad, r.pad, w.pad]).toEqual([2, 4, 6]);
    expect(n.w).toBeLessThan(r.w);
    expect(r.w).toBeLessThan(w.w);
    prefs.setPref('rackSpacing', 'normal');
    expect(P.stepW()).toBe(n.cw);
  });
});
