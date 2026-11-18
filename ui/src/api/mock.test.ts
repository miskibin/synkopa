import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';
import { demoProject, emptyProject, FACTORY_FILES, FACTORY_KIT, libPath } from '../lib/factory';
import { migrateProject } from '../lib/pattern';
import type { ExportProgress, ExportStemsResult } from './types';

// the mock publishes itself on `window` for dev tools; node has none
beforeAll(() => { vi.stubGlobal('window', {}); });

describe('standard library in the mock', () => {
  it('lists the standard library first, with groups and root keys', async () => {
    const be = new MockBackend();
    const { kits } = await be.libraryList();
    expect(kits[0]).toMatchObject({ id: FACTORY_KIT, installed: true, count: FACTORY_FILES.length });
    const files = (await be.libraryFiles({ kit: FACTORY_KIT })).files;
    expect(files).toHaveLength(FACTORY_FILES.length);
    expect(files.find(f => f.name === 'Marimba Low')).toMatchObject({ group: 'Tonal', root: 36, path: libPath('marimba-low.flac') });
  });
  it('keeps a library file as one source however often it is imported', async () => {
    const be = new MockBackend();
    await be.projectNew();
    const before = (await be.getState()).project.sources.length;
    const a = await be.importAudio({ paths: [libPath('triangle.flac')] });
    const b = await be.importAudio({ paths: [libPath('triangle.flac')], track: 2, beat: 4 });
    expect(b.sourceIds).toEqual(a.sourceIds);
    const p = (await be.getState()).project;
    expect(p.sources).toHaveLength(before + 1);
    expect(p.sources.find(s => s.id === a.sourceIds[0])).toMatchObject({ name: 'Triangle', path: 'lib:Standard/triangle.flac' });
    expect(p.clips.filter(c => c.kind === 'audio' && c.ref === a.sourceIds[0])).toHaveLength(1);
    // the starter kit's own file is found too
    const kick = await be.importAudio({ paths: [libPath('kick-808-short.flac')] });
    expect((await be.getState()).project.sources).toHaveLength(before + 1);
    expect(p.channels!.some(c => c.kind === 'sample' && c.source === kick.sourceIds[0])).toBe(true);
    // an ordinary file is a new source every time, as in the engine
    await be.importAudio({ paths: ['C:\\\\loops\\\\beat.wav'] });
    await be.importAudio({ paths: ['C:\\\\loops\\\\beat.wav'] });
    expect((await be.getState()).project.sources).toHaveLength(before + 3);
  });
  it('starts new projects like the engine: empty with the starter kit, or the demo project', async () => {
    const be = new MockBackend();
    const empty = (await be.projectNew()).project;
    expect(empty).toEqual(emptyProject());
    expect(empty.channels!.map(c => c.name)).toEqual(['Kick', 'Clap', 'Hat', 'Snare']);
    const demo = (await be.projectNew(true)).project;
    expect(demo).toEqual(demoProject());
    expect(demo.bpm).toBe(136);
    expect(demo.sources.every(s => s.path.startsWith('lib:Standard/'))).toBe(true);
    for (const p of [empty, demo]) { const q = structuredClone(p); migrateProject(q); expect(q).toEqual(p); }
  });
});

describe('pattern export in the mock (#125)', () => {
  const opts = { useLoop: false, tail: 2, sampleRate: 48000 };
  it('renders the current pattern with an empty Playlist, over the pattern, and leaves the project as it was', async () => {
    const be = new MockBackend();
    await be.projectNew();
    const st = await be.getState();
    const [kick] = st.project.channels!;
    const pt = st.project.patterns[0];
    await be.applyOps([{ op: 'putPattern', pattern: { ...pt, notes: [{ ch: kick.id, step: 0, len: 1, trig: true }] } }]);
    const before = (await be.getState()).project;
    expect(before.clips).toEqual([]);
    const song = await be.exportAudio({ ...opts, format: 'wav', wavBits: 32 });
    if (song.cancelled) throw new Error('cancelled');
    expect(song.stats.silent).toBe(true); // what the issue saw: Song of an empty Playlist
    const r = await be.exportAudio({ ...opts, pattern: pt.id, format: 'wav', wavBits: 32 });
    if (r.cancelled) throw new Error('cancelled');
    expect(r.stats.silent).toBe(false);
    expect([r.fromBeat, r.toBeat]).toEqual([0, 4]);
    expect((await be.getState()).project).toEqual(before);
    // the starter kit plays into its own inserts: the kick's is a stem of the pattern
    const stems = await be.exportStems({ ...opts, pattern: pt.id, includeMaster: false });
    if (stems.cancelled) throw new Error('cancelled');
    expect(stems.files.map(f => f.name)).toContain('Kick');
  });
  it('refuses a wrapped tail or a pattern that is gone', async () => {
    const be = new MockBackend();
    await expect(be.exportAudio({ ...opts, pattern: 999_999, format: 'wav' })).rejects.toThrow(/no longer exists/);
    const pt = (await be.getState()).project.patterns[0];
    await expect(be.exportAudio({ ...opts, pattern: pt.id, format: 'wav', wrapTail: true })).rejects.toThrow(/wrap/);
  });
});

