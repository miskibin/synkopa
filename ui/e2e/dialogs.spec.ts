// Modal dialogs and the keyboard (#247): Tab stays in the dialog on top, Esc closes it however
// often you pressed Tab first, the app's shortcuts are off behind it, and the focus goes back to
// what opened it.
import { test as base, type Page } from '@playwright/test';
import { engine, expect, MOD, pollEngine, test } from './fixtures';

/** The dialog the focused element is in (its accessible name), or null when the focus is outside every dialog. */
const focusedDialog = (page: Page) => page.evaluate(() => {
  const d = document.activeElement?.closest('[aria-modal="true"]');
  return d ? d.getAttribute('aria-label') ?? document.getElementById(d.getAttribute('aria-labelledby') ?? '')?.textContent ?? '?' : null;
});

/** Give the project unsaved changes straight in the MockBackend (any edit would do). */
const makeDirty = (page: Page) => page.evaluate(() => {
  const m = (window as unknown as { __waveMock: { revision: number; emit(e: string, p: unknown): void; dto(): unknown } }).__waveMock;
  m.revision++;
  m.emit('state', m.dto());
});

base('Welcome: focus starts on the primary action, Tab and Shift+Tab never leave it, Esc closes it', async ({ page }) => {
  base.setTimeout(120_000); // the first page load compiles the app
  await page.goto('/');
  const welcome = page.getByRole('dialog', { name: 'Welcome to Synkopa' });
  await expect(welcome).toBeVisible({ timeout: 60_000 });
  await expect(welcome.getByRole('button', { name: 'Open demo project' })).toBeFocused();
  for (let i = 0; i < 6; i++) {
    await page.keyboard.press('Tab');
    expect(await focusedDialog(page)).toBe('Welcome to Synkopa');
  }
  for (let i = 0; i < 6; i++) {
    await page.keyboard.press('Shift+Tab');
    expect(await focusedDialog(page)).toBe('Welcome to Synkopa');
  }
  await page.keyboard.press('Escape');
  await expect(welcome).toBeHidden();
});

test('Unsaved changes: Tab stays in the prompt, Esc still cancels after it, and no shortcut fires behind it', async ({ app }) => {
  await makeDirty(app);
  await pollEngine(app, s => s.dirty).toBe(true);
  const metro = (await engine(app)).transport.metro;
  await app.keyboard.press(`${MOD}+Shift+N`);
  const prompt = app.getByRole('alertdialog', { name: 'Unsaved changes' });
  await expect(prompt).toBeVisible();
  await expect(prompt.getByRole('button', { name: 'Save', exact: true })).toBeFocused();
  // the report's repro: two Tabs used to put the focus on File behind the prompt
  await app.keyboard.press('Tab');
  await app.keyboard.press('Tab');
  expect(await focusedDialog(app)).toBe('Unsaved changes');
  await expect(app.getByRole('menuitem', { name: 'File', exact: true })).not.toBeFocused();
  // a shortcut of the app behind it (Ctrl+M: metronome) does nothing
  await app.keyboard.press(`${MOD}+m`);
  await app.keyboard.press('Escape');
  await expect(prompt).toBeHidden();
  expect((await engine(app)).transport.metro).toBe(metro);
  // nothing was replaced: Esc cancelled
  expect((await engine(app)).dirty).toBe(true);
});

test('Esc closes a dialog even after a click on its text moved the focus off its controls', async ({ app }) => {
  await app.keyboard.press('?');
  const help = app.getByRole('dialog', { name: 'Keyboard shortcuts' });
  await expect(help).toBeVisible();
  await help.locator('.helplist .sc').first().click();
  expect(await focusedDialog(app)).toBe('Keyboard shortcuts');
  await app.keyboard.press('Tab');
  await app.keyboard.press('Tab');
  await app.keyboard.press('Tab');
  expect(await focusedDialog(app)).toBe('Keyboard shortcuts');
  await app.keyboard.press('Escape');
  await expect(help).toBeHidden();
});

test('closing a dialog gives the focus back to the control that opened it', async ({ app }) => {
  const opener = app.locator('.toolbar button:visible').first();
  await opener.focus();
  await app.keyboard.press(`${MOD}+k`);
  const palette = app.getByRole('dialog', { name: 'Find a command' });
  await expect(palette.getByRole('searchbox')).toBeFocused();
  // Esc first clears the typed text, then closes
  await app.keyboard.type('zz');
  await app.keyboard.press('Escape');
  await expect(palette).toBeVisible();
  await expect(palette.getByRole('searchbox')).toHaveValue('');
  await app.keyboard.press('Escape');
  await expect(palette).toBeHidden();
  await expect(opener).toBeFocused();
});

test('Settings: the open tab gets the focus, Tab cycles inside, a select list closes on Esc before the dialog does', async ({ app }) => {
  await app.getByRole('menuitem', { name: 'File', exact: true }).click();
  await app.getByRole('menuitem', { name: 'Settings…', exact: true }).click();
  const settings = app.getByRole('dialog', { name: 'Settings' });
  await expect(settings).toBeVisible();
  await expect(settings.getByRole('tab', { selected: true })).toBeFocused();
  for (let i = 0; i < 25; i++) {
    await app.keyboard.press('Tab');
    expect(await focusedDialog(app)).toBe('Settings');
  }
  const select = settings.locator('select:visible').first();
  if (await select.count()) {
    await select.focus();
    await app.keyboard.press('Alt+ArrowDown');
    await expect(app.locator('.selpop[role="listbox"]')).toBeVisible();
    await app.keyboard.press('Escape');
    await expect(app.locator('.selpop[role="listbox"]')).toBeHidden();
    await expect(settings).toBeVisible();
  }
  await app.keyboard.press('Escape');
  await expect(settings).toBeHidden();
});

test('the Opening project status shows a spinner beside what is happening', async ({ app }) => {
  await app.evaluate(() => {
    const m = (window as unknown as { __waveMock: { projectOpen: unknown } }).__waveMock;
    m.projectOpen = () => new Promise(() => {});
  });
  await app.keyboard.press(`${MOD}+o`);
  const status = app.getByRole('status').filter({ hasText: 'Opening project…' });
  await expect(status).toBeVisible();
  await expect(status.locator('.spin')).toBeVisible();
});
