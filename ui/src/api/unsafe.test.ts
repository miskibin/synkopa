import { beforeAll, describe, expect, it, vi } from 'vitest';
import { MockBackend } from './mock';
import { UNSAFE_PLUGIN, unsafeReason } from './types';

// the mock publishes itself on `window` for dev tools; node has none
beforeAll(() => { vi.stubGlobal('window', {}); });

describe('plugins refused by the safety check', () => {
  it('reads the reason without the marker, and nothing from other load errors', () => {
    expect(unsafeReason(`Vinyl: ${UNSAFE_PLUGIN}it crashed in Synkopa's safety check.`)).toBe("Vinyl: it crashed in Synkopa's safety check.");
    expect(unsafeReason('Amp (LV2): the LV2 plugin x is not installed')).toBeNull();
    expect(unsafeReason(undefined)).toBeNull();
  });

  it('the mock refuses a crashing plugin until it is loaded anyway, without an edit', async () => {
    const be = new MockBackend();
    const s0 = await be.getState();
    const ins = s0.project.inserts[1].id;
    const fxId = s0.project.nextId + 50;
    await be.applyOps([{ op: 'putFx', insertId: ins, fx: { id: fxId, type: 'plugin', bypass: false, preset: null, params: {},
      plugin: { format: 'lv2', path: '/usr/lib/lv2/crashy.lv2', cid: 'urn:example:lv2:crashy-vinyl', name: 'Crashy Vinyl', vendor: 'Example' } } }]);
    const before = await be.getState();
    expect(unsafeReason(before.fxErrors?.[fxId])).toContain('crashed');
    await be.pluginLoadAnyway(fxId);
    const after = await be.getState();
    expect(after.fxErrors?.[fxId]).toBeUndefined();
    expect(after.revision).toBe(before.revision);
    await expect(be.pluginLoadAnyway(999999)).rejects.toThrow();
  });
});
