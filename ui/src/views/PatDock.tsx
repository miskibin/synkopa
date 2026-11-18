// The main window's work area (#90): the Browser, then the docked panels around the Playlist —
// strips on the left and right edges beside it, strips at the bottom under everything — laid out
// from `U.dock` (`lib/dock.ts`). A strip has a splitter on its Playlist side (its thickness is its
// first panel's, `U.paneSize`, clamped by `fitStrips` so the Playlist keeps room) and splitters
// between the panels sharing it. While a title bar is dragged the dock guides show (`DockGuides`).
import { createMemo, For, Show, type JSX } from 'solid-js';
import { t } from '../lib/i18n';
import { clamp } from '../lib/format';
import { MIX_HANDLE_H, mixHandleDrag, mixSplitDrag, PLAYLIST_MIN_H, PLAYLIST_MIN_W, SPLIT } from '../lib/layout';
import { dim, fitStrips, PANE_MIN, PANE_MIN_LEN, side, splitShares, type DockPane, type Edge, type PaneId } from '../lib/dock';
import { channels, closeRack, closeRoll, curPat, newPattern, openRack, RACK_LEFT, RACK_MIN_W, RACK_PAD_X, selChan, STEP_W } from '../state/pattern';
import { rackDockFitWidth } from '../lib/rack';
import { stepsOf } from '../lib/pattern';
import { say, setU, U } from '../state/ui';
import { tuckOnClick } from '../state/actions';
import { guides, openDockMenu, paneDock, paneHeadDown, paneOpen } from '../state/docking';
import { track } from '../ui/drag';
import { Browser } from './Browser';
import { ChannelRack } from './ChannelRack';
import { MixCollapsedHead, MixDockPane } from './Mixer';
import { PianoRoll } from './PianoRoll';
import { Playlist } from './Playlist';
import { guardKeys, useSize } from './patternShared';
import { WinControls } from './PanelFrame';

/** A strip as laid out: its edge and index, its panels shown (with their index in the strip), or the hidden Mixer's handle. */
interface StripView { edge: Edge; si: number; panes: { p: DockPane; at: number }[]; handle: boolean }

