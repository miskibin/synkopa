import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import type { MediaReload } from '../api/types';

// Hot reload against the mock backend, which reports a changed file like app/src/media_watch.rs: the source's
// waveform is fetched again (`audioGen`), the project stays saved, and the status line says what happened.
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
});

let mock: MockBackend;
let store: typeof import('./store');
let peaks: typeof import('./peaks');
let mw: typeof import('./mediaWatch');
const PAD = 'C:\\Bounces\\pad.wav';
const settle = (ms = 30) => new Promise(r => setTimeout(r, ms));
const sync = async () => { await settle(); store.replaceState(await mock.getState()); };

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('./store');
  peaks = await import('./peaks');
  mw = await import('./mediaWatch');
  await mock.importAudio({ paths: [PAD], track: 0, beat: 0 });
  await sync();
});

const padId = () => store.S.project.sources.find(s => s.path === PAD)!.id;
const ev = (e: Partial<MediaReload>): MediaReload => ({
  kind: 'reloaded', path: PAD, name: 'pad', sourceIds: [1], seconds: 2, lengthDiffers: false, wasMissing: false, error: null, ...e,
});

describe('hot reload of changed audio files', () => {
  it('a reloaded file gets its waveform fetched again, without an unsaved change or undo step', async () => {
    const id = padId();
    expect(peaks.getPeaks(id)).toBeNull(); // loading
    await settle();
    const before = peaks.getPeaks(id)!;
    expect(before).toBeInstanceOf(Float32Array);
    const { canUndo, dirty } = store.S;
    mock.simulateChangedFile(PAD);
    await sync();
    expect(store.S.dirty).toBe(dirty);
    expect(store.S.canUndo).toBe(canUndo);
    expect(peaks.getPeaks(id)).toBeNull(); // the old waveform was dropped
    await settle();
    const after = peaks.getPeaks(id)!;
    expect(after).not.toEqual(before);
  });

  it('an unrelated state keeps the cached waveform', async () => {
    const id = padId();
    peaks.getPeaks(id); await settle();
    const pk = peaks.getPeaks(id);
    await mock.applyOps([{ op: 'setBpm', bpm: 99 }]);
    await sync();
    expect(peaks.getPeaks(id)).toBe(pk);
  });

  it('the status line tells what happened, several files at once as one notice', async () => {
    const said: string[] = [];
    await mw.watchMedia(cb => mock.on('media-reload', cb), m => said.push(m), 20);
    mock.simulateChangedFile(PAD, { seconds: 7.25 });
    await settle(60);
    expect(said).toEqual(['Reloaded “pad”: the file changed on disk and is 7.25 s long now (clips keep their length)']);
    await sync();
    expect(store.S.project.sources.find(s => s.id === padId())!.dur).toBe(7.25);

    await mock.importAudio({ paths: ['C:\\Bounces\\bass.wav'], track: 1, beat: 0 });
    mock.simulateChangedFile(PAD);
    mock.simulateChangedFile('C:\\Bounces\\bass.wav');
    await settle(60);
    expect(said[1]).toBe('Reloaded 2 files that changed on disk');

    mock.simulateChangedFile(PAD, { fail: 'unexpected end of file' });
    await settle(60);
    expect(said[2]).toBe('“pad” changed on disk but cannot be read (unexpected end of file); the previous audio still plays');
    mock.simulateChangedFile(PAD, { gone: true });
    await settle(60);
    expect(said[3]).toContain('deleted or moved');
    mock.simulateChangedFile('C:\\not\\in\\the\\project.wav');
    await settle(60);
    expect(said).toHaveLength(4);
  });

  it('a missing file that comes back plays again', async () => {
    mock.simulateMovedFiles({ [PAD]: [] });
    await sync();
    expect(Object.keys(store.S.missing)).toEqual([String(padId())]);
    const got: MediaReload[] = [];
    await mock.on('media-reload', e => got.push(e));
    mock.simulateChangedFile(PAD);
    await sync();
    expect(store.S.missing).toEqual({});
    expect(got[0].wasMissing).toBe(true);
    expect(mw.mediaNotice(got)).toBe('“pad” is back: its file was found and plays again');
  });

  it('a failure outranks reloads that worked in the same notice', () => {
    expect(mw.mediaNotice([])).toBeNull();
    expect(mw.mediaNotice([ev({}), ev({ kind: 'gone', name: 'kick', path: 'k' }), ev({ kind: 'failed', name: 'hat', path: 'h', error: 'locked' })]))
      .toBe('“hat” changed on disk but cannot be read (locked); the previous audio still plays (+1 more)');
    expect(mw.mediaNotice([ev({}), ev({})])).toBe('Reloaded “pad”: the file changed on disk');
    expect(mw.mediaNotice([ev({ lengthDiffers: true, seconds: 125.04 })])).toContain('125 s long');
  });
});
