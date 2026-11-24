import type { Page } from '@playwright/test';
import type { Clip } from '../src/api/types';
import { drag, engine, expect, MOD, pollEngine, test } from './fixtures';

/** Page coordinates of beat `beat` on track `track`, from the grid's box, the zoom and the lane heights. */
async function at(page: Page, track: number, beat: number) {
  const grid = (await page.locator('.gridArea').boundingBox())!;
  const bw: number = await page.evaluate(() => JSON.parse(localStorage.getItem('wave.layout') ?? '{}').bw ?? 22);
  const tracks = (await engine(page)).project.tracks;
  const top = tracks.slice(0, track).reduce((y, t) => y + t.h, 0);
  return { x: grid.x + beat * bw, y: grid.y + top + tracks[track].h / 2 };
}
const clipsOn = (page: Page, track: number) => engine(page).then(s => s.project.clips.filter(c => c.track === track));
const only = (cs: Clip[]) => cs.map(c => ({ start: c.start, kind: c.kind }));

const emptyTip = (page: Page) => page.getByText('Choose a pattern in the Browser, then click an empty Playlist lane with Draw (P).', { exact: true });
async function newEmptyProject(page: Page) {
  await page.getByRole('menuitem', { name: 'File', exact: true }).click();
  await page.getByRole('menuitem', { name: /^New project\b/ }).click();
  await pollEngine(page, s => s.project.clips.length).toBe(0);
}

