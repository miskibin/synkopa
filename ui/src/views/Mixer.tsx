import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { PLUGIN_FORMAT_LABEL, unsafeReason, VOL_UNITY, type Fx, type Id, type Insert, type Op } from '../api/types';
import { clamp, panText, volText } from '../lib/format';
import { stripLabel } from '../lib/mixer';
import { t as tr, tName } from '../lib/i18n';
import { masterClip, masterClipTip, resetMasterClip } from '../state/masterClip';
import { channels, routeSelectedChannels, selectedChannels } from '../state/pattern';
import { hideMixer, mixFront, revealed, toggleMixView } from '../state/mixView';
import { openDockMenu, paneDock, paneHeadDown } from '../state/docking';
import { findPane } from '../lib/dock';
import { PanelFrame, WinControls } from './PanelFrame';
import { presetInfo } from '../lib/presets';
import * as A from '../state/actions';
import { MuteSolo } from '../ui/MuteSolo';
import { commit, Gesture, wheelEdit } from '../state/edit';
import { autoDisplay, isAutomated, paramMenu } from '../state/automation';
import { MiniKnob } from '../ui/Knob';
import { heardInserts, insName, S } from '../state/store';
import { tele } from '../state/telemetry';
import { openMenu, openRename, say, setU, U, type MenuItem } from '../state/ui';
import { dblCheck, dragCursor, fineMul, track } from '../ui/drag';
import { addFrameTask } from '../ui/frameLoop';
import { FXMETA, FxTile, Icon, ICONS } from '../ui/icons';
import { guessIcon, InstIcon, trackIcon } from '../ui/instIcons';
import { openFxPicker } from './FxPicker';
import { openFxChains, saveFxChain } from './FxChains';
import { useSize } from './patternShared';
import { canSend, sendsOf, toMaster } from '../lib/routing';
import { pct, removeSend, routeSummary, sendBlock, sendGainOp, sendTo, setToMaster } from '../state/routing';

const L30 = 51.84, C30 = 69.12;

// meter masks per insert, updated from the frame loop
const meterEls = new Map<Id, [HTMLDivElement, HTMLDivElement]>();
const hold = new Map<Id, [number, number]>();
function meterTask() {
  const ins = tele.frame.ins;
  for (const [id, els] of meterEls) {
    const e = ins.get(id), h = hold.get(id) ?? [0, 0];
    const l = Math.max(e?.l ?? 0, h[0] * 0.9), r = Math.max(e?.r ?? 0, h[1] * 0.9);
    if (l === h[0] && r === h[1] && hold.has(id)) continue;
    hold.set(id, [l, r]);
    const f = (v: number) => { const db = v > 1e-5 ? 20 * Math.log10(v) : -90; return 1 - clamp((db + 60) / 66, 0, 1); };
    els[0].style.transform = `scaleY(${f(l)})`;
    els[1].style.transform = `scaleY(${f(r)})`;
  }
}

export function openFxMenu(x: number, y: number, insId: Id) {
  openFxPicker(insId, x, y);
}

/** Save / load the insert's whole effect chain as a preset in the user's library. */
function chainItems(x: number, y: number, m: Insert): MenuItem[] {
  return [
    { t: 'item', label: tr('e.fxc.mnSave'), disabled: !m.fx.length, title: m.fx.length ? undefined : tr('e.fxc.mnSaveNone'), act: () => saveFxChain(m.id, x, y) },
    { t: 'item', label: tr('e.fxc.mnLoad'), act: () => openFxChains(m.id, x, y) },
  ];
}

function openChainMenu(x: number, y: number, m: Insert) {
  openMenu(x, y, [{ t: 'title', label: tr('e.fxc.mnTitle', { ins: insName(m) }) }, ...chainItems(x, y, m)], 110);
}

/** Route-to-insert items (FL's "link selected channels"): disabled, saying why, until channels are selected in the Channel Rack. */
function routeItems(id: Id): MenuItem[] {
  const n = selectedChannels().length, master = id === 0;
  const none = n ? undefined : tr('c.mv.noSelTip');
  return [
    { t: 'item', label: n > 1 ? tr('c.mv.routeN', { n }) : tr('c.mv.route'), kbd: 'Ctrl+L', disabled: !n, title: none, act: () => routeSelectedChannels(id, false) },
    { t: 'item', label: tr('c.mv.spread'), kbd: 'Shift+Ctrl+L', disabled: !n || master, title: none ?? (master ? tr('c.mv.spreadMaster') : undefined), act: () => routeSelectedChannels(id, true) },
  ];
}

function openStripMenu(x: number, y: number, id: Id) {
  const m = S.project.inserts.find(v => v.id === id); if (!m) return;
  openMenu(x, y, [
    { t: 'title', label: insName(m) },
    { t: 'item', label: tr('c.mix.rename'), act: () => openRename(x, y, m.name, v => A.setInsert(id, { name: v }, tr('c.mix.undoName'))) },
    { t: 'swatches', current: m.color, pick: col => A.setInsert(id, { color: col }, tr('c.mix.undoColor')) },
    { t: 'sep' },
    { t: 'item', label: tr('c.mix.addFx'), act: () => openFxMenu(x, y, id) },
    ...chainItems(x, y, m),
    { t: 'item', label: tr('c.mix.resetVolPan'), act: () => A.setInsert(id, { vol: VOL_UNITY, pan: 0 }, tr('c.mix.undoReset')) },
    { t: 'sep' },
    ...routeItems(id),
    ...(id === 0 ? [] : sendItems(m)),
    ...(id === 0 ? [] : [{ t: 'sep' }, { t: 'item', label: tr('c.mix.removeInsert'), danger: true, act: () => A.removeInsert(id) }] as MenuItem[]),
  ], 330);
}

/** "Send to ▸" (every other insert: checked = sends there, click = add / remove; greyed when it
 *  would loop, with why) and "Output to Master" (FL's "route to this track only" when off). */
function sendItems(m: Insert): MenuItem[] {
  const sends = sendsOf(m);
  const targets = S.project.inserts.filter(x => x.id !== 0 && x.id !== m.id);
  return [
    { t: 'sub', label: tr('c.rt.sendTo'), items: targets.length ? targets.map(x => {
      const s = sends.find(v => v.to === x.id), why = s ? null : sendBlock(m.id, x.id);
      return { t: 'item' as const, label: insName(x), checked: !!s, kbd: s ? pct(s.gain) : undefined, disabled: !!why, title: why ?? undefined,
        act: () => (s ? removeSend(m.id, x.id) : sendTo(m.id, x.id)) };
    }) : [{ t: 'item', label: tr('c.mix.addInsert'), act: A.addInsert }] },
    { t: 'item', label: tr('c.rt.toMaster'), checked: toMaster(m), disabled: toMaster(m) && !sends.length, title: tr('c.rt.toMasterTip'), act: () => setToMaster(m.id, !toMaster(m)) },
  ];
}

