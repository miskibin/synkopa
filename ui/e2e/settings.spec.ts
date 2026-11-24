import { engine, expect, MOD, pollEngine, test } from './fixtures';

test('Settings tabs start at the top after a pointer or keyboard switch', async ({ app }) => {
  await app.setViewportSize({ width: 960, height: 600 });
  await app.getByRole('menuitem', { name: 'File', exact: true }).click();
  await app.getByRole('menuitem', { name: 'Settings…', exact: true }).click();
  const settings = app.getByRole('dialog', { name: 'Settings' });
  const panel = settings.getByRole('tabpanel');
  const audio = settings.getByRole('tab', { name: 'Audio' });
  const look = settings.getByRole('tab', { name: 'Look' });
  const scrollTop = () => panel.evaluate(el => el.scrollTop);
  await expect(settings.getByRole('button', { name: 'Open demo project' })).toHaveCount(0);
  await audio.click();
  await panel.focus();
  await panel.press('End');
  await expect.poll(scrollTop).toBeGreaterThan(0);
  await look.click();
  await expect(panel).toHaveAccessibleName('Look');
  await expect.poll(scrollTop).toBe(0);

  await panel.focus();
  await panel.press('End');
  await expect.poll(scrollTop).toBeGreaterThan(0);
  await look.press('ArrowDown');
  await expect(audio).toBeFocused();
  await expect(panel).toHaveAccessibleName('Audio');
  await expect.poll(scrollTop).toBe(0);
});

test('File opens the demo only after confirming unsaved changes', async ({ app }) => {
  const original = (await engine(app)).project;
  await app.keyboard.press(`${MOD}+k`);
  const palette = app.getByRole('dialog', { name: 'Find a command' });
  await palette.getByRole('searchbox').fill('add a bar');
  await palette.getByRole('searchbox').press('Enter');
  await pollEngine(app, s => s.project.patterns[0].len).toBe(original.patterns[0].len + 4);
  const openDemo = async () => {
    await app.getByRole('menuitem', { name: 'File', exact: true }).click();
    await app.getByRole('menuitem', { name: 'Open demo project', exact: true }).click();
    await expect(app.getByText(/Save before opening the demo project/)).toBeVisible();
  };
  await openDemo();
  await app.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(app.getByText('Unsaved changes', { exact: true })).toBeHidden();
  expect((await engine(app)).project.patterns[0].len).toBe(original.patterns[0].len + 4);

  await openDemo();
  await app.getByRole('button', { name: "Don't save", exact: true }).click();
  await pollEngine(app, s => s.project.clips.length).toBeGreaterThan(original.clips.length);
});
