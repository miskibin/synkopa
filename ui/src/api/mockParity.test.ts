// deep-3: the mock's command layer against the engine's contract. The ops.json rows that
// `opsParity.test.ts` replays through the strict port are replayed here through the whole
// MockBackend command path: accept/reject, the exact error strings, revision and undo
// semantics (the engine skips undo/revision/state events for an unchanged batch).
// The mock builds its initial project from `window.__waveFixture` (the perf harness hook);
// node has no window, so it is stubbed per test.
import { beforeAll, describe, expect, it, vi } from 'vitest';
import fixture from '../lib/__fixtures__/ops.json';
import { close } from '../lib/closeCompare';
import { projectEq, type Json } from '../lib/projectEq';
import { MockBackend } from './mock';
import type { Op, Project } from './types';

type Row = { name: string; base: string; ops: Op[]; result: Project | null; error: string | null };
const bases = fixture.bases as unknown as Record<string, Project>;
const rows = fixture.cases as unknown as Row[];

beforeAll(() => {
  vi.stubGlobal('window', {});
});

function withBase(key: string): MockBackend {
  vi.stubGlobal('window', { __waveFixture: structuredClone(bases[key]) });
  return new MockBackend();
}

describe('MockBackend parity with the engine (ops.json)', () => {
  for (const row of rows) {
    it(`applyOps: ${row.name}`, async () => {
      const be = withBase(row.base);
      if (row.error) {
        let msg = '';
        try {
          await be.applyOps(row.ops);
        } catch (e) {
          msg = String(e).replace(/^Error: /, '');
        }
        expect(msg).toBe(row.error);
        const st = await be.getState();
        close(st.project, bases[row.base]);
        expect(st.revision).toBe(1);
        expect(st.canUndo).toBe(false);
        return;
      }
      const { revision } = await be.applyOps(row.ops);
      const st = await be.getState();
      expect(st.revision).toBe(revision);
      close(st.project, row.result);
      expect(st.canUndo).toBe(!projectEq(bases[row.base] as unknown as Json, row.result as unknown as Json));
    });
  }

  it('an empty batch is not an edit, like the engine', async () => {
    const be = withBase('main');
    const r = await be.applyOps([]);
    const st = await be.getState();
    expect(r.revision).toBe(1);
    expect(st.revision).toBe(1);
    expect(st.canUndo).toBe(false);
  });

  it('rejects a dead automation target with the shared INVALID_TARGET prefix, like the engine', async () => {
    const be = withBase('main');
    const ops = [{
      op: 'putAutomation',
      automation: { id: 51, name: 'x', color: '#e0a44a', target: { kind: 'plugin', fxId: 903, paramId: 999 }, points: [] },
    }] as unknown as Op[];
    await expect(be.applyOps(ops)).rejects.toThrow(/^INVALID_TARGET: effect 903 has no plugin parameter 999 to automate$/);
    const st = await be.getState();
    expect(st.project.automations?.length ?? 0).toBe(bases.main.automations?.length ?? 0);
  });

  it('undo returns to the base project, redo reapplies', async () => {
    const be = withBase('main');
    await be.applyOps([{ op: 'setBpm', bpm: 140 }]);
    const afterUndo = await be.undo();
    expect(afterUndo.project.bpm).toBe(bases.main.bpm);
    expect(afterUndo.canRedo).toBe(true);
    const afterRedo = await be.redo();
    expect(afterRedo.project.bpm).toBe(140);
  });
});
