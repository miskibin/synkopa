// Parts of the toolbar's second row: the song position slider (FL's, under the transport), the
// position readout, and the status line, which shows the Browser's preview (FL shows the clicked
// sample there) and otherwise what the last action did and what the tool does.
import { For, Show } from 'solid-js';
import { createSignal, onCleanup, onMount } from '../lib/reactive';
import { t } from '../lib/i18n';
import { clamp, fmtClock, fmtPos } from '../lib/format';
import { songEnd } from '../lib/export';
import { S } from '../state/store';
import { setU, U } from '../state/ui';
import { curBeat, tele } from '../state/telemetry';
import { curPat, patMode, patStartOf, previewPattern, setPatStart } from '../state/pattern';
import * as A from '../state/actions';
import * as MK from '../state/markers';
import { addFrameTask, fitCanvas } from '../ui/frameLoop';
import { track } from '../ui/drag';
import { shownPreview } from '../state/previewWave';

const BEATS_PER_BAR = 4;
/** The track's inset from the slider's ends (styles.css `.songpos .sptrack`): the handle stays whole at both ends. */
const INSET = 6;

/** What the slider and the readout follow: the song (SONG) or the current pattern (PAT, FL's Piano roll position). */
function span(): { len: number; at: number; pat: number | null } {
  if (patMode()) {
    const pt = curPat();
    if (!pt) return { len: BEATS_PER_BAR, at: 0, pat: null };
    const playing = U.patPlaying && previewPattern() === pt.id && tele.frame.patBeat >= 0;
    return { len: Math.max(pt.len, 1), at: playing ? tele.frame.patBeat : patStartOf(pt.id), pat: pt.id };
  }
  const loopEnd = S.project.loop?.b ?? 0;
  return { len: Math.max(songEnd(S.project), loopEnd, BEATS_PER_BAR), at: curBeat(), pat: null };
}

/** Moves the start (and the playhead, when it plays) to `beat` of what the slider follows. */
function seekTo(beat: number, pat: number | null) {
  if (pat != null) setPatStart(pat, beat);
  else A.seekSong(beat);
}

/**
 * FL's song position slider: where the playhead is in the song (SONG) or in the current pattern
 * (PAT); a press or drag moves it there, snapped to the beat (Shift: to the 16th). While playing
 * the handle follows the pointer and the jump happens on release, so the song is not restarted at
 * every pixel; stopped, the start marker follows the drag. ← → move a bar (Shift: a beat), Home / End.
 */
export function SongPos() {
  let el!: HTMLDivElement, fill!: HTMLDivElement, head!: HTMLDivElement;
  const [drag, setDrag] = createSignal<number | null>(null);
  const [len, setLen] = createSignal(16);
  let shown = -1;
  onMount(() => onCleanup(addFrameTask(() => {
    const s = span(), d = drag();
    if (s.len !== len()) setLen(s.len);
    const f = Math.round(clamp((d ?? s.at) / s.len, 0, 1) * 2000) / 2000;
    if (f === shown) return;
    shown = f;
    // the handle rides the track, which is inset by INSET on both sides
    head.style.left = `calc(${INSET}px + (100% - ${2 * INSET}px) * ${f})`;
    fill.style.width = f * 100 + '%';
    el.setAttribute('aria-valuenow', String(Math.round((d ?? s.at) * 100) / 100));
  })));
  const beatAt = (clientX: number, fine: boolean) => {
    const r = el.getBoundingClientRect(), s = span(), q = fine ? 0.25 : 1;
    const f = clamp((clientX - r.left - INSET) / Math.max(1, r.width - 2 * INSET), 0, 1);
    return clamp(Math.round((f * s.len) / q) * q, 0, Math.max(0, s.len - (s.pat != null ? q : 0)));
  };
  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault(); el.focus({ preventScroll: true });
    const pat = span().pat, live = !(U.playing || U.patPlaying);
    const go = (ev: PointerEvent) => {
      const b = beatAt(ev.clientX, ev.shiftKey);
      setDrag(b);
      setU('readout', t('a.tb.posAt', { pos: fmtPos(b) }));
      if (live) seekTo(b, pat);
    };
    go(e);
    track(go, () => { const b = drag(); setDrag(null); if (b != null && !live) seekTo(b, pat); }, 'ew-resize');
  };
  const key = (e: KeyboardEvent) => {
    const s = span(), step = e.shiftKey ? 1 : BEATS_PER_BAR;
    let b: number | null = null;
    if (e.key === 'ArrowRight') b = Math.floor(s.at / step + 1e-6) * step + step;
    else if (e.key === 'ArrowLeft') b = Math.ceil(s.at / step - 1e-6) * step - step;
    else if (e.key === 'Home') b = 0;
    else if (e.key === 'End') b = s.pat != null ? s.len - 1 : s.len;
    if (b == null) return;
    e.preventDefault(); e.stopPropagation();
    seekTo(clamp(b, 0, s.len), s.pat);
  };
  const loop = () => (patMode() ? null : S.project.loop);
  return (
    <div ref={el} class="songpos" role="slider" tabindex={0} aria-label={patMode() ? t('a.tb.posPat') : t('a.tb.posSong')} aria-valuemin={0} aria-valuemax={len()}
      title={patMode() ? t('a.tb.posPatTip') : t('a.tb.posSongTip')} onPointerDown={down} onKeyDown={key}>
      <div class="sptrack">
        <Show when={loop()}>{l => <div class="sploop" style={{ left: (l().a / len()) * 100 + '%', width: (Math.max(0, l().b - l().a) / len()) * 100 + '%' }} />}</Show>
        <div ref={fill} class={['spfill', { pat: patMode() }]} />
        <Show when={!patMode()}>
          <For each={MK.markers()}>{m => <div class="spmark" style={{ left: clamp(m.at / len(), 0, 1) * 100 + '%' }} title={m.name} />}</For>
        </Show>
      </div>
      <div ref={head} class="sphead" />
    </div>
  );
}

