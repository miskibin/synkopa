// FX chain presets (issue #70): save a mixer insert's effects by name to the user's library and
// load them onto any insert of any project. Loading onto an insert that has effects asks
// Replace (the default) or Append; either is one undo step and never touches volume, pan,
// name, colour or routing. The list is searchable; rename (F2) and delete (Del) work in place.
import { createEffect, createMemo, createSignal, For, on, Show } from 'solid-js';
import { be } from '../api/backend';
import type { FxChainPreset, Id } from '../api/types';
import { t } from '../lib/i18n';
import { enqueue, resync } from '../state/edit';
import { insName, S } from '../state/store';
import { openMenu, openRename, say, setU, U } from '../state/ui';
import { FxTile } from '../ui/icons';

const errText = (e: unknown) => String(e).replace(/^Error: /, '');

/** Insert menu → "Save FX chain as preset…": asks for a name (the insert's by default). */
export function saveFxChain(insId: Id, x: number, y: number) {
  const m = S.project.inserts.find(i => i.id === insId);
  if (!m?.fx.length) return;
  const save = async (name: string, overwrite: boolean) => {
    try {
      await enqueue(() => Promise.resolve());
      const r = await be().fxChainSave(insId, name, overwrite);
      say(t('e.fxc.saved', { name: r.name, n: r.effects }));
    } catch (e) {
      const msg = errText(e);
      if (msg.startsWith('EXISTS:')) {
        openMenu(x, y, [
          { t: 'title', label: t('e.fxc.exists', { name: name.trim() }) },
          { t: 'item', label: t('e.fxc.overwrite'), danger: true, act: () => void save(name, true) },
          { t: 'item', label: t('e.fxc.keep'), act: () => {} },
        ], 120);
      } else say(t('e.fxc.fail', { err: msg }));
    }
  };
  openRename(x, y, m.id === 0 ? 'Master chain' : insName(m), name => void save(name, false), t('e.fxc.saveTitle'));
}

/** Insert menu → "Load FX chain preset…". */
export function openFxChains(insId: Id, x: number, y: number) {
  setU('fxChainWin', { insId, x, y });
}

export function FxChainsHost() {
  return <Show when={U.fxChainWin}>{w => <FxChainLoader insId={w().insId} x={w().x} y={w().y} />}</Show>;
}

const W = 400, H = 440;

