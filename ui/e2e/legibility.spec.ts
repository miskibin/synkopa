// Issue #241, U5: at the default 100% interface scale no interface text is under 10 px and every
// pointer target meets WCAG 2.2 SC 2.5.8 (24 × 24 CSS px, or spaced so a 24 px circle on its
// centre touches no other target), on the screens most people have: 1920 × 1080 and 1366 × 768.
import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';

/** Visible text below `floor` px and undersized, crowded targets, measured in the live page. */
const audit = (page: Page) => page.evaluate(() => {
  const shown = (el: Element) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden' && r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth;
  };
  const small: string[] = [];
  const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walk.nextNode(); n; n = walk.nextNode()) {
    const el = n.parentElement;
    if (!n.nodeValue?.trim() || !el || !shown(el)) continue;
    const px = parseFloat(getComputedStyle(el).fontSize);
    if (px < 10) small.push(`${px}px "${n.nodeValue.trim().slice(0, 20)}" .${el.className}`);
  }
  // topmost targets only: one under a floating window cannot be clicked anyway
  const targets = [...document.querySelectorAll('button, [role=button], [role=radio], [role=tab], [role=menuitem], [role=switch], [role=checkbox], select, input, a[href]')]
    .filter(shown).filter(e => {
      const r = e.getBoundingClientRect();
      const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return !!top && (e === top || e.contains(top));
    });
  const rects = targets.map(e => e.getBoundingClientRect());
  const crowded: string[] = [];
  targets.forEach((e, i) => {
    const r = rects[i];
    if (r.width >= 24 && r.height >= 24) return;
    const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    const hit = rects.some((o, j) => {
      if (j === i || targets[j].contains(e) || e.contains(targets[j])) return false;
      const dx = Math.max(o.left - cx, 0, cx - o.right), dy = Math.max(o.top - cy, 0, cy - o.bottom);
      if (Math.hypot(dx, dy) < 12) return true;
      return (o.width < 24 || o.height < 24) && Math.hypot(o.left + o.width / 2 - cx, o.top + o.height / 2 - cy) < 24;
    });
    if (hit) crowded.push(`${Math.round(r.width)}x${Math.round(r.height)} ${e.getAttribute('aria-label') ?? e.getAttribute('title') ?? e.textContent}`);
  });
  const overflow = document.documentElement.scrollWidth > innerWidth || document.documentElement.scrollHeight > innerHeight;
  return { small, crowded, overflow };
});

for (const size of [{ width: 1920, height: 1080 }, { width: 1366, height: 768 }]) {
  test(`text and targets are legible at 100% on ${size.width} × ${size.height}`, async ({ app }) => {
    await app.setViewportSize(size);
    await app.getByRole('menuitem', { name: 'File', exact: true }).click();
    await app.getByRole('menuitem', { name: 'Open demo project', exact: true }).click();
    await expect(app.locator('.lane').first()).toBeVisible();
    for (const view of ['playlist', 'F6', 'F7']) {
      if (view !== 'playlist') await app.keyboard.press(view);
      const r = await audit(app);
      expect(r.small, `text under 10 px (${view})`).toEqual([]);
      expect(r.crowded, `crowded targets under 24 px (${view})`).toEqual([]);
      expect(r.overflow, `page overflow (${view})`).toBe(false);
    }
    // the switch that decides what Space plays is a full-height target
    const song = app.getByRole('radio', { name: /SONG/ }).or(app.getByRole('button', { name: /^SONG/ })).first();
    expect((await song.boundingBox())!.height).toBeGreaterThanOrEqual(22);
  });
}

test('View shows the interface scale and changes it', async ({ app }) => {
  await app.getByRole('menuitem', { name: 'View', exact: true }).click();
  await app.getByRole('button', { name: /^Interface scale: 100%/ }).hover();
  await app.getByRole('menuitemcheckbox', { name: '110%', exact: true }).click();
  // the browser build zooms with CSS (the desktop app uses the webview's own zoom)
  await expect.poll(() => app.evaluate(() => document.documentElement.style.zoom)).toBe('1.1');
  await app.getByRole('menuitem', { name: 'View', exact: true }).click();
  await expect(app.getByRole('button', { name: /^Interface scale: 110%/ })).toBeVisible();
  const saved = await app.evaluate(() => JSON.parse(localStorage.getItem('wave.prefs') ?? '{}').uiScale);
  expect(saved).toBe(1.1);
});
