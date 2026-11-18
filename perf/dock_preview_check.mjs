// Dock guide previews (#161): for every guide in several layouts, drag a panel onto it, read the blue
// preview, drop it, and check the panel lands exactly where the preview showed.
//   VITE_PERF=1 pnpm --dir ui dev --port 1433   (the __perf hook)   then   node perf/dock_preview_check.mjs [url]
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { chromium } = require('playwright-core');
import { readdirSync, existsSync } from 'node:fs'; import { join } from 'node:path';
const base = join(process.env.LOCALAPPDATA, 'ms-playwright'); let exe;
for (const d of readdirSync(base).sort().reverse()) { const p = join(base, d, 'chrome-headless-shell-win64/chrome-headless-shell.exe'); if (existsSync(p)) { exe = p; break; } }
const url = process.argv[2] || 'http://localhost:1433/';
const b = await chromium.launch({ executablePath: exe });
let pg;
async function fresh(d, openList = []) { if (pg) await pg.context().close(); const ctx = await b.newContext({ viewport: { width: 1600, height: 900 } }); pg = await ctx.newPage(); await pg.goto(url); await pg.waitForTimeout(2500);
  await pg.mouse.click(800, 300); await pg.keyboard.press('F6'); await pg.waitForTimeout(150); await pg.keyboard.press('F7'); await pg.waitForTimeout(150);
  await pg.evaluate(d => { const P = window.__perf; P.setU('dock', d); const has = id => [...d.left, ...d.right, ...d.bottom].some(s => s.some(p => p.id === id)); if (!has('roll')) P.setU('rollWin', null); if (!has('rack') && !(d.open || []).includes('rack')) P.setU('rackWin', null); }, { ...d, open: openList });
  await pg.waitForTimeout(400); }
const scenarios = [
  { name: 'mix bottom, drag mix', dock: { left: [], right: [], bottom: [[{ id: 'mix', share: 1 }]] }, drag: 'mix' },
  { name: 'rack left + mix bottom, drag mix', dock: { left: [[{ id: 'rack', share: 1 }]], right: [], bottom: [[{ id: 'mix', share: 1 }]] }, drag: 'mix' },
  { name: 'rack+roll stacked left + mix bottom, drag mix', dock: { left: [[{ id: 'rack', share: 1 }, { id: 'roll', share: 1 }]], right: [], bottom: [[{ id: 'mix', share: 1 }]] }, drag: 'mix' },
  { name: 'roll right + mix bottom, drag rack', dock: { left: [], right: [[{ id: 'roll', share: 1 }]], bottom: [[{ id: 'mix', share: 1 }]] }, drag: 'rack', open: ['rack'] },
  { name: 'roll + mix side by side bottom, drag rack', dock: { left: [], right: [], bottom: [[{ id: 'roll', share: 1 }, { id: 'mix', share: 1 }]] }, drag: 'rack', open: ['rack'] },
  { name: 'rack left + mix bottom, drag rack', dock: { left: [[{ id: 'rack', share: 1 }]], right: [], bottom: [[{ id: 'mix', share: 1 }]] }, drag: 'rack' },
];
const R = r => r && [Math.round(r.x), Math.round(r.y), Math.round(r.w), Math.round(r.h)].join(',');
let bad = 0, total = 0;
for (const sc of scenarios) {
  await fresh(sc.dock, sc.open);
  console.log(sc.name, 'docked panes:', await pg.$$eval('[data-dockpane]', es => es.map(e => e.dataset.dockpane).join(' ')));
  const nGuides = await (async () => { await startDrag(sc.drag); const n = await pg.$$eval('.dockguide', e => e.length); await pg.mouse.up(); return n; })();
  for (let i = 0; i < nGuides; i++) {
    await fresh(sc.dock, sc.open);
    await startDrag(sc.drag);
    const gs = await pg.$$eval('.dockguide', es => es.map(e => { const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; }));
    const g = gs[i];
    await pg.mouse.move(g.x, g.y, { steps: 4 }); await pg.waitForTimeout(50);
    const prev = await pg.$eval('.dockpreview', e => { const r = e.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; }).catch(() => null);
    await pg.mouse.up(); await pg.waitForTimeout(250);
    const land = await pg.evaluate(id => {
      const sel = id === 'mix' ? '.mixhd' : '.dockpane .winhd';
      for (const p of document.querySelectorAll('[data-dockpane]')) {
        if (id === 'mix' ? p.querySelector('.mixhd') : (p.textContent || '').includes('Channel rack') && !p.querySelector('.mixhd')) { const r = p.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; }
      }
      return null;
    }, sc.drag);
    total++;
    // place = the preview's position and length along the edge; thickness can differ (strips are fitted)
    const ok = prev && land && Math.abs(prev.x - land.x) <= 12 && Math.abs(prev.y - land.y) <= 12 && (Math.abs(prev.w - land.w) <= 12 || Math.abs(prev.h - land.h) <= 12)
      && Math.abs((prev.x + prev.w) - (land.x + land.w)) <= 12 + (Math.abs(prev.w - land.w) > 12 ? 9999 : 0);
    const exact = prev && land && Math.abs(prev.x - land.x) <= 12 && Math.abs(prev.y - land.y) <= 12 && Math.abs(prev.w - land.w) <= 12 && Math.abs(prev.h - land.h) <= 12;
    if (!exact) bad++;
    console.log(`${sc.name} #${i} guide@${Math.round(g.x)},${Math.round(g.y)}  preview ${R(prev)}  landed ${R(land)}  ${exact ? 'MATCH' : ok ? 'same place, other thickness' : 'MISMATCH'}`);
  }
}
console.log(`${total - bad}/${total} exact`);
await b.close();
process.exit(bad ? 1 : 0);
async function startDrag(id) {
  const sel = id === 'mix' ? '.mixhd' : (await pg.$('[data-dockpane] .rackhd, [data-dockpane] .winhd')) && !(await pg.$('.winlayer .winhd')) ? '[data-dockpane] .winhd' : '.winlayer .winhd';
  const h = await pg.$(sel); const r = await h.boundingBox();
  await pg.mouse.move(r.x + 60, r.y + r.height / 2); await pg.mouse.down();
  await pg.mouse.move(r.x + 90, r.y + r.height / 2 + 30, { steps: 5 }); await pg.waitForTimeout(80);
}
