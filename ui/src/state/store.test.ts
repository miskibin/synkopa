// The engine-state mirror (`store.ts`) is what every view reads. These tests pin its public
// contract, the part a reactivity rewrite (Solid 2) is most likely to break:
// - optimistic edits apply in place, so unchanged items keep their identity and only the
//   readers of a changed field re-run;
// - `projVersion` / `arrVersion` bump once per change, `arrVersion` not for mixer/loop edits;
// - `onArrangementChange` hears every arrangement edit *before* it lands;
// - an engine state reconciles by id (identity kept), `replaceMeta` leaves the project alone;
// - `plain` / `plainNotes` return detached copies.
// Reads of derived state go through `settle()` (src/test/settle.ts), the one place to flush.
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Clip, Insert, Project, StateDto, Track } from '../api/types';
import { settle } from '../test/settle';

beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
});

let store: typeof import('./store');
// imported after resetModules with the store, so the effects below track the store's own Solid
let createEffect: typeof import('solid-js').createEffect;
let createRoot: typeof import('solid-js').createRoot;
let snapshot: typeof import('solid-js').snapshot;
beforeEach(async () => {
  vi.resetModules(); // the store, its listeners and its id counter are module state
  store = await import('./store');
  ({ createEffect, createRoot, snapshot } = await import('solid-js'));
});

const track = (id: number, extra: Partial<Track> = {}): Track => ({ id, name: 'T' + id, color: '#888', muted: false, solo: false, arm: false, vol: 0.8, h: 56, insert: 0, ...extra });
const clip = (id: number, extra: Partial<Clip> = {}): Clip => ({ id, track: 0, start: 0, len: 4, offset: 0, kind: 'pat', ref: 1, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false, ...extra });
const master = (): Insert => ({ id: 0, name: 'Master', color: '#ddd', vol: 0.79, pan: 0, muted: false, solo: false, fx: [] });
const ins = (id: number): Insert => ({ id, name: 'Ins' + id, color: '#888', vol: 0.79, pan: 0, muted: false, solo: false, fx: [{ id: 900 + id, type: 'comp', bypass: false, preset: null, params: { thr: -10 } } as never] });

function project(over: Partial<Project> = {}): Project {
  return {
    version: 2, bpm: 120, loop: null, nextId: 100,
    tracks: [track(1), track(2), track(3)],
    inserts: [master(), ins(1)],
    patterns: [{ id: 1, name: 'Beat', color: '#4f8ef7', len: 4, notes: [{ ch: 10, step: 0, len: 1 }, { ch: 10, step: 2, len: 1 }] } as never],
    sources: [{ id: 50, name: 'Kick', color: '#fff', path: '/a/kick.wav', dur: 1, sr: 48000, ch: 1, peakDb: 0 }],
    clips: [clip(10, { track: 0, start: 0 }), clip(11, { track: 1, start: 4 }), clip(12, { track: 2, start: 8 })],
    ...over,
  };
}
function dto(p: Project, over: Partial<StateDto> = {}): StateDto {
  return {
    project: p, revision: 1, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null, path: null, dirty: false,
    transport: { playing: false, rec: false, monitor: false, metro: false, countIn: true, startBeat: 0, recAudio: true, recNotes: true, recQuantize: 0, recAutomation: true, recAutoLatch: false },
    ...over,
  };
}
/** Run `read` in an effect; returns how many times it has run (1 = the initial run, done here). */
function runs(read: () => unknown): { n: number; dispose: () => void } {
  const r = { n: 0, dispose: () => {} };
  createRoot(d => { r.dispose = d; createEffect(() => { read(); }, () => { r.n++; }); });
  settle(); // Solid 2 makes an effect's first run on the next flush
  return r;
}
const clipById = (id: number) => store.S.project.clips.find(c => c.id === id)!;

