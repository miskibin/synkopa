// Parts of the toolbar's second row: the position readout and the status line, which shows the Browser's preview (FL shows the clicked
// sample there) and otherwise what the last action did and what the tool does.
import { Show } from 'solid-js';
import { onCleanup, onMount } from '../lib/reactive';
import { t } from '../lib/i18n';
import { clamp, fmtClock } from '../lib/format';
import { songEnd } from '../lib/export';
import { S } from '../state/store';
import { U } from '../state/ui';
import { curBeat, tele } from '../state/telemetry';
import { curPat, patMode, patStartOf, previewPattern } from '../state/pattern';
import { addFrameTask, fitCanvas } from '../ui/frameLoop';
import { shownPreview } from '../state/previewWave';

const BEATS_PER_BAR = 4;

/** What the readout follows: the song (SONG) or the current pattern (PAT, FL's Piano roll position). */
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

/** The position as bar:beat:step, of the song or (PAT) of the current pattern. */
export function PosReadout() {
  let el!: HTMLSpanElement;
  let last = '';
  onMount(() => onCleanup(addFrameTask(() => {
    const s = fmtClock(span().at);
    if (s !== last) { last = s; el.textContent = s; }
  })));
  return <span ref={el} class="clock" title={t('a.tb.clockTip')}>1:1:1</span>;
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
