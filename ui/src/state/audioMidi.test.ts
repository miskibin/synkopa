import { afterEach, beforeAll, beforeEach, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import { libPath } from '../lib/factory';

// Convert to MIDI's dialog: a clip not analysed yet is analysed once, and the preview (and any
// setting changed meanwhile) waits for that analysis instead of starting its own.
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
let D: typeof import('./audioMidi');

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  store = await import('./store');
  const edit = await import('./edit');
  D = await import('./audioMidi');
  await mock.on('state', edit.onStateEvent);
  await mock.projectNew();
  await mock.importAudio({ paths: [libPath('triangle.flac')], track: 0, beat: 4 });
  store.replaceState(await mock.getState());
});
afterEach(() => mock.dispose());

it('previews only after the one analysis, and only for the latest settings', async () => {
  const clipId = store.S.project.clips.find(c => c.kind === 'audio')!.id;
  let analysing = 0;
  const log: string[] = [];
  const analyze = mock.pitchAnalyze.bind(mock);
  mock.pitchAnalyze = async (id: number) => { analysing++; log.push('analyze'); const r = await analyze(id); log.push('analysed'); return r; };
  const a2m = mock.audioToMidi.bind(mock);
  mock.audioToMidi = async a => { log.push('preview'); return a2m(a); };
  D.openAudioMidi(clipId);
  D.setAudioMidiOpts({ quantize: 4 }, true);
  D.setAudioMidiOpts({ quantize: 2 }, true);
  await vi.waitFor(() => expect(D.AMIDI.preview).not.toBeNull());
  // (the mock's preview reads the analysis again, from its cache; the dialog started one)
  expect(analysing).toBeGreaterThanOrEqual(1);
  // the preview asks only once the analysis is done, and only once (the latest settings)
  expect(log.slice(0, 3)).toEqual(['analyze', 'analysed', 'preview']);
  expect(log.filter(x => x === 'preview')).toHaveLength(1);
});
