// Master window, three parts:
//
// * Balance (left): the song's third-octave balance against the assistant's reference (0 dB):
//   grey = the mix as it reaches the effect, amber = after the master, both from the last
//   listen; blue = the tone EQ the knobs make now, with a handle on each of its five bands (drag
//   up / down, double-click = 0). The two dashed lines are the glue's crossovers (drag sideways).
//   Hovering reads mix / master / EQ at that frequency. The live output spectrum sits faintly
//   behind while the song plays.
// * Dynamics (right), Maximus-style: each band's level now (bar) against its glue threshold (the
//   handle: drag up / down) and the reduction it gets (red from the level down), then the
//   clipper's and the limiter's reduction, and the loudness in → out against the aim.
// * History (bottom): the last ten seconds, Pro-L style: input peaks (grey), output peaks
//   (amber) and the clipper + limiter reduction (red, from the top).
//
// Meters (dsp/master.rs): [0..3] band gain dB (≤ 0), [3] clipper dB, [4] limiter dB, [5] input
// loudness, [6] output loudness (before level match), [7] level-match gain dB, [8..11] band
// detector level dB.
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { fx2x, x2f } from '../../lib/eq';
import { clamp } from '../../lib/format';
import { MASTER_TARGETS, THIRDS, TONE_BANDS, toneDb } from '../../lib/master';
import { t } from '../../lib/i18n';
import { BAND_F, MONO10, MONO9, rgba, rr, SANS10, SpecSmoother, specTilt, T, TeleGate, type Viz } from './common';

export const MASTER_COLOR = '#f2b84b';
const AFTER = '#f2b84b';
const EQC = '#4fb6ff';
const GRC = '#ff6f61';
const ZONE = ['#ff6f61', '#ffb347', '#ffe066'];
const DB = 12;
const GR_MAX = 12;
const LVL_LO = -60;
const HIST = 600; // ~10 s of 60 Hz frames
const TONE_NAMES = ['Sub', 'Low', 'Mid', 'Presence', 'Air'];

export interface MsGeom {
  /** balance plot */ bx: number; by: number; bw: number; bh: number;
  /** dynamics column */ dx: number; dw: number;
  /** history strip */ hy: number; hh: number;
}
export function msGeom(W: number, H: number): MsGeom {
  const hh = 64, side = Math.min(270, Math.max(200, W * 0.3));
  const top = H - hh - 8;
  return { bx: 32, by: 18, bw: W - side - 32 - 14, bh: top - 18 - 32, dx: W - side, dw: side - 6, hy: top + 4, hh };
}
const bxOf = (f: number, G: MsGeom) => G.bx + fx2x(f, G.bw);
const byOf = (db: number, G: MsGeom) => G.by + (1 - (clamp(db, -DB, DB) + DB) / (2 * DB)) * G.bh;
export const msFreq = (x: number, G: MsGeom) => x2f(clamp(x - G.bx, 0, G.bw), G.bw);
export const msBalDb = (y: number, G: MsGeom) => -DB + (1 - (y - G.by) / G.bh) * 2 * DB;

/** Dynamics column: three band columns, then clip / limit, then the loudness readout. */
function dynCols(G: MsGeom) {
  const cw = 22, gap = 10, x0 = G.dx + 28;
  const band = [0, 1, 2].map(b => x0 + b * (cw + gap));
  const g0 = x0 + 3 * (cw + gap) + 6;
  return { cw, band, gr: [g0, g0 + 24], grw: 18, lx: g0 + 24 + 18 + 12, y0: G.by, h: G.bh };
}
const lvlY = (db: number, top: number, h: number) => top + (1 - (clamp(db, LVL_LO, 0) - LVL_LO) / -LVL_LO) * h;
export const msLvlDb = (y: number, G: MsGeom) => LVL_LO + (1 - (y - G.by) / G.bh) * -LVL_LO;

const THR = ['tl', 'tm', 'th'];