test.describe('Playlist', () => {
  test('a fresh Playlist explains how to place a pattern and clears the tip after the first clip', async ({ app }) => {
    await app.setViewportSize({ width: 1366, height: 768 });
    await expect(emptyTip(app)).toBeHidden();
    await newEmptyProject(app);
    await expect(emptyTip(app)).toBeVisible();
    const pattern = (await engine(app)).project.patterns[0];
    await app.getByRole('button', { name: 'Channel rack', exact: true }).click();
    const p = await at(app, 0, 0.5);
    await app.mouse.click(p.x, p.y);
    await pollEngine(app, s => s.project.clips.map(c => c.ref)).toEqual([pattern.id]);
    await expect(emptyTip(app)).toBeHidden();
    await app.keyboard.press(`${MOD}+z`);
    await pollEngine(app, s => s.project.clips.length).toBe(0);
    await expect(emptyTip(app)).toBeHidden();
  });

  test('dismissing the Playlist tip via keyboard remembers the choice for new projects', async ({ app }) => {
    await newEmptyProject(app);
    const dismiss = app.getByRole('button', { name: 'Dismiss Playlist tip' });
    await expect(dismiss).toBeVisible();
    await dismiss.focus();
    await dismiss.press('Enter');
    await expect(emptyTip(app)).toBeHidden();
    await app.reload();
    await newEmptyProject(app);
    await expect(emptyTip(app)).toBeHidden();
  });

  test('saving an empty project hides the Playlist tip', async ({ app }) => {
    await newEmptyProject(app);
    await expect(emptyTip(app)).toBeVisible();
    await app.getByRole('menuitem', { name: 'File', exact: true }).click();
    await app.getByRole('menuitem', { name: /^Save\s*Ctrl\+S$/ }).click();
    await pollEngine(app, s => s.path).not.toBeNull();
    await expect(emptyTip(app)).toBeHidden();
  });

  test('draw a clip, move it, delete it, with undo and redo', async ({ app }) => {
    const T = 6; // "Track 7": empty in the sketch
    expect(await clipsOn(app, T)).toEqual([]);
    const ref = (await engine(app)).project.patterns[0];

    // Draw tool click on an empty lane: a clip of the current pattern at the snapped beat
    const p = await at(app, T, 8.2);
    await app.mouse.click(p.x, p.y);
    await pollEngine(app, s => only(s.project.clips.filter(c => c.track === T))).toEqual([{ start: 8, kind: 'pat' }]);
    const [made] = await clipsOn(app, T);
    expect(made.ref).toBe(ref.id);

    // drag it 8 beats later
    const from = await at(app, T, made.start + made.len / 2);
    const to = await at(app, T, made.start + made.len / 2 + 8);
    await drag(app, from, to);
    await pollEngine(app, s => s.project.clips.find(c => c.id === made.id)?.start).toBe(16);
    await expect.poll(async () => (await engine(app)).undoLabel).toBe('Move clips');

    // Ctrl+Z puts it back, Ctrl+Y moves it again
    await app.keyboard.press(`${MOD}+z`);
    await pollEngine(app, s => s.project.clips.find(c => c.id === made.id)?.start).toBe(8);
    await app.keyboard.press(`${MOD}+y`);
    await pollEngine(app, s => s.project.clips.find(c => c.id === made.id)?.start).toBe(16);

    // the moved clip is selected: Delete removes it, undo brings it back
    await app.keyboard.press('Delete');
    await pollEngine(app, s => s.project.clips.some(c => c.id === made.id)).toBe(false);
    await app.keyboard.press(`${MOD}+z`);
    await pollEngine(app, s => s.project.clips.find(c => c.id === made.id)?.start).toBe(16);
  });

  test('clicking a pattern clip makes its pattern the current one', async ({ app }) => {
    const { patterns, clips } = (await engine(app)).project;
    const sel = app.locator('.patselbtn');
    await expect(sel).toContainText(patterns[0].name);
    const other = clips.find(c => c.kind === 'pat' && c.ref !== patterns[0].id)!;
    const name = patterns.find(p => p.id === other.ref)!.name;
    const p = await at(app, other.track, other.start + Math.min(1, other.len / 2));
    await app.mouse.click(p.x, p.y);
    await expect(sel).toContainText(name);
  });

  test('right-click deletes a clip; the Delete tool does too', async ({ app }) => {
    const before = (await engine(app)).project.clips;
    const victim = before.find(c => c.track === 0)!;
    const p = await at(app, 0, victim.start + 1);
    await app.mouse.click(p.x, p.y, { button: 'right' });
    await pollEngine(app, s => s.project.clips.some(c => c.id === victim.id)).toBe(false);

    await app.getByRole('radio', { name: /^Delete \(D\)/ }).click();
    await expect(app.getByRole('radio', { name: /^Delete \(D\)/ })).toBeChecked();
    const next = before.find(c => c.track === 1)!;
    const q = await at(app, 1, next.start + 1);
    await app.mouse.click(q.x, q.y);
    await pollEngine(app, s => s.project.clips.some(c => c.id === next.id)).toBe(false);
    expect((await engine(app)).project.clips).toHaveLength(before.length - 2);
  });

  test('right-click on the name bar of a clip opens its menu and keeps the clip; on its body it deletes', async ({ app }) => {
    const victim = (await engine(app)).project.clips.find(c => c.track === 0)!;
    const p = await at(app, 0, victim.start + 1);
    const tracks = (await engine(app)).project.tracks;
    // the name bar: the top 15 px of the clip (one px below the lane's top edge)
    const head = { x: p.x, y: p.y - tracks[0].h / 2 + 8 };
    await app.mouse.click(head.x, head.y, { button: 'right' });
    await expect(app.getByText('Rename pattern…', { exact: true })).toBeVisible();
    expect((await engine(app)).project.clips.some(c => c.id === victim.id)).toBe(true);
    await app.keyboard.press('Escape');
    await app.mouse.click(p.x, p.y, { button: 'right' });
    await pollEngine(app, s => s.project.clips.some(c => c.id === victim.id)).toBe(false);
  });

  test('dragging a lane header reorders the lanes with their clips; Esc cancels; undo restores', async ({ app }) => {
    const names = () => engine(app).then(s => s.project.tracks.map(t => t.name));
    const start = await names();
    const firstClips = (await clipsOn(app, 0)).map(c => c.id).sort();
    const lane = (i: number) => app.locator('.lanesCol .lane').nth(i);
    const b0 = (await lane(0).boundingBox())!, b2 = (await lane(2).boundingBox())!;
    const from = { x: b0.x + 60, y: b0.y + 12 }, to = { x: b0.x + 60, y: b2.y + b2.height - 4 };

    // lift past a few px: the lane shows lifted and the drop line where it lands; Esc puts it back
    await app.mouse.move(from.x, from.y);
    await app.mouse.down();
    await app.mouse.move(to.x, to.y, { steps: 10 });
    await expect(lane(0)).toHaveClass(/lifted/);
    await expect(app.locator('.laneDropLine')).toBeVisible();
    await app.keyboard.press('Escape');
    await expect(app.locator('.laneDropLine')).toBeHidden();
    await app.mouse.up();
    expect(await names()).toEqual(start);

    // a drag to below the third lane moves the first there, with its clips
    await drag(app, from, to, { steps: 10 });
    await expect.poll(names).toEqual([start[1], start[2], start[0], ...start.slice(3)]);
    expect((await clipsOn(app, 2)).map(c => c.id).sort()).toEqual(firstClips);
    await app.keyboard.press(`${MOD}+z`);
    await expect.poll(names).toEqual(start);
    expect((await clipsOn(app, 0)).map(c => c.id).sort()).toEqual(firstClips);
  });
});
