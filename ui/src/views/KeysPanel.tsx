// Synkopa Keys: the sampled-instrument player in the channel window, laid out like a sampler
// plugin (Kontakt, FL's DirectWave browser): the instrument library on the left — every
// instrument of the catalog, what is on this computer and what a download costs — and the
// chosen instrument's face on the right: its name and character, where its recordings stand
// (loading, not downloaded, failed), its articulations, its knobs and the credit its licence asks for.
import { For, Show } from 'solid-js';
import { createSignal, onCleanup, onMount, watch } from '../lib/reactive';
import { be } from '../api/backend';
import { KEYS_DEFAULT_INSTRUMENT, KEYS_DYNAMICS, KEYS_RELEASE, type Channel, type KeysCategory, type KeysInstrument, type KeysStatus } from '../api/types';
import { clamp, fmtBytes } from '../lib/format';
import { t, type TranslationKey } from '../lib/i18n';
import { keysInstrument } from '../lib/keysCatalog';
import { noteName } from '../lib/pitch';
import { autoDisplay, autoTweak, autoWheel, isAutomated, isOverridden, paramMenu, type Tweak } from '../state/automation';
import { commit } from '../state/edit';
import { chanById, chanOp, setKeysArticulation, setKeysInstrument } from '../state/pattern';
import { say } from '../state/ui';
import { Knob } from '../ui/Knob';
import { ChannelLevelKnobs } from './ChannelWindow';

type KeysChannel = Extract<Channel, { kind: 'keys' }>;

/** Release knob travel is logarithmic: 40 ms .. 3 s. */
const relT = (ms: number) => Math.log(ms / KEYS_RELEASE.min) / Math.log(KEYS_RELEASE.max / KEYS_RELEASE.min);
const relMs = (t: number) => Math.round(KEYS_RELEASE.min * Math.pow(KEYS_RELEASE.max / KEYS_RELEASE.min, clamp(t, 0, 1)));
const relText = (ms: number) => (ms >= 1000 ? (ms / 1000).toFixed(2) + ' s' : Math.round(ms) + ' ms');

const CATEGORIES: KeysCategory[] = ['piano', 'electric', 'strings', 'mallets'];
/** Each instrument's artwork (`ui/src/art/keys/<id>.webp`), by catalog id. */
const ART: Record<string, string> = Object.fromEntries(
  Object.entries(import.meta.glob<string>('../art/keys/*.webp', { eager: true, query: '?url', import: 'default' }))
    .map(([path, url]) => [path.slice(path.lastIndexOf('/') + 1, -'.webp'.length), url]),
);

/** The instrument's one-line character in the UI's language (the engine's English as a fallback). */
export function instDesc(i: Pick<KeysInstrument, 'id' | 'description'>) {
  const k = ('b.kp.d.' + i.id) as TranslationKey;
  const s = t(k);
  return s === k ? i.description : s;
}

