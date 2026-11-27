// Patina window: one column per module, each drawing what that module is doing right now, from
// the engine's readings (crates/engine/src/dsp/patina.rs `meter`):
// [0] noise peak, [1]/[2] wobble pitch L/R in cents, [3] peak into the driven band,
// [4] Digital sample rate Hz, [5] bits, [6]/[7] dropout depth L/R, [8] wear low-pass Hz,
// [9] wear peak compression dB, [10..15] module amounts after Magnitude and Wander, [15] Magnitude,
// [16] clicks started (a counter).
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { clamp, dbg } from '../../lib/format';
import { t as tr, tName } from '../../lib/i18n';
import { patinaScaled, toCents, wowDepth } from '../../lib/patina';
import { satDriveDb } from '../../lib/saturator';
import { MONO9, rgba, T, type Viz } from './common';

/** Module accents, in chain order: noise, wobble, drive, digital, wear. */
export const PATINA_COLORS = ['#e8705a', '#e8a64a', '#e8cf5a', '#c9d1dc', '#6fa8ff'];
const NOISE_TYPES = ['Winyl', 'Winyl — kurz', 'Szelak 78', 'Kaseta', 'Taśma szpulowa', 'Brum'];
const STYLES = ['Taśma', 'Konsola', 'Transformator', 'Trioda', 'Pentoda'];
const TAPES = ['Kaseta', 'Taśma 19 cm/s', 'Taśma 38 cm/s'];
const HIST = 120;
const M = { NOISE: 0, CL: 1, CR: 2, DRIVE_IN: 3, SRATE: 4, BITS: 5, DL: 6, DR: 7, WEAR_HZ: 8, WEAR_GR: 9, EFF: 10, MAG: 15, CLICKS: 16 };

/** A fixed-length history, newest last. */
class Ring {
  v: Float32Array;
  constructor(n = HIST) { this.v = new Float32Array(n); }
  push(x: number) { this.v.copyWithin(0, 1); this.v[this.v.length - 1] = x; }
  clear() { this.v.fill(0); }
}

const fmtHz = (f: number) => (f >= 1000 ? (f / 1000).toFixed(f >= 10000 ? 0 : 1) + ' kHz' : Math.round(f) + ' Hz');

export class PatinaViz implements Viz {
  cursor = 'default';
  private seq = -1;
  private noise = new Ring();
  private cl = new Ring();
  private cr = new Ring();
  private drop = [new Ring(), new Ring()];
  private eff = new Float32Array(5);
  private driveIn = 0;
  private srate = 12000;
  private bits = 12;
  private wearHz = 20000;
  private wearGr = 0;
  private clicks = -1;
  private clickRate = 0;
  private lastAt = 0;
  private live = false;

  ingest(a: FxFrame, at: number) {
    const m = a.meters;
    this.noise.push(m[M.NOISE]);
    this.cl.push(m[M.CL]); this.cr.push(m[M.CR]);
    this.drop[0].push(m[M.DL]); this.drop[1].push(m[M.DR]);
    const c = m[M.CLICKS];
    if (this.clicks >= 0 && at > this.lastAt) {
      const d = c >= this.clicks ? c - this.clicks : c + 16777216 - this.clicks;
      this.clickRate += (d / ((at - this.lastAt) / 1000) - this.clickRate) * 0.15;
    }
    this.clicks = c; this.lastAt = at;
  }

