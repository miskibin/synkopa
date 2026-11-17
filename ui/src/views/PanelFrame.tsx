// The one window frame (FL Studio's window model): every panel that can float has the same title
// bar — its icon and name, its own controls, then dock / maximize / close always in that order on
// the right — the same move (drag the title bar), the same resize grip and the same "click brings it
// to the front". Docked, the same frame is laid out in the main window (no move, no grip).
import type { JSX } from 'solid-js';
import { Show } from 'solid-js';
import { clamp } from '../lib/format';
import { t } from '../lib/i18n';
import { track } from '../ui/drag';
import { Icon } from '../ui/icons';
import { moveWindow } from './FxWindow';
import { guardKeys, useSize } from './patternShared';
import type { PaneId } from '../lib/dock';
import { openDockMenu, paneHeadDown } from '../state/docking';

export interface Geom { x: number; y: number; w: number; h: number }

const MAX_ICON = 'M5 5h14v14H5z M5 8h14';
const RESTORE_ICON = 'M8 9h11v10H8z M8 12h11 M5 15V5h11v4';

/** The controls every window ends its title bar with: dock / float, maximize / restore, close. */
export function WinControls(props: {
  dock?: { docked: boolean; toggle: () => void; tip: string; icon: string };
  max?: { on: boolean; toggle: () => void };
  close: { tip: string; act: () => void };
}) {
  return (
    <>
      <Show when={props.dock}>{d => (
        <button class="iconbtn dockbtn" title={d().tip} aria-label={d().tip} aria-pressed={d().docked} onClick={() => d().toggle()}>
          <Icon d={d().icon} size={13} />
        </button>
      )}</Show>
      <Show when={props.max}>{m => {
        const tip = () => t(m().on ? 'b.dk.restore' : 'b.dk.max');
        return (
          <button class="iconbtn dockbtn maxbtn" title={tip()} aria-label={tip()} aria-pressed={m().on} onClick={() => m().toggle()}>
            <Icon d={m().on ? RESTORE_ICON : MAX_ICON} size={13} />
          </button>
        );
      }}</Show>
      <button class="closebtn" title={props.close.tip} aria-label={props.close.tip} onClick={() => props.close.act()}>×</button>
    </>
  );
}

export function PanelFrame(props: {
  geom: () => Geom;
  /** A move or resize by the user (only the fields that changed are passed). */
  setGeom: (g: Partial<Geom>) => void;
  z: number; class: string; minW: number; minH: number; docked?: boolean;
  /** A dockable panel: its title bar drags it onto the dock guides (docked too) and right-clicks to its dock menu. */
  dockId?: PaneId;
  front: () => void; onKey?: (e: KeyboardEvent) => void;
  /** Left part of the title bar: the window's icon, name and its own controls. */
  head: JSX.Element;
  /** Right end of the title bar: a `WinControls`. */
  controls: JSX.Element;
  /** Below this height (px) the window hides its status / hint footer (`.pstatus`). */
  compactH?: number;
  /** Wheel over the title bar. */
  onHeadWheel?: (e: WheelEvent) => void;
  /** Double-click on the empty part of the title bar (maximize / restore where the window has it). */
  onHeadDbl?: () => void;
  children: JSX.Element;
}) {
  let root!: HTMLDivElement;
  const size = useSize(() => root);
  const compact = () => props.compactH != null && size().h > 0 && size().h < props.compactH;
  const put = (g: Partial<Geom>) => {
    const o = props.geom();
    if (Object.entries(g).every(([k, v]) => o[k as keyof Geom] === v)) return;
    props.setGeom(g);
  };
  const resize = (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation();
    const sx = e.clientX, sy = e.clientY, o = { ...props.geom() };
    track(ev => put({ w: clamp(o.w + ev.clientX - sx, props.minW, window.innerWidth - 20), h: clamp(o.h + ev.clientY - sy, props.minH, window.innerHeight - 20) }), null, 'nwse-resize');
  };
  return (
    <div ref={root} tabIndex={0} class={'win patwin ' + props.class} classList={{ docked: !!props.docked, compact: compact() }}
      style={props.docked ? undefined : { left: props.geom().x + 'px', top: props.geom().y + 'px', width: props.geom().w + 'px', height: props.geom().h + 'px', 'z-index': props.z }}
      // to the front in the capture phase, before the contents act on the press: a channel name or a
      // mini roll opens its window on pointerdown, and raising this one afterwards covered it
      on:pointerdown={{ capture: true, handleEvent: () => { if (!props.docked) props.front(); } }}
      onPointerDown={e => { const tg = (e.target as HTMLElement).tagName; if (tg !== 'SELECT' && tg !== 'INPUT') root.focus({ preventScroll: true }); }}
      onKeyDown={e => guardKeys(e, props.onKey)}>
      <div class="winhd" onPointerDown={e => { if (props.dockId) paneHeadDown(e, props.dockId); else if (!props.docked) moveWindow(e, () => props.geom(), (x, y) => put({ x, y })); }} onWheel={e => props.onHeadWheel?.(e)}
        onContextMenu={e => { if (props.dockId) openDockMenu(e, props.dockId); }}
        onDblClick={e => { if (!props.docked && !(e.target as HTMLElement).closest('button, select, input, .numf, .seg')) props.onHeadDbl?.(); }}>
        {props.head}
        <div style={{ flex: 1 }} />
        {props.controls}
      </div>
      {props.children}
      {!props.docked && <div class="resizer" onPointerDown={resize} title={t('b.pe.resize')} />}
    </div>
  );
}
