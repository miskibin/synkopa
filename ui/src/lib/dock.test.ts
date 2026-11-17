import { describe, expect, it } from 'vitest';
import {
  defaultDock, defaultHomes, dockGuides, edgeTarget, findPane, fitStrips, guideAt, homeOf, parseDock, placePane, removePane, splitShares,
  type DockLayout, type Guide,
} from './dock';

const L = (l: Partial<DockLayout>): DockLayout => ({ left: [], right: [], bottom: [], ...l });
const one = (id: 'rack' | 'roll' | 'mix', share = 1) => ({ id, share });

describe('placing panels', () => {
  it('a new strip beside the Playlist or outside the others', () => {
    const l = L({ bottom: [[one('mix')]] });
    expect(placePane(l, 'roll', { kind: 'strip', edge: 'bottom', at: 0 }).bottom).toEqual([[one('roll')], [one('mix')]]);
    expect(placePane(l, 'roll', { kind: 'strip', edge: 'bottom', at: 9 }).bottom).toEqual([[one('mix')], [one('roll')]]);
    expect(placePane(l, 'rack', { kind: 'strip', edge: 'right', at: 0 }).right).toEqual([[one('rack')]]);
  });
  it('joining a strip: side by side / stacked, taking an average share', () => {
    const l = L({ bottom: [[one('mix', 3)]] });
    expect(placePane(l, 'roll', { kind: 'join', edge: 'bottom', strip: 0, at: 0 }).bottom).toEqual([[one('roll', 3), one('mix', 3)]]);
    expect(placePane(l, 'roll', { kind: 'join', edge: 'bottom', strip: 0, at: 1 }).bottom).toEqual([[one('mix', 3), one('roll', 3)]]);
  });
  it('a docked panel moves: it leaves its old place, and a strip it empties goes', () => {
    const l = L({ left: [[one('rack')]], bottom: [[one('roll')], [one('mix')]] });
    const n = placePane(l, 'rack', { kind: 'join', edge: 'bottom', strip: 1, at: 1 });
    expect(n).toEqual(L({ bottom: [[one('roll')], [one('mix'), one('rack')]] }));
    expect(findPane(n, 'rack')).toEqual({ edge: 'bottom', strip: 1, at: 1 });
    // the input is not changed
    expect(l.left).toEqual([[one('rack')]]);
  });
  it('a join into a strip that is gone makes a new strip instead', () => {
    expect(placePane(L({}), 'mix', { kind: 'join', edge: 'left', strip: 3, at: 0 }).left).toEqual([[one('mix')]]);
  });
  it('removing keeps the rest in order', () => {
    const l = L({ right: [[one('roll'), one('mix')], [one('rack')]] });
    expect(removePane(l, 'roll').right).toEqual([[one('mix')], [one('rack')]]);
    expect(removePane(l, 'rack').right).toEqual([[one('roll'), one('mix')]]);
  });
  it('the dock button goes back to the edge and side it came from', () => {
    const l = L({ bottom: [[one('roll')], [one('mix')]] });
    const h = homeOf(l, 'mix', defaultHomes().mix);
    expect(h).toEqual({ edge: 'bottom', outer: true });
    expect(homeOf(l, 'roll', defaultHomes().roll)).toEqual({ edge: 'bottom', outer: false });
    // floated, then docked home: outside the roll again
    const f = removePane(l, 'mix');
    expect(placePane(f, 'mix', edgeTarget(f, 'mix', h.edge, h.outer)).bottom).toEqual(l.bottom);
    // a floating panel keeps its home
    expect(homeOf(f, 'mix', h)).toBe(h);
  });
  it('first launch: only the Mixer is docked, at the bottom', () => {
    expect(defaultDock()).toEqual(L({ bottom: [[one('mix')]] }));
  });
});

describe('fitStrips', () => {
  it('gives each strip what it wants when it fits, never under its minimum', () => {
    expect(fitStrips([500, 300], [480, 170], 2000, 320)).toEqual([500, 300]);
    expect(fitStrips([100], [480], 2000, 320)).toEqual([480]);
  });
  it('is the wanted size until the area is measured', () => {
    expect(fitStrips([700], [480], 0, 320)).toEqual([700]);
    expect(fitStrips([100], [480], NaN, 320)).toEqual([480]);
  });
  it('leaves the Playlist its minimum: each strip gives up the same part of what it has above its minimum', () => {
    const px = fitStrips([800, 600], [480, 300], 1400, 320);
    expect(px[0] + px[1]).toBeLessThanOrEqual(1400 - 320);
    expect(px[0]).toBeGreaterThanOrEqual(480); expect(px[1]).toBeGreaterThanOrEqual(300);
    expect((px[0] - 480) / (800 - 480)).toBeCloseTo((px[1] - 300) / (600 - 300), 1);
  });
  it('one strip: like the old single docked panel', () => {
    expect(fitStrips([1500], [480], 1000, 320)).toEqual([680]);
    expect(fitStrips([640], [480], 780, 320)).toEqual([460]);
  });
  it('never overflows when even the minimums do not fit: they give way alike, down to half', () => {
    // 1280×720 at 150 % UI scale: ~853 px wide, browser 220 and splitters
    const room = 853 - 220 - 10;
    const px = fitStrips([640, 500], [480, 420], room, 320);
    const sum = px[0] + px[1];
    expect(sum).toBeLessThanOrEqual(room);
    expect(sum).toBeGreaterThanOrEqual(Math.floor(room / 2) - 1);
    expect(px[0] / px[1]).toBeCloseTo(480 / 420, 1);
  });
  it('the roll and the mixer both docked at the bottom of a 720 px window keep the Playlist visible', () => {
    const col = 720 - 54 - 2 * 5; // under the toolbar, two splitters
    const [roll, mix] = fitStrips([320, 300], [300, 170], col, 140);
    expect(col - roll - mix).toBeGreaterThanOrEqual(140);
    expect(roll).toBeGreaterThanOrEqual(300 - 30);
  });
});

