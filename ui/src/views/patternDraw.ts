// Canvas rendering of the pattern windows: the Channel Rack step grid and graph editor, the
// velocity lane and the piano roll. Static parts (cell backgrounds, key rows, grid lines, the keyboard) are cached
// offscreen in `Layer`s keyed by geometry and theme; notes are drawn on top on change only.
import { shadeBands, shadeUnit } from '../lib/gridShade';
import type { Channel, Id, Note, Pattern } from '../api/types';
import { t } from '../lib/i18n';
import { mix } from '../lib/color';
import { BEAT_GAP, gridW, inScale, keyOf, noteWidth, stepAt, stepSpans, stepX, velOf } from '../lib/pattern';
import { isBlack, noteName } from '../lib/pitch';
import type { Layer } from '../ui/frameLoop';
import { rgba, T, theme } from '../ui/theme';

const MONO8 = '600 8px "Geist Mono Variable", "Geist Mono", monospace';
const MONO9 = '500 9px "Geist Mono Variable", "Geist Mono", monospace';
const SANS10 = '500 10px "Geist Variable", Geist, system-ui, sans-serif';

function rr(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  r = Math.max(0, Math.min(r, w / 2, h / 2));
  g.moveTo(x + r, y); g.arcTo(x + w, y, x + w, y + h, r); g.arcTo(x + w, y + h, x, y + h, r); g.arcTo(x, y + h, x, y, r); g.arcTo(x, y, x + w, y, r); g.closePath();
}

/**
 * Step-grid geometry: cell width, scroll offsets (px), row height, rows, `end` = the pattern's
 * steps and `steps` = the steps shown (the pattern plus one greyed bar: a click there extends it).
 */
export interface RackGeo { cw: number; sx: number; sy: number; rh: number; steps: number; end: number; rows: number }

const STEP_PAD = 2, STEP_VPAD = 5, STEP_R = 3;
/** Unlit step colours: alternating groups of four (beats), like FL's. */
function stepOff(s: number) {
  return mix(T.fg5, Math.floor(s / 4) % 2 ? (theme.light ? 18 : 13) : (theme.light ? 30 : 24), T.bg2);
}
/** A lit step: one neutral colour for every channel (the channel colour is the row's strip); velocity = brightness. */
function stepOn(v: number, dim: boolean) {
  const c = mix(theme.light ? T.fg2 : T.fgHi, Math.round(45 + 50 * v), T.bg7);
  return dim ? mix(c, 45, T.bg7) : c;
}

/** Cell backgrounds, bar lines, the greyed extra bar. Cached: depends only on geometry and theme. */
function rackBg(g: CanvasRenderingContext2D, W: number, H: number, G: RackGeo) {
  const s0 = Math.max(0, stepAt(G.sx, G.cw)), s1 = Math.min(G.steps - 1, stepAt(G.sx + W, G.cw));
  const r0 = Math.max(0, Math.floor(G.sy / G.rh)), r1 = Math.min(G.rows - 1, Math.ceil((G.sy + H) / G.rh));
  for (let r = r0; r <= r1; r++) {
    const y = r * G.rh - G.sy;
    for (let s = s0; s <= s1; s++) {
      const x = stepX(s, G.cw) - G.sx;
      g.globalAlpha = s >= G.end ? 0.35 : 1;
      g.fillStyle = stepOff(s);
      g.beginPath(); rr(g, x + STEP_PAD, y + STEP_VPAD, G.cw - 2 * STEP_PAD, G.rh - 2 * STEP_VPAD, STEP_R); g.fill();
    }
  }
  g.globalAlpha = 1;
  // bar lines; the pattern's end a little stronger
  const h = Math.min(H, G.rows * G.rh - G.sy);
  for (let s = Math.ceil(s0 / 16) * 16; s <= s1 + 1; s += 16) {
    if (s <= 0 || s >= G.steps) continue;
    g.fillStyle = s === G.end ? T.line3 : T.line2;
    g.fillRect(Math.round(stepX(s, G.cw) - G.sx - BEAT_GAP / 2) - 0.5, 0, 1, h);
  }
}

