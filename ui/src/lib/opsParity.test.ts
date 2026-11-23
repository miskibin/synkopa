// deep-3: the engine's op contract as fixtures. `crates/engine/tests/ui_ops_fixture.rs`
// runs every row through the real `ops::apply_all` and writes ops.json; this suite replays
// the same rows through the UI port (`applyOps.ts`). Happy rows must land on the very
// project the engine produced; reject rows must fail with the engine's exact message —
// these strings are user-visible toasts. CI fails here when the engine moved and the port
// did not follow (`WAVE_WRITE_FIXTURES=1` regenerates the file).
import { describe, expect, it } from 'vitest';
import fixture from './__fixtures__/ops.json';
import { applyAll } from './applyOps';
import { close } from './closeCompare';
import type { Op, Project } from '../api/types';

type Row = { name: string; base: string; ops: Op[]; result: Project | null; error: string | null };
const bases = fixture.bases as unknown as Record<string, Project>;
const rows = fixture.cases as unknown as Row[];

describe('ops parity with the engine (ops.json)', () => {
  it('covers every op and a rejection of most', () => {
    const kinds = new Set(rows.flatMap(r => r.ops.map(o => o.op)));
    expect(kinds.size).toBe(27);
    expect(rows.filter(r => r.error).length).toBeGreaterThanOrEqual(50);
    expect(rows.filter(r => !r.error).length).toBeGreaterThanOrEqual(30);
  });

  for (const row of rows) {
    it(row.name, () => {
      const p0 = structuredClone(bases[row.base]);
      if (row.error) {
        let msg = '';
        try {
          applyAll(p0, row.ops);
        } catch (e) {
          msg = String(e).replace(/^Error: /, '');
        }
        expect(msg).toBe(row.error);
        return;
      }
      close(applyAll(p0, row.ops), row.result);
    });
  }
});
