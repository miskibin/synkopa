import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';

// Edits are named (a drag's label, a script's `--label`) and the Edit menu shows the name of
// the step Ctrl+Z would take back — engine.rs `UndoStep`, `StateDto.undoLabel`.
beforeAll(() => {
  vi.stubGlobal('window', {});
  const noop = () => {};
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
});

const setVol = (mock: MockBackend, vol: number) =>
  mock.getState().then(({ project }) => mock.applyOps([{ op: 'putTrack', track: { ...project.tracks[0], vol } }], { label: 'Set volume' }));

describe('undo labels', () => {
  it('the menu sees what each step was called, in both directions', async () => {
    const mock = new MockBackend();
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
});
