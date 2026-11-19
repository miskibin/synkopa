// Reverb display: how long the tail rings at each frequency, and the tail in time.
//
// Top (FabFilter Pro-R's idea, the most useful picture of a reverb): frequency across, decay time
// up. The curve is the RT60 the network gives each frequency (Bass decay below ~250 Hz, Decay in
// the middle, Tone setting how soon the highs die), the same numbers the DSP uses
// (lib/reverbSpace `derive`, reverb_v2.rs). What the wet input filters remove (Low cut, High cut)
// is shaded. Behind it, the live spectrum of the tail itself (the analyser's `specOut` is the wet
// signal for the reverb). Drag the three dots up and down (bass, decay, treble) and the cut lines
// sideways; double-click a handle resets it.
//
// Bottom: the tail in time: pre-delay (drag), the early reflections, the decay down to −60 dB and,
// after a hit, the measured output traced over it.
import type { Fx } from '../../api/types';
import type { FxFrame } from '../../api/telemetry';
import { fx2x, x2f } from '../../lib/eq';
import { clamp } from '../../lib/format';
import { t as tr } from '../../lib/i18n';
import { derive, envelope, timeSpan } from '../../lib/reverbSpace';
import { Layer } from '../../ui/frameLoop';
import { BAND_F, MONO10, MONO11, rgba, SANS10, SpecSmoother, T, type Viz } from './common';

export const VERB_COLOR = '#47b8a8';
const LIVE = '#4fb8cc';
const DB_MIN = -66;
/** Where the handles sit on the frequency axis. */
const F_BASS = 80, F_MID = 1000, F_TREBLE = 8000;

export type VerbHandle = 'pre' | 'size' | 'bass' | 'tone' | 'lo' | 'hi';

type Params = Record<string, number | undefined>;

/** RT60 (s) at frequency `f`: mid decay, bass below ~250 Hz, treble above the tone crossover (smooth, in decay rate). */
export function rt60At(p: Params, f: number): number {
  const d = derive(p);
  const xh = 3000 * Math.pow(2, clamp((p.tone ?? 0) / 100, -1, 1));
  const wl = 1 / (1 + Math.pow(f / 250, 4)), wh = Math.pow(f / xh, 4) / (1 + Math.pow(f / xh, 4));
  const r = 60 / d.tMid + (60 / d.tLow - 60 / d.tMid) * wl + (60 / d.tHigh - 60 / d.tMid) * wh;
  return 60 / r;
}

/** A round top for the decay axis. */
const decayTop = (p: Params) => {
  const d = derive(p), need = Math.max(d.tMid, d.tLow) * 1.35;
  return [0.5, 1, 1.5, 2, 3, 4, 6, 8, 10, 15, 20, 30].find(s => s >= need) ?? need;
};

/** Where things are on a canvas of W × H (shared with the window's hit testing and dragging). */
export function verbLayout(W: number, H: number, fx: Fx) {
  const p = fx.params;
  const x0 = 40, x1 = W - 12, fy0 = 24, fy1 = Math.round(H * 0.64), ty0 = fy1 + 32, ty1 = H - 18;
  const top = decayTop(p), span = timeSpan(p), e = envelope(p);
  const fx_ = (f: number) => x0 + fx2x(f, x1 - x0);
  const xf = (x: number) => x2f(clamp(x - x0, 0, x1 - x0), x1 - x0);
  const ys = (s: number) => fy1 - clamp(s / top, 0, 1) * (fy1 - fy0);
  const sy = (y: number) => (fy1 - y) / (fy1 - fy0) * top;
  const tx = (t: number) => x0 + clamp(t / span, 0, 1) * (x1 - x0);
  const xt = (x: number) => (x - x0) / (x1 - x0) * span;
  const tdb = (db: number) => ty0 + clamp(-db / -DB_MIN, 0, 1) * (ty1 - ty0);
  const d = derive(p);
  const handles: Record<Exclude<VerbHandle, 'pre' | 'lo' | 'hi'>, [number, number]> = {
    bass: [fx_(Math.max(F_BASS, (p.lo ?? 300) * 1.25)), ys(d.tLow)], size: [fx_(F_MID), ys(d.tMid)],
    // the treble dot stays inside what is heard (below the high cut)
    tone: [fx_(Math.min(F_TREBLE, (p.hi ?? 7000) / 1.25)), ys(d.tHigh)],
  };
  return { x0, x1, fy0, fy1, ty0, ty1, top, span, e, d, fx: fx_, xf, ys, sy, tx, xt, tdb, handles,
    loX: fx_(p.lo ?? 300), hiX: fx_(p.hi ?? 7000), preX: tx(e.pre) };
}

