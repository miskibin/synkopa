// "Convert to MIDI…" (Playlist audio clip menu): the clip's melody as Piano roll notes. The preview draws the
// sung pitch and the notes Convert will write, and follows every setting at once; Convert puts them in a new
// pattern under the clip (on a new piano or the picked channel) and opens it in the Piano roll — one undo step.
import { For, Show } from 'solid-js';
import { effect, onCleanup, onMount } from '../lib/reactive';
import { t } from '../lib/i18n';
import { parseKey } from '../lib/rollEdit';
import { noteName } from '../lib/pitch';
import { fmtSpan } from '../state/stems';
import * as A from '../state/actions';
import { AMIDI, closeAudioMidi, convertAudioMidi, setAudioMidiOpts } from '../state/audioMidi';
import { channels } from '../state/pattern';
import { S } from '../state/store';
import { NumField } from '../ui/Knob';
import { DialogBoundary } from '../ui/PanelBoundary';

/** Timing choices, in steps (0 = as sung). */
const GRIDS: [number, string][] = [[0, 'd.a2m.asSung'], [0.5, '1/32'], [1, '1/16'], [2, '1/8'], [4, '1/4']];
const clampKey = (v: number) => Math.max(0, Math.min(127, Math.round(v)));

export function AudioMidiDialog() {
  const clip = () => (AMIDI.win != null ? S.project.clips.find(c => c.id === AMIDI.win) : undefined);
  const src = () => S.project.sources.find(s => s.id === clip()?.ref);
  const pv = () => AMIDI.preview;
  const melodic = () => channels().filter(c => c.kind !== 'sample');
  const seconds = () => {
    const c = clip(); if (!c) return 0;
    return c.len * 60 / S.project.bpm / (c.stretch ?? 1);
  };
  const go = () => { if (pv()?.count && !AMIDI.busy) void convertAudioMidi(); };
  const rangeText = () => {
    const p = pv(); if (!p || p.low == null || p.high == null) return '';
    return p.low === p.high ? noteName(p.low) : `${noteName(p.low)}–${noteName(p.high)}`;
  };
  const take = () => pv()?.takeRange ?? null;
  const keyField = (which: 'low' | 'high') => (
    <span class="a2m-key">
      <NumField label={t(which === 'low' ? 'd.a2m.low' : 'd.a2m.high')} width={64} title={t('d.a2m.rangeTip')}
        value={() => AMIDI.opts[which] ?? take()?.[which === 'low' ? 0 : 1] ?? 60}
        text={v => (AMIDI.opts[which] == null ? t('d.a2m.any') : noteName(v))}
        parse={s => parseKey(s)} drag={(v, d) => clampKey(v + d / 6)}
        apply={(v, mode) => setAudioMidiOpts({ [which]: clampKey(v) }, mode === 'end' || mode === 'set')} />
      <Show when={AMIDI.opts[which] != null}>
        <button class="a2m-clear" title={t('d.a2m.anyTip')} aria-label={t('d.a2m.anyTip')} onClick={() => setAudioMidiOpts({ [which]: null }, true)}>×</button>
      </Show>
    </span>
  );

  return (
    <Show when={clip()}>
      {c => (
        <DialogBoundary name={t('d.a2m.title')} close={closeAudioMidi}>
          <div class="overlay" onPointerDown={closeAudioMidi}>
            <div class="dialog a2mdlg" role="dialog" aria-labelledby="a2m-title" onPointerDown={e => e.stopPropagation()} data-a2m-dialog
              onKeyDown={e => {
                if (e.key === 'Escape') { e.preventDefault(); closeAudioMidi(); }
                else if (e.key === 'Enter' && !(e.target instanceof HTMLButtonElement) && !(e.target instanceof HTMLSelectElement) && !(e.target instanceof HTMLInputElement)) { e.preventDefault(); go(); }
              }}>
              <div class="stems-hd">
                <div class="stems-hd-txt">
                  <span id="a2m-title" class="stems-title">{t('d.a2m.title')}</span>
                  <span class="stems-sub">
                    <span class="ell" title={src()?.name}>{src()?.name}</span>
                    <span class="mono">{fmtSpan(seconds())}</span>
                    <Show when={pv()}>{p => (
                      <span class="detchip" title={t(p().noteBackend === 'game' ? 'd.a2m.notesGameTip' : 'd.a2m.notesTrackerTip') + (p().edited ? ' · ' + t('d.a2m.editedTip') : '')}>
                        {p().noteBackend === 'game' ? 'GAME' : t('d.pitch.notes.trackerShort')}{p().edited ? ' · ' + t('d.a2m.edited') : ''}
                      </span>
                    )}</Show>
                    <Show when={pv() && pv()!.noteBackend !== 'game'}>
                      <button class="linkbtn" title={t('d.a2m.betterTip')} onClick={() => { const id = c().id; closeAudioMidi(); A.openPitchEditor(id); }}>{t('d.a2m.better')}</button>
                    </Show>
                  </span>
                </div>
                <button class="stems-x" onClick={closeAudioMidi} title={t('d.stems.close')} aria-label={t('d.stems.close')}>×</button>
              </div>
              <Preview />
              <div class="a2m-row">
                <NumField label={t('d.a2m.shortest')} width={70} title={t('d.a2m.shortestTip')} value={() => Math.round(AMIDI.opts.shortest * 1000)}
                  text={v => Math.round(v) + ' ms'} parse={s => { const n = parseFloat(s); return Number.isFinite(n) ? n : null; }}
                  drag={(v, d) => Math.max(0, Math.min(500, Math.round(v + d)))}
                  apply={(v, mode) => setAudioMidiOpts({ shortest: Math.max(0, Math.min(500, v)) / 1000 }, mode === 'end' || mode === 'set')} />
                <label class="plab" title={t('d.a2m.timingTip')}>{t('d.a2m.timing')}
                  <select class="sel" value={AMIDI.opts.quantize} data-a2m-grid onChange={e => setAudioMidiOpts({ quantize: Number(e.currentTarget.value) }, true)}>
                    <For each={GRIDS}>{([v, k]) => <option value={v}>{k.startsWith('d.') ? t(k as 'd.a2m.asSung') : k}</option>}</For>
                  </select>
                </label>
                <label class="plab" title={t('d.a2m.velocityTip')}>{t('d.a2m.velocity')}
                  <select class="sel" value={AMIDI.opts.velocity == null ? 'follow' : 'fixed'} data-a2m-vel
                    onChange={e => setAudioMidiOpts({ velocity: e.currentTarget.value === 'follow' ? null : 0.8 }, true)}>
                    <option value="follow">{t('d.a2m.velFollow')}</option>
                    <option value="fixed">{t('d.a2m.velFixed')}</option>
                  </select>
                </label>
                <Show when={AMIDI.opts.velocity != null}>
                  <NumField label="VEL" width={52} title={t('d.a2m.velocityTip')} value={() => Math.round((AMIDI.opts.velocity ?? 0.8) * 100)} text={v => Math.round(v) + '%'}
                    parse={s => { const n = parseFloat(s); return Number.isFinite(n) ? n : null; }} drag={(v, d) => Math.max(1, Math.min(100, Math.round(v + d / 2)))}
                    apply={(v, mode) => setAudioMidiOpts({ velocity: Math.max(1, Math.min(100, v)) / 100 }, mode === 'end' || mode === 'set')} />
                </Show>
                {keyField('low')}
                {keyField('high')}
              </div>
              <Show when={AMIDI.error}><div class="stems-box err" role="alert" data-a2m-error>{AMIDI.error}</div></Show>
              <div class="stems-foot a2m-foot">
                <span class="a2m-stat" data-a2m-count>
                  <Show when={pv()} fallback={<span class="note">{t('d.a2m.finding')}</span>}>
                    {p => (p().count ? <><b>{t(p().count === 1 ? 'd.a2m.count1' : 'd.a2m.count', { n: p().count })}</b> · {rangeText()}</> :
                      <span class="note">{AMIDI.opts.low != null || AMIDI.opts.high != null ? t('d.a2m.noneInRange') : t('d.a2m.none')}</span>)}
                  </Show>
                </span>
                <div style={{ flex: 1 }} />
                <select class="sel" title={t('d.a2m.intoTip')} value={AMIDI.opts.channel ?? ''} data-a2m-channel
                  onChange={e => setAudioMidiOpts({ channel: e.currentTarget.value === '' ? null : Number(e.currentTarget.value) }, true)}>
                  <option value="">{t('d.chords.newPiano')}</option>
                  <For each={melodic()}>{ch => <option value={ch.id}>{ch.name}</option>}</For>
                </select>
                <button class="smallbtn" onClick={closeAudioMidi}>{t('d.pitch.cancel')}</button>
                <button class="smallbtn primary" disabled={!pv()?.count || AMIDI.busy} onClick={go} data-a2m-go title={t('d.a2m.convertTip')}>{t('d.a2m.convert')}</button>
              </div>
            </div>
          </div>
        </DialogBoundary>
      )}
    </Show>
  );
}

