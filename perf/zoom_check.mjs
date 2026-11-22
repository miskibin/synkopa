// Playlist zoom on a slow machine (#228): with the CPU throttled the Playlist takes its "stretched"
// path (the last sharp paint drawn at the new zoom until the zoom settles). On every frame of a
// Ctrl+wheel gesture the grid canvas must cover the whole visible grid and must not be blank, and
// the stretched pixels must line up with the sharp repaint at the same zoom (only blurrier).
//   node perf/zoom_check.mjs [--size large] [--no-build] [--dpr 1.25] [--throttle 6]
// Exits 1 on an uncovered or blank frame, a misaligned stretch, or when no stretch was seen.
import { chromium } from 'playwright-core';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const arg = (k, d) => { const i = process.argv.indexOf('--' + k); return i < 0 ? d : (process.argv[i + 1]?.startsWith('--') || process.argv[i + 1] == null ? true : process.argv[i + 1]); };
const size = arg('size', 'large'), dpr = +arg('dpr', 1.25), throttle = +arg('throttle', 6);
const dist = join(root, 'perf/.cache/ui-dist-check');
const fixture = join(root, 'perf/.cache', size, 'project.json');
if (!existsSync(fixture)) { console.error(`missing ${fixture}; run: cargo run --release -p wave-perf -- gen ${size}`); process.exit(2); }
if (!arg('no-build', false)) {
  const r = spawnSync('npx', ['vite', 'build', '--outDir', dist, '--emptyOutDir', '--minify', 'false'], { cwd: join(root, 'ui'), env: { ...process.env, VITE_PERF: '1' }, shell: true, stdio: ['ignore', 'ignore', 'inherit'] });
  if (r.status !== 0) process.exit(r.status ?? 1);
}
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.woff2': 'font/woff2', '.svg': 'image/svg+xml', '.json': 'application/json' };
const server = createServer((req, res) => {
  let p = decodeURIComponent((req.url || '/').split('?')[0]); if (p === '/') p = '/index.html';
  const f = join(dist, p);
  if (!f.startsWith(dist) || !existsSync(f) || statSync(f).isDirectory()) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { 'content-type': mime[extname(f)] || 'application/octet-stream' }); res.end(readFileSync(f));
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
const base = join(process.env.LOCALAPPDATA || '', 'ms-playwright');
const shell = existsSync(base) && readdirSync(base).sort().reverse().filter(d => d.startsWith('chromium_headless_shell-')).map(d => join(base, d, 'chrome-headless-shell-win64/chrome-headless-shell.exe')).find(existsSync);
const browser = await chromium.launch({ executablePath: process.env.PERF_CHROME || shell });
const ctx = await browser.newContext({ viewport: { width: 1920, height: 1000 }, deviceScaleFactor: dpr });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', e => errors.push(String(e)));
await page.addInitScript(p => { window.__waveFixture = p; }, JSON.parse(readFileSync(fixture, 'utf8')));
await page.goto(`http://127.0.0.1:${server.address().port}/`);
await page.waitForFunction(() => window.__perf && window.__perf.S.ready, null, { timeout: 60000 });
const cdp = await ctx.newCDPSession(page);
await cdp.send('Emulation.setCPUThrottlingRate', { rate: throttle });

// Runs in the page: a gesture of `steps` Ctrl+wheel notches (one per frame) over the middle of the
// grid, then waits for the sharp repaint. Per frame: is the visible grid covered and not blank; at
// the last stretched frame and after the settle, the grid canvas pixels (same tile) to compare.
const gesture = (dir, steps) => page.evaluate(async ({ dir, steps }) => {
  const { geom } = window.__perf;
  const raf = () => new Promise(r => requestAnimationFrame(() => r()));
  const cv = document.querySelector('canvas.gridCanvas:not(.gridOverlay)');
  const grab = () => ({ t: cv.style.transform, w: cv.width, h: cv.height, d: cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data });
  const bg = (() => { const d = cv.getContext('2d').getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2]]; })();
  const check = () => {
    const s = geom.scroll.getBoundingClientRect(), c = cv.getBoundingClientRect(), g = geom.grid.getBoundingClientRect();
    // the visible part of the grid (the lane headers sit left of it)
    const vx0 = Math.max(s.left, g.left), vx1 = Math.min(s.right, g.right), vy0 = Math.max(s.top + 26, g.top), vy1 = Math.min(s.bottom, g.bottom);
    const covered = c.left <= vx0 + 1 && c.right >= vx1 - 1 && c.top <= vy0 + 1 && c.bottom >= vy1 - 1;
    // blank: no pixel in a sparse sample differs from the background
    const d = cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data;
    let ink = 0;
    for (let i = 0; i < d.length; i += 4 * 97) if (Math.abs(d[i] - bg[0]) + Math.abs(d[i + 1] - bg[1]) + Math.abs(d[i + 2] - bg[2]) > 12) ink++;
    return { covered, ink, scaled: /scale/.test(cv.style.transform) };
  };
  const r = geom.scroll.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2;
  const frames = [];
  let stretchedShot = null;
  for (let i = 0; i < steps; i++) {
    geom.scroll.dispatchEvent(new WheelEvent('wheel', { deltaY: dir * 100, ctrlKey: true, clientX: x, clientY: y, bubbles: true, cancelable: true }));
    await raf(); // the Playlist draws first in a frame: its rAF was queued before this one
    frames.push(check());
  }
  stretchedShot = grab();
  // settle: the sharp repaint
  await new Promise(r => setTimeout(r, 400)); await raf(); await raf(); await raf();
  frames.push(check());
  const sharp = grab();
  // mean abs difference of 8×8 block averages (blur-tolerant), aligned and shifted by 24 px
  const diff = (a, b, shift) => {
    if (a.w !== b.w || a.h !== b.h || a.t !== b.t) return null;
    const B = 8, W = a.w, H = a.h; let sum = 0, n = 0;
    for (let by = 0; by + B <= H; by += B * 3) for (let bx = 0; bx + B + Math.abs(shift) <= W; bx += B * 3) {
      let sa = 0, sb = 0;
      for (let yy = 0; yy < B; yy++) for (let xx = 0; xx < B; xx++) {
        const ia = ((by + yy) * W + bx + xx) * 4, ib = ((by + yy) * W + bx + xx + Math.max(0, shift)) * 4;
        sa += a.d[ia] + a.d[ia + 1] + a.d[ia + 2]; sb += b.d[ib] + b.d[ib + 1] + b.d[ib + 2];
      }
      sum += Math.abs(sa - sb) / (B * B * 3); n++;
    }
    return sum / n;
  };
  return { frames, aligned: diff(stretchedShot, sharp, 0), shifted: diff(stretchedShot, sharp, Math.round(24 * devicePixelRatio)), identical: diff(stretchedShot, sharp, 0) === 0 };
}, { dir, steps });

