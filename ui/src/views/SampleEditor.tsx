// One floating sample editor for Playlist audio clips and Channel Rack sampler channels.
import { createEffect, createMemo, For, onCleanup, onMount, Show, type JSX } from 'solid-js';
import { be } from '../api/backend';
import { CLIP_PITCH, SAMPLER, samplerDefault, type AutoTarget, type Channel, type Clip, type Op, type SamplerState, type Source } from '../api/types';
import { FIT_BEATS, fitClip, TOTAL } from '../lib/clipOps';
import { baseName, clamp, dbg, dbText, panText } from '../lib/format';
import { t, type TranslationKey } from '../lib/i18n';
import { attackFromT, attackToT, decayFromT, decayToT, envelopePoints, msText, releaseFromT, releaseToT } from '../lib/sampler';
import * as A from '../state/actions';
import { autoDisplay, controlMenu, isAutomated, paramMenu } from '../state/automation';
import { commit, Gesture, wheelEdit } from '../state/edit';
import { audition, chanById, chanOp, chanSource, replaceWithFile } from '../state/pattern';
import { getPeaks, peakRange, peaksVersion } from '../state/peaks';
import { plain, S } from '../state/store';
import { tele } from '../state/telemetry';
import { say, setU, U, type SampleTarget } from '../state/ui';
import { track } from '../ui/drag';
import { addFrameTask, fitCanvas, Layer } from '../ui/frameLoop';
import { channelIcon, guessIcon, InstIcon } from '../ui/instIcons';
import { InsertPicker } from '../ui/InsertPicker';
import { Knob } from '../ui/Knob';
import { PianoKeys } from '../ui/PianoKeys';
import { rgba, T, theme } from '../ui/theme';
import { chanHeardInsert, ChannelLevelKnobs, ChannelRootKnob } from './ChannelWindow';
import { moveWindow } from './FxWindow';
import { PitchCorrectButton } from './PitchCorrectButton';

type SampleChannel = Extract<Channel, { kind: 'sample' }>;
type Fade = 'fadeIn' | 'fadeOut';
type Handle = 'start' | 'end' | Fade;

export function SampleEditor() {
  // Remount controls when the target changes, so an in-progress gesture keeps its original target.
  const target = createMemo(() => {
    const ed = U.editor;
    return ed ? { kind: ed.kind, id: ed.id } : null;
  });
  return <Show when={target()} keyed>{ed => <EditorTarget target={ed} />}</Show>;
}

function EditorTarget(props: { target: SampleTarget }) {
  const id = props.target.id;
  if (props.target.kind === 'clip') {
    const clip = () => S.project.clips.find(c => c.id === id && c.kind === 'audio');
    const source = () => {
      const ref = clip()?.ref;
      return S.project.sources.find(s => s.id === ref);
    };
    return <Show when={clip() && source()}><ClipSample clip={() => clip()!} source={() => source()!} /></Show>;
  }
  const channel = () => { const c = chanById(id); return c?.kind === 'sample' ? c : undefined; };
  return <Show when={channel()}>{c => <ChannelSample channel={c} />}</Show>;
}

type WaveRegion = { start: number; end: number; fadeIn: number; fadeOut: number };
type DragEdit = { op: Op; status: string };
interface WaveProps {
  source: () => Source | undefined;
  color: () => string;
  normalized: () => boolean;
  reversed: () => boolean;
  gain?: () => number;
  envelope?: () => [number, number][];
  region: () => WaveRegion;
  preview: () => void;
  previewPosition?: () => number | null;
  title: TranslationKey;
  drag: (handle: Handle) => (delta: number) => DragEdit | undefined;
  reset?: (handle: Handle | undefined) => void;
}