describe('splitShares', () => {
  it('moves the room between the two panels beside the splitter only', () => {
    expect(splitShares([300, 300, 200], 0, 50, [100, 100, 100])).toEqual([350, 250, 200]);
    expect(splitShares([300, 300, 200], 1, -40, [100, 100, 100])).toEqual([300, 260, 240]);
  });
  it('stops at either panel’s minimum', () => {
    expect(splitShares([300, 300], 0, 1000, [100, 120])).toEqual([480, 120]);
    expect(splitShares([300, 300], 0, -1000, [100, 120])).toEqual([100, 500]);
  });
});

describe('dock guides', () => {
  const stage = { x: 0, y: 50, w: 1600, h: 900 }, center = { x: 300, y: 50, w: 1000, h: 500 };
  const panes = [{ edge: 'bottom' as const, strip: 0, at: 0, rect: { x: 0, y: 555, w: 1600, h: 395 } }];
  const gs = dockGuides(stage, center, panes, { left: 0, right: 1, bottom: 1 });
  const find = (pred: (g: Guide) => boolean) => gs.find(pred)!;
  it('outer edges dock outside every strip there, the Playlist compass beside the Playlist', () => {
    expect(find(g => g.dir === 'right' && g.x > 1500).target).toEqual({ kind: 'strip', edge: 'right', at: 1 });
    expect(find(g => g.dir === 'down' && g.y > 900).target).toEqual({ kind: 'strip', edge: 'bottom', at: 1 });
    const c = gs.filter(g => Math.abs(g.y - 300) <= 41 && g.x > 700 && g.x < 900);
    expect(c.map(g => g.target)).toEqual([
      { kind: 'strip', edge: 'left', at: 0 }, { kind: 'strip', edge: 'right', at: 0 }, { kind: 'strip', edge: 'bottom', at: 0 },
    ]);
  });
  it('a bottom panel’s compass: left / right share its strip (side by side), up / down make a strip above / below', () => {
    const cy = 555 + 395 / 2, on = gs.filter(g => Math.abs(g.y - cy) <= 41 && Math.abs(g.x - 800) <= 41);
    const by = (d: Guide['dir']) => on.find(g => g.dir === d)!.target;
    expect(by('left')).toEqual({ kind: 'join', edge: 'bottom', strip: 0, at: 0 });
    expect(by('right')).toEqual({ kind: 'join', edge: 'bottom', strip: 0, at: 1 });
    expect(by('up')).toEqual({ kind: 'strip', edge: 'bottom', at: 0 });
    expect(by('down')).toEqual({ kind: 'strip', edge: 'bottom', at: 1 });
  });
  it('a side panel’s compass: up / down stack in its strip', () => {
    const g2 = dockGuides(stage, center, [{ edge: 'left', strip: 1, at: 0, rect: { x: 0, y: 50, w: 290, h: 500 } }], { left: 2, right: 0, bottom: 0 });
    const on = g2.filter(g => Math.abs(g.x - 145) <= 41 && Math.abs(g.y - 300) <= 41);
    const by = (d: Guide['dir']) => on.find(g => g.dir === d)!.target;
    expect(by('up')).toEqual({ kind: 'join', edge: 'left', strip: 1, at: 0 });
    expect(by('down')).toEqual({ kind: 'join', edge: 'left', strip: 1, at: 1 });
    expect(by('right')).toEqual({ kind: 'strip', edge: 'left', at: 1 }); // toward the Playlist
    expect(by('left')).toEqual({ kind: 'strip', edge: 'left', at: 2 });
  });
  it('only a guide under the pointer counts (a drop anywhere else leaves the window floating)', () => {
    const g = gs[0];
    expect(guideAt(gs, g.x + 10, g.y - 10)).toBe(g);
    expect(guideAt(gs, 600, 200)).toBeNull();
  });
  it('previews stay inside what they split', () => {
    for (const g of gs) {
      expect(g.preview.w).toBeGreaterThan(0); expect(g.preview.h).toBeGreaterThan(0);
      expect(g.preview.x).toBeGreaterThanOrEqual(stage.x); expect(g.preview.x + g.preview.w).toBeLessThanOrEqual(stage.x + stage.w + 0.5);
    }
  });
});

describe('parseDock', () => {
  it('drops unknown and repeated panels, bad shares and empty strips', () => {
    expect(parseDock({ left: [[{ id: 'rack', share: -1 }, { id: 'bogus' }], [], [{ id: 'rack' }]], bottom: [[{ id: 'mix', share: 2 }]] }))
      .toEqual(L({ left: [[one('rack')]], bottom: [[one('mix', 2)]] }));
  });
  it('is undefined for something that is not a layout', () => {
    expect(parseDock('left')).toBeUndefined();
    expect(parseDock([])).toBeUndefined();
    expect(parseDock({ left: 'x' })).toBeUndefined();
    expect(parseDock({})).toEqual(L({}));
  });
});
