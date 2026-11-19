// Stereo window, and the vectorscope and correlation meter the Analyzer shares.
//
// Vectorscope ("polar sample", as Ozone Imager draws it): every output sample is a dot. Mono sits
// on the vertical line, a signal only on the left leans 45° left, only on the right 45° right,
// out of phase lies flat. Points below the axis are mirrored up (the picture is symmetric), so a
// half disc holds it all. Dots fade over ~0.25 s (phosphor), so the shape reads like a cloud.
//
// Meters (dsp/stereo.rs): [0] correlation out, [1] balance −1..1, [2] side/mid dB out, [3] bass
// (< 120 Hz) correlation, [4] correlation in, [5] side/mid dB in, [6] bass mono Hz, [7] bass
// present (1). X/Y points come from the stereo output tap (FxFrame.xy).
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { clamp } from '../../lib/format';
import { t as tr } from '../../lib/i18n';
import { theme } from '../../ui/theme';
import { MONO10, rgba, rr, SANS10, T, TeleGate, type Viz } from './common';

export const STEREO_COLOR = '#f58bd5';

/** Fades older dots out: alpha a frame keeps (at ~60 fps, ~0.25 s to fade). */
const KEEP = 0.82;

/** The persistent dot layer of a polar vectorscope. */
export class Vectorscope {
  private cv: HTMLCanvasElement | null = null;
  private w = 0; private h = 0; private epoch = -1;
  /** Peak-following zoom: quiet material still fills the disc. */
  private scale = 1;
  /** Plot `xy` (n interleaved L, R pairs) onto the layer, fading what was there. */
  feed(xy: Float32Array, n: number, W: number, H: number, color: string) {
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(W * dpr)), h = Math.max(1, Math.round(H * dpr));
    if (!this.cv) this.cv = document.createElement('canvas');
    if (this.w !== w || this.h !== h || this.epoch !== theme.epoch) {
      this.cv.width = w; this.cv.height = h; this.w = w; this.h = h; this.epoch = theme.epoch;
    }
    const g = this.cv.getContext('2d')!;
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.globalCompositeOperation = 'destination-in';
    g.fillStyle = `rgba(0,0,0,${KEEP})`; g.fillRect(0, 0, w, h);
    g.globalCompositeOperation = 'source-over';
    if (!n) return;
    let peak = 0;
    for (let i = 0; i < 2 * n; i++) peak = Math.max(peak, Math.abs(xy[i]));
    // zoom out at once on a louder passage, in slowly when it gets quieter (a quiet vocal
    // still fills the disc; digital silence leaves the zoom alone)
    if (peak > 1e-5) {
      const want = clamp(0.9 / peak, 1, 1000);
      this.scale = want < this.scale ? want : this.scale + (want - this.scale) * 0.04;
    }
    const cx = w / 2, cy = h - 2 * dpr, R = Math.min(w / 2, h) - 6 * dpr;
    const k = R * this.scale / Math.SQRT2;
    g.fillStyle = color;
    const s = Math.max(1, 1.3 * dpr);
    // brighter where dots pile up: additive blending on a dark plot (normal on light themes)
    g.globalAlpha = 0.55;
    if (!theme.light) g.globalCompositeOperation = 'lighter';
    for (let i = 0; i < n; i++) {
      const l = xy[2 * i], r = xy[2 * i + 1];
      let x = (r - l) * k, y = (l + r) * k;
      if (y < 0) { x = -x; y = -y; }
      const d = Math.hypot(x, y);
      if (d > R) { x *= R / d; y *= R / d; }
      g.fillRect(cx + x - s / 2, cy - y - s / 2, s, s);
    }
    g.globalAlpha = 1; g.globalCompositeOperation = 'source-over';
  }
  clear() { if (this.cv) this.cv.getContext('2d')!.clearRect(0, 0, this.w, this.h); }
  /** Grid, then the dots, into `x0, y0, W, H` (the half disc stands on the bottom edge). */
  draw(g: CanvasRenderingContext2D, x0: number, y0: number, W: number, H: number) {
    const cx = x0 + W / 2, cy = y0 + H - 2, R = Math.min(W / 2, H) - 6;
    g.save();
    g.strokeStyle = T.vizGrid2; g.lineWidth = 1;
    for (const f of [1, 0.66, 0.33]) { g.beginPath(); g.arc(cx, cy, R * f, Math.PI, 2 * Math.PI); g.stroke(); }
    g.beginPath(); g.moveTo(cx - R, cy); g.lineTo(cx + R, cy); g.stroke();
    // L, M, R axes
    g.strokeStyle = T.vizAxis;
    for (const a of [-45, 0, 45]) {
      const rad = (a - 90) * Math.PI / 180;
      g.beginPath(); g.moveTo(cx, cy); g.lineTo(cx + Math.cos(rad) * R, cy + Math.sin(rad) * R); g.stroke();
    }
    g.font = SANS10; g.fillStyle = T.vizLabel; g.textAlign = 'center';
    g.fillText('L', cx - R * 0.74, cy - R * 0.74 - 4);
    g.fillText('R', cx + R * 0.74, cy - R * 0.74 - 4);
    g.fillText('M', cx, cy - R - 3 < y0 + 8 ? y0 + 9 : cy - R - 3);
    g.textAlign = 'left';
    if (this.cv) g.drawImage(this.cv, x0, y0, W, H);
    g.restore();
  }
}

