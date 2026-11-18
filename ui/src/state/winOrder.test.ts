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
    const z2 = U.zTop + 1; setU('zTop', z2); setU('plugs', 0, 'z', z2); A.playlistFront(); A.editorFront();
    expect(outerZ(U.editor!.z)).toBeGreaterThan(45);
  });
});

describe('one stack for every floating window', () => {
  it('the window used last is in front, whatever its kind; F5 tucks them all away', async () => {
    const { U, setU } = await import('../state/ui');
    const A = await import('../state/actions');
    const { topFloating, shownFloating, anyTucked } = await import('../state/winOrder');
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
    // the editor comes back on its own: the effect window stays behind the playlist
    A.openSampleEditor({ kind: 'clip', id: 1 });
    expect(shownFloating().map(f => f.kind)).toEqual(['editor']);
    expect(anyTucked()).toBe(true);
    // F5 then shows the tucked ones
    A.showPlugs();
    expect(shownFloating().map(f => f.kind).sort()).toEqual(['editor', 'plug']);
    expect(anyTucked()).toBe(false);
  });

  it('a window opened or raised while the others are tucked shows alone', async () => {
    const { U, setU } = await import('../state/ui');
    const A = await import('../state/actions');
    const P = await import('../state/pattern');
    const { shownFloating, anyTucked } = await import('../state/winOrder');
    setU({ zTop: 3, plugs: [{ fxId: 7, x: 0, y: 0, w: 400, z: 2 }, { fxId: 8, x: 0, y: 0, w: 400, z: 3 }], editor: null, pitchEd: null, plugsHidden: false, tuckZ: 0,
      rackWin: { x: 0, y: 0, w: 600, h: 300, z: 1 }, rollWin: null, mixView: false, dock: { left: [], right: [], bottom: [] } });
    A.playlistFront();
    expect(shownFloating()).toEqual([]);
    // the top window, already at the top of the stack, still comes back when raised
    A.plugFront(8);
    expect(shownFloating()).toMatchObject([{ kind: 'plug', id: 8 }]);
    A.playlistFront();
    P.rackFront();
    expect(shownFloating().map(f => f.kind)).toEqual(['rack']);
    expect(anyTucked()).toBe(true);
    // a click that opened a window: only what was there before it is tucked
    const before = U.zTop;
    A.plugFront(7);
    A.tuckBelow(before);
    expect(shownFloating()).toMatchObject([{ kind: 'plug', id: 7 }]);
  });
});

describe('F5 then F6 (#125)', () => {
  it('brings back the Channel Rack alone, not the maximised Piano roll over the Browser; F7 brings the roll back as it was', async () => {
    const { U, setU } = await import('../state/ui');
    const A = await import('../state/actions');
    const P = await import('../state/pattern');
    const { setS } = await import('../state/store');
    const { emptyProject } = await import('../lib/factory');
    setS('project', emptyProject());
    setU({ zTop: 1, plugs: [], editor: null, pitchEd: null, plugsHidden: false, tuckZ: 0, rollMax: false, rollFloat: null,
      rackWin: { x: 0, y: 0, w: 600, h: 300, z: 1 }, rollWin: null, mixView: false, dock: { left: [], right: [], bottom: [] } });
    const W = await import('../state/winOrder');
    const shownFloating = () => W.shownFloating().filter(f => f.kind === 'rack' || f.kind === 'roll');
    P.toggleRoll(); // F7: the Piano roll, full screen
    expect(U.rollMax).toBe(true);
    expect(shownFloating().map(f => f.kind).sort()).toEqual(['rack', 'roll']);
    A.playlistFront(); // F5
    expect(shownFloating()).toEqual([]);
    P.toggleRack(); // F6
    expect(shownFloating().map(f => f.kind)).toEqual(['rack']);
    P.toggleRoll(); // F7
    expect([...shownFloating()].sort((a, b) => b.z - a.z)[0]).toMatchObject({ kind: 'roll' }); // in front
    expect(U.rollMax).toBe(true);
  });
});

describe('the Mixer is a window like the others', () => {
  it('F9 shows, brings to front and hides it; floating, it is in the stack', async () => {
    const { U, setU } = await import('../state/ui');
    const MV = await import('../state/mixView');
    const { topFloating } = await import('../state/winOrder');
    setU({ zTop: 1, plugs: [], editor: null, pitchEd: null, plugsHidden: false, rackWin: null, rollWin: null, mixView: false, dock: { left: [], right: [], bottom: [[{ id: 'mix', share: 1 }]] }, mixMin: false });
    // docked: F9 hides it to its handle and shows it again; it is never "in front"
    MV.toggleMixer(); expect(U.mixMin).toBe(true);
    MV.toggleMixer(); expect(U.mixMin).toBe(false);
    expect(topFloating()).toBeNull();
    // floating: it joins the stack in front
    const D = await import('../state/docking');
    D.floatPane('mix');
    expect(topFloating()?.kind).toBe('mix');
    // another window comes in front: F9 brings the Mixer back to the front instead of hiding it
    setU({ zTop: U.zTop + 1, plugs: [{ fxId: 3, x: 0, y: 0, w: 300, z: U.zTop + 1 }] });
    expect(topFloating()?.kind).toBe('plug');
    MV.toggleMixer(); expect(U.mixMin).toBe(false); expect(topFloating()?.kind).toBe('mix');
    MV.toggleMixer(); expect(U.mixMin).toBe(true); expect(topFloating()?.kind).toBe('plug');
    // maximized: F9 restores it
    MV.showMixer(); MV.toggleMixView(); expect(U.mixView).toBe(true);
    MV.toggleMixer(); expect(U.mixView).toBe(false); expect(U.mixMin).toBe(false);
  });
});
