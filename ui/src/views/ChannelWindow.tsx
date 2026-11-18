// Channel settings window (FL "Channel settings"): click a Channel Rack name to see what makes
// the sound — the synth, Synkopa Keys or a hosted VST3 instrument (its editor and parameters) — plus level,
// pan, root key, mixer routing and a keyboard to play it.
import { createEffect, Show } from 'solid-js';
import { VOL_UNITY, type AutoTarget, type Channel, type Id } from '../api/types';
import { clamp, panText, volText } from '../lib/format';
import { t, type TranslationKey } from '../lib/i18n';
import { noteName } from '../lib/pitch';
import { autoDisplay, controlMenu, isAutomated, paramMenu } from '../state/automation';
import { commit, Gesture, wheelEdit } from '../state/edit';
import { audition, chanById, chanOp } from '../state/pattern';
import { outerZ, setWin, win, type Win } from '../state/winOrder';
import { docked, say, setU, tucked, U } from '../state/ui';
import { showInsert } from '../state/mixView';
import { channelIcon, InstIcon } from '../ui/instIcons';
import { Knob } from '../ui/Knob';
import { PianoKeys } from '../ui/PianoKeys';
import { InsertPicker } from '../ui/InsertPicker';
import { moveWindow, PluginBody } from './FxWindow';
import { KeysPanel } from './KeysPanel';
import { SynthPanel } from './SynthPanel';
import { S } from '../state/store';
import { closeEditor, openSampleEditor } from '../state/actions';
import { togglePlugin } from '../state/pluginWindows';

/** The insert a channel is heard on: its own, else the track of the first playlist clip of a pattern with its
 *  notes (rt.rs `ChanRt::heard`); null when it plays nowhere yet. */
export function chanHeardInsert(c: Channel): Id | null {
  if (c.insert != null) return c.insert;
  const pats = new Set(S.project.patterns.filter(p => p.notes.some(n => n.ch === c.id)).map(p => p.id));
  const first = S.project.clips.filter(x => x.kind === 'pat' && pats.has(x.ref)).sort((a, b) => a.start - b.start)[0];
  return first ? S.project.tracks[first.track]?.insert ?? null : null;
}

/** The channel window over the plugin / pattern windows (it is used last). */
function chanFront() {
  const w = win(); if (!w || (w.z === U.zTop && !tucked(w.z))) return;
  const z = U.zTop + 1;
  setU('zTop', z); setWin({ ...w, z });
}
export function chanWin() {
  const ed = U.editor;
  return ed?.kind === 'channel' ? { id: ed.id, x: ed.x, y: ed.y, w: ed.w } : win();
}

/**
 * Open the channel's window, or close it when it is already showing that channel. A plugin instrument shows its
 * own window instead (FL), and a second click closes that; Synkopa's window comes from its header's gear
 * (`settings`), or when the plugin has no window of its own.
 */
export function toggleChannelWindow(id: Id, settings = false) {
  const w = win();
  const kind = chanById(id)?.kind;
  if (!kind) return;
  if (kind === 'plugin' && !settings) { togglePlugin(id, () => toggleChannelWindow(id, true)); return; }
  if (settings && chanWin()?.id === id) return;
  if (chanWin()?.id === id) { closeChannelWindow(); return; }
  const width = kind === 'synth' ? 900 : kind === 'keys' ? 700 : kind === 'plugin' ? 560 : Math.min(620, window.innerWidth - 20);
  // beside the Channel Rack: the floating window, or the docked panel's place on screen
  const dk = docked('rack') ? document.querySelector('.rackwin')?.getBoundingClientRect() : undefined;
  const r = dk ? { x: dk.left, y: dk.top, w: dk.width } : U.rackWin, W = window.innerWidth;
  const center = Math.max(20, (W - width) / 2);
  const x = r && r.x + r.w + 8 + width <= W - 10 ? r.x + r.w + 8 : r && r.x - 8 - width >= 10 ? r.x - 8 - width : center;
  const y = r && x !== center ? r.y : kind === 'synth' ? 70 : 110;
  if (kind === 'sample') {
    setWin(null);
    openSampleEditor({ kind: 'channel', id }, w ?? { x, y });
    return;
  }
  if (U.editor?.kind === 'channel') closeEditor();
  const z = U.zTop + 1;
  setU({ zTop: z });
  setWin({ id, x: w?.x ?? x, y: w?.y ?? y, w: width, z });
}
export function closeChannelWindow() {
  if (U.editor?.kind === 'channel') closeEditor();
  setWin(null);
}

export function ChannelWindow() {
  createEffect(() => {
    const w = win();
    if (!w) return;
    const c = chanById(w.id);
    if (c?.kind === 'sample') openSampleEditor({ kind: 'channel', id: c.id }, w);
    if (!c || c.kind === 'sample') setWin(null);
  });
  return (
    <Show when={win()}>
      {w => (
        <Show when={chanById(w().id)}>
          {c => <Body w={w()} c={c()} />}
        </Show>
      )}
    </Show>
  );
}