export function Workspace() {
  let col!: HTMLDivElement, row!: HTMLDivElement;
  const colSize = useSize(() => col), rowSize = useSize(() => row);
  // until the user drags its splitter, a rack docked on a side is as wide as the pattern's steps
  const autoW = () => { const pt = curPat(); return rackDockFitWidth(pt ? stepsOf(pt) : 16, { left: RACK_LEFT, cw: STEP_W, pad: 2 * RACK_PAD_X + 2, min: RACK_MIN_W }); };

  const views = (edge: Edge): StripView[] => U.dock[edge].map((s, si) => {
    const panes = s.map((p, at) => ({ p, at })).filter(x => paneOpen(x.p.id));
    // the hidden Mixer alone in its strip stays as a thin handle (click or drag it out); sharing a strip it just leaves
    return { edge, si, panes, handle: !panes.length && s.length === 1 && s[0].id === 'mix' };
  }).filter(v => v.panes.length || v.handle);
  const left = createMemo(() => views('left')), right = createMemo(() => views('right')), bottom = createMemo(() => views('bottom'));

  const want = (v: StripView) => { const id = v.panes[0].p.id, d = dim(v.edge), w = U.paneSize[id][d]; return id === 'rack' && d === 'w' && !w ? autoW() : w; };
  const minOf = (v: StripView) => Math.max(...v.panes.map(x => PANE_MIN[x.p.id][dim(v.edge)]));
  const sized = (vs: StripView[]) => vs.filter(v => v.panes.length);
  // the side strips share the row beside the Browser, the bottom strips the column's height; each splitter and handle takes its own
  const sideAvail = () => rowSize().w - (U.pickOpen ? U.pickW + SPLIT : 0) - [...left(), ...right()].reduce((a, v) => a + (v.handle ? MIX_HANDLE_H : SPLIT), 0);
  const bottomAvail = () => colSize().h - bottom().reduce((a, v) => a + (v.handle ? MIX_HANDLE_H : SPLIT), 0);
  const sideFit = createMemo(() => { const vs = sized([...left(), ...right()]); return { vs, px: fitStrips(vs.map(want), vs.map(minOf), rowSize().w > 0 ? sideAvail() : 0, PLAYLIST_MIN_W) }; });
  const bottomFit = createMemo(() => { const vs = sized(bottom()); return { vs, px: fitStrips(vs.map(want), vs.map(minOf), colSize().h > 0 ? bottomAvail() : 0, PLAYLIST_MIN_H) }; });
  const fitOf = (v: StripView) => (side(v.edge) ? sideFit() : bottomFit());
  const sizeOf = (v: StripView) => { const f = fitOf(v), i = f.vs.findIndex(x => x.edge === v.edge && x.si === v.si); return i < 0 ? 0 : f.px[i]; };
  /** The most a strip can take: what the Playlist and the other strips on its axis leave. */
  const maxOf = (v: StripView) => {
    const f = fitOf(v), others = f.px.reduce((a, px, i) => (f.vs[i].edge === v.edge && f.vs[i].si === v.si ? a : a + px), 0);
    return side(v.edge) ? sideAvail() - PLAYLIST_MIN_W - others : bottomAvail() - PLAYLIST_MIN_H - others;
  };
  /** How much a pointer move grows a strip on `edge` (toward the Playlist = bigger). */
  const grow = (edge: Edge, dx: number, dy: number) => (edge === 'left' ? dx : edge === 'right' ? -dx : -dy);
  const setSize = (id: PaneId, d: 'w' | 'h', v: number) => setU('paneSize', id, d, Math.round(v));

  const stripSplit = (v: StripView) => (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation();
    const id = v.panes[0].p.id, d = dim(v.edge), sx = e.clientX, sy = e.clientY, s0 = sizeOf(v), max = maxOf(v), min = minOf(v);
    const mixOnly = v.panes.length === 1 && id === 'mix';
    track(ev => {
      const g = grow(v.edge, ev.clientX - sx, ev.clientY - sy);
      if (mixOnly) {
        // dragged far enough toward the edge, the Mixer collapses to its handle (keeping its size)
        const r = mixSplitDrag(s0, -g, max, min);
        if (r.min !== U.mixMin) setU('mixMin', r.min);
        if (!r.min) setSize('mix', d, r.h);
        return;
      }
      setSize(id, d, clamp(s0 + g, min, Math.max(min, max)));
    }, null, side(v.edge) ? 'col-resize' : 'row-resize');
  };
  const stripKey = (v: StripView) => (e: KeyboardEvent) => {
    const id = v.panes[0].p.id, d = dim(v.edge), step = e.shiftKey ? 64 : 16;
    const inward = { left: 'ArrowRight', right: 'ArrowLeft', bottom: 'ArrowUp' }[v.edge], outward = { left: 'ArrowLeft', right: 'ArrowRight', bottom: 'ArrowDown' }[v.edge];
    if (e.key === inward || e.key === outward) {
      e.preventDefault(); e.stopPropagation();
      setSize(id, d, clamp(sizeOf(v) + (e.key === inward ? step : -step), minOf(v), Math.max(minOf(v), maxOf(v))));
    } else if ((e.key === 'Enter' || e.key === ' ') && v.panes.length === 1 && id === 'mix') {
      e.preventDefault(); e.stopPropagation();
      setU('mixMin', true); say(t('c.mv.collapsed'));
    }
  };
  const handleDown = (v: StripView) => (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault();
    const sx = e.clientX, sy = e.clientY, d = dim(v.edge), last = U.paneSize.mix[d], min = PANE_MIN.mix[d];
    // room for the opened mixer: what is left of the axis now that the handle would become a strip
    const max = Math.max(min, (side(v.edge) ? sideAvail() - PLAYLIST_MIN_W - sideFit().px.reduce((a, b) => a + b, 0) : bottomAvail() - PLAYLIST_MIN_H - bottomFit().px.reduce((a, b) => a + b, 0)) + MIX_HANDLE_H - SPLIT);
    let moved = false;
    track(ev => {
      const g = grow(v.edge, ev.clientX - sx, ev.clientY - sy);
      if (!moved && Math.abs(g) < 4) return;
      moved = true;
      const r = mixHandleDrag(last, -g, max, min);
      if (r.min !== U.mixMin) setU('mixMin', r.min);
      if (!r.min) setSize('mix', d, r.h);
    }, () => { if (!moved) { setU('mixMin', false); say(t('c.mv.expanded')); } }, side(v.edge) ? 'col-resize' : 'row-resize');
  };
  const handleKey = (e: KeyboardEvent) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault(); e.stopPropagation();
    setU('mixMin', false); say(t('c.mv.expanded'));
  };

  /** Splitter between panels `i` and `i + 1` of a strip: their shares follow the pointer, both keep their minimum length. */
  const paneSplit = (v: StripView, i: number) => (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation();
    const strip = (e.currentTarget as HTMLElement).parentElement!;
    const els = [...strip.querySelectorAll<HTMLElement>(':scope > [data-dockpane]')];
    const along = side(v.edge), px = els.map(el => (along ? el.clientHeight : el.clientWidth));
    const mins = v.panes.map(x => PANE_MIN_LEN[x.p.id][along ? 'h' : 'w']);
    const s0 = along ? e.clientY : e.clientX;
    track(ev => {
      const sh = splitShares(px, i, (along ? ev.clientY : ev.clientX) - s0, mins);
      v.panes.forEach((x, k) => setU('dock', v.edge, v.si, x.at, 'share', sh[k]));
    }, null, along ? 'row-resize' : 'col-resize');
  };

  const splitTip = (v: StripView) => (v.panes.length === 1 && v.panes[0].p.id === 'rack' && side(v.edge) ? t('b.dk.resizeRack') : t('b.dk.resizeStrip'));
  // Rows are keyed by strings, so a panel opening, closing or resizing elsewhere does not rebuild
  // (and reset) the panels already on screen; a row reads its strip through `byKey`.
  const keyOf = (v: StripView) => `${v.edge}:${v.si}:${v.handle ? 'h' : 's'}`;
  const byKey = createMemo(() => new Map([...left(), ...right(), ...bottom()].map(v => [keyOf(v), v])));
  const keys = (vs: StripView[]) => vs.map(keyOf);
  const Strip = (p: { k: string; splitFirst: boolean }) => (
    <Show when={byKey().get(p.k)}>{v => (
      <Show when={!v().handle} fallback={
        <div class="mixhandle" classList={{ vert: side(v().edge) }} style={side(v().edge) ? { width: MIX_HANDLE_H + 'px' } : { height: MIX_HANDLE_H + 'px' }}
          role="button" tabIndex={0} aria-expanded="false" aria-label={t('c.mv.handleLabel')} title={t('c.mv.handleTip')} onPointerDown={e => handleDown(v())(e)} onKeyDown={handleKey}>
          <MixCollapsedHead vert={side(v().edge)} />
        </div>
      }>
        {(() => {
          const split = (
            <div class={side(v().edge) ? 'vsplit' : 'docksplit'} role="separator" aria-orientation={side(v().edge) ? 'vertical' : 'horizontal'} tabIndex={0}
              aria-label={t('b.dk.resizeStrip')} title={splitTip(v())} onPointerDown={e => stripSplit(v())(e)} onKeyDown={e => stripKey(v())(e)}
              onDblClick={() => { if (v().panes[0].p.id === 'rack' && side(v().edge)) setU('paneSize', 'rack', 'w', 0); }} />
          );
          const body = (
            <div class={'dockstrip ' + (side(v().edge) ? 'stack' : 'beside')} style={side(v().edge) ? { width: sizeOf(v()) + 'px' } : { height: sizeOf(v()) + 'px' }}>
              <For each={v().panes.map(x => `${x.p.id}:${x.at}`)}>{(pk, k) => (
                <Show when={v().panes[k()]}>{x => <>
                  <Show when={k() > 0}>
                    <div class={side(v().edge) ? 'panesplit h' : 'panesplit v'} role="separator" aria-orientation={side(v().edge) ? 'horizontal' : 'vertical'}
                      title={t('b.dk.resizeShare')} onPointerDown={e => paneSplit(v(), k() - 1)(e)} />
                  </Show>
                  <div class="dockpane" data-dockpane={pk && `${v().edge}:${v().si}:${x().at}`}
                    style={{ flex: `${x().p.share} 1 0`, [side(v().edge) ? 'min-height' : 'min-width']: v().panes.length > 1 ? PANE_MIN_LEN[x().p.id][side(v().edge) ? 'h' : 'w'] + 'px' : '0' }}>
                    <PaneBody id={x().p.id} />
                  </div>
                </>}</Show>
              )}</For>
            </div>
          );
          return p.splitFirst ? <>{split}{body}</> : <>{body}{split}</>;
        })()}
      </Show>
    )}</Show>
  );

  return (
    // the Mix view (F9) covers this; it stays laid out (scroll, zoom and sizes are kept) but is not painted or reachable
    // a plain click anywhere in it (Browser, docked panels, Playlist) sends the floating windows behind it
    <div ref={col} class="workcol" classList={{ away: U.mixView }} inert={U.mixView} aria-hidden={U.mixView}
      on:pointerdown={{ capture: true, handleEvent: tuckOnClick }}>
      <div ref={row} class="workrow">
        <Show when={U.pickOpen}><Browser /></Show>
        {/* left strips: the outermost first, each with its splitter on the Playlist side */}
        <For each={keys(left()).reverse()}>{k => <Strip k={k} splitFirst={false} />}</For>
        <Playlist />
        <For each={keys(right())}>{k => <Strip k={k} splitFirst />}</For>
      </div>
      <For each={keys(bottom())}>{k => <Strip k={k} splitFirst />}</For>
    </div>
  );
}

