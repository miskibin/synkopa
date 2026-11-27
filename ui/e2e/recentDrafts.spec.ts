import { expect, MOD, pollEngine, test } from './fixtures';
import type { MockBackend } from '../src/api/mock';

test('leave an unnamed beat and reopen it from File → Open recent', async ({ app }) => {
  await app.evaluate(async () => {
    const mock = (window as unknown as { __waveMock: MockBackend }).__waveMock;
    await mock.applyOps([{ op: 'setBpm', bpm: 139 }]);
  });
  await pollEngine(app, s => s.dirty).toBe(true);
  await app.keyboard.press(`${MOD}+Shift+N`);
  await app.getByRole('button', { name: "Don't save" }).click();
  await pollEngine(app, s => s.dirty).toBe(false);
  await app.getByRole('menuitem', { name: 'File', exact: true }).click();
  await app.getByRole('button', { name: 'Open recent' }).hover();
  await expect(app.getByText('Recent drafts', { exact: true })).toBeVisible();
  await app.getByRole('menuitem', { name: /139 BPM/ }).click();
  await pollEngine(app, s => [s.path, s.dirty, s.project.bpm]).toEqual([null, true, 139]);
});
