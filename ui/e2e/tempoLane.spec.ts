import type { Page } from '@playwright/test';
import type { StateDto } from '../src/api/types';
import { drag, engine, expect, MOD, pollEngine, test } from './fixtures';

/** Page x of `beat` on the Playlist (the grid's box and the zoom). */
async function xOf(page: Page, beat: number) {
  const grid = (await page.locator('.gridArea').boundingBox())!;
  const bw: number = await page.evaluate(() => JSON.parse(localStorage.getItem('wave.layout') ?? '{}').bw ?? 22);
  return grid.x + beat * bw;
}
const tempoPts = (s: StateDto) => (s.project.tempoPoints ?? []).map(p => ({ beat: p.beat, bpm: p.bpm, ramp: p.ramp ?? false }));
const base = async (page: Page) => (await engine(page)).project.bpm;
const baseText = async (page: Page) => (await base(page)).toFixed(2);
const tpBox = async (page: Page) => (await page.locator('canvas.tpstrip').boundingBox())!;
const rulerY = async (page: Page) => {
  const b = (await page.locator('canvas.ruler:not(.mkstrip):not(.tpstrip)').boundingBox())!;
  return b.y + b.height / 2;
};

test.describe('Playlist tempo lane', () => {
  test('the strip shows the base tempo and a double-click adds a point', async ({ app }) => {
    await expect(app.locator('.tpnow')).toHaveText(await baseText(app));
    const b = await tpBox(app);
    await app.mouse.dblclick(await xOf(app, 8.05), b.y + 10);
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: await base(app), ramp: false }]);
    // the left edge: a point at beat 0 (the map plays it from the very start)
    await app.mouse.dblclick(await xOf(app, 0.05), b.y + 10);
    await pollEngine(app, tempoPts).toEqual([{ beat: 0, bpm: await base(app), ramp: false }, { beat: 8, bpm: await base(app), ramp: false }]);
  });

  test('dragging a point to the strip bottom reads base - 30, one undo step', async ({ app }) => {
    const b = await tpBox(app), yMid = b.y + 10, yBot = b.y + 17;
    await app.mouse.dblclick(await xOf(app, 8.05), yMid);
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: await base(app), ramp: false }]);
    await drag(app, { x: await xOf(app, 8.05) + 1, y: yMid }, { x: await xOf(app, 8.05) + 1, y: yBot });
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: (await base(app)) - 30, ramp: false }]);
    await expect.poll(async () => (await engine(app)).undoLabel).toBe('Move tempo point');
  });

  test('the point menu glides and types a value', async ({ app }) => {
    const b = await tpBox(app), yMid = b.y + 10;
    await app.mouse.dblclick(await xOf(app, 8.05), yMid);
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: await base(app), ramp: false }]);
    await app.mouse.click(await xOf(app, 8.05) + 1, yMid, { button: 'right' });
    await app.getByText('Glide to the next point').click();
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: await base(app), ramp: true }]);
    await app.mouse.click(await xOf(app, 8.05) + 1, yMid, { button: 'right' });
    await app.getByText('Type a BPM…').click();
    const box = app.locator('.menu input');
    await expect(box).toBeFocused();
    await box.fill('75');
    await box.press('Enter');
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: 75, ramp: true }]);
    // a value that is not a tempo is refused with a toast, the point unchanged
    await app.mouse.click(await xOf(app, 8.05) + 1, yMid, { button: 'right' });
    await app.getByText('Type a BPM…').click();
    await box.fill('abc');
    await box.press('Enter');
    await expect(app.locator('[data-readout]')).toContainText('Not a tempo');
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: 75, ramp: true }]);
    // a comma passes as the decimal separator
    await app.mouse.click(await xOf(app, 8.05) + 1, yMid, { button: 'right' });
    await app.getByText('Type a BPM…').click();
    await box.fill('70,5');
    await box.press('Enter');
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: 70.5, ramp: true }]);
  });

  test('right-click empty space adds at the base tempo; double-click deletes; undo', async ({ app }) => {
    const b = await tpBox(app), yMid = b.y + 10;
    await app.mouse.dblclick(await xOf(app, 8.05), yMid);
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: await base(app), ramp: false }]);
    await app.mouse.click(await xOf(app, 16.05), yMid, { button: 'right' });
    await app.getByText('Add tempo point here').click();
    await pollEngine(app, tempoPts).toEqual([
      { beat: 8, bpm: await base(app), ramp: false }, { beat: 16, bpm: await base(app), ramp: false },
    ]);
    await app.mouse.dblclick(await xOf(app, 16.05) + 1, yMid);
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: await base(app), ramp: false }]);
    // the store's undo flag trails the engine by a reply: a person's Ctrl+Z never lands that fast,
    // the test's can. The press is retried until the engine shows the restore (a premature press
    // only toasts "nothing to undo"; the poll stops at the first pass, so nothing extra is undone).
    await expect.poll(async () => {
      await app.keyboard.press(`${MOD}+z`);
      return tempoPts(await engine(app));
    }).toEqual([
      { beat: 8, bpm: await base(app), ramp: false }, { beat: 16, bpm: await base(app), ramp: false },
    ]);
  });

  test('the head readout follows the map at the playhead, through a glide', async ({ app }) => {
    const b = await tpBox(app), yMid = b.y + 10;
    await app.mouse.dblclick(await xOf(app, 8.05), yMid);
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: await base(app), ramp: false }]);
    // a drag to the bottom: base - 30, so the readout proves it reads the MAP, not the knob
    await drag(app, { x: await xOf(app, 8.05) + 1, y: yMid }, { x: await xOf(app, 8.05) + 1, y: b.y + 17 });
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: (await base(app)) - 30, ramp: false }]);
    await app.mouse.click(await xOf(app, 8.1), await rulerY(app));
    await expect(app.locator('.tpnow')).toHaveText(((await base(app)) - 30).toFixed(2));
    // a second point at the base tempo and a glide: halfway between, the readout glides too
    await app.mouse.click(await xOf(app, 16.05), yMid, { button: 'right' });
    await app.getByText('Add tempo point here').click();
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: (await base(app)) - 30, ramp: false }, { beat: 16, bpm: await base(app), ramp: false }]);
    await app.mouse.click(await xOf(app, 8.05) + 1, b.y + 17, { button: 'right' });
    await app.getByText('Glide to the next point').click();
    await pollEngine(app, tempoPts).toEqual([{ beat: 8, bpm: (await base(app)) - 30, ramp: true }, { beat: 16, bpm: await base(app), ramp: false }]);
    await app.mouse.click(await xOf(app, 12.1), await rulerY(app));
    await expect(app.locator('.tpnow')).toHaveText(((await base(app)) - 15).toFixed(2));
  });
});
