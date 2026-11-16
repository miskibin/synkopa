import { createEffect, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { PLUGIN_FORMAT_LABEL, VOL_UNITY, type Fx, type Id, type Insert } from '../api/types';
import { clamp, panText, volText } from '../lib/format';
import { fitMixH, MIX_HANDLE_H, mixHandleDrag, mixMaxH, mixSplitDrag, SPLIT } from '../lib/layout';
import { stripLabel } from '../lib/mixer';
import { t as tr, tName } from '../lib/i18n';
import { channels, routeSelectedChannels, selectedChannels } from '../state/pattern';
import { hideMixer, mixFront, revealed, setMixDock, toggleMixView } from '../state/mixView';
import { PanelFrame, WinControls } from './PanelFrame';
import { presetInfo } from '../lib/presets';
import * as A from '../state/actions';
import { commit, Gesture, wheelEdit } from '../state/edit';
import { autoDisplay, isAutomated, paramMenu } from '../state/automation';
import { MiniKnob } from '../ui/Knob';
import { anyInsertSolo, insName, S } from '../state/store';
import { tele } from '../state/telemetry';
import { openMenu, openRename, say, setU, U, type MenuItem } from '../state/ui';
import { dblCheck, dragCursor, track } from '../ui/drag';
import { addFrameTask } from '../ui/frameLoop';
import { FXMETA, FxTile, Icon, ICONS } from '../ui/icons';
import { guessIcon, InstIcon, trackIcon } from '../ui/instIcons';
import { openFxPicker } from './FxPicker';
import { openFxChains, saveFxChain } from './FxChains';

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
    ...(id === 0 ? [] : [{ t: 'sep' }, { t: 'item', label: tr('c.mix.removeInsert'), danger: true, act: () => A.removeInsert(id) }] as MenuItem[]),
  ], 330);
}

