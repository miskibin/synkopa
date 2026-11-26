import { engine, expect, test } from './fixtures';

test('the app boots into the sketch project with its main views', async ({ app }) => {
  await expect(app.getByRole('menubar')).toBeVisible();
  for (const m of ['File', 'Edit', 'View', 'Help']) await expect(app.getByRole('menuitem', { name: m })).toBeVisible();
  await expect(app.getByRole('radiogroup', { name: 'Playlist tools' })).toBeVisible();
  // the status line: with no news, what the tool does
  await expect(app.locator('.tbstatus .hinttxt')).toContainText('Draw');
  // the Playlist lists the project's tracks, the Mixer its inserts
  const s = await engine(app);
  await expect(app.locator('.lane .nm')).toHaveText(s.project.tracks.map(t => t.name));
  await expect(app.locator('.strip[data-ins]')).toHaveCount(s.project.inserts.length);
  await expect(app.getByText(`${s.project.patterns.length} patterns`)).toBeVisible();
});

test('a fresh start offers the demo song, and opening it replaces the sketch', async ({ page }) => {
  await page.goto('/');
  const welcome = page.getByRole('dialog', { name: 'Welcome to Synkopa' });
  await expect(welcome).toBeVisible(); // offered once the project has opened
  const sketch = await engine(page);
  await welcome.getByRole('button', { name: 'Open demo project' }).click();
  await expect(welcome).toBeHidden();
  await expect.poll(async () => (await engine(page)).project.clips.length).toBeGreaterThan(sketch.project.clips.length);
  const demo = (await engine(page)).project;
  await expect(page.getByText(`${demo.patterns.length} patterns`)).toBeVisible();
  expect((await engine(page)).canUndo).toBe(false); // a new project starts its own history
});
