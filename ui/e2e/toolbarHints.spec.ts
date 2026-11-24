import { expect, test } from './fixtures';

test.beforeEach(async ({ context }) => {
  await context.addInitScript(() => localStorage.setItem('wave.mock.noauto', '1'));
});

test('the update action and complete tool hint are reachable at laptop and minimum window widths', async ({ app }) => {
  await app.setViewportSize({ width: 1366, height: 768 });
  await app.getByRole('menuitem', { name: 'Help', exact: true }).click();
  await app.getByRole('menuitem', { name: 'Check for updates', exact: true }).click();
  const update = app.getByRole('dialog', { name: 'Update available' });
  await update.getByRole('button', { name: 'Download update' }).click();
  await update.getByRole('button', { name: 'Restart and install' }).waitFor();
  await update.getByRole('button', { name: 'Later', exact: true }).click();

  const toolbar = app.locator('.toolbar');
  const chip = toolbar.getByRole('button', { name: 'Restart to update', exact: true });
  const hint = toolbar.getByRole('button', { name: /^Draw:.*snap/ });
  for (const width of [1366, 960]) {
    await app.setViewportSize({ width, height: 600 });
    await expect(chip).toBeVisible();
    await expect(hint).toBeVisible();
    await expect.poll(() => toolbar.evaluate(el => {
      const bar = el.getBoundingClientRect();
      const [left, middle, right] = [...el.children].map(section => section.getBoundingClientRect());
      return left.right <= middle.left + 1 && middle.right <= right.left + 1 && right.right <= bar.right + 1
        && el.scrollWidth <= el.clientWidth + 1;
    })).toBe(true);
    const fullHint = await hint.getAttribute('title');
    expect(fullHint).toMatch(/Draw:.*shortcuts/);
    await hint.focus();
    await hint.press('Enter');
    await expect(app.locator('.notice-toast [role="status"]')).toHaveText(fullHint!);
  }
});