function Body(props: { w: Win; c: Channel }) {
  const c = () => chanById(props.c.id) ?? props.c;
  const live = () => chanById(props.c.id)!;
  const setPos = (x: number, y: number) => setWin(v => v && { ...v, x, y });
  const plugin = () => { const x = c(); return x.kind === 'plugin' ? x.plugin : undefined; };
  return (
    <div class="win chanwin" data-channel-window={c().id} style={{ left: props.w.x + 'px', top: props.w.y + 'px', width: props.w.w + 'px', 'z-index': outerZ(props.w.z) }}
      on:pointerdown={{ capture: true, handleEvent: chanFront }}>
      <div class="winhd" onPointerDown={e => moveWindow(e, () => props.w, setPos)} title={t('b.cw.headTip')}
        onDblClick={e => { if (!(e.target as HTMLElement).closest('button, select, input')) showInsert(chanHeardInsert(c()) ?? 0); }}>
        <span class="chicon" style={{ color: c().color }}><InstIcon id={channelIcon(c(), (id: number) => S.project.sources.find(s => s.id === id))} size={14} /></span>
        <span class="title">{c().name}</span>
        <span class="sub">{t(c().kind === 'synth' ? 'b.cw.kSynth' : c().kind === 'plugin' ? 'b.cw.kPlugin' : 'b.cw.kKeys')}</span>
        <div style={{ flex: 1 }} />
        <InsertPicker value={c().insert} nullLabel={t('b.cw.likeClipTrack')} ops={id => [chanOp(live(), { insert: id })]} label={t('b.pe.mixCh')} free={{ name: c().name, color: c().color }}
          wheelKey={'cwroute' + props.c.id} title={t('b.cw.mixerTip')} />
        <button class="iconbtn" title={t('b.cw.previewTitle')} onClick={() => audition(c().id, undefined, 0.9, true)}>▶</button>
        <button class="closebtn" title={t('b.cw.close')} onClick={closeChannelWindow}>×</button>
      </div>
      <Show when={c().kind === 'synth'}><SynthPanel c={c()} /></Show>
      <Show when={c().kind === 'keys' && c()}>{k => <KeysPanel c={k() as Extract<Channel, { kind: 'keys' }>} />}</Show>
      {/* keyed by the plugin class: a replaced instrument gets a fresh list, an edit elsewhere keeps its editor open */}
      <Show when={plugin()?.cid} keyed>
        {_cid => <PluginBody id={c().id} plugin={plugin()} instrument />}
      </Show>
      <div class="chknobs"><ChannelLevelKnobs channel={c()} /><Show when={c().kind === 'synth'}><ChannelRootKnob channel={c()} /></Show></div>
      <Show when={c().kind !== 'synth'}>
        <PianoKeys player={{ ch: c().id }} lo={c().kind === 'keys' ? 21 : 36} hi={c().kind === 'keys' ? 108 : 96} root={c().root}
          onRoot={key => { commit([chanOp(live(), { root: key })], t('b.cw.rootCh')); say(t('b.pk.rootSay', { name: c().name, note: noteName(key) })); }} />
      </Show>
    </div>
  );
}

function ChannelKnob(props: { channel: Channel; lab: TranslationKey; undoLab: TranslationKey; position: () => number; text: () => string; patch: (v: number) => Partial<Channel>; def: number; zero?: number; target?: () => AutoTarget }) {
  const { lab, undoLab, position, text, patch, def, zero, target } = props;
  let g: Gesture | null = null;
  const reset = () => commit([chanOp(props.channel, patch(def))], t(undoLab));
  return (
    <Knob key={'cw' + lab + props.channel.id} label={t(lab)} title={t('b.cw.knobTitle')} t={position} text={text}
      automated={() => !!target && isAutomated(target())}
      onMenu={(x, y) => target ? paramMenu(x, y, target(), reset, props.channel.color) : controlMenu(x, y, props.channel.name + ' · ' + t(lab), reset, t('a.au.noRoot'))}
      zero={zero != null ? () => zero : undefined}
      onStart={() => { g = new Gesture(t(undoLab)); }}
      onChange={v => { g?.update([chanOp(props.channel, patch(v))]); say(props.channel.name + ' · ' + t(lab).toLowerCase() + ' ' + text()); }}
      onEnd={() => { g?.end(); g = null; }}
      onReset={reset}
      onWheel={v => wheelEdit('cw' + lab, t(undoLab), [chanOp(props.channel, patch(v))])} />
  );
}

export function ChannelLevelKnobs(props: { channel: Channel; sample?: boolean }) {
  const volT = () => ({ kind: 'chanVol' as const, id: props.channel.id });
  const panT = () => ({ kind: 'chanPan' as const, id: props.channel.id });
  const vol = () => autoDisplay(volT()) ?? props.channel.vol;
  const pan = () => autoDisplay(panT()) ?? props.channel.pan;
  return <>
    <ChannelKnob channel={props.channel} lab={props.sample ? 'd.smp.k.vol' : 'b.cw.vol'} undoLab="b.cw.volCh" position={vol} text={() => volText(vol())}
      patch={t => ({ vol: Math.round(clamp(t, 0, 1) * 10000) / 10000 })} def={VOL_UNITY} target={volT} />
    <ChannelKnob channel={props.channel} lab={props.sample ? 'd.smp.k.pan' : 'b.cw.pan'} undoLab="b.cw.panCh" position={() => (pan() + 1) / 2} text={() => panText(pan())}
      patch={t => ({ pan: Math.round(clamp(t * 2 - 1, -1, 1) * 100) / 100 })} def={0.5} zero={0.5} target={panT} />
  </>;
}

export function ChannelRootKnob(props: { channel: Channel }) {
  const lo = 24, hi = 96;
  return <ChannelKnob channel={props.channel} lab="b.cw.root" undoLab="b.cw.rootCh" position={() => (props.channel.root - lo) / (hi - lo)}
    text={() => noteName(props.channel.root)} patch={t => ({ root: Math.round(lo + clamp(t, 0, 1) * (hi - lo)) })} def={(60 - lo) / (hi - lo)} />;
}
