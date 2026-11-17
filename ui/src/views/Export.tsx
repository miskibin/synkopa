// Export: what to render (mix or stems, song or loop, tail) next to the file format (WAV 16/24/32
// or MP3, sample rate), with a one-line summary of what will be written and its size. While it
// renders, the dialog shows the engine's progress: the file in progress, a live peak waveform,
// the song position, elapsed and remaining time, render speed and the peak level.
import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { be } from '../api/backend';
import { t } from '../lib/i18n';
import { baseName, fmtBytes } from '../lib/format';
import {
  depthText, exportBytes, exportOverall, exportRange, exportRemaining, exportSeconds, fmtExportLen, fmtMs, hasLoop, stemInserts, wavePath,
  type ExportEncoding,
} from '../lib/export';
import type { ExportAudioOptions, ExportProgress } from '../api/types';
import { S } from '../state/store';
import { say, setU, U } from '../state/ui';
import { Segmented } from '../ui/Knob';

const TAILS = [0, 2, 8, 0] as const;
const WRAP = 3;
const RATES = [48000, 44100] as const;
const KBPS = [320, 256, 192, 128] as const;
const BITS = [16, 24, 32] as const;
const FKEY = 'wave.export.fmt';

type Saved = {
  mp3?: boolean; stemsMp3?: boolean; kb?: number; stems?: boolean; master?: boolean;
  wavBits?: number; dither?: boolean; dither16?: boolean; rate?: number; tail?: number;
};

export function Export() {
  return <Show when={U.exportOpen}><ExportDialog /></Show>;
}