describe('stem export in the mock', () => {
  const opts = { useLoop: false, tail: 2, sampleRate: 48000, includeMaster: true };
  const done = (r: ExportStemsResult) => { if (r.cancelled) throw new Error('cancelled'); return r; };

  it('writes Master and one file per insert with sound, named after the insert, and reports progress', async () => {
    const be = new MockBackend();
    const seen: ExportProgress[] = [];
    await be.on('export-progress', p => seen.push(p));
    const r = done(await be.exportStems(opts));
    expect(r.files.map(f => f.file)).toEqual(['Master.wav', 'Drums.wav', 'Hats.wav', 'Bass.wav']);
    expect(r.files.map(f => f.master)).toEqual([true, false, false, false]);
    expect(new Set(r.files.map(f => f.stats.seconds)).size).toBe(1); // all the same length
    expect(r.format).toBe('wav');
    await new Promise(res => setTimeout(res, 5)); // the mock delivers events on a timer
    expect([...new Set(seen.map(p => p.done))]).toEqual([0, 1, 2, 3, 4]);
    expect(seen[0]).toMatchObject({ total: 4, name: 'Master', stage: 'prepare' });
    expect(seen.at(-1)).toMatchObject({ done: 4, total: 4, name: '', stage: 'done' });
    // like the engine: each file is prepared, rendered with a growing waveform, then written
    const bass = seen.filter(p => p.name === 'Bass');
    expect(bass.map(p => p.stage)).toEqual(['prepare', ...bass.slice(1, -1).map(() => 'render'), 'write']);
    const nonzero = bass.map(p => p.wave.filter(v => v > 0).length);
    expect(nonzero.every((n, i) => i === 0 || n >= nonzero[i - 1])).toBe(true);
    expect(bass.at(-1)!.wave).toHaveLength(240);
    expect(bass.at(-1)!.peak).toBeGreaterThan(0);
  });
  it('reports the progress of a mix export too', async () => {
    const be = new MockBackend();
    const seen: ExportProgress[] = [];
    await be.on('export-progress', p => seen.push(p));
    const { includeMaster: _, ...mix } = opts;
    const r = await be.exportAudio({ ...mix, format: 'wav', wavBits: 16, dither: true });
    expect(r.cancelled).toBeFalsy();
    await new Promise(res => setTimeout(res, 5));
    expect(seen[0]).toMatchObject({ done: 0, total: 1, name: 'Song.wav', stage: 'prepare' });
    const renders = seen.filter(p => p.stage === 'render');
    expect(renders.length).toBeGreaterThan(4);
    expect(renders.every((p, i) => i === 0 || p.beat >= renders[i - 1].beat)).toBe(true);
    expect(seen.at(-1)).toMatchObject({ done: 1, total: 1, stage: 'done' });
  });
  it('uses MP3 for every file including the master', async () => {
    const be = new MockBackend();
    const r = done(await be.exportStems({ ...opts, format: 'mp3', mp3Kbps: 192 }));
    expect(r.format).toBe('mp3');
    expect(r.files.map(f => f.file)).toEqual(['Master.mp3', 'Drums.mp3', 'Hats.mp3', 'Bass.mp3']);
  });
  it('can leave the master out', async () => {
    const be = new MockBackend();
    const r = done(await be.exportStems({ ...opts, includeMaster: false }));
    expect(r.files.map(f => f.file)).toEqual(['Drums.wav', 'Hats.wav', 'Bass.wav']);
  });
  it('refuses when nothing has sound, instead of writing an empty folder', async () => {
    const be = new MockBackend();
    const tracks = (await be.getState()).project.tracks;
    await be.applyOps(tracks.map(t => ({ op: 'putTrack' as const, track: { ...t, muted: true } })));
    await expect(be.exportStems({ ...opts, includeMaster: false })).rejects.toThrow(/nothing to export/);
  });
  it('stops on cancel without a result', async () => {
    const be = new MockBackend();
    const run = be.exportStems(opts);
    await be.exportCancel();
    expect(await run).toEqual({ cancelled: true });
  });
});