/** What the pointer is over: a tone band (`sub`…`air`), a crossover (`xlo`/`xhi`) or a band threshold (`tl`/`tm`/`th`). */
export function msHit(fx: Fx, x: number, y: number, W: number, H: number): string | null {
  const G = msGeom(W, H);
  if (x >= G.dx) {
    const D = dynCols(G);
    for (let b = 0; b < 3; b++) {
      if (x < D.band[b] - 6 || x > D.band[b] + D.cw + 4) continue;
      const ty = lvlY(fx.params[THR[b]] ?? 0, D.y0, D.h);
      if (Math.abs(y - ty) < 8) return THR[b];
    }
    return null;
  }
  if (y < G.by - 4 || y > G.by + G.bh + 4) return null;
  let best: string | null = null, bd = 9;
  for (const [id, , f] of TONE_BANDS) {
    const d = Math.hypot(x - bxOf(f, G), y - byOf(toneDb(fx.params, f), G));
    if (d < bd) { bd = d; best = id; }
  }
  if (best) return best;
  for (const id of ['xlo', 'xhi']) {
    if (Math.abs(x - bxOf(fx.params[id] ?? (id === 'xlo' ? 120 : 2800), G)) < 6) return id;
  }
  return null;
}

const fmtHz = (f: number) => (f >= 1000 ? (f / 1000).toFixed(f >= 10000 ? 1 : 2) + ' kHz' : Math.round(f) + ' Hz');
const sgn = (v: number) => (v >= 0 ? '+' : '−') + Math.abs(v).toFixed(1);

/** Linear interpolation of a third-octave series at `f`. */
function atThird(vals: number[], f: number): number {
  const lf = Math.log2(f);
  for (let i = 0; i < THIRDS.length - 1; i++) {
    const a = Math.log2(THIRDS[i]), b = Math.log2(THIRDS[i + 1]);
    if (lf <= b) return vals[i] + (vals[i + 1] - vals[i]) * clamp((lf - a) / (b - a), 0, 1);
  }
  return vals[vals.length - 1];
}

export class MasterViz implements Viz {
  cursor = 'default';
  hover: string | null = null;
  drag: string | null = null;
  /** pointer over the display (for the balance readout), css px */
  mouse: { x: number; y: number } | null = null;
  private gate = new TeleGate();
  private spec = new SpecSmoother(128, 30, 0.6, 20);
  private m = new Float32Array(11);
  private live = false;
  // history ring: input peak, output peak (linear), reduction dB (≥ 0)
  private hIn = new Float32Array(HIST);
  private hOut = new Float32Array(HIST);
  private hGr = new Float32Array(HIST);
  private hPos = 0;
  private hSeq = -1;

  ingest(a: FxFrame) {
    if (a.seq === this.hSeq) return;
    this.hSeq = a.seq;
    const i = this.hPos % HIST;
    this.hIn[i] = a.inPeak; this.hOut[i] = a.outPeak;
    this.hGr[i] = Math.max(0, -(a.meters[3] + a.meters[4]));
    this.hPos++;
  }