export function drawRack(g: CanvasRenderingContext2D, W: number, H: number, G: RackGeo, layer: Layer, pt: Pattern, chans: Channel[], selCh: Id | null, roll: Set<Id>, anySolo: boolean) {
  g.clearRect(0, 0, W, H);
  const bg = layer.get(W, H, ['rack', G.cw, G.sx, G.sy, G.rh, G.steps, G.end, G.rows].join('|'), (gg, w, h) => rackBg(gg, w, h, G));
  g.drawImage(bg, 0, 0, W, H);
  const vpad = STEP_VPAD;
  const r0 = Math.max(0, Math.floor(G.sy / G.rh)), r1 = Math.min(chans.length - 1, Math.ceil((G.sy + H) / G.rh));
  for (let r = r0; r <= r1; r++) {
    const c = chans[r], y = r * G.rh - G.sy;
    const dim = c.mute || (anySolo && !c.solo);
    if (c.id === selCh) { g.fillStyle = rgba(T.blue, theme.light ? 0.07 : 0.06); g.fillRect(0, y, W, G.rh); }
    const notes = pt.notes.filter(n => n.ch === c.id);
    if (!notes.length) continue;
    if (roll.has(c.id)) {
      // more than steps (lengths, chords): a mini piano roll over the pattern; a click opens the Piano roll
      const keys = notes.map(n => keyOf(n, c)), lo = Math.min(...keys), hi = Math.max(...keys);
      const span = hi - lo + 1, ih = G.rh - 2 * vpad, nh = Math.max(2, Math.min(4, ih / span));
      const top = y + vpad + (ih - nh * span) / 2;
      g.fillStyle = T.bg2; g.beginPath(); rr(g, -G.sx, y + vpad - 1, gridW(G.end, G.cw), G.rh - 2 * vpad + 2, 2); g.fill();
      for (const n of notes) {
        const x = stepX(n.step, G.cw) - G.sx, w = Math.max(2, stepX(n.step + n.len, G.cw) - G.sx - x - 1);
        if (x > W || x + w < 0) continue;
        g.fillStyle = stepOn(velOf(n), dim);
        g.fillRect(x, top + (hi - keyOf(n, c)) * nh, w, Math.max(1.5, nh - 0.5));
      }
      continue;
    }
    // a trigger off the grid (a triplet) sits at its place inside the cell, as wide as the gap to the next one
    const spans = stepSpans(notes);
    notes.forEach((n, k) => {
      const x = stepX(n.step, G.cw) - G.sx, cw = G.cw * spans[k];
      if (x > W || x + cw < 0) return;
      g.fillStyle = stepOn(velOf(n), dim);
      g.beginPath(); rr(g, x + STEP_PAD, y + vpad, Math.max(3, cw - 2 * STEP_PAD), G.rh - 2 * vpad, Math.min(STEP_R, cw / 4)); g.fill();
      // a step with its own key (graph editor): its note name, or a tick when the cell is narrow
      if (n.p != null && n.p !== c.root) {
        g.fillStyle = T.onFill;
        if (cw >= 18) { g.font = MONO8; g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText(noteName(n.p), x + cw / 2, y + G.rh / 2 + 1); }
        else g.fillRect(x + cw / 2 - 1, y + G.rh - vpad - 4, 2, 2);
      }
    });
  }
}

/** The playhead column of the step grid (separate overlay canvas, redrawn only when the step changes). */
export function drawRackHead(g: CanvasRenderingContext2D, W: number, H: number, G: RackGeo, step: number) {
  g.clearRect(0, 0, W, H);
  if (step < 0) return;
  const x = stepX(step, G.cw) - G.sx, h = Math.min(H, G.rows * G.rh - G.sy);
  g.fillStyle = rgba(T.playhead, theme.light ? 0.10 : 0.09);
  g.fillRect(x, 0, G.cw, h);
  g.fillStyle = rgba(T.playhead, 0.55);
  g.fillRect(x + 1, 0, G.cw - 2, 2);
}

