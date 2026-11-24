import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { syncEngine } from '../test/engine';

// The song key the toolbar and the Piano roll show, and the Browser preview the status line shows,
// against the mock backend (which behaves like the engine).
vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
  vi.stubGlobal('document', { querySelector: () => null, activeElement: null });
  vi.stubGlobal('requestAnimationFrame', (f: () => void) => setTimeout(f, 0));
});

let mock: MockBackend;
let store: typeof import('./store');
let ui: typeof import('./ui');
let edit: typeof import('./edit');
let K: typeof import('./songKey');
let PW: typeof import('./previewWave');
const sync = () => syncEngine(mock, store, edit);

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  edit = await import('./edit');
  store = await import('./store');
  ui = await import('./ui');
  K = await import('./songKey');
  PW = await import('./previewWave');
  await sync();
}, 30_000); // the first test imports the whole app

describe('song key', () => {
  it('says no key is known instead of pretending C major was chosen', async () => {
    await mock.applyOps([K.setKeyOp(null, 1)]); await sync();
    expect(K.songKey().from).toBe('none');
  });
  it('is the chosen key once set, and its name reads like the Piano roll\'s', async () => {
    await mock.applyOps([K.setKeyOp(9, 2)]); await sync();
    expect(K.songKey()).toEqual({ root: 9, scale: 2, from: 'set' });
    expect(K.keyName(K.songKey())).toBe('A minor');
  });
  it('clears back to unknown (undoable like any edit)', async () => {
    edit.commit([K.setKeyOp(2, 1)], 'set'); await sync();
    expect(K.songKey().from).toBe('set');
    edit.commit([K.setKeyOp(null, 1)], 'clear'); await sync();
    expect(K.songKey().from).toBe('none');
  });
});

describe('the Browser preview in the status line', () => {
  it('shows the waveform of the preview that plays', async () => {
    ui.setU('previewPath', 'lib/kick.wav');
    const w = await mock.previewFile('lib/kick.wav');
    expect(w.secs).toBeGreaterThan(0);
    expect(w.peaks.length % 2).toBe(0);
    expect(w.peaks.length / 2).toBeLessThanOrEqual(1024);
    PW.setPreviewWave('lib/kick.wav', 'kick.wav', w);
    ui.setU('previewing', true);
    expect(PW.shownPreview()?.name).toBe('kick.wav');
    ui.setU('previewing', false);
    expect(PW.shownPreview()).toBeNull(); // the hint comes back when the preview ends
  });
  it('drops a late answer for a sample the user already moved past', async () => {
    ui.setU({ previewPath: 'lib/b.wav', previewing: true });
    PW.setPreviewWave('lib/a.wav', 'a.wav', await mock.previewFile('lib/a.wav'));
    expect(PW.shownPreview()).toBeNull();
  });
});
