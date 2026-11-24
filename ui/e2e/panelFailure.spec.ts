// A window that fails to render (ui/PanelBoundary.tsx): the failure shows in that window, whose
// frame still stands, the rest of the app keeps working (transport, another window, Save), and
// once the edit that broke it is undone, Try again builds it back, working, where it was.
//
// The failure comes from the engine's state, not from the page's code: an edit leaves a lyrics
// page without its text (the engine's validation lets it through), which the Notebook cannot lay out.
import { expect, test as base } from '@playwright/test';
import type { StateDto } from '../src/api/types';
import { MOD, pollEngine } from './fixtures';

/** The booted app (welcome closed), with the page's uncaught errors collected instead of failing the test. */
const test = base.extend<{ errors: string[] }>({
  errors: async ({ page }, use) => {
    const errors: string[] = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.goto('/');
    const welcome = page.getByRole('dialog', { name: 'Welcome to Synkopa' });
    await welcome.getByRole('button', { name: 'Close' }).click();
    await expect(welcome).toBeHidden();
    await use(errors);
  },
});

const NL = String.fromCharCode(10);
const pages = (s: StateDto) => s.project.notebook?.pages ?? [];

test('a window that fails to render shows it in place, the app keeps working, it comes back working', async ({ page, errors }) => {
  // the lyrics notebook (F8) with a page of text
  await page.keyboard.press('F8');
  const nb = page.locator('.nbwin');
  await expect(nb).toBeVisible();
  await nb.getByRole('button', { name: '+ New page' }).click();
  await pollEngine(page, s => pages(s).length).toBe(1);
  const ta = nb.locator('textarea');
  await ta.fill('first line');
  await pollEngine(page, s => pages(s)[0]?.text).toBe('first line');
  const box0 = (await nb.boundingBox())!;

  // an edit leaves the page without its text, which the window cannot lay out
  await page.evaluate(async () => {
    const m = (window as unknown as { __waveMock: { getState(): Promise<StateDto>; applyOps(ops: unknown[], o: { label: string }): Promise<unknown> } }).__waveMock;
    const pg = (await m.getState()).project.notebook!.pages[0];
    await m.applyOps([{ op: 'putNotebook', notebook: { pages: [{ ...pg, text: null }] } }], { label: 'Lyrics' });
  });
  const alert = nb.getByRole('alert');
  await expect(alert).toContainText('Notebook stopped because of an error');
  await expect(alert).toContainText('saving still works');
  // in its place, with a title bar that still closes it
  expect((await nb.boundingBox())!).toEqual(box0);
  await expect(nb.locator('.winhd .closebtn')).toBeVisible();
  await expect.poll(() => errors.length).toBeGreaterThan(0); // still reported as uncaught

  // the rest works: the transport plays and stops (Space, as the keyboard is nowhere in particular)
  const play = page.locator('[data-act="play"]');
  await page.keyboard.press('Space');
  await expect(play).toHaveClass(/(^| )play( |$)/);
  await page.keyboard.press('Space');
  await expect(play).not.toHaveClass(/(^| )play( |$)/);
  // another window opens, works and closes
  await page.keyboard.press('F7');
  const roll = page.locator('.rollwin');
  await expect(roll).toBeVisible();
  await expect(roll.getByRole('alert')).toHaveCount(0);
  await roll.getByRole('button', { name: 'Close (Esc)' }).click();
  await expect(roll).toBeHidden();
  // Save writes the project
  await pollEngine(page, s => s.dirty).toBe(true);
  await page.keyboard.press(`${MOD}+s`);
  await pollEngine(page, s => s.dirty).toBe(false);

  // undo restores the text; Try again builds the window afresh from it, in the same place
  await page.keyboard.press(`${MOD}+z`);
  await pollEngine(page, s => pages(s)[0]?.text).toBe('first line');
  // (the window had already copied the bad text into its own state, so it stays failed until rebuilt)
  const retry = page.locator('.nbwin').getByRole('button', { name: 'Try again' });
  await expect(retry).toBeVisible();
  await retry.click();
  await expect(nb.getByRole('alert')).toHaveCount(0);
  await expect(ta).toHaveValue('first line');
  expect((await nb.boundingBox())!).toEqual(box0);
  // and it edits again
  await ta.fill('first line' + NL + 'second line');
  await pollEngine(page, s => pages(s)[0]?.text).toBe('first line' + NL + 'second line');

  // every uncaught error was this failure (the window's render, and an effect of it reading the same text): nothing else broke
  for (const e of errors) expect(e).toMatch(/^Cannot read properties of null \(reading '(split|length)'\)$/);
});