function EditorFrame(props: {
  name: string; description: string; color: string; icon: JSX.Element; wave: WaveProps;
  fileInfo: JSX.Element; fileAction?: JSX.Element; header: JSX.Element; channelId?: number;
  toggle: (key: 'reverse' | 'norm') => void;
  pitch: JSX.Element; level: JSX.Element; fades: JSX.Element; details: JSX.Element;
}) {
  const editor = () => U.editor!;
  const move = (x: number, y: number) => setU('editor', ed => ed && { ...ed, x, y });
  const resize = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault(); e.stopPropagation();
    const start = editor().w, sx = e.clientX;
    track(ev => {
      const max = window.innerWidth - 20;
      const w = clamp(start + ev.clientX - sx, Math.min(520, max), max);
      setU('editor', ed => ed && { ...ed, w, x: clamp(ed.x, 0, window.innerWidth - w) });
    }, null, 'ew-resize');
  };

  return (
    <div class="win chanwin smpwin" data-editor data-channel-window={props.channelId}
      style={{ left: editor().x + 'px', top: editor().y + 'px', width: editor().w + 'px', 'z-index': 47 }}>
      <div class="winhd" onPointerDown={e => moveWindow(e, editor, move)}>
        <span class="chicon" style={{ color: props.color }}>{props.icon}</span>
        <span class="title ell" style={{ 'min-width': '0', flex: '0 1 auto' }} title={props.name}>{props.name}</span>
        <span class="sub ell">{props.description}</span>
        <div style={{ flex: 1 }} />
        {props.header}
        <button class="iconbtn" aria-label={t('b.cw.previewTitle')} title={t('b.cw.previewTitle')} onClick={() => props.wave.preview()}
          disabled={!props.wave.source()}>{props.channelId == null && U.previewing ? '■' : '▶'}</button>
        <button class="closebtn" aria-label={t('d.smp.close')} title={t('d.smp.close')} onClick={A.closeEditor}>×</button>
      </div>
      <SampleWave {...props.wave} />
      <div class="chsrc">
        <span class="ell" title={props.wave.source()?.path ?? ''}>{baseName(props.wave.source()?.path ?? '') || t('b.cw.noFile')}</span>
        <span class="mono">{props.fileInfo}</span>
        {props.fileAction}
      </div>
      <div class="smprow">
        <button class="smptog" aria-pressed={props.wave.reversed()} classList={{ on: props.wave.reversed() }}
          title={t('b.sm.reverseTitle')} onClick={() => props.toggle('reverse')}>{t('b.sm.reverse')}</button>
        <button class="smptog" aria-pressed={props.wave.normalized()} classList={{ on: props.wave.normalized() }}
          title={t('b.sm.normTitle')} onClick={() => props.toggle('norm')}>{t('b.sm.norm')}</button>
      </div>
      <div class="smpknobs">
        <div class="smpgrp">{props.pitch}</div>
        <div class="smpgrp">{props.level}</div>
        <div class="smpgrp">{props.fades}</div>
      </div>
      {props.details}
      <div class="resizer" onPointerDown={resize} title={t('d.smp.resize')} />
    </div>
  );
}

const HANDLE_LABELS = { start: 'd.smp.regstart', end: 'd.smp.regend', fadeIn: 'd.smp.fadein.tip', fadeOut: 'd.smp.fadeout.tip' } as const;
const FADE_LABELS = { fadeIn: 'd.smp.k.fadein', fadeOut: 'd.smp.k.fadeout' } as const;
const HANDLES = ['start', 'end', 'fadeIn', 'fadeOut'] as const;