// the same cases as crates/engine/tests/make_unique.rs
describe('make unique (audio) in the mock', () => {
  async function setup() {
    const be = new MockBackend();
    await be.projectNew();
    const r = await be.importAudio({ paths: ['C:\\loops\\guitar.wav'], track: 0, beat: 0 });
    const a = (await be.getState()).project.clips.find(c => c.id === r.clipIds[0])!;
    await be.applyOps([{ op: 'putClips', clips: [{ ...a, id: 70001, start: 4 }, { ...a, id: 70002, track: 1 }] }]);
    return { be, src: r.sourceIds[0], a: a.id };
  }
  const clip = async (be: MockBackend, id: number) => (await be.getState()).project.clips.find(c => c.id === id)!;

  it('gives the clip its own source on a new track with the old routing, in one undo step', async () => {
    const { be, src, a } = await setup();
    const before = structuredClone((await be.getState()).project);
    const r = await be.makeUnique([a]);
    const p = (await be.getState()).project;
    expect(p.sources.find(s => s.id === r.sourceId)).toMatchObject({ name: 'guitar #2', path: p.sources.find(s => s.id === src)!.path });
    expect(r.track).toBe(1);
    expect(p.tracks).toHaveLength(before.tracks.length + 1);
    expect(p.inserts).toEqual(before.inserts);
    expect(p.tracks[1]).toMatchObject({ name: 'guitar #2', insert: 0 });
    expect(r.insertId).toBe(0);
    expect(await clip(be, a)).toMatchObject({ track: 1, ref: r.sourceId });
    expect(await clip(be, 70001)).toMatchObject({ track: 0, ref: src });
    expect(await clip(be, 70002)).toMatchObject({ track: 2, ref: src });
    await be.undo();
    expect((await be.getState()).project).toEqual(before);
  });
  it('moves several clips together and counts names up', async () => {
    const { be, a } = await setup();
    const r = await be.makeUnique([a, 70001]);
    const r2 = await be.makeUnique([70002]);
    const r3 = await be.makeUnique([a]);
    const name = async (id: number) => (await be.getState()).project.sources.find(s => s.id === id)!.name;
    expect([await name(r.sourceId), await name(r2.sourceId), await name(r3.sourceId)]).toEqual(['guitar #2', 'guitar #3', 'guitar #4']);
    expect([r.insertId, r2.insertId, r3.insertId]).toEqual([0, 0, 0]);
  });
  it('keeps a routing the user picked', async () => {
    const { be, a } = await setup();
    const p0 = (await be.getState()).project;
    await be.applyOps([{ op: 'putTrack', track: { ...p0.tracks[0], insert: p0.inserts[3].id } }]);
    const r = await be.makeUnique([a]);
    const p = (await be.getState()).project;
    expect(r.insertId).toBe(p0.inserts[3].id);
    expect(p.tracks[1].insert).toBe(p0.inserts[3].id);
    expect(p.inserts).toHaveLength(p0.inserts.length);
  });
  it('refuses what it cannot do and changes nothing', async () => {
    const { be, a } = await setup();
    const before = structuredClone((await be.getState()).project);
    await expect(be.makeUnique([])).rejects.toThrow();
    await expect(be.makeUnique([999999])).rejects.toThrow();
    await expect(be.makeUnique([a, 70002])).rejects.toThrow(/same track/);
    expect((await be.getState()).project).toEqual(before);
  });
});