/** One insert's strip. `full`: in the Mix view (with the level readout and what plays through it); docked it stays lean. */
/** Height (px) of a strip's meters and fader: the strip keeps it however tall the mixer is. */
const FADER_H = 140;
/** The fader's dB marks as [dB, fader position 0..1]; the gain law is 2t³ (`gainLin`), so 0 dB sits at 0.7937. */
const FADER_MARKS: [number, number][] = [6, 0, -6, -12, -24, -48].map(db => [db, Math.cbrt(Math.pow(10, db / 20) / 2)]);
/** Effect rows the maximized mixer always has room for (and the "+ effect" row after them). */
const FX_ROWS_FULL = 5;
/** One effect row in a strip, its 2 px gap included. */
const FX_ROW = 20;
/** How far an empty effect area pulls the strip up (`.strip .sfx:empty` in styles.css). */
const SFX_EMPTY_PULL = 6;

/**
 * The insert's effects as rows, in the room a taller mixer leaves above the knobs (the strip below
 * keeps its size): the effect's icon and name (click = open it), a dot (on / bypassed). As many as
 * fit; the last row then says how many more there are (click = the chain beside the strips), or
 * offers to add one when there is room.
 */
function StripFx(props: { m: Insert; full: boolean }) {
  let el!: HTMLDivElement;
  const size = useSize(() => el);
  // `.sfx:empty` pulls the strip 6 px up, which makes the empty box 6 px taller: measured as is,
  // a mixer at just the wrong height fits a row, shows it, loses the 6 px, hides it… every frame
  // (#160). The room is measured without that margin, so both states agree.
  const room = () => Math.floor((size().h - (el?.firstElementChild ? 0 : SFX_EMPTY_PULL) + 2) / FX_ROW);
  const fit = () => { const r = room(), n = props.m.fx.length; return n + 1 <= r ? n : Math.max(0, r - 1); };
  const stop = (e: Event) => e.stopPropagation();
  return (
    <div ref={el} class="sfx" style={props.full ? { flex: `0 0 ${(FX_ROWS_FULL + 1) * FX_ROW}px` } : undefined}>
      <Show when={room() > 0}>
        <For each={props.m.fx.slice(0, fit())}>{f => (
          <div class="sfxr" classList={{ off: f.bypass, on: U.fxSel === f.id }} title={A.fxName(f) + ' · ' + tr('c.mix.fxRowTip')}
            onClick={() => { setU('fxSel', f.id); A.openPlug(f.id); }}>
            <FxTile type={f.type} size={14} dim={f.bypass} />
            <span class="ell">{A.fxName(f)}</span>
            <button class="sfxled" title={f.bypass ? tr('c.fx.bypassOn') : tr('c.fx.bypassOff')} aria-label={f.bypass ? tr('c.fx.bypassOn') : tr('c.fx.bypassOff')}
              onPointerDown={stop} onClick={e => { e.stopPropagation(); A.toggleBypass(f.id); }} />
          </div>
        )}</For>
        <Show when={props.m.fx.length > fit()} fallback={
          <div class="sfxr add" title={tr('c.mix.addFxTip')} onClick={e => openFxMenu(e.clientX, e.clientY, props.m.id)}>+ {tr('c.mix.addFxShort')}</div>
        }>
          <div class="sfxr more" title={tr('c.mix.fxMoreTip')} onClick={() => A.selectInsert(props.m.id)}>{tr('c.mix.fxMore', { n: props.m.fx.length - fit() })}</div>
        </Show>
      </Show>
    </div>
  );
}