/** The handle under (x, y), if any. */
export function verbHit(fx: Fx, x: number, y: number, W: number, H: number): VerbHandle | null {
  const L = verbLayout(W, H, fx);
  for (const k of ['size', 'bass', 'tone'] as const) { const [hx, hy] = L.handles[k]; if (Math.hypot(x - hx, y - hy) < 10) return k; }
  if (y >= L.fy0 - 8 && y <= L.fy1) {
    if (Math.abs(x - L.loX) < 6) return 'lo';
    if (Math.abs(x - L.hiX) < 6) return 'hi';
  }
  if (y >= L.ty0 - 10 && y <= L.ty1 && Math.abs(x - L.preX) < 6) return 'pre';
  return null;
}

/** The parameter a handle drags and its new value at (x, y), mapped through layout `L` (taken at the drag's start). */
export function verbDrag(h: VerbHandle, L: ReturnType<typeof verbLayout>, x: number, y: number, p: Params): { id: string; value: number } {
  const tMid = L.d.tMid;
  switch (h) {
    case 'pre': return { id: 'pre', value: Math.round(clamp(L.xt(x) * 1000, 0, 150)) };
    case 'lo': return { id: 'lo', value: Math.round(clamp(L.xf(x), 20, Math.min(1000, (p.hi ?? 7000) / 2))) };
    case 'hi': return { id: 'hi', value: Math.round(clamp(L.xf(x), Math.max(2000, (p.lo ?? 300) * 2), 16000) / 10) * 10 };
    case 'size': return { id: 'size', value: Math.round(clamp(L.sy(y), 0.2, 20) * 100) / 100 };
    case 'bass': return { id: 'bass', value: Math.round(clamp(L.sy(y) / tMid, 0.5, 1.5) * 100) / 100 };
    case 'tone': {
      // treble decay / mid decay: 0.25 (dark) … 0.5 … 0.85 (bright), see reverb_v2.rs `derive`
      const hf = clamp(L.sy(y) / tMid, 0.25, 0.85);
      return { id: 'tone', value: Math.round((hf < 0.5 ? (hf - 0.5) / 0.25 : (hf - 0.5) / 0.35) * 100) };
    }
  }
}

export class ReverbViz implements Viz {
  cursor = 'default';
  hover: VerbHandle | null = null;
  /** The handle being dragged (drawn highlighted). */
  drag: VerbHandle | null = null;
  private stat = new Layer();
  private spec = new SpecSmoother(128, 24, 0.8, 18);
  private live = false;
  /** Output level (dB) per analysis frame since the last hit: [seconds after it, dB]. */
  private trace: [number, number][] = [];
  private hitAt = -1;
  private floorDb = -120;
  private lastT = 0;

  update(_fx: Fx, a: FxFrame | null, now: number): boolean {
    let changed = false;
    if (a) { this.spec.feed(a.specOut); this.live = true; } else if (this.live) { this.spec.silence(); this.live = false; }
    if (this.spec.step(now)) changed = true;
    const t = now / 1000;
    const db = a && a.outPeak > 1e-6 ? 20 * Math.log10(a.outPeak) : -120;
    // a hit: the level jumps well above where it was (and is audible); the trace restarts there
    if (db > -50 && db > this.floorDb + 15 && (this.hitAt < 0 || t - this.hitAt > 0.15)) { this.hitAt = t; this.trace = []; }
    this.floorDb = Math.min(db, this.floorDb + (t - this.lastT) * 40);
    this.lastT = t;
    if (this.hitAt < 0) return changed;
    const since = t - this.hitAt;
    if (since > 10) { this.hitAt = -1; this.trace = []; return true; }
    if (!this.trace.length || since - this.trace[this.trace.length - 1][0] > 0.012) this.trace.push([since, db]);
    return true;
  }

