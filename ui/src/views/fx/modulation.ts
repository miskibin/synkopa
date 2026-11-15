// Chorus and Phaser windows.
// Chorus: where each voice reads the delay line right now (dots), the range its LFO sweeps
// (bars), left on top and right below, on a millisecond axis. Meters: [0] LFO phase, [1..4] left
// voices' delay ms, [4..7] right voices', [7] voices (crates/engine/src/dsp/modulation.rs).
// Phaser: the frequency response the stages make at this moment, left (bright) and right
// (dim), with the notches moving as the LFO sweeps. Meters: [0]/[1] break frequency left/right,
// [2] stages, [3] LFO phase. The curve is computed here from the same formulas as the DSP.
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { clamp } from '../../lib/format';
import { fx2x } from '../../lib/eq';
import { t as tr } from '../../lib/i18n';
import { freqGrid, MONO10, rgba, SANS10, T, type Viz } from './common';

export const CHORUS_COLOR = '#6cc4ff';
export const PHASER_COLOR = '#c58cff';
const STAGE_N = [2, 4, 6, 8, 12];
const SR = 48000;

/** catalog.rs: the chorus's voices per mode, and how far the LFO swings them (dsp::modulation). */
export function chorusGeometry(fx: Fx): { voices: number; centres: number[]; swing: number } {
  const p = fx.params, mode = Math.round(p.mode ?? 0);
  const depth = clamp((p.depth ?? 40) / 100, 0, 1), base = clamp(p.delay ?? 12, 1, 40);
  const rate = (p.sync ?? 0) >= 0.5 ? 1 : clamp(p.rate ?? 0.6, 0.05, 10);
  if (mode === 2) {
    const swing = Math.min(0.06 * depth / (2 * Math.PI * rate), 0.012) * 1000;
    return { voices: 1, centres: [swing + 1], swing };
  }
  const swing = Math.min(depth * 0.25 * base, 6);
  return mode === 1 ? { voices: 3, centres: [base, base, base], swing } : { voices: 2, centres: [base, base * 1.37], swing };
}

export class ChorusViz implements Viz {
  cursor = 'default';
  private taps = new Float32Array(6);
  private live = false;
  update(_fx: Fx, a: FxFrame | null): boolean {
    if (!a) { const was = this.live; this.live = false; return was; }
    this.live = true;
    for (let i = 0; i < 6; i++) this.taps[i] = a.meters[1 + i] ?? 0;
    return true;
  }
  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    const { voices, centres, swing } = chorusGeometry(fx);
    const maxMs = Math.max(10, ...centres.map(c => c + swing)) * 1.15;
    const pad = 36, x = (ms: number) => pad + clamp(ms / maxMs, 0, 1) * (W - pad - 12);
    g.font = MONO10; g.lineWidth = 1;
    const step = maxMs > 40 ? 10 : maxMs > 16 ? 5 : 2;
    for (let ms = 0; ms <= maxMs; ms += step) {
      const xx = Math.round(x(ms)) + 0.5;
      g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(xx, 14); g.lineTo(xx, H - 18); g.stroke();
      g.fillStyle = T.vizLabel; g.fillText(ms + ' ms', xx + 3, H - 5);
    }
    if (fx.bypass) g.globalAlpha = 0.5;
    const lanes = [{ y: H * 0.34, lab: 'L', off: 0 }, { y: H * 0.68, lab: 'R', off: 3 }];
    const laneH = Math.min(26, H * 0.22);
    for (const ln of lanes) {
      g.fillStyle = T.vizLabel; g.font = SANS10; g.fillText(ln.lab, 12, ln.y + 4);
      for (let v = 0; v < voices; v++) {
        const yy = ln.y + (v - (voices - 1) / 2) * (laneH / Math.max(1, voices));
        // the range this voice sweeps
        g.fillStyle = rgba(CHORUS_COLOR, 0.16);
        const x0 = x(centres[v] - swing), x1 = x(centres[v] + swing);
        g.fillRect(x0, yy - 3, Math.max(2, x1 - x0), 6);
        // where it reads now
        if (this.live) {
          g.fillStyle = CHORUS_COLOR;
          g.beginPath(); g.arc(x(this.taps[ln.off + v]), yy, 4, 0, Math.PI * 2); g.fill();
        }
      }
    }
    g.globalAlpha = 1;
    g.fillStyle = T.vizLabel; g.font = MONO10;
    const mode = ['Chorus', 'Ensemble', 'Vibrato'][Math.round(fx.params.mode ?? 0)] ?? '';
    g.fillText(`${mode} · ${tr('c.fx.mod.voices', { n: voices })}`, pad, 11);
  }
}

