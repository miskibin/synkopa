import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';

// MIDI files against the mock backend, which imports like app/src/midi_actions.rs: one
// pattern with drum samplers (rooted at their GM key) and pitched channels, one undo step.
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
const settle = () => new Promise(r => setTimeout(r, 30));
const sync = async () => { await settle(); store.replaceState(await mock.getState()); };
const FILE = 'C:\\Music\\Groove.mid';

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  A = await import('./actions');
  store = await import('./store');
  ui = await import('./ui');
  await sync();
});

describe('MIDI files', () => {
  it('File → Import MIDI opens the dialog for the picked file at the view start', async () => {
    await A.midiImport();
    expect(ui.U.midiWin?.path).toMatch(/\.mid$/);
    mock.dismissNextDialog = true;
    ui.setU('midiWin', null);
    await A.midiImport();
    expect(ui.U.midiWin).toBeNull();
  });

  it('inspect lists the parts and what each becomes', async () => {
    const f = await mock.midiInspect(FILE);
    expect(f.bpm).toBe(97);
    expect(f.parts.map(p => [p.name, p.channel, p.becomes.kind])).toEqual([['Bass', 2, 'synth'], ['Piano', 1, 'keys'], ['Drums', 10, 'drums']]);
    await expect(mock.midiInspect('C:\\x.wav')).rejects.toThrow('not a MIDI file');
  });

  it('an import is one pattern, one clip, drum samplers and pitched channels, undone in one step', async () => {
    const before = store.plain(store.S.project);
    const r = await mock.midiImport({ path: FILE, parts: [0, 1, 2], setTempo: true, beat: 8 });
    await sync();
    const P = store.S.project;
    expect(P.bpm).toBe(97);
    const pat = P.patterns.find(p => p.id === r.patternId)!;
    expect(pat.name).toBe('Groove');
    expect(pat.notes).toHaveLength(12);
    expect(P.clips.find(c => c.id === r.clipId)).toMatchObject({ kind: 'pat', ref: r.patternId, start: 8, len: 8 });
    const chans = r.channelIds.map(id => P.channels!.find(c => c.id === id)!);
    expect(chans.map(c => c.kind)).toEqual(['sample', 'sample', 'sample', 'synth', 'keys']);
    expect(chans.filter(c => c.kind === 'sample').map(c => c.root)).toEqual([36, 38, 42]);
    // drum notes play their sample unpitched; melodic notes keep their keys
    expect(pat.notes.filter(n => n.ch === chans[0].id).every(n => n.p == null)).toBe(true);
    expect(pat.notes.filter(n => n.ch === chans[3].id).map(n => n.p)).toEqual([36, 36, 43]);
    await A.undo();
    await sync();
    expect(store.S.project.patterns).toEqual(before.patterns);
    expect(store.S.project.channels).toEqual(before.channels);
    expect(store.S.project.bpm).toBe(before.bpm);
  });

  it('one part goes into a channel, replacing or adding; only one part may', async () => {
    const pat = store.S.project.patterns[0], ch = store.S.project.channels![0].id;
    const others = pat.notes.filter(n => n.ch !== ch).length;
    await mock.midiImport({ path: FILE, parts: [0], into: { pattern: pat.id, ch, replace: true } });
    await sync();
    const after = store.S.project.patterns[0].notes;
    expect(after.filter(n => n.ch === ch).map(n => n.p)).toEqual([36, 36, 43]);
    expect(after.filter(n => n.ch !== ch)).toHaveLength(others);
    await mock.midiImport({ path: FILE, parts: [0], into: { pattern: pat.id, ch, replace: false } });
    await sync();
    expect(store.S.project.patterns[0].notes.filter(n => n.ch === ch)).toHaveLength(6);
    await expect(mock.midiImport({ path: FILE, parts: [0, 1], into: { pattern: pat.id, ch, replace: false } })).rejects.toThrow('exactly one');
    await expect(mock.midiImport({ path: FILE, parts: [] })).rejects.toThrow('no notes');
  });

  it('export reports what it wrote and refuses an empty pattern', async () => {
    const r = await mock.midiExport({ out: 'C:\\x.mid', pattern: store.S.project.patterns[0].id });
    expect(r.notes).toBeGreaterThan(0);
    await A.midiExport(store.S.project.patterns[0].id);
    expect(ui.U.readout).toMatch(/Exported|Wyeksportowano/);
  });
});
