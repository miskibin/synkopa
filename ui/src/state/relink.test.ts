import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';

// Missing audio against the mock backend, which relinks like engine.rs (`relink_sources`,
// `relink_search`): state.missing, one undo step, the dialog offered after an open.
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
const KICK = 'C:\\Samples\\Drums\\kick.wav';
const settle = () => new Promise(r => setTimeout(r, 30));
const sync = async () => { await settle(); store.replaceState(await mock.getState()); };

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  A = await import('./actions');
  store = await import('./store');
  ui = await import('./ui');
  await mock.importAudio({ paths: [KICK], track: 0, beat: 0 });
  await sync();
});

const kickId = () => store.S.project.sources.find(s => s.path === KICK || s.name === 'kick')!.id;

describe('missing audio', () => {
  it('a moved file is reported missing and the dialog opens after the project opens', async () => {
    mock.simulateMovedFiles({ [KICK]: ['D:\\Backup\\Drums\\kick.wav'] });
    await A.projectOpen('C:\\Music\\Song.wave');
    // the open replaced the project: import the kick again into it, as the saved song would have it
    await mock.importAudio({ paths: [KICK], track: 0, beat: 0 });
    await sync();
    A.offerRelink();
    expect(Object.keys(store.S.missing)).toEqual([String(kickId())]);
    expect(ui.U.missingOpen).toBe(true);
    await expect(mock.sourcePeaks(kickId())).rejects.toThrow();
  });

  it('a folder search relinks the unambiguous file as one undo step', async () => {
    const id = kickId();
    mock.simulateMovedFiles({ [KICK]: ['D:\\Backup\\Drums\\kick.wav', 'E:\\Other\\kick.wav'] });
    await sync();
    const before = store.S.revision;
    const r = await mock.relinkSearch('D:\\Backup');
    expect(r.relinked).toEqual([{ sourceId: id, path: 'D:\\Backup\\Drums\\kick.wav', error: null, lengthDiffers: false }]);
    await sync();
    expect(store.S.missing).toEqual({});
    expect(store.S.revision).toBeGreaterThan(before);
    await A.undo();
    await sync();
    expect(store.S.missing[id]).toContain('kick.wav');
    await A.redo();
    await sync();
    expect(store.S.missing).toEqual({});
  });

  it('several equally good files are left for the user; a bad pick changes nothing', async () => {
    const id = kickId();
    mock.simulateMovedFiles({ [KICK]: ['D:\\A\\kick.wav', 'D:\\B\\kick.wav'] });
    await sync();
    const r = await mock.relinkSearch('D:\\');
    expect(r.relinked).toEqual([]);
    expect(r.candidates[id].map(c => c.path)).toEqual(['D:\\A\\kick.wav', 'D:\\B\\kick.wav']);
    expect((await mock.relinkSearch('Z:\\')).notFound).toEqual([id]);
    await expect(mock.relinkSources([{ sourceId: id, path: KICK }])).rejects.toThrow('file not found');
    await expect(mock.relinkSources([{ sourceId: id, path: 'kick.wav' }])).rejects.toThrow();
    const [one] = await mock.relinkSources([{ sourceId: id, path: 'D:\\B\\kick.wav' }]);
    expect(one.error).toBeNull();
    await sync();
    expect(store.S.project.sources.find(s => s.id === id)!.path).toBe('D:\\B\\kick.wav');
  });

  it('a library sample is never relinked', async () => {
    const lib = store.S.project.sources.find(s => s.path.startsWith('lib:'));
    if (!lib) return;
    await expect(mock.relinkSources([{ sourceId: lib.id, path: 'D:\\x.wav' }])).rejects.toThrow('library');
  });
});
