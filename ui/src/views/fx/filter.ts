// Filter window: the frequency response the filter makes at its set cutoff (outline), and the
// response right now with the LFO and envelope moving it, left (bright) and right (dim), plus the
// range the modulation can sweep (bar along the bottom). Meters (crates/engine/src/dsp/filter.rs):
// [0]/[1] cutoff left/right now Hz, [2] envelope 0..1, [3] LFO phase, [4] DJ: how much of the
// filter is heard, [5] 1 = soft clip working, [6] 1 = sidechain.
//
// The curve is the engine's: trapezoidal (bilinear) integrators make the digital response equal
// the analogue prototype at the pre-warped frequency Ω = tan(πf/sr) / tan(πfc/sr).
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { clamp } from '../../lib/format';
import { fx2x } from '../../lib/eq';
import { t as tr } from '../../lib/i18n';
import { audioStatus } from '../../state/audioWatch';
import { freqGrid, MONO10, rgba, T, TeleGate, type Viz } from './common';

export const FILTER_COLOR = '#ff7a59';
const SR = 48000;
export const [LP, LADDER, HP, BP, NOTCH, DJ] = [0, 1, 2, 3, 4, 5];

/** dsp::filter::svf_q: Q 0.5 at 0 % resonance to 64 at 100 %. */
export const svfQ = (res: number) => 0.5 * Math.pow(2, 7 * clamp(res, 0, 1));

/** dsp::filter::dj_map: the DJ knob (−1..1) → base cutoff, the side's mode, how much is heard. */
export function djMap(pos: number): { fc: number; mode: number; wet: number } {
  const m = Math.abs(pos), t = clamp((m - 0.01) / 0.99, 0, 1);
  let w = clamp((m - 0.01) / 0.04, 0, 1); w = w * w * (3 - 2 * w);
  return pos < 0 ? { fc: 20000 * Math.pow(2, -9.966 * t), mode: LP, wet: w } : { fc: 20 * Math.pow(2, 9.966 * t), mode: HP, wet: w };
}

type C = [number, number];
const mul = (a: C, b: C): C => [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]];
const div = (a: C, b: C): C => { const d = b[0] * b[0] + b[1] * b[1]; return [(a[0] * b[0] + a[1] * b[1]) / d, (a[1] * b[0] - a[0] * b[1]) / d]; };

/** One SVF stage (k = 1/Q) in `mode` at normalised s = jΩ, as `Svf::run` mixes its outputs. */
function svfStage(om: number, k: number, mode: number): C {
  const s2: C = [-om * om, 0];
  const den: C = [1 - om * om, k * om];
  if (mode === HP) return div(s2, den);
  if (mode === BP) return div([0, om * Math.sqrt(k)], den);
  if (mode === NOTCH) return div([1 - om * om, 0], den);
  return div([1, 0], den);
}

/** The rate the engine runs at: the response near the top depends on it (tan(πf/sr)). */
const liveRate = () => { const r = audioStatus()?.actualRate ?? 0; return r > 0 ? r : SR; };

/** |H(f)| in dB of the filter with these settings at cutoff `fc` (linear: drive and clip left out). */
export function filterResponseDb(f: number, fc: number, type: number, res: number, slope24: boolean, sr = SR, wet = 1): number {
  let mode = type, fcut = fc;
  if (type === DJ) mode = fc < 0 ? LP : HP;
  fcut = Math.abs(fcut);
  const g = Math.tan(Math.PI * clamp(fcut, 16, Math.min(0.45 * sr, 21000)) / sr);
  const om = Math.tan(Math.PI * Math.min(f, 0.4999 * sr) / sr) / g;
  let h: C;
  if (mode === LADDER) {
    const k = 4.15 * clamp(res, 0, 1);
    // (1 + 0.5k) / ((1 + s)^4 + k)
    let p: C = [1, 0];
    for (let i = 0; i < 4; i++) p = mul(p, [1, om]);
    h = div([1 + 0.5 * k, 0], [p[0] + k, p[1]]);
  } else {
    const kq = 1 / svfQ(res);
    h = slope24 ? mul(svfStage(om, Math.SQRT2, mode), svfStage(om, kq, mode)) : svfStage(om, kq, mode);
  }
  const y: C = [1 + wet * (h[0] - 1), wet * h[1]];
  return 10 * Math.log10(Math.max(1e-12, y[0] * y[0] + y[1] * y[1]));
}