function SampleWave(props: WaveProps) {
  let box!: HTMLDivElement, canvas!: HTMLCanvasElement, line!: HTMLDivElement;
  const layer = new Layer();
  let dirty = true, epoch = -1;

  createEffect(() => {
    props.source(); props.color(); props.normalized(); props.reversed(); props.gain?.();
    props.region(); props.envelope?.(); peaksVersion();
    dirty = true;
  });
  onMount(() => {
    const observer = new ResizeObserver(() => { dirty = true; });
    observer.observe(box);
    const stop = addFrameTask(() => {
      if (epoch !== theme.epoch) { epoch = theme.epoch; dirty = true; }
      if (dirty) { dirty = false; draw(); }
      if (!props.previewPosition) return;
      const pos = props.previewPosition();
      line.style.opacity = pos == null ? '0' : '1';
      if (pos != null) line.style.transform = `translateX(${pos * box.clientWidth}px)`;
    });
    onCleanup(() => { observer.disconnect(); stop(); });
  });

  const draw = () => {
    const w = box.clientWidth, h = box.clientHeight;
    if (!w || !h) return;
    fitCanvas(canvas, w, h);
    const g = canvas.getContext('2d')!, dpr = window.devicePixelRatio || 1;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const src = props.source(), norm = props.normalized(), rev = props.reversed();
    const peaks = src && getPeaks(src.id, norm, rev), gain = props.gain?.() ?? 1, color = props.color();
    const key = [src?.id, norm, rev, gain, color, peaksVersion()].join('|');
    const image = layer.get(w, h, key, (l, lw, lh) => {
      l.fillStyle = T.viz; l.fillRect(0, 0, lw, lh);
      l.fillStyle = T.vizGrid2; l.fillRect(0, Math.floor(lh / 2), lw, 1);
      if (!peaks) return;
      const bins = peaks.length / 2, mid = lh / 2, amp = mid * 0.95;
      l.fillStyle = color; l.beginPath();
      for (let x = 0; x < lw; x++) {
        const a = Math.floor(x / lw * bins), b = Math.floor((x + 1) / lw * bins);
        const [mn, mx] = peakRange(peaks, a, b);
        const y1 = mid - clamp(mx * gain, 0, 1) * amp, y2 = mid - clamp(mn * gain, -1, 0) * amp;
        l.rect(x, y1, 1, Math.max(0.6, y2 - y1));
      }
      l.fill();
    });
    g.drawImage(image, 0, 0, w, h);
    if (!src) return;

    const { start, end, fadeIn, fadeOut } = props.region();
    const a = start * w, b = end * w, fi = fadeIn * w, fo = fadeOut * w;
    g.fillStyle = T.shade; g.beginPath();
    g.moveTo(a, h); g.lineTo(a + fi, 0); g.lineTo(a, 0); g.closePath();
    g.moveTo(b - fo, 0); g.lineTo(b, h); g.lineTo(b, 0); g.closePath(); g.fill();
    g.strokeStyle = T.ink; g.lineWidth = 1.5; g.beginPath();
    if (fi) { g.moveTo(a, h); g.lineTo(a + fi, 0); }
    if (fo) { g.moveTo(b - fo, 0); g.lineTo(b, h); }
    g.stroke();
    g.fillStyle = rgba(T.viz, 0.78); g.fillRect(0, 0, a, h); g.fillRect(b, 0, w - b, h);

    const envelope = props.envelope?.();
    if (envelope?.length && b - a > 2) {
      g.strokeStyle = T.fgHi; g.globalAlpha = 0.75; g.lineWidth = 1.25; g.beginPath();
      envelope.forEach(([u, v], i) => {
        const x = a + u * (b - a), y = 4 + (1 - v) * (h - 8);
        if (i) g.lineTo(x, y); else g.moveTo(x, y);
      });
      g.stroke(); g.globalAlpha = 1;
    }
    g.fillStyle = T.fgHi;
    g.fillRect(Math.round(a), 0, 1, h); g.fillRect(Math.round(a), 0, 6, 6);
    g.fillRect(Math.round(b) - 1, 0, 1, h); g.fillRect(Math.round(b) - 6, h - 6, 6, 6);
  };

  const handleAt = (e: MouseEvent) => (e.target as HTMLElement).closest<HTMLElement>('[data-sample-handle]')?.dataset.sampleHandle as Handle | undefined;
  const down = (e: PointerEvent) => {
    if (e.button !== 0 || !props.source()) return;
    const handle = handleAt(e);
    if (!handle) { props.preview(); return; }
    e.preventDefault(); e.stopPropagation();
    const edit = props.drag(handle), sx = e.clientX, width = box.clientWidth;
    const gesture = new Gesture(t(HANDLE_LABELS[handle]));
    track(ev => {
      const next = edit((ev.clientX - sx) / width);
      if (next) { gesture.update([next.op]); say(next.status); }
    }, () => gesture.end(), 'ew-resize');
  };
  const position = (handle: Handle) => {
    const { start, end, fadeIn, fadeOut } = props.region();
    switch (handle) {
      case 'start': return start;
      case 'end': return end;
      case 'fadeIn': return start + fadeIn;
      case 'fadeOut': return end - fadeOut;
    }
  };

  return (
    <div ref={box} class="smpwave" title={t(props.title)} onPointerDown={down} onDblClick={e => props.reset?.(handleAt(e))}>
      <canvas ref={canvas} />
      <Show when={props.source()}>
        <For each={HANDLES}>{handle => <div data-sample-handle={handle} title={t(HANDLE_LABELS[handle])}
          style={{ left: position(handle) * 100 + '%' }} />}</For>
      </Show>
      <Show when={props.previewPosition}><div ref={line} class="smppreview" /></Show>
    </div>
  );
}