describe('routing channels to inserts in the mock (like the engine)', () => {
  it('creates inserts and routes channels to them in one undo step', async () => {
    const be = new MockBackend();
    const p0 = (await be.projectNew()).project;
    const before = structuredClone(p0);
    const [kick, clap] = p0.channels!;
    const last = p0.inserts[p0.inserts.length - 1];
    const fresh = { id: 90001, name: 'Clap', color: '#8b93a1', vol: 0.7937, pan: 0, muted: false, solo: false, fx: [] };
    await be.applyOps([
      { op: 'putInsert', insert: fresh },
      { op: 'putChannel', channel: { ...kick, insert: last.id } },
      { op: 'putChannel', channel: { ...clap, insert: fresh.id } },
    ], { label: 'Route channels to inserts' });
    const p = (await be.getState()).project;
    expect(p.inserts[p.inserts.length - 1]).toMatchObject({ id: 90001, name: 'Clap' });
    expect(p.channels!.find(c => c.id === kick.id)!.insert).toBe(last.id);
    expect(p.channels!.find(c => c.id === clap.id)!.insert).toBe(90001);
    await be.undo();
    expect((await be.getState()).project).toEqual(before);
  });
  it('refuses a channel routed to an insert that does not exist, changing nothing', async () => {
    const be = new MockBackend();
    const p0 = (await be.projectNew()).project;
    const before = structuredClone(p0);
    await expect(be.applyOps([{ op: 'putChannel', channel: { ...p0.channels![0], insert: 424242 } }])).rejects.toThrow(/insert/);
    expect((await be.getState()).project).toEqual(before);
  });
});

describe('pattern loop in the mock (like the engine)', () => {
  it('starts where asked, loops the range, and keeps its place when the range changes', async () => {
    const be = new MockBackend();
    const p = (await be.getState()).project, pt = p.patterns.find(x => x.len >= 4)!, spb = 60 / p.bpm;
    let now = 1000;
    const clock = vi.spyOn(performance, 'now').mockImplementation(() => now);
    const pos = () => (be as unknown as { patPos(len: number, spb: number): number }).patPos(pt.len, spb);
    try {
      await be.patternPlay(pt.id, { from: 2, range: { a: 1, b: 3 } });
      expect(pos()).toBe(2);
      now += 1000 * spb * 1.5; // 1.5 beats: past the range's end, wrapped
      expect(pos()).toBeCloseTo(1.5);
      await be.patternLoop({ a: 0, b: 1 }); // outside the new range: its start
      expect(pos()).toBe(0);
      now += 1000 * spb * 0.25;
      await be.patternLoop(null); // inside the whole pattern: stays
      expect(pos()).toBeCloseTo(0.25);
      await expect(be.patternPlay(pt.id, { from: NaN })).rejects.toThrow();
      await expect(be.patternLoop({ a: 0, b: Infinity })).rejects.toThrow();
      await expect(be.patternPlay(424242)).rejects.toThrow();
    } finally { clock.mockRestore(); }
  });
});

describe('recording in the mock (like the engine)', () => {
  const recordOneSecond = async (be: MockBackend) => {
    await be.transportSet({ rec: true, countIn: false });
    await be.transportPlay(0);
    await new Promise(r => setTimeout(r, 300));
    await be.transportStop();
  };
  it('records to the first free track without arming, onto a new track when none has room, and undo removes both', async () => {
    {
      const be = new MockBackend();
      await be.subscribeTelemetry(() => {});
      const p0 = (await be.getState()).project;
      // fill every track where the take goes
      const busy = p0.tracks.map((_, i) => ({ id: 90000 + i, track: i, start: 0, len: 64, offset: 0, kind: 'pat' as const, ref: p0.patterns[0].id, muted: false, gain: 0, fadeIn: 0, fadeOut: 0, pan: 0, norm: false, rev: false }));
      for (const t of p0.tracks) if (t.arm) await be.applyOps([{ op: 'putTrack', track: { ...t, arm: false } }], { label: 'disarm' });
      await be.applyOps([{ op: 'putClips', clips: busy }], { label: 'fill' });
      const n = (await be.getState()).project.tracks.length;
      await recordOneSecond(be);
      const p = (await be.getState()).project;
      expect(p.tracks).toHaveLength(n + 1);
      expect(p.clips.find(c => c.kind === 'audio')?.track).toBe(n);
      await be.undo();
      const u = (await be.getState()).project;
      expect(u.tracks).toHaveLength(n);
      expect(u.clips.some(c => c.kind === 'audio')).toBe(false);
    }
  });
  it('records nothing with Audio unticked and keeps one kind ticked', async () => {
    const be = new MockBackend();
    expect(await be.transportSet({ recAudio: false })).toMatchObject({ recAudio: false, recNotes: true });
    expect(await be.transportSet({ recNotes: false })).toMatchObject({ recAudio: false, recNotes: true });
  });
});

