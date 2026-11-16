import { describe, expect, it } from 'vitest';
import { dockKeyAction, fitDock, fitMixH, MIX_COLLAPSE_AT, MIX_MIN_H, MIX_OPEN_DRAG, mixHandleDrag, mixMaxH, mixSplitDrag, MIXER_BAR_H, parseGeo, parseLayout, PLAYLIST_MIN_H, PLAYLIST_MIN_W, ROLL_DOCK_MIN_H, ROLL_DOCKED_RESERVE, SPLIT, TOOLBAR_H } from './layout';
/** The Channel Rack's narrowest width (state/pattern RACK_MIN_W; not imported: that module needs the app's store). */
const RACK_MIN_W = 480;

describe('parseLayout', () => {
  it('keeps the docked panels and the floating places', () => {
    const p = parseLayout(JSON.stringify({
      rackDock: true, rackDockW: 700, rollDock: true, rollDockH: 360, rollMax: false,
      rackFloat: { x: 40, y: 80, w: 900, h: 400 }, rollFloat: { x: 0, y: 54, w: 1280, h: 600 },
    }));
    expect(p).toEqual({
      rackDock: true, rackDockW: 700, rollDock: true, rollDockH: 360, rollMax: false,
      rackFloat: { x: 40, y: 80, w: 900, h: 400 }, rollFloat: { x: 0, y: 54, w: 1280, h: 600 },
    });
  });
  it('drops bad values and keeps the good ones', () => {
    const p = parseLayout(JSON.stringify({
      rackDock: 'yes', rackDockW: -5, rollDock: 1, rollDockH: 1e9, rollMax: null,
      rackFloat: { x: 'a', y: 0, w: 500, h: 300 }, rollFloat: [1, 2, 3, 4], mixH: 300,
    }));
    expect(p).toEqual({ mixH: 300 });
  });
  it('ignores a non-number size (NaN / Infinity are not JSON, strings are)', () => {
    expect(parseLayout('{"rackDockW":"700","rollDockH":"Infinity"}')).toEqual({});
    // 0 = the docked rack fits its pattern; small non-zero widths are still rejected
    expect(parseLayout('{"rackDockW":0}')).toEqual({ rackDockW: 0 });
    expect(parseLayout('{"rackDockW":50}')).toEqual({});
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

describe('fitDock', () => {
  it('gives the panel what the user asked for when it fits', () => {
    expect(fitDock(640, RACK_MIN_W, 1000, PLAYLIST_MIN_W)).toBe(640);
  });
  it('keeps the panel at its minimum', () => {
    expect(fitDock(100, RACK_MIN_W, 2000, PLAYLIST_MIN_W)).toBe(RACK_MIN_W);
  });
  it('leaves the Playlist its minimum', () => {
    expect(fitDock(1500, RACK_MIN_W, 1000, PLAYLIST_MIN_W)).toBe(1000 - PLAYLIST_MIN_W);
  });
  it('rounds to whole pixels', () => {
    expect(fitDock(600.6, 480, 2000, 320)).toBe(601);
  });
  it('is the wanted size until the area is measured', () => {
    expect(fitDock(700, RACK_MIN_W, 0, PLAYLIST_MIN_W)).toBe(700);
    expect(fitDock(100, RACK_MIN_W, NaN, PLAYLIST_MIN_W)).toBe(RACK_MIN_W);
  });
  it('never overflows when both minimums do not fit: the panel gives way, down to half', () => {
    // 1280×720 at 150 % UI scale: ~853 CSS px wide, browser 220 + splitters
    const room = 853 - 220 - 2 * SPLIT;
    const w = fitDock(640, RACK_MIN_W, room, PLAYLIST_MIN_W);
    expect(w).toBeLessThanOrEqual(room);
    expect(w).toBeGreaterThanOrEqual(room / 2);
    expect(room - w).toBeGreaterThan(0);
    // just short of both minimums: the Playlist keeps its minimum, the panel takes the rest
    expect(fitDock(640, 480, 780, 320)).toBe(460);
  });
  it('fits the docked Piano roll between the Playlist and the Mixer at 1280×720', () => {
    // the mixer gives way to a docked roll (Mixer.tsx: at most innerHeight − ROLL_DOCKED_RESERVE),
    // so the column under the toolbar holds the Playlist, the roll's splitter and the roll's minimum
    const mixer = 720 - ROLL_DOCKED_RESERVE;
    expect(mixer).toBeGreaterThanOrEqual(170); // the mixer's own minimum still fits
    const col = 720 - TOOLBAR_H - MIXER_BAR_H - mixer;
    const h = fitDock(320, ROLL_DOCK_MIN_H, col - SPLIT, PLAYLIST_MIN_H);
    expect(h).toBeGreaterThanOrEqual(ROLL_DOCK_MIN_H);
    expect(col - SPLIT - h).toBeGreaterThanOrEqual(PLAYLIST_MIN_H);
    // a tall mixer leaves too little: both shrink, nothing runs off the window
    const tight = 150;
    const h2 = fitDock(320, ROLL_DOCK_MIN_H, tight, PLAYLIST_MIN_H);
    expect(h2).toBeLessThanOrEqual(tight);
    expect(h2).toBe(75);
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
    expect(parseLayout(JSON.stringify({ mixOpen: false, mixMin: false, mixH: 280 }))).toEqual({ mixMin: true, mixH: 280 });
    expect(parseLayout(JSON.stringify({ mixOpen: true, mixMin: true }))).toEqual({ mixMin: true });
    expect(parseLayout(JSON.stringify({ mixOpen: true, mixMin: false }))).toEqual({ mixMin: false });
    expect(parseLayout(JSON.stringify({ mixMin: 'no' }))).toEqual({});
    // mixOpen itself is gone: the docked mixer is always there
    expect(parseLayout(JSON.stringify({ mixOpen: true }))).toEqual({});
  });
  it('keeps the mixer between its minimum and what leaves the Playlist room', () => {
    expect(mixMaxH(1080, false)).toBe(880);
    expect(mixMaxH(720, true)).toBe(720 - ROLL_DOCKED_RESERVE);
    expect(mixMaxH(300, false)).toBe(MIX_MIN_H); // a tiny window: never below the minimum
    expect(fitMixH(400.4, 880)).toBe(400);
    expect(fitMixH(50, 880)).toBe(MIX_MIN_H);
    expect(fitMixH(2000, 880)).toBe(880);
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
  it('the docked mixer has no title bar: only its splitter counts towards a docked roll', () => {
    expect(MIXER_BAR_H).toBe(SPLIT);
  });
});
