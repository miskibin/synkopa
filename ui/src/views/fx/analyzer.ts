// Analyzer window: the spectrum (SPAN-style tilt, peak hold, a cursor that names the note), the
// loudness of the last 30 s against a target, the numbers a master needs (short-term, integrated,
// range, true peak) and the stereo field (vectorscope, correlation).
//
// Meters (dsp/loudness.rs): [0] momentary LUFS, [1] short-term, [2] integrated (−200 = none
// yet), [3] loudness range LU (−1 = none yet), [4] true peak max dBTP, [5] true peak now,
// [6] correlation, [7] balance, [8] seconds measured, [9] side/mid dB. The spectrum is the
// output's mid (specOut, bass from the long FFT), X/Y points the stereo output.
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { fx2x, x2f } from '../../lib/eq';
import { clamp } from '../../lib/format';
import { t as tr } from '../../lib/i18n';
import { hzToMidi, noteName } from '../../lib/pitch';
import { BAND_F, freqGrid, MONO10, MONO11, rgba, SANS10, SpecSmoother, T, TeleGate, TimeRing, type Viz } from './common';
import { corrBar, Vectorscope } from './stereo';

export const ANALYZER_COLOR = '#9fb4d0';
/** catalog.rs ANALYZER `target`: LUFS per option (null = off). */
export const TARGETS: (number | null)[] = [null, -14, -9, -23];
const SLOPES = [0, 3, 4.5];
/** Loudness history span, seconds. */
const HIST_S = 30;
const NONE = -150;

export interface AnalyzerLayout { specX: number; specY: number; specW: number; specH: number; histY: number; histH: number; colX: number; colW: number }

export function analyzerLayout(W: number, H: number): AnalyzerLayout {
  const pad = 12, colW = clamp(W * 0.27, 200, 250), colX = W - colW - pad;
  const specW = colX - 2 * pad, specH = Math.round((H - 2 * pad) * 0.66);
  const histY = pad + specH + 10;
  return { specX: pad, specY: pad, specW, specH, histY, histH: H - histY - pad, colX, colW };
}

const fmtLu = (v: number) => (v <= -69.5 ? '—' : v.toFixed(1));

export class AnalyzerViz implements Viz {
  cursor = 'crosshair';
  private gate = new TeleGate();
  private spec = new SpecSmoother(128, 30, 1.5, 12);
  private scope = new Vectorscope();
  private ring = new TimeRing(HIST_S * 70, 2);
  private m = new Float32Array(10).fill(NONE);
  private live = false;
  private pending: FxFrame | null = null;
  /** cursor over the spectrum (canvas px), or null */
  hover: { x: number; y: number } | null = null;

  ingest(a: FxFrame, at: number) {
    // only while something plays: the history then stops instead of drawing a flat floor
    if (a.meters[0] > -100) this.ring.push(at, [a.meters[0], a.meters[1]], a.seq);
  }

