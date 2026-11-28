import type { Page } from '@playwright/test';
import type { Note } from '../src/api/types';
import { drag, engine, expect, MOD, pollEngine, test } from './fixtures';

// The roll is a canvas: read the notes from the engine and check the direction of each move.
const notesOf = async (page: Page, pattern: number, ch: number): Promise<Note[]> =>
  (await engine(page)).project.patterns.find(p => p.id === pattern)!.notes.filter(n => n.ch === ch);
/** Switches the UI language through the app's own i18n module (the one the page loaded). */
const setLang = (page: Page, lang: 'en' | 'pl') =>
  page.evaluate(async l => { const url = '/src/lib/i18n.ts'; ((await import(/* @vite-ignore */ url)) as { setLang(v: string): void }).setLang(l); }, lang);
const same = (a: Note) => (b: Note) => b.step === a.step && b.p === a.p;

test('Piano roll: selecting the same notes again leaves the grid and velocity lane settled', async ({ app }) => {
  await app.keyboard.press('F7');
  const roll = app.locator('.rollwin');
  const grid = roll.locator('.parea canvas');
  const lane = roll.locator('.lanecv canvas');
  await expect(grid).toBeVisible();
  await app.keyboard.press(`${MOD}+a`);
  await expect(roll.locator('.ptools .note')).toContainText(' sel.');

  const paints = await app.evaluate(async () => {
    const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    await frame();
    await frame();
    const canvases = [...document.querySelectorAll<HTMLCanvasElement>('.rollwin .parea canvas, .rollwin .lanecv canvas')];
    const contexts = canvases.map(c => c.getContext('2d')!);
    let paints = 0;
    const clear = contexts.map(g => g.clearRect);
    for (const [i, g] of contexts.entries()) g.clearRect = (...args) => { paints++; clear[i].apply(g, args); };
    try {
      for (let i = 0; i < 4; i++) {
        document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', code: 'KeyA', ctrlKey: true, repeat: true, bubbles: true, cancelable: true }));
        await frame();
      }
      return paints;
    } finally {
      for (const [i, g] of contexts.entries()) g.clearRect = clear[i];
    }
  });
  await expect(lane).toBeVisible();
  expect(paints).toBe(0);
});

test('Piano roll: draw, move, transpose and delete a note, with undo', async ({ app }) => {
  await app.keyboard.press('F7');
  const roll = app.locator('.rollwin');
  await expect(roll).toBeVisible();
  const chName = await roll.getByRole('combobox', { name: /Channel being edited/ }).locator('option:checked').textContent();
  const s0 = await engine(app);
  const pattern = s0.project.patterns[0];
  const pat = pattern.id;
  const ch = s0.project.channels!.find(c => c.name === chName)!.id;
  const before = await notesOf(app, pat, ch);

  const grid = (await roll.locator('.parea > canvas').boundingBox())!;
  const keyboardWidth = (await roll.locator('.rollruler .corner').boundingBox())!.width;
  // The opening view fits the pattern at 6–48 pixels per step. Aim inside the note body.
  const stepWidth = Math.max(6, Math.min(48, (grid.width - keyboardWidth) / (pattern.len * 4)));
  const xAtStep = (step: number) => grid.x + keyboardWidth + (step + 0.25) * stepWidth;
  const p0 = { x: xAtStep(5), y: grid.y + grid.height * 0.3 };
  await app.mouse.click(p0.x, p0.y);
  await pollEngine(app, s => s.project.patterns.find(p => p.id === pat)!.notes.filter(n => n.ch === ch).length).toBe(before.length + 1);
  const drawn = (await notesOf(app, pat, ch)).find(n => !before.some(same(n)))!;
  expect(drawn.len).toBeGreaterThan(0);

  // drag right: later, same key
  await drag(app, { x: xAtStep(drawn.step), y: p0.y }, { x: xAtStep(drawn.step + 3), y: p0.y });
  await expect.poll(async () => (await notesOf(app, pat, ch)).some(same(drawn))).toBe(false);
  const moved = (await notesOf(app, pat, ch)).find(n => !before.some(same(n)))!;
  expect(moved.step).toBeGreaterThan(drawn.step);
  expect(moved.p).toBe(drawn.p);

  // drag up: higher, same time
  const p1 = { x: xAtStep(moved.step), y: p0.y };
  await drag(app, p1, { x: p1.x, y: p1.y - 60 });
  await expect.poll(async () => (await notesOf(app, pat, ch)).some(same(moved))).toBe(false);
  const up = (await notesOf(app, pat, ch)).find(n => !before.some(same(n)))!;
  expect(up.p!).toBeGreaterThan(moved.p!);
  expect(up.step).toBe(moved.step);

  // right-click deletes it; undo brings it back where it was
  await app.mouse.click(p1.x, p1.y - 60, { button: 'right' });
  await pollEngine(app, s => s.project.patterns.find(p => p.id === pat)!.notes.filter(n => n.ch === ch).length).toBe(before.length);
  await app.keyboard.press(`${MOD}+z`);
  await expect.poll(async () => (await notesOf(app, pat, ch)).some(same(up))).toBe(true);
  // the notes that were there are untouched
  for (const n of before) expect((await notesOf(app, pat, ch)).some(same(n))).toBe(true);
});

