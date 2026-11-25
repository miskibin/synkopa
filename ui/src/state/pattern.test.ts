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

describe('the wheel over the pattern selector (FL)', () => {
  const ids = () => store.S.project.patterns.map(p => p.id);
  const last = () => store.S.project.patterns[store.S.project.patterns.length - 1];
  it('steps without wrapping, and past the last pattern opens a new blank one', async () => {
    const before = ids();
    P.setCurPattern(before[0], true);
    P.scrollPattern(-1);
    expect(ui.U.pat).toBe(before[0]); // no wrap to the last one
    P.setCurPattern(before[before.length - 1], true);
    if (!last().notes.length) edit.commit([P.putNotes(last(), [{ ch: P.channels()[0].id, step: 0, len: 1 }])], 'test');
    await sync();
    P.scrollPattern(1);
    await sync();
    expect(store.S.project.patterns.length).toBe(before.length + 1);
    expect(ui.U.pat).toBe(last().id);
    // already on the blank one: the wheel adds no second blank pattern
    P.scrollPattern(1);
    await sync();
    expect(store.S.project.patterns.length).toBe(before.length + 1);
  });
  it('drops the untouched blank pattern when wheeling back off it, keeps one with notes', async () => {
    const n0 = ids().length;
    P.setCurPattern(ids()[n0 - 1], true);
    if (!last().notes.length) edit.commit([P.putNotes(last(), [{ ch: P.channels()[0].id, step: 0, len: 1 }])], 'test');
    await sync();
    P.scrollPattern(1); await sync();
    P.scrollPattern(-1); await sync();
    expect(store.S.project.patterns.length).toBe(n0);
    P.scrollPattern(1); await sync();
    const blank = last();
    edit.commit([P.putNotes(blank, [{ ch: P.channels()[0].id, step: 4, len: 1 }])], 'test'); await sync();
    P.scrollPattern(-1); await sync();
    expect(store.S.project.patterns.some(p => p.id === blank.id)).toBe(true);
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

describe('Synkopa Instruments instruments', () => {
  const keysChan = () => P.channels().find(c => c.kind === 'keys') as Extract<ReturnType<typeof P.channels>[number], { kind: 'keys' }>;
  it('switches the instrument, renames a channel still named after it and drops the old articulation', async () => {
    P.addKeys();
    await sync();
    const id = keysChan().id;
    P.setKeysInstrument(id, 'strings');
    await sync();
    expect(keysChan().name).toBe('String Section');
    P.setKeysArticulation(id, 26);
    await sync();
    expect(keysChan().keys).toMatchObject({ instrument: 'strings', articulation: 26 });
    P.setKeysInstrument(id, 'harp');
    await sync();
    expect(keysChan().name).toBe('Concert Harp');
    expect(keysChan().keys.articulation).toBeUndefined();
    expect(keysChan().keys.release).toBeUndefined();
  });
  it('keeps a name the user gave and ignores an unknown instrument', async () => {
    P.addKeys();
    await sync();
    const c = keysChan();
    edit.commit([P.chanOp(store.plain(c), { name: 'Lead keys' })], 'rename');
    await sync();
    P.setKeysInstrument(c.id, 'wurlitzer');
    await sync();
    expect(keysChan().name).toBe('Lead keys');
    P.setKeysInstrument(c.id, 'kazoo');
    await sync();
    expect(keysChan().keys.instrument).toBe('wurlitzer');
  });
  it('lists the catalog with what is installed', async () => {
    const st = await mock.keysStatus();
    expect(st.instruments.map(i => i.id)).toContain('grand-piano');
    expect(st.instruments.find(i => i.id === 'grand-piano')).toMatchObject({ bundled: true, installed: true });
    expect(st.instruments.find(i => i.id === 'strings')?.articulations.map(a => a.name)).toEqual(['Sustain', 'Tremolo', 'Pizzicato', 'Spiccato']);
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

describe('inserting a lane', () => {
  it('moves the open takes with the lanes that shift down', () => {
    const s1 = store.S.project.tracks[1].id;
    const n = store.S.project.tracks.length;
    ui.setU('takesOpen', { [s1]: true });
    A.insertTrack(1);
    const ids = store.S.project.tracks.map(t => t.id);
    expect(store.S.project.tracks.length).toBe(n + 1);
    expect(ui.U.takesOpen[ids[2]]).toBe(true); // lane 1's takes followed it to slot 2
    expect(ui.U.takesOpen[ids[1]]).toBeUndefined(); // the fresh lane has none
  });
  it('appending one keeps every lane key where it is', () => {
    const lanes = store.S.project.tracks, last = lanes[lanes.length - 1].id, n = lanes.length;
    ui.setU('takesOpen', { [last]: true });
    A.addTrack();
    const ids = store.S.project.tracks.map(t => t.id);
    expect(store.S.project.tracks.length).toBe(n + 1);
    expect(ui.U.takesOpen[last]).toBe(true);
    expect(ui.U.takesOpen[ids[ids.length - 1]]).toBeUndefined(); // the appended lane has none
  });
});