export function KeysPanel(props: { c: KeysChannel }) {
  const live = () => (chanById(props.c.id) as KeysChannel | undefined) ?? props.c;
  const [st, setSt] = createSignal<KeysStatus | null>(null);
  /** Kit being downloaded and its progress 0..1. */
  const [dl, setDl] = createSignal<{ kit: string; p: number } | null>(null);
  const [err, setErr] = createSignal<string | null>(null);
  const refresh = () => void be().keysStatus().then(setSt).catch(e => setErr(String(e)));

  onMount(() => {
    refresh();
    const offs = [
      be().on('keys-status', setSt),
      be().on('library-progress', p => {
        if (p.id !== dl()?.kit) return;
        if (p.error) { setDl(null); setErr(p.error); return; }
        if (p.total) setDl({ kit: p.id, p: p.done / p.total });
        if (p.total && p.done >= p.total) { setDl(null); refresh(); }
      }),
    ];
    onCleanup(() => offs.forEach(o => void o.then(un => un())));
  });

  const instId = () => live().keys.instrument ?? KEYS_DEFAULT_INSTRUMENT;
  const cur = () => st()?.instruments.find(i => i.id === instId()) ?? null;
  // another instrument: where its recordings stand (the app also pushes `keys-status` as it loads)
  watch(instId, () => refresh(), { defer: true });

  const download = async () => {
    const i = cur(); if (!i) return;
    setErr(null); setDl({ kit: i.kit, p: 0 });
    try { await be().libraryInstall(i.kit); say(t('b.kp.downloaded', { name: i.name })); }
    catch (e) { setErr(String(e)); setDl(null); }
    finally { refresh(); }
  };
  const pick = (i: KeysInstrument) => { setErr(null); setKeysInstrument(props.c.id, i.id); };

  const knob = (lab: TranslationKey, titleKey: TranslationKey, tv: () => number, text: () => string, patch: (v: number) => KeysChannel['keys'], reset: () => KeysChannel['keys'], param: 'release' | 'dynamics') => {
    let g: Tweak | null = null;
    const op = (v: number) => [chanOp(live(), { keys: patch(v) } as Partial<Channel>)];
    const target = () => ({ kind: 'keys' as const, chanId: props.c.id, param });
    const doReset = () => commit([chanOp(live(), { keys: reset() } as Partial<Channel>)], t('b.kp.gest', { name: t(lab) }));
    return (
      <Knob key={'keys' + lab + props.c.id} color={props.c.color} label={t(lab)} title={t('b.kp.knobTitle', { title: t(titleKey) })} t={tv} text={text}
        automated={() => isAutomated(target())}
        overridden={() => isOverridden(target())}
        onMenu={(x, y) => paramMenu(x, y, target(), doReset, props.c.color)}
        onStart={() => { g = autoTweak(target(), t('b.kp.gest', { name: t(lab) }), v => op(v)); }}
        onChange={v => { g?.move(v); say(props.c.name + ' · ' + t(lab).toLowerCase() + ' ' + text()); }}
        onEnd={() => { g?.end(); g = null; }}
        onReset={doReset}
        onWheel={v => autoWheel(target(), 'keys' + lab + props.c.id, t('b.kp.gest', { name: t(lab) }), v, vv => op(vv))} />
    );
  };

  // the knobs show the automated value while it plays
  const keys = () => live().keys;
  const release = () => autoDisplay({ kind: 'keys', chanId: props.c.id, param: 'release' }) ?? keys().release;
  const dynamics = () => autoDisplay({ kind: 'keys', chanId: props.c.id, param: 'dynamics' }) ?? keys().dynamics;
  const withoutRelease = () => { const k = { ...keys() }; delete k.release; return k; };

  const state = () => cur()?.state ?? 'idle';
  const missing = () => !!cur() && !cur()!.installed;
  const busy = () => (dl() && dl()!.kit === cur()?.kit ? dl()!.p : null);
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

  return (
    <div class="keyspanel">
      <aside class="kbrowse" aria-label={t('b.kp.lib')}>
        <div class="kbrand"><span>SYNKOPA</span> KEYS</div>
        <div class="klist">
          <For each={CATEGORIES}>{cat => (
            <Show when={st()?.instruments.some(i => i.category === cat)}>
              <div class="kcat">{t(('b.kp.cat.' + cat) as TranslationKey)}</div>
              <For each={st()!.instruments.filter(i => i.category === cat)}>{i => (
                <button class={['kitem', { on: i.id === instId(), off: !i.installed }]} title={t('b.kp.itemTip', { name: i.name, desc: instDesc(i) })}
                  aria-pressed={i.id === instId() ? "true" : "false"} onClick={() => pick(i)}>
                  <span class="nm ell">{i.name}</span>
                  <span class="meta">{i.installed ? (i.bundled ? t('b.kp.inApp') : '') : fmtBytes(i.bytes)}</span>
                </button>
              )}</For>
            </Show>
          )}</For>
        </div>
      </aside>

      <section class="kface">
        <div class="kplate" style={{ '--kc': props.c.color }}>
          <Show when={ART[instId()]}>{src => <img class="kart-img" src={src()} alt="" draggable={false} />}</Show>
          <div class="ktitle">
            <div class="kcatlbl">{cur() ? t(('b.kp.cat.' + cur()!.category) as TranslationKey) : ''}</div>
            <div class="knm">{cur()?.name ?? instId()}</div>
            <div class="kdesc">{cur() ? instDesc(cur()!) : ''}</div>
          </div>
          <Show when={badge()}>{b => <span class={'kbadge ' + b().cls}>{b().text}</span>}</Show>
        </div>

        <Show when={missing() && busy() == null}>
          <div class="keysmsg">
            <span>{t('b.kp.missing', { name: cur()!.name, size: fmtBytes(cur()!.bytes) })}</span>
            <button class="smallbtn primary" onClick={download}>{t('b.kp.download', { size: fmtBytes(cur()!.bytes) })}</button>
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
          {m => <div class="keysmsg bad"><span>{m()}</span><button class="smallbtn" onClick={() => (missing() ? download() : (setErr(null), refresh()))}>{t('b.kp.retry')}</button></div>}
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
                <button class={{ on: a.key === activeArt() }} role="radio" aria-checked={a.key === activeArt() ? "true" : "false"}
                  title={t('b.kp.artTip', { name: a.name, note: noteName(a.key) })}
                  onClick={() => setKeysArticulation(props.c.id, !auto() && a.key === arts()[0].key ? undefined : a.key)}>
                  {a.name}
                </button>
              )}</For>
            </div>
          </div>
        </Show>

        <div class="keysknobs">
          {knob('b.kp.release', 'b.kp.releaseTip', () => relT(release() ?? KEYS_RELEASE.def), () => (release() == null ? t('b.kp.auto') : relText(release()!)),
            v => ({ ...keys(), release: relMs(v) }), withoutRelease, 'release')}
          {knob('b.kp.dynamics', 'b.kp.dynamicsTip', () => dynamics() / 100, () => Math.round(dynamics()) + ' %',
            v => ({ ...keys(), dynamics: Math.round(clamp(v, 0, 1) * 100) }), () => ({ ...keys(), dynamics: KEYS_DYNAMICS.def }), 'dynamics')}
          <span class="ksep" />
          <ChannelLevelKnobs channel={live()} />
        </div>
        <Show when={cur()}>
          {i => <div class="keyscredit">{t(i().license.startsWith('CC0') ? 'b.kp.creditFree' : 'b.kp.creditBy', { credit: i().credit, license: i().license })}</div>}
        </Show>
      </section>
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
