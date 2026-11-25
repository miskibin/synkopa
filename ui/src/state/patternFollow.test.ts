import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import type { Clip, Id } from '../api/types';
import { syncEngine } from '../test/engine';

// #245 against the mock backend (which behaves like the engine): "+ bar" in the Channel Rack grows
// the Playlist clips that follow the pattern, never over the next clip, says so with Undo, and the
// whole change is one undo step.
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
let store: typeof import('./store');
let edit: typeof import('./edit');
let notice: typeof import('./notice');
let follow: typeof import('./patternFollow');
const sync = () => syncEngine(mock, store, edit);

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  edit = await import('./edit');
  P = await import('./pattern');
  store = await import('./store');
  notice = await import('./notice');
  follow = await import('./patternFollow');
  await sync();
}, 60_000); // the first import of the app is slow on a cold cache

const pc = (id: Id, track: number, start: number, len: number, ref: Id): Clip =>
  ({ id, track, start, len, offset: 0, kind: 'pat', ref, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false });
const lens = (ids: Id[]) => ids.map(id => store.S.project.clips.find(c => c.id === id)?.len);

describe('a growing pattern and its Playlist clips', () => {
  it('grows clips up to the next clip, says so with Undo, flashes them, and undoes in one step', async () => {
    const pt = store.S.project.patterns.find(x => x.len === 4)!;
    expect(pt).toBeTruthy();
    const lane = store.S.project.tracks.length - 1; // the last lane: empty in the starter project
    const [a, b] = [store.allocId(), store.allocId()];
    edit.commit([{ op: 'putClips', clips: [pc(a, lane, 0, 4, pt.id), pc(b, lane, 6, 4, pt.id)] }], 'place');
    await sync();
    P.addBar(store.S.project.patterns.find(x => x.id === pt.id));
    expect(store.S.project.patterns.find(x => x.id === pt.id)!.len).toBe(8);
    expect(lens([a, b])).toEqual([6, 8]); // the first stops at the second: no overlap
    const n = notice.notice()!;
    expect(n.msg).toContain(pt.name);
    expect(n.msg).toContain('2');
    expect(n.msg).toMatch(/stopped at the next clip: 1/);
    expect(n.actions.map(x => x.label)).toEqual(['Undo']);
    const fl = follow.followFlash()!;
    expect([...fl.ids].sort()).toEqual([a, b].sort());
    expect([...fl.stopped]).toEqual([a]);
    await sync();
    // the engine (mock) agrees with the optimistic edit
    expect((await mock.getState()).project.clips.filter(c => c.id === a || c.id === b).map(c => c.len)).toEqual([6, 8]);
    // Undo on the notice: the pattern and both clips come back in one step, and the notice goes
    n.actions[0].act();
    await sync();
    expect(store.S.project.patterns.find(x => x.id === pt.id)!.len).toBe(4);
    expect(lens([a, b])).toEqual([4, 4]);
    expect(notice.notice()).toBeNull();
  });

  it('says nothing when no clip changed, and another edit closes the notice', async () => {
    // a pattern of its own, so no other clip shows it
    const id = store.allocId(), ch = store.S.project.channels![0].id, lane = store.S.project.tracks.length - 1;
    edit.commit([{ op: 'putPattern', pattern: { id, name: 'Solo', color: '#fff', len: 4, notes: [{ ch, step: 0, len: 1 }] } }], 'new');
    const [a, b] = [store.allocId(), store.allocId()];
    // a clip sized on purpose (two passes) does not follow
    edit.commit([{ op: 'putClips', clips: [pc(a, lane, 0, 8, id)] }], 'place');
    await sync();
    const pat = () => store.S.project.patterns.find(x => x.id === id)!;
    P.addBar(pat());
    await sync();
    expect([pat().len, ...lens([a])]).toEqual([8, 8]);
    expect(notice.notice()).toBeNull();
    // a whole-pass clip follows: the notice shows, and an unrelated edit closes it (its Undo
    // would no longer undo the growth)
    edit.commit([{ op: 'putClips', clips: [pc(b, lane, 16, 8, id)] }], 'place');
    P.addBar(pat());
    expect(lens([b])).toEqual([12]);
    expect(notice.notice()).not.toBeNull();
    edit.commit([{ op: 'putClips', clips: [pc(store.allocId(), lane, 40, 2, id)] }], 'place');
    expect(notice.notice()).toBeNull();
    await sync();
  });
});
