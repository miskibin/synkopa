// Toolbar fit: sweeps window widths 1920-960 and reports any toolbar control that is clipped or
// overlapping, and which ones the priority rules hid (styles.css, "One row, like FL Studio's bar").
//   pnpm --dir ui dev --port 1431   then   node perf/toolbar_fit.mjs [url]
// The song fixture (cargo run --release -p wave-perf -- gen song) gives a realistic project.
import { chromium } from 'playwright-core';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
const base = join(process.env.LOCALAPPDATA, 'ms-playwright');
let exe = process.env.PERF_CHROME; if (!exe) for (const d of readdirSync(base).sort().reverse()) { const p = join(base, d, 'chrome-headless-shell-win64/chrome-headless-shell.exe'); if (existsSync(p)) { exe = p; break; } }
const b = await chromium.launch({ executablePath: exe });
const pg = await b.newPage({ viewport: { width: 1920, height: 900 } });
const fx = JSON.parse(readFileSync('C:/Users/skibi/mydaw/perf/.cache/song/project.json', 'utf8'));
await pg.addInitScript(p => { window.__waveFixture = p; }, fx);
await pg.goto(process.argv[2] || 'http://localhost:1431/'); await pg.waitForTimeout(3000);
// the native window buttons (3 × 46 px) are not drawn in a browser: reserve their room
await pg.evaluate(() => { const d = document.createElement('div'); d.className = 'fakewin'; d.style.cssText = 'width:138px;flex:none;align-self:stretch;background:#333;order:99'; document.querySelector('.tbright').appendChild(d); });
console.log(await pg.evaluate(() => [...document.querySelectorAll('.tbmid > *, .tbright > *, .tbleft .tbmenu > *')].map(e => (e.className||e.tagName).toString().split(' ')[0] + ':' + Math.round(e.getBoundingClientRect().width)).join('  ')));
for (let w = 1920; w >= 960; w -= 40) {
  await pg.setViewportSize({ width: w, height: 900 }); await pg.waitForTimeout(80);
  const r = await pg.evaluate(() => {
    const tb = document.querySelector('.toolbar').getBoundingClientRect();
    const limit = tb.right - 138;
    const bad = [];
    const vis = el => el.offsetParent !== null && el.getBoundingClientRect().width > 0;
    for (const sec of ['.tbleft', '.tbmid', '.tbright']) {
      const s = document.querySelector(sec); const sr = s.getBoundingClientRect();
      if (s.scrollWidth > s.clientWidth + 1) bad.push(sec + ' overflows ' + (s.scrollWidth - s.clientWidth));
      for (const c of s.querySelectorAll('button:not(.mbtn), .clockbox, select, .patselbox, .loopText')) {
        if (!vis(c)) continue; const cr = c.getBoundingClientRect();
        if (cr.right > Math.min(sr.right, limit) + 1 || cr.left < sr.left - 1) bad.push((c.className || c.tagName).toString().slice(0, 20) + ' clipped');
      }
    }
    const mid = document.querySelector('.tbmid').getBoundingClientRect(), right = document.querySelector('.tbright').getBoundingClientRect(), left = document.querySelector('.tbleft').getBoundingClientRect();
    if (mid.left < left.right - 1 || right.left < mid.right - 1) bad.push('sections overlap');
    const hidden = ['.loopText', '.clockbox .tag9', '.count', '.monbtn', '.hinttxt', '.metrobtn', '.typkb', '.panelbtn', '.tapbtn', '.projName', '.cpumeter'].filter(q => { const e = document.querySelector(q); return e && !vis(e); });
    return { bad: [...new Set(bad)], hidden };
  });
  console.log(w, r.bad.length ? 'BAD ' + r.bad.join(', ') : 'ok', '| hidden:', r.hidden.join(' '));
}
await b.close();
