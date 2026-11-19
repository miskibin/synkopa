// The M / S pair, the same on a Playlist track and a Mixer strip (FL): click M = mute; a right-click,
// Ctrl+click or Alt+click on M solos; click S = solo (Ctrl+click adds to the soloed ones).
import { Show } from 'solid-js';

export function MuteSolo(props: {
  muted: boolean; solo: boolean; muteTip: string; soloTip: string;
  onMute: () => void; onSolo: (add: boolean) => void;
  /** No S button (the master). */
  noSolo?: boolean;
  /** Extra class on both buttons. */
  class?: string;
}) {
  const cls = () => (props.class ? ' ' + props.class : '');
  return (
    <>
      <button class={'mini m' + cls()} classList={{ on: props.muted }} title={props.muteTip}
        onPointerDown={e => { e.stopPropagation(); if (e.button === 2) { e.preventDefault(); props.onSolo(false); } }}
        onContextMenu={e => { e.preventDefault(); e.stopPropagation(); }}
        onClick={e => (e.ctrlKey || e.metaKey || e.altKey ? props.onSolo(false) : props.onMute())}>M</button>
      <Show when={!props.noSolo}>
        <button class={'mini s' + cls()} classList={{ on: props.solo }} title={props.soloTip}
          onPointerDown={e => e.stopPropagation()} onClick={e => props.onSolo(e.ctrlKey || e.metaKey)}>S</button>
      </Show>
    </>
  );
}
