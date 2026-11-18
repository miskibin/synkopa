// Where you were in a project when you last saved it: the current pattern, the selected channel
// and the start marker. FL reopens a song on the pattern you saved it with; here a project always
// opened on pattern 1 at bar 1. This is view state, so it lives beside the layout (localStorage,
// by project path), not in the project file or its undo history.
import type { Id, Project } from '../api/types';

export interface ProjView { pat: Id | null; selCh: Id | null; start: number }
/** Path (lower-cased, slashes normalised) → view, newest last. */
export type ProjViews = [string, ProjView][];

/** How many projects are remembered. */
export const PROJ_VIEWS_MAX = 40;
const norm = (path: string) => path.replace(/\\/g, '/').toLowerCase();

/** `views` with `v` remembered for `path` (moved to the end; the oldest drop off). */
export function rememberView(views: ProjViews, path: string, v: ProjView): ProjViews {
  const key = norm(path);
  return [...views.filter(([k]) => k !== key), [key, v] as [string, ProjView]].slice(-PROJ_VIEWS_MAX);
}

/**
 * What to restore for the project at `path`: only what still exists in it (a pattern or channel
 * deleted since, or a file replaced by another song, falls back to the first one), and a start
 * marker inside the song.
 */
export function viewFor(views: ProjViews, path: string | null, p: Project, totalBeats: number): ProjView {
  const first: ProjView = { pat: p.patterns[0]?.id ?? null, selCh: p.channels?.[0]?.id ?? null, start: 0 };
  const v = path ? views.find(([k]) => k === norm(path))?.[1] : undefined;
  if (!v) return first;
  return {
    pat: p.patterns.some(x => x.id === v.pat) ? v.pat : first.pat,
    selCh: (p.channels ?? []).some(x => x.id === v.selCh) ? v.selCh : first.selCh,
    start: Number.isFinite(v.start) && v.start > 0 && v.start < totalBeats ? v.start : 0,
  };
}

/** Parses the stored list; anything malformed is dropped. */
export function parseViews(text: string | null): ProjViews {
  try {
    const raw: unknown = JSON.parse(text ?? '[]');
    if (!Array.isArray(raw)) return [];
    return raw.flatMap(e => {
      if (!Array.isArray(e) || typeof e[0] !== 'string' || !e[1] || typeof e[1] !== 'object') return [];
      const v = e[1] as Partial<ProjView>;
      const id = (x: unknown) => (typeof x === 'number' && Number.isFinite(x) ? x : null);
      return [[e[0], { pat: id(v.pat), selCh: id(v.selCh), start: typeof v.start === 'number' && Number.isFinite(v.start) ? v.start : 0 }] as [string, ProjView]];
    }).slice(-PROJ_VIEWS_MAX);
  } catch { return []; }
}
