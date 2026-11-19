// The sampler's Slice page (FL Slicex / Fruity Slicer, Ableton Simpler's Slice mode, MPC chop):
// cut the sample at its transients or into equal parts, adjust the markers on the waveform,
// play slices from the keys (the root key is slice 1) and write them into the pattern.
//
// On the waveform: click a slice to hear it, drag a marker to move it, double-click empty space
// to add a marker and a marker to remove it. Detection runs once per file in the engine
// (`source_onsets`); the Sensitivity knob only filters the candidates, so it answers instantly.
import { createEffect, createMemo, createSignal, onCleanup, onMount, Show } from 'solid-js';
import { be } from '../api/backend';
import type { Channel, Id, Op, Source } from '../api/types';
import { clamp } from '../lib/format';
import { t } from '../lib/i18n';
import { noteName } from '../lib/pitch';
import { addSlice, dumpNotes, EQUAL_COUNTS, equalSlices, moveSlice, onsetsFrom, pickSlices, removeSlice, sliceAt, type Onset } from '../lib/slicer';
import { commit, Gesture } from '../state/edit';
import { audition, chanOp, curPat, openRoll, putNotes } from '../state/pattern';
import { getPeaks, peakRange, peaksVersion } from '../state/peaks';
import { S } from '../state/store';
import { say } from '../state/ui';
import { track } from '../ui/drag';
import { addFrameTask, fitCanvas } from '../ui/frameLoop';
import { Knob, Segmented } from '../ui/Knob';
import { rgba, T, theme } from '../ui/theme';

type SampleChannel = Extract<Channel, { kind: 'sample' }>;

/** Candidates per source id (the engine's analysis of one file never changes). */
const onsetCache = new Map<Id, Promise<Onset[]>>();
const onsetsOf = (id: Id) => {
  let p = onsetCache.get(id);
  if (!p) {
    p = be().sourceOnsets(id).then(onsetsFrom);
    p.catch(() => onsetCache.delete(id));
    onsetCache.set(id, p);
  }
  return p;
};

/** How the last detection was made, per channel (the UI's memory, not the project's). */
const [modes, setModes] = createSignal<Record<Id, { mode: 0 | 1; sens: number; equal: number }>>({});