interface SampleKnobProps {
  id: string; label: TranslationKey; title?: TranslationKey; position: () => number; display: () => string;
  op: (position: number) => Op; wheelOp?: (position: number) => Op; zero?: number; defaultPosition: number;
  target?: () => AutoTarget; color?: () => string; menuTip?: TranslationKey;
}

function SampleKnob(props: SampleKnobProps) {
  let gesture: Gesture | null = null;
  const label = () => t(props.label);
  const reset = () => commit([props.op(props.defaultPosition)], label());
  return <Knob key={props.id} label={label()} title={t(props.title ?? 'd.smp.knob.tip')}
    t={props.position} text={props.display} zero={props.zero == null ? undefined : () => props.zero!}
    automated={() => !!props.target && isAutomated(props.target())}
    onMenu={props.target || props.menuTip ? (x, y) => {
      if (props.target) paramMenu(x, y, props.target(), reset, props.color?.());
      else if (props.menuTip) controlMenu(x, y, label(), reset, t(props.menuTip));
    } : undefined}
    onStart={() => { gesture = new Gesture(label()); }}
    onChange={v => { gesture?.update([props.op(v)]); say(label() + ' ' + props.display()); }}
    onEnd={() => { gesture?.end(); gesture = null; }}
    onReset={reset}
    onWheel={v => wheelEdit(props.id, label(), [(props.wheelOp ?? props.op)(v)])} />;
}

function FadeKnobs(props: {
  id: string; value: (fade: Fade) => number; limit: (fade: Fade) => number;
  op: (fade: Fade, ms: number) => Op; title: TranslationKey; menuTip?: TranslationKey;
}) {
  const scale = (fade: Fade) => Math.max(1, props.limit(fade));
  return <For each={['fadeIn', 'fadeOut'] as const}>{fade => (
    <SampleKnob id={props.id + fade} label={FADE_LABELS[fade]} title={props.title}
      position={() => Math.cbrt(clamp(props.value(fade) / scale(fade), 0, 1))}
      display={() => Math.round(props.value(fade) * 10) / 10 + ' ms'} defaultPosition={0}
      op={v => props.op(fade, Math.min(props.limit(fade), v ** 3 * scale(fade)))}
      menuTip={props.menuTip} />
  )}</For>;
}

const fitLabel = (b: number) => b < 4 ? t(b === 1 ? 'd.smp.fit.beat' : 'd.smp.fit.beats', { n: b }) : t(b === 4 ? 'd.smp.fit.bar' : 'd.smp.fit.bars', { n: b / 4 });
const fileFormat = (src: Source) => Math.round(src.sr / 100) / 10 + ' kHz · ' + (src.ch === 1 ? 'mono' : 'stereo');
const pitchText = (v: number, unit: string) => (v > 0 ? '+' : '') + v + ' ' + unit;

