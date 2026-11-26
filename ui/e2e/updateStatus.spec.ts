// Issue #251 (#241 U6): a downloaded update shows as a row 2 switch on the right of the toolbar,
// beside Follow, and never takes the status line's room: the tool hint keeps its space and its
// full text is on hover. Not `fixtures.ts`' app: this start is not the first one, so there is
// no welcome and the automatic update check runs (the mock always finds a newer version).
import { expect, test, type Page } from '@playwright/test';

async function boot(page: Page, size: { width: number; height: number }) {
  await page.setViewportSize(size);
  await page.addInitScript(() => { if (!sessionStorage.getItem('booted')) { localStorage.setItem('wave.mock.seen', '0.9.2'); sessionStorage.setItem('booted', '1'); } });
  await page.goto('/');
}

for (const size of [{ width: 1920, height: 1080 }, { width: 1366, height: 768 }, { width: 1100, height: 700 }]) {
  test(`the update sits beside Follow and leaves the hint its room at ${size.width} px`, async ({ page }) => {
    await boot(page, size);
    const update = page.getByRole('button', { name: 'Restart to update' });
    await expect(update).toBeVisible({ timeout: 30_000 });
    // in row 2's right group, not on the status line
    await expect(page.locator('.tbview').getByRole('button', { name: 'Restart to update' })).toBeVisible();
    await expect(page.locator('.tbstatus button')).toHaveCount(0);
    const hint = page.locator('.tbstatus .hinttxt');
    await expect(hint).toBeVisible();
    const full = await hint.textContent();
    expect(full).toMatch(/^Draw:/);
    await expect(hint).toHaveAttribute('title', full!);
    // the hint keeps a readable share of the line at every width the toolbar supports
    expect((await hint.boundingBox())!.width).toBeGreaterThan(size.width >= 1366 ? 250 : 120);
    // the button is whole, on screen, and the bar did not grow past the window
    const b = (await update.boundingBox())!;
    expect(b.x + b.width).toBeLessThanOrEqual(size.width);
    expect(b.height).toBeGreaterThanOrEqual(20); // the row 2 switches' height
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    // ready is the call to action: filled with the theme's green, unlike the neutral switches beside it
    const fill = await update.evaluate(e => getComputedStyle(e).backgroundColor);
    const green = await page.evaluate(() => { const p = document.createElement('i'); p.style.color = 'var(--green)'; document.body.append(p); const c = getComputedStyle(p).color; p.remove(); return c; });
    expect(fill).toBe(green);
    expect(await page.locator('.tbview .follow').evaluate(e => getComputedStyle(e).backgroundColor)).toBe('rgba(0, 0, 0, 0)');
  });
}

for (const theme of ['dark', 'light']) {
  test(`on the narrowest bar (${theme}) the ready update is a filled icon button with its name`, async ({ page }) => {
    await page.addInitScript(th => localStorage.setItem('wave.theme', th), theme);
    await boot(page, { width: 760, height: 600 });
    const update = page.getByRole('button', { name: 'Restart to update' });
    await expect(update).toBeVisible({ timeout: 30_000 });
    await expect(page.locator('.toolbar')).toHaveClass(/(^| )f11( |$)/);
    await expect(update.locator('.lbl')).toBeHidden();
    expect(await update.evaluate(e => getComputedStyle(e).backgroundColor)).not.toBe('rgba(0, 0, 0, 0)');
    const b = (await update.boundingBox())!;
    expect(b.x + b.width).toBeLessThanOrEqual(760);
  });
}

test('Restart to update restarts into the new version and shows what is new', async ({ page }) => {
  await boot(page, { width: 1366, height: 768 });
  await page.getByRole('button', { name: 'Restart to update' }).click({ timeout: 30_000 });
  await expect(page.getByRole('dialog', { name: /What.s new/ })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole('button', { name: 'Restart to update' })).toHaveCount(0);
});

test('the status line shows one message at a time: what happened for a moment, then the tool hint', async ({ page }) => {
  await page.setViewportSize({ width: 1366, height: 768 });
  await page.goto('/');
  const welcome = page.getByRole('dialog', { name: 'Welcome to Synkopa' });
  await welcome.getByRole('button', { name: 'Open demo project' }).click();
  await expect(welcome).toBeHidden();
  const readout = page.locator('.tbstatus .readout');
  await expect(readout).toHaveText(/^Demo project/);
  await expect(readout).toHaveAttribute('title', /^Demo project/); // the full readout on hover
  await expect(page.locator('.tbstatus .hinttxt')).toHaveCount(0); // never both at once
  // a few seconds later the tool's hint has the whole line
  await expect(page.locator('.tbstatus .hinttxt')).toContainText('Draw', { timeout: 15_000 });
  await expect(readout).toHaveCount(0);
});