function Strip(props: { m: Insert; idx: number; full: boolean }) {
  const m = () => props.m;
  let fl!: HTMLDivElement, fr!: HTMLDivElement;
  onMount(() => { meterEls.set(props.m.id, [fl, fr]); hold.delete(props.m.id); });
  onCleanup(() => meterEls.delete(props.m.id));
  // solo follows the routing: a soloed bus keeps what feeds it (lib/routing.ts)
  const audible = () => heardInserts().has(m().id);
  // what plays into this insert: tracks, channels, samples routed to it ('' when nothing is)
  const sources = () => m().id === 0 ? '' : [...new Set([...S.project.tracks.filter(t => t.insert === m().id).map(t => t.name), ...channels().filter(c => c.insert === m().id).map(c => c.name), ...samplesOn(m().id)])].join(', ');
  const route = () => m().id === 0 ? 'OUT' : sources() || '—';
  const panT = () => ({ kind: 'insertPan' as const, id: m().id }), volT = () => ({ kind: 'insertVol' as const, id: m().id });
  const shownPan = () => autoDisplay(panT()) ?? m().pan, shownVol = () => autoDisplay(volT()) ?? m().vol;
  const pan = () => { const t = (shownPan() + 1) / 2; return { dash: Math.abs(t - 0.5) * L30 + 'px ' + C30 + 'px', off: -Math.min(t, 0.5) * L30 + 'px', angle: -135 + t * 270 }; };
  const put = (patch: Partial<Insert>) => A.insertOp(m().id, patch)!;
  // stereo width: 0 = mono, 100 % = as is, 200 % = extra wide (arc grows from the 100 % centre)
  const wid = () => m().width ?? 1;
  const widArc = () => { const t = wid() / 2; return { dash: Math.abs(t - 0.5) * L30 + 'px ' + C30 + 'px', off: -Math.min(t, 0.5) * L30 + 'px', angle: -135 + t * 270 }; };
  const widText = (w: number) => (w <= 0.005 ? 'mono' : Math.round(w * 100) + '%');
  const widDown = (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation(); A.selectInsert(m().id);
    if (e.altKey || dblCheck('wid' + m().id)) return A.setInsert(m().id, { width: 1 }, tr('c.mix.width'));
    const g = new Gesture(tr('c.mix.width')); let v = wid(), ly = e.clientY;
    track(ev => { v = clamp(v + (ly - ev.clientY) / 70 * fineMul(ev) * 2, 0, 2); if (Math.abs(v - 1) < 0.03) v = 1; ly = ev.clientY; g.update([put({ width: Math.round(v * 100) / 100 })]); say(tr('c.mix.sayWidth', { v: widText(v) })); }, () => g.end(), 'ns-resize');
  };
  const widWheel = (e: WheelEvent) => {
    const d = e.deltaY || e.deltaX; if (!d) return; e.preventDefault(); e.stopPropagation();
    const v = clamp(Math.round((wid() + (d < 0 ? 1 : -1) * 0.05 * (e.shiftKey ? 0.2 : 1)) * 100) / 100, 0, 2);
    wheelEdit('wid' + m().id, tr('c.mix.width'), [put({ width: v })]); say(tName(m().name) + ' · ' + tr('c.mix.widthLc') + ' ' + widText(v));
  };
  const select = (e: PointerEvent) => {
    if (e.button === 2) { e.preventDefault(); return openStripMenu(e.clientX, e.clientY, m().id); }
    // FL: Ctrl+Shift+click selects several mixer tracks; their faders and mutes then move together
    if (e.button === 0 && (e.ctrlKey || e.metaKey) && e.shiftKey) return A.toggleInsertSelected(m().id);
    A.selectInsert(m().id);
  };
  const panDown = (e: PointerEvent) => {
    if (e.button === 2) { e.preventDefault(); e.stopPropagation(); return paramMenu(e.clientX, e.clientY, panT(), () => A.setInsert(m().id, { pan: 0 }, tr('c.mix.pan')), m().color); }
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation(); A.selectInsert(m().id);
    if (e.altKey || dblCheck('pan' + m().id)) return A.setInsert(m().id, { pan: 0 }, tr('c.mix.pan'));
    const g = new Gesture(tr('c.mix.pan')); let v = m().pan, ly = e.clientY;
    track(ev => { v = clamp(v + (ly - ev.clientY) / 70 * fineMul(ev), -1, 1); ly = ev.clientY; g.update([put({ pan: v })]); say('Pan ' + panText(v)); }, () => g.end(), 'ns-resize');
  };
  const faderDown = (e: PointerEvent) => {
    if (e.button === 2) { e.preventDefault(); e.stopPropagation(); return paramMenu(e.clientX, e.clientY, volT(), () => A.setInsert(m().id, { vol: VOL_UNITY }, tr('c.mix.volume')), m().color); }
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation(); A.selectInsert(m().id);
    if (e.altKey || dblCheck('vol' + m().id)) return A.setInsert(m().id, { vol: VOL_UNITY }, tr('c.mix.volume'));
    const h = Math.max(40, (e.currentTarget as HTMLElement).getBoundingClientRect().height - 14);
    const g = new Gesture(tr('c.mix.volume')); let v = m().vol, ly = e.clientY;
    // the other selected strips move by the same fader travel, each from where it was
    const v0 = v, group = A.selectedInserts().includes(m().id) ? A.selectedInserts().filter(id => id !== m().id).map(id => ({ id, vol: S.project.inserts.find(x => x.id === id)?.vol ?? VOL_UNITY })) : [];
    track(ev => {
      v = clamp(v + (ly - ev.clientY) / h * fineMul(ev), 0, 1); ly = ev.clientY;
      const linked = group.map(o => A.insertOp(o.id, { vol: clamp(o.vol + (v - v0), 0, 1) })).filter((o): o is Op => !!o);
      g.update([put({ vol: v }), ...linked]); say(tr('c.mix.volume') + ' ' + volText(v) + (group.length ? ' · +' + group.length : ''));
    }, () => g.end(), 'ns-resize');
  };
  const wheel = (k: 'pan' | 'vol') => (e: WheelEvent) => {
    const d = e.deltaY || e.deltaX; if (!d) return;
    e.preventDefault(); e.stopPropagation();
    const n = d < 0 ? 1 : -1, f = e.shiftKey ? 0.2 : 1;
    if (k === 'pan') { const v = clamp(m().pan + n * 0.04 * f, -1, 1); wheelEdit('pan' + m().id, tr('c.mix.pan'), [put({ pan: v })]); say(tName(m().name) + ' · pan ' + panText(v)); }
    else { const v = clamp(m().vol + n * 0.015 * f, 0, 1); wheelEdit('vol' + m().id, tr('c.mix.volume'), [put({ vol: v })]); say(tName(m().name) + ' · ' + volText(v)); }
  };
  const stop = (e: Event) => e.stopPropagation();
  const icon = () => { if (m().id === 0) return null; const t = S.project.tracks.find(x => x.insert === m().id); return guessIcon(m().name) ?? (t ? trackIcon(t) : null); };
  const label = () => stripLabel(m().name, props.idx);
  // the instrument icon gives way when the name would be cut off (it is decoration; the name is the information)
  let nmEl: HTMLSpanElement | undefined, nmText: HTMLSpanElement | undefined;
  const [tight, setTight] = createSignal(false);
  const measure = () => { if (nmEl && nmText) setTight(nmText.getBoundingClientRect().width + ICON_W > nmEl.clientWidth); };
  createEffect(() => { label(); icon(); U.mixView; m().fx.length; requestAnimationFrame(measure); });
  onMount(() => { void document.fonts?.ready.then(measure); });
  const rename = (e: MouseEvent) => openRename(e.clientX, e.clientY, m().name, v => A.setInsert(m().id, { name: v }, tr('c.mix.undoName')));
  // a send being dragged over this strip: would it be taken?
  const dropHere = () => { const d = routeDrag(); return d && d.over === m().id && d.from !== m().id ? (canSend(S.project, d.from, m().id) ? 'ok' : 'no') : null; };
  // the selected insert sends here (its targets are marked while its cables are lit)
  const sendsHere = () => props.full && !!S.project.inserts.find(x => x.id === U.mixSel)?.sends?.some(s => s.to === m().id);
  const portDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault(); e.stopPropagation(); A.selectInsert(m().id);
    const from = m().id;
    const at = (ev: PointerEvent) => {
      const r = laneEl?.getBoundingClientRect(); if (!r) return null;
      const x = ev.clientX - r.left, y = clamp(ev.clientY - r.top, -24, LANE_H - 4);
      const over = S.project.inserts[Math.floor(x / stripW())]?.id ?? null;
      return { from, x, y, over: over === from ? null : over };
    };
    setRouteDrag(at(e)); say(tr('c.rt.dragHint'));
    track(ev => setRouteDrag(at(ev)), ev => {
      const d = at(ev); setRouteDrag(null);
      if (d?.over != null && ev.type === 'pointerup') sendTo(from, d.over);
    }, 'copy');
  };
  return (
    <div class="strip" data-ins={m().id} style={{ '--c': m().color }} onPointerDown={select}
      classList={{ on: U.mixSel === m().id, multi: U.mixSel !== m().id && A.selectedInserts().includes(m().id), dimmed: !audible(), 'rt-ok': dropHere() === 'ok', 'rt-no': dropHere() === 'no', 'rt-to': sendsHere() }}>
      <div class="bar" style={{ background: m().color }} />
      {/* one row: the insert's number and name (a default "Insert 7" is just its number), the effect count on the right */}
      <div class="nmrow" title={tName(label().full) + ' — ' + tr('c.mix.nameTip')} onDblClick={rename}>
        <Show when={label().num}><span class="num" classList={{ only: !label().name && (props.full || !sources()) }}>{label().num}</span></Show>
        {/* docked, an insert without its own name: what is routed to it, quiet, after the number */}
        <Show when={!props.full && !label().name && sources()}>
          <span class="nm src" title={sources()}>{sources()}</span>
        </Show>
        <Show when={label().name}>
          <span ref={nmEl} class="nm" style={{ color: audible() ? 'var(--fg-hi)' : 'var(--fg4)' }}>
            <Show when={!tight() && icon()}>{id => <span class="micon" style={{ color: m().color }}><InstIcon id={id()} size={11} /></span>}</Show><span ref={nmText}>{tName(label().name)}</span></span>
        </Show>
        <Show when={m().fx.length}>
          {/* docked: a dot and the count; the wider Mix view strips spell out "3 FX" */}
          <button class="fxn" classList={{ off: m().fx.every(f => f.bypass) }} onPointerDown={stop} onDblClick={stop} onClick={() => A.toggleInsertFx(m().id)}
            title={tr('c.mix.fxCountTip', { n: m().fx.length }) + ' · ' + tr('c.mix.fxToggleTip')} aria-label={tr('c.mix.fxCountTip', { n: m().fx.length })}>
            <span class="fxdot" /><span class="fxt">{m().fx.length}<span class="fxw"> FX</span></span></button>
        </Show>
      </div>
      <StripFx m={m()} full={props.full} />
      <div class="knobrow">
      <div style={{ display: 'flex', 'flex-direction': 'column', 'align-items': 'center', gap: '2px' }}>
        <div class="pandial" classList={{ auto: isAutomated(panT()) }} onPointerDown={panDown} onWheel={wheel('pan')} onContextMenu={e => e.preventDefault()} title={tr('c.mix.panTip')}>
          <svg width="30" height="30" viewBox="0 0 30 30" style={{ position: 'absolute', inset: 0 }}>
            <circle cx="15" cy="15" r="11" fill="none" transform="rotate(135 15 15)" style={{ stroke: 'var(--knob-track)', 'stroke-width': '2.5px', 'stroke-linecap': 'round', 'stroke-dasharray': '51.84px 69.12px' }} />
            <circle cx="15" cy="15" r="11" fill="none" transform="rotate(135 15 15)" style={{ stroke: 'var(--pan)', 'stroke-width': '2.5px', 'stroke-linecap': 'round', 'stroke-dasharray': pan().dash, 'stroke-dashoffset': pan().off }} />
          </svg>
          <div class="pcap" />
          <div class="pptr" style={{ transform: `rotate(${pan().angle}deg)` }} />
        </div>
        <span class="pantxt" classList={{ def: Math.abs(shownPan()) < 1e-4 }}>{panText(shownPan())}</span>
      </div>
      <div style={{ display: 'flex', 'flex-direction': 'column', 'align-items': 'center', gap: '2px' }}>
        <div class="pandial wdial" onPointerDown={widDown} onWheel={widWheel} onContextMenu={e => e.preventDefault()} title={tr('c.mix.widthTip')}>
          <svg width="30" height="30" viewBox="0 0 30 30" style={{ position: 'absolute', inset: 0 }}>
            <circle cx="15" cy="15" r="11" fill="none" transform="rotate(135 15 15)" style={{ stroke: 'var(--knob-track)', 'stroke-width': '2.5px', 'stroke-linecap': 'round', 'stroke-dasharray': '51.84px 69.12px' }} />
            <circle cx="15" cy="15" r="11" fill="none" transform="rotate(135 15 15)" style={{ stroke: 'var(--green)', 'stroke-width': '2.5px', 'stroke-linecap': 'round', 'stroke-dasharray': widArc().dash, 'stroke-dashoffset': widArc().off }} />
          </svg>
          <div class="pcap" />
          <div class="pptr" style={{ transform: `rotate(${widArc().angle}deg)` }} />
        </div>
        <span class="pantxt" classList={{ def: widText(wid()) === widText(1) }}>{widText(wid())}</span>
      </div>
      </div>
      <div class="row" style={{ gap: '2px', 'justify-content': 'center' }}>
        <MuteSolo class="mute" muted={m().muted} solo={m().solo} noSolo={m().id === 0} muteTip={tr('c.mix.muteTip')} soloTip={tr('c.mix.soloTip')}
          onMute={() => A.toggleInsertMute(m().id)} onSolo={add => A.toggleInsertSolo(m().id, add)} />
        {/* the master's clip LED (its meter's red cap): latched over 0 dBFS, where an export clips */}
        <Show when={m().id === 0}>
          <button class="clipcap" classList={{ on: !!masterClip() }} onPointerDown={stop}
            title={masterClipTip()}
            onClick={() => resetMasterClip()}>{masterClip() ? '+' + masterClip()!.overDb.toFixed(1) : 'CLIP'}</button>
        </Show>
      </div>
      {/* docked, the fader keeps one height (a taller mixer shows effect rows above the knobs instead,
          StripFx); maximized, the effect rows have a fixed room (five and "+ effect") and the fader
          takes all the rest, for fine moves */}
      <div style={{ flex: props.full ? '1 1 0' : `0 1 ${FADER_H}px`, 'min-height': '50px', display: 'flex', 'justify-content': 'center', gap: '3px' }}>
        <div class="meter"><div ref={fl} /></div>
        <div class="meter"><div ref={fr} /></div>
        <div class="fader" classList={{ auto: isAutomated(volT()) }} onPointerDown={faderDown} onWheel={wheel('vol')} onContextMenu={e => e.preventDefault()} title={tr('c.mix.faderTip')}>
          <div class="fslot" />
          {/* the dB scale: a tick per mark on every fader; the numbers on the master and under the pointer */}
          <For each={FADER_MARKS}>{([db, t]) => (
            <div class="tick" classList={{ u: db === 0 }} style={{ top: `calc(${1 - t} * (100% - 14px) + 7px)` }}><span>{db > 0 ? '+' + db : db === 0 ? '0' : String(-db)}</span></div>
          )}</For>
          <div class="cap" style={{ top: `calc(${1 - shownVol()} * (100% - 14px))` }}><div /></div>
        </div>
      </div>
      <span class="db" classList={{ unity: Math.abs(shownVol() - VOL_UNITY) < 1e-4 }}>{volText(shownVol())}</span>
      <Show when={props.full}>
        <span class="route" title={route()}>{route()}</span>
        {/* routing (the cables below the strips): drag the dot onto another strip = a send; M = into the master */}
        <div class="rtport">
          <Show when={m().id !== 0}>
            <span class="rtdot" title={tr('c.rt.portTip')} onPointerDown={portDown} />
            <button class="rtm" classList={{ on: toMaster(m()) }} title={tr('c.rt.mTip')} aria-pressed={toMaster(m())} aria-label={tr('c.rt.toMaster')}
              onPointerDown={stop} onClick={() => setToMaster(m().id, !toMaster(m()))}>M</button>
          </Show>
        </div>
      </Show>
    </div>
  );
}

