import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';

// The optimistic pass must know every op: one it misses shows nothing until the idle
// reconcile (up to 1.5 s) — or, for a removed track, lets the next drag compute against
// stale clip→track indices and land on the wrong track engine-side.
const noop = () => {};
beforeAll(() => {
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
  vi.stubGlobal('requestAnimationFrame', (cb: () => void) => setTimeout(cb, 0));
});

let mock: MockBackend;
let store: typeof import('./store');
let edit: typeof import('./edit');
beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('./store');
  edit = await import('./edit');
  await mock.projectOpen();
  store.replaceState(await mock.getState());
});
afterEach(() => {
  mock.dispose(); // the backend's interval outlives resetModules otherwise
});

describe('optimistic editing covers every op', () => {
  it('removes a track (and renumbers the clips below) in the same frame', async () => {
    const S = store.S;
    expect(S.project.tracks.length).toBeGreaterThan(1);
    const victim = S.project.tracks[1].id;
    const onVictim = S.project.clips.filter(c => c.track === 1).map(c => c.id);
    const below = S.project.clips.filter(c => c.track === 2).map(c => c.id);
    edit.commit([{ op: 'removeTrack', id: victim }], 'test: remove track');
    expect(S.project.tracks.some(t => t.id === victim)).toBe(false); // gone locally, no engine reply yet
    expect(S.project.clips.some(c => onVictim.includes(c.id))).toBe(false); // the removed track's clips went with it
    for (const id of below) expect(S.project.clips.find(c => c.id === id)?.track).toBe(1);
    await edit.settled();
    expect((await mock.getState()).project.tracks.some(t => t.id === victim)).toBe(false); // the engine agrees
  });

  it('applies a note transform (quantize) before any engine reply', async () => {
    const S = store.S;
    const pat = S.project.patterns[0];
    const ch = S.project.channels?.[0]?.id ?? 1;
    await mock.applyOps([{ op: 'putPattern', pattern: { ...pat, notes: [{ ch, step: 1.37, len: 0.5, v: 0.9 }] } }]);
    store.replaceState(await mock.getState());
    edit.commit([{ op: 'transformNotes', pattern: pat.id, verb: { kind: 'quantize', grid: 1 } }], 'test: quantize');
    const pt = S.project.patterns.find(x => x.id === pat.id)!;
    expect(Math.abs(pt.notes[0].step - 1)).toBeLessThan(1e-6); // snapped locally: the button feels instant
    await edit.settled();
    const engine = (await mock.getState()).project.patterns.find(x => x.id === pat.id)!;
    expect(Math.abs(engine.notes[0].step - 1)).toBeLessThan(1e-6);
  });

  it('sets key and scale immediately', async () => {
    const S = store.S;
    edit.commit([{ op: 'setKey', key: 3, scale: 2 }], 'test: set key');
    expect(S.project.key).toBe(3);
    expect(S.project.scale).toBe(2);
  });

  it('pulls the engine state back when the engine refuses a gesture batch', async () => {
    const S = store.S;
    edit.commit([{ op: 'setKey', key: 3, scale: 2 }], 'test: set key');
    await edit.settled();
    const g = new edit.Gesture('test: refused drag');
    g.update([{ op: 'setKey', key: 5, scale: 9 }]); // the engine refuses: scale must be 0..4
    expect(S.project.scale).toBe(9); // applied locally…
    g.end();
    await edit.settled();
    expect(store.S.project.scale).toBe(2); // …but the engine's truth is back on screen
    expect((await import('./ui')).U.readout).toContain('scale must be 0..4');
  });

  it('reapplies still-pending drag frames after a refused frame wipes them', async () => {
    edit.commit([{ op: 'setKey', key: 3, scale: 2 }], 'test: set key');
    await edit.settled();
    const realApply = mock.applyOps.bind(mock);
    const realGet = mock.getState.bind(mock);
    let applyCalls = 0;
    let getCalls = 0;
    let releaseRead: () => void = () => {};
    const readGate = new Promise<void>(r => { releaseRead = r; });
    vi.spyOn(mock, 'applyOps').mockImplementation(async (ops, opts) => {
      if (applyCalls++ === 0) throw new Error('scale must be 0..4'); // only the first frame refuses
      return realApply(ops, opts);
    });
    // hold the refusal's state read open, so a newer frame can buffer while it is in flight
    vi.spyOn(mock, 'getState').mockImplementation(async () => {
      if (getCalls++ === 0) await readGate;
      return realGet();
    });
    const g = new edit.Gesture('test: refused drag, newer frame buffered');
    g.update([{ op: 'setKey', key: 5, scale: 9 }]); // frame A: refused engine-side
    await new Promise(r => setTimeout(r, 0)); // the rAF flush: frame A refused, its read-back in flight
    g.update([{ op: 'setKey', key: 3, scale: 3 }]); // frame B: valid, buffered while the read-back is in flight
    expect(store.S.project.scale).toBe(3); // applied locally
    releaseRead(); // the read-back lands now: it must not leave frame B off the screen
    // asserted while the gesture is still open: reconciliation waits for the gesture to
    // close, so a wiped preview could not silently repair itself here
    await edit.settled();
    expect(store.S.project.scale).toBe(3); // the still-pending frame is back on screen…
    g.end();
    await edit.settled();
    expect(store.S.project.scale).toBe(3); // …and it stays
    expect((await mock.getState()).project.scale).toBe(3); // the engine took the pending frame too
  });

  it('repairs a refused frame even when the first state read fails', async () => {
    edit.commit([{ op: 'setKey', key: 3, scale: 2 }], 'test: set key');
    await edit.settled();
    const g = new edit.Gesture('test: refused drag, backend down');
    g.update([{ op: 'setKey', key: 5, scale: 9 }]);
    g.end();
    // the refusal's read-back itself fails once: the bounded retry must still repair the screen
    vi.spyOn(mock, 'getState').mockRejectedValueOnce(new Error('backend down'));
    await vi.waitFor(() => expect(store.S.project.scale).toBe(2), { timeout: 3000 });
  });

  it('refuses an out-of-range euclid size instead of allocating it', async () => {
    const pat = store.S.project.patterns[0];
    const before = JSON.stringify(pat.notes ?? []);
    edit.commit([{ op: 'transformNotes', pattern: pat.id, verb: { kind: 'euclidFill', ch: 1, k: 4, n: 1e9 } }], 'test: huge euclid');
    // refused before the transform allocates per slot: the notes stay as they were, at once
    expect(JSON.stringify(store.S.project.patterns.find(x => x.id === pat.id)!.notes ?? [])).toBe(before);
  });
});
