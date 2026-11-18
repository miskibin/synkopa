// Where the docked panels sit around the Playlist (#90): the Channel Rack, the Piano roll and the
// Mixer each float as a window or are docked to the left, right or bottom edge of the main window.
// An edge holds strips, from the Playlist outward; a strip holds one panel or several that share
// it (stacked on a side edge, side by side at the bottom). Saved per user in `wave.layout`, never
// in the project. Pure: tested in dock.test.ts.

export type PaneId = 'rack' | 'roll' | 'mix';
export type Edge = 'left' | 'right' | 'bottom';
export const PANES: readonly PaneId[] = ['rack', 'roll', 'mix'];
export const EDGES: readonly Edge[] = ['left', 'right', 'bottom'];

/** A docked panel and its part of the strip's length (relative to the others in the strip). */
export interface DockPane { id: PaneId; share: number }
/** Strips per edge, the one beside the Playlist first. A strip is never empty. */
export type DockLayout = Record<Edge, DockPane[][]>;
/** A docked panel's wanted thickness: `w` on a side edge, `h` at the bottom (px). The rack's `w` 0 = as wide as the pattern. */
export interface PaneSize { w: number; h: number }
export type PaneSizes = Record<PaneId, PaneSize>;
/** Where the dock button puts a floating panel: its edge, and outside the other strips there (`outer`) or beside the Playlist. */
export interface DockHome { edge: Edge; outer: boolean }
export type DockHomes = Record<PaneId, DockHome>;

/** A place to dock a panel: a new strip at index `at` of the edge (0 = beside the Playlist), or into strip `strip` at panel index `at`. */
export type DockTarget =
  | { kind: 'strip'; edge: Edge; at: number }
  | { kind: 'join'; edge: Edge; strip: number; at: number };

/** First launch: the Mixer docked at the bottom, the Channel Rack and the Piano roll float (FL Studio's default). */
export const defaultDock = (): DockLayout => ({ left: [], right: [], bottom: [[{ id: 'mix', share: 1 }]] });
export const defaultSizes = (): PaneSizes => ({ rack: { w: 0, h: 260 }, roll: { w: 620, h: 320 }, mix: { w: 460, h: 300 } });
export const defaultHomes = (): DockHomes => ({
  rack: { edge: 'left', outer: false }, roll: { edge: 'bottom', outer: false }, mix: { edge: 'bottom', outer: true },
});

/** The thinnest a docked panel gets across its edge: `w` on a side edge, `h` at the bottom. The Piano roll keeps a band of keys, the Mixer its faders. */
export const PANE_MIN: Record<PaneId, PaneSize> = { rack: { w: 480, h: 180 }, roll: { w: 420, h: 300 }, mix: { w: 360, h: 170 } };
/** The shortest a panel gets along a strip it shares (CSS min size): its height on a side edge, its width at the bottom. */
export const PANE_MIN_LEN: Record<PaneId, PaneSize> = { rack: { w: 320, h: 160 }, roll: { w: 380, h: 240 }, mix: { w: 340, h: 200 } };

export const side = (e: Edge) => e !== 'bottom';
/** A panel's thickness field on an edge. */
export const dim = (e: Edge): keyof PaneSize => (side(e) ? 'w' : 'h');

export function findPane(l: DockLayout, id: PaneId): { edge: Edge; strip: number; at: number } | null {
  for (const edge of EDGES) {
    const ss = l[edge];
    for (let s = 0; s < ss.length; s++) { const at = ss[s].findIndex(p => p.id === id); if (at >= 0) return { edge, strip: s, at }; }
  }
  return null;
}
export const isDocked = (l: DockLayout, id: PaneId) => findPane(l, id) != null;

const clone = (l: DockLayout): DockLayout => ({ left: l.left.map(s => s.map(p => ({ ...p }))), right: l.right.map(s => s.map(p => ({ ...p }))), bottom: l.bottom.map(s => s.map(p => ({ ...p }))) });
export const copyDock = clone;

/** The layout without `id` (a strip it leaves empty goes too). */
export function removePane(l: DockLayout, id: PaneId): DockLayout {
  const n = clone(l);
  for (const e of EDGES) n[e] = n[e].map(s => s.filter(p => p.id !== id)).filter(s => s.length);
  return n;
}