/** The mixer strip under a point, other than `from` (an effect drag's target), marked for the drop. */
let fxTarget: HTMLElement | null = null;
function fxDropAt(x: number, y: number, from: Id): Insert | null {
  const el = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest<HTMLElement>('.strip[data-ins]') ?? null;
  const m = el ? S.project.inserts.find(i => i.id === Number(el.dataset.ins)) : undefined;
  const hit = m && m.id !== from ? el : null;
  if (fxTarget !== hit) { if (fxTarget) delete fxTarget.dataset.fxDrop; fxTarget = hit; if (hit) hit.dataset.fxDrop = '1'; }
  return hit ? m! : null;
}
const fxDropEnd = () => { if (fxTarget) delete fxTarget.dataset.fxDrop; fxTarget = null; setU('ghost', null); };
/** The ghost label of an effect drag over a strip ("Move to Insert 3", Ctrl: "Copy to …"). */
const fxGhost = (x: number, y: number, m: Insert | null, copy: boolean, color: string) =>
  setU('ghost', m ? { x: x + 12, y: y + 8, name: tr(copy ? 'c.mix.dropCopy' : 'c.mix.dropMove', { name: insName(m) }), color } : null);

function FxSlot(props: { f: Fx; n: number; insId: Id }) {
  const f = () => props.f;
  const info = () => presetInfo(S.catalog, f(), S.project.bpm);
  const mixT = () => ({ kind: 'mix' as const, fxId: f().id });
  const mixV = () => autoDisplay(mixT()) ?? f().mix ?? 1;
  let mg: Gesture | null = null;
  const mixOp = (v: number) => A.fxOp({ ...JSON.parse(JSON.stringify(f())), mix: Math.round(clamp(v, 0, 1) * 1000) / 1000 }, props.insId);
  const down = (e: PointerEvent) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest('button,.miniknob')) return;
    e.preventDefault();
    const sy = e.clientY, rowH = 34, ins0 = S.project.inserts.find(m => m.id === props.insId)!;
    const i0 = ins0.fx.findIndex(x => x.id === f().id), fxId = f().id;
    let g: Gesture | null = null;
    setU('fxSel', fxId);
    const sx = e.clientX, color = FXMETA[f().type].color;
    let moved = false, onto: Insert | null = null, copy = false;
    track(ev => {
      const dy = ev.clientY - sy;
      if (!moved) { if (Math.hypot(ev.clientX - sx, dy) < 4) return; moved = true; setU('fxDrag', fxId); dragCursor('grabbing'); }
      // over another insert's strip: the effect goes there on release; anywhere else it reorders its own chain
      copy = ev.ctrlKey || ev.metaKey;
      onto = fxDropAt(ev.clientX, ev.clientY, props.insId);
      fxGhost(ev.clientX, ev.clientY, onto, copy, color);
      if (onto) return;
      const ins = S.project.inserts.find(m => m.id === props.insId); if (!ins) return;
      const cur = ins.fx.findIndex(x => x.id === fxId), to = clamp(i0 + Math.round(dy / rowH), 0, ins.fx.length - 1);
      if (cur >= 0 && to !== cur) { g ??= new Gesture(tr('c.mix.fxOrder')); g.update([{ op: 'moveFx', fxId, to }]); }
    }, ev => {
      fxDropEnd();
      if (!moved) return A.openPlug(fxId);
      dragCursor(null); setU('fxDrag', null);
      if (g) { g.end(); if (!onto) say(tr('c.mix.fxOrderDone')); }
      if (onto && ev.type === 'pointerup') void A.moveFxTo([fxId], onto.id, copy);
    });
  };
  return (
    <div class="slot" classList={{ on: U.fxSel === f().id, drag: U.fxDrag === f().id }} onPointerDown={down} title={tr('c.mix.slotTip')}>
      {/* the row drags by its grip (and anywhere else on it): the order is the signal path */}
      <span class="grip" aria-hidden="true"><svg width="8" height="14" viewBox="0 0 8 14"><g fill="currentColor"><circle cx="2" cy="2" r="1.2" /><circle cx="6" cy="2" r="1.2" /><circle cx="2" cy="7" r="1.2" /><circle cx="6" cy="7" r="1.2" /><circle cx="2" cy="12" r="1.2" /><circle cx="6" cy="12" r="1.2" /></g></svg></span>
      <FxTile type={f().type} size={22} dim={f().bypass} />
      <div style={{ flex: 1, 'min-width': 0, display: 'flex', 'flex-direction': 'column', gap: '1px' }}>
        <span class="ell" style={{ font: '500 12px var(--sans)', color: f().bypass ? 'var(--fg4)' : 'var(--fg-hi)' }}>{A.fxName(f())}</span>
        <Show when={S.fxErrors[f().id]} fallback={
          <span class="ell mono" style={{ 'font-size': '9px', color: 'var(--fg4)' }}>{f().type === 'plugin' ? (f().plugin?.vendor || PLUGIN_FORMAT_LABEL[f().plugin?.format ?? 'vst3'] || 'Plugin') : tName(info().name) + (info().mod ? ' *' : '')}</span>
        }>
          {e => <span class="ell mono" style={{ 'font-size': '9px', color: 'var(--red)' }} title={unsafeReason(e()) ?? e()}>{tr(unsafeReason(e()) ? 'c.mix.pluginUnsafe' : 'c.mix.pluginNotLoaded')}</span>}
        </Show>
      </div>
      <Show when={f().type !== 'plugin'}>
        <div class="slotmix" title={'Mix ' + Math.round(mixV() * 100) + '%'}>
          <MiniKnob key={'smix' + f().id} title={tr('c.mix.slotMixTip', { v: Math.round(mixV() * 100) })} t={mixV} automated={() => isAutomated(mixT())}
            onStart={() => { mg = new Gesture('Mix'); }} onChange={t => { mg?.update([mixOp(t)]); say(A.fxName(f()) + ' · Mix ' + Math.round(t * 100) + '%'); }}
            onEnd={() => { mg?.end(); mg = null; }} onReset={() => commit([mixOp(1)], 'Mix')} onWheel={t => wheelEdit('smix' + f().id, 'Mix', [mixOp(t)])}
            onMenu={(x, y) => paramMenu(x, y, mixT(), () => commit([mixOp(1)], 'Mix'), FXMETA[f().type].color)} />
          <span class="mono">{Math.round(mixV() * 100)}</span>
        </div>
      </Show>
      <button class="ledbtn" title={f().bypass ? tr('c.fx.bypassOn') : tr('c.fx.bypassOff')} onPointerDown={e => { e.stopPropagation(); A.toggleBypass(f().id); }}>
        <span class="led" style={{ background: f().bypass ? 'var(--line2)' : 'var(--green)' }} />
      </button>
      <button class="ledbtn xdel" title={tr('c.mix.removeFx')} onPointerDown={e => e.stopPropagation()} onClick={() => A.removeFx(f().id)}>×</button>
    </div>
  );
}

