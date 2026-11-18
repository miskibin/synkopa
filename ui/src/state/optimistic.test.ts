import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
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
/** The mutating queue is serial and async: let it drain before reading the mock's truth. */
const settle = () => new Promise(r => setTimeout(r, 10));
beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('./store');
  edit = await import('./edit');
  await mock.projectOpen();
  store.replaceState(await mock.getState());
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
    await settle();
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
    await settle();
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
    await settle();
    const g = new edit.Gesture('test: refused drag');
    g.update([{ op: 'setKey', key: 5, scale: 9 }]); // the engine refuses: scale must be 0..4
    expect(S.project.scale).toBe(9); // applied locally…
    g.end();
    await settle();
    expect(store.S.project.scale).toBe(2); // …but the engine's truth is back on screen
    expect((await import('./ui')).U.readout).toContain('scale must be 0..4');
  });
});
