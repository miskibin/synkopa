import { describe, expect, it } from 'vitest';
import { THEMES } from './theme';

const lum = (hex: string) => {
  const n = parseInt(hex.replace('#', ''), 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map(v => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; });
  return 0.2126 * ch[0] + 0.7152 * ch[1] + 0.0722 * ch[2];
};
const contrast = (a: string, b: string) => { const x = lum(a), y = lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };

describe('themes', () => {
  it('have unique ids', () => {
    expect(new Set(THEMES.map(t => t.id)).size).toBe(THEMES.length);
  });
  it('offer mid-grey variants on both sides', () => {
    for (const id of ['slate', 'dim', 'fog', 'sand']) expect(THEMES.some(t => t.id === id)).toBe(true);
  });
  for (const th of THEMES) {
    it(`${th.id}: body text and secondary labels stay readable on panels`, () => {
      for (const bg of [th.t.bg2, th.t.bg4, th.t.bg6]) {
        expect(contrast(th.t.fg, bg)).toBeGreaterThanOrEqual(7);
        expect(contrast(th.t.fg4, bg)).toBeGreaterThanOrEqual(3);
      }
    });
    it(`${th.id}: is on the side it claims (light vs dark)`, () => {
      expect(lum(th.t.bg2) > 0.18).toBe(th.light);
    });
  }
});
