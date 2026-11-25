// Master window. Left: the song's balance against the reference the assistant uses (0 dB line):
// grey = the mix as it reaches the effect, coloured = after the master, both from the last
// listen; the thin curve is the tone EQ the knobs make now, and the live output spectrum sits
// faintly behind while the song plays. Right: what the chain is doing right now — each band's
// glue, the clipper and the limiter as gain-reduction bars (down from the top, 0…−12 dB), and
// the short-term loudness going in and coming out against the target.
//
// Meters (dsp/master.rs): [0..3] band gain dB (≤ 0), [3] clipper dB, [4] limiter dB, [5] input
// loudness, [6] output loudness (before level match), [7] level-match gain dB.
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { fx2x } from '../../lib/eq';
import { clamp } from '../../lib/format';
import { MASTER_TARGETS, THIRDS, toneDb } from '../../lib/master';
import { t } from '../../lib/i18n';
import { BAND_F, MONO10, MONO9, rgba, SANS10, SpecSmoother, specTilt, T, TeleGate, type Viz } from './common';

export const MASTER_COLOR = '#f2b84b';
const AFTER = '#f2b84b';
const BAR_COLORS = ['#ff6f61', '#ffb347', '#ffe066', '#c58cff', '#4fb6ff'];
const DB = 12;
const GR_MAX = 12;
const SIDE = 232;

export class MasterViz implements Viz {
  cursor = 'default';
  private gate = new TeleGate();
  private spec = new SpecSmoother(128, 30, 0.6, 20);
  private m = new Float32Array(8);
  private live = false;