/** One velocity bar per note of the selected channel. `x0`: where step 0 is (px), `cw` px per step. */
export function drawLane(g: CanvasRenderingContext2D, W: number, H: number, o: {
  x0: number; cw: number; sx: number; steps: number; notes: { n: Note; i: number }[]; color: string; sel?: Set<number>; cells: boolean; muted: boolean; beatGap?: boolean;
}) {
  g.clearRect(0, 0, W, H);
  const top = 6, bh = H - top - 3;
  const px = (s: number) => (o.beatGap ? stepX(s, o.cw) : s * o.cw);
  const endX = Math.min(W, o.x0 + (o.beatGap ? gridW(o.steps, o.cw) : o.steps * o.cw) - o.sx);
  g.fillStyle = T.vizPanel; g.fillRect(0, 0, W, H);
  g.fillStyle = T.viz; g.fillRect(o.x0, 0, endX - o.x0, H);
  g.fillStyle = T.vizGrid2;
  for (const f of [0.25, 0.5, 0.75]) g.fillRect(o.x0, Math.round(top + bh * (1 - f)) + 0.5, endX - o.x0, 1);
  g.fillStyle = T.vizAxis; g.fillRect(o.x0, top, endX - o.x0, 1);
  const spans = o.cells ? stepSpans(o.notes.map(x => x.n)) : [];
  o.notes.forEach(({ n, i }, k) => {
    const v = velOf(n);
    const x = o.x0 + px(n.step) - o.sx, cw = o.cw * (spans[k] ?? 1);
    const w = o.cells ? Math.max(2, cw - 6) : Math.max(3, Math.min(8, n.len * o.cw - 2));
    const bx = o.cells ? x + (cw - w) / 2 : x;
    if (bx > W || bx + w < o.x0 - 4) return;
    const y = top + bh * (1 - v), sel = !!o.sel?.has(i);
    g.fillStyle = rgba(o.color, (o.muted ? 0.35 : 0.75) * (sel ? 1.25 : 1));
    g.fillRect(bx, y, w, top + bh - y);
    g.fillStyle = sel ? T.fgHi : mix(o.color, 60, T.fgHi);
    g.fillRect(bx, y - 1, w, 2);
  });
}

/**
 * The graph editor's key lane: one bar per step of the selected channel, its height the key
 * relative to the channel's root (the middle line), ± `span` semitones. `off`: the channel is a
 * melodic line (edit it in the piano roll) — the lane says so instead.
 */
export function drawKeyGraph(g: CanvasRenderingContext2D, W: number, H: number, o: {
  cw: number; sx: number; steps: number; notes: { n: Note; i: number }[]; color: string; root: number; span: number; off: boolean;
}) {
  g.clearRect(0, 0, W, H);
  const top = 6, bh = H - top - 3, endX = Math.min(W, gridW(o.steps, o.cw) - o.sx);
  g.fillStyle = T.vizPanel; g.fillRect(0, 0, W, H);
  g.fillStyle = T.viz; g.fillRect(0, 0, endX, H);
  const yOf = (k: number) => top + bh * (1 - (k - o.root + o.span) / (2 * o.span));
  g.fillStyle = T.vizGrid2;
  for (const d of [-12, -7, -5, 5, 7, 12]) if (Math.abs(d) <= o.span) g.fillRect(0, Math.round(yOf(o.root + d)) + 0.5, endX, 1);
  g.fillStyle = T.vizAxis; g.fillRect(0, Math.round(yOf(o.root)) + 0.5, endX, 1);
  if (o.off) {
    g.fillStyle = T.fg5; g.font = SANS10; g.textAlign = 'left'; g.textBaseline = 'middle';
    g.fillText(t('b.rk.keyOff'), 10, H / 2);
    return;
  }
  g.font = MONO8; g.textAlign = 'center'; g.textBaseline = 'bottom';
  const spans = stepSpans(o.notes.map(x => x.n));
  for (const [j, { n }] of o.notes.entries()) {
    const k = n.p ?? o.root, cw = o.cw * spans[j];
    const x = stepX(n.step, o.cw) - o.sx, w = Math.max(2, cw - 6), bx = x + (cw - w) / 2;
    if (bx > W || bx + w < 0) continue;
    const y0 = yOf(o.root), y = yOf(Math.max(o.root - o.span, Math.min(o.root + o.span, k)));
    g.fillStyle = rgba(o.color, 0.75);
    g.fillRect(bx, Math.min(y, y0), w, Math.max(2, Math.abs(y - y0)));
    g.fillStyle = mix(o.color, 60, T.fgHi); g.fillRect(bx, y - 1, w, 2);
    if (k !== o.root && cw >= 22) { g.fillStyle = T.fg3; g.fillText(noteName(k), x + cw / 2, Math.min(y, y0) - 1); }
  }
}

/** Piano-roll geometry: px per step, scroll (px), row height, keyboard width, steps. */
export interface RollGeo { pps: number; sx: number; sy: number; rowH: number; kw: number; steps: number }
export const rollY = (G: RollGeo, key: number) => (127 - key) * G.rowH - G.sy;
export const rollX = (G: RollGeo, step: number) => G.kw + step * G.pps - G.sx;

