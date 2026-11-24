import { describe, expect, it } from 'vitest';
import { fitLevel, FIT_MAX, gridFits } from './toolbarFit';

/** A bar whose content needs `need` px at level 0 and frees `free[i]` px at level i + 1. */
function bar(room: number, need: number, free: number[]) {
  let level = -1;
  const applied: number[] = [];
  const apply = (n: number) => { level = n; applied.push(n); };
  const fits = () => need - free.slice(0, level).reduce((a, b) => a + b, 0) <= room;
  return { apply, fits, applied };
}

describe('fitLevel', () => {
  it('hides nothing when everything fits', () => {
    const b = bar(1000, 900, [50, 50]);
    expect(fitLevel(b.apply, b.fits)).toBe(0);
    expect(b.applied).toEqual([0]);
  });
  it('steps aside only as far as needed', () => {
    const b = bar(1000, 1080, [50, 50, 50]);
    expect(fitLevel(b.apply, b.fits)).toBe(2);
  });
  it('starts from nothing hidden every time, so growing the window brings parts back', () => {
    const narrow = bar(900, 1080, [50, 50, 50, 50]);
    expect(fitLevel(narrow.apply, narrow.fits)).toBe(4);
    const wide = bar(1100, 1080, [50, 50, 50, 50]);
    expect(fitLevel(wide.apply, wide.fits)).toBe(0);
  });
  it('stops at the last level when even that does not fit', () => {
    const b = bar(10, 1080, []);
    expect(fitLevel(b.apply, b.fits)).toBe(FIT_MAX);
  });
});

describe('gridFits', () => {
  it('fits when the columns need no more than the bar is wide', () => {
    expect(gridFits({ scrollWidth: 1000, clientWidth: 1000 })).toBe(true);
    expect(gridFits({ scrollWidth: 1001, clientWidth: 1000 })).toBe(true); // a sub-pixel rounding
  });
  it('does not fit when the columns run past the bar', () => expect(gridFits({ scrollWidth: 1010, clientWidth: 1000 })).toBe(false));
});
