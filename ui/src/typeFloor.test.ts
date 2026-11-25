import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// Interface text has a 10 px floor at 100% interface scale (issue #241, U5): Apple's macOS HIG puts
// the minimum legible text size at 10 pt, and the 8–9.5 px labels it replaced measured about 2 mm
// tall on a 27" 1440p screen. Effect displays' own graph axes (views/fx) are plots, not chrome.
const FLOOR = 10;
const src = __dirname;
const files = (dir: string): string[] => readdirSync(dir, { withFileTypes: true }).flatMap(e =>
  e.isDirectory() ? files(join(dir, e.name)) : /\.(tsx?|css)$/.test(e.name) && !/\.test\./.test(e.name) ? [join(dir, e.name)] : []);

/** Every font size below the floor in CSS `font`/`font-size` declarations, inline styles and canvas fonts. */
function tooSmall(text: string): string[] {
  const out: string[] = [];
  const re = /(?:font(?:-size)?\s*:|'font-size'\s*:\s*'|\.font\s*=\s*['"`])[^;}\n]*?(?<![\d.])(\d+(?:\.\d+)?)px/g;
  for (const m of text.matchAll(re)) if (+m[1] < FLOOR) out.push(m[0].trim());
  return out;
}

describe('type floor', () => {
  it('sees the shorthand, the longhand, inline styles and canvas fonts', () => {
    expect(tooSmall('.a { font: 600 9px var(--mono); } .b { font-size: 8.5px; }')).toHaveLength(2);
    expect(tooSmall(`<span style={{ 'font-size': '9px' }}>`)).toHaveLength(1);
    expect(tooSmall(`g.font = '600 9px "Geist Mono Variable", monospace';`)).toHaveLength(1);
    expect(tooSmall('.a { font: 500 10px/12px var(--sans); } .b { font-size: 11.5px; } .c { width: 8px; }')).toEqual([]);
  });

  it('no interface text is smaller than 10 px', () => {
    const bad = files(src).filter(f => !/[\\/]views[\\/]fx[\\/]/.test(f))
      .flatMap(f => tooSmall(readFileSync(f, 'utf8')).map(d => `${f.slice(src.length + 1)}: ${d}`));
    expect(bad).toEqual([]);
  });
});
