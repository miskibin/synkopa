// Synkopa Instruments: the sampled-instrument player in the channel window. Two views share the panel,
// as in the library players musicians know (Kontakt's library, Arturia's and Spitfire's tile
// browsers, FL's preset arrows):
// - the instrument's face: its artwork, name and character, where its recordings stand (loading,
//   not downloaded, failed), its articulations, its knobs in sections, and the credit its licence
//   asks for; arrows step to the previous / next instrument while you play;
// - the library: every instrument of the catalog as a card with its artwork, category filters,
//   what is on this computer and what a download costs. A click makes the channel play it (and
//   the keyboard below keeps playing it), a double click or Enter goes back to its face.
import { For, Show } from 'solid-js';
import type { JSX } from '@solidjs/web';
import { createSignal, onCleanup, onMount, watch } from '../lib/reactive';
import { be } from '../api/backend';
import { KEYS_DEFAULT_INSTRUMENT, type Channel, type KeysCategory, type KeysInstrument, type KeysParam, type KeysStatus } from '../api/types';
import { fmtBytes } from '../lib/format';
import { t, type TranslationKey } from '../lib/i18n';
import { keysInstrument } from '../lib/keysCatalog';
import { keysDefault, keysDenorm, keysNorm, keysText, keysValue } from '../lib/keysKnobs';
import { noteName } from '../lib/pitch';
import { autoDisplay, autoTweak, autoWheel, isAutomated, isOverridden, paramMenu, type Tweak } from '../state/automation';
import { commit } from '../state/edit';
import { chanById, chanOp, setKeysArticulation, setKeysInstrument } from '../state/pattern';
import { say } from '../state/ui';
import { Knob } from '../ui/Knob';
import { ChannelLevelKnobs } from './ChannelWindow';

type KeysChannel = Extract<Channel, { kind: 'keys' }>;

const CATEGORIES: KeysCategory[] = ['piano', 'electric', 'guitar', 'strings', 'mallets'];
/** Each instrument's artwork (`ui/src/art/keys/<id>.webp`), by catalog id. */
const ART: Record<string, string> = Object.fromEntries(
  Object.entries(import.meta.glob<string>('../art/keys/*.webp', { eager: true, query: '?url', import: 'default' }))
    .map(([path, url]) => [path.slice(path.lastIndexOf('/') + 1, -'.webp'.length), url]),
);
const catName = (c: KeysCategory) => t(('b.kp.cat.' + c) as TranslationKey);

/** The instrument's one-line character in the UI's language (the engine's English as a fallback). */
export function instDesc(i: Pick<KeysInstrument, 'id' | 'description'>) {
  const k = ('b.kp.d.' + i.id) as TranslationKey;
  const s = t(k);
  return s === k ? i.description : s;
}

/** The library filter: a category, or every instrument. */
type Filter = KeysCategory | 'all';
/** The library's filter, kept while the window closes and opens again. */
const [filter, setFilter] = createSignal<Filter>('all');