  draw(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx) {
    const p = fx.params, key = [W, H, fx.bypass, this.hover, this.drag, ...['size', 'pre', 'space', 'tone', 'bass', 'early', 'lo', 'hi'].map(k => p[k])].join('|');
    const L = verbLayout(W, H, fx);
    g.fillStyle = T.viz; g.fillRect(0, 0, W, H);
    // the tail's spectrum, live, under the decay curve (its own dB scale, filling the plot)
    g.save(); g.beginPath(); g.rect(L.x0, L.fy0, L.x1 - L.x0, L.fy1 - L.fy0); g.clip();
    g.beginPath(); g.moveTo(L.x0, L.fy1);
    for (let k = 0; k < 128; k++) {
      const v = this.spec.cur[k] + 4.5 * Math.log2(BAND_F[k] / 1000);
      g.lineTo(L.fx(BAND_F[k]), L.fy1 - clamp((v + 80) / 80, 0, 1) * (L.fy1 - L.fy0) * 0.85);
    }
    g.lineTo(L.x1, L.fy1); g.closePath();
    g.fillStyle = rgba(LIVE, 0.12); g.fill();
    g.restore();
    g.drawImage(this.stat.get(W, H, key, c => drawStatic(c, W, H, fx, this.hover ?? this.drag)) as CanvasImageSource, 0, 0, W, H);
    // the measured output after the last hit, level-matched to the tail's start
    if (this.trace.length > 1) {
      const peak = Math.max(...this.trace.map(q => q[1])), off = L.e.lateDb - peak;
      g.save(); g.beginPath(); g.rect(L.x0, L.ty0, L.x1 - L.x0, L.ty1 - L.ty0); g.clip();
      g.beginPath();
      this.trace.forEach(([s, db], i) => { const x = L.tx(L.e.pre + s), y = L.tdb(db + off); if (i) g.lineTo(x, y); else g.moveTo(x, y); });
      g.strokeStyle = rgba(LIVE, 0.9); g.lineWidth = 1.4; g.stroke();
      g.restore();
      g.font = MONO10; g.fillStyle = LIVE; g.textAlign = 'right'; g.fillText(tr('c.verb.lastHit'), L.x1, L.ty0 - 4); g.textAlign = 'left';
    }
  }
}