/**
 * `id` docked at `target`. The target's indices are those of the layout without `id` (the guides
 * are worked out while the panel floats), and are clamped to what exists. A panel joining a strip
 * gets as much of it as an average panel there.
 */
export function placePane(l: DockLayout, id: PaneId, target: DockTarget): DockLayout {
  const n = removePane(l, id), ss = n[target.edge];
  if (target.kind === 'join' && ss[target.strip]) {
    const st = ss[target.strip], avg = st.reduce((a, p) => a + p.share, 0) / st.length;
    st.splice(Math.max(0, Math.min(target.at, st.length)), 0, { id, share: avg || 1 });
    return n;
  }
  ss.splice(Math.max(0, Math.min(target.at, ss.length)), 0, [{ id, share: 1 }]);
  return n;
}

/** Docking to an edge from a menu or the dock button: outside the strips already there, or beside the Playlist. */
export function edgeTarget(l: DockLayout, id: PaneId, edge: Edge, outer: boolean): DockTarget {
  return { kind: 'strip', edge, at: outer ? removePane(l, id)[edge].length : 0 };
}

/** Where a docked panel would go back to after floating (its edge, and whether it was outside the other strips there). */
export function homeOf(l: DockLayout, id: PaneId, prev: DockHome): DockHome {
  const f = findPane(l, id);
  if (!f) return prev;
  const n = l[f.edge].length;
  return { edge: f.edge, outer: n > 1 ? f.strip === n - 1 && f.strip > 0 : f.edge === prev.edge ? prev.outer : false };
}

/**
 * Thicknesses (px) for strips that want `wants`, no thinner than `mins`, leaving `rest` of `avail`
 * for the Playlist. When the wants do not fit, each strip gives up the same part of what it has
 * above its minimum; when even the minimums do not fit (a small window, a high UI scale) they all
 * give way alike, down to half of what there is together, so nothing runs off the window.
 * `avail` ≤ 0 means "not measured yet".
 */
export function fitStrips(wants: number[], mins: number[], avail: number, rest: number): number[] {
  const w = wants.map((v, i) => Math.max(v, mins[i]));
  if (!(avail > 0)) return w.map(Math.round);
  const room = avail - rest, sum = w.reduce((a, b) => a + b, 0);
  if (sum <= room) return w.map(Math.round);
  const minSum = mins.reduce((a, b) => a + b, 0);
  if (minSum <= room) {
    const over = sum - minSum, k = over > 0 ? (room - minSum) / over : 0;
    return w.map((v, i) => Math.floor(mins[i] + (v - mins[i]) * k));
  }
  const total = Math.min(minSum, Math.max(room, avail / 2));
  return mins.map(m => Math.max(0, Math.floor((m * total) / (minSum || 1))));
}

/**
 * New shares for a strip whose panels measure `px` (along the strip) after the splitter between
 * panel `i` and `i + 1` moved by `d` px (toward `i + 1` = positive). Both keep `mins`; the others
 * keep their size. The shares are the pixel sizes, so the panels stay where the drag left them.
 */
export function splitShares(px: number[], i: number, d: number, mins: number[]): number[] {
  const out = px.slice();
  const lo = -(px[i] - mins[i]), hi = px[i + 1] - mins[i + 1];
  const dd = Math.max(Math.min(d, hi), Math.min(lo, 0));
  out[i] = px[i] + dd; out[i + 1] = px[i + 1] - dd;
  return out.map(v => Math.max(1, Math.round(v)));
}

// ---------------------------------------------------------------- dock guides

export interface Rect { x: number; y: number; w: number; h: number }
/** Where a dock guide is drawn (its centre), which way its arrow points, where the panel would go and the area it would take. */
export interface Guide { x: number; y: number; dir: 'left' | 'right' | 'up' | 'down'; target: DockTarget; preview: Rect }
/** A docked panel on screen, for the guides (`id`: which panel, for its minimum size). */
export interface PaneRect { edge: Edge; strip: number; at: number; rect: Rect; id?: PaneId }

/** The guide buttons' size, and the distance from a compass centre to its arrows. */
export const GUIDE = 32, COMPASS = 40;
/** The splitter beside each strip and between panels sharing one (px; PatDock, styles.css). */
export const DOCK_SPLIT = 5;
/** The narrowest / lowest the Playlist gets beside the strips (lib/layout's PLAYLIST_MIN_W / _H). */
const PL_MIN: PaneSize = { w: 320, h: 140 };
const EDGE_DIR: Record<Edge, Guide['dir']> = { left: 'left', right: 'right', bottom: 'down' };