describe('replaceState (a whole state from the engine)', () => {
  it('takes the project and the meta fields and marks the store ready', () => {
    expect(store.S.ready).toBe(false);
    store.replaceState(dto(project(), { revision: 7, canUndo: true, undoLabel: 'Move clips', path: '/song.synkopa', dirty: true }));
    settle();
    expect(store.S.ready).toBe(true);
    expect(store.S.project.clips.map(c => c.id)).toEqual([10, 11, 12]);
    expect([store.S.revision, store.S.canUndo, store.S.undoLabel, store.S.path, store.S.dirty]).toEqual([7, true, 'Move clips', '/song.synkopa', true]);
  });

  it('reconciles by id: unchanged items keep their identity, changed ones update in place', () => {
    store.replaceState(dto(project()));
    const c10 = clipById(10), c11 = clipById(11), t1 = store.S.project.tracks[0];
    const next = project();
    next.clips[1] = clip(11, { track: 1, start: 6 });
    next.clips.splice(2, 1); // clip 12 deleted engine-side
    store.replaceState(dto(next, { revision: 2 }));
    settle();
    expect(clipById(10)).toBe(c10);
    expect(clipById(11)).toBe(c11);
    expect(c11.start).toBe(6);
    expect(store.S.project.tracks[0]).toBe(t1);
    expect(store.S.project.clips.some(c => c.id === 12)).toBe(false);
  });

  it('re-runs only the readers of a field that changed', () => {
    store.replaceState(dto(project()));
    const r10 = runs(() => clipById(10).start);
    const r11 = runs(() => clipById(11).start);
    const next = project();
    next.clips[1] = clip(11, { track: 1, start: 6 });
    store.replaceState(dto(next, { revision: 2 }));
    settle();
    expect(r10.n).toBe(1);
    expect(r11.n).toBe(2);
    r10.dispose(); r11.dispose();
  });

  it('bumps projVersion and arrVersion once per state', () => {
    const p0 = store.projVersion(), a0 = store.arrVersion();
    store.replaceState(dto(project()));
    settle();
    expect(store.projVersion()).toBe(p0 + 1);
    expect(store.arrVersion()).toBe(a0 + 1);
  });

  it('an identical state (our own edit, echoed) bumps nothing and tells no arrangement listener', () => {
    store.replaceState(dto(project()));
    settle();
    const heard: unknown[] = [];
    store.onArrangementChange(ops => heard.push(ops));
    const p0 = store.projVersion(), a0 = store.arrVersion(), c10 = clipById(10), rawClips = store.rawOf(store.S.project).clips;
    const r = runs(() => store.S.project.clips.length + store.S.project.inserts.length);
    store.replaceState(dto(project(), { revision: 2, dirty: true }));
    settle();
    expect([store.projVersion(), store.arrVersion(), r.n]).toEqual([p0, a0, 1]);
    expect(heard).toEqual([]);
    expect(clipById(10)).toBe(c10);
    // the plain mirror keeps its objects too: what a canvas read from it before stays current
    expect(store.rawOf(store.S.project).clips).toBe(rawClips);
    expect([store.S.revision, store.S.dirty]).toEqual([2, true]); // the meta fields still land
    r.dispose();
  });

  it('a state that changes only the mixer, the notebook or the loop is not an arrangement change', () => {
    store.replaceState(dto(project()));
    settle();
    const heard: unknown[] = [];
    store.onArrangementChange(ops => heard.push(ops));
    const i1 = store.S.project.inserts[1], rawClips = store.rawOf(store.S.project).clips;
    const p0 = store.projVersion(), a0 = store.arrVersion();
    const next = project({ loop: { a: 0, b: 4 }, nextId: 101 });
    next.inserts[1] = { ...ins(1), vol: 0.5, name: 'Bass' };
    store.replaceState(dto(next, { revision: 2 }));
    settle();
    expect(store.S.project.inserts[1]).toBe(i1); // merged in place
    expect([i1.vol, i1.name]).toEqual([0.5, 'Bass']);
    expect(store.S.project.loop).toEqual({ a: 0, b: 4 });
    expect(store.projVersion()).toBe(p0 + 1);
    expect(store.arrVersion()).toBe(a0);
    expect(heard).toEqual([]);
    expect(store.rawOf(store.S.project).inserts[1].vol).toBe(0.5);
    expect(store.rawOf(store.S.project).clips).toBe(rawClips);
  });

  it('a project field the engine no longer sends is removed', () => {
    store.replaceState(dto(project({ automations: [{ id: 70, name: 'A', color: '#fff', target: { kind: 'insertVol', id: 1 }, points: [] } as never] })));
    settle();
    const a0 = store.arrVersion();
    store.replaceState(dto(project(), { revision: 2 }));
    settle();
    expect('automations' in store.S.project).toBe(false);
    expect(store.arrVersion()).toBe(a0 + 1);
  });

  it('tells arrangement listeners, with null ops and the project as it was', () => {
    store.replaceState(dto(project()));
    const seen: [unknown, number][] = [];
    store.onArrangementChange((ops, before) => seen.push([ops, before.clips.length]));
    const next = project(); next.clips.pop();
    store.replaceState(dto(next, { revision: 2 }));
    expect(seen).toEqual([[null, 3]]);
  });

  it('replaces the live-only maps instead of merging them', () => {
    store.replaceState(dto(project(), { missing: { 50: 'gone' }, fxErrors: { 901: 'not installed' }, stretching: [10], stretchErrors: { 11: 'failed' } }));
    expect(store.S.missing).toEqual({ 50: 'gone' });
    expect(store.S.stretching).toEqual({ 10: '', 11: 'failed' });
    store.replaceState(dto(project(), { revision: 2 }));
    settle();
    expect(store.S.missing).toEqual({});
    expect(store.S.fxErrors).toEqual({});
    expect(store.S.stretching).toEqual({});
  });
});