  update(_fx: Fx, a: FxFrame | null, now: number): boolean {
    let changed = false;
    if (this.gate.fresh(a, 30)) {
      this.live = true;
      this.m.set(a!.meters.subarray(0, 11));
      this.spec.feed(a!.specOut);
      changed = true;
    } else if (!a && this.live) { this.live = false; this.spec.silence(); changed = true; }
    if (this.spec.step(now)) changed = true;
    return changed;
  }

  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    const G = msGeom(W, H);
    this.drawBalance(g, G, fx);
    this.drawDynamics(g, G, W, fx);
    this.drawHistory(g, G, W, fx);
  }

  private drawBalance(g: CanvasRenderingContext2D, G: MsGeom, fx: Fx) {
    const { bx, by, bw, bh } = G;
    g.font = MONO10; g.lineWidth = 1;
    for (let d = -DB; d <= DB; d += 6) {
      const yy = Math.round(byOf(d, G)) + 0.5;
      g.strokeStyle = d === 0 ? T.vizGrid2 : T.vizGrid; g.beginPath(); g.moveTo(bx, yy); g.lineTo(bx + bw, yy); g.stroke();
      g.fillStyle = T.vizLabel; g.textAlign = 'right'; g.fillText((d > 0 ? '+' : '') + d, bx - 4, yy + 3); g.textAlign = 'left';
    }
    for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) {
      const xx = Math.round(bxOf(f, G)) + 0.5;
      g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(xx, by); g.lineTo(xx, by + bh); g.stroke();
      g.fillStyle = T.vizLabel; g.fillText(f >= 1000 ? f / 1000 + 'k' : String(f), xx + 3, by + bh + 12);
    }
    g.font = SANS10; g.fillStyle = T.vizLabel; g.fillText(t('c.ms.balance'), bx, 12);
    // glue bands as faint zones between the crossovers
    const xs = [20, fx.params.xlo ?? 120, fx.params.xhi ?? 2800, 20000];
    for (let b = 0; b < 3; b++) {
      const a = bxOf(xs[b], G), z = bxOf(xs[b + 1], G);
      g.fillStyle = rgba(ZONE[b], 0.035); g.fillRect(a, by, z - a, bh);
      // the band's width and level, if they are not neutral
      const w = fx.params[['wl', 'wm', 'wh'][b]] ?? 100, lv = fx.params[['gl', 'gm', 'gh'][b]] ?? 0;
      const bits = [w === 0 ? 'mono' : Math.round(w) !== 100 ? '↔ ' + Math.round(w) + '%' : '', Math.abs(lv) >= 0.05 ? sgn(lv) + ' dB' : ''].filter(Boolean);
      if (bits.length && z - a > 44) { g.font = MONO9; g.fillStyle = rgba(ZONE[b], 0.9); g.textAlign = 'center'; g.fillText(bits.join(' · '), (a + z) / 2, by + 10); g.textAlign = 'left'; }
    }
    if (fx.bypass) g.globalAlpha = 0.5;
    if (this.live && this.spec.cur.some(v => v > -100)) {
      let mean = 0, n = 0;
      for (let k = 0; k < 128; k++) if (BAND_F[k] >= 100 && BAND_F[k] <= 5000) { mean += this.spec.cur[k] + specTilt(k); n++; }
      mean /= Math.max(1, n);
      g.beginPath();
      for (let k = 0; k < 128; k++) { const xx = bxOf(BAND_F[k], G), yy = byOf(this.spec.cur[k] + specTilt(k) - mean, G); k ? g.lineTo(xx, yy) : g.moveTo(xx, yy); }
      g.lineTo(bx + bw, by + bh); g.lineTo(bx, by + bh); g.closePath();
      g.fillStyle = rgba(T.fg, 0.06); g.fill();
    }
    const L = fx.listen, has = !!L && L.balance.length === THIRDS.length, hasOut = has && L!.balanceOut.length === THIRDS.length;
    const fAt = (i: number) => 20 * Math.pow(1000, i / 120);
    const curve = (vals: number[]) => {
      g.beginPath();
      for (let i = 0; i <= 120; i++) { const xx = bxOf(fAt(i), G), yy = byOf(atThird(vals, fAt(i)), G); i ? g.lineTo(xx, yy) : g.moveTo(xx, yy); }
    };
    if (has) {
      if (hasOut) {
        // what the master moved, filled between mix and master
        g.beginPath();
        for (let i = 0; i <= 120; i++) { const xx = bxOf(fAt(i), G), yy = byOf(atThird(L!.balanceOut, fAt(i)), G); i ? g.lineTo(xx, yy) : g.moveTo(xx, yy); }
        for (let i = 120; i >= 0; i--) g.lineTo(bxOf(fAt(i), G), byOf(atThird(L!.balance, fAt(i)), G));
        g.closePath(); g.fillStyle = rgba(AFTER, 0.10); g.fill();
      }
      curve(L!.balance); g.strokeStyle = rgba(T.fg, 0.5); g.lineWidth = 1.5; g.lineJoin = 'round'; g.stroke();
      if (hasOut) { curve(L!.balanceOut); g.strokeStyle = AFTER; g.lineWidth = 2; g.stroke(); }
    } else {
      g.font = SANS10; g.fillStyle = T.vizLabel; g.textAlign = 'center';
      g.fillText(t('c.ms.noBalance'), bx + bw / 2, by + bh / 2 - 22); g.textAlign = 'left';
    }
    // tone EQ curve + its handles
    g.beginPath();
    for (let i = 0; i <= 120; i++) { const xx = bxOf(fAt(i), G), yy = byOf(toneDb(fx.params, fAt(i)), G); i ? g.lineTo(xx, yy) : g.moveTo(xx, yy); }
    g.strokeStyle = rgba(EQC, 0.9); g.lineWidth = 1.4; g.stroke();
    TONE_BANDS.forEach(([id, , f], i) => {
      const xx = bxOf(f, G), yy = byOf(toneDb(fx.params, f), G), on = this.hover === id || this.drag === id;
      g.beginPath(); g.arc(xx, yy, on ? 7 : 5.5, 0, Math.PI * 2);
      g.fillStyle = on ? EQC : rgba(EQC, 0.25); g.fill();
      g.strokeStyle = EQC; g.lineWidth = 1.5; g.stroke();
      if (on) {
        const v = (fx.params[id] ?? 0) * clamp((fx.params.tone ?? 100) / 100, 0, 1);
        this.tag(g, `${TONE_NAMES[i]} ${fmtHz(f)} ${sgn(v)} dB`, xx, yy - 14, G);
      }
    });
    g.globalAlpha = 1;
    // crossovers
    for (const id of ['xlo', 'xhi']) {
      const f = fx.params[id] ?? (id === 'xlo' ? 120 : 2800), xx = Math.round(bxOf(f, G)) + 0.5, on = this.hover === id || this.drag === id;
      g.strokeStyle = on ? T.fgHi : rgba(T.fg, 0.28); g.lineWidth = on ? 2 : 1; g.setLineDash(on ? [] : [3, 4]);
      g.beginPath(); g.moveTo(xx, by); g.lineTo(xx, by + bh); g.stroke(); g.setLineDash([]);
      rr(g, xx - 3, by + bh - 22, 6, 18, 2); g.fillStyle = on ? T.fgHi : rgba(T.fg, 0.35); g.fill();
      if (on) this.tag(g, fmtHz(f), xx, by + 14, G);
    }
    // legend
    g.font = MONO9; let lx = bx + bw; g.textAlign = 'right';
    for (const [label, col] of [[t('c.ms.tone'), EQC], [t('c.ms.after'), AFTER], [t('c.ms.before'), rgba(T.fg, 0.6)]] as const) {
      g.fillStyle = col; g.fillText(label, lx, 12); lx -= g.measureText(label).width + 12;
    }
    g.textAlign = 'left';
    // hover readout
    const mo = this.mouse;
    if (mo && !this.hover && !this.drag && mo.x >= bx && mo.x <= bx + bw && mo.y >= by && mo.y <= by + bh) {
      const f = msFreq(mo.x, G), xx = Math.round(mo.x) + 0.5;
      g.strokeStyle = rgba(T.fg, 0.25); g.lineWidth = 1; g.beginPath(); g.moveTo(xx, by); g.lineTo(xx, by + bh); g.stroke();
      const parts = [fmtHz(f)];
      if (hasOut) parts.push(`${t('c.ms.before')} ${sgn(atThird(L!.balance, f))}`, `${t('c.ms.after')} ${sgn(atThird(L!.balanceOut, f))}`);
      parts.push(`EQ ${sgn(toneDb(fx.params, f))}`);
      this.tag(g, parts.join(' · '), mo.x, by + bh - 8, G);
    }
  }

  /** A small label box centred at x, kept inside the plot. */
  private tag(g: CanvasRenderingContext2D, s: string, x: number, y: number, G: MsGeom) {
    g.font = MONO10;
    const w = g.measureText(s).width + 10, xx = clamp(x - w / 2, G.bx + 2, G.bx + G.bw - w - 2);
    rr(g, xx, y - 11, w, 16, 4); g.fillStyle = rgba(T.viz, 0.92); g.fill();
    g.strokeStyle = rgba(T.fg, 0.2); g.lineWidth = 1; g.stroke();
    g.fillStyle = T.fgHi; g.fillText(s, xx + 5, y + 1);
  }

  private drawDynamics(g: CanvasRenderingContext2D, G: MsGeom, W: number, fx: Fx) {
    const D = dynCols(G);
    g.strokeStyle = T.vizGrid; g.lineWidth = 1; g.beginPath(); g.moveTo(G.dx - 6 + 0.5, 6); g.lineTo(G.dx - 6 + 0.5, G.hy - 4); g.stroke();
    g.font = SANS10; g.fillStyle = T.vizLabel; g.fillText(t('c.ms.glue'), D.band[0], 12);
    g.fillText(t('c.ms.gr'), D.gr[0] - 2, 12);
    // level scale
    g.font = MONO9; g.textAlign = 'right';
    for (const d of [0, -12, -24, -36, -48]) { g.fillStyle = T.vizLabel; g.fillText(String(d), D.band[0] - 7, lvlY(d, D.y0, D.h) + 3); }
    g.textAlign = 'left';
    const labels = ['L', 'M', 'H'];
    for (let b = 0; b < 3; b++) {
      const x = D.band[b], thr = fx.params[THR[b]] ?? 0;
      rr(g, x, D.y0, D.cw, D.h, 3); g.fillStyle = rgba(ZONE[b], 0.06); g.fill();
      if (this.live && this.m[8 + b] > LVL_LO) {
        const lv = this.m[8 + b], gr = Math.min(0, this.m[b]);
        const yl = lvlY(lv, D.y0, D.h), yo = lvlY(lv + gr, D.y0, D.h);
        g.fillStyle = rgba(ZONE[b], 0.55); g.fillRect(x + 3, yo, D.cw - 6, D.y0 + D.h - yo);
        if (yo - yl > 0.5) { g.fillStyle = rgba(GRC, 0.85); g.fillRect(x + 3, yl, D.cw - 6, yo - yl); }
      }
      // threshold handle (dim when the band's glue is off)
      const on = this.hover === THR[b] || this.drag === THR[b], ty = Math.round(lvlY(thr, D.y0, D.h)) + 0.5;
      const col = on ? T.fgHi : thr >= -0.05 ? rgba(T.fg, 0.35) : '#ff8a3d';
      g.strokeStyle = col; g.fillStyle = col; g.lineWidth = on ? 2 : 1.5;
      g.beginPath(); g.moveTo(x - 3, ty); g.lineTo(x + D.cw + 3, ty); g.stroke();
      g.beginPath(); g.moveTo(x - 6, ty - 4); g.lineTo(x - 1, ty); g.lineTo(x - 6, ty + 4); g.closePath(); g.fill();
      g.font = MONO9; g.fillStyle = T.vizLabel; g.textAlign = 'center';
      g.fillText(labels[b], x + D.cw / 2, D.y0 + D.h + 11);
      const grv = this.live ? -this.m[b] : 0;
      g.fillStyle = on || grv > 0.05 ? T.fgHi : T.vizLabel;
      g.fillText(on ? (thr >= -0.05 ? t('c.ms.off') : thr.toFixed(1)) : grv > 0.05 ? grv.toFixed(1) : '–', x + D.cw / 2, D.y0 + D.h + 22);
      g.textAlign = 'left';
    }
    // clip / limit reduction, from the top
    const grs = [this.m[3], this.m[4]], names = [t('c.ms.clip'), t('c.ms.lim')], cols = ['#c58cff', GRC];
    for (let i = 0; i < 2; i++) {
      const x = D.gr[i];
      rr(g, x, D.y0, D.grw, D.h, 3); g.fillStyle = rgba(T.fg, 0.05); g.fill();
      const v = this.live ? clamp(-grs[i], 0, GR_MAX) : 0;
      if (v > 0.05) { g.fillStyle = rgba(cols[i], 0.85); g.fillRect(x + 2, D.y0, D.grw - 4, D.h * v / GR_MAX); }
      g.font = MONO9; g.textAlign = 'center'; g.fillStyle = T.vizLabel;
      g.fillText(names[i], x + D.grw / 2, D.y0 + D.h + 11);
      g.fillStyle = v > 0.05 ? T.fgHi : T.vizLabel; g.fillText(v > 0.05 ? v.toFixed(1) : '–', x + D.grw / 2, D.y0 + D.h + 22);
      g.textAlign = 'left';
    }
    // loudness
    const lx = D.lx; if (W - lx < 34) return;
    const tgt = Math.round(fx.params.target ?? 1);
    const aim = fx.listen?.guarded && fx.listen.target === tgt && fx.listen.aim != null ? fx.listen.aim : MASTER_TARGETS[tgt] ?? -12;
    const fmt = (v: number) => (this.live && v > -70 ? v.toFixed(1) : '–');
    g.font = MONO9; g.fillStyle = T.vizLabel; g.fillText('LUFS', lx, 12);
    const row = (label: string, v: number, y: number, col: string) => {
      g.font = MONO9; g.fillStyle = T.vizLabel; g.fillText(label, lx, y);
      g.font = MONO10; g.fillStyle = col; g.fillText(fmt(v), lx, y + 12);
    };
    row(t('c.ms.in'), this.m[5], 32, T.fg);
    row(t('c.ms.out'), this.m[6], 62, this.live && Math.abs(this.m[6] - aim) < 1 ? '#5fbf8a' : T.fgHi);
    g.font = MONO9; g.fillStyle = T.vizLabel; g.fillText('→ ' + (Number.isInteger(aim) ? String(aim) : aim.toFixed(1)), lx, 92);
    if (Math.abs(this.m[7]) > 0.05 && this.live) { g.fillStyle = EQC; g.fillText('= ' + this.m[7].toFixed(1), lx, 106); }
  }

  private drawHistory(g: CanvasRenderingContext2D, G: MsGeom, W: number, fx: Fx) {
    const x0 = G.bx, w = W - G.bx - 8, y0 = G.hy, h = G.hh;
    rr(g, x0, y0, w, h, 4); g.fillStyle = rgba(T.fg, 0.03); g.fill();
    g.font = MONO9; g.fillStyle = T.vizLabel; g.fillText(t('c.ms.history'), x0 + 6, y0 + 11);
    const n = Math.min(this.hPos, HIST); if (n < 2) return;
    // peaks on a dB scale (−36…0 dBFS), mirrored around the middle
    const ceil = fx.params.ceil ?? -1;
    const half = (h - 4) / 2, cy = y0 + h / 2, step = w / (HIST - 1);
    const amp = (p: number) => clamp((20 * Math.log10(Math.max(p, 1e-6)) + 36) / 36, 0, 1) * half;
    const xAt = (k: number) => x0 + w - (n - 1 - k) * step;
    const idx = (k: number) => (this.hPos - n + k) % HIST;
    const env = (buf: Float32Array, col: string) => {
      g.beginPath();
      for (let k = 0; k < n; k++) { const a = amp(buf[idx(k)]); k ? g.lineTo(xAt(k), cy - a) : g.moveTo(xAt(k), cy - a); }
      for (let k = n - 1; k >= 0; k--) g.lineTo(xAt(k), cy + amp(buf[idx(k)]));
      g.closePath(); g.fillStyle = col; g.fill();
    };
    env(this.hIn, rgba(T.fg, 0.12));
    env(this.hOut, rgba(AFTER, 0.55));
    // the ceiling
    const cA = amp(Math.pow(10, ceil / 20));
    g.strokeStyle = rgba(GRC, 0.45); g.lineWidth = 1; g.setLineDash([2, 3]);
    for (const yy of [cy - cA, cy + cA]) { g.beginPath(); g.moveTo(x0, Math.round(yy) + 0.5); g.lineTo(x0 + w, Math.round(yy) + 0.5); g.stroke(); }
    g.setLineDash([]);
    // clipper + limiter reduction, from the top
    g.beginPath(); g.moveTo(xAt(0), y0);
    for (let k = 0; k < n; k++) g.lineTo(xAt(k), y0 + clamp(this.hGr[idx(k)] / GR_MAX, 0, 1) * h * 0.5);
    g.lineTo(x0 + w, y0); g.closePath(); g.fillStyle = rgba(GRC, 0.5); g.fill();
  }
}