/** One insert's strip. `full`: in the Mix view (with the level readout and what plays through it); docked it stays lean. */
function Strip(props: { m: Insert; idx: number; full: boolean }) {
  const m = () => props.m;
  let fl!: HTMLDivElement, fr!: HTMLDivElement;
  onMount(() => { meterEls.set(props.m.id, [fl, fr]); hold.delete(props.m.id); });
  onCleanup(() => meterEls.delete(props.m.id));
  const audible = () => !m().muted && (m().id === 0 || !anyInsertSolo() || m().solo);
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
    if (dblCheck('wid' + m().id)) return A.setInsert(m().id, { width: 1 }, tr('c.mix.width'));
    const g = new Gesture(tr('c.mix.width')); let v = wid(), ly = e.clientY;
    track(ev => { v = clamp(v + (ly - ev.clientY) / 70 * (ev.shiftKey ? 0.2 : 1) * 2, 0, 2); if (Math.abs(v - 1) < 0.03) v = 1; ly = ev.clientY; g.update([put({ width: Math.round(v * 100) / 100 })]); say(tr('c.mix.sayWidth', { v: widText(v) })); }, () => g.end(), 'ns-resize');
  };
  const widWheel = (e: WheelEvent) => {
    const d = e.deltaY || e.deltaX; if (!d) return; e.preventDefault(); e.stopPropagation();
    const v = clamp(Math.round((wid() + (d < 0 ? 1 : -1) * 0.05 * (e.shiftKey ? 0.2 : 1)) * 100) / 100, 0, 2);
    wheelEdit('wid' + m().id, tr('c.mix.width'), [put({ width: v })]); say(tName(m().name) + ' · ' + tr('c.mix.widthLc') + ' ' + widText(v));
  };
  const select = (e: PointerEvent) => {
    if (e.button === 2) { e.preventDefault(); return openStripMenu(e.clientX, e.clientY, m().id); }
    A.selectInsert(m().id);
  };
  const panDown = (e: PointerEvent) => {
    if (e.button === 2) { e.preventDefault(); e.stopPropagation(); return paramMenu(e.clientX, e.clientY, panT(), () => A.setInsert(m().id, { pan: 0 }, tr('c.mix.pan')), m().color); }
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation(); A.selectInsert(m().id);
    if (dblCheck('pan' + m().id)) return A.setInsert(m().id, { pan: 0 }, tr('c.mix.pan'));
    const g = new Gesture(tr('c.mix.pan')); let v = m().pan, ly = e.clientY;
    track(ev => { v = clamp(v + (ly - ev.clientY) / 70 * (ev.shiftKey ? 0.2 : 1), -1, 1); ly = ev.clientY; g.update([put({ pan: v })]); say('Pan ' + panText(v)); }, () => g.end(), 'ns-resize');
  };
  const faderDown = (e: PointerEvent) => {
    if (e.button === 2) { e.preventDefault(); e.stopPropagation(); return paramMenu(e.clientX, e.clientY, volT(), () => A.setInsert(m().id, { vol: VOL_UNITY }, tr('c.mix.volume')), m().color); }
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation(); A.selectInsert(m().id);
    if (dblCheck('vol' + m().id)) return A.setInsert(m().id, { vol: VOL_UNITY }, tr('c.mix.volume'));
    const h = Math.max(40, (e.currentTarget as HTMLElement).getBoundingClientRect().height - 14);
    const g = new Gesture(tr('c.mix.volume')); let v = m().vol, ly = e.clientY;
    track(ev => { v = clamp(v + (ly - ev.clientY) / h * (ev.shiftKey ? 0.2 : 1), 0, 1); ly = ev.clientY; g.update([put({ vol: v })]); say(tr('c.mix.volume') + ' ' + volText(v)); }, () => g.end(), 'ns-resize');
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
  return (
    <div class="strip" data-ins={m().id} classList={{ on: U.mixSel === m().id, dimmed: !audible() }} onPointerDown={select}>
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
      <div class="knobrow">
      <div style={{ display: 'flex', 'flex-direction': 'column', 'align-items': 'center', gap: '2px' }}>
        <div class="pandial" classList={{ auto: isAutomated(panT()) }} onPointerDown={panDown} onWheel={wheel('pan')} onContextMenu={e => e.preventDefault()} title={tr('c.mix.panTip')}>
          <svg width="30" height="30" viewBox="0 0 30 30" style={{ position: 'absolute', inset: 0 }}>
            <circle cx="15" cy="15" r="11" fill="none" transform="rotate(135 15 15)" style={{ stroke: 'var(--knob-track)', 'stroke-width': '2.5px', 'stroke-linecap': 'round', 'stroke-dasharray': '51.84px 69.12px' }} />
            <circle cx="15" cy="15" r="11" fill="none" transform="rotate(135 15 15)" style={{ stroke: 'var(--blue)', 'stroke-width': '2.5px', 'stroke-linecap': 'round', 'stroke-dasharray': pan().dash, 'stroke-dashoffset': pan().off }} />
          </svg>
          <div class="pcap" />
          <div class="pptr" style={{ transform: `rotate(${pan().angle}deg)` }} />
        </div>
        <span class="pantxt">{panText(shownPan())}</span>
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
        <span class="pantxt">{widText(wid())}</span>
      </div>
      </div>
      <div class="row" style={{ gap: '2px', 'justify-content': 'center' }}>
        <button class="mini m mute" classList={{ on: m().muted }} title={tr('c.mix.muteTip')} onPointerDown={stop} onClick={() => A.toggleInsertMute(m().id)}>M</button>
        <Show when={m().id !== 0}>
          <button class="mini s mute" classList={{ on: m().solo }} title={tr('c.mix.soloTip')} onPointerDown={stop} onClick={e => A.toggleInsertSolo(m().id, e.ctrlKey || e.metaKey)}>S</button>
        </Show>
      </div>
      <div style={{ flex: 1, 'min-height': '50px', display: 'flex', 'justify-content': 'center', gap: '3px' }}>
        <div class="meter"><div ref={fl} /></div>
        <div class="meter"><div ref={fr} /></div>
        <div class="fader" classList={{ auto: isAutomated(volT()) }} onPointerDown={faderDown} onWheel={wheel('vol')} onContextMenu={e => e.preventDefault()} title={tr('c.mix.faderTip')}>
          <div class="fslot" />
          <div class="unity" style={{ top: 'calc(0.2063 * (100% - 14px) + 7px)' }} />
          <div class="cap" style={{ top: `calc(${1 - shownVol()} * (100% - 14px))` }}><div /></div>
        </div>
      </div>
      <Show when={props.full}>
        <span class="db">{volText(shownVol())}</span>
        <span class="route" title={route()}>{route()}</span>
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
      <span class="n">{props.n}</span>
      <FxTile type={f().type} size={22} dim={f().bypass} />
      <div style={{ flex: 1, 'min-width': 0, display: 'flex', 'flex-direction': 'column', gap: '1px' }}>
        <span class="ell" style={{ font: '500 12px var(--sans)', color: f().bypass ? 'var(--fg4)' : 'var(--fg-hi)' }}>{A.fxName(f())}</span>
        <Show when={S.fxErrors[f().id]} fallback={
          <span class="ell mono" style={{ 'font-size': '9px', color: 'var(--fg4)' }}>{f().type === 'plugin' ? (f().plugin?.vendor || PLUGIN_FORMAT_LABEL[f().plugin?.format ?? 'vst3'] || 'Plugin') : tName(info().name) + (info().mod ? ' *' : '')}</span>
        }>
          {e => <span class="ell mono" style={{ 'font-size': '9px', color: 'var(--red)' }} title={e()}>{tr('c.mix.pluginNotLoaded')}</span>}
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
  return (
    <div ref={el} class="strips">
      <For each={S.project.inserts}>{(m, i) => <Show when={near(i())} fallback={<div class="strip" />}><Strip m={m} idx={i()} full={props.full} /></Show>}</For>
      <button class="addIns" onClick={A.addInsert} title={tr('c.mix.addInsert')}>+</button>
    </div>
  );
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
            <span class="mono" style={{ 'font-size': '10px', color: 'var(--fg5)' }}>{ins().id === 0 ? tr('c.mix.toOutput') : '→ Master'}</span>
            <button class="chainbtn" title={tr('e.fxc.btnTip')} onClick={e => { const r = e.currentTarget.getBoundingClientRect(); openChainMenu(r.left, r.bottom + 4, ins()); }}>{tr('e.fxc.btn')}</button>
          </div>
          <div style={{ flex: 1, 'min-height': 0, overflow: 'auto' }}>
            <For each={ins().fx}>{(f, i) => <FxSlot f={f} n={i() + 1} insId={ins().id} />}</For>
            <div class="slot" title={tr('c.mix.addFxTip')} onPointerDown={e => { if (e.button === 0) openFxMenu(e.clientX, e.clientY, ins().id); }}>
              <span class="n">{ins().fx.length + 1}</span>
              <span class="tile addtile"><Icon d={ICONS.plus} /></span>
              <span style={{ flex: 1, font: '500 12px var(--sans)', color: 'var(--blue)' }}>{tr('c.mix.addFxLabel')}</span>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}

/** FL: a right-click on the mixer sends the floating windows behind (like a click on the playlist). */
const hideWins = (e: PointerEvent) => { if (e.button === 2) A.playlistFront(); };

const MIX_ICON = 'M6 4v16 M12 4v16 M18 4v16 M4 14h4 M10 8h4 M16 12h4';
const DOCK_BOTTOM_ICON = 'M4 5h16v14H4z M4 13h16';
const FLOAT_ICON = 'M9 9h11v10H9z M5 15H4V5h11v1';
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
      <span class="mvhint">{n() ? tr('c.mv.selHint', { n: n() }) : tr('c.mv.noSelHint')}</span>
    </>
  );
}
function MixControls() {
  return (
    <WinControls
      dock={{ docked: U.mixDock, toggle: () => setMixDock(!U.mixDock), tip: tr(U.mixDock ? 'b.dk.float' : 'c.mv.dockBottom'), icon: U.mixDock ? FLOAT_ICON : DOCK_BOTTOM_ICON }}
      max={{ on: U.mixView, toggle: toggleMixView }}
      close={{ tip: tr('c.mv.hideTip'), act: hideMixer }} />
  );
}
/** The title bar of the docked and the maximized Mixer (the floating one gets it from `PanelFrame`). */
function MixBar() {
  return (
    <div class="winhd mixhd" style={{ height: MIXHD_H + 'px' }}
      onDblClick={e => { if (!(e.target as HTMLElement).closest('button')) toggleMixView(); }}>
      <MixHead />
      <div style={{ flex: 1 }} />
      <MixControls />
    </div>
  );
}