/** Strip width (`.strip`, border-box) and how many strips past each edge stay built. */
const STRIP_W = 86, STRIP_MARGIN = 4;
/** Where each strip row was scrolled (the docked row is built again after the Mix view, and the other way round). */
const stripScroll = { dock: 0, full: 0 };

/** Names of the samples whose clips play into insert `id` on their own (the clip editor's MIXER). */
function samplesOn(id: Id): string[] {
  const refs = new Set(S.project.clips.filter(c => c.kind === 'audio' && c.insert === id).map(c => c.ref));
  return S.project.sources.filter(s => refs.has(s.id)).map(s => s.name);
}

/**
 * The strip row. Only strips in (or near) the visible part are built; the others are empty
 * placeholders of the same size. Out of view they look the same, and the browser no longer
 * lays every strip of a 60-insert mixer into the compositor on each frame of playback.
 * One strip row is mounted at a time (docked or Mix view): it runs the meters of its strips.
 */
function Strips(props: { full: boolean }) {
  let el!: HTMLDivElement;
  const key = props.full ? 'full' : 'dock';
  const [view, setView] = createSignal({ x: 0, w: window.innerWidth });
  const sync = () => { stripScroll[key] = el.scrollLeft; if (el.scrollLeft !== view().x || el.clientWidth !== view().w) setView({ x: el.scrollLeft, w: el.clientWidth }); };
  onMount(() => {
    el.scrollLeft = stripScroll[key];
    sync();
    el.addEventListener('scroll', sync, { passive: true });
    const ro = new ResizeObserver(sync); ro.observe(el);
    const stop = addFrameTask(meterTask);
    onCleanup(() => { el.removeEventListener('scroll', sync); ro.disconnect(); stop(); });
  });
  // an insert asked for elsewhere (double-click on a window's header, a track's number): scroll its strip into view
  // (a request already shown is not repeated when the row mounts again: it keeps its own scroll)
  let seen = revealed()?.n;
  createEffect(() => {
    const r = revealed(); if (!r || r.n === seen) return;
    seen = r.n;
    const i = S.project.inserts.findIndex(m => m.id === r.id); if (i < 0) return;
    const left = i * STRIP_W, w = el.clientWidth;
    if (left < el.scrollLeft || left + STRIP_W > el.scrollLeft + w) el.scrollTo({ left: Math.max(0, left - (w - STRIP_W) / 2), behavior: 'smooth' });
  });
  const near = (i: number) => { const v = view(), m = STRIP_MARGIN * STRIP_W; return (i + 1) * STRIP_W > v.x - m && i * STRIP_W < v.x + v.w + m; };
  const row = () => (
    <>
      <For each={S.project.inserts}>{(m, i) => <Show when={near(i())} fallback={<div class="strip" />}><Strip m={m} idx={i()} full={props.full} /></Show>}</For>
      <button class="addIns" onClick={A.addInsert} title={tr('c.mix.addInsert')}>+</button>
    </>
  );
  return (
    <div ref={el} class="strips" classList={{ routed: props.full }}>
      <Show when={props.full} fallback={row()}>
        <div class="striprow">{row()}</div>
        <RouteLane />
      </Show>
    </div>
  );
}