// warm up: a few full paints so the frame gaps are measured
await page.evaluate(() => { const { geom } = window.__perf; geom.scroll.scrollLeft = 2000; });
await page.waitForTimeout(1500);
let bad = 0, sawStretch = false;
for (const [name, dir] of [['in', -1], ['out', 1], ['in', -1]]) {
  const res = await gesture(dir, 4);
  const holes = res.frames.filter(f => !f.covered).length, blank = res.frames.filter(f => f.ink === 0).length, scaled = res.frames.filter(f => f.scaled).length;
  const stretched = res.aligned != null && !res.identical;
  sawStretch ||= stretched;
  const misaligned = stretched && !(res.aligned < res.shifted * 0.6);
  console.log(`zoom ${name}: frames ${res.frames.length}, uncovered ${holes}, blank ${blank}, css-scaled ${scaled}, stretched ${stretched}, block diff aligned ${res.aligned?.toFixed(2)} vs 24px-off ${res.shifted?.toFixed(2)}`);
  if (holes || blank || scaled || misaligned) bad++;
}
if (!sawStretch) { console.log('no stretched frame seen: raise --throttle'); bad++; }
if (errors.length) console.log('page errors:', errors);
await browser.close(); server.close();
console.log(bad || errors.length ? 'FAIL' : 'OK');
process.exit(bad || errors.length ? 1 : 0);
