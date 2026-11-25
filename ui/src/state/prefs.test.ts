import { beforeEach, describe, expect, it, vi } from 'vitest';

// The interface scale is a saved choice: one the user picked is kept as it is, and a value the
// app no longer offers (a hand-edited or older profile) falls back to the default.
vi.mock('../api/backend', () => ({ be: () => ({ win: { setZoom: async () => true } }) }));
vi.mock('./actions', () => ({ recoveryTick: async () => {} }));
vi.mock('./ui', () => ({ U: {} }));
vi.mock('./fullscreen', () => ({ setFullscreen: async () => {} }));

const store = new Map<string, string>();
vi.stubGlobal('localStorage', {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => void store.set(k, v),
  removeItem: (k: string) => void store.delete(k),
});

async function boot(saved: object) {
  store.clear();
  store.set('wave.prefs', JSON.stringify(saved));
  vi.resetModules();
  const m = await import('./prefs');
  m.initPrefs();
  return m;
}

describe('interface scale preference', () => {
  beforeEach(() => store.clear());

  it('defaults to 100% and offers it among the scales', async () => {
    const m = await boot({});
    expect(m.prefs().uiScale).toBe(1);
    expect(m.UI_SCALES).toContain(1);
  });

  it('keeps a scale the user chose', async () => {
    const m = await boot({ uiScale: 1.25 });
    expect(m.prefs().uiScale).toBe(1.25);
  });

  it('falls back to the default for a scale that is not offered', async () => {
    for (const bad of [1.05, 0, -1, 'big', null]) {
      const m = await boot({ uiScale: bad });
      expect(m.prefs().uiScale).toBe(1);
    }
  });

  it('saves a change for the next start', async () => {
    const m = await boot({});
    m.setPref('uiScale', 1.1);
    expect(JSON.parse(store.get('wave.prefs')!).uiScale).toBe(1.1);
  });
});
