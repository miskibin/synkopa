import type { Page } from '@playwright/test';
import { expect, MOD, test } from './fixtures';

test.beforeEach(async ({ context }) => {
  // An old provider preference must not redirect the new action.
  await context.addInitScript(() => localStorage.setItem('wave.ask.service', 'deepwiki'));
  await context.route('https://chatgpt.com/**', route => route.fulfill({ body: 'ChatGPT destination' }));
});

async function destination(app: Page, act: () => Promise<void>): Promise<URL> {
  const tabOpened = app.context().waitForEvent('page');
  await act();
  const tab = await tabOpened;
  try {
    await tab.waitForURL('https://chatgpt.com/**');
    await expect(app.getByRole('dialog')).toHaveCount(0);
    return new URL(tab.url());
  } finally {
    await tab.close();
  }
}

test('the Help menu and F1 each open ChatGPT with a Synkopa starter prompt (no toolbar button)', async ({ app }) => {
  await app.setViewportSize({ width: 1366, height: 768 });
  await expect(app.getByRole('button', { name: 'Ask ChatGPT' })).toHaveCount(0);
  await app.getByRole('menuitem', { name: 'Help', exact: true }).click();
  const fromHelp = await destination(app, () => app.getByRole('menuitem', { name: /^Ask ChatGPT/ }).click());
  const fromF1 = await destination(app, () => app.keyboard.press('F1'));

  for (const url of [fromHelp, fromF1]) {
    const prompt = url.searchParams.get('q');
    expect(prompt).toContain('I use Synkopa ');
    expect(prompt).toContain('https://github.com/miskibin/synkopa');
    expect(prompt).toContain('Start by asking what I’m trying to do.');
  }
});

test('a question in the command palette goes straight to ChatGPT', async ({ app }) => {
  const question = 'Jak dodać efekt & przestawić go w mikserze?';
  await app.keyboard.press(`${MOD}+k`);
  const input = app.getByRole('searchbox', { name: 'Find a command' });
  await input.fill(question);
  await expect(app.getByRole('option', { name: /^Ask ChatGPT:/ })).toContainText(question);
  const url = await destination(app, () => input.press('Enter'));

  const prompt = url.searchParams.get('q');
  expect(prompt).toContain('Open in Synkopa right now: Playlist.');
  expect(prompt?.endsWith(`Question: ${question}`)).toBe(true);
});

test('a question beyond the link limit is copied and opens a blank ChatGPT chat', async ({ app, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  const question = 'ą'.repeat(400);
  await app.keyboard.press(`${MOD}+k`);
  await app.getByRole('searchbox', { name: 'Find a command' }).fill(question);
  const url = await destination(app, () => app.getByRole('option', { name: /^Ask ChatGPT:/ }).click());

  expect(url.href).toBe('https://chatgpt.com/');
  expect(await app.evaluate(() => navigator.clipboard.readText())).toContain(`Question: ${question}`);
  await expect(app.getByText('Prompt copied. Paste it into ChatGPT.')).toBeVisible();
});
