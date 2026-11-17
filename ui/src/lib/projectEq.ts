// Structural equality of project JSON, used where the engine compares `Project` structs
// (engine.rs apply: an unchanged batch is not an edit — no undo step, no revision, no
// state event; the mock needs the same call). Object key order does not matter, array
// order does, numbers compare exactly (both sides hold TS numbers here).

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

const isObj = (v: Json): v is { [k: string]: Json } => v != null && typeof v === 'object' && !Array.isArray(v);

export function projectEq(a: Json, b: Json): boolean {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x, i) => projectEq(x, b[i]));
  if (isObj(a) && isObj(b)) {
    const ka = Object.keys(a).sort(), kb = Object.keys(b).sort();
    return ka.length === kb.length && ka.every((k, i) => k === kb[i] && projectEq(a[k], b[kb[i]]));
  }
  return false;
}
