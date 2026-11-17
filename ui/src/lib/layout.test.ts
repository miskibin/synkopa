import { describe, expect, it } from 'vitest';
import { dockKeyAction, fitMixH, MIX_COLLAPSE_AT, MIX_MIN_H, MIX_OPEN_DRAG, mixHandleDrag, mixSplitDrag, parseGeo, parseLayout } from './layout';
import { defaultHomes, defaultSizes } from './dock';

describe('parseLayout', () => {
  it('translates the docked panels of a layout saved before #90 and keeps the floating places', () => {
    const p = parseLayout(JSON.stringify({
      rackDock: true, rackDockW: 700, rollDock: true, rollDockH: 360, rollMax: false, mixH: 280,
      rackFloat: { x: 40, y: 80, w: 900, h: 400 }, rollFloat: { x: 0, y: 54, w: 1280, h: 600 },
    }));
    expect(p).toEqual({
      // the rack left, the roll at the bottom above the mixer, as they were
      dock: { left: [[{ id: 'rack', share: 1 }]], right: [], bottom: [[{ id: 'roll', share: 1 }], [{ id: 'mix', share: 1 }]] },
      paneSize: { rack: { w: 700, h: defaultSizes().rack.h }, roll: { w: defaultSizes().roll.w, h: 360 }, mix: { w: defaultSizes().mix.w, h: 280 } },
      rollMax: false, rackFloat: { x: 40, y: 80, w: 900, h: 400 }, rollFloat: { x: 0, y: 54, w: 1280, h: 600 },
    });
    // the floating mixer of an old layout stays floating
    expect(parseLayout(JSON.stringify({ mixDock: false })).dock).toEqual({ left: [], right: [], bottom: [] });
  });
  it('keeps a saved dock layout, sizes and homes', () => {
    const dock = { left: [], right: [[{ id: 'roll', share: 2 }, { id: 'rack', share: 1 }]], bottom: [[{ id: 'mix', share: 1 }]] };
    const p = parseLayout(JSON.stringify({ dock, paneSize: { roll: { w: 700, h: 300 } }, dockHome: { rack: { edge: 'right', outer: true } } }));
    expect(p.dock).toEqual(dock);
    expect(p.paneSize!.roll).toEqual({ w: 700, h: 300 });
    expect(p.paneSize!.mix).toEqual(defaultSizes().mix);
    expect(p.dockHome).toEqual({ ...defaultHomes(), rack: { edge: 'right', outer: true } });
  });
  it('drops bad values and keeps the good ones', () => {
    const p = parseLayout(JSON.stringify({
      rollMax: null, rackFloat: { x: 'a', y: 0, w: 500, h: 300 }, rollFloat: [1, 2, 3, 4], headW: 300,
    }));
    expect(p).toEqual({ headW: 300 });
    // a broken dock layout falls back to the default one (the store's)
    expect(parseLayout(JSON.stringify({ dock: 'left' }))).toEqual({ paneSize: defaultSizes(), dockHome: defaultHomes() });
  });
  it('old sizes that are not numbers fall back to the defaults', () => {
    expect(parseLayout('{"rackDockW":"700","rollDockH":"Infinity"}').paneSize).toEqual(defaultSizes());
    // 0 = the docked rack fits its pattern; small non-zero widths are rejected
    expect(parseLayout('{"rackDockW":0}').paneSize!.rack.w).toBe(0);
    expect(parseLayout('{"rackDockW":50}').paneSize!.rack.w).toBe(defaultSizes().rack.w);
  });
  it('survives junk', () => {
    expect(parseLayout(null)).toEqual({});
    expect(parseLayout('not json')).toEqual({});
    expect(parseLayout('[1,2]')).toEqual({});
    expect(parseLayout('42')).toEqual({});
    expect(parseLayout('null')).toEqual({});
  });
  it('keeps the older fields', () => {
    expect(parseLayout(JSON.stringify({ tool: 'paint', pickOpen: false, browserTab: 'library' }))).toEqual({ tool: 'paint', pickOpen: false, browserTab: 'library' });
  });
  it('opens the old Project browser tab as Patterns', () => {
    expect(parseLayout(JSON.stringify({ browserTab: 'project' })).browserTab).toBe('patterns');
    expect(parseLayout(JSON.stringify({ browserTab: 'audio' })).browserTab).toBe('audio');
    expect(parseLayout(JSON.stringify({ browserTab: 'nope' })).browserTab).toBeUndefined();
  });
  it('keeps the closed Library sections and forgets junk', () => {
    expect(parseLayout(JSON.stringify({ browserSec: { std: true } })).browserSec).toEqual({ std: true, kits: undefined, folders: undefined });
    expect(parseLayout(JSON.stringify({ browserSec: { std: 'yes', kits: true, folders: false } })).browserSec).toEqual({ std: undefined, kits: true, folders: false });
    expect(parseLayout(JSON.stringify({ browserSec: 'closed' })).browserSec).toBeUndefined();
    expect(parseLayout(null).browserSec).toBeUndefined();
  });
  it('keeps a deep playlist zoom from before the ceiling rose (#117)', () => {
    expect(parseLayout(JSON.stringify({ bw: 384 })).bw).toBe(384);
    expect(parseLayout(JSON.stringify({ bw: 96 })).bw).toBe(96);
    expect(parseLayout(JSON.stringify({ bw: 1024 })).bw).toBe(1024);
    expect(parseLayout(JSON.stringify({ bw: 1100 })).bw).toBeUndefined(); // past the ceiling: not trusted
    expect(parseLayout(JSON.stringify({ bw: 2 })).bw).toBeUndefined();
  });
});

