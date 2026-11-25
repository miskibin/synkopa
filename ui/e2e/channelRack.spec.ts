import type { Page } from '@playwright/test';
import { centre, drag, engine, expect, MOD, pollEngine, test } from './fixtures';

const stepsOf = async (page: Page, pattern: number, ch: number) =>
  (await engine(page)).project.patterns.find(p => p.id === pattern)!.notes.filter(n => n.ch === ch).map(n => n.step).sort((a, b) => a - b);

test('Channel rack: a click lights a step (again keeps it, FL), right-click clears it, a drag paints, undo restores', async ({ app }) => {
  await app.getByRole('button', { name: 'Channel rack' }).click();
  const rack = app.locator('.rackwin');
  await expect(rack).toBeVisible();
  await expect(app.getByRole('button', { name: 'Channel rack' })).toHaveAttribute('aria-pressed', 'true');

  const s = await engine(app);
  const pat = s.project.patterns[0].id;
  const snare = s.project.channels!.find(c => c.name === 'Snare')!;
  const before = await stepsOf(app, pat, snare.id);
  expect(before).not.toContain(0);

  // the step grid is a canvas: the first step of the Snare row is at its left edge, level with the row's name
  const steps = await rack.locator('canvas').evaluateAll(cs => cs.map(c => c.getBoundingClientRect()).map(r => ({ x: r.x, w: r.width, h: r.height })).sort((a, b) => b.w * b.h - a.w * a.h)[0]);
  const row = await centre(rack.getByRole('button', { name: 'Drums Snare' }));
  const step0 = { x: steps.x + 6, y: row.y };

  await app.mouse.click(step0.x, step0.y);
  await expect.poll(() => stepsOf(app, pat, snare.id)).toEqual([0, ...before]);
  // a left press on a lit step keeps it: a drag from it extends a run, never erases one
  await app.mouse.click(step0.x, step0.y);
  // edits reach the engine in order: once a later one (Hat, step 0) has landed, so has this press
  const hat = s.project.channels!.find(c => c.name === 'Hat')!;
  const hatBefore = await stepsOf(app, pat, hat.id);
  expect(hatBefore).not.toContain(0);
  await app.mouse.click(step0.x, (await centre(rack.getByRole('button', { name: 'Drums Hat' }))).y);
  await expect.poll(() => stepsOf(app, pat, hat.id)).toEqual([0, ...hatBefore]);
  expect(await stepsOf(app, pat, snare.id)).toEqual([0, ...before]);

  await app.mouse.click(step0.x, step0.y, { button: 'right' });
  await expect.poll(() => stepsOf(app, pat, snare.id)).toEqual(before);
  await app.keyboard.press(`${MOD}+z`);
  await expect.poll(() => stepsOf(app, pat, snare.id)).toEqual([0, ...before]);
  await pollEngine(app, st => st.canRedo).toBe(true);

  // a right-drag clears a run; a left-drag paints one
  const kick = s.project.channels!.find(c => c.name === 'Kick')!;
  const kickRow = await centre(rack.getByRole('button', { name: 'Drums Kick' }));
  await drag(app, { x: steps.x + 6, y: kickRow.y }, { x: steps.x + steps.w - 6, y: kickRow.y }, { button: 'right' });
  await expect.poll(() => stepsOf(app, pat, kick.id)).toEqual([]);
  await drag(app, step0, { x: steps.x + steps.w * 0.2, y: step0.y });
  await expect.poll(async () => (await stepsOf(app, pat, snare.id)).length).toBeGreaterThan(before.length + 1);

  // Escape closes the window
  await rack.click({ position: { x: 20, y: 10 } });
  await app.keyboard.press('Escape');
  await expect(rack).toBeHidden();
  await expect(app.getByRole('button', { name: 'Channel rack' })).toHaveAttribute('aria-pressed', 'false');
});

test('Channel rack: keyboard shifts wrap inside the selected channel loop', async ({ app }) => {
  await app.getByRole('button', { name: 'Channel rack' }).click();
  const rack = app.locator('.rackwin');
  const s = await engine(app);
  const pat = s.project.patterns[0];
  const hat = s.project.channels!.find(c => c.name === 'Hat')!;
  // Seed a three-step cell with a hit at each edge. The shortcuts must rotate it, not the whole bar.
  await app.evaluate(async ({ pattern, ch }) => {
    const mock = (window as unknown as { __waveMock: import('../src/api/mock').MockBackend }).__waveMock;
    await mock.applyOps([{ op: 'putPattern', pattern: {
      ...pattern,
      notes: [...pattern.notes.filter(n => n.ch !== ch), { ch, step: 0, len: 1, trig: true }, { ch, step: 2, len: 1, trig: true }],
      channelLoops: { ...pattern.channelLoops, [String(ch)]: 3 },
    } }]);
  }, { pattern: pat, ch: hat.id });
  await expect.poll(() => stepsOf(app, pat.id, hat.id)).toEqual([0, 2]);
  const grid = await rack.locator('canvas').first().boundingBox();
  const row = await centre(rack.getByRole('button', { name: 'Drums Hat' }));
  await app.mouse.click(grid!.x + 6, row.y);
  await app.keyboard.press(`${MOD}+Shift+ArrowRight`);
  await expect.poll(() => stepsOf(app, pat.id, hat.id)).toEqual([0, 1]);
  await app.keyboard.press(`${MOD}+Shift+ArrowLeft`);
  await expect.poll(() => stepsOf(app, pat.id, hat.id)).toEqual([0, 2]);
});