function rollBg(g: CanvasRenderingContext2D, W: number, H: number, G: RollGeo, root: number, mask: number, grid: number) {
  const k0 = Math.max(0, Math.floor(127 - (G.sy + H) / G.rowH)), k1 = Math.min(127, Math.ceil(127 - G.sy / G.rowH));
  g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
  for (let k = k0; k <= k1; k++) {
    const y = rollY(G, k);
    const isRoot = (((k - root) % 12) + 12) % 12 === 0;
    g.fillStyle = isRoot ? rgba(T.blue, theme.light ? 0.10 : 0.09) : inScale(k, root, mask) ? (theme.light ? T.bg7 : mix(T.viz, 70, T.bg6)) : isBlack(k) ? mix(T.viz, 80, T.bg0) : T.viz;
    g.fillRect(G.kw, y, W - G.kw, G.rowH);
    g.fillStyle = (k % 12 === 0) ? T.vizAxis : T.vizGrid;
    g.fillRect(G.kw, Math.round(y + G.rowH) - 0.5, W - G.kw, 1);
  }
  // vertical lines: the snap grid's own lines (16ths, triplets, ... when there is room), then
  // beats and bars over them. A grid of a beat or longer shows 16ths, like 1/16.
  const s0 = Math.max(0, Math.floor(G.sx / G.pps)), s1 = Math.ceil((G.sx + W - G.kw) / G.pps);
  // every other beat (bar when zoomed out) a shade lighter, over the key rows and under the lines (4 steps = a beat)
  g.fillStyle = T.gridShade;
  for (const [b, n] of shadeBands(s0 / 4, s1 / 4, shadeUnit(G.pps * 4))) {
    const xa = Math.max(G.kw, Math.round(rollX(G, b * 4))), xb = Math.round(rollX(G, (b + n) * 4));
    if (xb > xa) g.fillRect(xa, 0, xb - xa, H);
  }
  const u = grid > 0 && grid < 4 ? grid : 1;
  if (u * G.pps >= 7) {
    g.fillStyle = T.vizGrid;
    for (let i = Math.floor(s0 / u); i * u <= s1; i++) {
      const s = i * u;
      if (Math.abs(s / 4 - Math.round(s / 4)) < 1e-6) continue;
      g.fillRect(Math.round(rollX(G, s)) - 0.5, 0, 1, H);
    }
  }
  for (let s = Math.ceil(s0 / 4) * 4; s <= s1; s += 4) {
    g.fillStyle = s % 16 === 0 ? T.vizAxis : T.vizGrid2;
    g.fillRect(Math.round(rollX(G, s)) - 0.5, 0, 1, H);
  }
  // past the pattern end: drawing there makes the pattern longer
  const endX = rollX(G, G.steps);
  if (endX < W) { g.fillStyle = T.shade; g.fillRect(Math.max(G.kw, endX), 0, W - Math.max(G.kw, endX), H); }
  // keyboard
  g.fillStyle = T.vizPanel; g.fillRect(0, 0, G.kw, H);
  for (let k = k0; k <= k1; k++) {
    const y = rollY(G, k), b = isBlack(k);
    g.fillStyle = b ? T.keyB : T.keyW;
    g.fillRect(0, y + 0.5, b ? G.kw * 0.62 : G.kw - 1, G.rowH - 1);
    if (k % 12 === 0 && G.rowH >= 9) {
      g.fillStyle = T.keyWText; g.font = MONO9; g.textBaseline = 'middle'; g.textAlign = 'right';
      g.fillText(noteName(k), G.kw - 4, y + G.rowH / 2 + 0.5);
    }
  }
  g.fillStyle = T.line2; g.fillRect(G.kw - 1, 0, 1, H);
}

