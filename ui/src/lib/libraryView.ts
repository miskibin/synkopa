// What the browser's Library tab shows of a list of files: its groups, the files a search
// keeps, and the flat row model the list is drawn from. The list is virtualized: every row
// has a place in the model (so scrolling and the keyboard walk work over any folder size),
// but only the rows on screen are built.
import type { LibFile } from '../api/types';

/** Groups of this many files or fewer start open; a longer list starts as closed group rows. */
export const OPEN_UP_TO = 40;
/** Row heights the list is laid out with (styles.css: `.fitem`, `.grow`). */
export const ROW_H = 22;
export const GHEAD_H = 26;
/** Extra rows drawn above and below the viewport so fast scrolling never shows a gap. */
export const OVERSCAN = 12;

export interface LibGroup { name: string; files: LibFile[]; /** every file of the group (the badge) */ total: number }

const fold = (s: string) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
/** The words of a search (lower case, accents dropped); empty = no search. */
export const searchTerms = (q: string): string[] => fold(q).split(/\s+/).filter(Boolean);

/**
 * Does the file match every word? A word matches the file's name, its group as stored or its
 * group as shown (`shown`, the translated name), so "werble" and "snares" both find snares.
 */
export function matches(f: LibFile, terms: string[], shown: (group: string) => string = g => g): boolean {
  if (!terms.length) return true;
  const hay = fold(`${f.name} ${f.group} ${shown(f.group)}`);
  return terms.every(w => hay.includes(w));
}

/** The files a search keeps, grouped in the order the list gives them; every file is kept. */
export function groupFiles(files: readonly LibFile[], terms: string[] = [], shown?: (group: string) => string): LibGroup[] {
  const m = new Map<string, LibFile[]>();
  for (const f of files) {
    if (!matches(f, terms, shown)) continue;
    const g = m.get(f.group);
    if (g) g.push(f); else m.set(f.group, [f]);
  }
  return [...m.entries()].map(([name, files]) => ({ name, files, total: files.length }));
}

/** Is a group open? The user's choice (made in this view: browsing or this search), else open in a search and by the list's size otherwise. */
export const groupOpen = (choice: boolean | undefined, searching: boolean, listSize: number): boolean =>
  choice ?? (searching || listSize <= OPEN_UP_TO);

/** One row of a drawn list: a group header or a file, at its offset in the list's block. */
export interface FlatItem {
  kind: 'ghead' | 'frow';
  y: number;
  g?: LibGroup;
  open?: boolean;
  f?: LibFile;
  /** the row sits inside a named group (indent) */
  sub?: boolean;
}

/**
 * The rows of a whole list in draw order with each row's offset: a header for every group,
 * then the group's files when it is open. The block is as tall as the last row, whatever is
 * drawn on screen.
 */
export function flatten(groups: readonly LibGroup[], open: (group: string) => boolean): FlatItem[] {
  const out: FlatItem[] = [];
  let y = 0;
  for (const g of groups) {
    // an unnamed group (the engine never makes one; the mock's folder files come as one) draws
    // its files flat: no header to click, always open — as the list did before it was virtualized
    const isOpen = g.name ? open(g.name) : true;
    if (g.name) {
      out.push({ kind: 'ghead', y, g, open: isOpen });
      y += GHEAD_H;
    }
    if (isOpen) for (const f of g.files) {
      out.push({ kind: 'frow', y, f, sub: !!g.name });
      y += ROW_H;
    }
  }
  return out;
}

/**
 * The run of rows `i0..i1` (inclusive) intersecting the viewport `[top, bottom)` of the
 * scroll container, with `overscan` rows of margin on both sides. Empty list → `[0, -1]`.
 */
export function windowRange(items: readonly FlatItem[], top: number, bottom: number, overscan = OVERSCAN): [number, number] {
  const from = top - overscan * ROW_H, to = bottom + overscan * ROW_H;
  const rowH = (it: FlatItem) => (it.kind === 'ghead' ? GHEAD_H : ROW_H);
  let i0 = 0;
  while (i0 < items.length && items[i0].y + rowH(items[i0]) <= from) i0++;
  if (i0 >= items.length || items[i0].y >= to) return [0, -1];
  let i1 = i0;
  while (i1 + 1 < items.length && items[i1 + 1].y < to) i1++;
  return [i0, i1];
}

/** The row `step` after (+1) or before (−1) the current one, staying on the ends; no current = the first (or last going up). */
export function stepIndex(count: number, current: number, step: 1 | -1): number {
  if (count <= 0) return -1;
  if (current < 0) return step > 0 ? 0 : count - 1;
  return Math.min(count - 1, Math.max(0, current + step));
}
