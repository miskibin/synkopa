// A piano keyboard you can click, like the keyboard at the bottom of FL Studio's Channel settings.
// Left button plays: a key sounds while it is held; lower on a key = harder hit; drag across keys to
// slide. Right button on a key makes it the root note (the key at which a sample plays at its own
// pitch), when the owner allows it; the root key is tinted and named. Keys held on the MIDI keyboard
// or the typing keyboard light up too.
import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { be } from '../api/backend';
import type { Id } from '../api/types';
import { t } from '../lib/i18n';
import { isBlack, noteName } from '../lib/pitch';
import { typingHeld } from '../state/typingKeys';
import { say } from '../state/ui';

/** How a key is heard: a Channel Rack channel (`ch`, through the typing-keyboard path), or the owner's own `play`. */
export type KeyPlayer = { ch: Id } | { play: (key: number, vel: number, on: boolean) => Promise<void> | void };

export function PianoKeys(props: {
  player: KeyPlayer; lo: number; hi: number;
  /** The root key, tinted and named; the keyboard grows to show it. */
  root?: number;
  /** Right-click on a key makes it the root. Without it right-click does nothing. */
  onRoot?: (key: number) => void;
}) {
  let el!: HTMLDivElement;
  const [down, setDown] = createSignal<number | null>(null);
  const [held, setHeld] = createSignal<ReadonlySet<number>>(new Set());
  // whole white keys at both ends, and the root always on the keyboard
  const lo = () => { const k = Math.min(props.lo, props.root ?? props.lo); return isBlack(k) ? k - 1 : k; };
  const hi = () => { const k = Math.max(props.hi, props.root ?? props.hi); return isBlack(k) ? k + 1 : k; };
  const whites = () => { const w: number[] = []; for (let k = lo(); k <= hi(); k++) if (!isBlack(k)) w.push(k); return w; };
  const blacks = () => { const b: { key: number; at: number }[] = []; whites().forEach((k, i) => { if (k + 1 <= hi() && isBlack(k + 1)) b.push({ key: k + 1, at: i + 1 }); }); return b; };

  /** Key and velocity under a pointer. */
  const hit = (e: PointerEvent | MouseEvent): { key: number; vel: number } | null => {
    const r = el.getBoundingClientRect(), ws = whites();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    if (x < 0 || x >= r.width || !ws.length) return null;
    const ww = r.width / ws.length, i = Math.min(ws.length - 1, Math.floor(x / ww));
    const vel = (h: number) => Math.round((0.35 + 0.65 * Math.min(1, Math.max(0, y / h))) * 100) / 100;
    const bh = r.height * 0.6;
    if (y < bh) {
      const near = Math.round(x / ww), b = blacks().find(q => q.at === near);
      if (b && Math.abs(x - near * ww) < ww * 0.32) return { key: b.key, vel: vel(bh) };
    }
    return { key: ws[i], vel: vel(r.height) };
  };
  const send = async (key: number, vel: number, on: boolean) => {
    const p = props.player;
    try {
      if ('ch' in p) await be().typingNote({ ch: p.ch, key, vel, on });
      else await p.play(key, vel, on);
    } catch (err) { if (on) say(t('b.pk.fail', { err: String(err) })); }
  };
  const release = () => { const k = down(); if (k != null) { setDown(null); void send(k, 0, false); } };
  const play = (e: PointerEvent) => {
    const h = hit(e);
    if (!h || h.key === down()) return;
    release();
    setDown(h.key);
    void send(h.key, h.vel, true);
  };
  const menu = (e: MouseEvent) => {
    e.preventDefault();
    const h = hit(e);
    if (h && props.onRoot && h.key !== props.root) props.onRoot(h.key);
  };

  onMount(() => {
    const off = be().on('midi-note', ev => setHeld(s => { const n = new Set(s); if (ev.note.on) n.add(ev.note.key); else n.delete(ev.note.key); return n; }));
    onCleanup(() => { release(); void off.then(un => un()); });
  });

  const lit = (k: number) => down() === k || held().has(k) || typingHeld().includes(k);
  const isRoot = (k: number) => props.root === k;
  return (
    <div ref={el} class="pkeys" role="group" aria-label={t('b.pk.aria')} title={t(props.onRoot ? 'b.pk.titleRoot' : 'b.pk.title')}
      onPointerDown={e => { if (e.button !== 0) return; el.setPointerCapture(e.pointerId); play(e); }}
      onPointerMove={e => { if (down() != null) play(e); }}
      onPointerUp={release} onPointerCancel={release} onLostPointerCapture={release}
      onContextMenu={menu}>
      <For each={whites()}>{(k, i) => (
        <div class="w" classList={{ on: lit(k), root: isRoot(k) }} style={{ left: (i() / whites().length) * 100 + '%', width: 100 / whites().length + '%' }}>
          <Show when={isRoot(k) || (k % 12 === 0 && (props.root == null || Math.abs(k - props.root) > 2))}>
            <span class="lbl">{noteName(k)}</span>
          </Show>
        </div>
      )}</For>
      <For each={blacks()}>{b => (
        <div class="b" classList={{ on: lit(b.key), root: isRoot(b.key) }} style={{ left: 'calc(' + (b.at / whites().length) * 100 + '% - ' + (32 / whites().length) + '%)', width: 64 / whites().length + '%' }} />
      )}</For>
    </div>
  );
}
