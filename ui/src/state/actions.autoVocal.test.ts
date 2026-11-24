import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { libPath } from '../lib/factory';

// The Auto vocal button's journey on the mock: one shared command does the whole job the
// app used to compose by hand (plan, bake, chain), the mixer lands on the insert, and the
// readout tells what happened — or asks for an insert when the track still plays the master.
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
let ui: typeof import('./ui');
let edit: typeof import('./edit');
let A: typeof import('./actions');
let M: typeof import('./automation');

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('./store');
  ui = await import('./ui');
  edit = await import('./edit');
  A = await import('./actions');
  M = await import('./automation');
  await mock.on('state', edit.onStateEvent); // main.tsx wires this in the app
  await mock.projectOpen();
  store.replaceState(await mock.getState());
});
afterEach(() => {
  mock.dispose(); // the backend's interval outlives resetModules otherwise
  M.setBeatSourceForTests(null);
});

/** An imported audio clip on a track routed to insert 1, plus a second clip as the beat. */
async function vocalTrack(beat = true): Promise<number> {
  const take = await mock.importAudio({ paths: [libPath('triangle.flac')], track: 0, beat: 0 });
  if (beat) await mock.importAudio({ paths: [libPath('kick-808-short.flac')], track: 1, beat: 4 });
  const p = (await mock.getState()).project;
  const clip = p.clips.find(c => c.kind === 'audio' && c.ref === take.sourceIds[0])!.id;
  await mock.applyOps([{ op: 'putTrack', track: { ...p.tracks[0], insert: 1 } }]);
  store.replaceState(await mock.getState());
  return clip;
}

it('one command does the whole journey: the chain lands, the mixer follows, the readout tells', async () => {
  const clip = await vocalTrack();

  await A.autoVocal(clip);

  const p = store.S.project;
  const fx = p.inserts.find(m => m.id === 1)?.fx ?? [];
  expect(fx.length, 'the calibrated chain is on the insert').toBeGreaterThanOrEqual(5);
  expect(fx.some(f => f.type === 'rider'), 'the rider is baked into the audio, not live on the insert').toBe(false);
  expect(ui.U.mixSel).toBe(1);
  expect(ui.U.readout).toMatch(/Auto (vocal|wokal): /);
  expect(ui.U.readout).toMatch(/key|klucz|no key|bez tonacji/);
}, 15000);

it('without an insert the button asks for one instead of running', async () => {
  const take = await mock.importAudio({ paths: [libPath('triangle.flac')], track: 0, beat: 0 });
  const p = (await mock.getState()).project;
  const clip = p.clips.find(c => c.kind === 'audio' && c.ref === take.sourceIds[0])!.id;
  // the sketch routes track 0 to insert 1: send it to the master (insert 0) for this test
  const before = store.S.project.inserts.find(m => m.id === 1)?.fx.map(f => f.id) ?? [];
  await mock.applyOps([{ op: 'putTrack', track: { ...p.tracks[0], insert: 0 } }]);
  store.replaceState(await mock.getState());

  await A.autoVocal(clip);

  expect(ui.U.readout).toMatch(/assign the track|przypisz ścieżkę/);
  expect(store.S.project.inserts.find(m => m.id === 1)?.fx.map(f => f.id) ?? []).toEqual(before);
});