/** The position as bar:beat:step, of the song or (PAT) of the current pattern, like the slider. */
export function PosReadout() {
  let el!: HTMLSpanElement;
  let last = '';
  onMount(() => onCleanup(addFrameTask(() => {
    const s = fmtClock(span().at);
    if (s !== last) { last = s; el.textContent = s; }
  })));
  return <span ref={el} class="clock" title={t('a.tb.clockTip')}>1:01:01</span>;
}

const fmtSecs = (s: number) => (s < 10 ? s.toFixed(2) : s < 60 ? s.toFixed(1) : `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`);

/**
 * The Browser's preview in the status line: the sample's name, its waveform (the part already
 * heard bright) and where it is. Drawn per frame only while a preview plays; nothing to click.
 */
export function PreviewLine() {
  let cv!: HTMLCanvasElement, time!: HTMLSpanElement;
  let drawn = '';
  onMount(() => onCleanup(addFrameTask(() => {
    const w = shownPreview();
    if (!w || !cv?.isConnected || !time) return;
    const W = cv.clientWidth, H = cv.clientHeight;
    if (!W || !H) return;
    const pos = Math.max(0, tele.frame.previewPos), f = w.secs > 0 ? clamp(pos / w.secs, 0, 1) : 0;
    const k = `${w.key}|${W}|${H}|${Math.round(f * W)}`;
    const tx = fmtSecs(pos) + ' / ' + fmtSecs(w.secs) + ' s';
    if (time.textContent !== tx) time.textContent = tx;
    if (k === drawn) return;
    drawn = k;
    fitCanvas(cv, W, H);
    const g = cv.getContext('2d')!, dpr = window.devicePixelRatio || 1;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, W, H);
    const n = w.peaks.length / 2, css = getComputedStyle(cv);
    const on = css.getPropertyValue('--pv-on').trim() || '#e8ebf0', off = css.getPropertyValue('--pv-off').trim() || 'rgba(255,255,255,.25)';
    const mid = H / 2, played = f * W;
    // one bar every 2 px: the loudest of the pairs under it
    for (let x = 0; x < W; x += 2) {
      const a = Math.floor((x / W) * n), b = Math.max(a + 1, Math.floor(((x + 2) / W) * n));
      let mx = 0;
      for (let i = a; i < b && i < n; i++) mx = Math.max(mx, -w.peaks[i * 2], w.peaks[i * 2 + 1]);
      const h = Math.max(0.5, Math.min(1, mx) * (mid - 0.5));
      g.fillStyle = x < played ? on : off;
      g.fillRect(x, mid - h, 1, h * 2);
    }
  })));
  return (
    <Show when={shownPreview()}>{w => (
      <span class="pvline" title={w().name}>
        <span class="pvname">{w().name}</span>
        <canvas ref={cv} class="pvwave" aria-hidden="true" />
        <span ref={time} class="pvtime" />
      </span>
    )}</Show>
  );
}