describe('replaceMeta (our own edit came back: the project is already applied locally)', () => {
  it('updates revision, undo and transport but never touches the project', () => {
    store.replaceState(dto(project()));
    const p0 = store.projVersion();
    const c10 = clipById(10);
    const r = runs(() => store.S.project.clips.length);
    const other = project(); other.clips = []; // must be ignored
    store.replaceMeta(dto(other, { revision: 5, canUndo: true, undoLabel: 'Draw', dirty: true, transport: { ...dto(other).transport, playing: true } }));
    settle();
    expect(store.S.revision).toBe(5);
    expect(store.S.undoLabel).toBe('Draw');
    expect(store.S.transport.playing).toBe(true);
    expect(store.S.project.clips).toHaveLength(3);
    expect(clipById(10)).toBe(c10);
    expect(store.projVersion()).toBe(p0);
    expect(r.n).toBe(1);
    r.dispose();
  });

  it('keeps allocated ids above the engine’s nextId', () => {
    store.replaceState(dto(project({ nextId: 100 })));
    expect(store.allocId()).toBe(100);
    store.replaceMeta(dto(project({ nextId: 500 }), { revision: 2 }));
    expect(store.allocId()).toBe(500);
    expect(store.allocId()).toBe(501);
  });
});

describe('applyLocal (optimistic edits)', () => {
  beforeEach(() => store.replaceState(dto(project())));

  it('moves a clip in place: same object, only its readers re-run, synchronously visible', () => {
    const c10 = clipById(10), c11 = clipById(11);
    const r10 = runs(() => c10.start), r11 = runs(() => c11.start);
    store.applyLocal([{ op: 'putClips', clips: [{ ...store.plain(c10), start: 12 }] }]);
    settle();
    expect(clipById(10)).toBe(c10);
    expect(c10.start).toBe(12);
    expect(r10.n).toBe(2);
    expect(r11.n).toBe(1);
    r10.dispose(); r11.dispose();
  });

  it('adds a new clip and raises nextId past it', () => {
    store.applyLocal([{ op: 'putClips', clips: [clip(150, { start: 16 })] }]);
    settle();
    expect(clipById(150).start).toBe(16);
    expect(store.S.project.nextId).toBe(151);
    expect(store.allocId()).toBe(151);
  });

  it('marks the project dirty and bumps both versions for an arrangement edit', () => {
    const p0 = store.projVersion(), a0 = store.arrVersion();
    store.applyLocal([{ op: 'removeClips', ids: [11] }]);
    settle();
    expect(store.S.dirty).toBe(true);
    expect(store.projVersion()).toBe(p0 + 1);
    expect(store.arrVersion()).toBe(a0 + 1);
  });

  it('a mixer-only or loop edit bumps projVersion but not arrVersion, and is not an arrangement change', () => {
    const heard: unknown[] = [];
    store.onArrangementChange(ops => heard.push(ops));
    const p0 = store.projVersion(), a0 = store.arrVersion();
    const fx = store.S.project.inserts[1].fx[0];
    store.applyLocal([{ op: 'putFx', insertId: 1, fx: { ...store.plain(fx), bypass: true } }]);
    store.applyLocal([{ op: 'setLoop', loop: { a: 0, b: 8 } }]);
    settle();
    expect(store.S.project.inserts[1].fx[0].bypass).toBe(true);
    expect(store.S.project.loop).toEqual({ a: 0, b: 8 });
    expect(store.projVersion()).toBe(p0 + 2);
    expect(store.arrVersion()).toBe(a0);
    expect(heard).toEqual([]);
  });

  it('a mixed batch counts as an arrangement edit', () => {
    const a0 = store.arrVersion();
    store.applyLocal([{ op: 'setLoop', loop: { a: 0, b: 4 } }, { op: 'setBpm', bpm: 140 }]);
    expect(store.arrVersion()).toBe(a0 + 1);
  });

  it('normalizes the optimistic tempo map like the engine accepts it, and the bpm knob clears it', () => {
    store.applyLocal([{ op: 'setTempoPoints', points: [{ beat: 8, bpm: 400 }, { beat: -1, bpm: 10 }] }]);
    expect(store.S.project.tempoPoints).toEqual([{ beat: 0, bpm: 20, ramp: false }, { beat: 8, bpm: 300, ramp: false }]);
    store.applyLocal([{ op: 'setTempoPoints', points: [] }]);
    expect(store.S.project.tempoPoints).toBeUndefined();
    store.applyLocal([{ op: 'setTempoPoints', points: [{ beat: 4, bpm: 150 }] }]);
    expect(store.S.project.tempoPoints).toEqual([{ beat: 4, bpm: 150, ramp: false }]);
    // above the cap the optimistic arm refuses whole (the engine's reply reports the refusal)
    const over = Array.from({ length: 4097 }, (_, i) => ({ beat: i % 2048, bpm: 100 + (i % 10) }));
    store.applyLocal([{ op: 'setTempoPoints', points: over }]);
    expect(store.S.project.tempoPoints).toEqual([{ beat: 4, bpm: 150, ramp: false }]);
    store.applyLocal([{ op: 'setBpm', bpm: 132 }]);
    expect(store.S.project.bpm).toBe(132);
    expect(store.S.project.tempoPoints).toBeUndefined();
  });

  it('an empty batch changes nothing', () => {
    const p0 = store.projVersion();
    store.applyLocal([]);
    expect(store.projVersion()).toBe(p0);
    expect(store.S.dirty).toBe(false);
  });

  it('tells arrangement listeners the ops before they land', () => {
    let startSeen = -1, opsSeen: unknown = null;
    store.onArrangementChange((ops, before) => { opsSeen = ops; startSeen = before.clips.find(c => c.id === 10)!.start; });
    const ops = [{ op: 'putClips' as const, clips: [{ ...store.plain(clipById(10)), start: 20 }] }];
    store.applyLocal(ops);
    expect(opsSeen).toBe(ops);
    expect(startSeen).toBe(0);
    expect(clipById(10).start).toBe(20);
  });

  it('stops telling a listener once it unsubscribes', () => {
    let n = 0;
    const off = store.onArrangementChange(() => { n++; });
    store.applyLocal([{ op: 'setBpm', bpm: 130 }]);
    off();
    store.applyLocal([{ op: 'setBpm', bpm: 131 }]);
    expect(n).toBe(1);
  });

  it('one batch: an effect reading the version and the data runs once, with both current', () => {
    const seen: [number, number][] = [];
    const r = createRoot(d => { createEffect(() => [store.arrVersion(), store.S.project.clips.length] as [number, number], v => { seen.push(v); }); return d; });
    settle();
    const a0 = store.arrVersion();
    store.applyLocal([{ op: 'removeClips', ids: [10] }, { op: 'removeClips', ids: [11] }]);
    settle();
    expect(seen).toEqual([[a0, 3], [a0 + 1, 1]]);
    r();
  });

  it('removing a track drops its clips and moves the clips below it up', () => {
    store.applyLocal([{ op: 'removeTrack', id: 2 }]);
    settle();
    expect(store.S.project.tracks.map(t => t.id)).toEqual([1, 3]);
    expect(store.S.project.clips.map(c => [c.id, c.track])).toEqual([[10, 0], [12, 1]]);
  });

  it('derived lookups follow local edits (solo memo)', () => {
    const r = runs(() => store.anyTrackSolo());
    expect(store.anyTrackSolo()).toBe(false);
    store.applyLocal([{ op: 'putTrack', track: { ...store.plain(store.S.project.tracks[1]), solo: true } }]);
    settle();
    expect(store.anyTrackSolo()).toBe(true);
    expect(r.n).toBe(2);
    r.dispose();
  });

  it('the engine’s agreeing reply keeps identity and re-runs nobody; a disagreeing one corrects', () => {
    const c10 = clipById(10);
    store.applyLocal([{ op: 'putClips', clips: [{ ...store.plain(c10), start: 8 }] }]);
    const r = runs(() => c10.start);
    const agree = project(); agree.clips[0] = clip(10, { start: 8 });
    store.replaceState(dto(agree, { revision: 2 }));
    settle();
    expect(clipById(10)).toBe(c10);
    expect(r.n).toBe(1);
    const refuse = project(); // the engine kept the clip at 0
    store.replaceState(dto(refuse, { revision: 3 }));
    settle();
    expect(c10.start).toBe(0);
    expect(r.n).toBe(2);
    r.dispose();
  });
});