/** Correlation colour: red out of phase, amber borderline, green safe. */
export const corrColor = (c: number) => (c < 0 ? T.red : c < 0.25 ? T.amber : T.green);

/** Horizontal −1…+1 meter with a needle; `label` on the left, value on the right. */
export function corrBar(g: CanvasRenderingContext2D, x: number, y: number, w: number, value: number, label: string, dim = false) {
  const h = 6;
  g.font = SANS10; g.fillStyle = T.vizLabel; g.fillText(label, x, y - 4);
  const txt = (value >= 0 ? '+' : '') + value.toFixed(2);
  g.font = MONO10; g.fillStyle = dim ? T.vizLabel : corrColor(value);
  g.textAlign = 'right'; g.fillText(txt, x + w, y - 4); g.textAlign = 'left';
  rr(g, x, y, w, h, 3); g.fillStyle = T.vizGrid; g.fill();
  // zones
  const px = (v: number) => x + (v + 1) / 2 * w;
  g.fillStyle = rgba(T.red, 0.22); g.fillRect(x + 1, y + 1, px(0) - x - 1, h - 2);
  g.fillStyle = T.vizAxis; g.fillRect(Math.round(px(0)), y - 2, 1, h + 4);
  if (dim) return;
  const vx = px(clamp(value, -1, 1));
  g.fillStyle = corrColor(value);
  rr(g, vx - 2, y - 3, 4, h + 6, 2); g.fill();
}

/** −1 (left) … +1 (right) balance bar. */
export function balanceBar(g: CanvasRenderingContext2D, x: number, y: number, w: number, value: number) {
  const h = 6;
  g.font = SANS10; g.fillStyle = T.vizLabel; g.fillText(tr('c.st.balance'), x, y - 4);
  const db = Math.abs(value) < 1e-3 ? 'C' : (value < 0 ? 'L ' : 'R ') + Math.abs(10 * Math.log10((1 + Math.abs(value)) / Math.max(1e-6, 1 - Math.abs(value)))).toFixed(1) + ' dB';
  g.font = MONO10; g.fillStyle = T.fg3; g.textAlign = 'right'; g.fillText(db, x + w, y - 4); g.textAlign = 'left';
  rr(g, x, y, w, h, 3); g.fillStyle = T.vizGrid; g.fill();
  const mid = x + w / 2, vx = x + (clamp(value, -1, 1) + 1) / 2 * w;
  g.fillStyle = rgba(T.blue, 0.55); g.fillRect(Math.min(mid, vx), y + 1, Math.abs(vx - mid), h - 2);
  g.fillStyle = T.vizAxis; g.fillRect(Math.round(mid), y - 2, 1, h + 4);
}

export class StereoViz implements Viz {
  cursor = 'default';
  private gate = new TeleGate();
  private scope = new Vectorscope();
  private m = new Float32Array(8);
  private live = false;
  private pending: FxFrame | null = null;
  update(_fx: Fx, a: FxFrame | null): boolean {
    if (!this.gate.fresh(a, 30)) {
      if (!a && this.live) { this.live = false; return true; }
      return false;
    }
    this.live = true;
    this.m.set(a!.meters.subarray(0, 8));
    this.pending = a;
    return true;
  }
  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    const pad = 14, side = Math.min(240, Math.max(180, W * 0.36));
    const sx = W - side - pad, sw = side;
    const scopeW = sx - 2 * pad, scopeH = H - 2 * pad;
    if (this.pending) {
      this.scope.feed(this.pending.xy, this.pending.xyN, scopeW, scopeH, STEREO_COLOR);
      this.pending = null;
    }
    if (fx.bypass) g.globalAlpha = 0.5;
    this.scope.draw(g, pad, pad, scopeW, scopeH);
    g.globalAlpha = 1;
    // readouts: how correlated (mono-safe), how balanced, and whether the bass is mono
    const m = this.m, live = this.live;
    let y = pad + 26;
    corrBar(g, sx, y, sw, live ? m[0] : 1, tr('c.st.corr'), !live);
    if (live && Math.abs(m[0] - m[4]) > 0.03) {
      // where it was before this effect
      const px = sx + (clamp(m[4], -1, 1) + 1) / 2 * sw;
      g.strokeStyle = rgba(T.fg, 0.55); g.lineWidth = 1.5;
      g.beginPath(); g.moveTo(px, y - 2); g.lineTo(px, y + 8); g.stroke();
      g.font = MONO10; g.fillStyle = T.vizLabel; g.fillText(tr('c.st.from', { v: (m[4] >= 0 ? '+' : '') + m[4].toFixed(2) }), sx, y + 22);
    }
    y += 54;
    balanceBar(g, sx, y, sw, live ? m[1] : 0);
    y += 54;
    const bassLive = live && m[7] > 0.5;
    corrBar(g, sx, y, sw, bassLive ? m[3] : 1, tr('c.st.bass'), !bassLive);
    if (bassLive) {
      g.font = SANS10;
      const ok = m[3] > 0.9;
      g.fillStyle = ok ? T.green : T.amber;
      g.fillText(ok ? tr('c.st.bassMono') : tr('c.st.bassWide'), sx, y + 22);
    }
  }
}