describe('lane heights are view state (engine.rs only_track_heights)', () => {
  it('a vertical zoom or a lane resize adds no undo step and survives undoing an older edit', async () => {
    const be = new MockBackend();
    await be.projectNew(true);
    const p0 = (await be.getState()).project, t1 = p0.tracks[1];
    await be.applyOps([{ op: 'putTrack', track: { ...t1, muted: !t1.muted } }]);
    await be.applyOps(p0.tracks.map(t => ({ op: 'putTrack' as const, track: { ...(t.id === t1.id ? { ...t, muted: !t1.muted } : t), h: t.h + 12 } })));
    await be.beginGesture();
    const t0 = (await be.getState()).project.tracks[0];
    await be.applyOps([{ op: 'putTrack', track: { ...t0, h: 140 } }], { transient: true });
    await be.endGesture();
    let s = await be.getState();
    expect(s.project.tracks[0].h).toBe(140);
    expect(s.project.tracks[2].h).toBe(p0.tracks[2].h + 12);
    // one undo: the mute goes back, the lanes stay
    await be.undo();
    s = await be.getState();
    expect(s.project.tracks[1].muted).toBe(t1.muted);
    expect(s.project.tracks[0].h).toBe(140);
    expect(s.project.tracks[2].h).toBe(p0.tracks[2].h + 12);
    expect(s.canUndo).toBe(false);
    await be.redo();
    s = await be.getState();
    expect(s.project.tracks[1].muted).toBe(!t1.muted);
    expect(s.project.tracks[0].h).toBe(140);
  });
  it('a height change together with another change is an ordinary edit', async () => {
    const be = new MockBackend();
    await be.projectNew(true);
    const t0 = (await be.getState()).project.tracks[0];
    await be.applyOps([{ op: 'putTrack', track: { ...t0, h: t0.h + 20, name: 'Renamed' } }]);
    expect((await be.getState()).canUndo).toBe(true);
    await be.undo();
    expect((await be.getState()).project.tracks[0]).toMatchObject({ h: t0.h, name: t0.name });
  });
});

describe('import as an undo step (engine import_audio_with)', () => {
  it('into the audio list it is one step; loaded for an edit that follows it is none, and redo is dropped', async () => {
    const be = new MockBackend();
    await be.projectNew();
    const n = (await be.getState()).project.sources.length;
    await be.importAudio({ paths: ['C:\\loops\\a.wav'] });
    expect((await be.getState()).canUndo).toBe(true);
    await be.undo();
    expect((await be.getState()).project.sources).toHaveLength(n);
    expect((await be.getState()).canRedo).toBe(true);
    // for a sampler channel: no step of its own, and the stale redo (a project without the source) goes
    await be.importAudio({ paths: ['C:\\loops\\b.wav'], step: false });
    const s = await be.getState();
    expect(s.project.sources).toHaveLength(n + 1);
    expect(s.canUndo).toBe(false);
    expect(s.canRedo).toBe(false);    // the channel made from it is the step, and undoing it takes the file out too
    const src = s.project.sources.at(-1)!.id;
    const ch = { ...s.project.channels![0], id: s.project.nextId + 5, name: 'Drop', source: src, insert: null };
    await be.applyOps([{ op: 'putChannel', channel: ch }]);
    await be.undo();
    const u = await be.getState();
    expect(u.project.sources).toHaveLength(n);
    expect(u.project.channels!.some(c => c.name === 'Drop')).toBe(false);
    expect(u.canUndo).toBe(false);
  });
  it('a lane-height gesture reports its state when it ends, though it adds no undo step', async () => {
    const be = new MockBackend();
    await be.projectNew(true);
    const states: number[] = [];
    await be.on('state', s => states.push(s.revision));
    const t0 = (await be.getState()).project.tracks[0];
    await be.beginGesture();
    await be.applyOps([{ op: 'putTrack', track: { ...t0, h: t0.h + 30 } }], { transient: true });
    await be.endGesture();
    await new Promise(r => setTimeout(r, 5));
    expect(states.length).toBeGreaterThan(0);
    expect((await be.getState()).canUndo).toBe(false);
  });
});