  update(_fx: Fx, a: FxFrame | null, now: number): boolean {
    let changed = false;
    if (this.gate.fresh(a, 90)) {
      this.live = true;
      this.m.set(a!.meters.subarray(0, 10));
      this.spec.feed(a!.specOut);
      this.pending = a;
      changed = true;
    } else if (!a && this.live) {
      this.live = false; this.spec.silence(); changed = true;
    }
    if (this.spec.step(now)) changed = true;
    // the history scrolls while sound comes in
    return changed || (this.live && this.ring.n > 0);
  }

  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx, _a: FxFrame | null, extra: { now: number }) {
    const L = analyzerLayout(W, H);
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    if (fx.bypass) g.globalAlpha = 0.5;
    this.drawSpectrum(g, L, fx);
    this.drawHistory(g, L, fx, extra.now);
    this.drawNumbers(g, L, fx, H);
    g.globalAlpha = 1;
  }

  private drawSpectrum(g: CanvasRenderingContext2D, L: AnalyzerLayout, fx: Fx) {
    const { specX: x0, specY: y0, specW: w, specH: h } = L;
    const slope = SLOPES[Math.round(fx.params.slope ?? 2)] ?? 4.5;
    const lo = -90, hi = 0;
    const y = (db: number) => y0 + (1 - clamp((db - lo) / (hi - lo), 0, 1)) * h;
    g.save();
    g.beginPath(); g.rect(x0, y0, w, h); g.clip();
    g.fillStyle = T.vizPanel; g.fillRect(x0, y0, w, h);
    g.translate(x0, y0);
    freqGrid(g, w, h);
    g.translate(-x0, -y0);
    g.font = MONO10; g.lineWidth = 1;
    for (let d = -80; d < 0; d += 20) {
      const yy = Math.round(y(d)) + 0.5;
      g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(x0, yy); g.lineTo(x0 + w, yy); g.stroke();
      g.fillStyle = T.vizLabel; g.fillText(String(d), x0 + 4, yy - 3);
    }
    const tilt = (k: number) => slope * Math.log2(BAND_F[k] / 1000);
    const path = (v: Float32Array) => {
      g.beginPath();
      for (let k = 0; k < v.length; k++) {
        const xx = x0 + fx2x(BAND_F[k], w), yy = y(v[k] + tilt(k));
        k ? g.lineTo(xx, yy) : g.moveTo(xx, yy);
      }
    };
    // fill under the live curve, peak hold as a thin line above it
    path(this.spec.cur);
    g.lineTo(x0 + w, y0 + h); g.lineTo(x0, y0 + h); g.closePath();
    const grad = g.createLinearGradient(0, y0, 0, y0 + h);
    grad.addColorStop(0, rgba(ANALYZER_COLOR, 0.55)); grad.addColorStop(1, rgba(ANALYZER_COLOR, 0.06));
    g.fillStyle = grad; g.fill();
    path(this.spec.cur); g.strokeStyle = T.ink; g.lineWidth = 1.4; g.stroke();
    path(this.spec.peak); g.strokeStyle = rgba(T.amber, 0.7); g.lineWidth = 1; g.stroke();
    g.font = SANS10; g.fillStyle = T.vizLabel; g.textAlign = 'right';
    g.fillText(tr('c.an.slope', { v: String(slope) }), x0 + w - 6, y0 + 12); g.textAlign = 'left';
    // cursor: frequency, the nearest note and its offset, the level there
    const hv = this.hover;
    if (hv && hv.x >= x0 && hv.x <= x0 + w && hv.y >= y0 && hv.y <= y0 + h) {
      const f = x2f(hv.x - x0, w), m = hzToMidi(f), n = Math.round(m), cents = Math.round((m - n) * 100);
      const k = clamp(Math.round(Math.log(f / 20) / Math.log(1000) * 127), 0, 127);
      const lvl = this.spec.cur[k];
      g.strokeStyle = rgba(T.fg, 0.35); g.setLineDash([3, 3]);
      g.beginPath(); g.moveTo(Math.round(hv.x) + 0.5, y0); g.lineTo(Math.round(hv.x) + 0.5, y0 + h); g.stroke(); g.setLineDash([]);
      const txt = `${f >= 1000 ? (f / 1000).toFixed(2) + ' kHz' : Math.round(f) + ' Hz'}  ${noteName(n)} ${cents >= 0 ? '+' : ''}${cents}c  ${lvl > -120 ? lvl.toFixed(0) + ' dB' : ''}`;
      g.font = MONO11;
      const tw = g.measureText(txt).width + 12, bx = clamp(hv.x + 10, x0, x0 + w - tw);
      g.fillStyle = rgba(T.viz, 0.92); g.fillRect(bx, y0 + 6, tw, 20);
      g.fillStyle = T.fgHi; g.fillText(txt, bx + 6, y0 + 20);
    }
    g.restore();
  }

  private drawHistory(g: CanvasRenderingContext2D, L: AnalyzerLayout, fx: Fx, now: number) {
    const { specX: x0, histY: y0, specW: w, histH: h } = L;
    const lo = -42, hi = 0;
    const y = (lu: number) => y0 + (1 - clamp((lu - lo) / (hi - lo), 0, 1)) * h;
    g.save();
    g.beginPath(); g.rect(x0, y0, w, h); g.clip();
    g.fillStyle = T.vizPanel; g.fillRect(x0, y0, w, h);
    g.font = MONO10; g.lineWidth = 1;
    for (const d of [-36, -24, -12]) {
      const yy = Math.round(y(d)) + 0.5;
      g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(x0, yy); g.lineTo(x0 + w, yy); g.stroke();
      g.fillStyle = T.vizLabel; g.fillText(String(d), x0 + 4, yy - 3);
    }
    const target = TARGETS[Math.round(fx.params.target ?? 1)] ?? null;
    if (target != null) {
      const ty = Math.round(y(target)) + 0.5;
      g.strokeStyle = rgba(T.green, 0.8); g.setLineDash([4, 4]);
      g.beginPath(); g.moveTo(x0, ty); g.lineTo(x0 + w, ty); g.stroke(); g.setLineDash([]);
      g.fillStyle = T.green; g.textAlign = 'right'; g.fillText(target + ' LUFS', x0 + w - 4, ty - 3); g.textAlign = 'left';
    }
    const r = this.ring;
    if (r.n > 1) {
      const tNow = r.clock(now), span = HIST_S * 1000;
      const line = (c: number, col: string, width: number, fill: boolean) => {
        g.beginPath();
        let started = false, firstX = 0, lastX = 0;
        for (let i = 0; i < r.n; i++) {
          const j = r.at(i), tt = r.t[j];
          if (tt < tNow - span || tt > tNow) continue;
          const v = r.ch[c][j];
          const xx = x0 + w - (tNow - tt) / span * w, yy = y(v > -100 ? v : lo - 10);
          if (!started) { g.moveTo(xx, yy); started = true; firstX = xx; } else g.lineTo(xx, yy);
          lastX = xx;
        }
        if (!started) return;
        if (fill) {
          g.lineTo(lastX, y0 + h); g.lineTo(firstX, y0 + h); g.closePath();
          g.fillStyle = rgba(ANALYZER_COLOR, 0.18); g.fill();
        } else { g.strokeStyle = col; g.lineWidth = width; g.stroke(); }
      };
      line(0, rgba(T.fg, 0.35), 1, false);
      line(1, ANALYZER_COLOR, 1, true);
      line(1, T.ink, 1.8, false);
    }
    g.font = SANS10; g.fillStyle = T.vizLabel;
    g.fillText(tr('c.an.history'), x0 + 30, y0 + 12);
    g.restore();
  }

  private drawNumbers(g: CanvasRenderingContext2D, L: AnalyzerLayout, fx: Fx, H: number) {
    const m = this.m, x = L.colX, w = L.colW;
    let y = L.specY + 10;
    g.font = SANS10; g.fillStyle = T.vizLabel; g.fillText(tr('c.an.short'), x, y);
    g.font = '600 32px "Geist Mono Variable", "Geist Mono", monospace';
    const st = m[1];
    g.fillStyle = st > -69.5 ? T.fgHi : T.vizLabel;
    const big = fmtLu(st);
    g.fillText(big, x, y + 34);
    const bw = g.measureText(big).width;
    g.font = MONO11; g.fillStyle = T.vizLabel;
    g.fillText('LUFS', x + bw + 8, y + 34);
    y += 52;
    const row = (label: string, value: string, col: string = T.fg, sub?: string) => {
      g.font = SANS10; g.fillStyle = T.vizLabel; g.fillText(label, x, y);
      if (sub) { const lw = g.measureText(label).width; g.font = MONO10; g.fillText(sub, x + lw + 6, y); }
      g.font = MONO11; g.fillStyle = col; g.textAlign = 'right'; g.fillText(value, x + w, y);
      g.textAlign = 'left';
      y += 19;
    };
    const secs = m[8];
    const since = secs > 0 ? Math.floor(secs / 60) + ':' + String(Math.floor(secs % 60)).padStart(2, '0') : '';
    row(tr('c.an.integrated'), m[2] > -69.5 ? m[2].toFixed(1) + ' LUFS' : '—', T.fgHi, since);
    row(tr('c.an.momentary'), m[0] > -69.5 ? m[0].toFixed(1) + ' LUFS' : '—');
    row(tr('c.an.range'), m[3] >= 0 ? m[3].toFixed(1) + ' LU' : '—');
    const tp = m[4];
    row(tr('c.an.truePeak'), tp > -99 ? (tp > 0 ? '+' : '') + tp.toFixed(1) + ' dBTP' : '—', tp > -1 ? T.red : T.fg);
    // what a streaming service would do with it, without telling anyone to hit a number
    const target = TARGETS[Math.round(fx.params.target ?? 1)] ?? null;
    if (target != null && m[2] > -69.5) {
      const d = m[2] - target;
      g.font = SANS10; g.fillStyle = T.fg3;
      const msg = Math.abs(d) < 0.5 ? tr('c.an.atTarget') : d > 0 ? tr('c.an.turnedDown', { v: d.toFixed(1) }) : tr('c.an.below', { v: (-d).toFixed(1) });
      g.fillText(msg, x, y + 2);
    }
    y += 16;
    // stereo field
    const sh = Math.max(60, H - y - 46);
    if (this.pending) { this.scope.feed(this.pending.xy, this.pending.xyN, w, sh, ANALYZER_COLOR); this.pending = null; }
    this.scope.draw(g, x, y, w, sh);
    corrBar(g, x, y + sh + 22, w, this.live ? m[6] : 1, tr('c.st.corr'), !this.live);
  }
}