function drawStatic(g: CanvasRenderingContext2D, W: number, H: number, fx: Fx, hot: VerbHandle | null) {
  const p = fx.params, L = verbLayout(W, H, fx), d = L.d, e = L.e;
  g.globalAlpha = fx.bypass ? 0.45 : 1;
  // ---- decay per frequency
  g.lineWidth = 1; g.font = MONO10;
  const step = L.top > 10 ? 5 : L.top > 4 ? 1 : L.top > 2 ? 0.5 : L.top > 1 ? 0.25 : 0.1;
  g.textAlign = 'right';
  for (let s = 0; s <= L.top + 1e-6; s += step) {
    const y = Math.round(L.ys(s)) + 0.5;
    g.strokeStyle = s === 0 ? T.vizGrid2 : T.vizGrid; g.beginPath(); g.moveTo(L.x0, y); g.lineTo(L.x1, y); g.stroke();
    if (s > 0) { g.fillStyle = T.vizLabel; g.fillText(+s.toFixed(2) + ' s', L.x0 - 4, y + 3); }
  }
  g.textAlign = 'center';
  for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) {
    const x = Math.round(L.fx(f)) + 0.5;
    g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(x, L.fy0); g.lineTo(x, L.fy1); g.stroke();
    g.fillStyle = T.vizLabel; g.fillText(f >= 1000 ? f / 1000 + 'k' : String(f), x, L.fy1 + 12);
  }
  // what the wet filters take away
  g.fillStyle = rgba(T.viz, 0.65);
  g.fillRect(L.x0, L.fy0, L.loX - L.x0, L.fy1 - L.fy0); g.fillRect(L.hiX, L.fy0, L.x1 - L.hiX, L.fy1 - L.fy0);
  // the curve, filled, brighter where it is heard
  const pts: [number, number][] = [];
  for (let i = 0; i <= 200; i++) { const f = 20 * Math.pow(1000, i / 200); pts.push([L.fx(f), L.ys(rt60At(p, f))]); }
  g.save(); g.beginPath(); g.rect(L.loX, L.fy0 - 4, L.hiX - L.loX, L.fy1 - L.fy0 + 4); g.clip();
  g.beginPath(); g.moveTo(pts[0][0], L.fy1); pts.forEach(q => g.lineTo(q[0], q[1])); g.lineTo(pts[pts.length - 1][0], L.fy1); g.closePath();
  const grad = g.createLinearGradient(0, L.fy0, 0, L.fy1);
  grad.addColorStop(0, rgba(VERB_COLOR, 0.38)); grad.addColorStop(1, rgba(VERB_COLOR, 0.04));
  g.fillStyle = grad; g.fill();
  g.restore();
  g.beginPath(); pts.forEach((q, i) => (i ? g.lineTo(q[0], q[1]) : g.moveTo(q[0], q[1])));
  g.strokeStyle = rgba(VERB_COLOR, 0.35); g.lineWidth = 1.5; g.stroke();
  g.save(); g.beginPath(); g.rect(L.loX, 0, L.hiX - L.loX, H); g.clip();
  g.beginPath(); pts.forEach((q, i) => (i ? g.lineTo(q[0], q[1]) : g.moveTo(q[0], q[1])));
  g.strokeStyle = VERB_COLOR; g.lineWidth = 2.2; g.stroke();
  g.restore();
  // cut lines
  for (const [h, x, f] of [['lo', L.loX, p.lo ?? 300], ['hi', L.hiX, p.hi ?? 7000]] as const) {
    const on = hot === h;
    g.strokeStyle = on ? T.fgHi : rgba(T.fg, 0.45); g.lineWidth = on ? 2 : 1;
    g.beginPath(); g.moveTo(Math.round(x) + 0.5, L.fy0 - 6); g.lineTo(Math.round(x) + 0.5, L.fy1); g.stroke();
    g.font = MONO10; g.fillStyle = on ? T.fgHi : T.vizLabel; g.textAlign = h === 'lo' ? 'left' : 'right';
    g.fillText((h === 'lo' ? 'LO ' : 'HI ') + (f >= 1000 ? (f / 1000).toFixed(1) + 'k' : Math.round(f)), x + (h === 'lo' ? 4 : -4), L.fy0 - 8);
  }
  // handles with their values
  const lab: Record<string, string> = {
    bass: tr('c.verb.bass', { s: d.tLow.toFixed(2) }), size: d.tMid.toFixed(2) + ' s', tone: tr('c.verb.treble', { s: d.tHigh.toFixed(2) }),
  };
  for (const k of ['bass', 'tone', 'size'] as const) {
    const [hx, hy] = L.handles[k], on = hot === k, big = k === 'size';
    g.beginPath(); g.arc(hx, hy, on ? 7 : big ? 6 : 5, 0, Math.PI * 2);
    g.fillStyle = on ? T.fgHi : big ? VERB_COLOR : T.viz; g.fill();
    g.strokeStyle = on ? T.fgHi : VERB_COLOR; g.lineWidth = 2; g.stroke();
    g.font = big ? '600 13px "Geist Mono Variable", "Geist Mono", monospace' : MONO10;
    g.fillStyle = on || big ? T.fgHi : T.fg3; g.textAlign = 'center';
    g.fillText(lab[k], hx, hy - (big ? 12 : 10));
  }
  // space and pre-delay summary, top left
  g.font = SANS10; g.textAlign = 'left'; g.fillStyle = T.fg3;
  g.fillText(e.sp.name.toUpperCase() + '  ·  ' + tr('c.verb.decayAxis'), L.x0, 12);
  // ---- the tail in time
  const { ty0, ty1, tx, tdb } = L;
  g.fillStyle = T.vizPanel; g.fillRect(L.x0, ty0, L.x1 - L.x0, ty1 - ty0);
  g.font = MONO10; g.textAlign = 'center';
  const ts = L.span > 20 ? 5 : L.span > 8 ? 2 : L.span > 4 ? 1 : L.span > 1.6 ? 0.5 : L.span > 0.8 ? 0.25 : 0.1;
  for (let t = 0; t <= L.span + 1e-6; t += ts) {
    const x = Math.round(tx(t)) + 0.5;
    g.strokeStyle = T.vizGrid; g.beginPath(); g.moveTo(x, ty0); g.lineTo(x, ty1); g.stroke();
    g.fillStyle = T.vizLabel; g.fillText((Math.round(t * 100) / 100) + ' s', x, ty1 + 12);
  }
  if (e.pre > 0) { g.fillStyle = rgba(T.amber, 0.1); g.fillRect(L.x0, ty0, L.preX - L.x0, ty1 - ty0); }
  g.strokeStyle = rgba(T.ink, 0.45); g.lineWidth = 1;
  for (const r of e.early) { const x = Math.round(tx(r.t)) + 0.5; g.beginPath(); g.moveTo(x, ty1); g.lineTo(x, tdb(r.db)); g.stroke(); }
  g.beginPath(); g.moveTo(L.preX, ty1); g.lineTo(tx(e.onset), tdb(e.lateDb));
  g.lineTo(tx(e.onset + e.tMid * (e.lateDb - DB_MIN) / 60), ty1); g.closePath();
  g.fillStyle = rgba(VERB_COLOR, 0.16); g.fill(); g.strokeStyle = rgba(VERB_COLOR, 0.8); g.lineWidth = 1.4; g.stroke();
  const preHot = hot === 'pre';
  g.strokeStyle = preHot ? T.fgHi : rgba(T.amber, 0.9); g.lineWidth = preHot ? 2 : 1.2;
  g.beginPath(); g.moveTo(Math.round(L.preX) + 0.5, ty0 - 6); g.lineTo(Math.round(L.preX) + 0.5, ty1); g.stroke();
  g.font = MONO11; g.textAlign = 'left'; g.fillStyle = preHot ? T.fgHi : T.amber;
  g.fillText('PRE ' + Math.round(e.pre * 1000) + ' ms', Math.min(L.preX + 4, L.x1 - 80), ty0 - 4);
  g.globalAlpha = 1;
}
