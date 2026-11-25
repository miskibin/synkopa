import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';

// Edits are named (a drag's label, a script's `--label`) and the Edit menu shows the name of
// the step Ctrl+Z would take back — engine.rs `UndoStep`, `StateDto.undoLabel`.
beforeAll(() => {
  vi.stubGlobal('window', {});
  const noop = () => {};
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
});

let mock: MockBackend;
afterEach(() => mock.dispose());

const setVol = (backend: MockBackend, vol: number) =>
  backend.getState().then(({ project }) => backend.applyOps([{ op: 'putTrack', track: { ...project.tracks[0], vol } }], { label: 'Set volume' }));

describe('undo labels', () => {
  it('the menu sees what each step was called, in both directions', async () => {
    mock = new MockBackend();
    await mock.beginGesture('Move clips');
    await setVol(mock, 0.3);
    await mock.endGesture();
    expect((await mock.getState()).undoLabel).toBe('Move clips');
    await setVol(mock, 0.4);
    expect((await mock.getState()).undoLabel).toBe('Set volume');
    await mock.undo();
    expect((await mock.getState()).undoLabel).toBe('Move clips') /* undoing reveals the step below */;
    expect((await mock.getState()).redoLabel).toBe('Set volume');
    await mock.redo();
    expect((await mock.getState()).undoLabel).toBe('Set volume');
    expect((await mock.getState()).redoLabel).toBeNull();
  });
  it('the chords-to-pattern and the MIDI import name their own steps', async () => {
    mock = new MockBackend();
    await mock.projectNew();
    const r = await mock.importAudio({ paths: ['C:/loops/song.wav'], track: 0, beat: 4 });
    const c = (await mock.getState()).project.clips.find(x => x.id === r.clipIds[0])!;
    await mock.applyOps([{ op: 'putClips', clips: [{ ...c, offset: 0, len: 32 }] }], { label: 'Extend clip' });
    await mock.chordsModelInstall();
    await mock.chordsToPattern({ clipId: c.id });
    expect((await mock.getState()).undoLabel).toBe('Send chords to piano roll');
    const m = await mock.midiImport({ path: 'C:/midi/song.mid', parts: [0, 1, 2] });
    expect(m.revision).toBeGreaterThan(0);
    expect((await mock.getState()).undoLabel).toBe('Import MIDI');
  });
});