/** The settings the curve needs from the parameters (DJ: the knob gives cutoff, side and blend). */
export function filterShape(fx: Fx): { type: number; fc: number; res: number; slope24: boolean; wet: number } {
  const p = fx.params, type = Math.round(p.type ?? 1), res = clamp((p.res ?? 25) / 100, 0, 1), slope24 = (p.slope ?? 1) >= 0.5;
  if (type === DJ) {
    const d = djMap(clamp((p.dj ?? 0) / 100, -1, 1));
    return { type, fc: d.mode === LP ? -d.fc : d.fc, res, slope24, wet: d.wet };
  }
  return { type, fc: p.cutoff ?? 1600, res, slope24, wet: 1 };
}

export class FilterViz implements Viz {
  cursor = 'default';
  private gate = new TeleGate();
  private fc: [number, number] = [0, 0];
  private live = false; private clip = false; private env = 0; private sr = SR;
  update(_fx: Fx, a: FxFrame | null): boolean {
    const sr = liveRate(), moved = sr !== this.sr;
    this.sr = sr;
    if (!a) { const was = this.live; this.live = false; return was || moved; }
    if (!this.gate.fresh(a, 4)) return moved;
    this.live = true;
    this.fc = [a.meters[0] || 1000, a.meters[1] || 1000];
    this.env = a.meters[2] ?? 0;
    this.clip = (a.meters[5] ?? 0) > 0.5;
    return true;
  }
  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    freqGrid(g, W, H);
    const top = 10, bot = H - 20;
    const y = (db: number) => top + (1 - clamp((db + 36) / 54, 0, 1)) * (bot - top);
    g.font = MONO10; g.lineWidth = 1;
    for (const d of [-24, -12, 0, 12]) {
      const yy = Math.round(y(d)) + 0.5;
      g.strokeStyle = d ? T.vizGrid : T.vizGrid2; g.beginPath(); g.moveTo(0, yy); g.lineTo(W, yy); g.stroke();
      g.fillStyle = T.vizLabel; g.fillText((d > 0 ? '+' : '') + d + ' dB', 4, yy - 3);
    }
    const sh = filterShape(fx);
    const curve = (fc: number, col: string, width: number, fill: boolean) => {
      g.beginPath();
      for (let i = 0; i <= 240; i++) {
        const f = 20 * Math.pow(1000, i / 240), xx = fx2x(f, W), yy = y(filterResponseDb(f, fc, sh.type, sh.res, sh.slope24, this.sr, sh.wet));
        i ? g.lineTo(xx, yy) : g.moveTo(xx, yy);
      }
      if (fill) { g.lineTo(W, bot); g.lineTo(0, bot); g.closePath(); g.fillStyle = rgba(col, 0.1); g.fill(); }
      else { g.strokeStyle = col; g.lineWidth = width; g.stroke(); }
    };
    if (fx.bypass) g.globalAlpha = 0.5;
    const p = fx.params;
    const moving = Math.abs(p.lfo ?? 0) > 0.005 || Math.abs(p.env ?? 0) > 0.005;
    // DJ keeps its side's sign through the live cutoff (negative = low-pass)
    const live = (hz: number) => (sh.type === DJ && sh.fc < 0 ? -hz : hz);
    curve(sh.fc, FILTER_COLOR, 0, true);
    if (this.live && moving) {
      curve(sh.fc, rgba(FILTER_COLOR, 0.45), 1, false);
      curve(live(this.fc[1]), rgba(FILTER_COLOR, 0.5), 1.5, false);
      curve(live(this.fc[0]), FILTER_COLOR, 2, false);
    } else {
      curve(sh.fc, FILTER_COLOR, 2, false);
    }
    // the range the LFO and the envelope can sweep (octaves around the cutoff)
    if (moving && sh.type !== DJ) {
      const lfo = Math.abs(p.lfo ?? 0), env = p.env ?? 0;
      const lo = Math.abs(sh.fc) * Math.pow(2, -lfo + Math.min(0, env)), hi = Math.abs(sh.fc) * Math.pow(2, lfo + Math.max(0, env));
      g.fillStyle = rgba(FILTER_COLOR, 0.18);
      g.fillRect(fx2x(Math.max(20, lo), W), bot + 2, Math.max(2, fx2x(Math.min(20000, hi), W) - fx2x(Math.max(20, lo), W)), 4);
      if (this.live && Math.abs(env) > 0.005) {
        g.fillStyle = FILTER_COLOR;
        g.fillRect(W - 14, bot - clamp(this.env, 0, 1) * (bot - top), 6, clamp(this.env, 0, 1) * (bot - top));
      }
    }
    g.globalAlpha = 1;
    g.fillStyle = T.vizLabel; g.font = MONO10;
    if (sh.type === DJ && sh.wet < 0.001) g.fillText(tr('c.fx.filter.djOff'), 36, 14);
    if (this.live && this.clip && (p.clip ?? 1) >= 0.5) {
      g.fillStyle = T.red; g.fillText(tr('c.fx.filter.clip').toUpperCase(), W - 44, 14);
    }
  }
}
