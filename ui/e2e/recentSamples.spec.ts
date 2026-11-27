import { expect, test } from './fixtures';

test('Recent lists used samples rather than previews, newest first, and survives reload', async ({ app }, testInfo) => {
  const browser = app.locator('.browser');
  await browser.getByRole('button', { name: 'LIBRARY', exact: true }).click();
  const search = browser.getByRole('textbox', { name: 'Search samples' });
  const recent = browser.locator('.brecent');
  const use = async (q: string) => {
    await search.fill(q);
    const row = browser.locator('.fitem').first();
    await expect(row).toBeVisible();
    const path = await row.getAttribute('data-path');
    await row.click({ button: 'right' });
    await app.getByText('Add as sampler channel', { exact: true }).click();
    await expect.poll(() => app.evaluate(() => JSON.parse(localStorage.getItem('wave.browser.recent') ?? '[]')[0]?.path)).toBe(path);
    await search.fill('');
    return path;
  };
  await search.fill('snare');
  await expect(browser.locator('.fitem').first()).toBeVisible();
  await browser.locator('.fitem').first().click();
  await search.fill('');
  await expect(recent).toHaveCount(0);

  const first = await use('snare');
  const second = await use('kick');
  await expect(recent.locator('.fitem').first()).toHaveAttribute('data-path', second!);
  await expect(recent.locator('.fitem').last()).toHaveAttribute('data-path', first!);
  await use('snare');
  await expect(recent.locator('.fitem')).toHaveCount(2);
  await expect(recent.locator('.fitem').first()).toHaveAttribute('data-path', first!);
  await browser.screenshot({ path: testInfo.outputPath('library-recent.png') });

  await recent.getByRole('button').click();
  await expect(recent.locator('.fitem')).toHaveCount(0);
  await app.reload();
  await browser.getByRole('button', { name: 'LIBRARY', exact: true }).click();
  await expect(recent.getByRole('button')).toHaveAttribute('aria-expanded', 'false');
  await recent.getByRole('button').click();
  await expect(recent.locator('.fitem').first()).toHaveAttribute('data-path', first!);
});
