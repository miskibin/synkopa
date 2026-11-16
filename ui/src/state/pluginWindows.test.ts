import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import type { PluginInfo } from '../api/types';

// FL's plugin wrapper: opening an external plugin shows its own window under WAVE's header; the header's
// gear (or a plugin without a window) brings WAVE's window.
vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout, setInterval, clearInterval });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: noop, removeItem: noop });
  vi.stubGlobal('document', { querySelector: () => null, activeElement: null });
  vi.stubGlobal('requestAnimationFrame', (f: () => void) => setTimeout(f, 0));
});

let mock: MockBackend;
let A: typeof import('./actions');
let store: typeof import('./store');
let ui: typeof import('./ui');
let pw: typeof import('./pluginWindows');
const settle = () => new Promise(r => setTimeout(r, 30));
const sync = async () => { await settle(); store.replaceState(await mock.getState()); };

beforeEach(async () => {
  vi.resetModules();
  const backend = await import('../api/backend');
  mock = (await backend.getBackend()) as MockBackend;
  A = await import('./actions');
  store = await import('./store');
  ui = await import('./ui');
  pw = await import('./pluginWindows');
  await sync();
});

const vst3 = (name: string): PluginInfo => ({ format: 'vst3', path: `/vst3/${name}.vst3`, cid: name.padEnd(32, '0').slice(0, 32), name, vendor: 'Test', category: 'Fx' });
const insert = () => store.S.project.inserts.find(m => m.id !== 0)!;

describe('plugin windows (FL wrapper)', () => {
  it('adding a plugin effect shows its own window under a header naming it and its insert, not WAVE\'s window', async () => {
    const m = insert();
    expect(A.addPlugin(m.id, vst3('Vitalium'))).toBe(true);
    await sync();
    const fx = store.S.project.inserts.find(x => x.id === m.id)!.fx.at(-1)!;
    await vi.waitFor(() => expect(mock.editorLabel(fx.id)).toBeDefined());
    expect(mock.editorLabel(fx.id)).toEqual({ title: 'Vitalium', subtitle: `VST3 · ${store.insName(m)}` });
    expect(pw.editorOpen(fx.id)).toBe(true);
    expect(ui.U.plugs.some(p => p.fxId === fx.id)).toBe(false);
    // the gear: WAVE's window for it, the plugin's own stays open
    A.openPlug(fx.id, true);
    expect(ui.U.plugs.some(p => p.fxId === fx.id)).toBe(true);
    expect(pw.editorOpen(fx.id)).toBe(true);
  });

  it('a second click closes the plugin window; one closed with its own X is noticed', async () => {
    const m = insert();
    A.addPlugin(m.id, vst3('Surge XT'));
    await sync();
    const id = store.S.project.inserts.find(x => x.id === m.id)!.fx.at(-1)!.id;
    await vi.waitFor(() => expect(pw.editorOpen(id)).toBe(true));
    pw.togglePlugin(id, () => { throw new Error('no fallback expected'); });
    await vi.waitFor(() => expect(mock.editorLabel(id)).toBeUndefined());
    expect(pw.editorOpen(id)).toBe(false);
    await pw.showPlugin(id, () => {});
    expect(pw.editorOpen(id)).toBe(true);
    mock.closeEditorWindow(id);
    await vi.waitFor(() => expect(pw.editorOpen(id)).toBe(false), { timeout: 2000 });
  });

  it('a plugin with no window of its own opens WAVE\'s window, and is remembered', async () => {
    const m = insert();
    A.addPlugin(m.id, vst3('WAVE Demo Synth'));
    await sync();
    const f = store.S.project.inserts.find(x => x.id === m.id)!.fx.at(-1)!;
    await vi.waitFor(() => expect(ui.U.plugs.some(p => p.fxId === f.id)).toBe(true));
    expect(pw.knownWithoutEditor(f.plugin)).toBe(true);
    expect(pw.editorOpen(f.id)).toBe(false);
    let fell = false;
    await pw.showPlugin(f.id, () => { fell = true; });
    expect(fell).toBe(true);
  });

  it('a plugin instrument\'s header names the channel', async () => {
    const pattern = await import('./pattern');
    pattern.addPlugin(vst3('Nekobi'));
    await sync();
    const c = pattern.channels().at(-1)!;
    expect(pw.pluginLabel(c.id)).toEqual({ title: c.name, subtitle: 'VST3 instrument' });
  });
});
