import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '../api/mock';
import type { PluginInfo } from '../api/types';

// FL's plugin wrapper: opening an external plugin shows its own window under Synkopa's header; the header's
// gear (or a plugin without a window) brings Synkopa's window. The header's pin is remembered per plugin.
vi.mock('../ui/Confirm', () => ({ confirmUnsaved: async () => 'discard', confirmRecover: async () => 'later' }));
const localStorageMap = new Map<string, string>();
beforeAll(() => {
  const noop = () => {};
  vi.stubGlobal('window', { addEventListener: noop, removeEventListener: noop, innerWidth: 1600, innerHeight: 900, setTimeout, setInterval, clearInterval });
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => localStorageMap.get(k) ?? null,
    setItem: (k: string, v: string) => void localStorageMap.set(k, v),
    removeItem: (k: string) => void localStorageMap.delete(k),
  });
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
  localStorageMap.clear();
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
  it('adding a plugin effect shows its own window under a header naming it and its insert, not Synkopa\'s window', async () => {
    const m = insert();
    expect(A.addPlugin(m.id, vst3('Vitalium'))).toBe(true);
    await sync();
    const fx = store.S.project.inserts.find(x => x.id === m.id)!.fx.at(-1)!;
    await vi.waitFor(() => expect(mock.editorLabel(fx.id)).toBeDefined());
    expect(mock.editorLabel(fx.id)).toEqual({ title: 'Vitalium', subtitle: `VST3 · ${store.insName(m)}` });
    expect(pw.editorOpen(fx.id)).toBe(true);
    expect(ui.U.plugs.some(p => p.fxId === fx.id)).toBe(false);
    // the gear: Synkopa's window for it, the plugin's own stays open
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

  it('a plugin with no window of its own opens Synkopa\'s window, and is remembered', async () => {
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

  it('a pin in the header is remembered per plugin: the next window opens pinned, unpinning is kept too', async () => {
    const m = insert();
    A.addPlugin(m.id, vst3('Vitalium'));
    await sync();
    const fx = store.S.project.inserts.find(x => x.id === m.id)!.fx.at(-1)!;
    await vi.waitFor(() => expect(pw.editorOpen(fx.id)).toBe(true));
    expect(mock.editorPinned(fx.id)).toBe(false);
    // the user clicks the header's pin; the app reports back and the choice is kept
    mock.on('plugin-pin', ({ id, pinned }) => pw.reportPin(id, pinned));
    mock.pressEditorPin(fx.id);
    await vi.waitFor(() => expect(localStorageMap.get('wave.pluginPin')).toContain('Vitalium'), { timeout: 2000 });
    await pw.closeEditor(fx.id);
    await pw.openEditor(fx.id);
    await vi.waitFor(() => expect(mock.editorPinned(fx.id)).toBe(true));
    // unpinning survives the same way
    mock.pressEditorPin(fx.id);
    await vi.waitFor(() => expect(JSON.parse(localStorageMap.get('wave.pluginPin') ?? '{}')).toEqual({}), { timeout: 2000 });
    await pw.closeEditor(fx.id);
    await pw.openEditor(fx.id);
    await vi.waitFor(() => expect(mock.editorPinned(fx.id)).toBe(false));
  });
});