function ExportDialog() {
  const saved: Saved = (() => { try { return (JSON.parse(localStorage.getItem(FKEY) ?? '{}') ?? {}) as Saved; } catch { return {}; } })();
  const loopOk = () => hasLoop(S.project.loop);
  const [useLoop, setUseLoop] = createSignal(false);
  const [tailI, setTailI] = createSignal<0 | 1 | 2 | 3>(([0, 1, 2, 3] as const).find(i => i === saved.tail) ?? 1); // 2 s
  const [rateI, setRateI] = createSignal(Math.max(0, RATES.indexOf((saved.rate ?? 48000) as 48000)));
  const [mixMp3, setMixMp3] = createSignal(!!saved.mp3);
  const [stemsMp3, setStemsMp3] = createSignal(!!saved.stemsMp3);
  const [kbI, setKbI] = createSignal(Math.max(0, KBPS.indexOf((saved.kb ?? 320) as 320)));
  const [stems, setStems] = createSignal(!!saved.stems);
  const [master, setMaster] = createSignal(saved.master !== false);
  const [wavBits, setWavBits] = createSignal<16 | 24 | 32>(BITS.find(b => b === saved.wavBits) ?? 32);
  // 16-bit is dithered unless turned off (truncation distortion is audible there); 24-bit keeps its own choice
  const [dither24, setDither24] = createSignal(saved.dither === true);
  const [dither16, setDither16] = createSignal(saved.dither16 !== false);
  const dither = () => wavBits() === 16 ? dither16() : wavBits() === 24 && dither24();
  const setDither = (v: boolean) => (wavBits() === 16 ? setDither16 : setDither24)(v);
  const remember = () => {
    try {
      localStorage.setItem(FKEY, JSON.stringify({
        mp3: mixMp3(), stemsMp3: stemsMp3(), kb: KBPS[kbI()], stems: stems(), master: master(),
        wavBits: wavBits(), dither: dither24(), dither16: dither16(), rate: RATES[rateI()], tail: tailI(),
      } satisfies Saved));
    } catch { /* ignore */ }
  };
  const mp3 = () => stems() ? stemsMp3() : mixMp3();
  const setMp3 = (value: boolean) => { (stems() ? setStemsMp3 : setMixMp3)(value); remember(); };
  const wav = () => !mp3();
  const canWrap = () => wav() && useLoop() && loopOk();
  const tailChoice = () => tailI() === WRAP && !canWrap() ? 1 : tailI();
  const tailSeconds = () => TAILS[tailChoice()];
  const enc = (): ExportEncoding => mp3() ? { format: 'mp3', kbps: KBPS[kbI()] } : { format: 'wav', bits: wavBits() };
  const encText = () => mp3() ? `MP3 ${KBPS[kbI()]} kbps` : depthText(wavBits()) + (dither() ? ' + dither' : '');

  const stemNames = () => [...(master() ? ['Master'] : []), ...stemInserts(S.project).map(m => m.name.trim() || `Insert ${m.id}`)];
  const nFiles = () => stemNames().length;
  const range = () => exportRange(S.project, useLoop() && loopOk());
  const seconds = () => exportSeconds(range().from, range().to, S.project.bpm, tailSeconds());
  const bytes = () => exportBytes(seconds(), RATES[rateI()], enc()) * (stems() ? nFiles() : 1);
  const songName = () => (S.path ? baseName(S.path).replace(/\.(synkopa|wave)$/i, '') : 'Song') + '.' + (mp3() ? 'mp3' : 'wav');

  // ---- progress
  const [busy, setBusy] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  const [prog, setProg] = createSignal<ExportProgress | null>(null);
  const [startedAt, setStartedAt] = createSignal(0);
  const [now, setNow] = createSignal(0);
  onMount(() => {
    const un = be().on('export-progress', p => {
      if (!busy()) return;
      // the clock starts with the render, not while the save dialog is open
      if (!prog()) { setStartedAt(Date.now()); setNow(Date.now()); }
      setProg(p);
    });
    const tick = setInterval(() => { if (busy() && prog()) setNow(Date.now()); }, 250);
    onCleanup(() => { clearInterval(tick); void un.then(f => f()); });
  });
  const elapsed = () => Math.max(0, now() - startedAt());
  const overall = () => prog() ? exportOverall(prog()!) : 0;
  const remaining = () => exportRemaining(elapsed(), overall());
  // audio seconds rendered per wall-clock second, over the whole export so far
  const speed = () => {
    const p = prog();
    if (!p || elapsed() < 600) return null;
    const audio = (p.done + (p.stage === 'render' ? p.frac : p.stage === 'prepare' ? 0 : 1)) * seconds();
    return audio / (elapsed() / 1000);
  };
  const peakDb = () => {
    const pk = prog()?.peak ?? 0;
    return pk < 1e-5 ? '-∞ dBFS' : (20 * Math.log10(pk)).toFixed(1) + ' dBFS';
  };
  const over = () => (prog()?.peak ?? 0) > 1;
  const posText = () => {
    const p = prog(), r = range();
    if (!p) return '';
    // a wrapped loop never passes `to`: its second pass starts over at `from`
    if (p.stage === 'render' && p.beat >= r.to - 1e-6) return t('a.ex.sTail');
    return t('a.ex.pos', { pos: Math.min(Math.floor(p.beat / 4) + 1, Math.ceil(r.to / 4)), end: Math.ceil(r.to / 4) });
  };
  const stageText = () => {
    const p = prog();
    if (!p) return stems() ? t('a.ex.waitFolder') : t('a.ex.waitPick');
    if (p.stage === 'prepare') return t('a.ex.sPrepare');
    if (p.stage === 'write') return t('a.ex.sWrite', { fmt: mp3() ? 'MP3' : 'WAV' });
    return stems() ? t('a.ex.pStem', { i: Math.min(p.done + 1, p.total), n: p.total, name: p.name }) : t('a.ex.pMix', { name: p.name || songName() });
  };

  const close = () => { if (!busy()) setU('exportOpen', false); };

  const start = async () => {
    if (busy() || (stems() && nFiles() === 0)) return;
    setBusy(true); setU('exportBusy', true); setErr(null); setProg(null);
    try {
      const common = { useLoop: useLoop() && loopOk(), tail: tailSeconds(), sampleRate: RATES[rateI()] };
      const options: ExportAudioOptions = mp3()
        ? { ...common, format: 'mp3', mp3Kbps: KBPS[kbI()] }
        : { ...common, format: 'wav', wavBits: wavBits(), dither: dither(), wrapTail: tailChoice() === WRAP };
      if (stems()) {
        const r = await be().exportStems({ ...options, includeMaster: master() });
        setU('exportBusy', false);
        if (!U.exportOpen) return; // dialog was closed (Escape) while bouncing
        if (r.cancelled) {
          if (prog()) say(t('a.ex.cancelled'));
          setBusy(false); setProg(null);
          return;
        }
        const clipped = r.files.reduce((n, f) => n + f.stats.clippedSamples, 0);
        const clip = clipped > 0 ? ' · ' + t('a.ex.clipped', { n: clipped }) : '';
        const skipped = r.skipped.length ? ' · ' + t('a.ex.stemsSkipped', { n: r.skipped.length, names: r.skipped.join(', ') }) : '';
        say(t('a.ex.doneStems', { n: r.files.length, dir: baseName(r.dir), len: fmtExportLen(r.files[0].stats.seconds), clip, skipped }) + tookText());
        setU('exportOpen', false);
        return;
      }
      const r = await be().exportAudio(options);
      setU('exportBusy', false);
      if (!U.exportOpen) return; // dialog was closed (Escape) while bouncing
      if (r.cancelled) {
        if (prog()) say(t('a.ex.cancelled'));
        setBusy(false); setProg(null);
        return;
      }
      const peak = r.stats.silent ? t('a.ex.silence') : r.stats.peakDbfs.toFixed(1) + ' dBFS';
      const clip = r.stats.clippedSamples > 0 ? ' · ' + t('a.ex.clipped', { n: r.stats.clippedSamples }) : '';
      say(t('a.ex.done', { name: baseName(r.out), len: fmtExportLen(r.stats.seconds), peak, clip }) + tookText());
      setU('exportOpen', false);
    } catch (e) {
      setU('exportBusy', false);
      if (!U.exportOpen) return;
      setErr(String(e));
      setBusy(false); setProg(null);
    }
  };
  const tookText = () => prog() ? ' · ' + t('a.ex.took', { t: fmtMs(Date.now() - startedAt()) }) : '';

  const cancel = () => {
    if (busy()) {
      void be().exportCancel().catch(() => {});
      return;
    }
    setU('exportOpen', false);
  };

  const fileState = (i: number) => {
    const p = prog();
    if (!p) return '';
    return i < p.done ? 'ok' : i === p.done && p.stage !== 'done' ? 'cur' : '';
  };

  return (
    <div class="overlay" onPointerDown={() => close()}>
      <div class="dialog exdlg" role="dialog" aria-modal="true" aria-label={t('a.ex.title')} onPointerDown={e => e.stopPropagation()}
        onKeyDown={e => {
          if (e.key === 'Enter' && !e.repeat && !busy() && (e.target as HTMLElement).tagName !== 'BUTTON') { e.preventDefault(); void start(); }
          if (e.key !== 'Escape') e.stopPropagation();
        }}>
        <div class="hd"><span>{t('a.ex.title')}</span><button disabled={busy()} onClick={() => setU('exportOpen', false)} aria-label={t('a.ex.cancel')}>×</button></div>
        <Show when={!busy()} fallback={
          <div class="exprog" aria-live="polite">
            <div class="row exhead">
              <span class="exname" title={prog()?.name}>{stageText()}</span>
              <span class="mono">{Math.floor(overall() * 100)} %</span>
            </div>
            <div class="exwave" classList={{ idle: !prog() || prog()!.stage === 'prepare' }}>
              <svg viewBox="0 0 240 48" preserveAspectRatio="none" aria-hidden="true">
                <path d={wavePath(prog()?.wave ?? [], 240, 48)} />
                <Show when={prog()?.stage === 'render'}><rect class="head" x={Math.min(239.5, (prog()!.frac) * 240)} y="0" width="0.6" height="48" /></Show>
              </svg>
            </div>
            <div class="prog" classList={{ busy: !prog() }}><div style={prog() ? { width: (overall() * 100).toFixed(1) + '%' } : {}} /></div>
            <div class="exstats mono">
              <span>{posText()}</span>
              <span>
                {prog() ? t('a.ex.elapsed', { t: fmtMs(elapsed()) }) : ''}
                <Show when={remaining() != null}>{' · ' + t('a.ex.left', { t: fmtMs(remaining()!) })}</Show>
                <Show when={speed() != null}>{' · ' + t('a.ex.speed', { x: speed()! >= 10 ? Math.round(speed()!) : speed()!.toFixed(1) })}</Show>
              </span>
            </div>
            <Show when={prog()}>
              <div class="exstats mono">
                <span classList={{ exover: over() && !(wav() && wavBits() === 32) }}>
                  {t('a.ex.peak', { db: peakDb() })}{over() ? ' · ' + (wav() && wavBits() === 32 ? t('a.ex.peakFloat') : t('a.ex.peakClip')) : ''}
                </span>
                <span>{encText()} · {RATES[rateI()] / 1000} kHz</span>
              </div>
            </Show>
            <Show when={stems()}>
              <div class="exstems">
                <For each={stemNames()}>{(n, i) => <span class={fileState(i())} title={n}>{fileState(i()) === 'ok' ? '✓ ' : ''}{n}</span>}</For>
              </div>
            </Show>
            <div class="row" style={{ 'justify-content': 'flex-end', gap: '8px' }}>
              <button class="smallbtn" onClick={cancel}>{t('a.ex.cancel')}</button>
            </div>
          </div>
        }>
          <div class="exgrid">
            <section class="exsec">
              <h4>{t('a.ex.what')}</h4>
              <div class="form">
                <label>{t('a.ex.output')}</label>
                <Segmented options={[t('a.ex.mix'), t('a.ex.stems')]} value={() => (stems() ? 1 : 0)}
                  pick={i => { setStems(i === 1); remember(); }} title={t('a.ex.outputTip')} />
                <Show when={stems()}>
                  <label>{t('a.ex.master')}</label>
                  <Segmented options={[t('a.ex.masterOn'), t('a.ex.masterOff')]} value={() => (master() ? 0 : 1)}
                    pick={i => { setMaster(i === 0); remember(); }} title={t('a.ex.masterTip')} />
                </Show>
                <label>{t('a.ex.range')}</label>
                <Segmented options={loopOk() ? [t('a.ex.song'), t('a.ex.loop')] : [t('a.ex.song')]} value={() => (useLoop() && loopOk() ? 1 : 0)}
                  pick={i => setUseLoop(i === 1)} title={loopOk() ? t('a.ex.rangeTip') : t('a.ex.rangeTipNoLoop')} />
                <label>{t('a.ex.tail')}</label>
                <Segmented options={canWrap() ? [t('a.ex.cut'), '2 s', '8 s', t('a.ex.wrap')] : [t('a.ex.cut'), '2 s', '8 s']} value={tailChoice}
                  pick={i => { setTailI(i as 0 | 1 | 2 | 3); remember(); }} title={t(canWrap() ? 'a.ex.wrapTip' : 'a.ex.tailTip')} />
              </div>
            </section>
            <section class="exsec">
              <h4>{t('a.ex.fileFormat')}</h4>
              <div class="form">
                <label>{t('a.ex.format')}</label>
                <Segmented options={['WAV', 'MP3']} value={() => (mp3() ? 1 : 0)} pick={i => setMp3(i === 1)} title={t('a.ex.formatTip')} />
                <Show when={wav()} fallback={<>
                  <label>{t('a.ex.mp3q')}</label>
                  <Segmented options={KBPS.map(k => String(k))} value={() => kbI()} pick={i => { setKbI(i); remember(); }} title={t('a.ex.mp3qTip')} />
                </>}>
                  <label>{t('a.ex.bitDepth')}</label>
                  <Segmented options={['16', '24', '32 float']} value={() => BITS.indexOf(wavBits())}
                    pick={i => { setWavBits(BITS[i]); remember(); }} title={t('a.ex.bitsTip')} />
                  <Show when={wavBits() !== 32}>
                    <label>{t('a.ex.dither')}</label>
                    <label class="row exchk" title={t('a.ex.ditherTip')}>
                      <input type="checkbox" checked={dither()} onChange={e => { setDither(e.currentTarget.checked); remember(); }} />
                      {t('a.ex.ditherLabel')}
                    </label>
                  </Show>
                </Show>
                <label>{t('a.ex.rate')}</label>
                <Segmented options={['48 kHz', '44.1 kHz']} value={() => rateI()} pick={i => { setRateI(i as 0 | 1); remember(); }} title={t('a.ex.rateTip')} />
              </div>
            </section>
          </div>
          <div class="exsum mono" title={stems() ? t('a.ex.sumTipStems') : undefined}>
            <span>{stems() ? t('a.ex.sumFiles', { n: nFiles() }) : songName()}</span>
            <span>{fmtExportLen(seconds())}</span>
            <span>{RATES[rateI()] === 48000 ? '48 kHz' : '44.1 kHz'}</span>
            <span>{encText()}</span>
            <span>≈ {fmtBytes(bytes())}</span>
          </div>
          <Show when={stems() && nFiles() === 0}><div class="note err" style={{ 'margin-bottom': '10px' }}>{t('a.ex.stemsNone')}</div></Show>
          <Show when={err()}><div class="note err" style={{ 'margin-bottom': '10px' }}>{err()}</div></Show>
          <div class="row" style={{ 'justify-content': 'flex-end', gap: '8px' }}>
            <button class="smallbtn" onClick={cancel}>{t('a.ex.cancel')}</button>
            <button class="smallbtn primary" disabled={stems() && nFiles() === 0} onClick={() => void start()}>{stems() ? t('a.ex.goStems') : t('a.ex.go')}</button>
          </div>
        </Show>
      </div>
    </div>
  );
}