function ClipSample(props: { clip: () => Clip; source: () => Source }) {
  const c = props.clip, src = props.source;
  const id = 'clip' + c().id;
  const spb = () => 60 / S.project.bpm, stretch = () => c().stretch ?? 1;
  const durB = () => src().dur / spb() * stretch();
  const seconds = (b: number) => b * spb() / stretch();
  const insert = () => S.project.tracks[c().track]?.insert;
  const busy = () => c().id in S.stretching;
  const put = (patch: Partial<Clip>): Op => ({ op: 'putClips', clips: [{ ...plain(c()), ...patch }] });
  const preview = () => U.previewing ? void be().previewStop() : void be().previewClip(c().id).catch(e => say(String(e)));
  const previewPosition = () => {
    const f = tele.frame;
    if (!f.previewing || f.previewPos < 0) return null;
    const rendered = (c().pitch || (stretch() !== 1 && !c().resample)) && !busy();
    return clamp(f.previewPos / (src().dur * (rendered && !c().resample ? stretch() : 1)), 0, 1);
  };
  const drag = (handle: Handle) => {
    const old = plain(c()), beatSec = spb(), duration = durB();
    return (delta: number) => {
      const d = delta * duration, next = { ...old };
      if (handle === 'start') {
        next.offset = clamp(old.offset + d, Math.max(0, old.offset - old.start), old.offset + old.len - 1 / 16);
        next.start += next.offset - old.offset;
        next.len -= next.offset - old.offset;
      } else if (handle === 'end') {
        next.len = clamp(old.len + d, 1 / 16, Math.min(duration - old.offset, TOTAL - old.start));
      } else {
        const other = handle === 'fadeIn' ? old.fadeOut : old.fadeIn;
        next[handle] = clamp(old[handle] + d * beatSec * (handle === 'fadeIn' ? 1 : -1), 0, Math.max(0, old.len * beatSec - other));
      }
      const status = handle === 'fadeIn' || handle === 'fadeOut' ? t(FADE_LABELS[handle]) + ' ' + msText(next[handle] * 1000)
        : seconds(next.offset).toFixed(3) + ' – ' + seconds(next.offset + next.len).toFixed(3) + ' s';
      return { op: { op: 'putClips' as const, clips: [next] }, status };
    };
  };
  const semi = () => Math.trunc(Math.round((c().pitch ?? 0) * 100) / 100);
  const fine = () => Math.round(((c().pitch ?? 0) - semi()) * 100);
  const pitch = (s: number, f: number) => put({ pitch: Math.round(clamp(s + f / 100, -CLIP_PITCH, CLIP_PITCH) * 100) / 100 });
  const level = (key: 'gain' | 'pan') => {
    const gain = key === 'gain', lo = gain ? -24 : -1, hi = gain ? 12 : 1;
    const target = () => ({ kind: gain ? 'clipGain' as const : 'clipPan' as const, id: c().id });
    const v = () => autoDisplay(target()) ?? c()[key];
    return <SampleKnob id={id + key} label={gain ? 'd.smp.k.vol' : 'd.smp.k.pan'}
      position={() => (v() - lo) / (hi - lo)} display={() => gain ? dbText(v()) : panText(v())} zero={-lo / (hi - lo)}
      defaultPosition={-lo / (hi - lo)} op={x => put({ [key]: lo + x * (hi - lo) })} target={target} color={() => src().color} />;
  };
  const fit = (b: number) => {
    const n = fitClip(plain(c()), b); if (!n) return;
    commit([{ op: 'putClips', clips: [n] }], t('d.smp.fit.op'));
    say(t('d.smp.fit.say', { len: fitLabel(b), k: (n.stretch ?? 1).toFixed(2) }) + (n.resample ? '' : t('d.smp.fit.keep')));
  };

  return <EditorFrame name={src().name} description={t('d.smp.kind', { track: S.project.tracks[c().track]?.name ?? '' })}
    color={src().color} icon={<InstIcon id={guessIcon(src().name) ?? 'sample'} size={14} />}
    wave={{
      source: src, color: () => src().color, normalized: () => c().norm, reversed: () => c().rev,
      gain: () => dbg(c().gain), preview, previewPosition, title: 'd.smp.waveTip', drag,
      region: () => {
        const duration = durB();
        return { start: c().offset / duration, end: (c().offset + c().len) / duration,
          fadeIn: c().fadeIn / spb() / duration, fadeOut: c().fadeOut / spb() / duration };
      },
    }}
    fileInfo={seconds(c().offset).toFixed(2) + '–' + seconds(c().offset + c().len).toFixed(2) + ' / ' + src().dur.toFixed(2) + ' s · ' + fileFormat(src())}
    header={<>
      <InsertPicker value={insert() ?? 0} master ops={id => { const op = A.trackOp(c().track, { insert: id ?? 0 }); return op ? [op] : []; }}
        label={t('d.smp.routing')} wheelKey={id + 'route'} title={t('d.smp.mixer.tip', { track: S.project.tracks[c().track]?.name ?? '' })} />
      <button class="smallbtn" title={t('d.smp.pitchEd.tip')} onClick={() => { const cid = c().id; A.closeEditor(); A.openPitchEditor(cid); }}>
        {t('d.smp.pitchEd')}{c().tune ? ' •' : ''}
      </button>
      <PitchCorrectButton insert={() => insert() ?? null} />
    </>}
    toggle={key => {
      const k = key === 'reverse' ? 'rev' : 'norm';
      const on = !c()[k];
      commit([put({ [k]: on })], t(key === 'reverse' ? 'b.sm.reverse' : 'b.sm.norm'));
      say(t(key === 'reverse' ? (on ? 'd.smp.rev.on' : 'd.smp.rev.off') : (on ? 'd.smp.norm.on' : 'd.smp.norm.off')));
    }}
    pitch={<>
      <SampleKnob id={id + 'semi'} label="d.smp.k.pitch" title="d.smp.k.pitchTip" position={() => (semi() + CLIP_PITCH) / (2 * CLIP_PITCH)}
        display={() => pitchText(semi(), 'st')} defaultPosition={0.5} zero={0.5}
        op={v => pitch(Math.round((2 * v - 1) * CLIP_PITCH), fine())}
        wheelOp={v => {
          const delta = (2 * v - 1) * CLIP_PITCH - semi();
          return pitch(semi() + Math.sign(delta) * Math.max(1, Math.round(Math.abs(delta))), fine());
        }} menuTip="d.smp.noAutoPitch" />
      <SampleKnob id={id + 'fine'} label="d.smp.k.fine" title="d.smp.k.fineTip" position={() => (fine() + 100) / 200}
        display={() => pitchText(fine(), 'ct')} defaultPosition={0.5} zero={0.5}
        op={v => pitch(semi(), Math.round(v * 200 - 100))} menuTip="d.smp.noAutoPitch" />
    </>}
    level={<>{level('gain')}{level('pan')}</>}
    fades={<FadeKnobs id={id} value={k => c()[k] * 1000} limit={k => Math.max(0, c().len * spb() - c()[k === 'fadeIn' ? 'fadeOut' : 'fadeIn']) * 1000}
      op={(k, v) => put({ [k]: v / 1000 })} title="d.smp.knob.tip" menuTip="a.au.noFade" />}
    details={<>
      <div class="smprow">
        <div class="seg smpmode" role="radiogroup" aria-label={t('d.smp.method')} title={t('d.smp.methodTip')}>
          <button role="radio" aria-checked={!c().resample} classList={{ on: !c().resample }} onClick={() => A.setStretchMethod(c(), false)}>{t('a.pl.keepPitch')}</button>
          <button role="radio" aria-checked={!!c().resample} classList={{ on: !!c().resample }} onClick={() => A.setStretchMethod(c(), true)}>{t('a.pl.resample')}</button>
        </div>
        <span class="smpstat mono" classList={{ bad: !!S.stretching[c().id] }} title={S.stretching[c().id] || ''}>
          {busy() ? (S.stretching[c().id] ? t('d.smp.renderFail') : t('d.smp.rendering')) : stretch() !== 1 ? '×' + stretch().toFixed(2) : ''}
        </span>
      </div>
      <div class="smprow smpfit">
        <span class="tag9">{t('d.smp.fit')}</span>
        <div class="seg"><For each={FIT_BEATS}>{b => {
          const allowed = () => !!fitClip(plain(c()), b);
          return <button classList={{ on: Math.abs(c().len - b) < 1e-6 }} disabled={!allowed()}
            title={t(allowed() ? 'd.smp.fit.tip' : 'd.smp.fit.no', { len: fitLabel(b) })} onClick={() => fit(b)}>{fitLabel(b)}</button>;
        }}</For></div>
      </div>
    </>}
  />;
}

