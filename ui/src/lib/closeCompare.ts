// Deep comparison against engine-written fixtures (tune-edit.json, ops.json): fails with a
// path-annotated assertion, tolerating 1e-6 on numbers (the engine writes f32-widened
// floats). Test-only — it exists for the vitest expect.
import { expect } from 'vitest';

const isObj = (v: unknown): v is Record<string, unknown> => v != null && typeof v === 'object' && !Array.isArray(v);

export function close(a: unknown, b: unknown, path = ''): void {
  if (typeof a === 'number' && typeof b === 'number') { expect(Math.abs(a - b), path).toBeLessThan(1e-6); return; }
  if (Array.isArray(a) && Array.isArray(b)) { expect(a.length, path).toBe(b.length); a.forEach((x, i) => close(x, b[i], `${path}[${i}]`)); return; }
  if (isObj(a) && isObj(b)) {
    const ka = Object.keys(a).sort(), kb = Object.keys(b).sort();
    expect(ka, path).toEqual(kb);
    for (const k of ka) close(a[k], b[k], `${path}.${k}`);
    return;
  }
  expect(a, path).toEqual(b);
}