/**
 * The Mixer docked under the Playlist: its splitter sizes it, its title bar floats, maximizes or
 * hides it. Hidden (× or F9) or dragged below the breakpoint it collapses to a thin handle (click
 * it, Enter, or drag it up to open it again at the height it had). While maximized, the slot keeps
 * its size (so the Playlist under the full view does not move) and builds no strips.
 */
/** The instrument icon's width in a strip's name, its margin included (px). */
const ICON_W = 14;

export function Mixer() {
  // One clamp for both the drag and the display: the old pair (drag up to innerHeight-200,
  // show at most 40 %) let the stored height run past what was drawn, so the splitter
  // stalled, then jumped, and the playlist above it jerked.
  const [winH, setWinH] = createSignal(window.innerHeight);
  onMount(() => { const r = () => setWinH(window.innerHeight); window.addEventListener('resize', r); onCleanup(() => window.removeEventListener('resize', r)); });
  // A docked Piano roll needs its minimum height too: the mixer gives way (down to its own minimum)
  // rather than squeezing the roll to a toolbar without notes. Its stored height is kept.
  const maxH = () => mixMaxH(winH(), !!(U.rollDock && U.rollWin));
  const mixH = () => fitMixH(U.mixH, maxH());
  const split = (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault();
    const sy = e.clientY, h0 = mixH();
    track(ev => { const r = mixSplitDrag(h0, ev.clientY - sy, maxH()); if (r.min !== U.mixMin || r.h !== U.mixH) setU({ mixMin: r.min, mixH: r.h }); }, null, 'row-resize');
  };
  const expand = () => { setU('mixMin', false); say(tr('c.mv.expanded')); };
  const collapse = () => { setU('mixMin', true); say(tr('c.mv.collapsed')); };
  const handleDown = (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault();
    const sy = e.clientY, last = U.mixH;
    let moved = false;
    track(ev => {
      if (!moved && Math.abs(ev.clientY - sy) < 4) return;
      moved = true;
      const r = mixHandleDrag(last, ev.clientY - sy, maxH());
      if (r.min !== U.mixMin || r.h !== U.mixH) setU({ mixMin: r.min, mixH: r.h });
    }, () => { if (!moved) expand(); }, 'row-resize');
  };
  const key = (open: boolean) => (e: KeyboardEvent) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault(); e.stopPropagation();
    if (open) expand(); else collapse();
  };
  return (
    // floating, the Mixer is a window in the window layer (`MixerWindow`): the dock slot is empty
    <Show when={U.mixDock}>
    <div class="mixdock" inert={U.mixView} on:pointerdown={{ capture: true, handleEvent: hideWins }}>
      <Show when={!U.mixMin} fallback={
        <div class="mixhandle" style={{ height: MIX_HANDLE_H + 'px' }} role="button" tabIndex={0} aria-expanded="false"
          aria-label={tr('c.mv.handleLabel')} title={tr('c.mv.handleTip')} onPointerDown={handleDown} onKeyDown={key(true)}>
          <span class="grip" />
        </div>
      }>
        <div class="mixsplit" style={{ height: SPLIT + 'px' }} role="separator" aria-orientation="horizontal" tabIndex={0}
          aria-label={tr('c.mv.splitLabel')} title={tr('c.mv.splitTip')} onPointerDown={split} onKeyDown={key(false)} />
        <MixBar />
        <div class="mixer" style={{ height: Math.max(60, mixH() - MIXHD_H) + 'px' }}>
          <Show when={!U.mixView}>
            <Strips full={false} />
            <FxChain />
          </Show>
        </div>
      </Show>
    </div>
    </Show>
  );
}

/** The floating Mixer: the same window frame as the Channel Rack and the Piano roll. */
export function MixerWindow(props: { z: number }) {
  return (
    <PanelFrame geom={() => U.mixWin} setGeom={g => setU('mixWin', w => ({ ...w, ...g }))} z={props.z} class="mixwin" minW={480} minH={260}
      front={mixFront} onHeadDbl={toggleMixView} head={<MixHead />} controls={<MixControls />}>
      <div class="mixer floating" on:pointerdown={{ capture: true, handleEvent: hideWins }}>
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
    <div class="mixview" role="region" aria-label={tr('c.mv.title')} on:pointerdown={{ capture: true, handleEvent: hideWins }}>
      <MixBar />
      <div class="mixer full">
        <Strips full />
        <FxChain />
      </div>
    </div>
  );
}

