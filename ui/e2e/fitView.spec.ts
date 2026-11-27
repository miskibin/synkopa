import type { Locator, Page } from '@playwright/test';
import { engine, expect, test } from './fixtures';
import { TOTAL_BEATS } from '../src/api/types';

async function zoomIn(page: Page, bar: Locator) {
  await bar.hover();
  await page.keyboard.down('Control');
  try { await page.mouse.wheel(0, -900); } finally { await page.keyboard.up('Control'); }
}
const thumbWidth = (bar: Locator) => bar.locator('.zthumb').evaluate(el => el.getBoundingClientRect().width);
const thumbHeight = (bar: Locator) => bar.locator('.zthumb').evaluate(el => el.getBoundingClientRect().height);
async function checkThickness(editor: Locator) {
  const horizontal = editor.locator('.zbar.h.ov');
  const vertical = editor.locator('.zbar.v');
  await expect(vertical).toHaveCSS('width', '24px');
  await expect(horizontal).toHaveCSS('height', '24px');
  await expect(vertical.locator('.zthumb')).toHaveCSS('width', '24px');
}

test('Playlist: fit the song in time using the overview button', async ({ app }, testInfo) => {
  const editor = app.locator('.plwrap');
  const bar = editor.locator('.zbar.h.ov');
  const button = editor.getByRole('button', { name: 'Fit whole song (Shift+4)', exact: true });
  const scroll = editor.locator('.plscroll');
  await checkThickness(editor);
  const before = (await engine(app)).project;
  await button.click();
  await expect(scroll).toHaveJSProperty('scrollLeft', 0);
  // Wait for the queued zoom to land on the canvas and overview.
  const grid = editor.locator('.gridArea');
  const songEnd = Math.max(16, ...before.clips.map(c => c.start + c.len));
  const viewW = await scroll.evaluate(el => el.clientWidth - (el.querySelector('.plhead')?.getBoundingClientRect().width ?? 0));
  const gridWidth = () => grid.evaluate(el => el.getBoundingClientRect().width);
  const fit = Math.max(4, Math.min(1024, viewW / (Math.ceil(songEnd / 4) * 4)));
  await expect.poll(gridWidth).toBeCloseTo(TOTAL_BEATS * fit, -1);
  const fitted = await thumbWidth(bar);
  await zoomIn(app, bar);
  await expect.poll(() => thumbWidth(bar)).toBeLessThan(fitted * 0.9);
  await editor.screenshot({ path: testInfo.outputPath('fit-view-playlist-zoomed.png') });
  await button.click();
  await expect(scroll).toHaveJSProperty('scrollLeft', 0);
  await expect.poll(gridWidth).toBeCloseTo(TOTAL_BEATS * fit, -1);
  await expect.poll(() => thumbWidth(bar)).toBeGreaterThan(fitted * 0.9);
  await editor.screenshot({ path: testInfo.outputPath('fit-view-playlist.png') });
  expect((await engine(app)).project).toEqual(before);
});

test('Piano roll: fit pattern time and pitch range without editing notes', async ({ app }, testInfo) => {
  await app.keyboard.press('F7');
  const editor = app.locator('.rollwin');
  await expect(editor).toBeVisible();
  await checkThickness(editor);
  const bar = editor.locator('.zbar.h.ov');
  const vertical = editor.locator('.zbar.v');
  const button = editor.getByRole('button', { name: 'Fit whole pattern and notes (Shift+4)', exact: true });
  const before = (await engine(app)).project;
  await button.click();
  const fitted = await thumbWidth(bar);
  const pitchFit = await thumbHeight(vertical);
  const pitchPos = await vertical.getAttribute('aria-valuenow');
  await zoomIn(app, bar);
  await expect.poll(() => thumbWidth(bar)).toBeLessThan(fitted * 0.9);
  // Move away vertically, then fit must restore the note-centred view.
  await vertical.hover();
  await app.mouse.wheel(0, -1000);
  await expect.poll(() => vertical.getAttribute('aria-valuenow')).not.toBe(pitchPos);
  await editor.screenshot({ path: testInfo.outputPath('fit-view-piano-roll-zoomed.png') });
  await button.click();
  await expect.poll(() => thumbWidth(bar)).toBeCloseTo(fitted, 0);
  await expect.poll(() => thumbHeight(vertical)).toBeCloseTo(pitchFit, 0);
  await expect(vertical).toHaveAttribute('aria-valuenow', pitchPos!);
  await editor.screenshot({ path: testInfo.outputPath('fit-view-piano-roll.png') });
  expect((await engine(app)).project).toEqual(before);
});
