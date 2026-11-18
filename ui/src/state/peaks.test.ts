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
/** n MiB of peaks, tagged with the id at index 0. */
const peaksOf = (id: number, mib: number) => {
  const p = new Float32Array((mib * 1024 * 1024) / 4);
  p[0] = id;
  return p;
};

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

// ---- the byte budget (drawn-waveform working set), superseded requests, reload loops ----
// These run with real timers: the tests above own the fake-clock retry semantics.
describe('peaks cache bound', () => {
  beforeEach(async () => {
    vi.useRealTimers();
    vi.resetModules();
    const backend = await import('../api/backend');
    mock = (await backend.getBackend()) as MockBackend;
    P = await import('./peaks');
    P.reloadCooldown.ms = 0;
  });

  it('evicts the least recently used peaks over the byte budget', async () => {
    vi.spyOn(mock, 'sourcePeaks').mockImplementation(async (id: number) => peaksOf(id, 2));
    // 129 × 2 MiB = 258 MiB, over the 256 MiB budget: the first entries go
    for (let id = 0; id < 129; id++) P.getPeaks(id);
    await flush();
    expect(P.getPeaks(128)).toBeInstanceOf(Float32Array);
    expect(P.getPeaks(0)).toBeNull(); // evicted (and re-requested)
    await flush();
    expect(P.getPeaks(0)).toBeInstanceOf(Float32Array); // refetched
  });

  it('drawing a source keeps it in the cache', async () => {
    vi.spyOn(mock, 'sourcePeaks').mockImplementation(async (id: number) => peaksOf(id, 2));
    for (let id = 0; id < 127; id++) P.getPeaks(id); // 254 MiB: everything fits
    await flush();
    expect(P.getPeaks(0)).toBeInstanceOf(Float32Array); // a draw refreshes its recency
    P.getPeaks(127);
    P.getPeaks(128);
    await flush(); // two more × 2 MiB push the cache over: id 0 was just used, id 1 was not
    expect(P.getPeaks(0)).toBeInstanceOf(Float32Array);
    expect(P.getPeaks(1)).toBeNull();
    await flush();
  });

  it('a just-evicted waveform is not re-requested right away', async () => {
    const spy = vi.spyOn(mock, 'sourcePeaks').mockImplementation(async (id: number) => peaksOf(id, 2));
    P.reloadCooldown.ms = 5000;
    for (let id = 0; id < 129; id++) P.getPeaks(id);
    await flush();
    const calls = spy.mock.calls.length;
    expect(P.getPeaks(0)).toBeNull();
    expect(spy.mock.calls.length).toBe(calls); // no reload loop while the cooldown runs
    P.reloadCooldown.ms = 0;
    P.getPeaks(0);
    await flush();
    expect(P.getPeaks(0)).toBeInstanceOf(Float32Array); // after the cooldown it returns
  });

  it('a superseded request cannot clobber the newer answer', async () => {
    let reject1: (e: unknown) => void = () => {};
    vi.spyOn(mock, 'sourcePeaks')
      .mockImplementationOnce(() => new Promise((_, rej) => { reject1 = rej; }))
      .mockImplementation(async (id: number) => peaksOf(id, 2));
    P.getPeaks(5); // request 1 pending
    await flush();
    // fill the cache so request 1's 'loading' marker is evicted (placeholders take no room,
    // but they sit at the front of the recency order)
    for (let id = 1000; id < 1140; id++) P.getPeaks(id);
    await flush();
    P.getPeaks(5); // drawn again while its first request is still out: request 2
    await flush();
    expect(P.getPeaks(5)).toBeInstanceOf(Float32Array);
    reject1(new Error('the device went away'));
    await flush();
    expect(P.getPeaks(5)).toBeInstanceOf(Float32Array); // the old rejection must not win
  });

  it('rendered clip peaks share the budget and drop first', async () => {
    vi.spyOn(mock, 'sourcePeaks').mockResolvedValue(new Float32Array(4));
    vi.spyOn(mock, 'clipPeaks').mockImplementation(async () => peaksOf(1, 100)); // 100 MiB per render
    const clip = (id: number) =>
      ({ id, ref: 1000 + id, norm: false, rev: false, tune: { reference: null, pencil: [], notes: [{ shift: 1 }] } }) as unknown as Clip;
    for (const k of [1, 2, 3]) {
      P.getClipPeaks(clip(k));
      await flush();
      await new Promise(r => setTimeout(r, 120)); // the render requests are debounced ~90 ms
    }
    // 3 × 100 MiB = 300 MiB: the oldest render dropped, the newest stays
    expect(P.clipPeaksPending(clip(1))).toBe(true);
    expect(P.clipPeaksPending(clip(3))).toBe(false);
    expect(P.getClipPeaks(clip(3))).toBeInstanceOf(Float32Array);
  });
});