// ---- routing cables (the maximized mixer): one per send, from the source strip to the target's

/** Height (px) of the cable lane under the strips. */
const LANE_H = 86;
/** A send being dragged from a strip's dot: lane coordinates, and the strip under the pointer. */
const [routeDrag, setRouteDrag] = createSignal<{ from: Id; x: number; y: number; over: Id | null } | null>(null);
/** Width (px) of one strip in the maximized mixer, as laid out (measured by the lane). */
const [stripW, setStripW] = createSignal(112);
let laneEl: HTMLDivElement | undefined;

function RouteLane() {
  let el!: HTMLDivElement;
  onMount(() => {
    laneEl = el;
    const measure = () => { const s = el.parentElement?.querySelector<HTMLElement>('.striprow > .strip'); if (s?.offsetWidth) setStripW(s.offsetWidth); };
    measure();
    const ro = new ResizeObserver(measure); ro.observe(el.parentElement!);
    onCleanup(() => { ro.disconnect(); if (laneEl === el) laneEl = undefined; });
  });
  const index = createMemo(() => new Map(S.project.inserts.map((m, i) => [m.id, i])));
  // keyed "from:to:k" strings: a level change keeps the cable's elements (a drag on its label goes on)
  const cables = createMemo(() => S.project.inserts.flatMap(m => sendsOf(m).filter(s => index().has(s.to)).map((s, k) => `${m.id}:${s.to}:${k}`)));
  const cx = (i: number) => i * stripW() + stripW() / 2;
  // a cable dips lower the farther it goes, and one more step per send of the same strip (no stacking)
  const geo = (a: number, b: number, k: number) => {
    const x1 = cx(a), x2 = cx(b), depth = Math.min(LANE_H - 14, 16 + Math.min(44, Math.abs(x2 - x1) * 0.12) + k * 9);
    return { d: `M${x1} 0 C${x1} ${depth} ${x2} ${depth} ${x2} 0`, mx: (x1 + x2) / 2, my: depth * 0.75 };
  };
  const labelDown = (e: PointerEvent, from: Id, to: Id) => {
    e.stopPropagation(); A.selectInsert(from);
    if (e.button === 2) { e.preventDefault(); return cableMenu(e.clientX, e.clientY, from, to); }
    if (e.button !== 0) return;
    e.preventDefault();
    if (e.altKey || dblCheck('rt' + from + ':' + to)) { const op = sendGainOp(from, to, 1); if (op) commit([op], tr('c.rt.undoLevel')); return say(levelText(from, to, 1)); }
    const g = new Gesture(tr('c.rt.undoLevel'));
    let v = sendsOf(S.project.inserts.find(m => m.id === from)!).find(s => s.to === to)?.gain ?? 1, ly = e.clientY;
    track(ev => {
      v = clamp(v + (ly - ev.clientY) / 100 * fineMul(ev), 0, 2); if (Math.abs(v - 1) < 0.03) v = 1; ly = ev.clientY;
      const op = sendGainOp(from, to, v); if (op) g.update([op]);
      say(levelText(from, to, v));
    }, () => g.end(), 'ns-resize');
  };
  const labelWheel = (e: WheelEvent, from: Id, to: Id, gain: number) => {
    const dy = e.deltaY || e.deltaX; if (!dy) return;
    e.preventDefault(); e.stopPropagation();
    const v = clamp(Math.round((gain + (dy < 0 ? 1 : -1) * 0.05 * (e.shiftKey ? 0.2 : 1)) * 100) / 100, 0, 2);
    const op = sendGainOp(from, to, v); if (op) wheelEdit('rt' + from + ':' + to, tr('c.rt.undoLevel'), [op]);
    say(levelText(from, to, v));
  };
  const lit = (from: Id, to: Id) => U.mixSel === from || U.mixSel === to;
  return (
    <div ref={el} class="routelane" style={{ height: LANE_H + 'px', width: (S.project.inserts.length * stripW()) + 'px' }}>
      <svg width={S.project.inserts.length * stripW()} height={LANE_H}>
        <For each={cables()}>{key => {
          const [fromId, to, k] = key.split(':').map(Number);
          const src = () => S.project.inserts[index().get(fromId) ?? 0];
          const c = {
            get from() { return src(); }, to, k,
            get gain() { return sendsOf(src()).find(x => x.to === to)?.gain ?? 1; },
          };
          const g = () => geo(index().get(fromId) ?? 0, index().get(to) ?? 0, k);
          return (
            <g class="cable" classList={{ dim: !lit(c.from.id, c.to) }}>
              <path d={g().d} fill="none" stroke={c.from.color} stroke-width={lit(c.from.id, c.to) ? 2.5 : 1.5} stroke-linecap="round" />
              <g class="clabel" onPointerDown={e => labelDown(e, c.from.id, c.to)} onWheel={e => labelWheel(e, c.from.id, c.to, c.gain)} onContextMenu={e => e.preventDefault()}>
                <title>{tr('c.rt.cableTip', { a: nameOf(c.from.id), b: nameOf(c.to), v: pct(c.gain) })}</title>
                <rect x={g().mx - 19} y={g().my - 8} width="38" height="16" rx="3" stroke={c.from.color} />
                <text x={g().mx} y={g().my + 3.5} text-anchor="middle">{pct(c.gain)}</text>
              </g>
            </g>
          );
        }}</For>
        <Show when={routeDrag()}>{d => {
          const x1 = () => cx(index().get(d().from) ?? 0);
          return <path class="rtghost" d={`M${x1()} 0 C${x1()} 36 ${d().x} 36 ${d().x} ${d().y}`} fill="none" stroke-width="2" stroke-dasharray="5 4" />;
        }}</Show>
      </svg>
      <Show when={!cables().length && !routeDrag()}><div class="lanehint">{tr('c.rt.laneEmpty')}</div></Show>
    </div>
  );
}

