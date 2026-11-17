import { describe, expect, it } from 'vitest';
import { rulerLabel, rulerSubdivision } from './timeRuler';

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