function ChannelSample(props: { channel: () => SampleChannel }) {
  const c = props.channel, id = 'channel' + c().id;
  const s = () => c().sampler ?? samplerDefault();
  const src = () => chanSource(c()), duration = () => src()?.dur ?? 0;
  const put = (patch: Partial<SamplerState>) => chanOp(c(), { sampler: { ...s(), ...patch } });
  const set = (patch: Partial<SamplerState>, label: TranslationKey, heard: string) => {
    commit([put(patch)], t(label)); say(c().name + ' · ' + heard);
  };
  const fadeKey = { fadeIn: 'fadeInMs', fadeOut: 'fadeOutMs' } as const;
  const fadeValue = (key: Fade) => s()[fadeKey[key]] ?? 0;
  const fadeLimit = () => Math.min(SAMPLER.fadeMaxMs, (s().end - s().start) * duration() * 1000);
  const region = (): WaveRegion => {
    const st = s(), len = st.end - st.start, ms = duration() * 1000;
    return { start: st.reverse ? 1 - st.end : st.start, end: st.reverse ? 1 - st.start : st.end,
      fadeIn: ms ? Math.min(len, fadeValue('fadeIn') / ms) : 0, fadeOut: ms ? Math.min(len, fadeValue('fadeOut') / ms) : 0 };
  };
  const drag = (handle: Handle) => {
    const initial = region(), reverse = s().reverse;
    return (delta: number) => {
      if (!src()) return;
      if (handle === 'fadeIn' || handle === 'fadeOut') {
        const f = initial[handle] + delta * (handle === 'fadeIn' ? 1 : -1);
        const ms = clamp(f * duration() * 1000, 0, fadeLimit());
        return { op: put({ [fadeKey[handle]]: ms }), status: t(FADE_LABELS[handle]) + ' · ' + msText(ms) };
      }
      const position = initial[handle] + delta;
      const key = reverse ? (handle === 'start' ? 'end' : 'start') : handle;
      const fraction = Math.round((reverse ? 1 - position : position) * 1000) / 1000;
      const value = key === 'start' ? clamp(fraction, 0, s().end - SAMPLER.minSpan) : clamp(fraction, s().start + SAMPLER.minSpan, 1);
      return { op: put({ [key]: value }), status: t(handle === 'start' ? 'b.sm.startSay' : 'b.sm.endSay', { pct: Math.round((reverse ? 1 - value : value) * 100) }) };
    };
  };
  const reset = (handle: Handle | undefined) => {
    if (handle === 'fadeIn' || handle === 'fadeOut') {
      if (fadeValue(handle)) commit([put({ [fadeKey[handle]]: 0 })], t(FADE_LABELS[handle]));
    } else if (s().start !== 0 || s().end !== 1) commit([put({ start: 0, end: 1 })], t('b.sm.region'));
  };
  const samplerKnob = (key: 'semi' | 'fine', unit: string) => {
    const { min, max } = SAMPLER[key];
    return <SampleKnob id={id + key} label={key === 'semi' ? 'd.smp.k.pitch' : 'd.smp.k.fine'}
      title={key === 'semi' ? 'b.sm.pitchTitle' : 'b.sm.fineTitle'} position={() => (s()[key] - min) / (max - min)}
      display={() => pitchText(Math.round(s()[key]), unit)} defaultPosition={0.5} zero={0.5}
      op={v => put({ [key]: Math.round(min + v * (max - min)) })} />;
  };

  return <EditorFrame name={c().name} description={t('b.cw.kSampler')} channelId={c().id}
    color={c().color} icon={<InstIcon id={channelIcon(c())} size={14} />}
    wave={{
      source: src, color: () => c().color, normalized: () => s().norm, reversed: () => s().reverse, region,
      preview: () => { if (src()) audition(c().id, c().root, 0.9, true); }, title: 'b.sm.waveTitle', drag, reset,
      envelope: () => envelopePoints(s(), duration() * (s().end - s().start) / 2 ** ((s().semi + s().fine / 100) / 12)),
    }}
    fileInfo={<Show when={src()}>{file => (file().dur * (s().end - s().start)).toFixed(2) + ' s · ' + fileFormat(file())}</Show>}
    fileAction={<button class="smallbtn" onClick={() => void replaceWithFile(c())} title={t('b.cw.changeSampleTitle')}>{t('b.cw.changeSampleBtn')}</button>}
    header={<>
      <InsertPicker value={c().insert} nullLabel={t('b.cw.likeClipTrack')} ops={insert => [chanOp(c(), { insert })]}
        label={t('d.smp.routing')} wheelKey={id + 'route'} title={t('b.cw.mixerTip')} />
      <PitchCorrectButton insert={() => chanHeardInsert(c())} />
    </>}
    toggle={key => {
      const on = !s()[key];
      set({ [key]: on }, key === 'reverse' ? 'b.sm.reverse' : 'b.sm.norm',
        t(key === 'reverse' ? (on ? 'b.sm.reverseOn' : 'b.sm.reverseOff') : (on ? 'b.sm.normOn' : 'b.sm.normOff')));
    }}
    pitch={<>{samplerKnob('semi', 'st')}{samplerKnob('fine', 'ct')}</>}
    level={<ChannelLevelKnobs channel={c()} sample />}
    fades={<FadeKnobs id={id} value={fadeValue} limit={fadeLimit} op={(k, v) => put({ [fadeKey[k]]: v })} title="b.sm.fadeTip" />}
    details={<>
      <div class="smprow">
        <div class="seg smpmode" role="radiogroup" aria-label={t('b.sm.mode')}>
          <button role="radio" aria-checked={!s().gate} classList={{ on: !s().gate }} title={t('b.sm.oneshotTitle')}
            onClick={() => s().gate && set({ gate: false }, 'b.sm.mode', t('b.sm.oneshot'))}>{t('b.sm.oneshot')}</button>
          <button role="radio" aria-checked={s().gate} classList={{ on: s().gate }} title={t('b.sm.gateTitle')}
            onClick={() => !s().gate && set({ gate: true }, 'b.sm.mode', t('b.sm.gate'))}>{t('b.sm.gate')}</button>
        </div>
        <label class="smpcut" title={t('b.sm.cutTitle')}>
          <span>{t('b.sm.cut')}</span>
          <select class="sel" value={String(s().cut)} onChange={e => {
            const cut = +e.currentTarget.value;
            set({ cut }, 'b.sm.cut', cut ? t('b.sm.cutSay', { n: cut }) : t('b.sm.cutNoneSay'));
          }}>
            <option value="0">{t('b.sm.cutNone')}</option>
            <For each={Array.from({ length: SAMPLER.cutGroups }, (_, i) => i + 1)}>{n => <option value={String(n)}>{n}</option>}</For>
          </select>
        </label>
      </div>
      <div class="smpknobs">
        <div class="smpgrp">
          <SampleKnob id={id + 'attack'} label="b.sm.attack" title="b.sm.attackTitle" position={() => attackToT(s().attack)}
            display={() => msText(s().attack)} defaultPosition={0} op={v => put({ attack: attackFromT(v) })} />
          <SampleKnob id={id + 'decay'} label="b.sm.decay" title="b.sm.decayTitle" position={() => decayToT(s().decay)}
            display={() => msText(s().decay)} defaultPosition={0} op={v => put({ decay: decayFromT(v) })} />
          <SampleKnob id={id + 'sustain'} label="b.sm.sustain" title="b.sm.sustainTitle" position={() => s().sustain}
            display={() => Math.round(s().sustain * 100) + ' %'} defaultPosition={1} op={v => put({ sustain: Math.round(v * 100) / 100 })} />
          <div classList={{ smpdim: !s().gate }}>
            <SampleKnob id={id + 'release'} label="b.sm.release" title={s().gate ? 'b.sm.releaseTitle' : 'b.sm.releaseOff'}
              position={() => releaseToT(s().release)} display={() => msText(s().release)}
              defaultPosition={releaseToT(SAMPLER.release.def)} op={v => put({ release: releaseFromT(v) })} />
          </div>
        </div>
        <div class="smpgrp"><ChannelRootKnob channel={c()} /></div>
      </div>
      <PianoKeys ch={c().id} lo={36} hi={96} mark={c().root} />
    </>}
  />;
}
