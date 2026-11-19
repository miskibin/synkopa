// Multiband window. The three bands are zones on a frequency axis, split where the crossovers
// sit (drag the dividers). Each zone holds its two thresholds as lines (drag them up and down):
// orange = downward (peaks above it are pulled down), blue = upward (detail below it is lifted).
// The column in the middle of a zone is that band right now: the tick is its level going in, the
// arrow how far the compressor moves it (blue up, orange down) and the number the same in dB.
// The output spectrum sits faintly behind, so you see what each band holds.
//
// Meters (dsp/multiband.rs): [0..3] band level in dB (after the band drive), [3..6] gain dB,
// [6..9] band level out dB, [9]/[10] crossover Hz the filters are at.
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { fx2x, x2f } from '../../lib/eq';
import { clamp } from '../../lib/format';
import { BAND_F, MONO10, rgba, rr, SANS10, SpecSmoother, T, TeleGate, type Viz } from './common';

export const MULTIBAND_COLOR = '#ff9e3d';
export const UP_COLOR = '#4fb6ff';
export const DOWN_COLOR = '#ff8a3d';
const ZONE = ['#ff6f61', '#ffb347', '#ffe066'];
const DB_LO = -84, DB_HI = 6;
const IDS = ['l', 'm', 'h'];

export interface MbGeom { x0: number; y0: number; w: number; h: number }
export const mbGeom = (W: number, H: number): MbGeom => ({ x0: 34, y0: 8, w: W - 34 - 8, h: H - 8 - 20 });
const yOf = (db: number, G: MbGeom) => G.y0 + (1 - clamp((db - DB_LO) / (DB_HI - DB_LO), 0, 1)) * G.h;
export const dbOf = (y: number, G: MbGeom) => DB_LO + (1 - (y - G.y0) / G.h) * (DB_HI - DB_LO);
const xOf = (f: number, G: MbGeom) => G.x0 + fx2x(f, G.w);
export const fOf = (x: number, G: MbGeom) => x2f(clamp(x - G.x0, 0, G.w), G.w);

/** The zone edges in Hz. */
const edges = (fx: Fx): [number, number, number, number] => [20, fx.params.xlo ?? 88.3, fx.params.xhi ?? 2500, 20000];

/** What the pointer is over: a crossover (`xlo`/`xhi`) or a threshold (`dl`…`uh`). */
export function mbHit(fx: Fx, x: number, y: number, W: number, H: number): string | null {
  const G = mbGeom(W, H), e = edges(fx);
  let best: string | null = null, bd = 7;
  for (const [id, f] of [['xlo', e[1]], ['xhi', e[2]]] as const) {
    const d = Math.abs(x - xOf(f, G));
    if (d < bd) { bd = d; best = id; }
  }
  for (let b = 0; b < 3; b++) {
    if (x < xOf(e[b], G) || x > xOf(e[b + 1], G)) continue;
    for (const k of ['d', 'u']) {
      const id = k + IDS[b], d = Math.abs(y - yOf(fx.params[id] ?? -40, G));
      if (d < bd) { bd = d; best = id; }
    }
  }
  return best;
}

