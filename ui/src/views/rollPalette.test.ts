import { afterAll, describe, expect, it } from 'vitest';
import { rollPalette } from './patternDraw';
import { setTheme, THEMES } from '../ui/theme';

// "rgb(r,g,b)", "rgba(r,g,b,a)" or "#rrggbb" → [r, g, b, a]
function parse(c: string): [number, number, number, number] {
  if (c.startsWith('#')) { const n = parseInt(c.slice(1, 7), 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1]; }
  const v = c.slice(c.indexOf('(') + 1, -1).split(',').map(Number);
  return [v[0], v[1], v[2], v[3] ?? 1];
}
const lin = (u: number) => { const s = u / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
const lum = (c: string) => { const [r, g, b] = parse(c); return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b); };
/** CIE L*: perceived lightness, 0..100 (a tonal step reads the same near black as near white). */
const lstar = (c: string) => { const y = lum(c); return y > 0.008856 ? 116 * Math.cbrt(y) - 16 : 903.3 * y; };
const alpha = (c: string) => parse(c)[3];

describe('Piano roll grid palette', () => {
  afterAll(() => setTheme('dark'));

  for (const th of THEMES) {
    it(`${th.id}: black-key rows are a clear step darker than white-key rows`, () => {
      setTheme(th.id);
      const P = rollPalette();
      expect(lum(P.black)).toBeLessThan(lum(P.white));
      // the old rows were nearly one tone (#250); OLED keeps true black, so its white rows lift less
      expect(lstar(P.white) - lstar(P.black)).toBeGreaterThanOrEqual(th.id === 'oled' ? 2.4 : 3.3);
    });

    it(`${th.id}: lines rank bar > beat > subdivision, octave seam > row seam`, () => {
      setTheme(th.id);
      const P = rollPalette();
      expect(alpha(P.bar)).toBeGreaterThan(alpha(P.beat));
      expect(alpha(P.beat)).toBeGreaterThan(alpha(P.sub));
      expect(alpha(P.sub)).toBeLessThan(0.05); // quiet: a texture, not a mesh
      // the octave seam is light ink; dark themes draw row seams as a darker groove instead
      if (th.light) expect(alpha(P.octave)).toBeGreaterThan(alpha(P.seam) * 3);
      else { expect(lum(P.octave)).toBeGreaterThan(0.5); expect(lum(P.seam)).toBe(0); }
    });
  }
});
