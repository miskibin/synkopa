// The tempo and key dialog's Chords section (an audio clip): the chords on a strip of the clip's bars, a range to
// pick by dragging (or a chord by clicking), the detail, and "Send to piano roll" — block chords in a new pattern.
import { createMemo, For, Show, untrack } from 'solid-js';
import { createSignal, effect, onCleanup, onMount } from '../lib/reactive';
import { be } from '../api/backend';
import { ERR_NO_CHORD_MODEL } from '../api/errors';
import type { ChordAnalysis, ChordSegment, Id } from '../api/types';
import { chordsIn, defaultChordRange } from '../lib/chords';
import { fmtBytes, fmtPos } from '../lib/format';
import { t } from '../lib/i18n';
import { CHORDS, installChordModel, refreshChordModel, sendChordsToRoll, setChordDetail } from '../state/chords';
import { channels, selChan } from '../state/pattern';
import { S } from '../state/store';
import { Segmented } from '../ui/Knob';

/** "Bars 5–8" for whole bars, else the positions: "5.3 – 7.1". */
function spanText(a: number, b: number): string {
  const whole = (x: number) => Math.abs(x - Math.round(x / 4) * 4) < 1e-6;
  if (whole(a) && whole(b)) {
    const from = Math.round(a / 4) + 1, to = Math.round(b / 4);
    return to <= from ? t('d.chords.bar', { a: from }) : t('d.chords.bars', { a: from, b: to });
  }
  return t('d.chords.span', { a: fmtPos(a), b: fmtPos(b) });
}

/** Fewest pixels per beat on the strip: a one-bar chord keeps room for its name (the strip scrolls). */
const MIN_PX_PER_BEAT = 6;

