import { expect, MOD, test } from './fixtures';

const layout = (page: import('@playwright/test').Page) => page.evaluate(() => JSON.parse(localStorage.getItem('wave.layout') ?? '{}'));

test.describe('Windows', () => {
  test('the command palette identifies each dock target and moves the named panel', async ({ app }) => {
    await app.keyboard.press(`${MOD}+k`);
    const query = app.getByRole('searchbox', { name: 'Find a command' });
    await query.fill('dock');
    const options = app.getByRole('option', { name: /^Dock / });
    const labels = ['Channel rack', 'Piano roll', 'Mixer'].flatMap(panel => [
      `Dock ${panel} left`, `Dock ${panel} right`, `Dock ${panel} at the bottom`,
    ]);
    await expect(options.locator('.name')).toHaveText(labels);
    await query.fill('rack right dock');
    await app.getByRole('option', { name: /^Dock Channel rack right\b/ }).click();

    const rack = app.locator('.rackwin');
    await expect(rack).toHaveClass(/\bdocked\b/);
    await expect.poll(async () => JSON.stringify((await layout(app)).dock.right)).toContain('rack');
    await rack.locator('.winhd').first().click({ button: 'right' });
    await expect(app.getByRole('menuitemcheckbox', { name: 'Dock right', exact: true })).toHaveAttribute('aria-checked', 'true');
    await app.keyboard.press('Escape');

    await app.keyboard.press(`${MOD}+k`);
    await query.fill('float rack');
    await app.getByRole('option', { name: /^Float Channel rack as a window\b/ }).click();
    await expect(rack).toBeVisible();
    await expect(rack).not.toHaveClass(/\bdocked\b/);
  });

  test('the Channel rack opens floating, docks, floats again and closes', async ({ app }) => {
    const toggle = app.getByRole('button', { name: 'Channel rack' });
    const rack = app.locator('.rackwin');
    await expect(rack).toBeHidden();
    await toggle.click();
    await expect(rack).toBeVisible();
    await expect(rack).not.toHaveClass(/\bdocked\b/);

    await rack.getByRole('button', { name: /^Dock on the left/ }).click();
    await expect(rack).toHaveClass(/\bdocked\b/);
    await expect(rack.getByRole('button', { name: /^Float as a window/ })).toHaveAttribute('aria-pressed', 'true');
    // the layout survives a restart
    await expect.poll(async () => JSON.stringify((await layout(app)).dock.left)).toContain('rack');

    await rack.getByRole('button', { name: /^Float as a window/ }).click();
    await expect(rack).not.toHaveClass(/\bdocked\b/);
    await expect.poll(async () => JSON.stringify((await layout(app)).dock.left)).not.toContain('rack');

    await rack.getByRole('button', { name: 'Close (Esc)' }).click();
    await expect(rack).toBeHidden();
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  });

  test('the Mixer hides and shows from its own button, the toolbar and F9', async ({ app }) => {
    const mixer = app.getByRole('button', { name: 'Mixer', exact: true });
    const strips = app.locator('.strip[data-ins="0"]');
    await expect(strips).toBeVisible();
    await app.getByRole('button', { name: /^Hide the mixer/ }).click();
    await expect(strips).toBeHidden();
    await expect(mixer).toHaveAttribute('aria-pressed', 'false');
    await mixer.click();
    await expect(strips).toBeVisible();
    await app.keyboard.press('F9');
    await expect(strips).toBeHidden();
    await app.keyboard.press('F9');
    await expect(strips).toBeVisible();
    await expect(mixer).toHaveAttribute('aria-pressed', 'true');
  });

  test('the Piano roll opens maximised and restores to a window', async ({ app }) => {
    await app.getByRole('button', { name: 'Piano roll' }).click();
    const roll = app.locator('.rollwin');
    await expect(roll).toBeVisible();
    const full = (await roll.boundingBox())!;
    const restore = roll.getByRole('button', { name: /^Restore the window size/ });
    await expect(restore).toHaveAttribute('aria-pressed', 'true');
    await restore.click();
    await expect.poll(async () => (await roll.boundingBox())!.width).toBeLessThan(full.width);
    await roll.getByRole('button', { name: /^Maximize/ }).click();
    await expect.poll(async () => (await roll.boundingBox())!.width).toBe(full.width);
    await roll.getByRole('button', { name: 'Close (Esc)' }).click();
    await expect(roll).toBeHidden();
  });
});