export function KeysPanel(props: { c: KeysChannel }) {
  const live = () => (chanById(props.c.id) as KeysChannel | undefined) ?? props.c;
  const [st, setSt] = createSignal<KeysStatus | null>(null);
  /** Kits being downloaded and their progress 0..1. */
  const [dl, setDl] = createSignal<Record<string, number>>({});
  const [err, setErr] = createSignal<string | null>(null);
  /** The instrument's face, or the library (it stays open while you click through instruments). */
  const [view, setView] = createSignal<'face' | 'lib'>('face');
  // a successful status fetch clears a stale error (a Retry that lands must dismiss the banner)
  const refresh = () => void be().keysStatus().then(s => { setErr(null); setSt(s); }).catch(e => setErr(String(e)));
  const setKit = (kit: string, p: number | null) => setDl(d => {
    const n = { ...d };
    if (p == null) delete n[kit]; else n[kit] = p;
    return n;
  });

  onMount(() => {
    refresh();
    const offs = [
      be().on('keys-status', setSt),
      be().on('library-progress', p => {
        if (dl()[p.id] == null) return;
        if (p.error) { setKit(p.id, null); setErr(p.error); return; }
        if (p.total) setKit(p.id, p.done / p.total);
        if (p.total && p.done >= p.total) { setKit(p.id, null); refresh(); }
      }),
    ];
    onCleanup(() => offs.forEach(o => void o.then(un => un())));
  });

  const instId = () => live().keys.instrument ?? KEYS_DEFAULT_INSTRUMENT;
  const all = () => st()?.instruments ?? [];
  const cur = () => all().find(i => i.id === instId()) ?? null;
  // another instrument: where its recordings stand (the app also pushes `keys-status` as it loads)
  watch(instId, () => refresh(), { defer: true });

  const download = async (i: KeysInstrument) => {
    if (dl()[i.kit] != null) return;
    setErr(null); setKit(i.kit, 0);
    try { await be().libraryInstall(i.kit); say(t('b.kp.downloaded', { name: i.name })); }
    catch (e) { setErr(String(e)); }
    finally { setKit(i.kit, null); refresh(); }
  };
  // a failed load is tried again by the app; a failed download is downloaded again; with no
  // instrument to retry (the status itself failed) the catalog is fetched again
  const retry = () => {
    const i = cur();
    if (!i) return refresh();
    if (!i.installed) return download(i);
    setErr(null);
    void be().keysRetry(instId()).then(setSt).catch(e => setErr(String(e)));
  };
  const pick = (i: KeysInstrument) => { setErr(null); setKeysInstrument(props.c.id, i.id); };
  /** The instrument `d` places from the current one in the catalog's order (wrapping). */
  const step = (d: number) => {
    const list = all(); if (!list.length) return;
    const i = list.findIndex(x => x.id === instId());
    pick(list[(i + d + list.length) % list.length]);
  };

  const keys = () => live().keys;
  const target = (param: KeysParam) => ({ kind: 'keys' as const, chanId: props.c.id, param });
  /** The value a knob shows: the automated one while it plays. */
  const shown = (param: KeysParam) => autoDisplay(target(param)) ?? keysValue(keys(), param);
  const knob = (param: KeysParam, opts: { zero?: number; text?: () => string; reset?: () => KeysChannel['keys']; color?: string } = {}) => {
    const lab = ('b.kp.' + param) as TranslationKey;
    let g: Tweak | null = null;
    const op = (v: number) => [chanOp(live(), { keys: { ...keys(), [param]: Math.round(keysDenorm(param, v)) } } as Partial<Channel>)];
    const reset = () => opts.reset?.() ?? { ...keys(), [param]: keysDefault(param) };
    const doReset = () => commit([chanOp(live(), { keys: reset() } as Partial<Channel>)], t('b.kp.gest', { name: t(lab) }));
    const text = opts.text ?? (() => keysText(param, shown(param)));
    return (
      <Knob key={'keys' + param + props.c.id} color={opts.color ?? props.c.color} label={t(lab)} title={t('b.kp.knobTitle', { title: t((lab + 'Tip') as TranslationKey) })}
        t={() => keysNorm(param, shown(param))} text={text} zero={opts.zero != null ? () => opts.zero! : undefined}
        automated={() => isAutomated(target(param))}
        overridden={() => isOverridden(target(param))}
        onMenu={(x, y) => paramMenu(x, y, target(param), doReset, props.c.color)}
        onStart={() => { g = autoTweak(target(param), t('b.kp.gest', { name: t(lab) }), op); }}
        onChange={v => { g?.move(v); say(live().name + ' · ' + t(lab).toLowerCase() + ' ' + text()); }}
        onEnd={() => { g?.end(); g = null; }}
        onReset={doReset}
        onWheel={v => autoWheel(target(param), 'keys' + param + props.c.id, t('b.kp.gest', { name: t(lab) }), v, op)} />
    );
  };
  /** A knob the instrument has nothing for: dimmed, and its tooltip says why. */
  const unavailable = (why: () => string, k: JSX.Element) => <div class="kna" title={why()} aria-disabled="true">{k}</div>;

  const state = () => cur()?.state ?? 'idle';
  const busyOf = (i: KeysInstrument | null) => (i ? dl()[i.kit] ?? null : null);
  const busy = () => busyOf(cur());
  const missing = () => !!cur() && !cur()!.installed;
  const failed = () => err() ?? (state() === 'error' ? cur()?.error ?? t('b.kp.loadFail') : null);
  const badge = () => {
    if (busy() != null) return { cls: 'busy', text: Math.round(busy()! * 100) + ' %' };
    switch (state()) {
      case 'ready': return { cls: 'ok', text: t('b.kp.stLoaded') };
      case 'loading': return { cls: 'busy', text: t('b.kp.stLoading') };
      case 'error': return { cls: 'bad', text: t('b.kp.stError') };
      default: return missing() ? { cls: 'off', text: t('b.kp.stMissing') } : null;
    }
  };
  const art = () => keys().articulation ?? null;
  const arts = () => cur()?.articulations ?? [];
  const auto = () => cur()?.auto ?? null;
  // no choice = Auto where the instrument has it, else its first articulation
  const activeArt = () => art() ?? (auto() ? null : arts()[0]?.key ?? null);
  const nameOf = (key: number) => arts().find(a => a.key === key)?.name ?? noteName(key);
  const release = () => autoDisplay(target('release')) ?? keys().release;
  const withoutRelease = () => { const k = { ...keys() }; delete k.release; return k; };

  // ---------------------------------------------------------------- library
  const shownList = () => (filter() === 'all' ? all() : all().filter(i => i.category === filter()));
  const count = (f: Filter) => (f === 'all' ? all().length : all().filter(i => i.category === f).length);
  let grid: HTMLDivElement | undefined;
  /** Esc in the library goes back to the face; it must not reach the app's Esc (which closes the
   *  window). A native listener on the panel: it runs before the app's window listener. */
  const libEsc = (e: KeyboardEvent) => {
    if (e.key !== 'Escape' || view() !== 'lib') return;
    e.preventDefault(); e.stopPropagation();
    closeLib();
  };
  onMount(() => {
    root?.addEventListener('keydown', libEsc);
    onCleanup(() => root?.removeEventListener('keydown', libEsc));
  });
  const cards = () => [...(grid?.querySelectorAll<HTMLElement>('.kcard') ?? [])];
  const openLib = () => {
    setView('lib');
    // the current instrument's card takes the focus (the arrows move from there)
    queueMicrotask(() => (grid?.querySelector<HTMLElement>('.kcard.on') ?? cards()[0])?.focus());
  };
  const closeLib = () => {
    setView('face');
    // the keyboard stays in the window (on the name that opens the library again)
    queueMicrotask(() => root?.querySelector<HTMLElement>('.knm')?.focus());
  };
  let root: HTMLDivElement | undefined;
  const onGridKey = (e: KeyboardEvent) => {
    const list = cards(), i = list.indexOf(document.activeElement as HTMLElement);
    if (i < 0) return;
    const cols = Math.max(1, getComputedStyle(grid!).gridTemplateColumns.split(' ').length);
    const go = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -cols, ArrowDown: cols, Home: -i, End: list.length - 1 - i }[e.key];
    if (go != null) { e.preventDefault(); list[Math.max(0, Math.min(list.length - 1, i + go))]?.focus(); return; }
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      const inst = shownList()[i]; if (!inst) return;
      // a second Enter on the instrument the channel already plays goes to its face
      if (inst.id === instId() && e.key === 'Enter') closeLib(); else pick(inst);
    }
  };
  const cardStatus = (i: KeysInstrument) => {
    const b = busyOf(i);
    if (b != null) return <div class="kcard-bar" title={Math.round(b * 100) + ' %'}><div style={{ width: Math.round(b * 100) + '%' }} /></div>;
    if (!i.installed) {
      return <button class="smallbtn kcard-dl" title={t('b.kp.cardDlTip', { name: i.name, size: fmtBytes(i.bytes) })}
        onClick={e => { e.stopPropagation(); void download(i); }} onDblClick={e => e.stopPropagation()}>{t('b.kp.cardDl', { size: fmtBytes(i.bytes) })}</button>;
    }
    return <span class="kcard-st">{i.bundled ? t('b.kp.inApp') : t('b.kp.ready')}</span>;
  };

  return (
    <div class={['keyspanel', view() === 'lib' ? 'is-lib' : 'is-face']} ref={root}>
      <Show when={view() === 'lib'} fallback={
        <section class="kface">
          <div class="kplate" style={{ '--kc': props.c.color }}>
            <Show when={ART[instId()]}>{src => <img class="kart-img" src={src()} alt="" draggable={false} />}</Show>
            <div class="ktop">
              <div class="kstep">
                <button class="kstepbtn" title={t('b.kp.prev')} aria-label={t('b.kp.prev')} onClick={() => step(-1)}>‹</button>
                <button class="kstepbtn" title={t('b.kp.next')} aria-label={t('b.kp.next')} onClick={() => step(1)}>›</button>
              </div>
              <span class="kcatlbl">{cur() ? catName(cur()!.category) : ''}</span>
              <div style={{ flex: 1 }} />
              <Show when={badge()}>{b => <span class={'kbadge ' + b().cls}>{b().text}</span>}</Show>
              <button class="kbrowsebtn" title={t('b.kp.browseTip')} onClick={openLib}>
                <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M1 1h4v4H1zM7 1h4v4H7zM1 7h4v4H1zM7 7h4v4H7z" fill="currentColor" /></svg>
                {t('b.kp.browse')}
              </button>
            </div>
            <div class="ktitle">
              <button class="knm" title={t('b.kp.browseTip')} onClick={openLib}>{cur()?.name ?? instId()}<span class="kchev" aria-hidden="true">▾</span></button>
              <div class="kdesc">{cur() ? instDesc(cur()!) : ''}</div>
            </div>
          </div>

          <Show when={missing() && busy() == null}>
            <div class="keysmsg">
              <span>{t('b.kp.missing', { name: cur()!.name, size: fmtBytes(cur()!.bytes) })}</span>
              <button class="smallbtn primary" onClick={() => void download(cur()!)}>{t('b.kp.download', { size: fmtBytes(cur()!.bytes) })}</button>
            </div>
          </Show>
          <Show when={busy() != null}>
            <div class="keysmsg">
              <span>{t('b.kp.downloading', { name: cur()!.name, n: Math.round(busy()! * 100) })}</span>
              <div class="keysbar"><div style={{ width: Math.round(busy()! * 100) + '%' }} /></div>
            </div>
          </Show>
          <Show when={state() === 'loading' && busy() == null}><div class="keysmsg"><span>{t('b.kp.loading')}</span></div></Show>
          <Show when={failed()}>
            {m => <div class="keysmsg bad"><span>{m()}</span><button class="smallbtn" onClick={retry}>{t('b.kp.retry')}</button></div>}
          </Show>
          <Show when={!err() && state() === 'idle' && all().length === 0}>
            <div class="keysmsg"><span>{t('b.kp.libEmpty')}</span></div>
          </Show>

          <Show when={arts().length > 0}>
            <div class="kart">
              <span class="lab">{t('b.kp.art')}</span>
              <div class="seg" role="radiogroup" aria-label={t('b.kp.art')}>
                <Show when={auto()}>
                  {au => (
                    <button class={{ on: activeArt() == null }} role="radio" aria-checked={activeArt() == null ? 'true' : 'false'}
                      title={t('b.kp.autoTip', { short: nameOf(au().short), long: nameOf(au().long), ms: Math.round(au().seconds * 1000) })}
                      onClick={() => setKeysArticulation(props.c.id, undefined)}>
                      {t('b.kp.auto')}
                    </button>
                  )}
                </Show>
                <For each={arts()}>{a => (
                  <button class={{ on: a.key === activeArt() }} role="radio" aria-checked={a.key === activeArt() ? 'true' : 'false'}
                    title={t('b.kp.artTip', { name: a.name, note: noteName(a.key) })}
                    onClick={() => setKeysArticulation(props.c.id, !auto() && a.key === arts()[0].key ? undefined : a.key)}>
                    {a.name}
                  </button>
                )}</For>
              </div>
            </div>
          </Show>

          <div class="keysknobs">
            <div class="ksec">
              <span class="ksec-lab">{t('b.kp.secSound')}</span>
              <div class="ksec-row">
                {knob('tone', { zero: 0.5 })}
                <Show when={cur()?.stereo !== false} fallback={unavailable(() => t('b.kp.widthMono', { name: cur()?.name ?? '' }), knob('width'))}>
                  {knob('width')}
                </Show>
                {knob('tune', { zero: 0.5 })}
              </div>
            </div>
            <div class="ksec">
              <span class="ksec-lab">{t('b.kp.secShape')}</span>
              <div class="ksec-row">
                {knob('attack')}
                {knob('release', { text: () => (release() == null ? t('b.kp.auto') : keysText('release', release()!)), reset: withoutRelease })}
              </div>
            </div>
            <div class="ksec">
              <span class="ksec-lab">{t('b.kp.secPlay')}</span>
              <div class="ksec-row">
                {knob('dynamics')}
                <Show when={cur()?.releaseNoise !== false} fallback={unavailable(() => t('b.kp.noiseNone', { name: cur()?.name ?? '' }), knob('noise'))}>
                  {knob('noise')}
                </Show>
              </div>
            </div>
            <div class="ksec">
              <span class="ksec-lab">{t('b.kp.secChan')}</span>
              <div class="ksec-row"><ChannelLevelKnobs channel={live()} /></div>
            </div>
          </div>
          <Show when={cur()}>
            {i => <div class="keyscredit">{t(i().license.startsWith('CC0') ? 'b.kp.creditFree' : 'b.kp.creditBy', { credit: i().credit, license: i().license })}</div>}
          </Show>
        </section>
      }>
        <section class="klib" aria-label={t('b.kp.lib')}>
          <div class="klibhd">
            <div class="kchips" role="radiogroup" aria-label={t('b.kp.lib')}>
              <For each={['all', ...CATEGORIES.filter(c => count(c) > 0)] as Filter[]}>{f => (
                <button class={['kchip', { on: filter() === f }]} role="radio" aria-checked={filter() === f ? 'true' : 'false'} onClick={() => setFilter(f)}>
                  {f === 'all' ? t('b.kp.all') : catName(f)}<span class="n">{count(f)}</span>
                </button>
              )}</For>
            </div>
            <div style={{ flex: 1 }} />
            <button class="smallbtn kback" title={t('b.kp.backTip')} onClick={closeLib}>‹ {t('b.kp.back', { name: cur()?.name ?? '' })}</button>
          </div>
          <div class="kgrid" ref={grid} role="listbox" aria-label={t('b.kp.lib')} onKeyDown={onGridKey}>
            {/* keyed by id: a status refresh brings new objects, and rebuilt cards would drop the keyboard focus */}
            <For each={shownList().map(i => i.id)}>{id => <Show when={all().find(x => x.id === id)}>{i => (
              <div class={['kcard', { on: i().id === instId(), off: !i().installed }]} role="option" tabindex={i().id === instId() ? 0 : -1}
                aria-selected={i().id === instId() ? 'true' : 'false'} title={t('b.kp.cardTip', { name: i().name, desc: instDesc(i()) })}
                onClick={() => pick(i())} onDblClick={() => { pick(i()); closeLib(); }}>
                <div class="kcard-art">
                  <Show when={ART[i().id]} fallback={<span class="kcard-ph">{i().name.slice(0, 1)}</span>}>{src => <img src={src()} alt="" draggable={false} loading="lazy" />}</Show>
                </div>
                <div class="kcard-txt">
                  <div class="kcard-hd"><span class="nm ell">{i().name}</span><Show when={i().id === instId()}><span class="kcard-on">{t('b.kp.playing')}</span></Show></div>
                  <div class="kcard-cat">{catName(i().category)}</div>
                  <div class="kcard-desc">{instDesc(i())}</div>
                  <div class="kcard-ft">{cardStatus(i())}</div>
                </div>
              </div>
            )}</Show>}</For>
          </div>
          <Show when={!err() && all().length === 0}>
            <div class="keysmsg">{t('b.kp.libEmpty')}</div>
          </Show>
          <Show when={err()}>{m => <div class="keysmsg bad klib-err"><span>{m()}</span></div>}</Show>
        </section>
      </Show>
    </div>
  );
}

/** What the channel window's keyboard shows for a Keys channel: the playable range and the keyswitches. */
export function keysKeyboard(c: KeysChannel) {
  const i = keysInstrument(c.keys.instrument ?? KEYS_DEFAULT_INSTRUMENT);
  const arts = i?.articulations ?? [];
  // Auto lights both the articulations it picks between
  const active = c.keys.articulation != null ? [c.keys.articulation] : i?.auto ? [i.auto.short, i.auto.long] : arts.slice(0, 1).map(a => a.key);
  return {
    range: i?.range,
    switches: arts.map(a => ({ key: a.key, name: a.name, on: active.includes(a.key) })),
  };
}