describe('parseGeo', () => {
  it('needs all four finite numbers in range', () => {
    expect(parseGeo({ x: 10, y: 20, w: 300, h: 200 })).toEqual({ x: 10, y: 20, w: 300, h: 200 });
    expect(parseGeo({ x: 10, y: 20, w: 300 })).toBeUndefined();
    expect(parseGeo({ x: 10, y: 20, w: 50, h: 200 })).toBeUndefined(); // too small to be a window
    expect(parseGeo(null)).toBeUndefined();
    expect(parseGeo('x')).toBeUndefined();
  });
});

describe('dockKeyAction (F6 / F7 on a docked panel)', () => {
  it('shows a hidden panel', () => {
    expect(dockKeyAction(false, false)).toBe('show');
    expect(dockKeyAction(false, true)).toBe('show');
  });
  it('gives a shown panel the keyboard first', () => {
    expect(dockKeyAction(true, false)).toBe('focus');
  });
  it('hides the panel that has the keyboard', () => {
    expect(dockKeyAction(true, true)).toBe('hide');
  });
});

describe('docked mixer height and collapse', () => {
  it('migrates a saved closed or minimised mixer to the collapsed handle', () => {
    expect(parseLayout(JSON.stringify({ mixOpen: false, mixMin: false, mixH: 280 }))).toMatchObject({ mixMin: true, paneSize: { mix: { h: 280 } } });
    expect(parseLayout(JSON.stringify({ mixOpen: true, mixMin: true }))).toEqual({ mixMin: true });
    expect(parseLayout(JSON.stringify({ mixOpen: true, mixMin: false }))).toEqual({ mixMin: false });
    expect(parseLayout(JSON.stringify({ mixMin: 'no' }))).toEqual({});
    // mixOpen itself is gone: the docked mixer is always there
    expect(parseLayout(JSON.stringify({ mixOpen: true }))).toEqual({});
  });
  it('keeps the mixer between its minimum and what leaves the Playlist room', () => {
    expect(fitMixH(400.4, 880)).toBe(400);
    expect(fitMixH(50, 880)).toBe(MIX_MIN_H);
    expect(fitMixH(2000, 880)).toBe(880);
    expect(fitMixH(100, 120)).toBe(MIX_MIN_H); // a tiny window: never below the minimum
    // docked on a side edge its minimum is a width
    expect(fitMixH(200, 880, 360)).toBe(360);
  });
  it('collapses when the splitter goes below the breakpoint, keeping the height it had', () => {
    expect(mixSplitDrag(300, 100, 880)).toEqual({ min: false, h: 200 });
    // between the breakpoint and the minimum it stays at the minimum
    expect(mixSplitDrag(300, 300 - MIX_COLLAPSE_AT, 880)).toEqual({ min: false, h: MIX_MIN_H });
    expect(mixSplitDrag(300, 300 - MIX_COLLAPSE_AT + 1, 880)).toEqual({ min: true, h: 300 });
    expect(mixSplitDrag(300, 290, 880)).toEqual({ min: true, h: 300 });
    // dragged back up in the same drag it opens again
    expect(mixSplitDrag(300, -50, 880)).toEqual({ min: false, h: 350 });
    expect(mixSplitDrag(300, -2000, 880)).toEqual({ min: false, h: 880 });
  });
  it('opens the collapsed handle once it is dragged up far enough, following the pointer', () => {
    expect(mixHandleDrag(320, -10, 880)).toEqual({ min: true, h: 320 });
    expect(mixHandleDrag(320, -MIX_OPEN_DRAG, 880)).toEqual({ min: false, h: MIX_MIN_H });
    expect(mixHandleDrag(320, -400, 880)).toEqual({ min: false, h: 400 });
    expect(mixHandleDrag(320, 30, 880)).toEqual({ min: true, h: 320 });
  });
});
