import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { t } from '../lib/i18n';
import { syncEngine } from '../test/engine';

// Dragging effects between mixer inserts, and per-sample mixer routing, against the mock backend.
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
  await sync();
  ui.setU({ rackWin: null, rollWin: null, mixView: false });
});

describe('effects dragged between mixer inserts', () => {
  const ins = (id: number) => store.S.project.inserts.find(m => m.id === id)!;
  const withFx = () => store.S.project.inserts.find(m => m.id !== 0 && m.fx.length > 0)!;
  const other = (not: number) => store.S.project.inserts.find(m => m.id !== 0 && m.id !== not)!;
  it('moves one effect, keeping its id (automation and the running instance follow it)', async () => {
    const from = withFx(), to = other(from.id), f = from.fx[0];
    await A.moveFxTo([f.id], to.id, false);
    await sync();
    expect(ins(from.id).fx.some(x => x.id === f.id)).toBe(false);
    expect(ins(to.id).fx[ins(to.id).fx.length - 1].id).toBe(f.id);
    expect(ui.U.mixSel).toBe(to.id);
  });
  it('copies with Ctrl: the source keeps it, the target gets a new one', async () => {
    const from = withFx(), to = other(from.id), f = from.fx[0], n = ins(to.id).fx.length;
    await A.moveFxTo([f.id], to.id, true);
    await sync();
    expect(ins(from.id).fx.some(x => x.id === f.id)).toBe(true);
    const added = ins(to.id).fx[n];
    expect(added.type).toBe(f.type);
    expect(added.id).not.toBe(f.id);
  });
  it('moves a whole chain in order, as one undo step', async () => {
    const from = withFx(), to = other(from.id), ids = from.fx.map(f => f.id), n = ins(to.id).fx.length;
    await A.moveFxTo(ids, to.id, false);
    await sync();
    expect(ins(from.id).fx.length).toBe(0);
    expect(ins(to.id).fx.slice(n).map(f => f.id)).toEqual(ids);
    await A.undo();
    await sync();
    expect(ins(from.id).fx.map(f => f.id)).toEqual(ids);
  });
  it('dropping on its own insert does nothing', async () => {
    const from = withFx(), before = from.fx.map(f => f.id);
    await A.moveFxTo([before[0]], from.id, false);
    await sync();
    expect(ins(from.id).fx.map(f => f.id)).toEqual(before);
  });
});

describe('a sample plays into its own mixer insert', () => {
  it('is rejected for an insert that does not exist and cleared when its insert is removed', async () => {
    await mock.importAudio({ paths: ['C:\loops\pad.wav'], track: 0, beat: 0 });
    await sync();
    const c = store.S.project.clips.find(x => x.kind === 'audio')!;
    expect(c).toBeDefined();
    const target = store.S.project.inserts.find(m => m.id !== 0)!;
    await expect(mock.applyOps([{ op: 'putClips', clips: [{ ...store.plain(c), insert: 987654 }] }])).rejects.toThrow();
    await mock.applyOps([{ op: 'putClips', clips: [{ ...store.plain(c), insert: target.id }] }]);
    await mock.applyOps([{ op: 'removeInsert', id: target.id }]);
    await sync();
    expect(store.S.project.clips.find(x => x.id === c.id)!.insert ?? null).toBeNull();
  });
});