export class MultibandViz implements Viz {
  cursor = 'default';
  private gate = new TeleGate();
  private spec = new SpecSmoother(128, 30, 0.6, 20);
  private m = new Float32Array(11);
  private live = false;
  hover: string | null = null;
  drag: string | null = null;

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
    const G = mbGeom(W, H), e = edges(fx);
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    // zones
    for (let b = 0; b < 3; b++) {
      const a = xOf(e[b], G), z = xOf(e[b + 1], G);
      g.fillStyle = rgba(ZONE[b], 0.06); g.fillRect(a, G.y0, z - a, G.h);
    }
    // dB grid
    g.font = MONO10; g.lineWidth = 1;
    for (let d = -72; d <= 0; d += 12) {
      const yy = Math.round(yOf(d, G)) + 0.5;
      g.strokeStyle = d === 0 ? T.vizGrid2 : T.vizGrid; g.beginPath(); g.moveTo(G.x0, yy); g.lineTo(G.x0 + G.w, yy); g.stroke();
      g.fillStyle = T.vizLabel; g.textAlign = 'right'; g.fillText(String(d), G.x0 - 5, yy + 3); g.textAlign = 'left';
    }
    for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) {
      const xx = Math.round(xOf(f, G)) + 0.5;
      g.fillStyle = T.vizLabel; g.fillText(f >= 1000 ? f / 1000 + 'k' : String(f), xx + 3, H - 6);
      g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(xx, G.y0); g.lineTo(xx, G.y0 + G.h); g.stroke();
    }
    if (fx.bypass) g.globalAlpha = 0.5;
    // the output spectrum, faint (4.5 dB/oct tilt, as the other analysers)
    g.beginPath();
    for (let k = 0; k < 128; k++) {
      const xx = xOf(BAND_F[k], G), yy = yOf(this.spec.cur[k] + 4.5 * Math.log2(BAND_F[k] / 1000) + 6, G);
      k ? g.lineTo(xx, yy) : g.moveTo(xx, yy);
    }
    g.lineTo(G.x0 + G.w, G.y0 + G.h); g.lineTo(G.x0, G.y0 + G.h); g.closePath();
    g.fillStyle = rgba(T.fg, 0.07); g.fill();
    // bands
    for (let b = 0; b < 3; b++) {
      const a = xOf(e[b], G), z = xOf(e[b + 1], G), zw = z - a;
      const down = fx.params['d' + IDS[b]] ?? -33.8, up = fx.params['u' + IDS[b]] ?? -40.8;
      const thr = (db: number, col: string, id: string, label: string) => {
        const yy = Math.round(yOf(db, G)) + 0.5, on = this.hover === id || this.drag === id;
        g.strokeStyle = rgba(col, on ? 1 : 0.75); g.lineWidth = on ? 2 : 1.2;
        g.setLineDash(on ? [] : [5, 4]);
        g.beginPath(); g.moveTo(a + 3, yy); g.lineTo(z - 3, yy); g.stroke(); g.setLineDash([]);
        if (zw > 70 || on) {
          g.font = MONO10; g.fillStyle = rgba(col, on ? 1 : 0.85);
          g.fillText(label + ' ' + Math.round(db), a + 6, yy + (id[0] === 'd' ? -4 : 12));
        }
      };
      thr(down, DOWN_COLOR, 'd' + IDS[b], '↓');
      thr(up, UP_COLOR, 'u' + IDS[b], '↑');
      // the band now
      const cw = clamp(zw * 0.3, 10, 40), cx = a + zw / 2;
      rr(g, cx - cw / 2, G.y0 + 2, cw, G.h - 4, 4); g.fillStyle = rgba(ZONE[b], 0.06); g.fill();
      if (!this.live) continue;
      const inDb = this.m[b], gain = this.m[3 + b];
      if (inDb < DB_LO) continue;
      const yi = yOf(inDb, G), yo = yOf(inDb + gain, G);
      // the level after the move, as a soft bar
      const top = Math.min(yi, yo);
      rr(g, cx - cw / 2 + 3, top, cw - 6, G.y0 + G.h - 2 - top, 3);
      g.fillStyle = rgba(ZONE[b], 0.28); g.fill();
      // the move
      if (Math.abs(gain) > 0.3) {
        const col = gain > 0 ? UP_COLOR : DOWN_COLOR;
        g.fillStyle = rgba(col, 0.85);
        g.fillRect(cx - 2, Math.min(yi, yo), 4, Math.abs(yo - yi));
        g.beginPath();
        const dir = gain > 0 ? -1 : 1;
        g.moveTo(cx - 6, yo - dir * 7); g.lineTo(cx + 6, yo - dir * 7); g.lineTo(cx, yo + dir); g.closePath(); g.fill();
      }
      g.fillStyle = T.fgHi; g.fillRect(cx - cw / 2 + 1, Math.round(yi) - 1, cw - 2, 2);
      g.font = MONO10; g.textAlign = 'center';
      g.fillStyle = gain > 0.3 ? UP_COLOR : gain < -0.3 ? DOWN_COLOR : T.vizLabel;
      g.fillText((gain >= 0 ? '+' : '−') + Math.abs(gain).toFixed(1), cx, Math.max(G.y0 + 10, Math.min(yi, yo) - 8));
      g.textAlign = 'left';
    }
    g.globalAlpha = 1;
    // crossovers
    for (const [id, f] of [['xlo', e[1]], ['xhi', e[2]]] as const) {
      const xx = Math.round(xOf(f, G)) + 0.5, on = this.hover === id || this.drag === id;
      g.strokeStyle = on ? T.fgHi : T.vizAxis; g.lineWidth = on ? 2 : 1;
      g.beginPath(); g.moveTo(xx, G.y0); g.lineTo(xx, G.y0 + G.h); g.stroke();
      rr(g, xx - 4, G.y0 + G.h / 2 - 12, 8, 24, 3); g.fillStyle = on ? T.fgHi : T.vizAxis; g.fill();
      if (on) {
        g.font = MONO10; g.fillStyle = T.fgHi;
        g.fillText(f >= 1000 ? (f / 1000).toFixed(2) + ' kHz' : Math.round(f) + ' Hz', xx + 7, G.y0 + 12);
      }
    }
    void SANS10;
  }
}
