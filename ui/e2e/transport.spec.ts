import { VOL_UNITY } from '../src/api/types';
import { centre, drag, engine, expect, MOD, pollEngine, test } from './fixtures';

test.describe('Transport', () => {
  test('Main volume turns with a horizontal drag and double-click resets it', async ({ app }) => {
    const knob = app.locator('.mainvol .miniknob');
    const current = () => app.evaluate(() => {
      const mock = (window as unknown as { __waveMock: { programVolume(): Promise<number> } }).__waveMock;
      return mock.programVolume();
    });
    const from = await centre(knob);
    await drag(app, from, { x: from.x + 18, y: from.y });
    await expect.poll(current).toBeCloseTo(VOL_UNITY + 0.15, 5);
    await drag(app, from, { x: from.x - 36, y: from.y });
    await expect.poll(current).toBeCloseTo(VOL_UNITY - 0.15, 5);
    await knob.dblclick();
    await expect.poll(current).toBe(VOL_UNITY);
  });

  test('in SONG mode play runs the song clock, stop returns it to the start', async ({ app }) => {
    const play = app.locator('[data-act="play"]');
    const clock = app.getByText('1:1:1', { exact: true });
    await expect(clock).toBeVisible();
    await app.getByRole('button', { name: 'SONG', exact: true }).click();
    await expect(play).toHaveAttribute('title', 'Play / Stop (Space)');
    await play.click();
    await expect(play).toHaveClass(/\bplay\b/);
    await expect(clock).toBeHidden(); // the position moved on
    await app.getByRole('button', { name: /^Stop \(back to the start marker/ }).click();
    await expect(play).not.toHaveClass(/\bplay\b/);
    await expect(clock).toBeVisible();
    await pollEngine(app, s => s.transport.playing).toBe(false);
  });

  test('Space loops the pattern in PAT mode and plays the song in SONG mode', async ({ app }) => {
    const play = app.locator('[data-act="play"]');
    await expect(play).toHaveAttribute('title', /PAT mode/);
    await app.keyboard.press('Space');
    await expect(play).toHaveClass(/\bplay\b/);
    await expect(app.getByText(/Beat looping/)).toBeVisible();
    await app.keyboard.press('Space');
    await expect(play).not.toHaveClass(/\bplay\b/);
    await app.getByRole('button', { name: 'SONG', exact: true }).click();
    await expect(play).toHaveAttribute('title', 'Play / Stop (Space)');
    await app.keyboard.press('Space');
    await expect(play).toHaveClass(/\bplay\b/);
    await pollEngine(app, s => s.transport.playing).toBe(true);
    await app.keyboard.press('Space');
    await pollEngine(app, s => s.transport.playing).toBe(false);
  });
});

test('keyboard: tool keys, the command palette and undo from the keyboard', async ({ app }) => {
  const tools = app.getByRole('radiogroup', { name: 'Playlist tools' });
  await app.keyboard.press('d');
  await expect(tools.getByRole('radio', { name: /^Delete \(D\)/ })).toBeChecked();
  await app.keyboard.press('p');
  await expect(tools.getByRole('radio', { name: /^Draw \(P\)/ })).toBeChecked();

  // Ctrl+K: find "Add a bar" (current pattern) by typing, run it with Enter
  const pat = (await engine(app)).project.patterns[0];
  await app.keyboard.press(`${MOD}+k`);
  const palette = app.getByRole('dialog', { name: 'Find a command' });
  await expect(palette).toBeVisible();
  await expect(palette.getByRole('searchbox')).toBeFocused();
  await app.keyboard.type('add a bar');
  const first = palette.getByRole('option').first();
  await expect(first).toHaveAttribute('aria-selected', 'true');
  await expect(first).toContainText('Add a bar');
  await app.keyboard.press('Enter');
  await expect(palette).toBeHidden();
  await pollEngine(app, s => s.project.patterns[0].len).toBe(pat.len + 4);

  // the Edit menu's shortcut takes it back
  await app.keyboard.press(`${MOD}+z`);
  await pollEngine(app, s => s.project.patterns[0].len).toBe(pat.len);
});