/** What a docked panel shows: the Channel Rack, the Piano roll (or why it cannot) or the Mixer. */
function PaneBody(props: { id: PaneId }): JSX.Element {
  if (props.id === 'mix') return <MixDockPane />;
  if (props.id === 'rack') {
    return (
      <Show when={curPat()} fallback={<DockEmpty which="rack" msg={t('b.dk.noPattern')} action={{ label: t('b.dk.newPattern'), act: () => { newPattern(); openRack(); } }} />}>
        <ChannelRack z={0} docked />
      </Show>
    );
  }
  return (
    <Show when={curPat() && selChan()} fallback={<DockEmpty which="roll" msg={t(!curPat() ? 'b.dk.noPattern' : 'b.dk.noChannel')}
      action={!curPat() ? { label: t('b.dk.newPattern'), act: () => { newPattern(); openRack(); } } : channels().length ? undefined : { label: t('b.dk.openRack'), act: openRack }} />}>
      <PianoRoll z={0} docked />
    </Show>
  );
}

/** A docked panel with nothing to show (no pattern, no channel): says why and offers the way out; it can still float or hide. */
function DockEmpty(props: { which: 'rack' | 'roll'; msg: string; action?: { label: string; act: () => void } }): JSX.Element {
  return (
    <div tabIndex={0} class={'win patwin docked ' + (props.which === 'rack' ? 'rackwin' : 'rollwin')} onKeyDown={e => guardKeys(e)}>
      <div class="winhd" onPointerDown={e => paneHeadDown(e, props.which)} onContextMenu={e => openDockMenu(e, props.which)}>
        <span class="wintitle">{props.which === 'rack' ? 'Channel rack' : 'Piano roll'}</span>
        <div style={{ flex: 1 }} />
        <WinControls dock={paneDock(props.which)} close={{ tip: t(props.which === 'rack' ? 'b.dk.closeRack' : 'b.dk.closeRoll'), act: props.which === 'rack' ? closeRack : closeRoll }} />
      </div>
      <div class="dockempty">
        <span>{props.msg}</span>
        <Show when={props.action}>{a => <button class="smallbtn" onClick={() => a().act()}>{a().label}</button>}</Show>
      </div>
    </div>
  );
}

const ARROW: Record<string, string> = { left: 'M15 6l-6 6 6 6', right: 'M9 6l6 6-6 6', up: 'M6 15l6-6 6 6', down: 'M6 9l6 6 6-6' };
/**
 * While a panel's title bar is dragged, only the dock slot under the pointer shows (the arrow and
 * the area the panel would take there): a plain move of a window covers no edges with guides — they
 * appear where a drop would dock. The full slot list stays in `guides()` for the hit test.
 */
export function DockGuides() {
  return (
    <Show when={guides()}>{gs => (
      <div class="dockguides" aria-hidden="true">
        <Show when={gs().hot}>{h => (
          <>
            <div class="dockpreview" style={{ left: h().preview.x + 'px', top: h().preview.y + 'px', width: h().preview.w + 'px', height: h().preview.h + 'px' }} />
            <div class="dockguide hot" style={{ left: h().x - 16 + 'px', top: h().y - 16 + 'px' }}>
              <svg viewBox="0 0 24 24" width="18" height="18"><path d={ARROW[h().dir]} fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
            </div>
          </>
        )}</Show>
      </div>
    )}</Show>
  );
}