  update(_fx: Fx, a: FxFrame | null) {
    if (!a || a.seq === this.seq) return false;
    this.seq = a.seq;
    const m = a.meters;
    for (let i = 0; i < 5; i++) this.eff[i] = m[M.EFF + i];
    this.driveIn += (m[M.DRIVE_IN] - this.driveIn) * 0.5;
    this.srate = m[M.SRATE] || this.srate; this.bits = m[M.BITS] || this.bits;
    this.wearHz = m[M.WEAR_HZ] || this.wearHz; this.wearGr = m[M.WEAR_GR];
    this.live = true;
    return true;
  }

  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    const p = fx.params;
    // before the first reading, the amounts the knobs ask for
    const want = patinaScaled(p);
    const eff = this.live ? Array.from(this.eff) : want.slice(0, 5);
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    // the same 8 px gutters as the module columns under the picture
    const gap = 8, cw = (W - gap * 6) / 5, top = 20, bot = 16, ch = H - top - bot;
    const titles = [tr('c.fx.pt.noise'), tr('c.fx.pt.wobble'), tr('c.fx.pt.drive'), tr('c.fx.pt.digital'), tr('c.fx.pt.wear')];
    for (let k = 0; k < 5; k++) {
      const x = gap + k * (cw + gap), on = eff[k] > 0.002 && !fx.bypass;
      const col = fx.bypass ? T.vizLabel : PATINA_COLORS[k];
      g.fillStyle = T.vizPanel; g.beginPath(); g.roundRect(x, top, cw, ch, 4); g.fill();
      // title + the module's amount as a thin bar under it
      g.font = MONO9; g.textAlign = 'left'; g.fillStyle = on ? col : T.vizLabel;
      g.fillText(titles[k], x + 2, 12);
      g.textAlign = 'right'; g.fillStyle = T.vizLabel;
      g.fillText(on ? Math.round(eff[k] * 100) + '%' : tr('c.fx.pt.off'), x + cw - 2, 12);
      g.fillStyle = T.vizGrid; g.fillRect(x, 15, cw, 2);
      g.fillStyle = rgba(col, on ? 0.9 : 0.3); g.fillRect(x, 15, cw * clamp(eff[k], 0, 1), 2);
      g.save(); g.beginPath(); g.rect(x, top, cw, ch); g.clip();
      g.globalAlpha = on ? 1 : 0.35;
      const cx = x + cw / 2;
      if (k === 0) this.drawNoise(g, x, top, cw, ch, col);
      else if (k === 1) this.drawWobble(g, x, top, cw, ch, col, wowDepth(eff[1]));
      else if (k === 2) this.drawDrive(g, x, top, cw, ch, col, p, eff[2]);
      else if (k === 3) this.drawDigital(g, x, top, cw, ch, col, eff[3]);
      else this.drawWear(g, x, top, cw, ch, col);
      g.restore();
      // the module's setting under its panel
      g.globalAlpha = 1; g.font = MONO9; g.textAlign = 'center'; g.fillStyle = T.vizLabel;
      g.fillText(this.caption(k, p), cx, H - 4);
    }
    g.textAlign = 'left';
  }

  private caption(k: number, p: Record<string, number>): string {
    if (k === 0) {
      const ty = clamp(Math.round(p.ntype ?? 0), 0, 5), rate = this.clickRate;
      return tName(NOISE_TYPES[ty]) + (rate > 0.5 && this.eff[0] > 0.002 ? ' · ' + tr('c.fx.pt.clicks', { n: Math.round(rate) }) : '');
    }
    if (k === 1) {
      const ct = toCents(wowDepth(this.live ? this.eff[1] : patinaScaled(p)[1]));
      return '±' + ct.toFixed(ct < 10 ? 1 : 0) + ' ct · ' + (p.wrate ?? 0.6).toFixed(2) + ' Hz';
    }
    if (k === 2) return tName(STYLES[clamp(Math.round(p.dstyle ?? 0), 0, 4)]) + ((p.dlow ?? 20) > 20.5 || (p.dhigh ?? 20000) < 19900 ? ' · ' + fmtHz(p.dlow ?? 20) + '–' + fmtHz(p.dhigh ?? 20000) : '');
    if (k === 3) return fmtHz(this.live ? this.srate : p.srate ?? 12000) + ' · ' + (this.live ? this.bits : p.bits ?? 12).toFixed(1) + ' bit';
    return tName(TAPES[clamp(Math.round(p.tape ?? 0), 0, 2)]) + ' · ↓' + fmtHz(this.wearHz) + (this.wearGr < -0.1 ? ' · ' + this.wearGr.toFixed(1) + ' dB' : '');
  }

  /** The noise heard over the last frames, as a waveform envelope (it rides Follow/Duck too). */
  private drawNoise(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, col: string) {
    // −72..0 dBFS: a hiss floor reads low, crackle peaks reach up, Follow and Duck show as it breathes
    const v = this.noise.v, n = v.length, mid = y + h / 2;
    g.beginPath();
    for (let i = 0; i < n; i++) {
      const db = 20 * Math.log10(Math.max(1e-6, v[i]));
      const px = x + i / (n - 1) * w, a = Math.max(0.5, clamp((db + 72) / 72, 0, 1) * h * 0.47);
      g.moveTo(px, mid - a); g.lineTo(px, mid + a);
    }
    g.strokeStyle = rgba(col, 0.85); g.lineWidth = Math.max(1, w / n * 0.8); g.stroke();
    g.strokeStyle = T.vizGrid; g.lineWidth = 1; g.beginPath(); g.moveTo(x, mid + 0.5); g.lineTo(x + w, mid + 0.5); g.stroke();
  }

  /** Pitch deviation over time, left (bright) and right (dim); the scale is the set depth. */
  private drawWobble(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, col: string, depth: number) {
    const range = Math.max(4, toCents(depth) * 1.15), mid = y + h / 2;
    g.strokeStyle = T.vizGrid; g.lineWidth = 1;
    g.beginPath(); g.moveTo(x, mid + 0.5); g.lineTo(x + w, mid + 0.5); g.stroke();
    for (const [ring, a] of [[this.cr, 0.35], [this.cl, 1]] as const) {
      const v = ring.v, n = v.length;
      g.beginPath();
      for (let i = 0; i < n; i++) {
        const px = x + i / (n - 1) * w, py = mid - clamp(v[i] / range, -1, 1) * h * 0.42;
        i ? g.lineTo(px, py) : g.moveTo(px, py);
      }
      g.strokeStyle = rgba(col, a); g.lineWidth = 1.6; g.stroke();
    }
  }

  /** The drive's curve at its drive, with the level going into it marked on the curve. */
  private drawDrive(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, col: string, p: Record<string, number>, amt: number) {
    const s = Math.min(w, h) - 10, sx = x + (w - s) / 2, sy = y + (h - s) / 2;
    const dr = dbg(Math.min(satDriveDb({ drive: amt * 100 }), 40) * 0.6);
    const style = clamp(Math.round(p.dstyle ?? 0), 0, 4), asym = [0.18, 0.04, 0.12, 0.3, 0.02][style];
    const f = (u: number) => { const xb = u * dr + asym * Math.min(1, dr / 4) * (1 - u * u); return Math.tanh(clamp(xb, -1.2, 1.2) * 2.2) / Math.tanh(2.2); };
    g.strokeStyle = T.vizGrid; g.lineWidth = 1;
    g.beginPath(); g.moveTo(sx, sy + s); g.lineTo(sx + s, sy); g.stroke();
    g.beginPath();
    for (let i = 0; i <= 60; i++) {
      const u = i / 30 - 1, px = sx + i / 60 * s, py = sy + s / 2 - f(u) * s / 2 * 0.92;
      i ? g.lineTo(px, py) : g.moveTo(px, py);
    }
    g.strokeStyle = col; g.lineWidth = 1.8; g.stroke();
    const u = clamp(this.driveIn, 0, 1);
    if (u > 0.003) {
      const px = sx + (u + 1) / 2 * s, py = sy + s / 2 - f(u) * s / 2 * 0.92;
      g.fillStyle = T.fgHi; g.beginPath(); g.arc(px, py, 3, 0, Math.PI * 2); g.fill();
      g.strokeStyle = rgba(col, 0.6); g.lineWidth = 2; g.beginPath(); g.arc(px, py, 5.5, 0, Math.PI * 2); g.stroke();
    }
  }

  /** A sine as the module holds and quantises it: the stair steps are the rate, their heights the bits. */
  private drawDigital(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, col: string, amt: number) {
    const mid = y + h / 2, amp = h * 0.38;
    // two cycles of a 1 kHz sine shown at 48 kHz: the steps per cycle are what the rate leaves
    const steps = clamp(this.srate / 1000, 1.2, 48), levels = 2 ** (this.bits - 1);
    g.beginPath();
    for (let i = 0; i <= 80; i++) {
      const t = i / 80 * 2, px = x + 4 + i / 80 * (w - 8), py = mid - Math.sin(t * Math.PI * 2) * amp;
      i ? g.lineTo(px, py) : g.moveTo(px, py);
    }
    g.strokeStyle = T.vizGrid2; g.lineWidth = 1; g.stroke();
    g.beginPath();
    const n = Math.ceil(steps * 2);
    for (let k = 0; k <= n; k++) {
      const t0 = k / steps, t1 = Math.min(2, (k + 1) / steps);
      if (t0 > 2) break;
      const v = Math.round(Math.sin(t0 * Math.PI * 2) * levels) / levels;
      const shown = Math.sin(t0 * Math.PI * 2) + (v - Math.sin(t0 * Math.PI * 2)) * Math.max(0.25, amt);
      const x0 = x + 4 + t0 / 2 * (w - 8), x1 = x + 4 + t1 / 2 * (w - 8), py = mid - shown * amp;
      k ? g.lineTo(x0, py) : g.moveTo(x0, py);
      g.lineTo(x1, py);
    }
    g.strokeStyle = col; g.lineWidth = 1.6; g.stroke();
  }

  /** Dropouts over time (dips from the top line), left bright, right dim. */
  private drawWear(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, col: string) {
    const y0 = y + h * 0.3, span = h * 0.6;
    g.strokeStyle = T.vizGrid; g.lineWidth = 1;
    g.beginPath(); g.moveTo(x, y0 + 0.5); g.lineTo(x + w, y0 + 0.5); g.stroke();
    for (const [ring, a] of [[this.drop[1], 0.35], [this.drop[0], 1]] as const) {
      const v = ring.v, n = v.length;
      g.beginPath();
      for (let i = 0; i < n; i++) {
        const px = x + i / (n - 1) * w, py = y0 + clamp(v[i], 0, 1) * span;
        i ? g.lineTo(px, py) : g.moveTo(px, py);
      }
      g.strokeStyle = rgba(col, a); g.lineWidth = 1.6; g.stroke();
    }
    // the head loss as a high-frequency roll-off sketch in the top band
    const r = clamp(Math.log(this.wearHz / 1000) / Math.log(22), 0, 1);
    g.fillStyle = rgba(col, 0.25); g.fillRect(x + 4, y + 6, (w - 8) * r, 4);
    g.fillStyle = rgba(col, 0.7); g.fillRect(x + 4 + (w - 8) * r - 1, y + 4, 2, 8);
  }
}