/**
 * Where the strips of the work area go (PatDock's Workspace): side strips are as tall as the row
 * that holds the Browser, the side strips and the Playlist (`row`); bottom strips run under all of
 * it, as wide as the whole work column (`col`).
 */
export interface DockFrame {
  row: Rect; col: Rect;
  /** The dragged panel and its docked size: the thickness of a strip it leads (`w` on a side, `h` at the bottom). */
  drag?: PaneId; want?: PaneSize;
}

/**
 * The dock guides while a panel is dragged (Visual Studio's): one at the middle of each outer edge
 * of the work area (docks outside every strip there), a compass over the Playlist (docks beside
 * it) and one over every docked panel. On a panel's compass the arrows along its strip share the
 * strip with it (stacked on a side edge, side by side at the bottom); the arrows across it make a
 * new strip on that side of it.
 *
 * Each guide's preview is the area the panel will take once dropped, laid out as PatDock lays out
 * the strips (#161: the Playlist compass's ↓ showed a band under the Playlist only, then docked
 * across the window). A new strip spans its whole edge and sits next to the strips that stay put
 * (the outer ones; the inner ones and the Playlist make room); a panel joining a strip takes an
 * equal part of it.
 */
export function dockGuides(stage: Rect, center: Rect, panes: PaneRect[], counts: Record<Edge, number>, frame: DockFrame): Guide[] {
  const g: Guide[] = [];
  const m = GUIDE / 2 + 8;
  const { row, col } = frame;
  // the strips on screen, per edge, by index (the union of their panels)
  const strips: Record<Edge, Rect[]> = { left: [], right: [], bottom: [] };
  for (const p of panes) {
    const s = strips[p.edge], r = s[p.strip];
    if (!r) { s[p.strip] = { ...p.rect }; continue; }
    const x0 = Math.min(r.x, p.rect.x), y0 = Math.min(r.y, p.rect.y);
    s[p.strip] = { x: x0, y: y0, w: Math.max(r.x + r.w, p.rect.x + p.rect.w) - x0, h: Math.max(r.y + r.h, p.rect.y + p.rect.h) - y0 };
  }
  const drag = frame.drag, want = frame.want;
  /** How thick a strip led by the dragged panel gets on `edge`: its docked size, at least the minimums, at most what the Playlist can give. */
  const thickness = (edge: Edge, extraMin = 0) => {
    const d = dim(edge), sd = side(edge);
    const fallback = Math.round((sd ? center.w : center.h) * 0.4);
    return Math.round(Math.max(want?.[d] || fallback, drag ? PANE_MIN[drag][d] : 0, extraMin));
  };
  /**
   * The strips on `edge`'s axis fitted again with a new one among them (fitStrips, as PatDock
   * does: in a tight window they all give way): the new strip's thickness and each shown strip's.
   */
  const refit = (edge: Edge) => {
    const sd = side(edge), d = dim(edge), es: Edge[] = sd ? ['left', 'right'] : ['bottom'];
    const have = es.flatMap(e => strips[e].flatMap((r, si) => (r ? [{ key: e + si, px: sd ? r.w : r.h, min: Math.max(0, ...panes.filter(p => p.edge === e && p.strip === si).map(p => (p.id ? PANE_MIN[p.id][d] : 0))) }] : [])));
    const used = have.reduce((a, x) => a + x.px + DOCK_SPLIT, 0), avail = (sd ? center.w : center.h) + used - DOCK_SPLIT;
    const px = fitStrips([...have.map(x => x.px), thickness(edge)], [...have.map(x => x.min), drag ? PANE_MIN[drag][d] : 0], avail, sd ? PL_MIN.w : PL_MIN.h);
    const of = new Map(have.map((x, i) => [x.key, px[i]]));
    return { t: px[px.length - 1], px: (e: Edge, si: number) => of.get(e + si) ?? 0 };
  };
  /**
   * A new strip at index `k` of `edge`: just inside strip `k`, or outermost. The strips from `k`
   * outward stay stacked from the edge's outer side (a hidden panel's strip takes no room); the
   * inner ones and the Playlist make room.
   */
  const newStrip = (edge: Edge, k: number): Rect => {
    const f = refit(edge), t = f.t, ss = strips[edge];
    const shown = ss.flatMap((r, si) => (r ? [si] : [])), last = shown.length ? ss[shown[shown.length - 1]] : null;
    const outside = shown.filter(si => si >= k).reduce((a, si) => a + f.px(edge, si) + DOCK_SPLIT, 0);
    if (edge === 'left') {
      const anchor = last ? last.x : center.x; // the outer side of the left strips (the Browser is beyond it)
      return { x: anchor + outside, y: row.y, w: t, h: row.h };
    }
    if (edge === 'right') {
      const anchor = last ? last.x + last.w : center.x + center.w;
      return { x: anchor - outside - t, y: row.y, w: t, h: row.h };
    }
    return { x: col.x, y: col.y + col.h - outside - t, w: col.w, h: t };
  };
  /** The dragged panel joining strip `si` of `edge` at panel index `at`: an equal part of the strip; the others shrink alike. */
  const joinStrip = (edge: Edge, si: number, at: number): Rect => {
    const S = strips[edge][si], sd = side(edge);
    const inStrip = panes.filter(p => p.edge === edge && p.strip === si).sort((a, b) => a.at - b.at), n = inStrip.length;
    const along = (r: Rect) => (sd ? r.h : r.w);
    // n + 1 panels with n splitters between them, in the strip's length
    const room = along(S) - n * DOCK_SPLIT, k = room / (n + 1);
    const before = inStrip.filter(p => p.at < at).length;
    const pos = (sd ? S.y : S.x) + before * (k + DOCK_SPLIT);
    // across: the strip keeps its thickness, unless the panel goes first (a strip is as thick as its first panel wants)
    const mins = Math.max(0, ...inStrip.map(p => (p.id ? PANE_MIN[p.id][dim(edge)] : 0)));
    const across = at === 0 ? thickness(edge, mins) : sd ? S.w : S.h;
    if (edge === 'left') return { x: S.x, y: pos, w: across, h: k };
    if (edge === 'right') return { x: S.x + S.w - across, y: pos, w: across, h: k };
    return { x: pos, y: S.y + S.h - across, w: k, h: across };
  };
  g.push({ x: stage.x + m, y: stage.y + stage.h / 2, dir: 'left', target: { kind: 'strip', edge: 'left', at: counts.left }, preview: newStrip('left', counts.left) });
  g.push({ x: stage.x + stage.w - m, y: stage.y + stage.h / 2, dir: 'right', target: { kind: 'strip', edge: 'right', at: counts.right }, preview: newStrip('right', counts.right) });
  g.push({ x: stage.x + stage.w / 2, y: stage.y + stage.h - m, dir: 'down', target: { kind: 'strip', edge: 'bottom', at: counts.bottom }, preview: newStrip('bottom', counts.bottom) });
  const cx = center.x + center.w / 2, cy = center.y + center.h / 2;
  for (const edge of EDGES) {
    const dir = EDGE_DIR[edge];
    g.push({ x: cx + (dir === 'left' ? -COMPASS : dir === 'right' ? COMPASS : 0), y: cy + (dir === 'down' ? COMPASS : 0), dir,
      target: { kind: 'strip', edge, at: 0 }, preview: newStrip(edge, 0) });
  }
  for (const p of panes) {
    const r = p.rect, px = r.x + r.w / 2, py = r.y + r.h / 2;
    const along: Guide['dir'][] = side(p.edge) ? ['up', 'down'] : ['left', 'right'];
    // across: toward the Playlist = a new strip on its inner side (index `strip`), away = outer side
    const inward: Guide['dir'] = p.edge === 'left' ? 'right' : p.edge === 'right' ? 'left' : 'up';
    const outward: Guide['dir'] = p.edge === 'left' ? 'left' : p.edge === 'right' ? 'right' : 'down';
    const at = (d: Guide['dir']) => ({ x: px + (d === 'left' ? -COMPASS : d === 'right' ? COMPASS : 0), y: py + (d === 'up' ? -COMPASS : d === 'down' ? COMPASS : 0) });
    g.push({ ...at(along[0]), dir: along[0], target: { kind: 'join', edge: p.edge, strip: p.strip, at: p.at }, preview: joinStrip(p.edge, p.strip, p.at) });
    g.push({ ...at(along[1]), dir: along[1], target: { kind: 'join', edge: p.edge, strip: p.strip, at: p.at + 1 }, preview: joinStrip(p.edge, p.strip, p.at + 1) });
    g.push({ ...at(inward), dir: inward, target: { kind: 'strip', edge: p.edge, at: p.strip }, preview: newStrip(p.edge, p.strip) });
    g.push({ ...at(outward), dir: outward, target: { kind: 'strip', edge: p.edge, at: p.strip + 1 }, preview: newStrip(p.edge, p.strip + 1) });
  }
  return g;
}