test('Piano roll: the note properties box — knobs act at once, typed text survives, Reset, Accept, Esc', async ({ app }) => {
  await app.keyboard.press('F7');
  const roll = app.locator('.rollwin');
  await expect(roll).toBeVisible();
  const chName = await roll.getByRole('combobox', { name: /Channel being edited/ }).locator('option:checked').textContent();
  const s0 = await engine(app);
  const pat = s0.project.patterns[0].id;
  const ch = s0.project.channels!.find(c => c.name === chName)!.id;
  const before = await notesOf(app, pat, ch);
  const grid = await roll.locator('canvas').evaluateAll(cs => cs.map(c => c.getBoundingClientRect()).map(r => ({ x: r.x, y: r.y, width: r.width, height: r.height })).sort((a, b) => b.width * b.height - a.width * a.height)[0]);
  const p0 = { x: grid.x + grid.width * 0.25, y: grid.y + grid.height * 0.3 };
  await app.mouse.click(p0.x, p0.y);
  await pollEngine(app, s => s.project.patterns.find(p => p.id === pat)!.notes.filter(n => n.ch === ch).length).toBe(before.length + 1);
  const drawn = (await notesOf(app, pat, ch)).find(n => !before.some(same(n)))!;
  const vel = async () => (await notesOf(app, pat, ch)).find(same(drawn))?.v ?? 1;

  // a double-click opens the box on the note
  await app.mouse.dblclick(p0.x + 2, p0.y);
  const box = app.locator('.rollprops');
  await expect(box).toBeVisible();
  const reset = box.getByRole('button', { name: 'Reset' });
  await expect(reset).toBeDisabled();
  const start = box.locator('input[name=start]');
  await expect(start).toBeFocused();
  const shown = await start.inputValue();
  await expect(box.getByText('Bar:beat:16th', { exact: true })).toBeVisible();
  await expect(box.getByText('Note value', { exact: true })).toBeVisible();
  // one notation: the start reads like the transport (bar:beat:16th), the length like the Snap menu
  expect(shown).toMatch(/^\d+:[1-4]:[1-4](\.\d+)?$/);
  await expect(box.locator('input[name=len]')).toHaveValue(/^\d+(\.\d+)?\/\d+$/);

  // typed, not accepted yet: survives a knob turn, a language switch and an engine state
  await start.fill('2:1');
  await box.locator('input[name=len]').fill('1/12');
  const knob = box.locator('.knob').filter({ hasText: 'VEL' });
  const dial = (await knob.locator('.dial').boundingBox())!;
  await drag(app, { x: dial.x + dial.width / 2, y: dial.y + dial.height / 2 }, { x: dial.x + dial.width / 2, y: dial.y + dial.height / 2 + 60 });
  // the knob acts at once: the engine has the new velocity while the box is open
  await expect.poll(vel).toBeLessThan(0.9);
  await expect(knob.locator('.val')).not.toHaveText('100%');
  await expect(reset).toBeEnabled();
  await setLang(app, 'pl');
  await expect(box.locator('.rphead span').first()).not.toHaveText('Note properties');
  await setLang(app, 'en');
  await app.evaluate(() => (window as unknown as { __waveMock: { applyOps(o: unknown[]): Promise<unknown> } }).__waveMock.applyOps([{ op: 'setBpm', bpm: 131 }]));
  await pollEngine(app, s => s.project.bpm).toBe(131);
  await expect(start).toHaveValue('2:1');

  // Reset puts the velocity back, keeps the box open
  await reset.click();
  await expect.poll(vel).toBe(1);
  await expect(box).toBeVisible();

  // Accept applies the typed start and closes
  await box.getByRole('button', { name: 'Accept' }).click();
  await expect(box).toBeHidden();
  await expect.poll(async () => (await notesOf(app, pat, ch)).some(n => n.p === drawn.p && n.step === 16)).toBe(true);
  const moved = (await notesOf(app, pat, ch)).find(n => n.p === drawn.p && n.step === 16);
  expect(moved!.len).toBeCloseTo(4 / 3, 6);
  await app.keyboard.press(`${MOD}+z`);
  await expect.poll(async () => (await notesOf(app, pat, ch)).some(same(drawn))).toBe(true);

  // Esc closes as it was: a knob turn is undone with it
  await app.mouse.dblclick(p0.x + 2, p0.y);
  await expect(box).toBeVisible();
  await expect(box.locator('input[name=start]')).toHaveValue(shown);
  const dial2 = (await box.locator('.knob').filter({ hasText: 'VEL' }).locator('.dial').boundingBox())!;
  await drag(app, { x: dial2.x + dial2.width / 2, y: dial2.y + dial2.height / 2 }, { x: dial2.x + dial2.width / 2, y: dial2.y + dial2.height / 2 + 60 });
  await expect.poll(vel).toBeLessThan(0.9);
  await app.keyboard.press('Escape');
  await expect(box).toBeHidden();
  await expect.poll(vel).toBe(1);
});
