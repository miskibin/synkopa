// The main window's work area (#90): the Browser, then the docked panels around the Playlist —
// strips on the left and right edges beside it, strips at the bottom under everything — laid out
// from `U.dock` (`lib/dock.ts`). A strip has a splitter on its Playlist side (its thickness is its
// first panel's, `U.paneSize`, clamped by `fitStrips` so the Playlist keeps room) and splitters
// between the panels sharing it. While a title bar is dragged the dock guides show (`DockGuides`).
import { createMemo, For, Show } from 'solid-js';
import type { JSX } from '@solidjs/web';
import { captureOn } from '../lib/reactive';
import { t } from '../lib/i18n';
import { clamp } from '../lib/format';
import { PLAYLIST_MIN_H, PLAYLIST_MIN_W, SPLIT } from '../lib/layout';
import { dim, fitStrips, PANE_MIN, PANE_MIN_LEN, side, splitShares, type DockPane, type Edge, type PaneId } from '../lib/dock';
import { channels, closeRack, closeRoll, curPat, newPattern, openRack, RACK_LEFT, RACK_MIN_W, RACK_PAD_X, selChan, stepW } from '../state/pattern';
import { rackDockFitWidth } from '../lib/rack';
import { stepsOf } from '../lib/pattern';
import { setU, U } from '../state/ui';
import { tuckOnClick } from '../state/actions';
import { guides, openDockMenu, paneDock, paneHeadDown, paneOpen } from '../state/docking';
import { track } from '../ui/drag';
import { Browser } from './Browser';
import { ChannelRack } from './ChannelRack';
import { MixDockPane } from './Mixer';
import { PianoRoll } from './PianoRoll';
import { Playlist } from './Playlist';
import { guardKeys, useSize } from './patternShared';
import { WinControls } from './PanelFrame';
import { PanelBoundary, WindowBoundary } from '../ui/PanelBoundary';
import { hideMixer } from '../state/mixView';

/** A strip as laid out: its edge and index and its panels shown (with their index in the strip). A strip with none shown takes no room. */
interface StripView { edge: Edge; si: number; panes: { p: DockPane; at: number }[] }

