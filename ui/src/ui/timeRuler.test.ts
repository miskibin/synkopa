import { describe, expect, it } from 'vitest';
import { rulerDown, rulerLabel, rulerSubdivision, type RulerHost } from './timeRuler';

describe('ruler labels when zoomed in', () => {
  it('names beats, then 16ths, once they have room', () => {
    expect(rulerSubdivision(22)).toBe(0);
    expect(rulerSubdivision(48)).toBe(1);
    expect(rulerSubdivision(191)).toBe(1);
    expect(rulerSubdivision(192)).toBe(0.25);
    expect(rulerSubdivision(1024)).toBe(0.25);
  });
  it('labels bar.beat and bar.beat.16th, leaving bar lines to the bar number', () => {
    expect(rulerLabel(0)).toBe('');
    expect(rulerLabel(16)).toBe('');
    expect(rulerLabel(17)).toBe('5.2');
    expect(rulerLabel(17.75)).toBe('5.2.4');
    expect(rulerLabel(0.25)).toBe('1.1.2');
    expect(rulerLabel(3.5)).toBe('1.4.3');
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
});