export function drawRoll(g: CanvasRenderingContext2D, W: number, H: number, G: RollGeo, layer: Layer, o: {
  pt: Pattern; ch: Channel; chans: Map<Id, Channel>; sel: Set<number>; root: number; mask: number;
  /** Snap grid in steps: its lines are drawn (triplet grids get triplet lines). */
  grid: number; hoverKey: number | null; lit?: number[]; marquee: { x0: number; y0: number; x1: number; y1: number } | null;
  /** Ctrl+right-drag: the box the view will zoom to (dashed, no fill: it selects nothing). */
  zoomBox?: { x0: number; y0: number; x1: number; y1: number } | null;
  /** Chord-stamp tool: what a click at this cell would write (ghosted, dashed outline). */
  stampGhost?: { s: number; len: number; pitches: number[] } | null;
}) {
  g.clearRect(0, 0, W, H);
  const bg = layer.get(W, H, ['roll', G.pps, G.sx, G.sy, G.rowH, G.kw, G.steps, o.root, o.mask, o.grid].join('|'), (gg, w, h) => rollBg(gg, w, h, G, o.root, o.mask, o.grid));
  g.drawImage(bg, 0, 0, W, H);
  g.save();
  g.beginPath(); g.rect(G.kw, 0, W - G.kw, H); g.clip();
  // ghost notes: the pattern's other channels, faint (double right-click one to edit its channel)
  for (const n of o.pt.notes) {
    if (n.ch === o.ch.id) continue;
    const c = o.chans.get(n.ch); if (!c) continue;
    const x = rollX(G, n.step), y = rollY(G, keyOf(n, c)), w = noteWidth(n, G.pps);
    if (x > W || x + w < G.kw || y > H || y + G.rowH < 0) continue;
    g.fillStyle = rgba(c.color, 0.16); g.fillRect(x, y + 1, w, G.rowH - 2);
  }
  g.font = SANS10; g.textBaseline = 'middle'; g.textAlign = 'left';
  o.pt.notes.forEach((n, i) => {
    if (n.ch !== o.ch.id) return;
    const k = keyOf(n, o.ch), x = rollX(G, n.step), y = rollY(G, k), w = noteWidth(n, G.pps);
    if (x > W || x + w < G.kw || y > H || y + G.rowH < 0) return;
    const sel = o.sel.has(i), v = velOf(n);
    g.fillStyle = sel ? mix(o.ch.color, 55, theme.light ? '#000000' : '#ffffff') : mix(o.ch.color, Math.round(45 + 55 * v), T.viz);
    g.beginPath(); rr(g, x, y + 0.5, w, G.rowH - 1, 2.5); g.fill();
    g.strokeStyle = sel ? T.fgHi : rgba('#000000', theme.light ? 0.25 : 0.45); g.lineWidth = 1;
    g.beginPath(); rr(g, x + 0.5, y + 1, w - 1, G.rowH - 2, 2.5); g.stroke();
    // resize grip
    if (n.trig || w > 10) { g.fillStyle = rgba(T.onFill, 0.35); g.fillRect(x + w - 3, y + 3, 1, G.rowH - 6); }
    if (w > 26 && G.rowH >= 11) { g.fillStyle = rgba(T.onFill, 0.85); g.fillText(noteName(k), x + 4, y + G.rowH / 2 + 0.5); }
  });
  if (o.stampGhost) {
    const gh = o.stampGhost, w = Math.max(3, gh.len * G.pps - 1), x = rollX(G, gh.s);
    for (const k of gh.pitches) {
      const y = rollY(G, k);
      g.fillStyle = rgba(o.ch.color, 0.32);
      g.beginPath(); rr(g, x, y + 0.5, w, G.rowH - 1, 2.5); g.fill();
      g.strokeStyle = rgba(o.ch.color, 0.95); g.lineWidth = 1; g.setLineDash([3, 2]);
      g.beginPath(); rr(g, x + 0.5, y + 1, w - 1, G.rowH - 2, 2.5); g.stroke();
      g.setLineDash([]);
    }
  }
  if (o.marquee) {
    const m = o.marquee, x = Math.min(m.x0, m.x1), y = Math.min(m.y0, m.y1);
    g.fillStyle = rgba(T.blue, 0.12); g.fillRect(x, y, Math.abs(m.x1 - m.x0), Math.abs(m.y1 - m.y0));
    g.strokeStyle = rgba(T.blue, 0.8); g.strokeRect(x + 0.5, y + 0.5, Math.abs(m.x1 - m.x0), Math.abs(m.y1 - m.y0));
  }
  if (o.zoomBox) {
    const m = o.zoomBox, x = Math.min(m.x0, m.x1), y = Math.min(m.y0, m.y1);
    g.strokeStyle = T.fgHi; g.lineWidth = 1; g.setLineDash([4, 3]);
    g.strokeRect(Math.round(x) + 0.5, Math.round(y) + 0.5, Math.round(Math.abs(m.x1 - m.x0)), Math.round(Math.abs(m.y1 - m.y0)));
    g.setLineDash([]);
  }
  g.restore();
  // keys sounding: clicked on the keyboard, or played from the typing keyboard
  for (const k of o.hoverKey != null ? [o.hoverKey, ...(o.lit ?? [])] : o.lit ?? []) {
    const y = rollY(G, k);
    if (y > H || y + G.rowH < 0) continue;
    g.fillStyle = rgba(T.blue, 0.45); g.fillRect(0, y + 0.5, G.kw - 1, G.rowH - 1);
  }
}
