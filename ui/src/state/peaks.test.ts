import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Clip } from '../api/types';
import type { MockBackend } from '../api/mock';

// A waveform request that fails once (engine busy, file briefly locked) must not leave the clip
// without a waveform until the project is reopened; one that keeps failing must not hit the
// backend on every redraw either.
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout: (f: () => void, ms?: number) => setTimeout(f, ms) });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
});

let mock: MockBackend;
let P: typeof import('./peaks');
const PK = new Float32Array([-0.5, 0.5, -0.25, 0.25]);
const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };

beforeEach(async () => {
  vi.resetModules();
  vi.useFakeTimers();
  vi.spyOn(console, 'warn').mockImplementation(() => {});
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  P = await import('./peaks');
});
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

describe('waveform peaks after a failed request', () => {
  it('a source that fails once is asked again after a short pause and then drawn', async () => {
    const spy = vi.spyOn(mock, 'sourcePeaks').mockRejectedValueOnce(new Error('busy')).mockResolvedValue(PK);
    expect(P.getPeaks(7)).toBeNull();
    await flush();
    expect(spy).toHaveBeenCalledTimes(1);
    // redraws during the pause do not ask again
    for (let i = 0; i < 50; i++) expect(P.getPeaks(7)).toBeNull();
    expect(spy).toHaveBeenCalledTimes(1);
    // the pause ending wakes the canvases, whose next redraw retries
    const v = P.peaksVersion();
    await vi.advanceTimersByTimeAsync(P.retryDelay(1));
    expect(P.peaksVersion()).toBeGreaterThan(v);
    expect(P.getPeaks(7)).toBeNull();
    await flush();
    expect(spy).toHaveBeenCalledTimes(2);
    expect(P.getPeaks(7)).toBe(PK);
  });

  it('a source that stays broken is asked a bounded number of times, and again once relinked', async () => {
    const spy = vi.spyOn(mock, 'sourcePeaks').mockRejectedValue(new Error('unreadable'));
    for (let t = 0; t < 400; t++) {
      P.getPeaks(9);
      await vi.advanceTimersByTimeAsync(500);
    }
    expect(spy).toHaveBeenCalledTimes(P.RETRY_ATTEMPTS);
    // relinking the source starts over
    spy.mockResolvedValue(PK);
    P.dropSourcePeaks(9);
    P.getPeaks(9);
    await flush();
    expect(P.getPeaks(9)).toBe(PK);
  });

  it('a tuned clip whose render fails once is rendered again, not shown as current', async () => {
    const clip = { id: 3, ref: 7, norm: false, rev: false, tune: { notes: [{ shift: 1 }] } } as unknown as Clip;
    vi.spyOn(mock, 'sourcePeaks').mockResolvedValue(new Float32Array(4));
    const spy = vi.spyOn(mock, 'clipPeaks').mockRejectedValueOnce(new Error('render failed')).mockResolvedValue(PK);
    P.getClipPeaks(clip);
    await vi.advanceTimersByTimeAsync(100);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(P.clipPeaksPending(clip)).toBe(true);
    expect(P.getClipPeaks(clip)).not.toBe(PK);
    await vi.advanceTimersByTimeAsync(P.retryDelay(1) + 10);
    expect(spy).toHaveBeenCalledTimes(2);
    expect(P.getClipPeaks(clip)).toBe(PK);
    expect(P.clipPeaksPending(clip)).toBe(false);
  });

  it('a tuned clip whose render keeps failing gives up, falls back to the source and stops looking busy', async () => {
    const clip = { id: 4, ref: 8, norm: false, rev: false, tune: { notes: [{ shift: 2 }] } } as unknown as Clip;
    const src = new Float32Array([-1, 1]);
    vi.spyOn(mock, 'sourcePeaks').mockResolvedValue(src);
    const spy = vi.spyOn(mock, 'clipPeaks').mockRejectedValue(new Error('render failed'));
    P.getClipPeaks(clip);
    await vi.advanceTimersByTimeAsync(120_000);
    expect(spy).toHaveBeenCalledTimes(P.RETRY_ATTEMPTS);
    expect(P.clipPeaksPending(clip)).toBe(false);
    expect(P.getClipPeaks(clip)).toBe(src);
    await vi.advanceTimersByTimeAsync(120_000);
    P.getClipPeaks(clip);
    expect(spy).toHaveBeenCalledTimes(P.RETRY_ATTEMPTS);
  });
});