export function SlicePanel(props: { channel: () => SampleChannel; source: () => Source | undefined }) {
  const c = props.channel;
  const slices = () => c().slices ?? [];
  const dur = () => props.source()?.dur ?? 0;
  const st = () => modes()[c().id] ?? { mode: 0 as const, sens: 0.5, equal: 8 };
  const setSt = (patch: Partial<ReturnType<typeof st>>) => setModes(m => ({ ...m, [c().id]: { ...st(), ...patch } }));
  const [onsets, setOnsets] = createSignal<Onset[] | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  const op = (next: number[]): Op => chanOp(c(), { slices: next.length ? next : undefined });
  const sayCount = (n: number) => say(c().name + ' · ' + (n ? t('d.sl.count', { n }) : t('d.sl.off')));

  const load = async () => {
    const src = props.source(); if (!src) return null;
    if (onsets()) return onsets();
    setBusy(true); setErr(null);
    try { const o = await onsetsOf(src.id); setOnsets(o); return o; } catch (e) { setErr(String(e)); return null; } finally { setBusy(false); }
  };
  // a slice page with slices already there: fetch the candidates in the background, so the knob answers at once
  createEffect(() => { if (props.source()) void load(); });

  const detect = async () => {
    const o = await load(); if (!o) return;
    const next = pickSlices(o, st().sens, dur());
    commit([op(next)], t('d.sl.detect')); sayCount(next.length);
  };
  const chop = (n: number) => {
    setSt({ mode: 1, equal: n });
    const next = equalSlices(n);
    commit([op(next)], t('d.sl.equal')); sayCount(next.length);
  };
  let g: Gesture | null = null;
  const sensKnob = (
    <Knob key={'slsens' + c().id} label={t('d.sl.sens')} title={t('d.sl.sensTip')} size="sm"
      t={() => st().sens} text={() => Math.round(st().sens * 100) + ' %'}
      onStart={() => { g = new Gesture(t('d.sl.sens')); }}
      onChange={v => {
        setSt({ mode: 0, sens: v });
        const o = onsets(); if (!o) return;
        const next = pickSlices(o, v, dur());
        g?.update([op(next)]); sayCount(next.length);
      }}
      onEnd={() => { g?.end(); g = null; }}
      onReset={() => { setSt({ sens: 0.5 }); void detect(); }}
      onWheel={v => { setSt({ mode: 0, sens: v }); const o = onsets(); if (o) commit([op(pickSlices(o, v, dur()))], t('d.sl.sens')); }} />
  );

  const dump = () => {
    const pt = curPat(); if (!pt || !slices().length || !dur()) return;
    const mine = dumpNotes(slices(), dur(), S.project.bpm, c().id, c().root);
    const others = pt.notes.filter(n => n.ch !== c().id);
    commit([putNotes(pt, [...others, ...mine])], t('d.sl.dump'));
    say(t('d.sl.dumped', { n: mine.length, pat: pt.name }));
    openRoll(c().id);
  };
  const keyRange = () => slices().length ? noteName(c().root) + '–' + noteName(c().root + slices().length - 1) : '';

  return <div class="slpage">
    <div class="slbar">
      <Segmented small options={[t('d.sl.transients'), t('d.sl.equalMode')]} value={() => st().mode}
        pick={i => { if (i === 0) { setSt({ mode: 0 }); void detect(); } else chop(st().equal); }} title={t('d.sl.modeTip')} />
      <Show when={st().mode === 0} fallback={
        <Segmented small options={EQUAL_COUNTS.map(String)} value={() => EQUAL_COUNTS.indexOf(st().equal as typeof EQUAL_COUNTS[number])}
          pick={i => chop(EQUAL_COUNTS[i])} title={t('d.sl.equalTip')} />
      }>{sensKnob}</Show>
      <div style={{ flex: 1 }} />
      <Show when={slices().length}>
        <span class="slinfo mono" title={t('d.sl.keysTip')}>{t('d.sl.count', { n: slices().length })} · {keyRange()}</span>
        <button class="smallbtn" onClick={() => { commit([op([])], t('d.sl.clear')); sayCount(0); }} title={t('d.sl.clearTip')}>{t('d.sl.clear')}</button>
        <button class="smallbtn primary" onClick={dump} title={t('d.sl.dumpTip')} disabled={!curPat()}>{t('d.sl.dump')}</button>
      </Show>
    </div>
    <SliceWave channel={c} source={props.source} slices={slices} op={op} />
    <div class="slhint">
      <Show when={busy()}>{t('d.sl.analyzing')} · </Show>
      <Show when={err()}>{e => <span class="err">{e()} · </span>}</Show>
      {slices().length ? t('d.sl.hint') : t('d.sl.empty')}
    </div>
  </div>;
}

