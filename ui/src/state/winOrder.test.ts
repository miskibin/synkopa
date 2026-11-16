import { beforeAll, describe, expect, it, vi } from 'vitest';

// The sample editor / pitch editor float outside the effect-window layer (z-index 45): clicking a
// plugin must bring the plugin over them, and clicking them back must lift them over the plugin.
vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
  vi.stubGlobal('document', { querySelector: () => null, activeElement: null });
  vi.stubGlobal('requestAnimationFrame', () => 0);
});

describe('floating editor stacking', () => {
  it('follows the last clicked window, plugin or editor', async () => {
    const { U, setU } = await import('../state/ui');
    const A = await import('../state/actions');
    const { outerZ } = await import('../state/winOrder');
    setU({ zTop: 1, plugs: [{ fxId: 7, x: 0, y: 0, w: 400, z: 1 }] });
    A.openSampleEditor({ kind: 'clip', id: 1 });
    expect(outerZ(U.editor!.z)).toBeGreaterThan(45); // just opened: over the plugin
    // the plugin window is clicked
    const z = U.zTop + 1; setU('zTop', z); setU('plugs', 0, 'z', z);
    expect(outerZ(U.editor!.z)).toBeLessThan(45);
    A.editorFront();
    expect(outerZ(U.editor!.z)).toBeGreaterThan(45);
    // with the pitch editor also over the layer, the one used last is on top
    setU({ zTop: U.zTop + 1, pitchEd: { clipId: 1, x: 0, y: 0, w: 800, h: 400, z: U.zTop + 1 } });
    expect(outerZ(U.pitchEd!.z)).toBeGreaterThan(outerZ(U.editor!.z));
    A.editorFront();
    expect(outerZ(U.editor!.z)).toBeGreaterThan(outerZ(U.pitchEd!.z));
    // effect windows tucked away (F5): nothing to sit under
    const z2 = U.zTop + 1; setU('zTop', z2); setU('plugs', 0, 'z', z2); setU('plugsHidden', true);
    expect(outerZ(U.editor!.z)).toBeGreaterThan(45);
  });
});

describe('one stack for every floating window', () => {
  it('the window used last is in front, whatever its kind; F5 tucks them all away', async () => {
    const { U, setU } = await import('../state/ui');
    const A = await import('../state/actions');
    const { topFloating, shownFloating } = await import('../state/winOrder');
    setU({ zTop: 1, plugs: [], editor: null, pitchEd: null, plugsHidden: false, rackWin: null, rollWin: null });
    expect(topFloating()).toBeNull();
    setU({ zTop: 2, plugs: [{ fxId: 7, x: 0, y: 0, w: 400, z: 2 }] });
    A.openSampleEditor({ kind: 'clip', id: 1 });
    expect(topFloating()?.kind).toBe('editor'); // just opened
    const z = U.zTop + 1; setU('zTop', z); setU('plugs', 0, 'z', z); // the effect window is clicked
    expect(topFloating()).toMatchObject({ kind: 'plug', id: 7 });
    A.editorFront();
    expect(topFloating()?.kind).toBe('editor');
    // F5: the editor steps aside with the effect windows, and opening one brings them back
    A.playlistFront();
    expect(U.plugsHidden).toBe(true);
    expect(shownFloating()).toEqual([]);
    A.openPitchEditor(-1); // not an audio clip: nothing opens, nothing comes back
    expect(U.plugsHidden).toBe(true);
    A.openSampleEditor({ kind: 'clip', id: 1 });
    expect(U.plugsHidden).toBe(false);
    expect(shownFloating().map(f => f.kind).sort()).toEqual(['editor', 'plug']);
  });
});