/** The guide under the pointer, if any. */
export function guideAt(gs: Guide[], x: number, y: number): Guide | null {
  const h = GUIDE / 2;
  for (let i = gs.length - 1; i >= 0; i--) { const g = gs[i]; if (Math.abs(x - g.x) <= h && Math.abs(y - g.y) <= h) return g; }
  return null;
}

// ---------------------------------------------------------------- saved layout

const okId = (v: unknown): v is PaneId => v === 'rack' || v === 'roll' || v === 'mix';
const okEdge = (v: unknown): v is Edge => v === 'left' || v === 'right' || v === 'bottom';

/** A saved dock layout, or undefined when it is not one. Unknown or repeated panels and empty strips are dropped. */
export function parseDock(v: unknown): DockLayout | undefined {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return undefined;
  const o = v as Record<string, unknown>, seen = new Set<PaneId>(), out = { left: [], right: [], bottom: [] } as DockLayout;
  for (const e of EDGES) {
    const ss = o[e];
    if (ss === undefined) continue;
    if (!Array.isArray(ss)) return undefined;
    for (const s of ss) {
      if (!Array.isArray(s)) continue;
      const strip: DockPane[] = [];
      for (const p of s) {
        const q = p as Record<string, unknown> | null;
        if (!q || !okId(q.id) || seen.has(q.id)) continue;
        seen.add(q.id);
        const sh = typeof q.share === 'number' && isFinite(q.share) && q.share > 0 ? Math.min(q.share, 1e5) : 1;
        strip.push({ id: q.id, share: sh });
      }
      if (strip.length) out[e].push(strip);
    }
  }
  return out;
}
const num = (v: unknown, lo: number, hi: number, d: number) => (typeof v === 'number' && isFinite(v) && v >= lo && v <= hi ? v : d);
/** Saved panel sizes: every field valid or the default. The rack's width may be 0 (fit the pattern). */
export function parseSizes(v: unknown, base = defaultSizes()): PaneSizes {
  const o = (v && typeof v === 'object' ? v : {}) as Record<string, Record<string, unknown> | undefined>;
  const out = defaultSizes();
  for (const id of PANES) {
    const s = o[id] ?? {};
    out[id] = { w: id === 'rack' && s.w === 0 ? 0 : num(s.w, 100, 8000, base[id].w), h: num(s.h, 100, 8000, base[id].h) };
  }
  return out;
}
export function parseHomes(v: unknown): DockHomes {
  const o = (v && typeof v === 'object' ? v : {}) as Record<string, Record<string, unknown> | undefined>;
  const out = defaultHomes();
  for (const id of PANES) { const h = o[id]; if (h && okEdge(h.edge)) out[id] = { edge: h.edge, outer: h.outer === true }; }
  return out;
}

/**
 * The dock layout of a layout saved before panels could go anywhere: the Channel Rack docked left
 * (`rackDock`), the Piano roll at the bottom above the Mixer (`rollDock`), the Mixer at the bottom
 * unless `mixDock` was false.
 */
export function migrateDock(v: Record<string, unknown>): DockLayout {
  const l: DockLayout = { left: [], right: [], bottom: [] };
  if (v.rackDock === true) l.left.push([{ id: 'rack', share: 1 }]);
  if (v.rollDock === true) l.bottom.push([{ id: 'roll', share: 1 }]);
  if (v.mixDock !== false) l.bottom.push([{ id: 'mix', share: 1 }]);
  return l;
}
/** The panel sizes of such a layout (`rackDockW`, `rollDockH`, `mixH`). */
export function migrateSizes(v: Record<string, unknown>): PaneSizes {
  const d = defaultSizes();
  return parseSizes({ rack: { w: v.rackDockW, h: d.rack.h }, roll: { w: d.roll.w, h: v.rollDockH }, mix: { w: d.mix.w, h: v.mixH } });
}
