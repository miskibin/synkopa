// Patina: found in the picker by what people call this kind of effect, its window opens on the
// first preset with one section per module, a module knob and a preset change reach the engine,
// and Magnitude at zero leaves every module's own setting untouched.
import { engine, expect, pollEngine, test } from './fixtures';

const ins = (id: number) => (s: Awaited<ReturnType<typeof engine>>) => s.project.inserts.find(m => m.id === id)!;

test('add Patina from the picker, turn a module and change preset', async ({ app }) => {
  await app.locator('.strip[data-ins="1"] .nmrow').click();
  const n0 = ins(1)(await engine(app)).fx.length;
  await app.getByText('Add effect', { exact: true }).click();
  const search = app.getByPlaceholder('Search for an effect, plugin or tool…');
  // the name of the classic it stands in for finds it
  await search.fill('rc-20');
  await search.press('Enter');
  await pollEngine(app, s => ins(1)(s).fx.length).toBe(n0 + 1);
  const added = ins(1)(await engine(app)).fx[n0];
  expect(added.type).toBe('patina');
  // a new Patina starts on its first preset: a warm cassette with some noise
  expect(added.preset).toBe(0);
  expect(added.params.noise).toBeGreaterThan(0);

  const win = app.locator(`[data-fxwin="${added.id}"]`);
  await expect(win).toBeVisible();
  for (const title of ['Magnitude', 'Noise', 'Wobble', 'Drive', 'Digital', 'Wear']) {
    await expect(win.locator('.fxsec-h', { hasText: new RegExp(`^${title}$`, 'i') })).toBeVisible();
  }

  // a module's big knob moves its amount, one undo step
  const wobble = win.locator('.fxsec', { hasText: /^wobble/i }).locator('.knob.lg .dial');
  const before = (await engine(app)).project.inserts.find(m => m.id === 1)!.fx[n0].params.wobble;
  await wobble.hover();
  await app.mouse.wheel(0, -300);
  await pollEngine(app, s => ins(1)(s).fx[n0].params.wobble).not.toBe(before);

  // the noise type is a row of buttons
  await win.getByRole('button', { name: 'Dust', exact: true }).click();
  await pollEngine(app, s => ins(1)(s).fx[n0].params.ntype).toBe(1);

  // the next preset replaces the whole set
  await win.locator('.presetbar .arrow').last().click();
  await pollEngine(app, s => ins(1)(s).fx[n0].preset).toBe(1);
  await pollEngine(app, s => ins(1)(s).fx[n0].params.noise).toBe(0);

  if (process.env.PATINA_SHOT) await win.screenshot({ path: process.env.PATINA_SHOT });
});