/** |H(f)| in dB of the phaser with its stages at break frequency `fc` (linear, as the DSP without clipping). */
export function phaserResponseDb(f: number, fc: number, stages: number, fb: number, mix: number, sr = SR): number {
  const tn = Math.tan(Math.PI * clamp(fc, 20, 0.45 * sr) / sr), a = (tn - 1) / (tn + 1);
  const w = 2 * Math.PI * f / sr, zr = Math.cos(w), zi = -Math.sin(w); // z⁻¹
  // A = (a + z⁻¹) / (1 + a z⁻¹)
  const nr = a + zr, ni = zi, dr = 1 + a * zr, di = a * zi, dd = dr * dr + di * di;
  const ar = (nr * dr + ni * di) / dd, ai = (ni * dr - nr * di) / dd;
  // A^N
  let pr = 1, pi = 0;
  for (let i = 0; i < stages; i++) { const r = pr * ar - pi * ai; pi = pr * ai + pi * ar; pr = r; }
  // wet = A^N / (1 − fb · z⁻¹ · A^N)
  const qr = 1 - fb * (zr * pr - zi * pi), qi = -fb * (zr * pi + zi * pr), qd = qr * qr + qi * qi;
  const wr = (pr * qr + pi * qi) / qd, wi = (pi * qr - pr * qi) / qd;
  const hr = (1 - mix) + mix * wr, hi = mix * wi;
  return 10 * Math.log10(Math.max(1e-12, hr * hr + hi * hi));
}

export class PhaserViz implements Viz {
  cursor = 'default';
  private fc: [number, number] = [0, 0];
  private live = false;
  update(_fx: Fx, a: FxFrame | null): boolean {
    if (!a) { const was = this.live; this.live = false; return was; }
    this.live = true;
    this.fc = [a.meters[0] || 800, a.meters[1] || 800];
    return true;
  }
  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    freqGrid(g, W, H);
    const p = fx.params, stages = STAGE_N[Math.round(p.stages ?? 1)] ?? 4;
    const fb = clamp((p.fb ?? 30) / 100, -0.95, 0.95), mix = clamp((p.mix ?? 50) / 100, 0, 1);
    const top = 10, bot = H - 20;
    const y = (db: number) => top + (1 - clamp((db + 30) / 42, 0, 1)) * (bot - top);
    g.font = MONO10; g.lineWidth = 1;
    for (const d of [-24, -12, 0, 6]) {
      const yy = Math.round(y(d)) + 0.5;
      g.strokeStyle = d ? T.vizGrid : T.vizGrid2; g.beginPath(); g.moveTo(0, yy); g.lineTo(W, yy); g.stroke();
      g.fillStyle = T.vizLabel; g.fillText((d > 0 ? '+' : '') + d + ' dB', 4, yy - 3);
    }
    if (fx.bypass) g.globalAlpha = 0.5;
    // without audio the curve sits at the centre frequency
    const fcs = this.live ? this.fc : [p.center ?? 800, p.center ?? 800];
    const curve = (fc: number, col: string, width: number) => {
      g.strokeStyle = col; g.lineWidth = width; g.beginPath();
      for (let i = 0; i <= 240; i++) {
        const f = 20 * Math.pow(1000, i / 240), xx = fx2x(f, W), yy = y(phaserResponseDb(f, fc, stages, fb, mix));
        i ? g.lineTo(xx, yy) : g.moveTo(xx, yy);
      }
      g.stroke();
    };
    curve(fcs[1], rgba(PHASER_COLOR, 0.4), 1.5);
    curve(fcs[0], PHASER_COLOR, 2);
    // the sweep range
    const depth = clamp((p.depth ?? 70) / 100, 0, 1), c = p.center ?? 800;
    const lo = c * Math.pow(2, -3 * depth), hi = c * Math.pow(2, 3 * depth);
    g.fillStyle = rgba(PHASER_COLOR, 0.14);
    g.fillRect(fx2x(lo, W), bot + 2, Math.max(2, fx2x(hi, W) - fx2x(lo, W)), 4);
    g.globalAlpha = 1;
  }
}
