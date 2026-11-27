import { expect, engine, pollEngine, test } from './fixtures';
import type { Op } from '../src/api/types';

test('a trimmed audio clip follows global BPM without changing its source region', async ({ app }, testInfo) => {
  const apply = (ops: Op[]) => app.evaluate(ops => (window as unknown as { __waveMock: { applyOps(o: Op[]): Promise<unknown> } }).__waveMock.applyOps(ops), ops);
  await apply([{ op: 'setBpm', bpm: 80 }]);
  const ids = await app.evaluate(() => (window as unknown as { __waveMock: { importAudio(a: { paths: string[]; track: number; beat: number }): Promise<{ clipIds: number[] }> } }).__waveMock.importAudio({ paths: ['Vocal.wav'], track: 6, beat: 2 }));
  const id = ids.clipIds[0];
  const clip = (await engine(app)).project.clips.find(c => c.id === id)!;
  await apply([{ op: 'putClips', clips: [{ ...clip, offset: 1, len: 4 }] }]);
  await pollEngine(app, s => s.project.clips.find(c => c.id === id)?.len).toBe(4);

  const grid = (await app.locator('.gridArea').boundingBox())!;
  const bw: number = await app.evaluate(() => JSON.parse(localStorage.getItem('wave.layout') ?? '{}').bw ?? 22);
  const tracks = (await engine(app)).project.tracks;
  const top = tracks.slice(0, 6).reduce((y, t) => y + t.h, 0);
  await app.mouse.dblclick(grid.x + 3.5 * bw, grid.y + top + 8);
  const toggle = app.getByRole('button', { name: 'Follow project tempo', exact: true });
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await toggle.click();
  await pollEngine(app, s => s.project.clips.find(c => c.id === id)?.followTempo).toBe(true);
  await app.screenshot({ path: testInfo.outputPath('tempo-follow-before.png') });

  await apply([{ op: 'setBpm', bpm: 100 }]);
  await pollEngine(app, s => s.project.clips.find(c => c.id === id)?.stretch).toBeCloseTo(0.8);
  const changed = (await engine(app)).project.clips.find(c => c.id === id)!;
  expect([changed.start, changed.offset, changed.len]).toEqual([2, 1, 4]);
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await app.screenshot({ path: testInfo.outputPath('tempo-follow-after.png') });

  await toggle.click();
  await pollEngine(app, s => !!s.project.clips.find(c => c.id === id)?.followTempo).toBe(false);
  await apply([{ op: 'setBpm', bpm: 80 }]);
  await pollEngine(app, s => s.project.clips.find(c => c.id === id)?.len).toBeCloseTo(3.2);
  const free = (await engine(app)).project.clips.find(c => c.id === id)!;
  expect(free.offset).toBeCloseTo(0.8);
  expect(free.stretch).toBeCloseTo(0.8);
});