describe('plain copies', () => {
  beforeEach(() => store.replaceState(dto(project())));

  it('plain gives a detached deep copy of store data', () => {
    const p = store.plain(store.S.project.clips);
    expect(p).toEqual(project().clips);
    p[0].start = 99;
    expect(clipById(10).start).toBe(0);
  });

  it('plain unwraps store items inside a fresh container', () => {
    const c = store.plain({ a: clipById(10), list: [clipById(11)] });
    expect(c.a).toEqual(project().clips[0]);
    c.list[0].start = 77;
    expect(clipById(11).start).toBe(4);
  });

  it('plainNotes copies each note so editing the copy leaves the store alone', () => {
    const notes = store.S.project.patterns[0].notes;
    const copy = store.plainNotes(notes);
    expect(copy).toEqual([{ ch: 10, step: 0, len: 1 }, { ch: 10, step: 2, len: 1 }]);
    copy[0].step = 3;
    expect(store.S.project.patterns[0].notes[0].step).toBe(0);
  });
});

// Solid 2: the store keeps a plain copy of the project (`M`, see store.ts) beside the reactive one.
describe('the plain copy (rawOf / plain)', () => {
  // the store's own data (Solid's snapshot walks the proxies), not the plain copy
  const json = (v: unknown) => JSON.parse(JSON.stringify(v));
  const same = () => expect(json(store.rawOf(store.S.project))).toEqual(json(snapshot(store.S.project)));

  it('stays equal to the store through local edits and engine states, with no object shared', () => {
    store.replaceState(dto(project()));
    store.applyLocal([{ op: 'putClips', clips: [clip(10, { start: 3 }), clip(13, { track: 2, start: 1 })] }]);
    store.applyLocal([{ op: 'putInsert', insert: { ...ins(2), sends: [{ to: 1, gain: 0.5 }] } as never }]);
    store.applyLocal([{ op: 'removeTrack', id: 2 }, { op: 'setBpm', bpm: 99 }, { op: 'setLoop', loop: { a: 1, b: 5 } }]);
    store.applyLocal([{ op: 'putPattern', pattern: { id: 1, name: 'Beat', color: '#4f8ef7', len: 4, notes: [{ ch: 10, step: 1, len: 1 }] } as never }]);
    same();
    // the plain copy never hands an object to the store: editing it in place would change the store unseen
    const raw = store.rawOf(store.S.project);
    expect(raw.inserts.find(m => m.id === 2)!.sends).not.toBe(store.S.project.inserts.find(m => m.id === 2)!.sends);
    const next = project(); next.clips.reverse(); next.bpm = 77; next.loop = { a: 0, b: 2 };
    store.replaceState(dto(next, { revision: 9 }));
    same();
    expect(store.S.project.clips.map(c => c.id)).toEqual([12, 11, 10]);
  });

  it('rawOf gives the plain item behind a store item (by id), and plain data back as is', () => {
    store.replaceState(dto(project()));
    const c = clipById(11), r = store.rawOf(c);
    expect(r).not.toBe(c);
    expect(r).toEqual(project().clips[1]);
    expect(store.rawOf(r)).toBe(r);
    store.applyLocal([{ op: 'putClips', clips: [{ ...store.plain(c), start: 9 }] }]);
    expect(store.rawOf(c).start).toBe(9);
    expect(store.rawOf(store.S.project.patterns[0]).notes).toEqual([{ ch: 10, step: 0, len: 1 }, { ch: 10, step: 2, len: 1 }]);
  });

  it('an engine state that reorders and edits keeps every surviving item’s identity, and a held item stays live', () => {
    store.replaceState(dto(project()));
    const c10 = clipById(10), t3 = store.S.project.tracks[2];
    const next = project();
    next.clips = [clip(12, { track: 2, start: 8 }), clip(10, { start: 5 }), clip(14, { start: 1 })];
    next.tracks = [next.tracks[2], next.tracks[0]];
    store.replaceState(dto(next, { revision: 2 }));
    expect(clipById(10)).toBe(c10);
    expect(c10.start).toBe(5);
    expect(store.S.project.tracks[0]).toBe(t3);
    expect(store.S.project.clips.map(c => c.id)).toEqual([12, 10, 14]);
  });

  it('plainNotes copies notes given as store items or as plain ones', () => {
    store.replaceState(dto(project()));
    const pt = store.S.project.patterns[0];
    expect(store.plainNotes(pt.notes)).toEqual(store.plainNotes(store.rawOf(pt).notes));
    expect(store.plainNotes(store.rawOf(pt).notes)[0]).not.toBe(store.rawOf(pt).notes[0]);
  });
});

