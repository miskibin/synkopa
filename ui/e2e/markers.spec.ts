import type { Page } from '@playwright/test';
import type { StateDto } from '../src/api/types';
import { drag, engine, expect, MOD, pollEngine, test } from './fixtures';

/** Page x of `beat` on the Playlist (the grid's box and the zoom). */
async function xOf(page: Page, beat: number) {
  const grid = (await page.locator('.gridArea').boundingBox())!;
  const bw: number = await page.evaluate(() => JSON.parse(localStorage.getItem('wave.layout') ?? '{}').bw ?? 22);
  return grid.x + beat * bw;
}
const marks = (s: StateDto) => (s.project.markers ?? []).map(m => ({ at: m.at, name: m.name }));
const markers = (page: Page) => engine(page).then(marks);
const stripY = async (page: Page) => { const b = (await page.locator('canvas.mkstrip').boundingBox())!; return b.y + b.height / 2; };
const rulerY = async (page: Page) => { const b = (await page.locator('canvas.ruler:not(.mkstrip)').boundingBox())!; return b.y + b.height / 2; };
const readout = (page: Page) => page.locator('[data-readout]');

test.describe('Playlist markers', () => {
  test('add with Alt+T, name, move (Esc cancels), loop a section, jump, delete, undo', async ({ app }) => {
    await expect(app.locator('canvas.mkstrip')).toHaveCount(0); // no strip until the song has markers

    // Alt+T at the start position (bar 1): the name box opens with the default name selected
    await app.keyboard.press('Alt+t');
    const box = app.locator('.menu input');
    await expect(box).toBeFocused();
    await box.fill('Intro');
    await box.press('Enter');
    await pollEngine(app, marks).toEqual([{ at: 0, name: 'Intro' }]);
    await expect(app.locator('canvas.mkstrip')).toHaveCount(1);

    // a click on the ruler at bar 5 sets the start position; Alt+T adds the next marker there
    await app.mouse.click(await xOf(app, 16.1), await rulerY(app));
    await app.keyboard.press('Alt+t');
    await box.fill('Hook');
    await box.press('Enter');
    await pollEngine(app, marks).toEqual([{ at: 0, name: 'Intro' }, { at: 16, name: 'Hook' }]);
    await expect.poll(async () => (await engine(app)).undoLabel).toBe('Rename marker');

    // drag the hook to bar 9 (snapped); one undo step
    const y = await stripY(app);
    await drag(app, { x: await xOf(app, 16) + 3, y }, { x: await xOf(app, 32.2) + 3, y });
    await pollEngine(app, marks).toEqual([{ at: 0, name: 'Intro' }, { at: 32, name: 'Hook' }]);
    await expect.poll(async () => (await engine(app)).undoLabel).toBe('Move marker');

    // a drag that Esc cancels leaves it where it was
    await app.mouse.move(await xOf(app, 32) + 3, y);
    await app.mouse.down();
    await app.mouse.move(await xOf(app, 48) + 3, y, { steps: 6 });
    await app.keyboard.press('Escape');
    await app.mouse.up();
    await expect(readout(app)).toContainText('Marker move cancelled');
    expect(await markers(app)).toEqual([{ at: 0, name: 'Intro' }, { at: 32, name: 'Hook' }]);

    // double-click the intro: its section (up to the hook) becomes the loop
    await app.mouse.dblclick(await xOf(app, 0) + 3, y);
    await pollEngine(app, s => s.project.loop).toEqual({ a: 0, b: 32 });

    // jump keys: from the intro to the hook and back
    await app.keyboard.press('Alt+ArrowRight');
    await expect(readout(app)).toContainText('Hook');
    await app.keyboard.press('Alt+ArrowLeft');
    await expect(readout(app)).toContainText('Intro');
    await app.keyboard.press('Alt+ArrowLeft');
    await expect(readout(app)).toContainText('No marker before');

    // right-click the hook: rename, then delete; undo brings it back
    await app.mouse.click(await xOf(app, 32) + 3, y, { button: 'right' });
    await app.getByText('Rename…').click();
    await box.fill('Drop');
    await box.press('Enter');
    await pollEngine(app, marks).toEqual([{ at: 0, name: 'Intro' }, { at: 32, name: 'Drop' }]);
    await app.mouse.click(await xOf(app, 32) + 3, y, { button: 'right' });
    await app.getByText('Delete marker').click();
    await pollEngine(app, marks).toEqual([{ at: 0, name: 'Intro' }]);
    await app.keyboard.press(`${MOD}+z`);
    await pollEngine(app, marks).toEqual([{ at: 0, name: 'Intro' }, { at: 32, name: 'Drop' }]);
  });

  test('right-click on the ruler adds a marker where it was clicked', async ({ app }) => {
    await app.mouse.click(await xOf(app, 8.1), await rulerY(app), { button: 'right' });
    await app.getByText('Add marker here').click();
    const box = app.locator('.menu input');
    await expect(box).toBeFocused();
    await box.press('Escape'); // keeps the default name
    await pollEngine(app, marks).toEqual([{ at: 8, name: 'Marker 1' }]);
  });
});