const nameOf = (id: Id) => { const m = S.project.inserts.find(x => x.id === id); return m ? tName(m.name) || 'Insert ' + id : String(id); };
const levelText = (from: Id, to: Id, v: number) => tr('c.rt.level', { a: nameOf(from), b: nameOf(to), v: pct(v) });

/** Right-click on a cable's level: 100 %, only here (not to the master), remove. */
function cableMenu(x: number, y: number, from: Id, to: Id) {
  const m = S.project.inserts.find(v => v.id === from); if (!m) return;
  openMenu(x, y, [
    { t: 'title', label: nameOf(from) + ' → ' + nameOf(to) },
    { t: 'item', label: tr('c.rt.reset100'), act: () => { const op = sendGainOp(from, to, 1); if (op) commit([op], tr('c.rt.undoLevel')); } },
    { t: 'item', label: tr('c.rt.onlyHere'), checked: !toMaster(m), title: tr('c.rt.toMasterTip'), act: () => setToMaster(from, !toMaster(m)) },
    { t: 'sep' },
    { t: 'item', label: tr('c.rt.removeSend'), danger: true, act: () => removeSend(from, to) },
  ], 220);
}

/** Drag the effect chain's header onto a mixer strip: every effect of the insert goes there (Ctrl: copies). */
function chainDown(e: PointerEvent, ins: Insert) {
  if (e.button !== 0 || !ins.fx.length || (e.target as HTMLElement).closest('button')) return;
  e.preventDefault();
  const sx = e.clientX, sy = e.clientY, ids = ins.fx.map(f => f.id);
  let moved = false, onto: Insert | null = null, copy = false;
  track(ev => {
    if (!moved && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 4) return;
    moved = true;
    copy = ev.ctrlKey || ev.metaKey;
    onto = fxDropAt(ev.clientX, ev.clientY, ins.id);
    fxGhost(ev.clientX, ev.clientY, onto, copy, ins.color);
  }, ev => {
    fxDropEnd();
    if (moved && onto && ev.type === 'pointerup') void A.moveFxTo(ids, onto.id, copy);
  }, 'grabbing');
}

