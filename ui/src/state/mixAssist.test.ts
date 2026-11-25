// The Mix assistant setting reaches the engine even when it is applied at startup, before the
// backend exists (prefs are applied first; the engine starts with the assistant on).
import { beforeAll, describe, expect, it, vi } from 'vitest';

beforeAll(() => { vi.stubGlobal('window', {}); });

describe('setAssistant', () => {
  it('switches the engine off when called before the backend is up', async () => {
    const backend = await import('../api/backend');
    const { setAssistant } = await import('./mixAssist');
    const done = setAssistant(false);
    const b = await backend.getBackend();
    await done;
    expect(await b.mixAssist('mix_assistant', {})).toEqual({ on: false });
    // callers racing at startup share one backend
    expect(await backend.getBackend()).toBe(b);
  });
});