export function ChordsPanel(props: { clipId: Id }) {
  const clip = () => S.project.clips.find(c => c.id === props.clipId);
  const [res, setRes] = createSignal<ChordAnalysis | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  const [needModel, setNeedModel] = createSignal(false);
  const [range, setRange] = createSignal<[number, number] | null>(null);
  const [busy, setBusy] = createSignal(false);
  // a melodic channel the musician has selected, else a new piano
  const melodic = () => channels().filter(c => c.kind !== 'sample');
  const [channel, setChannel] = createSignal<Id | null>(untrack(() => { const c = selChan(); return c && c.kind !== 'sample' ? c.id : null; }));
  refreshChordModel();

  // what the chords depend on: the clip's place, region, speed and pitch, and the tempo
  const key = createMemo(() => {
    const c = clip();
    return c ? [c.id, c.start, c.len, c.offset, c.stretch ?? 1, c.pitch ?? 0, !!c.resample, !!c.rev, !!c.norm, c.ref, S.project.bpm, CHORDS.detail].join('|') : '';
  });
  // bumped when the model has been installed
  const [installed, setInstalled] = createSignal(0);
  let req = 0;
  effect(() => {
    if (!key()) return;
    installed();
    const id = ++req;
    setLoading(true); setErr(null);
    be().chordsAnalyze(props.clipId, CHORDS.detail)
      .then(r => {
        if (id !== req) return;
        setNeedModel(false); setRes(r);
        // keep the musician's range while it still fits the clip
        const cur = range();
        if (!cur || cur[0] < r.start - 1e-6 || cur[1] > r.end + 1e-6) setRange(defaultChordRange(r, S.project.loop));
      })
      .catch(e => {
        if (id !== req) return;
        const msg = String((e as Error)?.message ?? e);
        if (msg.startsWith(ERR_NO_CHORD_MODEL)) { setNeedModel(true); setRes(null); } else setErr(msg);
      })
      .finally(() => { if (id === req) setLoading(false); });
  });

  const picked = () => { const r = res(), g = range(); return r && g ? chordsIn(r.segments, g[0], g[1]) : []; };
  const whole = () => { const r = res(), g = range(); return !!r && !!g && Math.abs(g[0] - r.start) < 1e-6 && Math.abs(g[1] - r.end) < 1e-6; };
  const send = async () => {
    const r = res(), g = range();
    if (!r || !g || !picked().length || busy()) return;
    setBusy(true);
    await sendChordsToRoll({ clipId: r.clipId, from: g[0], to: g[1], detail: CHORDS.detail, channel: channel() });
    setBusy(false);
  };
  const size = () => fmtBytes(CHORDS.model?.bytes ?? 0);

  return (
    <div class="chords" data-chords>
      <div class="row" style={{ 'justify-content': 'space-between', gap: '8px' }}>
        <span class="tag9">{t('d.chords.title')}</span>
        <Show when={res()}>
          <Segmented small options={[t('d.chords.full'), t('d.chords.simple')]} value={() => (CHORDS.detail === 'simple' ? 1 : 0)}
            pick={i => setChordDetail(i === 1 ? 'simple' : 'full')} title={t('d.chords.detailTip')} />
        </Show>
      </div>

      <Show when={needModel()}>
        <div class="chords-box">
          <span>{t('d.chords.modelNeeded', { size: size() })}</span>
          <span class="mono sel-text chords-lic ell" title={CHORDS.model?.licenseNote}>{t('d.stems.license')}: {CHORDS.model?.licenseUrl?.replace(/^https?:\/\//, '')}</span>
          <Show when={CHORDS.download != null} fallback={
            <div class="row" style={{ 'justify-content': 'flex-end', gap: '8px' }}>
              <Show when={CHORDS.error}><span class="chords-err" role="alert">{CHORDS.error}</span></Show>
              <button class="smallbtn primary" data-chords-download onClick={() => void installChordModel().then(ok => { if (ok) setInstalled(n => n + 1); })}>
                {CHORDS.error ? t('d.stems.retry') : t('d.chords.download', { size: size() })}
              </button>
            </div>
          }>
            <div class="stems-bar" role="progressbar" aria-label={t('d.chords.downloading')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((CHORDS.download ?? 0) * 100)}>
              <div style={{ width: (CHORDS.download ?? 0) * 100 + '%' }} />
            </div>
          </Show>
        </div>
      </Show>

      <Show when={loading() && !res() && !needModel()}>
        <div class="row" style={{ gap: '8px', color: 'var(--fg3)', padding: '6px 0' }}><div class="spin" /><span>{t('d.chords.analyzing')}</span></div>
      </Show>
      <Show when={err()}>
        <div class="note err">{err()}</div>
      </Show>

      <Show when={res()}>
        {r => (
          <Show when={r().segments.length} fallback={<span class="note">{t('d.chords.none')}</span>}>
            <ChordStrip res={r()} range={range} setRange={setRange} dim={loading()} />
            <div class="chords-seq" title={picked().map(s => s.name).join(' ')} data-chords-seq>
              <For each={picked()} keyed={(s: ChordSegment) => s.start + ':' + s.name}>{s => <span class={['chords-name', { weak: s().confidence < 0.45 }]}>{s().name}</span>}</For>
            </div>
            <div class="row chords-foot">
              <span class="mono chords-range" data-chords-range>
                {range() ? spanText(range()![0], range()![1]) + ' · ' + (picked().length === 1 ? t('d.chords.count1') : t('d.chords.count', { n: picked().length })) : ''}
              </span>
              <Show when={!whole()}>
                <button class="smallbtn" onClick={() => setRange([r().start, r().end])}>{t('d.chords.wholeClip')}</button>
              </Show>
              <div style={{ flex: 1 }} />
              <select class="sel chords-ch" title={t('d.chords.channelTip')} value={channel() ?? ''} data-chords-channel
                onChange={e => setChannel(e.currentTarget.value === '' ? null : Number(e.currentTarget.value))}>
                <option value="">{t('d.chords.newPiano')}</option>
                <For each={melodic()}>{c => <option value={c.id}>{c.name}</option>}</For>
              </select>
              <button class="smallbtn primary" disabled={!picked().length || busy()} onClick={() => void send()} data-chords-send
                title={t('d.chords.sendTip')}>
                {t('d.chords.send')}
              </button>
            </div>
            <Show when={r().transpose !== 0}>
              <span class="note">{t('d.chords.transposed', { n: (r().transpose > 0 ? '+' : '−') + Math.abs(r().transpose) })}</span>
            </Show>
          </Show>
        )}
      </Show>
    </div>
  );
}

/** The clip's chords on its bars. Drag to pick a range (whole beats), click a chord to pick it, double-click for the whole clip. */
function ChordStrip(p: { res: ChordAnalysis; range: () => [number, number] | null; setRange: (r: [number, number]) => void; dim: boolean }) {
  let inner!: HTMLDivElement;
  const [width, setWidth] = createSignal(0);
  onMount(() => {
    const ro = new ResizeObserver(() => setWidth(inner.clientWidth));
    ro.observe(inner);
    setWidth(inner.clientWidth);
    onCleanup(() => ro.disconnect());
  });
  const len = () => Math.max(1e-6, p.res.end - p.res.start);
  const pct = (b: number) => ((b - p.res.start) / len()) * 100 + '%';
  // bar numbers: about every 48 px
  const bars = createMemo(() => {
    const pxBar = (width() / len()) * 4 || 1;
    const every = [1, 2, 4, 8, 16, 32, 64].find(n => n * pxBar >= 48) ?? 128;
    const out: number[] = [];
    for (let b = Math.ceil(p.res.start / 4) * 4; b < p.res.end; b += 4) if ((b / 4) % every === 0) out.push(b);
    return out;
  });
  const beatAt = (x: number) => {
    const r = inner.getBoundingClientRect();
    const b = p.res.start + ((x - r.left) / Math.max(1, r.width)) * len();
    return Math.min(p.res.end, Math.max(p.res.start, b));
  };
  const snap = (b: number) => Math.min(p.res.end, Math.max(p.res.start, Math.round(b)));
  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const x0 = e.clientX, anchor = beatAt(x0);
    let moved = false;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const move = (m: PointerEvent) => {
      if (!moved && Math.abs(m.clientX - x0) < 4) return;
      moved = true;
      const a = snap(anchor), b = snap(beatAt(m.clientX));
      p.setRange(a === b ? [a, Math.min(p.res.end, a + 1)] : [Math.min(a, b), Math.max(a, b)]);
    };
    const up = () => {
      window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', up);
      if (moved) return;
      const s = p.res.segments.find(x => anchor >= x.start && anchor < x.end);
      if (s) p.setRange([s.start, s.end]);
    };
    window.addEventListener('pointermove', move); window.addEventListener('pointerup', up);
  };
  const tip = (s: ChordSegment) => t('d.chords.segTip', { name: s.name, from: fmtPos(s.start), beats: +(s.end - s.start).toFixed(2), n: Math.round(s.confidence * 100) });

  return (
    <div class={["chords-strip", { dim: p.dim }]}>
      <div class="chords-inner" ref={inner} style={{ 'min-width': len() * MIN_PX_PER_BEAT + 'px' }}
        onPointerDown={down} onDblClick={() => p.setRange([p.res.start, p.res.end])} data-chords-strip>
        <div class="chords-ruler">
          <For each={bars()}>{b => <span style={{ left: pct(b) }}>{Math.floor(b / 4) + 1}</span>}</For>
        </div>
        <div class="chords-lane">
          <For each={p.res.segments}>
            {s => (
              <div class={['chords-seg', s.quality === 'min' || s.quality === 'm7' || s.quality === 'dim' ? 'minor' : 'major']}
                style={{ left: pct(s.start), width: `calc(${pct(s.end)} - ${pct(s.start)})`, opacity: 0.45 + 0.55 * s.confidence }} title={tip(s)}>
                <span>{s.name}</span>
              </div>
            )}
          </For>
        </div>
        <Show when={p.range()}>
          {g => (
            <>
              <div class="chords-shade" style={{ left: 0, width: pct(g()[0]) }} />
              <div class="chords-shade" style={{ left: pct(g()[1]), right: 0 }} />
              <div class="chords-sel" style={{ left: pct(g()[0]), width: `calc(${pct(g()[1])} - ${pct(g()[0])})` }} />
            </>
          )}
        </Show>
      </div>
    </div>
  );
}