export function Workspace() {
  let col!: HTMLDivElement, row!: HTMLDivElement;
  const colSize = useSize(() => col), rowSize = useSize(() => row);
  // until the user drags its splitter, a rack docked on a side is as wide as the pattern's steps
  const autoW = () => { const pt = curPat(); return rackDockFitWidth(pt ? stepsOf(pt) : 16, { left: RACK_LEFT, cw: stepW(), pad: 2 * RACK_PAD_X + 2, min: RACK_MIN_W }); };

  const views = (edge: Edge): StripView[] => U.dock[edge].map((s, si) => {
    const panes = s.map((p, at) => ({ p, at })).filter(x => paneOpen(x.p.id));
    // a hidden panel (the Mixer included) leaves no trace, like a closed Channel Rack or Piano roll
    return { edge, si, panes };
  }).filter(v => v.panes.length);
  const left = createMemo(() => views('left')), right = createMemo(() => views('right')), bottom = createMemo(() => views('bottom'));

  const want = (v: StripView) => { const id = v.panes[0].p.id, d = dim(v.edge), w = U.paneSize[id][d]; return id === 'rack' && d === 'w' && !w ? autoW() : w; };
  const minOf = (v: StripView) => Math.max(...v.panes.map(x => PANE_MIN[x.p.id][dim(v.edge)]));
  // the side strips share the row beside the Browser, the bottom strips the column's height; each splitter takes its own
  const sideAvail = () => rowSize().w - (U.pickOpen ? U.pickW + SPLIT : 0) - (left().length + right().length) * SPLIT;
  const bottomAvail = () => colSize().h - bottom().length * SPLIT;
  const sideFit = createMemo(() => { const vs = [...left(), ...right()]; return { vs, px: fitStrips(vs.map(want), vs.map(minOf), rowSize().w > 0 ? sideAvail() : 0, PLAYLIST_MIN_W) }; });
  const bottomFit = createMemo(() => { const vs = bottom(); return { vs, px: fitStrips(vs.map(want), vs.map(minOf), colSize().h > 0 ? bottomAvail() : 0, PLAYLIST_MIN_H) }; });
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
    // past what the Playlist can give, the strips between it and this one give way, the nearest first
    // (dragging the splitter between two docked panels trades their sizes, as in FL Studio)
    const f = fitOf(v), inner = f.vs.map((x, i) => ({ x, px: f.px[i] })).filter(o => o.x.edge === v.edge && o.x.si < v.si).reverse()
      .map(o => ({ id: o.x.panes[0].p.id, px: o.px, min: minOf(o.x) }));
    const spare = inner.reduce((a, o) => a + Math.max(0, o.px - o.min), 0);
    let lent = false; // the inner strips keep their wanted sizes untouched until the drag needs them
    const borrow = (over: number) => {
      if (over <= 0 && !lent) return;
      lent = true;
      let left = clamp(over, 0, spare);
      for (const o of inner) { const take = Math.min(left, Math.max(0, o.px - o.min)); setSize(o.id, d, o.px - take); left -= take; }
    };
    track(ev => {
      const g = grow(v.edge, ev.clientX - sx, ev.clientY - sy);
      if (inner.length) borrow(s0 + g - Math.max(min, max));
      setSize(id, d, clamp(s0 + g, min, Math.max(min, max) + spare));
    }, null, side(v.edge) ? 'col-resize' : 'row-resize');
  };
  const stripKey = (v: StripView) => (e: KeyboardEvent) => {
    const id = v.panes[0].p.id, d = dim(v.edge), step = e.shiftKey ? 64 : 16;
    const inward = { left: 'ArrowRight', right: 'ArrowLeft', bottom: 'ArrowUp' }[v.edge], outward = { left: 'ArrowLeft', right: 'ArrowRight', bottom: 'ArrowDown' }[v.edge];
    if (e.key === inward || e.key === outward) {
      e.preventDefault(); e.stopPropagation();
      setSize(id, d, clamp(sizeOf(v) + (e.key === inward ? step : -step), minOf(v), Math.max(minOf(v), maxOf(v))));
    }
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
  const keyOf = (v: StripView) => `${v.edge}:${v.si}`;
  const byKey = createMemo(() => new Map([...left(), ...right(), ...bottom()].map(v => [keyOf(v), v])));
  const keys = (vs: StripView[]) => vs.map(keyOf);
  const Strip = (p: { k: string; splitFirst: boolean }) => (
    <Show when={byKey().get(p.k)}>{v => (() => {
          const split = (
            <div class={side(v().edge) ? 'vsplit' : 'docksplit'} role="separator" aria-orientation={side(v().edge) ? 'vertical' : 'horizontal'} tabindex={0}
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
        })()}</Show>
  );

  return (
    // the Mix view (F9) covers this; it stays laid out (scroll, zoom and sizes are kept) but is not painted or reachable
    // a plain click in it (docked panels, Playlist; not the Browser) sends the floating windows behind it
    <div ref={[e => (col = e), captureOn('pointerdown', tuckOnClick)]} class={['workcol', { away: U.mixView }]} inert={U.mixView} aria-hidden={U.mixView ? 'true' : undefined}>
      <div ref={row} class="workrow">
        <Show when={U.pickOpen}><Browser /></Show>
        {/* left strips: the outermost first, each with its splitter on the Playlist side */}
        <For each={keys(left()).reverse()}>{k => <Strip k={k} splitFirst={false} />}</For>
        <PanelBoundary name={t('d.ui.pn.playlist')}><Playlist /></PanelBoundary>
        <For each={keys(right())}>{k => <Strip k={k} splitFirst />}</For>
      </div>
      <For each={keys(bottom())}>{k => <Strip k={k} splitFirst />}</For>
    </div>
  );
}

/** What a docked panel shows: the Channel Rack, the Piano roll (or why it cannot) or the Mixer. */
function PaneBody(props: { id: PaneId }): JSX.Element {
  if (props.id === 'mix') {
    return (
      <WindowBoundary name={t('c.mv.mixer')} class="mixdock" docked close={hideMixer} closeTip={t('c.mv.hideTip')}>
        <MixDockPane />
      </WindowBoundary>
    );
  }
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
    <div tabindex={0} class={'win patwin docked ' + (props.which === 'rack' ? 'rackwin' : 'rollwin')} onKeyDown={e => guardKeys(e)}>
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
 * While a panel's title bar is dragged, every dock slot shows as a small, faint arrow, so you can
 * see where the panel could dock without the edges being covered (issue #136); the one under the
 * pointer lights up with the area the panel would take there.
 */
export function DockGuides() {
  return (
    <Show when={guides()}>{gs => (
      <div class="dockguides" aria-hidden="true">
        <Show when={gs().hot}>{h => <div class="dockpreview" style={{ left: h().preview.x + 'px', top: h().preview.y + 'px', width: h().preview.w + 'px', height: h().preview.h + 'px' }} />}</Show>
        <For each={gs().list}>{g => (
          <div class={['dockguide', { hot: gs().hot === g, faint: gs().hot !== g }]} style={{ left: g.x - 16 + 'px', top: g.y - 16 + 'px' }}>
            <svg viewBox="0 0 24 24" width="18" height="18"><path d={ARROW[g.dir]} fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
          </div>
        )}</For>
      </div>
    )}</Show>
  );
}
