// The command palette (Ctrl+K): every menu command, found by typing what you want to do instead of
// knowing which menu holds it. It reads the app menus themselves, so a command added to a menu is
// here too, with the same shortcut, the same disabled state and the same action.
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js';
import { matchCommand } from '../lib/commandMatch';
import { t, type TranslationKey } from '../lib/i18n';
import type { MenuItem } from '../state/ui';
import { Icon } from '../ui/icons';

type MenuSource = [TranslationKey, () => MenuItem[]][];
let source: MenuSource = [];
/** The app bar's menus, registered by the menu bar when it mounts. */
export const setCommandMenus = (m: MenuSource) => { source = m; };
export const [paletteOpen, setPaletteOpen] = createSignal(false);

interface Cmd { path: string; label: string; kbd?: string; disabled?: boolean; act: () => void }

function flatten(items: MenuItem[], path: string, out: Cmd[]) {
  for (const it of items) {
    if (it.t === 'item') out.push({ path, label: it.label, kbd: it.kbd, disabled: it.disabled, act: it.act });
    else if (it.t === 'sub') flatten(it.items, path + ' › ' + it.label, out);
  }
}

export function Palette() {
  const [q, setQ] = createSignal('');
  const [at, setAt] = createSignal(0);
  let list!: HTMLDivElement;
  // read the menus when the palette opens: disabled states are those of this moment
  const all = createMemo(() => { const out: Cmd[] = []; if (paletteOpen()) for (const [name, items] of source) flatten(items(), t(name), out); return out; });
  const shown = createMemo(() => all().filter(c => matchCommand(c.path + ' ' + c.label, q())));
  createEffect(() => { if (!paletteOpen()) setQ(''); });
  createEffect(() => { q(); setAt(0); });
  const close = () => setPaletteOpen(false);
  const run = (c: Cmd | undefined) => { if (!c || c.disabled) return; close(); c.act(); };
  const move = (d: number) => {
    const n = shown().length; if (!n) return;
    setAt(i => (i + d + n) % n);
    queueMicrotask(() => list?.querySelector('.on')?.scrollIntoView({ block: 'nearest' }));
  };
  const key = (e: KeyboardEvent) => {
    // the palette owns the keyboard while it is open: no app shortcut fires from what is typed here
    e.stopPropagation();
    if (e.key === 'Escape') { e.preventDefault(); if (q()) setQ(''); else close(); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); move(1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); move(-1); }
    else if (e.key === 'Enter') { e.preventDefault(); run(shown()[at()]); }
    else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); close(); }
  };
  return (
    <Show when={paletteOpen()}>
      <div class="overlay cmdover" onPointerDown={close}>
        <div class="dialog cmddlg" role="dialog" aria-modal="true" aria-label={t('a.cmd.title')} onPointerDown={e => e.stopPropagation()} onKeyDown={key}>
          <div class="cmdfind">
            <Icon d="M11 4a7 7 0 1 0 0 14a7 7 0 0 0 0-14z M20 20l-4-4" size={16} />
            <input type="search" ref={el => queueMicrotask(() => el.focus())} value={q()} onInput={e => setQ(e.currentTarget.value)}
              placeholder={t('a.cmd.placeholder')} aria-label={t('a.cmd.title')} spellcheck={false} />
            <kbd>Esc</kbd>
          </div>
          <div class="cmdlist" ref={list} role="listbox">
            <For each={shown()} fallback={<div class="cmdnone">{t('a.cmd.none', { q: q() })}</div>}>
              {(c, i) => (
                <div class="cmd" role="option" aria-selected={at() === i()} aria-disabled={c.disabled} classList={{ on: at() === i(), dis: !!c.disabled }}
                  onPointerMove={() => setAt(i())} onClick={() => run(c)}>
                  <span class="path">{c.path}</span>
                  <span class="name ell">{c.label}</span>
                  <Show when={c.kbd}><kbd>{c.kbd}</kbd></Show>
                </div>
              )}
            </For>
          </div>
        </div>
      </div>
    </Show>
  );
}