  update(_fx: Fx, a: FxFrame | null, now: number): boolean {
    let changed = false;
    if (this.gate.fresh(a, 30)) {
      this.live = true;
      this.m.set(a!.meters.subarray(0, 8));
      this.spec.feed(a!.specOut);
      changed = true;
    } else if (!a && this.live) { this.live = false; this.spec.silence(); changed = true; }
    if (this.spec.step(now)) changed = true;
    return changed;
  }

  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    const pw = Math.max(120, W - SIDE - 10), x0 = 30, y0 = 18, w = pw - x0 - 6, h = H - y0 - 20;
    const xOf = (f: number) => x0 + fx2x(f, w);
    const yOf = (db: number) => y0 + (1 - (clamp(db, -DB, DB) + DB) / (2 * DB)) * h;
    // grid
    g.font = MONO10; g.lineWidth = 1;
    for (let d = -DB; d <= DB; d += 6) {
      const yy = Math.round(yOf(d)) + 0.5;
      g.strokeStyle = d === 0 ? T.vizGrid2 : T.vizGrid; g.beginPath(); g.moveTo(x0, yy); g.lineTo(x0 + w, yy); g.stroke();
      g.fillStyle = T.vizLabel; g.textAlign = 'right'; g.fillText((d > 0 ? '+' : '') + d, x0 - 4, yy + 3); g.textAlign = 'left';
    }
    for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) {
      const xx = Math.round(xOf(f)) + 0.5;
      g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(xx, y0); g.lineTo(xx, y0 + h); g.stroke();
      g.fillStyle = T.vizLabel; g.fillText(f >= 1000 ? f / 1000 + 'k' : String(f), xx + 3, H - 6);
    }
    g.font = SANS10; g.fillStyle = T.vizLabel; g.fillText(t('c.ms.balance'), x0, 12);
    // crossovers (where the glue's bands meet)
    for (const f of [fx.params.xlo ?? 120, fx.params.xhi ?? 2800]) {
      const xx = Math.round(xOf(f)) + 0.5;
      g.strokeStyle = rgba(T.fg, 0.18); g.setLineDash([2, 4]); g.beginPath(); g.moveTo(xx, y0); g.lineTo(xx, y0 + h); g.stroke(); g.setLineDash([]);
    }
    if (fx.bypass) g.globalAlpha = 0.5;
    // the live output, faint, around the reference (4.5 dB/oct tilt, levelled to the plot's middle)
    if (this.live && this.spec.cur.some(v => v > -100)) {
      let mean = 0, n = 0;
      for (let k = 0; k < 128; k++) if (BAND_F[k] >= 100 && BAND_F[k] <= 5000) { mean += this.spec.cur[k] + specTilt(k); n++; }
      mean /= Math.max(1, n);
      g.beginPath();
      for (let k = 0; k < 128; k++) {
        const xx = xOf(BAND_F[k]), yy = yOf(this.spec.cur[k] + specTilt(k) - mean);
        k ? g.lineTo(xx, yy) : g.moveTo(xx, yy);
      }
      g.lineTo(x0 + w, y0 + h); g.lineTo(x0, y0 + h); g.closePath();
      g.fillStyle = rgba(T.fg, 0.06); g.fill();
    }
    const L = fx.listen;
    if (L && L.balance.length === THIRDS.length) {
      const line = (vals: number[], col: string, lw: number) => {
        g.beginPath();
        vals.forEach((v, i) => { const xx = xOf(THIRDS[i]), yy = yOf(v); i ? g.lineTo(xx, yy) : g.moveTo(xx, yy); });
        g.strokeStyle = col; g.lineWidth = lw; g.lineJoin = 'round'; g.stroke();
      };
      line(L.balance, rgba(T.fg, 0.45), 1.5);
      if (L.balanceOut.length === THIRDS.length) line(L.balanceOut, AFTER, 2);
    } else {
      g.font = SANS10; g.fillStyle = T.vizLabel; g.textAlign = 'center';
      g.fillText(t('c.ms.noBalance'), x0 + w / 2, y0 + h / 2 - 16); g.textAlign = 'left';
    }
    // the tone EQ now
    g.beginPath();
    for (let i = 0; i <= 96; i++) {
      const f = 20 * Math.pow(1000, i / 96), xx = xOf(f), yy = yOf(toneDb(fx.params, f));
      i ? g.lineTo(xx, yy) : g.moveTo(xx, yy);
    }
    g.strokeStyle = rgba('#4fb6ff', 0.9); g.lineWidth = 1.2; g.stroke();
    // legend
    g.font = MONO9; let lx = x0 + w;
    g.textAlign = 'right';
    for (const [label, col] of [[t('c.ms.tone'), '#4fb6ff'], [t('c.ms.after'), AFTER], [t('c.ms.before'), rgba(T.fg, 0.6)]] as const) {
      g.fillStyle = col; g.fillText(label, lx, 12); lx -= g.measureText(label).width + 12;
    }
    g.textAlign = 'left';
    g.globalAlpha = 1;
    this.drawMeters(g, pw + 4, W, H, fx);
  }

  private drawMeters(g: CanvasRenderingContext2D, sx: number, W: number, H: number, fx: Fx) {
    const y0 = 18, h = H - y0 - 34, bw = 18, gap = 9;
    g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(sx - 4 + 0.5, 6); g.lineTo(sx - 4 + 0.5, H - 6); g.stroke();
    g.font = SANS10; g.fillStyle = T.vizLabel; g.fillText(t('c.ms.gr'), sx + 4, 12);
    const labels = ['L', 'M', 'H', t('c.ms.clip'), t('c.ms.lim')];
    const vals = [this.m[0], this.m[1], this.m[2], this.m[3], this.m[4]];
    for (let i = 0; i < 5; i++) {
      const x = sx + 4 + i * (bw + gap) + (i >= 3 ? 6 : 0);
      g.fillStyle = rgba(T.fg, 0.05); g.fillRect(x, y0, bw, h);
      const v = this.live ? clamp(-vals[i], 0, GR_MAX) : 0;
      if (v > 0.05) { g.fillStyle = rgba(BAR_COLORS[i], 0.85); g.fillRect(x, y0, bw, h * v / GR_MAX); }
      g.font = MONO9; g.fillStyle = T.vizLabel; g.textAlign = 'center';
      g.fillText(labels[i].slice(0, 5), x + bw / 2, y0 + h + 11);
      if (v > 0.05) { g.fillStyle = T.fgHi; g.fillText(v.toFixed(1), x + bw / 2, y0 + h + 22); }
      g.textAlign = 'left';
    }
    // loudness: in → out against the target
    const tgt = MASTER_TARGETS[Math.round(fx.params.target ?? 2)] ?? -12;
    const lx = sx + 4 + 5 * (bw + gap) + 14, lw = W - lx - 6;
    if (lw < 24) return;
    const fmt = (v: number) => (this.live && v > -70 ? v.toFixed(1) : '–');
    g.font = MONO9; g.fillStyle = T.vizLabel; g.fillText('LUFS', lx, 12);
    const row = (label: string, v: number, y: number, col: string) => {
      g.font = MONO9; g.fillStyle = T.vizLabel; g.fillText(label, lx, y);
      g.font = MONO10; g.fillStyle = col; g.fillText(fmt(v), lx, y + 12);
    };
    row(t('c.ms.in'), this.m[5], 34, T.fg);
    row(t('c.ms.out'), this.m[6], 66, this.live && Math.abs(this.m[6] - tgt) < 1 ? '#5fbf8a' : T.fgHi);
    g.font = MONO9; g.fillStyle = T.vizLabel; g.fillText('→ ' + tgt, lx, 98);
    if (Math.abs(this.m[7]) > 0.05 && this.live) { g.fillStyle = '#4fb6ff'; g.fillText('= ' + this.m[7].toFixed(1), lx, 112); }
  }
}