/** The waveform with the slices: alternate shading, numbered markers, the slice under the pointer lit. */
function SliceWave(props: { channel: () => SampleChannel; source: () => Source | undefined; slices: () => number[]; op: (s: number[]) => Op }) {
  let box!: HTMLDivElement, canvas!: HTMLCanvasElement;
  const [hover, setHover] = createSignal<{ x: number; marker: number | null } | null>(null);
  let dirty = true, epoch = -1;
  createEffect(() => { props.source(); props.slices(); hover(); props.channel().color; peaksVersion(); dirty = true; });
  onMount(() => {
    const ro = new ResizeObserver(() => { dirty = true; });
    ro.observe(box);
    const stop = addFrameTask(() => {
      if (epoch !== theme.epoch) { epoch = theme.epoch; dirty = true; }
      if (dirty) { dirty = false; draw(); }
    });
    onCleanup(() => { ro.disconnect(); stop(); });
  });
  const W = () => box.clientWidth;
  /** Marker under x (within 5 px), never the first (it is fixed at the start). */
  const markerAt = (x: number) => {
    const w = W(); let best: number | null = null, bd = 6;
    props.slices().forEach((s, i) => { if (i === 0) return; const d = Math.abs(s * w - x); if (d < bd) { bd = d; best = i; } });
    return best;
  };
  const draw = () => {
    const w = box.clientWidth, h = box.clientHeight; if (!w || !h) return;
    fitCanvas(canvas, w, h);
    const g = canvas.getContext('2d')!, dpr = window.devicePixelRatio || 1;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.fillStyle = T.viz; g.fillRect(0, 0, w, h);
    const src = props.source(), sl = props.slices(), color = props.channel().color;
    const hv = hover(), hoverSlice = hv && sl.length && hv.marker == null ? sliceAt(sl, hv.x / w) : -1;
    // slice zones: alternate, the hovered one lit
    sl.forEach((s, i) => {
      const a = s * w, b = (sl[i + 1] ?? 1) * w;
      g.fillStyle = i === hoverSlice ? rgba(color, 0.18) : i % 2 ? rgba(T.fg, 0.035) : 'transparent';
      g.fillRect(a, 0, b - a, h);
    });
    const top = 16, mid = top + (h - top) / 2, amp = (h - top) / 2 * 0.92;
    g.fillStyle = T.vizGrid2; g.fillRect(0, Math.floor(mid), w, 1);
    const peaks = src && getPeaks(src.id, false, false);
    if (peaks) {
      const bins = peaks.length / 2;
      g.fillStyle = color; g.beginPath();
      for (let x = 0; x < w; x++) {
        const [mn, mx] = peakRange(peaks, Math.floor(x / w * bins), Math.floor((x + 1) / w * bins));
        const y1 = mid - clamp(mx, 0, 1) * amp, y2 = mid - clamp(mn, -1, 0) * amp;
        g.rect(x, y1, 1, Math.max(0.6, y2 - y1));
      }
      g.fill();
    }
    // markers with their numbers (and the key) on a strip at the top
    g.font = '600 10px "Geist Mono Variable", "Geist Mono", monospace';
    sl.forEach((s, i) => {
      const x = Math.round(s * w) + 0.5, on = hv?.marker === i;
      g.strokeStyle = on ? T.fgHi : rgba(T.fgHi, i ? 0.75 : 0.35); g.lineWidth = on ? 2 : 1;
      g.beginPath(); g.moveTo(x, top - 2); g.lineTo(x, h); g.stroke();
      const next = (sl[i + 1] ?? 1) * w;
      if (next - x > 14) {
        g.fillStyle = i === hoverSlice ? T.fgHi : T.vizLabel;
        g.fillText(String(i + 1), x + 3, 11);
      }
      if (i) { g.fillStyle = on ? T.fgHi : rgba(T.fgHi, 0.85); g.beginPath(); g.moveTo(x - 4, top - 6); g.lineTo(x + 4, top - 6); g.lineTo(x, top - 1); g.closePath(); g.fill(); }
    });
  };
  const pos = (e: MouseEvent) => { const r = box.getBoundingClientRect(); return clamp((e.clientX - r.left) / r.width, 0, 1); };
  const durS = () => props.source()?.dur ?? 0;
  const down = (e: PointerEvent) => {
    if (e.button !== 0 || !props.source()) return;
    const r = box.getBoundingClientRect(), x = e.clientX - r.left, m = markerAt(x);
    if (m != null) {
      e.preventDefault();
      const gst = new Gesture(t('d.sl.move'));
      track(ev => {
        const next = moveSlice(props.slices(), m, clamp((ev.clientX - r.left) / r.width, 0, 1), durS());
        gst.update([props.op(next)]);
      }, () => gst.end(), 'ew-resize');
      return;
    }
    const sl = props.slices();
    if (sl.length) {
      const i = sliceAt(sl, pos(e));
      audition(props.channel().id, props.channel().root + i, 0.9, true);
      say(t('d.sl.playing', { n: i + 1, key: noteName(props.channel().root + i) }));
    } else audition(props.channel().id, props.channel().root, 0.9, true);
  };
  const dbl = (e: MouseEvent) => {
    if (!props.source()) return;
    const r = box.getBoundingClientRect(), m = markerAt(e.clientX - r.left);
    const cur = props.slices().length ? props.slices() : [0];
    if (m != null) { commit([props.op(removeSlice(cur, m))], t('d.sl.remove')); return; }
    const next = addSlice(cur, pos(e), durS());
    if (next) commit([props.op(next)], t('d.sl.add'));
  };
  const move = (e: PointerEvent) => {
    const r = box.getBoundingClientRect(), x = e.clientX - r.left, m = markerAt(x);
    setHover({ x, marker: m });
    box.style.cursor = m != null ? 'ew-resize' : 'pointer';
  };
  const count = createMemo(() => props.slices().length);
  return <div ref={box} class="slwave" onPointerDown={down} onDblClick={dbl} onPointerMove={move} onPointerLeave={() => setHover(null)}
    aria-label={t('d.sl.waveLabel', { n: count() })}>
    <canvas ref={canvas} />
  </div>;
}