/** The sung pitch (grey) and the notes Convert writes (blue) over the clip's bars. */
function Preview() {
  let cv!: HTMLCanvasElement;
  const draw = () => {
    const c = AMIDI.win != null ? S.project.clips.find(x => x.id === AMIDI.win) : undefined;
    const p = AMIDI.preview, a = AMIDI.curve;
    if (!cv || !c) return;
    const dpr = window.devicePixelRatio || 1, W = cv.clientWidth, H = cv.clientHeight;
    if (!W || !H) return;
    cv.width = Math.round(W * dpr); cv.height = Math.round(H * dpr);
    const g = cv.getContext('2d'); if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const css = getComputedStyle(cv), col = (v: string, d: string) => css.getPropertyValue(v).trim() || d;
    g.fillStyle = col('--viz', '#0f1115'); g.fillRect(0, 0, W, H);
    const spb = 60 / S.project.bpm, st = c.stretch ?? 1;
    const b0 = c.start, b1 = c.start + c.len, xs = W / Math.max(1e-6, b1 - b0);
    const beatOf = (sec: number) => c.start + sec * st / spb - c.offset;
    const t0 = c.offset * spb / st, t1 = (c.offset + c.len) * spb / st;
    // the keys shown: the notes and the sung curve, at least an octave
    let lo = Infinity, hi = -Infinity;
    for (const n of p?.notes ?? []) { lo = Math.min(lo, n.p); hi = Math.max(hi, n.p); }
    if (a) for (let f = Math.floor(t0 * a.rate); f < Math.min(a.curve.length, t1 * a.rate); f++) { const m = a.curve[f]; if (m != null) { lo = Math.min(lo, m); hi = Math.max(hi, m); } }
    if (!(hi >= lo)) { lo = 55; hi = 72; }
    const mid = (lo + hi) / 2, span = Math.max(12, hi - lo + 4);
    lo = Math.floor(mid - span / 2); hi = Math.ceil(mid + span / 2);
    const ky = H / (hi - lo + 1), y = (m: number) => H - (m - lo + 1) * ky;
    // black keys, bar and beat lines
    for (let k = lo; k <= hi; k++) if ([1, 3, 6, 8, 10].includes(((k % 12) + 12) % 12)) { g.fillStyle = 'rgba(255,255,255,0.03)'; g.fillRect(0, y(k), W, ky); }
    for (let b = Math.ceil(b0); b <= b1; b++) {
      g.fillStyle = b % 4 === 0 ? 'rgba(255,255,255,0.12)' : 'rgba(255,255,255,0.04)';
      g.fillRect(Math.round((b - b0) * xs), 0, 1, H);
    }
    g.fillStyle = col('--fg5', '#808a99'); g.font = '10px ' + (col('--mono', 'monospace'));
    for (let k = lo; k <= hi; k++) if (((k % 12) + 12) % 12 === 0) g.fillText(noteName(k), 3, y(k) + ky - 2);
    // the notes
    const start = p?.start ?? 0;
    g.fillStyle = col('--blue', '#4f8ef7');
    for (const n of p?.notes ?? []) {
      const x = (start + n.step / 4 - b0) * xs, w = Math.max(2, n.len / 4 * xs - 1);
      g.globalAlpha = 0.45 + 0.5 * n.v; g.fillRect(x, y(n.p) + 1, w, Math.max(2, ky - 2));
    }
    g.globalAlpha = 1;
    // the sung pitch on top
    if (a) {
      g.strokeStyle = col('--fg3', '#a8b0bb'); g.lineWidth = 1.2; g.beginPath();
      let pen = false;
      for (let f = Math.max(0, Math.floor(t0 * a.rate)); f < Math.min(a.curve.length, Math.ceil(t1 * a.rate)); f++) {
        const m = a.curve[f];
        if (m == null) { pen = false; continue; }
        const x = (beatOf(f / a.rate) - b0) * xs, yy = y(m) + ky / 2;
        if (pen) g.lineTo(x, yy); else g.moveTo(x, yy);
        pen = true;
      }
      g.stroke();
    }
  };
  effect(() => {
    // redraw when anything drawn changes
    void AMIDI.preview; void AMIDI.curve; void AMIDI.win; void S.project.bpm;
    draw();
  });
  onMount(() => {
    const ro = new ResizeObserver(() => draw());
    ro.observe(cv);
    onCleanup(() => ro.disconnect());
  });
  return (
    <div class="a2m-pv">
      <canvas ref={cv} data-a2m-preview aria-label={t('d.a2m.previewLabel')} />
      <Show when={!AMIDI.preview && !AMIDI.error}><div class="pstate"><span class="spin" />{t('d.a2m.finding')}</div></Show>
    </div>
  );
}
