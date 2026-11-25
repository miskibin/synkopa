import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
const noop = () => {};
beforeAll(() => {
  const doc = { addEventListener: noop, removeEventListener: noop };
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout, document: doc });
  vi.stubGlobal('document', doc);
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
  vi.stubGlobal('requestAnimationFrame', (cb: () => void) => setTimeout(cb, 0));
});

let mock: MockBackend;
let store: typeof import('./store');
let pattern: typeof import('./pattern');

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('./store');
  pattern = await import('./pattern');
  await mock.on('state', store.replaceState); // main.tsx wires this in the app
  await mock.projectOpen();
  store.replaceState(await mock.getState());
});
afterEach(() => {
  mock.dispose();
});

it('cloneChannel keeps the loop, so the clone plays like the original', async () => {
  const pt = store.S.project.patterns[0];
  const hat = pattern.channels().find(c => c.name === 'Hat')!;
  const notes = [{ ch: hat.id, step: 0, len: 1, trig: true }, { ch: hat.id, step: 2, len: 1, trig: true }];
  await mock.applyOps([{ op: 'putPattern', pattern: { ...pt, notes, channelLoops: { [String(hat.id)]: 3 } } }]);
  store.replaceState(await mock.getState());

  await pattern.cloneChannel(pattern.channels().find(c => c.id === hat.id)!);

  const clone = pattern.channels().find(c => c.name === 'Hat 2')!;
  const p1 = store.S.project.patterns[0];
  expect(p1.notes.some(n => n.ch === clone.id)).toBe(true);
  expect(p1.notes.filter(n => n.ch === clone.id).map(n => n.step).sort((a, b) => a - b)).toEqual([0, 2]);
  expect(p1.channelLoops?.[String(clone.id)]).toBe(3);
  // the original keeps its own loop
  expect(p1.channelLoops?.[String(hat.id)]).toBe(3);
});