/** The selected insert's effects; its small header names the insert the slots belong to. */
function FxChain() {
  const selIns = () => S.project.inserts.find(m => m.id === U.mixSel) ?? S.project.inserts[0];
  return (
    <Show when={selIns()}>
      {ins => (
        <div class="fxchain" data-ins={ins().id}>
          <div class="hd" classList={{ grab: ins().fx.length > 0 }} title={ins().fx.length ? tr('c.mix.chainDragTip') : ''} onPointerDown={e => chainDown(e, ins())}>
            <span style={{ width: '8px', height: '8px', 'border-radius': '2px', background: ins().color }} />
            <span class="ell" style={{ flex: 1, font: '600 11px var(--sans)', color: 'var(--fg-hi)' }}>{insName(ins())}</span>
            <span class="mono ell" style={{ 'font-size': '10px', color: 'var(--fg5)', 'max-width': '45%' }} title={routeSummary(ins().id)}>{routeSummary(ins().id)}</span>
            <button class="chainbtn" title={tr('e.fxc.btnTip')} onClick={e => { const r = e.currentTarget.getBoundingClientRect(); openChainMenu(r.left, r.bottom + 4, ins()); }}>{tr('e.fxc.btn')}</button>
          </div>
          <div style={{ flex: 1, 'min-height': 0, overflow: 'auto' }}>
            <For each={ins().fx}>{(f, i) => <FxSlot f={f} n={i() + 1} insId={ins().id} />}</For>
            <div class="slot addslot" title={tr('c.mix.addFxTip')} onPointerDown={e => { if (e.button === 0) openFxMenu(e.clientX, e.clientY, ins().id); }}>
              <span class="n">{ins().fx.length + 1}</span>
              <span class="tile addtile"><Icon d={ICONS.plus} /></span>
              <span style={{ flex: 1, font: '500 12px var(--sans)', color: 'var(--fg2)' }}>{tr('c.mix.addFxLabel')}</span>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}


const MIX_ICON = 'M6 4v16 M12 4v16 M18 4v16 M4 14h4 M10 8h4 M16 12h4';
/** Height of the Mixer's title bar (px). */
const MIXHD_H = 28;

/**
 * The Mixer's title bar, the same docked, floating or maximized (FL: every window has one):
 * its name, what Ctrl+L would route, then dock / float, maximize, hide. Double-click = maximize.
 */
function MixHead() {
  const n = () => selectedChannels().length;
  return (
    <>
      <span class="tile" style={{ width: '20px', height: '20px', background: 'var(--line)', color: 'var(--fg2)' }}><Icon d={MIX_ICON} size={12} /></span>
      <span class="wintitle">{tr('c.mv.mixer')}</span>
      {/* channels picked in the Channel rack can be sent to the selected insert from here (Ctrl+L); nothing picked, nothing shown */}
      <Show when={n()}>
        <button class="chip mvroute" title={tr('c.mv.selHint', { n: n() })} onPointerDown={e => e.stopPropagation()} onClick={e => routeSelectedChannels(U.mixSel, e.shiftKey)}>
          {tr('c.mv.routeBtn', { n: n() })}<kbd>Ctrl L</kbd>
        </button>
      </Show>
    </>
  );
}
function MixControls() {
  return (
    <WinControls
      dock={paneDock('mix')}
      max={{ on: U.mixView, toggle: toggleMixView }}
      close={{ tip: tr('c.mv.hideTip'), act: hideMixer }} />
  );
}
/** The title bar of the docked and the maximized Mixer (the floating one gets it from `PanelFrame`). Docked, it drags the mixer off onto the dock guides. */
function MixBar(props: { docked?: boolean }) {
  return (
    <div class="winhd mixhd" style={{ height: MIXHD_H + 'px' }}
      onPointerDown={e => { if (props.docked) paneHeadDown(e, 'mix'); }}
      onContextMenu={e => { if (props.docked) openDockMenu(e, 'mix'); }}
      onDblClick={e => { if (!(e.target as HTMLElement).closest('button')) toggleMixView(); }}>
      <MixHead />
      <div style={{ flex: 1 }} />
      <MixControls />
    </div>
  );
}

/**
 * What the hidden docked Mixer leaves in its strip: its title bar (on a side edge, a narrow column
 * with the title turned), so it stays findable. The strip's handle (`PatDock.tsx`) makes it a button:
 * click to open, drag toward the Playlist to open it at a size.
 */
export function MixCollapsedHead(props: { vert: boolean }) {
  return (
    <>
      <span class="tile" style={{ width: '20px', height: '20px', background: 'var(--line)', color: 'var(--fg2)', flex: 'none' }}><Icon d={MIX_ICON} size={12} /></span>
      <span class="wintitle">{tr('c.mv.mixer')}</span>
      <Show when={!props.vert}>
        <span class="mvhint">{tr('c.mv.handleHint')}</span>
        <div style={{ flex: 1 }} />
        <span class="mixopen" aria-hidden="true"><Icon d="M6 15l6-6 6 6" size={13} /></span>
      </Show>
    </>
  );
}

/** The instrument icon's width in a strip's name, its margin included (px). */
const ICON_W = 14;

/**
 * The Mixer docked (in a strip of the main window, `PatDock.tsx` sizes it): its title bar floats,
 * moves, maximizes or hides it. While maximized the pane keeps its size (so the Playlist under the
 * full view does not move) and builds no strips.
 */
export function MixDockPane() {
  return (
    <div class="mixdock">
      <MixBar docked />
      {/* on a side edge the mixer is narrow and tall: the effect chain goes under the strips */}
      <div class="mixer docked" classList={{ side: findPane(U.dock, 'mix')?.edge !== 'bottom' }}>
        <Show when={!U.mixView}>
          <Strips full={false} />
          <FxChain />
        </Show>
      </div>
    </div>
  );
}

/** The floating Mixer: the same window frame as the Channel Rack and the Piano roll. */
export function MixerWindow(props: { z: number }) {
  return (
    <PanelFrame geom={() => U.mixWin} setGeom={g => setU('mixWin', w => ({ ...w, ...g }))} z={props.z} class="mixwin" dockId="mix" minW={480} minH={260}
      front={mixFront} onHeadDbl={toggleMixView} head={<MixHead />} controls={<MixControls />}>
      <div class="mixer floating">
        <Show when={!U.mixView}>
          <Strips full={false} />
          <FxChain />
        </Show>
      </div>
    </PanelFrame>
  );
}

/** The Mixer maximized over the whole workspace (its title bar's maximize, or a double-click on it). */
export function MixView() {
  return (
    <div class="mixview" role="region" aria-label={tr('c.mv.title')}>
      <MixBar />
      <div class="mixer full">
        <Strips full />
        <FxChain />
      </div>
    </div>
  );
}