// `rawOf` caches a store item's plain object (views call it per note, per pointer move): a hit must
// always be the object the plain copy holds now, never one from before a change.
describe('rawOf cache', () => {
  /** The plain copy's own item with that id, found without `rawOf`'s cache (through the project). */
  const live = (pick: (p: Project) => unknown) => pick(store.rawOf(store.S.project));
  /** Ids unique across kinds, as the engine's counter makes them (`project()` reuses 1 for a track, an insert and the pattern). */
  const uniq = () => project({ inserts: [master(), ins(5)], patterns: [{ id: 6, name: 'Beat', color: '#4f8ef7', len: 4, notes: [{ ch: 10, step: 0, len: 1 }] } as never], clips: [clip(10, { ref: 6 }), clip(11, { track: 1, start: 4, ref: 6 }), clip(12, { track: 2, start: 8, ref: 6 })] });

  it('a repeated call is the live plain object, the same one every time', () => {
    store.replaceState(dto(uniq()));
    const c = clipById(11), pt = store.S.project.patterns[0], fx = store.S.project.inserts[1].fx[0];
    expect(store.rawOf(c)).toBe(live(p => p.clips[1]));
    expect(store.rawOf(c)).toBe(store.rawOf(c));
    expect(store.rawOf(pt)).toBe(live(p => p.patterns[0]));
    expect(store.rawOf(fx)).toBe(live(p => p.inserts[1].fx[0]));
    expect(store.rawOf(store.S.project)).toBe(store.rawOf(store.S.project));
  });

  it('after a local edit it is the new plain object, also when the edit replaced it', () => {
    store.replaceState(dto(uniq()));
    const c = clipById(11), pt = store.S.project.patterns[0];
    const oldPt = store.rawOf(pt);
    void store.rawOf(c);
    store.applyLocal([{ op: 'putClips', clips: [{ ...store.plain(c), start: 9 }] }]);
    expect(store.rawOf(c).start).toBe(9);
    expect(store.rawOf(c)).toBe(live(p => p.clips[1]));
    // putPattern gives the plain copy a new pattern object
    store.applyLocal([{ op: 'putPattern', pattern: { ...store.plain(pt), notes: [{ ch: 10, step: 3, len: 1 }] } }]);
    const pt2 = store.S.project.patterns[0]; // the store's pattern after the edit (the edit copies it)
    expect(store.rawOf(pt2)).not.toBe(oldPt);
    expect(store.rawOf(pt2)).toBe(live(p => p.patterns[0]));
    expect(store.rawOf(pt2).notes).toEqual([{ ch: 10, step: 3, len: 1 }]);
  });

  it('after an engine state it is the object of the new state, never the old one', () => {
    store.replaceState(dto(uniq()));
    const c = clipById(10), proj = store.S.project;
    const before = store.rawOf(c), beforeProject = store.rawOf(proj);
    const next = uniq(); next.clips = [clip(12, { track: 2, start: 8, ref: 6 }), clip(10, { start: 5, ref: 6 })];
    store.replaceState(dto(next, { revision: 2 }));
    expect(clipById(10)).toBe(c);
    expect(store.rawOf(c)).not.toBe(before);
    expect(store.rawOf(c)).toBe(next.clips[1]);
    expect(store.rawOf(c).start).toBe(5);
    expect(store.rawOf(proj)).not.toBe(beforeProject);
    // the new state's objects where it changed something (fields it left as they were keep theirs)
    expect(store.rawOf(proj)).toEqual(next);
    expect(store.rawOf(proj).clips).toBe(next.clips);
  });

  it('stays the live object through a run of edits and states, for every item kind it indexes', () => {
    store.replaceState(dto(uniq()));
    const check = () => {
      const p = store.S.project, raw = store.rawOf(p);
      p.clips.forEach((c, i) => expect(store.rawOf(c)).toBe(raw.clips[i]));
      p.tracks.forEach((t, i) => expect(store.rawOf(t)).toBe(raw.tracks[i]));
      p.inserts.forEach((m, i) => { expect(store.rawOf(m)).toBe(raw.inserts[i]); m.fx.forEach((f, j) => expect(store.rawOf(f)).toBe(raw.inserts[i].fx[j])); });
      p.patterns.forEach((x, i) => expect(store.rawOf(x)).toBe(raw.patterns[i]));
      expect(store.rawOf(p.clips).map(c => c.id)).toEqual(raw.clips.map(c => c.id));
    };
    check();
    store.applyLocal([{ op: 'putClips', clips: [clip(10, { start: 3, ref: 6 }), clip(13, { track: 2, start: 1, ref: 6 })] }]); check();
    store.applyLocal([{ op: 'removeClips', ids: [11] }]); check();
    store.applyLocal([{ op: 'removeTrack', id: 2 }, { op: 'putInsert', insert: ins(7) }]); check();
    store.applyLocal([{ op: 'putFx', insertId: 7, index: 0, fx: { id: 20, type: 'comp', bypass: false, preset: null, params: { thr: -3 } } as never }]); check();
    store.applyLocal([{ op: 'moveFx', fxId: 20, to: 1 }]); check();
    const next = uniq(); next.clips.reverse(); next.inserts = [master(), ins(7), ins(5)];
    store.replaceState(dto(next, { revision: 5 })); check();
    store.replaceMeta(dto(next, { revision: 6, canUndo: true })); check();
  });

  it('a removed item is not answered from the plain copy (its last data, as a copy)', () => {
    store.replaceState(dto(uniq()));
    const c = clipById(11), before = store.rawOf(c);
    store.applyLocal([{ op: 'removeClips', ids: [11] }]);
    const r = store.rawOf(c);
    expect(r).not.toBe(before);
    expect(store.rawOf(store.S.project).clips.includes(r)).toBe(false);
    expect(r).toEqual(before);
  });

  it('two kinds sharing an id: each item gets its own data (the guard against a wrong item)', () => {
    store.replaceState(dto(project())); // track 1, insert 1 and pattern 1
    const t1 = store.S.project.tracks[0], p1 = store.S.project.patterns[0];
    expect(store.rawOf(t1)).toEqual(project().tracks[0]);
    expect(store.rawOf(t1)).toEqual(project().tracks[0]); // twice: a cache hit too
    expect(store.rawOf(p1)).toBe(live(p => p.patterns[0]));
  });

  // The engine's demo song numbers patterns, clips and inserts from 1 each: a lookup by id that kept
  // one place per id copied the whole pattern (2000 notes: ~13 ms per drag frame) on every call.
  it('ids shared across lists (the demo song): every item is still its live plain object, not a copy', () => {
    store.replaceState(dto(project({ clips: [clip(1, { track: 0 }), clip(11, { track: 1, start: 4 })] }))); // track, insert, pattern and clip 1
    const p = store.S.project;
    for (let k = 0; k < 2; k++) { // the second round is answered by the cache
      expect(store.rawOf(p.patterns[0])).toBe(live(x => x.patterns[0]));
      expect(store.rawOf(p.tracks[0])).toBe(live(x => x.tracks[0]));
      expect(store.rawOf(p.inserts[1])).toBe(live(x => x.inserts[1]));
      expect(store.rawOf(p.clips[0])).toBe(live(x => x.clips[0]));
    }
    store.applyLocal([{ op: 'putPattern', pattern: { ...store.plain(p.patterns[0]), notes: [{ ch: 10, step: 3, len: 1 }] } }]);
    expect(store.rawOf(store.S.project.patterns[0])).toBe(live(x => x.patterns[0]));
    expect(store.rawOf(store.S.project.patterns[0]).notes).toEqual([{ ch: 10, step: 3, len: 1 }]);
  });

  it('a copy of an item in another store is never cached as the item', async () => {
    const { createStore } = await import('../lib/reactive');
    store.replaceState(dto(uniq()));
    const [ui, setUi] = createStore({ c: store.plain(clipById(11)) as Clip & { extra?: number } });
    // same id and fields: the item (the check rawOf has always made)
    expect(store.rawOf(ui.c)).toBe(store.rawOf(clipById(11)));
    // the copy gains a field: its own data from then on
    setUi('c', 'extra', 1);
    expect(store.rawOf(ui.c)).toEqual({ ...store.plain(clipById(11)), extra: 1 });
    expect(store.rawOf(ui.c)).not.toBe(store.rawOf(clipById(11)));
  });
});
