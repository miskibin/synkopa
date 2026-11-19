import { describe, expect, it } from 'vitest';
import { shadeBands, shadeUnit, SHADE_MIN_PX } from './gridShade';

describe('grid shading', () => {
  it('shades beats when zoomed in, then bars, then groups of bars', () => {
    expect(shadeUnit(96)).toBe(1);
    expect(shadeUnit(SHADE_MIN_PX)).toBe(1);
    expect(shadeUnit(SHADE_MIN_PX - 1)).toBe(4);
    expect(shadeUnit(6)).toBe(4);
    expect(shadeUnit(2)).toBe(16);
    expect(shadeUnit(0.5)).toBe(64);
    expect(shadeUnit(0.01)).toBe(256);
    // a wider minimum band (the Playlist): bars until a beat is that wide
    expect(shadeUnit(21, 48)).toBe(4);
    expect(shadeUnit(48, 48)).toBe(1);
  });
  it('shades the second unit of every pair, whatever the scroll position', () => {
    expect(shadeBands(0, 4, 1)).toEqual([[1, 1], [3, 1]]);
    expect(shadeBands(0, 16, 4)).toEqual([[4, 4], [12, 4]]);
    // the same bands when the view starts mid-way: a band that began before the view is kept
    expect(shadeBands(5, 9, 4)).toEqual([[4, 4]]);
    expect(shadeBands(8, 12, 4)).toEqual([]);
    expect(shadeBands(1.5, 2, 1)).toEqual([[1, 1]]);
  });
});
