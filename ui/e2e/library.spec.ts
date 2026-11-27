import { expect, test } from './fixtures';

test('Library collapse all closes the tree, clears search and remembers the closed groups', async ({ app }, testInfo) => {
  const browser = app.locator('.browser');
  await browser.getByRole('button', { name: 'LIBRARY', exact: true }).click();
  const search = browser.getByRole('textbox', { name: 'Search samples' });
  const collapse = browser.getByRole('button', { name: 'Collapse all (clears search)' });
  const std = browser.locator('.bhead.tog').filter({ hasText: 'Standard library' });
  const folders = browser.locator('.bhead.tog').filter({ hasText: 'FOLDERS' });

  const folder = browser.locator('.folder').first();
  await folder.click();
  await expect(folder).toHaveAttribute('aria-expanded', 'true');
  await expect(browser.locator('.grow').first()).toBeVisible();
  const group = browser.locator('.grow').first();
  if (await group.getAttribute('aria-expanded') === 'false') await group.click();
  await expect(group).toHaveAttribute('aria-expanded', 'true');

  await search.fill('snare');
  await expect(browser.locator('.fitem').first()).toBeVisible();
  await browser.screenshot({ path: testInfo.outputPath('library-before.png') });

  await collapse.click();
  await expect(search).toHaveValue('');
  await expect(std).toHaveAttribute('aria-expanded', 'false');
  await expect(folders).toHaveAttribute('aria-expanded', 'false');
  await expect(browser.locator('.fitem')).toHaveCount(0);
  await expect(browser.locator('.scroll')).toHaveJSProperty('scrollTop', 0);
  await browser.screenshot({ path: testInfo.outputPath('library-after.png') });

  // Reopening a parent must not restore its previously expanded children.
  await std.click();
  await expect(browser.locator('.grow').first()).toBeVisible();
  await expect(browser.locator('.grow[aria-expanded="true"]')).toHaveCount(0);
  await folders.click();
  await expect(folder).toHaveAttribute('aria-expanded', 'false');
  await collapse.focus();
  await app.keyboard.press('Enter');
  await expect(std).toHaveAttribute('aria-expanded', 'false');
  await expect(folders).toHaveAttribute('aria-expanded', 'false');

  // A new search still opens matches, independently of the collapsed browsing state.
  await search.fill('snare');
  await expect(browser.locator('.fitem').first()).toBeVisible();
  await search.fill('');
  await expect(std).toHaveAttribute('aria-expanded', 'false');
});