function FxChainLoader(props: { insId: Id; x: number; y: number }) {
  const [list, setList] = createSignal<FxChainPreset[] | null>(null);
  const [dir, setDir] = createSignal('');
  const [err, setErr] = createSignal<string | null>(null);
  const [q, setQ] = createSignal('');
  const [sel, setSel] = createSignal(0);
  const [busy, setBusy] = createSignal(false);
  const [renaming, setRenaming] = createSignal<string | null>(null);
  const [deleting, setDeleting] = createSignal<string | null>(null);
  let search: HTMLInputElement | undefined;
  const close = () => setU('fxChainWin', null);
  const ins = () => S.project.inserts.find(i => i.id === props.insId);
  const hasFx = () => (ins()?.fx.length ?? 0) > 0;

  const refresh = async () => {
    try {
      const r = await be().fxChains();
      setList(r.presets); setDir(r.dir); setErr(null);
    } catch (e) { setList([]); setErr(errText(e)); }
  };
  void refresh();
  // the insert went away (undo, deleted): nothing to load onto
  createEffect(() => { if (!ins()) close(); });

  const shown = createMemo(() => {
    const words = q().toLowerCase().split(/\s+/).filter(Boolean);
    return (list() ?? []).filter(p => {
      const hay = (p.name + ' ' + p.effects.map(e => e.name).join(' ')).toLowerCase();
      return words.every(w => hay.includes(w));
    });
  });
  createEffect(on(shown, s => setSel(i => Math.min(i, Math.max(0, s.length - 1)))));
  const cur = () => shown()[sel()];

  const load = async (p: FxChainPreset | undefined, mode: 'replace' | 'append') => {
    if (!p || p.error || busy()) return;
    setBusy(true);
    try {
      await enqueue(() => Promise.resolve());
      const r = await be().fxChainLoad(props.insId, p.name, hasFx() ? mode : undefined);
      await resync();
      close();
      const name = ins() ? insName(ins()!) : '';
      let msg = t(r.replaced ? 'e.fxc.loadedReplace' : 'e.fxc.loaded', { name: p.name, n: r.fxIds.length, ins: name });
      if (r.failed.length) msg += ' · ' + t('e.fxc.failedPlugins', { names: r.failed.map(f => f.name).join(', ') });
      say(msg);
    } catch (e) { setErr(errText(e)); }
    finally { setBusy(false); }
  };

  const rename = async (from: string, to: string) => {
    setRenaming(null);
    if (!to.trim() || to.trim() === from) return;
    try {
      const r = await be().fxChainRename(from, to);
      await refresh();
      setSel(Math.max(0, shown().findIndex(p => p.name === r.name)));
    } catch (e) { setErr(errText(e).replace(/^EXISTS: /, '')); }
    search?.focus();
  };
  const remove = async (name: string) => {
    setDeleting(null);
    try { await be().fxChainDelete(name); await refresh(); say(t('e.fxc.deleted', { name })); }
    catch (e) { setErr(errText(e)); }
    search?.focus();
  };

  const key = (e: KeyboardEvent) => {
    if (renaming() || deleting()) return;
    e.stopPropagation();
    if (e.key === 'Escape') { e.preventDefault(); close(); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); setSel(i => Math.min(i + 1, shown().length - 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setSel(i => Math.max(i - 1, 0)); }
    else if (e.key === 'Enter') { e.preventDefault(); void load(cur(), e.shiftKey ? 'append' : 'replace'); }
    else if (e.key === 'F2' && cur()) { e.preventDefault(); setRenaming(cur()!.name); }
    else if (e.key === 'Delete' && cur() && !q()) { e.preventDefault(); setDeleting(cur()!.name); }
  };

  const pos = () => {
    const vw = window.innerWidth, vh = window.innerHeight, m = 12;
    const w = Math.min(W, vw - 2 * m), h = Math.min(H, vh - 2 * m);
    return { left: Math.min(Math.max(m, props.x - 24), vw - w - m), top: Math.min(Math.max(m, props.y - h - 8 > m ? props.y - h - 8 : props.y + 8), vh - h - m), w, h };
  };

  return (
    <div class="fxpick-veil" onPointerDown={close}>
      <div class="fxpick fxcl" role="dialog" aria-label={t('e.fxc.loadTitle', { ins: ins() ? insName(ins()!) : '' })}
        style={{ left: pos().left + 'px', top: pos().top + 'px', width: pos().w + 'px', 'max-height': pos().h + 'px' }}
        onPointerDown={e => e.stopPropagation()} onKeyDown={key}>
        <div class="fxcl-hd">
          <span class="ell" title={dir()}>{t('e.fxc.loadTitle', { ins: ins() ? insName(ins()!) : '' })}</span>
          <button onClick={close} aria-label={t('e.fxc.close')}>×</button>
        </div>
        <input ref={el => { search = el; queueMicrotask(() => el.focus()); }} class="fxcl-q" placeholder={t('e.fxc.search')} value={q()}
          onInput={e => { setQ(e.currentTarget.value); setSel(0); }} aria-label={t('e.fxc.search')} />
        <div class="fxcl-list" role="listbox" aria-label={t('e.fxc.list')}>
          <Show when={list()} fallback={<div class="fxcl-empty"><span class="spin" /></div>}>
            <Show when={list()!.length} fallback={<div class="fxcl-empty">{t('e.fxc.none')}</div>}>
              <Show when={shown().length} fallback={<div class="fxcl-empty">{t('e.fxc.noMatch', { q: q() })}</div>}>
                <For each={shown()}>{(p, i) => (
                  <div class="fxcl-row" role="option" aria-selected={i() === sel()} classList={{ on: i() === sel(), bad: !!p.error }}
                    onPointerEnter={() => setSel(i())} onClick={() => { if (!renaming() && !deleting() && !hasFx()) void load(p, 'replace'); else setSel(i()); }}
                    onDblClick={() => void load(p, 'replace')} title={p.error ?? p.effects.map(e => e.name).join(' → ')}>
                    <Show when={renaming() === p.name} fallback={
                      <div class="fxcl-main">
                        <span class="fxcl-name ell">{p.name}</span>
                        <span class="fxcl-fx ell">{p.error ?? (t('e.fxc.count', { n: p.effects.length }) + ' · ' + p.effects.map(e => e.name).join(' · '))}</span>
                      </div>
                    }>
                      <input class="fxcl-ren" value={p.name} ref={el => queueMicrotask(() => { el.focus(); el.select(); })}
                        onKeyDown={e => { e.stopPropagation(); if (e.key === 'Enter') void rename(p.name, e.currentTarget.value); if (e.key === 'Escape') { setRenaming(null); search?.focus(); } }}
                        onBlur={e => void rename(p.name, e.currentTarget.value)} aria-label={t('e.fxc.rename')} />
                    </Show>
                    <Show when={!p.error && renaming() !== p.name}>
                      <span class="fxcl-tiles">
                        <For each={p.effects.slice(0, 5)}>{e => <FxTile type={e.type} size={16} />}</For>
                        <Show when={p.effects.length > 5}><span class="note">+{p.effects.length - 5}</span></Show>
                      </span>
                    </Show>
                    <Show when={deleting() === p.name} fallback={
                      <span class="fxcl-acts">
                        <button title={t('e.fxc.rename') + ' (F2)'} onClick={e => { e.stopPropagation(); setRenaming(p.name); }}>{t('e.fxc.renameShort')}</button>
                        <button class="danger" title={t('e.fxc.delete') + ' (Del)'} onClick={e => { e.stopPropagation(); setDeleting(p.name); }}>×</button>
                      </span>
                    }>
                      <span class="fxcl-confirm" onClick={e => e.stopPropagation()}>
                        {t('e.fxc.deleteQ')}
                        <button class="smallbtn danger" ref={el => queueMicrotask(() => el.focus())} onClick={() => void remove(p.name)}
                          onKeyDown={e => { e.stopPropagation(); if (e.key === 'Escape') { setDeleting(null); search?.focus(); } }}>{t('e.fxc.delete')}</button>
                        <button class="smallbtn" onClick={() => { setDeleting(null); search?.focus(); }}>{t('e.fxc.keep')}</button>
                      </span>
                    </Show>
                  </div>
                )}</For>
              </Show>
            </Show>
          </Show>
        </div>
        <Show when={err()}><div class="note err fxcl-err">{err()}</div></Show>
        <div class="fxcl-ft">
          <Show when={hasFx()} fallback={
            <>
              <span class="note">{t('e.fxc.emptyIns')}</span>
              <button class="smallbtn primary" disabled={!cur() || !!cur()!.error || busy()} onClick={() => void load(cur(), 'replace')}>{t('e.fxc.load')}</button>
            </>
          }>
            <span class="note">{t('e.fxc.hasFx', { n: ins()!.fx.length })}</span>
            <button class="smallbtn" disabled={!cur() || !!cur()!.error || busy()} title={t('e.fxc.appendTip') + ' (Shift+Enter)'} onClick={() => void load(cur(), 'append')}>{t('e.fxc.append')}</button>
            <button class="smallbtn primary" disabled={!cur() || !!cur()!.error || busy()} title={t('e.fxc.replaceTip') + ' (Enter)'} onClick={() => void load(cur(), 'replace')}>{t('e.fxc.replace')}</button>
          </Show>
        </div>
      </div>
    </div>
  );
}
