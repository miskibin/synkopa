// The main window's work area: Browser, the docked Channel Rack and the Playlist in a row, the
// docked Piano roll under them (the Mixer follows below, in App). A docked panel has a splitter
// on its Playlist side; its size is the user's (`U.rackDockW` / `U.rollDockH`, saved in
// `wave.layout`) clamped so the Playlist keeps room (`fitDock`).
import { Show, type JSX } from 'solid-js';
import { t } from '../lib/i18n';
import { fitDock, PLAYLIST_MIN_H, PLAYLIST_MIN_W, ROLL_DOCK_MIN_H, SPLIT } from '../lib/layout';
import { channels, closeRack, closeRoll, curPat, newPattern, openRack, RACK_LEFT, RACK_MIN_W, RACK_PAD_X, selChan, STEP_MIN } from '../state/pattern';
import { rackDockFitWidth } from '../lib/rack';
import { stepsOf } from '../lib/pattern';
import { setU, U } from '../state/ui';
import { track } from '../ui/drag';
import { Browser } from './Browser';
import { ChannelRack } from './ChannelRack';
import { PianoRoll } from './PianoRoll';
import { Playlist } from './Playlist';
import { DockBtn, guardKeys, useSize } from './patternShared';

export function Workspace() {
  let col!: HTMLDivElement, row!: HTMLDivElement;
  const colSize = useSize(() => col), rowSize = useSize(() => row);
  const rackOn = () => U.rackDock && !!U.rackWin, rollOn = () => U.rollDock && !!U.rollWin;
  // the room beside the browser (and the rack's splitter); the Playlist keeps PLAYLIST_MIN_W of it
  const rackRoom = () => rowSize().w - (U.pickOpen ? U.pickW + SPLIT : 0) - SPLIT;
  // until the user drags the splitter the rack is as wide as the pattern's steps at the compact step width
  const autoW = () => { const pt = curPat(); return rackDockFitWidth(pt ? stepsOf(pt) : 16, { left: RACK_LEFT, cw: STEP_MIN, pad: 2 * RACK_PAD_X + 2, min: RACK_MIN_W }); };
  const rackW = (want = U.rackDockW || autoW()) => fitDock(want, RACK_MIN_W, rackRoom(), PLAYLIST_MIN_W);
  const rollH = (want = U.rollDockH) => fitDock(want, ROLL_DOCK_MIN_H, colSize().h - SPLIT, PLAYLIST_MIN_H);
  const rackSplit = (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation();
    const sx = e.clientX, w0 = rackW();
    track(ev => setU('rackDockW', rackW(w0 + ev.clientX - sx)), null, 'col-resize');
  };
  const rollSplit = (e: PointerEvent) => {
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation();
    const sy = e.clientY, h0 = rollH();
    track(ev => setU('rollDockH', rollH(h0 - (ev.clientY - sy))), null, 'row-resize');
  };
  return (
    // the Mix view (F9) covers this; it stays laid out (scroll, zoom and sizes are kept) but is not painted or reachable
    <div ref={col} class="workcol" classList={{ away: U.mixView }} inert={U.mixView} aria-hidden={U.mixView}>
      <div ref={row} class="workrow">
        <Show when={U.pickOpen}><Browser /></Show>
        <Show when={rackOn()}>
          <div class="dockpane rackdock" style={{ width: rackW() + 'px' }}>
            <Show when={curPat()} fallback={<DockEmpty which="rack" msg={t('b.dk.noPattern')} action={{ label: t('b.dk.newPattern'), act: () => { newPattern(); openRack(); } }} />}>
              <ChannelRack z={0} docked />
            </Show>
          </div>
          <div class="vsplit" onPointerDown={rackSplit} onDblClick={() => setU('rackDockW', 0)} title={t('b.dk.resizeRack')} />
        </Show>
        <Playlist />
      </div>
      <Show when={rollOn()}>
        <div class="docksplit" onPointerDown={rollSplit} title={t('b.dk.resizeRoll')} />
        <div class="dockpane rolldock" style={{ height: rollH() + 'px' }}>
          <Show when={curPat() && selChan()} fallback={<DockEmpty which="roll" msg={t(!curPat() ? 'b.dk.noPattern' : 'b.dk.noChannel')}
            action={!curPat() ? { label: t('b.dk.newPattern'), act: () => { newPattern(); openRack(); } } : channels().length ? undefined : { label: t('b.dk.openRack'), act: openRack }} />}>
            <PianoRoll z={0} docked />
          </Show>
        </div>
      </Show>
    </div>
  );
}

/** A docked panel with nothing to show (no pattern, no channel): says why and offers the way out; it can still float or hide. */
function DockEmpty(props: { which: 'rack' | 'roll'; msg: string; action?: { label: string; act: () => void } }): JSX.Element {
  return (
    <div tabIndex={0} class={'win patwin docked ' + (props.which === 'rack' ? 'rackwin' : 'rollwin')} onKeyDown={e => guardKeys(e)}>
      <div class="winhd">
        <span class="wintitle">{props.which === 'rack' ? 'Channel rack' : 'Piano roll'}</span>
        <div style={{ flex: 1 }} />
        <DockBtn which={props.which} />
        <button class="closebtn" title={t(props.which === 'rack' ? 'b.dk.closeRack' : 'b.dk.closeRoll')} onClick={props.which === 'rack' ? closeRack : closeRoll}>×</button>
      </div>
      <div class="dockempty">
        <span>{props.msg}</span>
        <Show when={props.action}>{a => <button class="smallbtn" onClick={() => a().act()}>{a().label}</button>}</Show>
      </div>
    </div>
  );
}
