// Ranking of files found for a missing source, as crates/engine/src/relink.rs does it (the mock
// backend searches with it; the dialog labels candidates with the same notions).
import type { RelinkCandidate } from '../api/types';

const parts = (p: string) => p.split(/[\\/]+/).filter(Boolean);
const base = (p: string) => parts(p).pop() ?? '';
const stem = (name: string) => name.replace(/\.[^.]*$/, '');

/** File name of a stored path (either separator). */
export const fileName = base;
/** The folder a stored path is in (either separator); '' for a bare name. */
export const folderOf = (p: string) => p.replace(/[\\/]+[^\\/]*$/, '');

/** How `path` matches the old path, or null when its name does not. */
export function candidate(old: string, path: string): RelinkCandidate | null {
  const a = base(old).toLowerCase(), b = base(path).toLowerCase();
  const exactName = a === b;
  if (!exactName && stem(a) !== stem(b)) return null;
  const fa = parts(old).slice(0, -1).reverse(), fb = parts(path).slice(0, -1).reverse();
  let sharedFolders = 0;
  while (sharedFolders < fa.length && sharedFolders < fb.length && fa[sharedFolders].toLowerCase() === fb[sharedFolders].toLowerCase()) sharedFolders++;
  return { path, exactName, sharedFolders };
}

/** Best first: the old file name, then more shared folders, then by path. */
export function rank(list: RelinkCandidate[]): RelinkCandidate[] {
  return [...list].sort((x, y) => Number(y.exactName) - Number(x.exactName) || y.sharedFolders - x.sharedFolders || (x.path < y.path ? -1 : x.path > y.path ? 1 : 0));
}

/** The candidate to take without asking: the only best-ranked one with the old file name. */
export function unambiguous(ranked: RelinkCandidate[]): RelinkCandidate | null {
  const [first, second] = ranked;
  if (!first?.exactName) return null;
  if (second?.exactName && second.sharedFolders === first.sharedFolders) return null;
  return first;
}
