// The song's key as a badge (toolbar and Piano roll): a tiny keyboard marks the scale's notes (root
// brightest), the name beside it; a click opens the root and scale pickers. One key for the whole
// project. With no key known it says so — the keyboard dims and the picker offers detection.
import { createMemo, For, Show } from 'solid-js';
import { createSignal, effect, onCleanup } from '../lib/reactive';
import { t } from '../lib/i18n';
import { NOTE_NAMES } from '../lib/pitch';
import { SCALES, scaleMask } from '../lib/pattern';
import { commit } from '../state/edit';
import { S } from '../state/store';
import { keyName, setKeyOp, songKey } from '../state/songKey';
import * as A from '../state/actions';

const WHITE = [0, 2, 4, 5, 7, 9, 11];
const BLACK: [number, number][] = [[1, 0], [3, 1], [6, 3], [8, 4], [10, 5]];

/** The tiny keyboard: each pitch class lit by its role in the key (`none` = no key known: all plain). */
export function MiniKeys(props: { root: number; scale: number; known: boolean }) {
  const cls = (pc: number) => {
    if (!props.known) return 'out';
    if (pc === props.root) return 'root';
    return (scaleMask(props.scale) >> ((pc - props.root + 12) % 12)) & 1 ? 'in' : 'out';
  };
  return (
    <svg class="minikeys" width="43" height="16" viewBox="0 0 43 16" aria-hidden="true">
      <For each={WHITE}>{(pc, i) => <rect x={i() * 6 + 0.5} y="0.5" width="5" height="15" rx="1" class={cls(pc)} />}</For>
      <For each={BLACK}>{([pc, i]) => <rect x={i * 6 + 4} y="0" width="4" height="9" rx="1" class={'bk ' + cls(pc)} />}</For>
    </svg>
  );
}

/**
 * The badge and its picker. `onPicked` runs after a root or scale is chosen (the Piano roll gives
 * the keyboard back to its grid). The picker is placed under the badge in the window's coordinates,
 * so a toolbar or title bar that clips its children does not cut it off.
 */
export function KeyBadge(props: { class?: string; onPicked?: (el: HTMLElement) => void }) {
  const k = createMemo(songKey, { equals: (a, b) => a.root === b.root && a.scale === b.scale && a.from === b.from });
  const [open, setOpen] = createSignal<{ x: number; y: number } | null>(null);
  let box!: HTMLDivElement;
  effect(() => {
    if (!open()) return;
    const away = (e: PointerEvent) => { if (!box.contains(e.target as Node)) setOpen(null); };
    // Esc dismisses the picker no matter where the keyboard lives (after a pick the roll's grid
    // has it, and the window's own Escape clears the note selection): capture here, first
    const esc = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault(); e.stopPropagation();
      setOpen(null);
    };
    document.addEventListener('pointerdown', away, true);
    document.addEventListener('keydown', esc, true);
    onCleanup(() => {
      document.removeEventListener('pointerdown', away, true);
      document.removeEventListener('keydown', esc, true);
    });
  });
  const toggle = (el: HTMLElement) => {
    if (open()) return setOpen(null);
    const r = el.getBoundingClientRect();
    setOpen({ x: Math.max(8, Math.min(r.left, window.innerWidth - 280)), y: r.bottom + 6 });
  };
  const pick = (root: number, scale: number, el: HTMLElement) => {
    commit([setKeyOp(root, scale)], t('b.pr.keySet'));
    props.onPicked?.(el);
  };
  const tip = () => k().from === 'set' ? t('b.pr.songKeyTip') : k().from === 'tune' ? t('b.pe.keyAuto') : t('b.sk.noneTip');
  const hasAudio = () => S.project.clips.some(c => c.kind === 'audio');
  return (
    <div class={'keybox ' + (props.class ?? '')} ref={box}>
      <button class={['keybadge', { on: !!open(), none: k().from === 'none' }]} aria-haspopup="dialog" aria-expanded={open() ? 'true' : 'false'} title={tip()}
        onClick={e => toggle(e.currentTarget)}>
        <MiniKeys root={k().root} scale={k().scale} known={k().from !== 'none'} />
        <span class="kname">{k().from === 'none' ? t('b.sk.none') : keyName(k())}</span>
        <Show when={k().from === 'tune'}><span class="kauto">auto</span></Show>
        <svg width="8" height="8" viewBox="0 0 8 8"><path d="M1 2.5l3 3 3-3" fill="none" stroke="currentColor" stroke-width="1.3" /></svg>
      </button>
      <Show when={open()}>{pos => (
        <div class="toolpop keypop" role="dialog" aria-label={t('b.pr.songKey')} style={{ position: 'fixed', left: pos().x + 'px', top: pos().y + 'px', right: 'auto' }}>
          <div class="tphd">{t('b.pr.songKey')}</div>
          <div class="rootgrid" role="radiogroup">
            <For each={NOTE_NAMES}>{(n, i) => (
              <button role="radio" aria-checked={k().from !== 'none' && k().root === i() ? 'true' : 'false'} class={{ on: k().from !== 'none' && k().root === i(), bk: n.length > 1 }}
                onClick={e => pick(i(), k().scale, e.currentTarget)}>{n}</button>
            )}</For>
          </div>
          <div class="scalelist" role="radiogroup">
            <For each={SCALES}>{([l, id]) => (
              <button role="radio" aria-checked={k().from !== 'none' && k().scale === id ? 'true' : 'false'} class={{ on: k().from !== 'none' && k().scale === id }}
                onClick={e => pick(k().root, id, e.currentTarget)}>{t(l)}</button>
            )}</For>
          </div>
          <Show when={k().from !== 'set' && hasAudio()}>
            <button class="smallbtn" onClick={() => { setOpen(null); A.openMusicAnalysis({}); }}>{t('a.tb.detectProj')}</button>
          </Show>
          <Show when={k().from === 'set'}>
            <button class="smallbtn" title={t('b.sk.clearTip')} onClick={() => { commit([setKeyOp(null, k().scale)], t('b.sk.cleared')); setOpen(null); }}>{t('b.sk.clear')}</button>
          </Show>
          <div class="tpnote">{t('b.pr.songKeyNote')}</div>
        </div>
      )}</Show>
    </div>
  );
}