describe('export in place', () => {
  it('renders the selection onto a new track below it, mutes the originals, one undo step', async () => {
    const c = store.S.project.clips.find(x => x.kind === 'pat')!;
    const tracks = store.S.project.tracks.length;
    ui.setU('sel', [c.id]);
    await A.exportInPlace();
    await sync();
    expect(store.S.project.tracks.length).toBe(tracks + 1);
    const made = store.S.project.clips.find(x => x.id === ui.U.sel[0])!;
    expect(made.kind).toBe('audio');
    expect(made.track).toBe(c.track + 1);
    expect([made.start, made.len]).toEqual([c.start, c.len]);
    expect(store.S.project.clips.find(x => x.id === c.id)!.muted).toBe(true);
    await A.undo();
    await sync();
    expect(store.S.project.tracks.length).toBe(tracks);
  });
  it('a loop range exports only what plays inside it, splitting clips at its edges', async () => {
    const c = { ...store.S.project.clips.find(x => x.kind === 'pat' && !x.muted && x.len >= 4)! };
    const a = c.start + 1, b = c.start + c.len - 1;
    edit.commit([{ op: 'setLoop', loop: { a, b } }], 'Loop');
    await sync();
    ui.setU('sel', [c.id]);
    const n = store.S.project.clips.length, tracks = store.S.project.tracks.length;
    await A.exportRangeInPlace();
    await sync();
    expect(store.S.project.tracks.length).toBe(tracks + 1);
    const made = store.S.project.clips.find(x => x.id === ui.U.sel[0])!;
    expect([made.kind, made.start, made.len]).toEqual(['audio', a, b - a]);
    const mid = store.S.project.clips.find(x => x.id === c.id)!;
    expect([mid.start, mid.len, mid.offset, mid.muted]).toEqual([a, b - a, c.offset + 1, true]);
    const outer = store.S.project.clips.filter(x => x.ref === c.ref && x.kind === 'pat' && x.id !== c.id && x.track === mid.track && !x.muted
      && (Math.abs(x.start + x.len - a) < 1e-9 || Math.abs(x.start - b) < 1e-9));
    expect(outer.length).toBe(2);
    expect(store.S.project.clips.length).toBe(n + 3);
    await A.undo();
    await sync();
    expect(store.S.project.clips.length).toBe(n);
  });
  it('Ctrl+Alt+E with nothing selected exports the loop range', async () => {
    const c = { ...store.S.project.clips.find(x => x.kind === 'pat' && !x.muted)! };
    edit.commit([{ op: 'setLoop', loop: { a: c.start, b: c.start + c.len } }], 'Loop');
    await sync();
    ui.setU('sel', []);
    const spy = vi.spyOn(mock, 'exportInPlace');
    await A.exportInPlace();
    expect(spy).toHaveBeenCalledWith([], [c.start, c.start + c.len]);
  });
  it('says so when nothing is selected', async () => {
    ui.setU('sel', []);
    const spy = vi.spyOn(mock, 'exportInPlace');
    await A.exportInPlace();
    expect(spy).not.toHaveBeenCalled();
    expect(ui.U.readout).toBe(t('d.act.inPlaceNone'));
  });
});

describe('working in the playlist lights SONG', () => {
  it('a press on the grid leaves PAT mode, but not under a playing pattern loop', async () => {
    const pat = await import('./pattern');
    pat.setPatMode(true);
    A.songHere();
    expect(pat.patMode()).toBe(false);
    pat.setPatMode(true);
    ui.setU('patPlaying', true);
    A.songHere();
    expect(pat.patMode()).toBe(true); // the lit mode is what plays
    ui.setU('patPlaying', false);
  });
  it('the pattern loop meters on the master, not on the insert of its clip\'s track', async () => {
    const p = store.S.project, clip = p.clips.find(c => c.kind === 'pat')!;
    const trackIns = p.tracks[clip.track].insert;
    const pt = p.patterns.find(x => x.id === clip.ref)!;
    expect(trackIns).not.toBe(0);
    expect(pt.notes.every(n => p.channels?.find(c => c.id === n.ch)?.insert == null)).toBe(true);
    const { TelemetryDecoder } = await import('../api/telemetry');
    const dec = new TelemetryDecoder();
    let frames = 0;
    await mock.subscribeTelemetry(b => { if (dec.decode(b)) frames++; });
    await mock.patternPlay(pt.id);
    await vi.waitFor(() => expect(dec.frame.ins.get(0)?.l ?? 0).toBeGreaterThan(0));
    expect(frames).toBeGreaterThan(0);
    expect(dec.frame.ins.get(trackIns)?.l ?? 0).toBe(0);
    await mock.patternPlay(null);
  });
});
