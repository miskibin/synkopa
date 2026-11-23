// Gate window: the transfer curve (left: input dB → output dB, the hard gate's step or the
// expander's slope down to the floor), and a scrolling history (right): what the detector hears
// (grey), the gain the gate applies (red, from the top), the open threshold (solid) and the close
// threshold (dashed) with the hysteresis band between them shaded, so you see why it opens and
// closes. A lamp shows open / hold / closed. Meters (crates/engine/src/dsp/gate.rs): [0] detector
// peak dBFS, [1] gain dB, [2] state (0 closed, 1 open, 2 hold, 3 closing), [3] 1 = sidechain.
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { clamp } from '../../lib/format';
import { t as tr } from '../../lib/i18n';
import { Layer } from '../../ui/frameLoop';
import { MONO10, MONO11, rgba, T, TeleGate, type Viz } from './common';
import { gateCurve } from './gateCurve';

export const GATE_COLOR = '#e0c34a';
const HN = 300;
const LO = -72;

export class GateViz implements Viz {
  cursor = 'default';
  private layer = new Layer();
  private gate = new TeleGate();
  private hdet = new Float32Array(HN).fill(LO);
  private hgain = new Float32Array(HN);
  private n = 0; private head = 0;
  private det = LO; private gain = 0; private state = 0; private sc = false;

  update(fx: Fx, a: FxFrame | null): boolean {
    if (!this.gate.fresh(a, HN)) return false;
    const m = a!.meters;
    this.det = Math.max(LO, m[0] ?? LO);
    this.gain = Math.min(0, m[1] ?? 0);
    this.state = Math.round(m[2] ?? 0);
    this.sc = fx.key != null && (m[3] ?? 0) > 0.5;
    this.hdet[this.head] = this.det;
    this.hgain[this.head] = this.gain;
    this.head = (this.head + 1) % HN; this.n = Math.min(HN, this.n + 1);
    return true;
  }

  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    const p = fx.params;
    const thr = p.thr ?? -40, close = thr - (p.hyst ?? 4);
    const S = Math.min(H, W * 0.3), u = (d: number) => clamp((d - LO) / -LO, 0, 1) * S;
    const x0 = S + 14, HW = W - x0 - 26;
    const yh = (d: number) => H - clamp((d - LO) / -LO, 0, 1) * H;
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    g.drawImage(this.layer.get(W, H, 'gate' + JSON.stringify(p), c => {
      c.lineWidth = 1; c.strokeStyle = T.vizGrid;
      for (let d = -60; d < 0; d += 12) { const q = u(d); c.beginPath(); c.moveTo(q, 0); c.lineTo(q, S); c.moveTo(0, S - q); c.lineTo(S, S - q); c.stroke(); }
      c.strokeStyle = T.vizAxis; c.strokeRect(0.5, 0.5, S - 1, S - 1);
      c.strokeStyle = T.vizGrid2; c.beginPath(); c.moveTo(0, S); c.lineTo(S, 0); c.stroke();
      c.beginPath();
      for (let i = 0; i <= 144; i++) {
        const d = LO + i / 2, y = gateCurve(p, d);
        const yy = S - u(Number.isFinite(y) ? y : LO - 10);
        i ? c.lineTo(u(d), yy) : c.moveTo(u(d), yy);
      }
      c.strokeStyle = T.ink; c.lineWidth = 2; c.stroke();
      // the thresholds across the history: shaded hysteresis band, solid open, dashed close
      c.fillStyle = rgba(GATE_COLOR, 0.1);
      c.fillRect(x0, yh(thr), HW, Math.max(1, yh(close) - yh(thr)));
      c.strokeStyle = GATE_COLOR; c.lineWidth = 1;
      c.beginPath(); c.moveTo(x0, Math.round(yh(thr)) + 0.5); c.lineTo(x0 + HW, Math.round(yh(thr)) + 0.5); c.stroke();
      c.setLineDash([3, 3]);
      c.beginPath(); c.moveTo(x0, Math.round(yh(close)) + 0.5); c.lineTo(x0 + HW, Math.round(yh(close)) + 0.5); c.stroke();
      c.beginPath(); c.moveTo(u(thr), 0); c.lineTo(u(thr), S); c.stroke();
      c.setLineDash([]);
      c.font = MONO10; c.fillStyle = T.vizLabel;
      c.fillText(thr.toFixed(0) + ' dB', x0 + HW - 44, yh(thr) - 3);
    }) as CanvasImageSource, 0, 0, W, H);
    if (fx.bypass) g.globalAlpha = 0.5;
    if (this.det > LO) {
      const y = gateCurve(p, this.det);
      g.beginPath(); g.arc(u(this.det), S - u(Number.isFinite(y) ? y : LO), 5, 0, Math.PI * 2); g.fillStyle = T.blue; g.fill();
    }
    const L = this.n;
    if (L > 1) {
      const at = (i: number) => (this.head - L + i + HN) % HN;
      const px = (i: number) => x0 + i / (HN - 1) * HW;
      // the detector's level
      g.beginPath(); g.moveTo(x0, H);
      for (let i = 0; i < L; i++) g.lineTo(px(i), yh(this.hdet[at(i)]));
      g.lineTo(px(L - 1), H); g.closePath();
      g.fillStyle = rgba(T.post, 0.18); g.fill(); g.strokeStyle = rgba(T.post, 0.55); g.lineWidth = 1; g.stroke();
      // the gain, hanging from the top (0 dB) down to −72
      g.beginPath(); g.moveTo(x0, 0);
      for (let i = 0; i < L; i++) g.lineTo(px(i), clamp(-this.hgain[at(i)] / -LO, 0, 1) * H);
      g.lineTo(px(L - 1), 0); g.closePath();
      g.fillStyle = rgba(T.red, 0.35); g.fill();
    }
    // gain bar
    const gx = W - 16;
    g.fillStyle = T.vizPanel; g.fillRect(gx, 0, 12, H);
    g.fillStyle = T.red; g.fillRect(gx, 0, 12, clamp(-this.gain / -LO, 0, 1) * H);
    g.globalAlpha = 1;
    // lamp: green open, amber hold / closing, red closed
    const [col, lab] = this.state === 1 ? [T.green, 'c.fx.gate.open'] : this.state === 2 || this.state === 3 ? [T.amber, 'c.fx.gate.hold'] : [T.red, 'c.fx.gate.closed'];
    g.beginPath(); g.arc(x0 + 12, 14, 5, 0, Math.PI * 2); g.fillStyle = fx.bypass ? T.vizGrid2 : col; g.fill();
    g.font = MONO11; g.fillStyle = T.ink;
    g.fillText(tr(lab as 'c.fx.gate.open'), x0 + 22, 18);
    g.fillStyle = T.fg4;
    const gr = this.gain < -0.05 ? (this.gain <= -119 ? '−∞' : this.gain.toFixed(1)) : '0.0';
    g.fillText('GR ' + gr + ' dB', x0 + 6, 34);
    g.fillText((this.sc ? 'SC ' : 'IN ') + (this.det > LO ? this.det.toFixed(1) : '-∞') + ' dB', x0 + 6, 48);
    if ((p.listen ?? 0) >= 0.5) { g.fillStyle = GATE_COLOR; g.fillText(tr('c.fx.gate.listen'), x0 + 6, 62); }
  }
}
