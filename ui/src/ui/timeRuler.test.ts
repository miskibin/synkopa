import { describe, expect, it, vi } from 'vitest';
import { drawRuler, rulerDown, RULER_H, type RulerHost } from './timeRuler';
import { T } from './theme';

describe('ruler labels and ticks', () => {
  const render = (bw: number, offX = 0) => {
    const labels: { text: string; font: string; color: string }[] = [];
    const g = {
      fillRect: vi.fn(), save: vi.fn(), translate: vi.fn(), restore: vi.fn(),
      fillStyle: '', font: '',
      fillText(text: string) { labels.push({ text, font: this.font, color: this.fillStyle }); },
    };
    drawRuler(g as unknown as CanvasRenderingContext2D, offX, 4 * bw, bw, null, null);
    return { labels, rects: g.fillRect };
  };

  it('shows beat labels and shorter subdivision ticks as space allows', () => {
    const zoomedOut = render(22);
    expect(zoomedOut.labels.map(l => l.text)).toEqual(['1', '2']);
    const beatZoom = render(48);
    expect(beatZoom.labels.map(l => l.text)).toEqual(['1.2', '1.3', '1.4', '1', '2']);
    expect(beatZoom.rects).not.toHaveBeenCalledWith(12, RULER_H - 4, 1, 4);
    const highZoom = render(192);
    expect(highZoom.labels).toEqual(beatZoom.labels);
    expect(highZoom.rects).toHaveBeenCalledWith(48, RULER_H - 4, 1, 4);
    expect(highZoom.rects).toHaveBeenCalledWith(192, RULER_H - 8, 1, 8);
  });

  it('keeps bar labels prominent and counts beats correctly after scrolling', () => {
    const { labels } = render(192, 16.5 * 192);
    expect(labels).toContainEqual({ text: '5', font: expect.stringContaining('600 12px'), color: T.fgHi });
    expect(labels).toContainEqual({ text: '5.2', font: expect.stringContaining('500 10px'), color: T.fg4 });
    expect(labels.map(l => l.text)).toEqual(['5.2', '5.3', '5.4', '5', '6']);
  });
});

describe('ruler presses (FL)', () => {
  // a host that records what the ruler asks for; 10 px per beat, whole-beat grid
  const host = (loop: { a: number; b: number } | null = null) => {
    const rec = { seeks: [] as number[], sets: [] as { a: number; b: number }[], ended: 0, drawn: 0, cleared: 0 };
    let move: (e: PointerEvent) => void = () => {}, up: (e: PointerEvent) => void = () => {};
    let cur = loop;
    const h: RulerHost = {
      beatAt: x => x / 10, bw: () => 10, unit: () => 1, max: () => 512, loop: () => cur,
      edit: () => ({ set: r => { cur = r; rec.sets.push(r); }, end: () => { rec.ended++; } }),
      clear: () => { rec.cleared++; cur = null; }, seek: b => rec.seeks.push(b), drawn: () => { rec.drawn++; },
      track: (m, u) => { move = m; up = u; }, key: 'test-ruler-' + Math.random(),
    };
    const ev = (x: number, mods: Partial<PointerEvent> = {}) => ({ button: 0, clientX: x, clientY: 0, preventDefault() {}, ...mods }) as PointerEvent;
    return { h, rec, ev, move: (x: number) => move(ev(x)), up: (x: number) => up(ev(x)) };
  };

  it('a click sets the start position and draws no loop', () => {
    const r = host();
    rulerDown(r.h, r.ev(40)); r.up(40);
    expect(r.rec.seeks).toEqual([4]);
    expect(r.rec.sets).toEqual([]);
  });
  it('Ctrl+drag moves the start position with the pointer instead of drawing a loop', () => {
    const r = host();
    rulerDown(r.h, r.ev(40, { ctrlKey: true }));
    r.move(52); r.move(58); r.move(81); r.up(81);
    expect(r.rec.seeks).toEqual([4, 5, 6, 8]);
    expect(r.rec.sets).toEqual([]);
    expect(r.rec.ended).toBe(0);
  });
  it('a drag draws the loop, as one edit (FL)', () => {
    const r = host();
    rulerDown(r.h, r.ev(40));
    r.move(80); r.move(120); r.up(120);
    expect(r.rec.sets.at(-1)).toEqual({ a: 4, b: 12 });
    expect(r.rec.ended).toBe(1);
    expect(r.rec.drawn).toBe(1);
    expect(r.rec.seeks).toEqual([]);
  });
  it('a sloppy click that moves a few pixels draws no loop and adds no undo step', () => {
    const r = host();
    rulerDown(r.h, r.ev(40));
    r.move(42); r.move(43); r.up(43);
    expect(r.rec.sets).toEqual([]);
    expect(r.rec.ended).toBe(0);
    expect(r.rec.seeks).toEqual([4]);
  });
  it('an existing loop\'s edge drags too', () => {
    const r = host({ a: 4, b: 12 });
    rulerDown(r.h, r.ev(120));
    r.move(160); r.up(160);
    expect(r.rec.sets.at(-1)).toEqual({ a: 4, b: 16 });
    expect(r.rec.ended).toBe(1);
  });
  it('a right-drag draws the loop like a left drag, and opens no menu', () => {
    const r = host(), menu = vi.fn();
    r.h.menu = menu;
    rulerDown(r.h, r.ev(40, { button: 2 }));
    r.move(80); r.move(120); r.up(120);
    expect(r.rec.sets.at(-1)).toEqual({ a: 4, b: 12 });
    expect(r.rec.ended).toBe(1);
    expect(r.rec.seeks).toEqual([]);
    expect(menu).not.toHaveBeenCalled();
  });
  it('a right-press grabs the nearer boundary of a loop and the drag moves it; past the other they swap (FL)', () => {
    // 3..7 bars as beats 12..28, press at 20 (a tie: the start is taken), drag to 36 → 28..36 (FL: 7–9)
    const r = host({ a: 12, b: 28 });
    rulerDown(r.h, r.ev(200, { button: 2 }));
    r.move(280); r.move(360); r.up(360);
    expect(r.rec.sets.at(-1)).toEqual({ a: 28, b: 36 });
    expect(r.rec.ended).toBe(1);
    expect(r.rec.seeks).toEqual([]);
  });
  it('a right-press outside the loop extends its nearer end (FL)', () => {
    const r = host({ a: 28, b: 36 });
    rulerDown(r.h, r.ev(480, { button: 2 }));
    r.move(600); r.up(600);
    expect(r.rec.sets.at(-1)).toEqual({ a: 28, b: 60 });
  });
  it('a right-click on a loop moves its nearer boundary there, as one undo step, with no menu', () => {
    const r = host({ a: 4, b: 12 }), menu = vi.fn();
    r.h.menu = menu;
    rulerDown(r.h, r.ev(100, { button: 2 })); r.up(100);
    expect(r.rec.sets).toEqual([{ a: 4, b: 10 }]);
    expect(r.rec.ended).toBe(1);
    expect(menu).not.toHaveBeenCalled();
  });
  it('with no loop, a right-click without a drag opens the menu on release and changes nothing', () => {
    const r = host(), menu = vi.fn();
    r.h.menu = menu;
    rulerDown(r.h, r.ev(40, { button: 2 }));
    r.move(42); r.up(42);
    expect(menu).toHaveBeenCalledOnce();
    expect(r.rec.sets).toEqual([]);
    expect(r.rec.seeks).toEqual([]);
  });
});
