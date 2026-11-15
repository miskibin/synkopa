// "Missing audio": the sources of the open project whose files were moved, renamed or deleted
// (they play silence). Search a folder (relinks every unambiguous match at once), locate one
// file, or pick among several found. Every relink is an undo step of the engine.
import { createEffect, createMemo, createSignal, For, on, Show } from 'solid-js';
import { be } from '../api/backend';
import type { Id, RelinkCandidate, RelinkSearch, Source } from '../api/types';
import { t } from '../lib/i18n';
import { fileName, folderOf } from '../lib/relink';
import { enqueue } from '../state/edit';
import { S } from '../state/store';
import { say, setU, U } from '../state/ui';
import { LIB_SCHEME } from '../lib/factory';

type Status = { kind: 'busy' | 'ok' | 'warn' | 'err'; text: string } | null;

export function MissingAudio() {
  const [status, setStatus] = createSignal<Status>(null);
  const [busy, setBusy] = createSignal(false);
  const [cands, setCands] = createSignal<Record<string, RelinkCandidate[]>>({});
  const [notFound, setNotFound] = createSignal<Set<Id>>(new Set());
  let searchBtn: HTMLButtonElement | undefined;

  const rows = createMemo(() => S.project.sources.filter(s => s.id in S.missing));
  const relinkable = () => rows().filter(s => !s.path.startsWith(LIB_SCHEME));
  const close = () => setU('missingOpen', false);
  // a fresh round each time the dialog opens (or another project is loaded under it)
  createEffect(on(() => [U.missingOpen, S.path], () => { setStatus(null); setCands({}); setNotFound(new Set<Id>()); if (U.missingOpen) queueMicrotask(() => searchBtn?.focus()); }));

  const usage = (s: Source) => {
    const clips = S.project.clips.filter(c => c.kind === 'audio' && c.ref === s.id).length;
    const chans = (S.project.channels ?? []).filter(c => c.kind === 'sample' && c.source === s.id).length;
    const parts = [];
    if (clips) parts.push(t('d.miss.clips', { n: clips }));
    if (chans) parts.push(t('d.miss.chans', { n: chans }));
    return parts.join(' · ') || t('d.miss.unused');
  };

  /** Runs one relink request after the queued edits, reporting failures in the status line. */
  const run = async (work: () => Promise<void>) => {
    if (busy()) return;
    setBusy(true);
    try { await enqueue(() => Promise.resolve()); await work(); }
    catch (e) { setStatus({ kind: 'err', text: String(e) }); }
    finally { setBusy(false); }
  };

  const summarize = (r: RelinkSearch) => {
    const ok = r.relinked.filter(x => !x.error).length, choose = Object.keys(r.candidates).length, none = r.notFound.length;
    const text = [
      t('d.miss.sum.relinked', { n: ok }),
      choose ? t('d.miss.sum.choose', { n: choose }) : '',
      none ? t('d.miss.sum.none', { n: none }) : '',
    ].filter(Boolean).join(' · ') + (r.truncated ? ' ' + t('d.miss.sum.truncated', { n: r.scanned.toLocaleString() }) : '');
    setStatus({ kind: r.truncated || (!ok && (choose || none)) ? 'warn' : 'ok', text });
  };

  const search = () => run(async () => {
    const start = S.path ? folderOf(S.path) : folderOf(relinkable()[0]?.path ?? '');
    const folder = await be().pickFolder(start || undefined);
    if (!folder) return;
    setStatus({ kind: 'busy', text: t('d.miss.searching', { folder }) });
    const r = await be().relinkSearch(folder);
    setCands(r.candidates);
    setNotFound(new Set(r.notFound));
    summarize(r);
    if (r.relinked.some(x => !x.error)) say(t('d.miss.say', { n: r.relinked.filter(x => !x.error).length }));
  });

  const doRelink = async (s: Source, path: string) => {
    setStatus({ kind: 'busy', text: t('d.miss.loading', { name: fileName(path) }) });
    const [r] = await be().relinkSources([{ sourceId: s.id, path }]);
    if (r.error) { setStatus({ kind: 'err', text: r.error }); return; }
    setCands(c => { const n = { ...c }; delete n[s.id]; return n; });
    setStatus(r.lengthDiffers ? { kind: 'warn', text: t('d.miss.lenDiffers', { name: s.name }) } : { kind: 'ok', text: t('d.miss.one', { name: s.name }) });
    say(t('d.miss.say', { n: 1 }));
  };
  const relinkOne = (s: Source, path: string) => run(() => doRelink(s, path));
  const locate = (s: Source) => run(async () => {
    const path = await be().pickAudioFile(folderOf(s.path) || undefined);
    if (path) await doRelink(s, path);
  });

  const candTags = (c: RelinkCandidate) => [
    c.exactName ? '' : t('d.miss.tag.format'),
    c.sharedFolders ? t('d.miss.tag.folders', { n: c.sharedFolders }) : '',
  ].filter(Boolean).join(' · ');

  return (
    <Show when={U.missingOpen}>
      <div class="overlay" onPointerDown={close}>
        <div class="dialog missdlg" role="dialog" aria-modal="true" aria-label={t('d.miss.title', { n: rows().length })} onPointerDown={e => e.stopPropagation()} data-missing-dialog>
          <div class="hd">
            <span>{rows().length ? t('d.miss.title', { n: rows().length }) : t('d.miss.titleDone')}</span>
            <button onClick={close} aria-label={t('d.miss.close')}>×</button>
          </div>
          <Show when={rows().length} fallback={<p class="note">{t('d.miss.allFound')}</p>}>
            <p class="note">{t('d.miss.intro')}</p>
            <div class="row missbar">
              <button ref={searchBtn} class="smallbtn primary" disabled={busy() || !relinkable().length} onClick={() => void search()} title={t('d.miss.searchTip')}>{t('d.miss.search')}</button>
              <Show when={status()}>
                {st => (
                  <span class="note missstatus" classList={{ warn: st().kind === 'warn', err: st().kind === 'err' }} role="status" title={st().text}>
                    <Show when={st().kind === 'busy'}><span class="spin" /></Show>{st().text}
                  </span>
                )}
              </Show>
            </div>
            <ul class="misslist">
              <For each={rows()}>
                {s => {
                  const lib = () => s.path.startsWith(LIB_SCHEME);
                  const found = () => cands()[s.id] ?? [];
                  return (
                    <li>
                      <div class="row misshd">
                        <span class="missname" title={s.name}>{s.name}</span>
                        <span class="note">{usage(s)}</span>
                        <div style={{ flex: 1 }} />
                        <Show when={!lib()} fallback={<span class="note">{t('d.miss.lib')}</span>}>
                          <button class="smallbtn" disabled={busy()} onClick={() => void locate(s)} title={t('d.miss.locateTip')}>{t('d.miss.locate')}</button>
                        </Show>
                      </div>
                      <div class="misspath mono" title={S.missing[s.id]}><bdi>{s.path}</bdi></div>
                      <Show when={notFound().has(s.id) && !found().length}><div class="note warn">{t('d.miss.notFound')}</div></Show>
                      <Show when={found().length}>
                        <div class="note">{t('d.miss.pick', { n: found().length })}</div>
                        <ul class="misscands">
                          <For each={found()}>
                            {c => (
                              <li class="row">
                                <span class="misspath mono" title={c.path}><bdi>{c.path}</bdi></span>
                                <Show when={candTags(c)}><span class="tag9">{candTags(c)}</span></Show>
                                <button class="smallbtn" disabled={busy()} onClick={() => void relinkOne(s, c.path)}>{t('d.miss.use')}</button>
                              </li>
                            )}
                          </For>
                        </ul>
                      </Show>
                    </li>
                  );
                }}
              </For>
            </ul>
          </Show>
          <div class="row" style={{ 'justify-content': 'flex-end', 'margin-top': '12px' }}>
            <button class="smallbtn" classList={{ primary: !rows().length }} onClick={close}>{rows().length ? t('d.miss.later') : t('d.miss.done')}</button>
          </div>
        </div>
      </div>
    </Show>
  );
}

/** The toolbar chip: shown while sources are missing, opens the dialog. */
export function MissingChip() {
  const n = () => Object.keys(S.missing).length;
  return (
    <Show when={n()}>
      <button class="updchip misschip" title={t('d.miss.chipTip')} onClick={() => setU('missingOpen', true)}>{t('d.miss.chip', { n: n() })}</button>
    </Show>
  );
}
